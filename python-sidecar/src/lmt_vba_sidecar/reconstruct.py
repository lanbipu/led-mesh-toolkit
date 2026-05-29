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

from collections.abc import Callable

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
from lmt_vba_sidecar.model_constrained_ba import (
    Observation,
    model_constrained_ba,
    _residuals,
    _nonroot_cabinets,
    _pack,
)
from lmt_vba_sidecar.nominal import (
    nominal_cabinet_centers_model_frame,
    nominal_cabinet_normals_model_frame,
)
from lmt_vba_sidecar.observability import ObservabilityError, check_observability
from lmt_vba_sidecar.screen_mapping import ScreenMapping, ScreenMappingError


ROOT_CABINET = (0, 0)  # V000_R000 is the gauge cabinet (world == its frame)
MIN_PNP_CORNERS = 4
# Stage A PnP-RANSAC: gross-outlier reject threshold + RANSAC config (sidecar
# internal constants; NOT a CLI knob). 2-3px is below the minimum resolvable
# inter-dot spacing in the image, so near-neighbor mis-IDs still exceed it.
PNP_RANSAC_REPROJ_PX = 3.0
PNP_RANSAC_CONFIDENCE = 0.99
PNP_RANSAC_ITERS = 100
FALLBACK_ISOTROPIC_M = 0.005

# --- per-cabinet SOFT quality thresholds (tunable) ---
# These sit ABOVE the HARD observability gate (min_views=2, min_points=8) in
# check_observability: a cabinet that clears observability can still be flagged
# here as a soft warning. A cabinet seen by <2 views never reaches this stage
# (reconstruct aborts at observability_failed first).
QUALITY_MIN_VIEWS = 4  # below this (but >=2) -> "low_observation"
QUALITY_MAX_CABINET_RMS_PX = 2.0  # per-cabinet reproj RMS above this -> "high_residual"

# --- Stage B robust-residual trim (PRIMARY geometric authority) ---
STAGE_B_MAX_ITERS = 3
STAGE_B_MAD_K = 3.0
STAGE_B_ABS_PX_FLOOR = 3.0
STAGE_B_GROUP_MEDIAN_PX = 4.0  # whole-(cam,cab)-group coherence guard


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


def _obs_residual_norms(K, result, observations, root_idx):
    """Per-observation reprojection residual norm (px), using the CURRENT
    iteration's poses (recomputed, not stale sol.fun)."""
    nonroot = _nonroot_cabinets(
        max(observations, key=lambda o: o.cabinet_idx).cabinet_idx + 1, root_idx)
    # Reuse model_constrained_ba._residuals by packing the solved state.
    cabs = dict(result.cabinet_poses)
    for j in nonroot:
        cabs.setdefault(j, (np.eye(3), np.zeros(3)))
    x = _pack(result.camera_poses, cabs, nonroot)
    res = _residuals(x, len(result.camera_poses), nonroot, root_idx, K, observations)
    r = res.reshape(-1, 2)
    return np.sqrt((r * r).sum(axis=1))


