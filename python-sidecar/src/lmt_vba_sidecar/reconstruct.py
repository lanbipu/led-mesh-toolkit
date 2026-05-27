"""Reconstruct top-level: capture-manifest → detection → model-constrained BA
→ cabinet pose report + IR MeasuredPoints.

Zero total station. The scale trust is the per-cabinet active-surface size in
ScreenMapping (local mm); the root cabinet (V000_R000) is the gauge — its
active-surface frame IS the world frame (R=I, t=0). All other cabinet poses
and the cameras are solved relative to it, so the result is a self-consistent
screen-local reconstruction with no anchors / world datum.

Pipeline:
  1. Load capture manifest (charuco method only this release).
  2. Load screen_mapping + pattern_meta + intrinsics referenced by the manifest.
  3. Preflight: hash pattern_meta and check it against screen_mapping.
  4. Build per-cabinet ChArUco board descriptors + a deterministic cabinet
     index map (root = index of (0,0)).
  5. Detect ChArUco corners across all view images; for each corner look up its
     local mm via screen_mapping, undistort its pixel, and tag it with the
     view's camera index. → list[Observation].
  6. Observability gate.
  7. Init cameras via per-view PnP against the root cabinet (or any seen cabinet
     composed with that cabinet's nominal pose); init cabinet translations from
     the nominal grid (root re-centered to origin).
  8. model_constrained_ba.
  9. Per-cabinet geometry from solved pose + the 4 active-surface CORNERS.
 10. Write cabinet_pose_report.json (if requested).
 11. Emit MeasuredPoints (center in meters) + ResultEvent.
"""
from __future__ import annotations

import hashlib
import json
import os
import pathlib
import tempfile

import cv2
import numpy as np

from lmt_vba_sidecar.capture_manifest import load_capture_manifest
from lmt_vba_sidecar.detect import detect_charuco_corners
from lmt_vba_sidecar.eval_runner import reconstruct_cabinet_geometry
from lmt_vba_sidecar.io_utils import write_event
from lmt_vba_sidecar.ipc import (
    BaStats,
    CabinetPose,
    CabinetPoseReport,
    ErrorEvent,
    FrameSpec,
    MeasuredPoint,
    PatternMeta,
    PointSource,
    PointSourceVisualBa,
    ProgressEvent,
    ReconstructInput,
    ResultData,
    ResultEvent,
    Uncertainty,
    WarningEvent,
)
from lmt_vba_sidecar.model_constrained_ba import Observation, model_constrained_ba
from lmt_vba_sidecar.nominal import nominal_cabinet_centers_model_frame
from lmt_vba_sidecar.observability import ObservabilityError, check_observability
from lmt_vba_sidecar.screen_mapping import ScreenMapping, ScreenMappingError


ROOT_CABINET = (0, 0)  # V000_R000 is the gauge cabinet (world == its frame)
MIN_PNP_CORNERS = 4
FALLBACK_ISOTROPIC_M = 0.005

# --- per-cabinet SOFT quality thresholds (tunable) ---
# These sit ABOVE the HARD observability gate (min_views=2, min_points=8) in
# check_observability: a cabinet that clears observability can still be flagged
# here as a soft warning. A cabinet seen by <2 views never reaches this stage
# (reconstruct aborts at observability_failed first).
QUALITY_MIN_VIEWS = 4  # below this (but >=2) -> "low_observation"
QUALITY_MAX_CABINET_RMS_PX = 2.0  # per-cabinet reproj RMS above this -> "high_residual"


def _classify_cabinet_quality(observed_views: int, reproj_rms_px: float) -> str:
    """Classify a cabinet's soft quality after BA.

    Order: under-observation dominates residual (too few views makes the
    residual itself untrustworthy), so check views first.
    """
    if observed_views < QUALITY_MIN_VIEWS:
        return "low_observation"
    if reproj_rms_px > QUALITY_MAX_CABINET_RMS_PX:
        return "high_residual"
    return "ok"


