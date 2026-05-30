"""Step 1: calibrate camera intrinsics from structured-light white dots vs the
nominal design wall (a known 3D target). Solves fx,fy,cx,cy,k1,k2 with
cv2.calibrateCameraExtended and REFUSES on degenerate observability instead of
emitting a confidently-wrong K (spec 2026-05-30-sl-camera-calibration-design)."""
from __future__ import annotations

import hashlib
import json
import pathlib

import cv2
import numpy as np

from lmt_vba_sidecar.io_utils import write_event
from lmt_vba_sidecar.ipc import (
    BaStats,
    CalibrateStructuredLightInput,
    CorrespondenceFile,
    ErrorEvent,
    ProgressEvent,
    ResultData,
    ResultEvent,
    StructuredLightMeta,
)
from lmt_vba_sidecar.nominal import nominal_dot_positions_world
from lmt_vba_sidecar.sl_reconstruct import validate_sl_provenance
from lmt_vba_sidecar.calibrate import _atomic_write, FOCAL_BOUNDS_FRACTION

# Observability gate constants (spec §8 starting values).
# Coverage is the LARGER per-axis union-bbox span fraction, not a bbox-AREA
# product: a real LED wall (wide, short) projects to a thin band whose area
# fraction can never reach 0.40 even when perfectly observed, but whose dominant
# axis still spans a healthy fraction of the frame. The gate's intent — reject
# dots crammed into one small image region — is preserved by the per-axis span.
COVERAGE_MIN_FRAC = 0.20
COPLANAR_RATIO_MIN = 1e-3
POSE_ROT_DIVERSITY_DEG = 5.0
# Covariance gates retuned against the synthetic substrate (spec §8 says the plan
# pins final numbers there). A wide/short LED wall viewed from a shallow front arc
# constrains the principal point only weakly: at 0.3 px centroid noise this rig
# yields pp_std ~= 7-10 px and focal_std ~= 1.0%, while still recovering fx to
# < 2%. The §8 starting values (pp <= 3 px, focal <= 1%) were tighter than the
# geometry can deliver; these keep real margin (a genuinely degenerate solve
# produces tens-to-hundreds of px pp_std) and are caught earlier by the
# coplanarity / rotation-diversity gates regardless.
PP_STDDEV_MAX_PX = 12.0
FOCAL_STDDEV_MAX_FRAC = 0.015
MIN_DOTS_PER_POSE = 4


def _err(code: str, msg: str) -> int:
    write_event(ErrorEvent(event="error", code=code, message=msg, fatal=True))
    return 1


def _coplanarity_ratio(pts: np.ndarray) -> float:
    if len(pts) < 3:
        return 0.0
    s = np.linalg.svd(pts - pts.mean(axis=0), compute_uv=False)
    return float(s[-1] / s[0]) if s[0] > 0 else 0.0


def _max_pairwise_rot_deg(rvecs) -> float:
    Rs = [cv2.Rodrigues(np.asarray(r))[0] for r in rvecs]
    best = 0.0
    for a in range(len(Rs)):
        for b in range(a + 1, len(Rs)):
            Rrel = Rs[a].T @ Rs[b]
            cos = (np.trace(Rrel) - 1.0) / 2.0
            best = max(best, float(np.degrees(np.arccos(np.clip(cos, -1.0, 1.0)))))
    return best


def _coverage_frac(image_points, image_size) -> float:
    """Larger per-axis union-bbox span fraction (not a bbox-area product)."""
    allpts = np.concatenate([np.asarray(p).reshape(-1, 2) for p in image_points], axis=0)
    w = (allpts[:, 0].max() - allpts[:, 0].min()) / image_size[0]
    h = (allpts[:, 1].max() - allpts[:, 1].min()) / image_size[1]
    return float(max(w, h))