def stage_b_robust_solve(*, K, observations, n_cameras, n_cabinets,
                         root_cabinet_idx, init_cameras, init_cabinets,
                         per_cabinet_min_points):
    """Iterative robust-residual trim wrapping model_constrained_ba (PRIMARY
    geometric authority). Recomputes residuals each iter (sol.fun is stale),
    drops norm > max(k*MAD, abs_px_floor) plus whole-(cam,cab)-group coherence
    outliers, re-solves, <=3 iters. Never trims any cabinet below
    per_cabinet_min_points. Returns (result, rejected_per_cab, total,
    surviving_observations) where surviving_observations is the trimmed obs list
    the final solve ran on (caller reuses it for _per_cabinet_reproj_rms,
    per-cabinet view/point recompute, and the post-trim observability check)."""
    obs = list(observations)
    rejected_per_cab: dict[int, int] = {}
    result = model_constrained_ba(
        K=K, observations=obs, n_cameras=n_cameras, n_cabinets=n_cabinets,
        root_cabinet_idx=root_cabinet_idx, init_cameras=init_cameras,
        init_cabinets=init_cabinets, loss="huber")
    for _ in range(STAGE_B_MAX_ITERS):
        norms = _obs_residual_norms(K, result, obs, root_cabinet_idx)
        mad = float(np.median(np.abs(norms - np.median(norms)))) or 0.0
        thr = max(STAGE_B_MAD_K * mad, STAGE_B_ABS_PX_FLOOR)
        # group coherence: median residual per (cam,cab)
        group_norms: dict[tuple[int, int], list[float]] = {}
        for o, nrm in zip(obs, norms):
            group_norms.setdefault((o.camera_idx, o.cabinet_idx), []).append(nrm)
        bad_groups = {g for g, v in group_norms.items()
                      if float(np.median(v)) > STAGE_B_GROUP_MEDIAN_PX}
        # candidate drops: pointwise OR in a bad group
        drop = [(nrm > thr) or ((o.camera_idx, o.cabinet_idx) in bad_groups)
                for o, nrm in zip(obs, norms)]
        if not any(drop):
            break
        # floor guard: per cabinet, never go below min_points
        from collections import Counter
        kept_counts = Counter(o.cabinet_idx for o, d in zip(obs, drop) if not d)
        new_obs = []
        n_dropped_this_iter = 0
        for o, d in zip(obs, drop):
            if d and kept_counts.get(o.cabinet_idx, 0) >= per_cabinet_min_points:
                rejected_per_cab[o.cabinet_idx] = rejected_per_cab.get(o.cabinet_idx, 0) + 1
                n_dropped_this_iter += 1
            else:
                new_obs.append(o)
                if d:  # wanted to drop but floor blocked it -> protect by keeping
                    kept_counts[o.cabinet_idx] = kept_counts.get(o.cabinet_idx, 0) + 1
        if n_dropped_this_iter == 0:
            break
        obs = new_obs
        result = model_constrained_ba(
            K=K, observations=obs, n_cameras=n_cameras, n_cabinets=n_cabinets,
            root_cabinet_idx=root_cabinet_idx, init_cameras=init_cameras,
            init_cabinets=init_cabinets, loss="huber")
    total = sum(rejected_per_cab.values())
    return result, rejected_per_cab, total, obs


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

    # --- 5b. Stage A pre-clean: per-(cam,cab) PnP-RANSAC inlier filter ---
    (observations, per_view_cab_corners, per_cabinet_views, per_cabinet_points,
     n_rej_stage_a, rej_per_cab_stage_a) = stage_a_prune(observations, per_view_cab_corners, K)

    # --- 6. observability ---
    try:
        check_observability(observations, n_cabinets, min_views=2, min_points=8)
    except ObservabilityError as e:
        write_event(ErrorEvent(event="error", code="observability_failed", message=str(e), fatal=True))
        return 1

    # --- 7. nominal model (kept here: needs cmd.project) ---
    try:
        nominal_m = nominal_cabinet_centers_model_frame(cmd.project.cabinet_array, cmd.project.shape_prior)
        nominal_normals_m = nominal_cabinet_normals_model_frame(cmd.project.cabinet_array, cmd.project.shape_prior)
    except ValueError as e:
        write_event(ErrorEvent(event="error", code="invalid_input", message=str(e), fatal=True))
        return 1

    return solve_and_emit(
        K=K, observations=observations, per_view_cab_corners=per_view_cab_corners,
        n_cameras=len(view_images), cab_to_idx=cab_to_idx, root_idx=root_idx,
        n_cabinets=n_cabinets, nominal_m=nominal_m, nominal_normals_m=nominal_normals_m,
        per_cabinet_views=per_cabinet_views, per_cabinet_points=per_cabinet_points,
        corners_local_provider=lambda cid: _active_surface_corners_mm(screen_mapping, cid),
        pose_report_path=cmd.pose_report_path,
        n_rejected_pre=n_rej_stage_a, rejected_per_cab_pre=rej_per_cab_stage_a,
    )