def _per_cabinet_reproj_rms(
    K: np.ndarray,
    camera_poses: list[tuple[np.ndarray, np.ndarray]],
    cabinet_poses: dict[int, tuple[np.ndarray, np.ndarray]],
    observations: list[Observation],
) -> dict[int, float]:
    """Per-cabinet reprojection RMS (px) using the SAME projection convention as
    model_constrained_ba._residuals: xc = Rc @ (Rb @ p_local + tb) + tc, then
    p = K @ xc; residual = p[:2]/p[2] - pixel.

    RMS over a cabinet's observations is sqrt(mean over obs of (dx^2 + dy^2)),
    matching the BA's global rms = sqrt(mean_obs(dx^2 + dy^2)).

    Precondition: every observation's cabinet_idx must be present in
    cabinet_poses (guaranteed by check_observability upstream, which aborts
    reconstruct unless every cabinet has >=2 views / >=8 observations). The
    returned dict therefore has an entry for every observed cabinet.
    """
    sq_sum: dict[int, float] = {}
    counts: dict[int, int] = {}
    for o in observations:
        Rc, tc = camera_poses[o.camera_idx]
        Rb, tb = cabinet_poses[o.cabinet_idx]
        xw = Rb @ o.p_local + tb
        xc = Rc @ xw + tc
        p = K @ xc
        d = p[:2] / p[2] - o.pixel
        sq_sum[o.cabinet_idx] = sq_sum.get(o.cabinet_idx, 0.0) + float(d @ d)
        counts[o.cabinet_idx] = counts.get(o.cabinet_idx, 0) + 1
    return {
        idx: float(np.sqrt(sq_sum[idx] / counts[idx]))
        for idx in sq_sum
    }


def _undistort_obs(pix: np.ndarray, K: np.ndarray, dist: np.ndarray) -> np.ndarray:
    """Map a single (x, y) pixel through cv2.undistortPoints to its
    pinhole-equivalent pixel coordinate. Returns same shape (2,)."""
    pts = pix.reshape(1, 1, 2).astype(np.float32)
    undistorted_norm = cv2.undistortPoints(pts, K, dist)  # normalized cam
    norm = undistorted_norm.reshape(2)
    out = K @ np.array([norm[0], norm[1], 1.0])
    return out[:2] / out[2]


def pattern_hash(pattern_meta: PatternMeta) -> str:
    """Deterministic pattern hash scheme.

    SHA-256 over the canonical pydantic JSON dump of pattern_meta, truncated to
    16 hex chars. The fixture / pattern producer must set
    ScreenMapping.expected_pattern_hash with this exact scheme.
    """
    return hashlib.sha256(pattern_meta.model_dump_json().encode()).hexdigest()[:16]


def _cabinet_id(col: int, row: int) -> str:
    return f"V{col:03d}_R{row:03d}"


def _active_surface_corners_mm(screen_mapping: ScreenMapping, cabinet_id: str) -> np.ndarray:
    """The 4 active-surface CORNERS in local mm (center origin), BL,BR,TR,TL
    (counter-clockwise starting from bottom-left).

    These are the physical panel corners (±w/2, ±h/2) — NOT the inner ChArUco
    corners — used to derive cabinet center / normal / corners in the report.

    NOTE: the BL,BR,TR,TL ordering is load-bearing — compare_known derives
    cabinet size from this order (width=‖c1-c0‖, height=‖c2-c1‖). Do not reorder
    the array without updating compare_known.compare_known accordingly.
    """
    cab = None
    for c in screen_mapping.cabinets:
        if c.cabinet_id == cabinet_id:
            cab = c
            break
    if cab is None:
        raise ScreenMappingError(f"cabinet '{cabinet_id}' not in screen_mapping")
    w, h = cab.active_size_mm
    hw, hh = w / 2.0, h / 2.0
    return np.array(
        [
            [-hw, -hh, 0.0],
            [hw, -hh, 0.0],
            [hw, hh, 0.0],
            [-hw, hh, 0.0],
        ],
        dtype=float,
    )


def _atomic_write_json(path: str, payload: str) -> None:
    """Write text to path atomically (temp file + os.replace)."""
    directory = os.path.dirname(os.path.abspath(path)) or "."
    os.makedirs(directory, exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=directory, suffix=".tmp")
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as f:
            f.write(payload)
        os.replace(tmp, path)
    except BaseException:
        if os.path.exists(tmp):
            os.remove(tmp)
        raise