def run_calibrate_structured_light(cmd: CalibrateStructuredLightInput) -> int:
    # 1. sl_meta + provenance sha
    meta_path = pathlib.Path(cmd.sl_meta_path)
    try:
        meta = StructuredLightMeta.model_validate_json(meta_path.read_text())
    except (OSError, ValueError) as e:
        return _err("invalid_input", f"sl_meta unreadable: {e}")
    expected_sha = hashlib.sha256(meta_path.read_bytes()).hexdigest()

    # 2. correspondence files + provenance gate (reused from sl_reconstruct)
    corr_files: list[CorrespondenceFile] = []
    for p in cmd.correspondence_paths:
        try:
            corr_files.append(CorrespondenceFile.model_validate_json(pathlib.Path(p).read_text()))
        except (OSError, ValueError) as e:
            return _err("invalid_input", f"correspondence '{p}' unreadable: {e}")
    try:
        validate_sl_provenance(corr_files, expected_sha=expected_sha, expected_screen_id=cmd.project.screen_id)
    except ValueError as e:
        return _err("invalid_input", str(e))

    # 3. same-camera precondition: one camera_image_size across all poses
    sizes = {tuple(int(v) for v in c.camera_image_size) for c in corr_files}
    if len(sizes) != 1:
        return _err("invalid_input", f"correspondences disagree on camera_image_size: {sorted(sizes)}")
    (image_size,) = sizes

    # 4. per-dot nominal 3D world (known target). keys() == project present cells.
    try:
        dot_world = nominal_dot_positions_world(meta, cmd.project.cabinet_array, cmd.project.shape_prior)
    except ValueError as e:
        return _err("invalid_input", str(e))

    # 5. assemble per-pose object/image points (canonical (u,v) implicit via dot id)
    write_event(ProgressEvent(event="progress", stage="subpixel_refine", percent=0.3, message="assembling observations"))
    object_points, image_points = [], []
    for cf in corr_files:
        objp, imgp = [], []
        for pt in cf.points:
            X = dot_world.get(int(pt.id))
            if X is None:
                continue
            objp.append(X)
            imgp.append([pt.x, pt.y])
        if len(objp) >= MIN_DOTS_PER_POSE:
            object_points.append(np.asarray(objp, dtype=np.float32))
            image_points.append(np.asarray(imgp, dtype=np.float32))
    if len(object_points) < 1:
        return _err("observability_failed", f"no pose has >= {MIN_DOTS_PER_POSE} dots mapping to nominal")

    # 6. coplanarity OR >=3 poses gate (planar-PoC degeneracy)
    all_obj = np.concatenate(object_points, axis=0)
    ratio = _coplanarity_ratio(all_obj)
    if ratio < COPLANAR_RATIO_MIN and len(object_points) < 3:
        return _err("observability_failed",
                    f"near-coplanar target (ratio={ratio:.2e}) with only {len(object_points)} pose(s)")

    # 7. coverage gate
    cover = _coverage_frac(image_points, image_size)
    if cover < COVERAGE_MIN_FRAC:
        return _err("observability_failed", f"image coverage {cover:.2f} < {COVERAGE_MIN_FRAC}")

    # 8. solve (intrinsic guess; radial k1,k2 only)
    write_event(ProgressEvent(event="progress", stage="bundle_adjustment", percent=0.7, message="solving intrinsics"))
    long_dim = max(image_size)
    K0 = np.array([[1.2 * long_dim, 0.0, image_size[0] / 2.0],
                   [0.0, 1.2 * long_dim, image_size[1] / 2.0],
                   [0.0, 0.0, 1.0]])
    dist0 = np.zeros(5)
    flags = cv2.CALIB_USE_INTRINSIC_GUESS | cv2.CALIB_ZERO_TANGENT_DIST | cv2.CALIB_FIX_K3
    try:
        rms, K, dist, rvecs, _tvecs, std_int, _std_ext, _pv = cv2.calibrateCameraExtended(
            object_points, image_points, image_size, K0, dist0, flags=flags)
    except cv2.error as e:
        return _err("intrinsics_invalid", f"calibrateCamera failed: {e}")

    # 9. pose/baseline diversity gate (count != observability)
    if len(rvecs) >= 2 and _max_pairwise_rot_deg(rvecs) < POSE_ROT_DIVERSITY_DEG:
        return _err("observability_failed",
                    f"pose rotation diversity < {POSE_ROT_DIVERSITY_DEG} deg (near-duplicate captures)")

    # 10. quality + parameter-observability gates
    if not (np.isfinite(K).all() and np.isfinite(dist).all() and np.isfinite(rms)):
        return _err("intrinsics_invalid", f"calibration produced non-finite values (rms={rms})")
    fx, fy, cx, cy = float(K[0, 0]), float(K[1, 1]), float(K[0, 2]), float(K[1, 2])
    f_lo, f_hi = FOCAL_BOUNDS_FRACTION
    if not (f_lo * long_dim < fx < f_hi * long_dim) or not (f_lo * long_dim < fy < f_hi * long_dim):
        return _err("intrinsics_invalid", f"focal ({fx:.1f},{fy:.1f}) outside plausible range for {image_size}")
    if not (0 < cx < image_size[0]) or not (0 < cy < image_size[1]):
        return _err("intrinsics_invalid", f"principal point ({cx:.1f},{cy:.1f}) outside image {image_size}")
    if rms > cmd.max_rms_px:
        return _err("intrinsics_invalid", f"reproj RMS {rms:.2f}px exceeds gate {cmd.max_rms_px}px")
    std = np.asarray(std_int).flatten()
    pp_std = (float(std[2]), float(std[3]))
    foc_std = (float(std[0]), float(std[1]))
    if max(pp_std) > PP_STDDEV_MAX_PX:
        return _err("observability_failed", f"principal-point std {pp_std} px > {PP_STDDEV_MAX_PX} (under-constrained)")
    if max(foc_std) > FOCAL_STDDEV_MAX_FRAC * fx:
        return _err("observability_failed", f"focal std {foc_std} px > {FOCAL_STDDEV_MAX_FRAC*100:.0f}% of focal")

    # 11. write intrinsics (5-key contract + provenance)
    payload = json.dumps({
        "K": K.tolist(),
        "dist_coeffs": dist.flatten().tolist(),
        "image_size": list(image_size),
        "reproj_error_px": float(rms),
        "frames_used": len(object_points),
        "calibration_method": "structured_light_nominal",
        "pp_stddev_px": list(pp_std),
        "focal_stddev_px": list(foc_std),
        "n_poses": len(object_points),
    }, indent=2)
    _atomic_write(pathlib.Path(cmd.output_path), payload)

    # --- emit result + return 0 --- (mirrors calibrate.py success tail)
    write_event(ResultEvent(
        event="result",
        data=ResultData(
            measured_points=[],
            ba_stats=BaStats(rms_reprojection_px=float(rms), iterations=0, converged=True),
            frame_strategy_used="nominal_anchoring",
            procrustes_align_rms_m=0.0,  # calibrate does no Procrustes
        ),
    ))
    return 0