def solve_and_emit(
    *,
    K: np.ndarray,
    observations: list[Observation],
    per_view_cab_corners: dict[tuple[int, int], list[tuple[np.ndarray, np.ndarray]]],
    n_cameras: int,
    cab_to_idx: dict[tuple[int, int], int],
    root_idx: int,
    n_cabinets: int,
    nominal_m: dict[tuple[int, int], tuple[float, float, float]],
    nominal_normals_m: dict[tuple[int, int], tuple[float, float, float]],
    per_cabinet_views: dict[int, set[int]],
    per_cabinet_points: dict[int, int],
    corners_local_provider: Callable[[str], np.ndarray],
    pose_report_path: str | None,
    n_rejected_pre: int = 0,
    rejected_per_cab_pre: dict[int, int] | None = None,
) -> int:
    """Shared init -> model_constrained_ba -> per-cabinet geometry -> report ->
    ResultEvent. Used by both run_reconstruct (charuco) and
    sl_reconstruct.run_reconstruct_structured_light. corners_local_provider maps
    a cabinet_id string to its (4,3) active-surface corners in local mm."""
    # --- 7. init ---
    write_event(ProgressEvent(event="progress", stage="bundle_adjustment", percent=0.5, message="initializing"))
    if ROOT_CABINET not in nominal_m:
        write_event(ErrorEvent(
            event="error", code="invalid_input",
            message="root cabinet (0,0) missing from nominal model (absent_cells?)",
            fatal=True,
        ))
        return 1
    root_nominal_mm = np.array(nominal_m[ROOT_CABINET], dtype=float) * 1000.0
    idx_to_cab = {idx: cr for cr, idx in cab_to_idx.items()}
    # idx-keyed nominal normals/centers for branch disambiguation.
    nominal_normals_idx = {cab_to_idx[cr]: n for cr, n in nominal_normals_m.items()
                           if cr in cab_to_idx}
    nominal_centers_idx = {cab_to_idx[cr]: c for cr, c in nominal_m.items()
                           if cr in cab_to_idx}
    # Bridge-camera init: estimate each non-root cabinet's world pose from views
    # that see it together with the root, resolving the IPPE convex/concave mirror
    # against nominal model-frame normals. Falls back to nominal (flat/curved)
    # translation + identity rotation when no bridge view exists for a cabinet.
    bridge, undecidable_cabs = estimate_nonroot_cabinet_init(
        per_view_cab_corners, root_idx, K,
        nominal_normals=nominal_normals_idx, nominal_centers=nominal_centers_idx,
    )
    if undecidable_cabs:
        ids = sorted(_cabinet_id(*idx_to_cab[j]) for j in undecidable_cabs)
        write_event(ErrorEvent(
            event="error", code="observability_failed",
            message=(f"convex/concave undecidable for cabinet(s) {ids}: planar-PnP "
                     f"mirror branches equally match nominal and no redundant view "
                     f"breaks the tie; add a camera that sees these cabinets"),
            fatal=True))
        return 1
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
    for cam_idx in range(n_cameras):
        pose = _pnp_camera(cam_idx, root_idx, init_cabinets, per_view_cab_corners, K)
        init_cameras.append(pose)

    # --- 8. BA (Stage B robust-residual trim — PRIMARY geometric authority) ---
    result, rejected_per_cab_stage_b, n_rej_stage_b, surviving_observations = \
        stage_b_robust_solve(
            K=K, observations=observations, n_cameras=n_cameras,
            n_cabinets=n_cabinets, root_cabinet_idx=root_idx,
            init_cameras=init_cameras, init_cabinets=init_cabinets,
            per_cabinet_min_points=8)
    if not result.converged:
        write_event(ErrorEvent(
            event="error", code="ba_diverged",
            message=f"BA did not converge (rms={result.rms_reprojection_px:.2f}px after {result.iterations} iters)",
            fatal=True,
        ))
        return 1

    # Rejection accounting: Stage A removed n_rejected_pre observations before
    # this function got `observations`; Stage B trimmed n_rej_stage_b more.
    # n_used = surviving obs the final solve ran on; n_total folds both stages.
    rejected_per_cab_pre = rejected_per_cab_pre or {}
    n_used = len(surviving_observations)
    n_rej = n_rejected_pre + n_rej_stage_b
    n_total = n_used + n_rej

    # --- 9. per-cabinet geometry ---
    write_event(ProgressEvent(event="progress", stage="output", percent=0.9, message="building pose report"))
    # recompute per-cabinet indices from the trimmed (surviving) observations
    per_cabinet_views = {}
    per_cabinet_points = {}
    for o in surviving_observations:
        per_cabinet_views.setdefault(o.cabinet_idx, set()).add(o.camera_idx)
        per_cabinet_points[o.cabinet_idx] = per_cabinet_points.get(o.cabinet_idx, 0) + 1
    # post-trim observability: trimming an outlier-heavy cabinet below the floor
    # is a hard stop (no silent wrong measured.yaml). Re-enforce BOTH dimensions
    # of the pre-trim check_observability(min_views=2, min_points=8): a coherent
    # mis-decode in one of only two views gets its (cam,cab) group trimmed,
    # leaving the cabinet with a single view — geometrically under-determined, so
    # this must hard-stop rather than emit a 1-view "result".
    for idx in range(n_cabinets):
        n_pts = per_cabinet_points.get(idx, 0)
        n_views = len(per_cabinet_views.get(idx, set()))
        if n_pts < 8 or n_views < 2:
            cid = _cabinet_id(*idx_to_cab[idx])
            write_event(ErrorEvent(
                event="error", code="observability_failed",
                message=(f"after rejecting {n_rej_stage_b} outliers, cabinet {cid} "
                         f"has only {n_pts} observations across {n_views} view(s) "
                         f"(needs >=8 points and >=2 views)"),
                fatal=True))
            return 1
    # Per-cabinet reprojection RMS from the solved poses (same projection as BA).
    per_cabinet_rms = _per_cabinet_reproj_rms(
        K, result.camera_poses, result.cabinet_poses, surviving_observations
    )
    cabinet_poses: list[CabinetPose] = []
    measured_points: list[MeasuredPoint] = []
    for idx in range(n_cabinets):
        col, row = idx_to_cab[idx]
        cid = _cabinet_id(col, row)
        R, t = result.cabinet_poses[idx]
        corners_local = corners_local_provider(cid)
        center, normal, _size, world_corners = reconstruct_cabinet_geometry(R, t, corners_local)
        n_views = len(per_cabinet_views.get(idx, set()))
        n_points = per_cabinet_points.get(idx, 0)
        # Direct index (not .get): observability upstream guarantees every
        # cabinet has observations, so a missing entry is a broken invariant we
        # want surfaced as a loud KeyError, not masked as a fake 0.0 RMS.
        cab_rms = per_cabinet_rms[idx]
        quality = _classify_cabinet_quality(n_views, cab_rms)
        rejected_points = rejected_per_cab_pre.get(idx, 0) + rejected_per_cab_stage_b.get(idx, 0)

        cabinet_poses.append(CabinetPose(
            cabinet_id=cid,
            position_mm=center.tolist(),
            rotation_matrix=R.tolist(),
            normal=normal.tolist(),
            corners_mm=[c.tolist() for c in world_corners],
            reprojection_rms_px=cab_rms,
            observed_views=n_views,
            observed_points=n_points,
            rejected_points=rejected_points,
            quality=quality,
        ))
        if quality != "ok":
            write_event(WarningEvent(
                event="warning", code="cabinet_quality",
                message=f"cabinet {cid}: {quality} (views={n_views}, rms={cab_rms:.2f}px)",
                cabinet=cid,
            ))
        if rejected_points and rejected_points / (rejected_points + n_points) > 0.30:
            write_event(WarningEvent(
                event="warning", code="high_rejection",
                message=f"cabinet {cid}: rejected {rejected_points}/{rejected_points+n_points} observations",
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
    if pose_report_path:
        report = CabinetPoseReport(
            schema_version="visual_pose_report.v1",
            frame=FrameSpec(root_cabinet=list(ROOT_CABINET)),
            cabinet_poses=cabinet_poses,
        )
        _atomic_write_json(pose_report_path, report.model_dump_json(indent=2))

    # --- 11. result ---
    write_event(ResultEvent(
        event="result",
        data=ResultData(
            measured_points=measured_points,
            ba_stats=BaStats(
                rms_reprojection_px=float(result.rms_reprojection_px),
                iterations=int(result.iterations),
                converged=True,
                n_observations_total=n_total,
                n_observations_used=n_used,
                n_rejected=n_rej,
            ),
            frame_strategy_used="nominal_anchoring",  # vestigial; no Procrustes runs
            procrustes_align_rms_m=0.0,
        ),
    ))
    return 0


def _solve_pnp_branches(corners, K):
    """corners: list[(p_local_mm, pixel_undistorted)] ->
    (branches, inlier_mask) or None.

    branches: list of 1-2 (R, t) camera_from_obj poses. The planar PnP mirror
    ambiguity (IPPE) yields up to two near-equal-reprojection branches; both
    are returned so the model-frame assembly can disambiguate (Part C). Branch
    order is OpenCV's (ascending reprojection error).
    inlier_mask: bool ndarray (len(corners),) from solvePnPRansac — gross
    outliers are False (Part C disambiguation + Stage A both consume this).

    Returns None for < 4 correspondences and for geometrically degenerate sets
    (near-collinear -> cv2.error). tvec is reshaped to (3,).
    """
    if len(corners) < MIN_PNP_CORNERS:
        return None
    obj = np.array([p for p, _ in corners], dtype=np.float64)
    img = np.array([px for _, px in corners], dtype=np.float64)
    try:
        ok, _rvec, _tvec, inliers = cv2.solvePnPRansac(
            obj, img, K, None, iterationsCount=PNP_RANSAC_ITERS,
            reprojectionError=PNP_RANSAC_REPROJ_PX, confidence=PNP_RANSAC_CONFIDENCE,
            flags=cv2.SOLVEPNP_ITERATIVE,
        )
    except cv2.error:
        return None
    if not ok:
        return None
    mask = np.zeros(len(corners), dtype=bool)
    if inliers is not None:
        mask[inliers.reshape(-1)] = True
    else:
        mask[:] = True
    if int(mask.sum()) < MIN_PNP_CORNERS:
        return None
    in_obj = obj[mask]
    in_img = img[mask]
    # Two-branch planar solve on the inliers (IPPE needs coplanar z=0 points).
    try:
        retval, rvecs, tvecs = cv2.solvePnPGeneric(
            in_obj, in_img, K, None, flags=cv2.SOLVEPNP_IPPE
        )[:3]
    except cv2.error:
        return None
    if retval < 1:
        return None
    branches = []
    for i in range(retval):
        rvec = np.asarray(rvecs[i], dtype=float)
        tvec = np.asarray(tvecs[i], dtype=float).reshape(3)
        # Near-collinear / degenerate inputs let solvePnPRansac "succeed" but make
        # solvePnPGeneric(IPPE) emit NaN poses; reject those to preserve the
        # degenerate -> None contract (legacy _solve_pnp returned None here).
        if not (np.isfinite(rvec).all() and np.isfinite(tvec).all()):
            continue
        R, _ = cv2.Rodrigues(rvec)
        branches.append((R, tvec))
    if not branches:
        return None
    return branches, mask


def _solve_pnp(corners, K):
    """corners: list[(p_local_mm, pixel_undistorted)] -> (R, t) or None.

    Backward-compatible single-pose form: the RANSAC+IPPE best branch (branch 0,
    lowest reprojection). Used by callers that don't disambiguate (camera init).
    Returns None on the same degenerate / too-few conditions as
    _solve_pnp_branches.
    """
    res = _solve_pnp_branches(corners, K)
    if res is None:
        return None
    branches, _mask = res
    return branches[0]


def stage_a_prune(observations, per_view_cab_corners, K):
    """Stage A pre-clean: per-(cam,cab) PnP-RANSAC inlier filter. Drops gross /
    random-far and independent near-neighbor mis-IDs whose reprojection exceeds
    PNP_RANSAC_REPROJ_PX. NOT authoritative for coherent shifts (those pass to
    Stage B). Groups with < MIN_PNP_CORNERS are kept whole. Rebuilds the
    observation list + per_view_cab_corners + per-cabinet view/point indices
    from the inliers. Returns (obs_out, pvcc_out, per_cabinet_views,
    per_cabinet_points, n_rejected_total, rejected_per_cab) where
    rejected_per_cab: dict[int,int] is the per-cabinet Stage-A reject count
    (Task 6 stats + Task 7 tests consume it)."""
    # Map each (cam,cab) corner index back to its source so we can keep aligned
    # Observation objects (assembly appends to both lists in lockstep).
    keep_mask: dict[tuple[int, int], list[bool]] = {}
    n_rejected_total = 0
    rejected_per_cab: dict[int, int] = {}
    for key, corners in per_view_cab_corners.items():
        _cam_idx, cab_idx = key
        if len(corners) < MIN_PNP_CORNERS:
            keep_mask[key] = [True] * len(corners)
            continue
        res = _solve_pnp_branches(corners, K)
        if res is None:
            keep_mask[key] = [True] * len(corners)  # degenerate -> defer to Stage B
            continue
        _branches, mask = res
        keep_mask[key] = list(mask)
        n_rej = int((~mask).sum())
        n_rejected_total += n_rej
        if n_rej:
            rejected_per_cab[cab_idx] = rejected_per_cab.get(cab_idx, 0) + n_rej

    # Rebuild aligned outputs. Walk observations in order, consuming each
    # group's mask in the same append order assembly used.
    cursor: dict[tuple[int, int], int] = {}
    obs_out = []
    pvcc_out: dict[tuple[int, int], list] = {}
    views_out: dict[int, set] = {}
    pts_out: dict[int, int] = {}
    for o in observations:
        key = (o.camera_idx, o.cabinet_idx)
        i = cursor.get(key, 0)
        cursor[key] = i + 1
        if not keep_mask[key][i]:
            continue
        obs_out.append(o)
        pvcc_out.setdefault(key, []).append((o.p_local, o.pixel))
        views_out.setdefault(o.cabinet_idx, set()).add(o.camera_idx)
        pts_out[o.cabinet_idx] = pts_out.get(o.cabinet_idx, 0) + 1
    return obs_out, pvcc_out, views_out, pts_out, n_rejected_total, rejected_per_cab


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


# Branch disambiguation thresholds (sidecar internal): a branch is "well
# separated" only when its model-frame normal is meaningfully closer to nominal
# than the other; reproj ratio is the secondary tiebreak.
DISAMBIG_NORMAL_MARGIN_RAD = np.deg2rad(8.0)


def _disambiguate_world_branch(world_branches, nominal_normal):
    """world_branches: list of (R_world_from_cab, t) candidate poses.
    nominal_normal: (3,) expected model-frame surface normal.
    Returns the chosen (R, t), or the string "undecidable" when the two
    branches are equally consistent with nominal (no redundancy to break it)."""
    nn = np.asarray(nominal_normal, dtype=float)
    nn = nn / (np.linalg.norm(nn) + 1e-12)
    angs = []
    for R, _t in world_branches:
        n = R @ np.array([0.0, 0.0, 1.0])
        angs.append(float(np.arccos(np.clip(n @ nn, -1.0, 1.0))))
    order = np.argsort(angs)
    if len(world_branches) == 1:
        return world_branches[0]
    best, second = order[0], order[1]
    if angs[second] - angs[best] < DISAMBIG_NORMAL_MARGIN_RAD:
        return "undecidable"
    return world_branches[best]


def estimate_nonroot_cabinet_init(
    per_view_cab_corners: dict[tuple[int, int], list[tuple[np.ndarray, np.ndarray]]],
    root_idx: int,
    K: np.ndarray,
    *,
    nominal_normals: dict[int, tuple[float, float, float]],
    nominal_centers: dict[int, tuple[float, float, float]],
    min_corners: int = MIN_PNP_CORNERS,
) -> tuple[dict[int, tuple[np.ndarray, np.ndarray]], set[int]]:
    """Non-root cabinet_idx -> (R_world_from_cab, t_mm) via bridge cameras, with
    IPPE two-branch disambiguation against nominal model-frame normals.

    A bridge view sees the root cabinet AND a non-root cabinet (each with
    >= min_corners corners). PnP on the root gives camera_from_root; the
    two-branch IPPE solve on the non-root gives up to two camera_from_nonroot
    poses (the planar convex/concave mirror ambiguity). Each branch composes to
    world_from_nonroot via the root:
        R = Rc0.T @ Rc1 ,  t = Rc0.T @ (tc1 - tc0)
    The branch whose model-frame normal (R @ [0,0,1]) best matches the cabinet's
    nominal arc normal is kept. Multiple bridge views: rotations SVD-averaged,
    translation component-wise median.

    Returns (out, undecidable): `out` maps each bridged cabinet to its chosen
    world pose; `undecidable` is the set of cabinet_idx whose convex/concave
    branch could not be resolved from nominal (no redundant view broke the tie)
    so the caller must hard-stop. A cabinet resolved by at least one view is
    removed from `undecidable`. Cabinets with no bridge view are absent from
    both (caller falls back to nominal).

    Limitation: only direct root<->non-root bridging; no transitive bridging for
    distant cabinets that share no view with the root (large chained-topology
    screens). Current target is the monitor bench (2 panels) + small screens.
    """
    by_view: dict[int, dict[int, list]] = {}
    for (cam_idx, cab_idx), corners in per_view_cab_corners.items():
        by_view.setdefault(cam_idx, {})[cab_idx] = corners

    est_R: dict[int, list] = {}
    est_t: dict[int, list] = {}
    undecidable: set[int] = set()
    for cabs in by_view.values():
        root_corners = cabs.get(root_idx, [])
        if len(root_corners) < min_corners:
            continue
        pose_root = _solve_pnp(root_corners, K)  # root: nominal +z, unambiguous enough
        if pose_root is None:
            continue
        Rc0, tc0 = pose_root
        for cab_idx, corners in cabs.items():
            if cab_idx == root_idx or len(corners) < min_corners:
                continue
            res = _solve_pnp_branches(corners, K)
            if res is None:
                continue
            branches, _mask = res
            # Compose each camera_from_cab branch to world_from_cab via the root:
            #   R_wc = Rc0.T @ Rc1 ; t_wc = Rc0.T @ (tc1 - tc0)
            world_branches = [(Rc0.T @ Rc1, Rc0.T @ (tc1 - tc0)) for Rc1, tc1 in branches]
            chosen = _disambiguate_world_branch(world_branches, nominal_normals[cab_idx])
            if chosen == "undecidable":
                undecidable.add(cab_idx)
                continue
            est_R.setdefault(cab_idx, []).append(chosen[0])
            est_t.setdefault(cab_idx, []).append(chosen[1])

    out: dict[int, tuple] = {}
    for cab_idx, rotations in est_R.items():
        undecidable.discard(cab_idx)  # at least one view resolved it
        t = np.median(np.array(est_t[cab_idx]), axis=0)
        out[cab_idx] = (_avg_rotation(rotations), t)
    return out, undecidable


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