def run_reconstruct(cmd: ReconstructInput) -> int:
    write_event(ProgressEvent(event="progress", stage="load", percent=0.0, message="loading capture manifest"))

    # --- 1. capture manifest ---
    try:
        manifest = load_capture_manifest(cmd.capture_manifest_path)
    except Exception as e:  # CaptureManifestError or IO error
        write_event(ErrorEvent(event="error", code="invalid_input", message=str(e), fatal=True))
        return 1
    if manifest.method != "charuco":
        write_event(ErrorEvent(
            event="error", code="invalid_input",
            message=f"only charuco implemented; structured-light gated (method={manifest.method})",
            fatal=True,
        ))
        return 1

    # --- 2. referenced files ---
    # screen_mapping: an explicit cmd.screen_mapping_path overrides the
    # manifest's reference (lets a caller swap in a corrected mapping without
    # editing the manifest); otherwise use the manifest-resolved path.
    sm_path = cmd.screen_mapping_path or manifest.screen_mapping
    try:
        screen_mapping = ScreenMapping.model_validate(
            json.loads(pathlib.Path(sm_path).read_text(encoding="utf-8"))
        )
        pattern_meta = PatternMeta.model_validate(
            json.loads(pathlib.Path(manifest.pattern_meta).read_text(encoding="utf-8"))
        )
        intr = json.loads(pathlib.Path(manifest.intrinsics).read_text(encoding="utf-8"))
        K = np.array(intr["K"], dtype=float)
        dist = np.array(intr["dist_coeffs"], dtype=float)
        image_size = tuple(int(v) for v in intr["image_size"])
    except (OSError, json.JSONDecodeError, KeyError, ValueError) as e:
        write_event(ErrorEvent(event="error", code="invalid_input", message=f"failed to load manifest references: {e}", fatal=True))
        return 1

    # --- 3. preflight ---
    # image_size here is the CAMERA frame size, which is unrelated to a
    # cabinet's LED canvas resolution_px; passing it would false-positive the
    # best-effort cross-check. Only the pattern-hash check is load-bearing.
    _ = image_size  # retained for diagnostics; intentionally not cross-checked
    try:
        screen_mapping.preflight(pattern_hash(pattern_meta))
    except ScreenMappingError as e:
        write_event(ErrorEvent(event="error", code="invalid_input", message=str(e), fatal=True))
        return 1

    # Per-cabinet board shape (v2): (col,row) -> (squares_x, squares_y, square_px).
    shape_by_cr = {(c.col, c.row): (c.squares_x, c.squares_y, c.square_px)
                   for c in pattern_meta.cabinets}

    # --- 4. boards + deterministic cabinet index map ---
    present = sorted(
        ((c.col, c.row) for c in pattern_meta.cabinets),
        key=lambda cr: (cr[1], cr[0]),  # (row, col) order
    )
    if ROOT_CABINET not in present:
        write_event(ErrorEvent(
            event="error", code="invalid_input",
            message=f"root cabinet {_cabinet_id(*ROOT_CABINET)} (0,0) not present in pattern_meta",
            fatal=True,
        ))
        return 1
    cab_to_idx: dict[tuple[int, int], int] = {cr: i for i, cr in enumerate(present)}
    root_idx = cab_to_idx[ROOT_CABINET]
    n_cabinets = len(present)

    boards = [
        {"cabinet": (c.col, c.row),
         "aruco_id_start": c.aruco_id_start, "aruco_id_end": c.aruco_id_end,
         "squares_x": c.squares_x, "squares_y": c.squares_y}
        for c in pattern_meta.cabinets
    ]

    # --- 5. detect + build observations ---
    write_event(ProgressEvent(event="progress", stage="detect_charuco", percent=0.2, message="detecting ChArUco corners"))
    view_images: list[list[str]] = [list(v.images) for v in manifest.views]
    all_paths = [p for imgs in view_images for p in imgs]
    detections = detect_charuco_corners(all_paths, boards=boards)

    observations: list[Observation] = []
    # camera_idx == view index; aggregate corners per (view, cabinet) for PnP.
    per_view_cab_corners: dict[tuple[int, int], list[tuple[np.ndarray, np.ndarray]]] = {}
    per_cabinet_views: dict[int, set[int]] = {}
    per_cabinet_points: dict[int, int] = {}
    for cam_idx, imgs in enumerate(view_images):
        for path in imgs:
            for det in detections.get(path, []):
                cab_cr = tuple(det["cabinet"])
                if cab_cr not in cab_to_idx:
                    continue
                cab_idx = cab_to_idx[cab_cr]
                charuco_id = int(det["charuco_id"])
                sx, sy, spx = shape_by_cr[cab_cr]
                p_local = screen_mapping.charuco_corner_local_mm(
                    _cabinet_id(*cab_cr), charuco_id,
                    squares_x=sx, squares_y=sy, square_px=spx,
                )
                pixel = _undistort_obs(np.array(det["corner_px"], dtype=float), K, dist)
                observations.append(Observation(
                    camera_idx=cam_idx, cabinet_idx=cab_idx,
                    p_local=p_local, pixel=pixel,
                ))
                per_view_cab_corners.setdefault((cam_idx, cab_idx), []).append((p_local, pixel))
                per_cabinet_views.setdefault(cab_idx, set()).add(cam_idx)
                per_cabinet_points[cab_idx] = per_cabinet_points.get(cab_idx, 0) + 1

    if not observations:
        write_event(ErrorEvent(
            event="error", code="detection_failed",
            message="no ChArUco corners detected across any view",
            fatal=True,
        ))
        return 1

    # --- 6. observability ---
    try:
        check_observability(observations, n_cabinets, min_views=2, min_points=8)
    except ObservabilityError as e:
        write_event(ErrorEvent(event="error", code="observability_failed", message=str(e), fatal=True))
        return 1

    # --- 7. init ---
    write_event(ProgressEvent(event="progress", stage="bundle_adjustment", percent=0.5, message="initializing"))
    # Cabinet nominal world (model) positions, mm, with root re-centered to origin.
    try:
        nominal_m = nominal_cabinet_centers_model_frame(cmd.project.cabinet_array, cmd.project.shape_prior)
    except ValueError as e:
        write_event(ErrorEvent(event="error", code="invalid_input", message=str(e), fatal=True))
        return 1
    if ROOT_CABINET not in nominal_m:
        write_event(ErrorEvent(
            event="error", code="invalid_input",
            message="root cabinet (0,0) missing from nominal model (absent_cells?)",
            fatal=True,
        ))
        return 1
    root_nominal_mm = np.array(nominal_m[ROOT_CABINET], dtype=float) * 1000.0
    # Bridge-camera init: estimate each non-root cabinet's world pose from views
    # that see it together with the root. Falls back to nominal (flat/curved)
    # translation + identity rotation when no bridge view exists for a cabinet.
    bridge = estimate_nonroot_cabinet_init(per_view_cab_corners, root_idx, K)
    init_cabinets: dict[int, tuple[np.ndarray, np.ndarray]] = {}
    for cr, idx in cab_to_idx.items():
        if idx == root_idx:
            init_cabinets[idx] = (np.eye(3), np.zeros(3))
        elif idx in bridge:
            init_cabinets[idx] = bridge[idx]
        elif cr in nominal_m:
            t_mm = np.array(nominal_m[cr], dtype=float) * 1000.0 - root_nominal_mm
            init_cabinets[idx] = (np.eye(3), t_mm)
        else:
            init_cabinets[idx] = (np.eye(3), np.zeros(3))

    # Camera init via PnP. Object points are local mm; the root cabinet frame is
    # world, so PnP against the root gives camera_from_world directly. For views
    # that don't see the root well, PnP against any seen cabinet, then compose
    # with that cabinet's nominal world pose: T_cam_world = T_cam_cab @ T_cab_world.
    init_cameras: list[tuple[np.ndarray, np.ndarray]] = []
    n_cameras = len(view_images)
    for cam_idx in range(n_cameras):
        pose = _pnp_camera(cam_idx, root_idx, init_cabinets, per_view_cab_corners, K)
        init_cameras.append(pose)

    # --- 8. BA ---
    result = model_constrained_ba(
        K=K, observations=observations,
        n_cameras=n_cameras, n_cabinets=n_cabinets,
        root_cabinet_idx=root_idx,
        init_cameras=init_cameras, init_cabinets=init_cabinets,
        loss="huber",
    )
    if not result.converged:
        write_event(ErrorEvent(
            event="error", code="ba_diverged",
            message=f"BA did not converge (rms={result.rms_reprojection_px:.2f}px after {result.iterations} iters)",
            fatal=True,
        ))
        return 1

    # --- 9. per-cabinet geometry ---
    write_event(ProgressEvent(event="progress", stage="output", percent=0.9, message="building pose report"))
    # Per-cabinet reprojection RMS from the solved poses (same projection as BA).
    per_cabinet_rms = _per_cabinet_reproj_rms(
        K, result.camera_poses, result.cabinet_poses, observations
    )
    idx_to_cab = {idx: cr for cr, idx in cab_to_idx.items()}
    cabinet_poses: list[CabinetPose] = []
    measured_points: list[MeasuredPoint] = []
    for idx in range(n_cabinets):
        col, row = idx_to_cab[idx]
        cid = _cabinet_id(col, row)
        R, t = result.cabinet_poses[idx]
        corners_local = _active_surface_corners_mm(screen_mapping, cid)
        center, normal, _size, world_corners = reconstruct_cabinet_geometry(R, t, corners_local)
        n_views = len(per_cabinet_views.get(idx, set()))
        n_points = per_cabinet_points.get(idx, 0)
        # Direct index (not .get): observability upstream guarantees every
        # cabinet has observations, so a missing entry is a broken invariant we
        # want surfaced as a loud KeyError, not masked as a fake 0.0 RMS.
        cab_rms = per_cabinet_rms[idx]
        quality = _classify_cabinet_quality(n_views, cab_rms)

        cabinet_poses.append(CabinetPose(
            cabinet_id=cid,
            position_mm=center.tolist(),
            rotation_matrix=R.tolist(),
            normal=normal.tolist(),
            corners_mm=[c.tolist() for c in world_corners],
            reprojection_rms_px=cab_rms,
            observed_views=n_views,
            observed_points=n_points,
            quality=quality,
        ))
        if quality != "ok":
            write_event(WarningEvent(
                event="warning", code="cabinet_quality",
                message=f"cabinet {cid}: {quality} (views={n_views}, rms={cab_rms:.2f}px)",
                cabinet=cid,
            ))

        # MeasuredPoint position is in METERS.
        cov_mm = result.cabinet_covariances.get(idx)
        if cov_mm is not None and np.isfinite(cov_mm).all():
            cov_m = np.asarray(cov_mm, dtype=float) / 1.0e6  # mm^2 -> m^2
            uncertainty = Uncertainty(covariance=cov_m.tolist())
        else:
            write_event(WarningEvent(
                event="warning", code="missing_covariance",
                message=f"cabinet {cid} has no usable BA covariance; falling back to isotropic 5mm",
                cabinet=f"MAIN_{cid}",
            ))
            uncertainty = Uncertainty(isotropic=FALLBACK_ISOTROPIC_M)
        measured_points.append(MeasuredPoint(
            name=f"MAIN_{cid}",
            position=(center / 1000.0).tolist(),
            uncertainty=uncertainty,
            source=PointSource(visual_ba=PointSourceVisualBa(camera_count=max(1, n_views))),
        ))

    # --- 10. write pose report ---
    if cmd.pose_report_path:
        report = CabinetPoseReport(
            schema_version="visual_pose_report.v1",
            frame=FrameSpec(root_cabinet=list(ROOT_CABINET)),
            cabinet_poses=cabinet_poses,
        )
        _atomic_write_json(cmd.pose_report_path, report.model_dump_json(indent=2))

    # --- 11. result ---
    write_event(ResultEvent(
        event="result",
        data=ResultData(
            measured_points=measured_points,
            ba_stats=BaStats(
                rms_reprojection_px=float(result.rms_reprojection_px),
                iterations=int(result.iterations),
                converged=True,
            ),
            frame_strategy_used="nominal_anchoring",  # vestigial; no Procrustes runs
            procrustes_align_rms_m=0.0,
        ),
    ))
    return 0


def _solve_pnp(corners, K):
    """corners: list[(p_local_mm, pixel_undistorted)] -> (R, t) camera_from_obj, or None."""
    obj = np.array([p for p, _ in corners], dtype=np.float64)
    img = np.array([px for _, px in corners], dtype=np.float64)
    ok, rvec, tvec = cv2.solvePnP(obj, img, K, None, flags=cv2.SOLVEPNP_ITERATIVE)
    if not ok:
        return None
    R, _ = cv2.Rodrigues(rvec)
    return R, tvec.reshape(3)


def _avg_rotation(rotations):
    """SVD-average a set of rotation matrices; result is orthonormal with det=+1."""
    if not rotations:
        raise ValueError("_avg_rotation needs at least one rotation")
    S = sum(rotations)
    U, _, Vt = np.linalg.svd(S)
    R = U @ Vt
    if np.linalg.det(R) < 0:
        U[:, -1] *= -1
        R = U @ Vt
    return R


def estimate_nonroot_cabinet_init(
    per_view_cab_corners: dict[tuple[int, int], list[tuple[np.ndarray, np.ndarray]]],
    root_idx: int,
    K: np.ndarray,
    min_corners: int = MIN_PNP_CORNERS,
) -> dict[int, tuple[np.ndarray, np.ndarray]]:
    """Non-root cabinet_idx -> (R_world_from_cab, t_mm) via bridge cameras.

    A bridge view sees the root cabinet AND a non-root cabinet (each with
    >= min_corners corners). Two PnPs give camera_from_root / camera_from_nonroot;
    compose to world_from_nonroot:
        R = Rc0.T @ Rc1 ,  t = Rc0.T @ (tc1 - tc0)
    Multiple bridge views: rotations SVD-averaged, translation component-wise median.
    Cabinets with no bridge view are absent from the result (caller falls back to
    nominal).

    Limitation: only direct root<->non-root bridging; no transitive bridging for
    distant cabinets that share no view with the root (large chained-topology
    screens). Current target is the monitor bench (2 panels) + small screens.
    """
    by_view: dict[int, dict[int, list]] = {}
    for (cam_idx, cab_idx), corners in per_view_cab_corners.items():
        by_view.setdefault(cam_idx, {})[cab_idx] = corners

    est_R: dict[int, list] = {}
    est_t: dict[int, list] = {}
    for cabs in by_view.values():
        root_corners = cabs.get(root_idx, [])
        if len(root_corners) < min_corners:
            continue
        pose_root = _solve_pnp(root_corners, K)
        if pose_root is None:
            continue
        Rc0, tc0 = pose_root
        for cab_idx, corners in cabs.items():
            if cab_idx == root_idx or len(corners) < min_corners:
                continue
            pose_cab = _solve_pnp(corners, K)
            if pose_cab is None:
                continue
            Rc1, tc1 = pose_cab
            est_R.setdefault(cab_idx, []).append(Rc0.T @ Rc1)
            est_t.setdefault(cab_idx, []).append(Rc0.T @ (tc1 - tc0))

    out: dict[int, tuple] = {}
    for cab_idx, rotations in est_R.items():
        t = np.median(np.array(est_t[cab_idx]), axis=0)
        out[cab_idx] = (_avg_rotation(rotations), t)
    return out


def _pnp_camera(
    cam_idx: int,
    root_idx: int,
    init_cabinets: dict[int, tuple[np.ndarray, np.ndarray]],
    per_view_cab_corners: dict[tuple[int, int], list[tuple[np.ndarray, np.ndarray]]],
    K: np.ndarray,
) -> tuple[np.ndarray, np.ndarray]:
    """Initialize one camera's world-to-camera pose via PnP.

    Prefer the root cabinet (its frame == world). Fall back to any seen cabinet
    and compose with that cabinet's nominal world pose. If no cabinet yields ≥4
    corners, return a neutral guess (BA still has the metric scale from local
    coords + other well-init cameras to recover).
    """
    # Try root first.
    root_corners = per_view_cab_corners.get((cam_idx, root_idx), [])
    if len(root_corners) >= MIN_PNP_CORNERS:
        pose = _solve_pnp(root_corners, K)
        if pose is not None:
            return pose

    # Fall back to any other cabinet this view sees, composing with the inverse
    # of its nominal world_from_cabinet pose: T_cam_world = T_cam_cab @ T_cab_world,
    # where T_cab_world = inverse(world_from_cabinet).
    for (ci, cab_idx), corners in per_view_cab_corners.items():
        if ci != cam_idx or cab_idx == root_idx or len(corners) < MIN_PNP_CORNERS:
            continue
        cam_from_cab = _solve_pnp(corners, K)
        if cam_from_cab is None:
            continue
        Rcc, tcc = cam_from_cab  # camera_from_cabinet: x_cam = Rcc·p_local + tcc
        # init_cabinets stores world_from_cabinet (BA: xw = R_wc·p_local + t_wc).
        # camera_from_world = camera_from_cabinet ∘ inverse(world_from_cabinet).
        R_wc, t_wc = init_cabinets[cab_idx]  # world_from_cabinet (nominal)
        R = Rcc @ R_wc.T
        t = tcc - R @ t_wc
        return R, t

    # Neutral fallback: identity rotation, pushed back along +z.
    return np.eye(3), np.array([0.0, 0.0, 2200.0])
