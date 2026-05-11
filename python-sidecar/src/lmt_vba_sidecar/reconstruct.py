"""Reconstruct top-level: detection → BA → Procrustes → IR output.

Pipeline:
  1. Detect ChArUco markers across all images (detect.py).
  2. Undistort observation pixel coordinates with the supplied intrinsics.
  3. Initialize 3D points from per-cabinet nominal centers (one shared
     init per ArUco ID inside that cabinet) and cameras at the world
     origin pointing forward.
  4. Run bundle_adjust on undistorted observations.
  5. Aggregate BA output per cabinet (centroid of all marker positions
     inside that cabinet).
  6. Procrustes-align the cabinet centroids to either:
       A) nominal_anchoring: every cabinet's nominal center, OR
       C) three_points: 3 user-supplied frame_anchors (transformed from
          world frame to model frame via the project's CoordinateFrame).
  7. Emit one MeasuredPoint per cabinet (named MAIN_V<col>_R<row>) with
     position in model frame, covariance averaged from BA per-marker
     covariances, camera_count = unique cameras observing any marker.
"""
from __future__ import annotations

import collections

import cv2
import numpy as np

from lmt_vba_sidecar.ba import bundle_adjust
from lmt_vba_sidecar.detect import detect_charuco_observations
from lmt_vba_sidecar.io_utils import write_event
from lmt_vba_sidecar.ipc import (
    BaStats,
    ErrorEvent,
    FrameAnchor,
    MeasuredPoint,
    PointSource,
    PointSourceVisualBa,
    ProgressEvent,
    ReconstructInput,
    ResultData,
    ResultEvent,
    Uncertainty,
    WarningEvent,
)
from lmt_vba_sidecar.nominal import nominal_cabinet_centers_model_frame
from lmt_vba_sidecar.procrustes import procrustes_rigid


MIN_OBSERVATIONS = 30  # below this, abort with detection_failed

# Procrustes alignment quality gates. Both in meters.
#
# A mode aligns BA cabinet centroids to *nominal* cabinet positions: real
# screens deviate from the prior, so allow a generous tolerance.
# C mode aligns to 3 user-supplied anchors (typically total-station measured):
# anchor residuals should be tight; loose fit indicates inconsistent input.
PROCRUSTES_RMS_THRESHOLD_M = {
    "nominal_anchoring": 0.050,  # 50mm
    "three_points": 0.020,       # 20mm
}


def _undistort_obs(
    pix: np.ndarray, K: np.ndarray, dist: np.ndarray,
) -> np.ndarray:
    """Map a single (x, y) pixel through cv2.undistortPoints to its
    pinhole-equivalent pixel coordinate. Returns same shape (2,)."""
    pts = pix.reshape(1, 1, 2).astype(np.float32)
    undistorted_norm = cv2.undistortPoints(pts, K, dist)  # normalized cam
    norm = undistorted_norm.reshape(2)
    # Re-project through K to pixel coords.
    out = K @ np.array([norm[0], norm[1], 1.0])
    return out[:2] / out[2]


def _aruco_id_to_cabinet(pattern_meta) -> dict[int, tuple[int, int]]:
    """ArUco ID → (col, row) of its parent cabinet."""
    out: dict[int, tuple[int, int]] = {}
    for entry in pattern_meta.cabinets:
        for aid in range(entry.aruco_id_start, entry.aruco_id_end + 1):
            out[aid] = (entry.col, entry.row)
    return out


def _aggregate_observations(
    raw: dict[str, list[dict]],
    K: np.ndarray,
    dist: np.ndarray,
    aid_to_cabinet: dict[int, tuple[int, int]],
) -> tuple[list[str], list[int], list[tuple[int, int, np.ndarray]], dict[int, set[int]]]:
    """Returns (image_paths, point_index_to_aruco_id, observations,
    per_aruco_camera_set).

    Each "point" in the BA sense corresponds to one ArUco marker corner,
    represented here by the marker's center pixel. We undistort the center
    via the supplied intrinsics so BA only sees pinhole projections.
    """
    image_paths = list(raw.keys())
    aid_to_idx: dict[int, int] = {}
    obs: list[tuple[int, int, np.ndarray]] = []
    per_aruco_cams: dict[int, set[int]] = collections.defaultdict(set)
    for img_idx, path in enumerate(image_paths):
        for marker in raw[path]:
            aid = marker["aruco_id"]
            if aid not in aid_to_cabinet:
                # Marker outside any known cabinet; ignore (could be from a
                # stray ChArUco image or a misconfigured pattern_meta).
                continue
            if aid not in aid_to_idx:
                aid_to_idx[aid] = len(aid_to_idx)
            pt_idx = aid_to_idx[aid]
            corners = np.array(marker["corners_px"], dtype=float)
            center = corners.mean(axis=0)
            center_undistorted = _undistort_obs(center, K, dist)
            obs.append((img_idx, pt_idx, center_undistorted))
            per_aruco_cams[aid].add(img_idx)
    aruco_ids = [aid for aid, _ in sorted(aid_to_idx.items(), key=lambda kv: kv[1])]
    return image_paths, aruco_ids, obs, per_aruco_cams


def _initialize_state(
    *, n_cams: int, aruco_ids: list[int],
    aid_to_cabinet: dict[int, tuple[int, int]],
    nominal_cabinet: dict[tuple[int, int], tuple[float, float, float]],
):
    initial_points = np.zeros((len(aruco_ids), 3), dtype=float)
    for i, aid in enumerate(aruco_ids):
        cab_xy = aid_to_cabinet[aid]
        if cab_xy in nominal_cabinet:
            initial_points[i] = nominal_cabinet[cab_xy]
        else:
            initial_points[i] = (0.0, 0.0, 5.0)
    initial_cams: list[tuple[np.ndarray, np.ndarray]] = []
    for i in range(n_cams):
        R = np.eye(3)
        t = np.array([0.0, 0.0, 5.0 + 0.05 * i])
        initial_cams.append((R, t))
    return initial_points, initial_cams


def _aggregate_ba_per_cabinet(
    aruco_ids: list[int], ba_points: np.ndarray,
    aid_to_cabinet: dict[int, tuple[int, int]],
    per_aruco_cams: dict[int, set[int]],
    point_covariances: dict[int, np.ndarray],
) -> dict[tuple[int, int], dict]:
    """Per-cabinet aggregation of BA output.

    Returns:
      (col, row) -> {
        "position": (3,) np.ndarray,         # mean of marker centers
        "camera_count": int,                  # unique cameras across markers
        "covariance": (3,3) np.ndarray | None # mean of per-marker covariances
      }
    """
    by_cabinet: dict[tuple[int, int], dict] = {}
    grouped: dict[tuple[int, int], list[int]] = collections.defaultdict(list)
    for idx, aid in enumerate(aruco_ids):
        grouped[aid_to_cabinet[aid]].append(idx)
    for cab_xy, indices in grouped.items():
        positions = ba_points[indices]
        camera_set: set[int] = set()
        for idx in indices:
            camera_set.update(per_aruco_cams[aruco_ids[idx]])
        covs = [point_covariances[i] for i in indices if i in point_covariances]
        cov_mean = (
            np.mean(np.stack(covs), axis=0) if covs else None
        )
        by_cabinet[cab_xy] = {
            "position": positions.mean(axis=0),
            "camera_count": len(camera_set),
            "covariance": cov_mean,
        }
    return by_cabinet


def _world_to_model(coord_frame, world_pos: np.ndarray) -> np.ndarray:
    """Apply CoordinateFrame.world_to_model: model = R^T @ (world - origin).

    `coord_frame.basis` is a list of *column* vectors per the lmt_core IR
    docstring (`basis[i]` is the i-th column of R). numpy reads list-of-lists
    as rows, so plain `np.array(basis)` yields R^T. We use `np.column_stack`
    to build R, then take its transpose to apply the world→model rotation.

    Identity-frame inputs hide this bug because R == R^T == I; the regression
    test in test_reconstruct uses a non-identity basis to catch it.
    """
    R = np.column_stack([
        np.asarray(coord_frame.basis[0], dtype=float),
        np.asarray(coord_frame.basis[1], dtype=float),
        np.asarray(coord_frame.basis[2], dtype=float),
    ])
    o = np.array(coord_frame.origin_world, dtype=float)
    return R.T @ (world_pos - o)


def run_reconstruct(cmd: ReconstructInput) -> int:
    write_event(ProgressEvent(event="progress", stage="load", percent=0.0, message="starting"))

    # Detection
    raw = detect_charuco_observations(image_paths=cmd.images)
    detected = sum(len(v) for v in raw.values())
    if detected < MIN_OBSERVATIONS:
        write_event(ErrorEvent(
            event="error", code="detection_failed",
            message=f"only {detected} marker observations across {len(cmd.images)} images (need ≥ {MIN_OBSERVATIONS})",
            fatal=True,
        ))
        return 1

    K = np.array(cmd.intrinsics.K, dtype=float)
    dist = np.array(cmd.intrinsics.dist_coeffs, dtype=float)
    aid_to_cabinet = _aruco_id_to_cabinet(cmd.pattern_meta)
    image_paths, aruco_ids, observations, per_aruco_cams = _aggregate_observations(
        raw, K, dist, aid_to_cabinet,
    )
    if len(observations) < MIN_OBSERVATIONS:
        write_event(ErrorEvent(
            event="error", code="detection_failed",
            message=f"after filtering to known ArUco IDs, only {len(observations)} observations remained",
            fatal=True,
        ))
        return 1

    write_event(ProgressEvent(
        event="progress", stage="detect_charuco", percent=0.3,
        message=f"{len(observations)} observations / {len(aruco_ids)} unique markers / {len(image_paths)} images",
    ))

    try:
        nominal_cabinet = nominal_cabinet_centers_model_frame(
            cmd.project.cabinet_array, cmd.project.shape_prior,
        )
    except ValueError as e:
        write_event(ErrorEvent(event="error", code="invalid_input", message=str(e), fatal=True))
        return 1

    initial_points, initial_cams = _initialize_state(
        n_cams=len(image_paths), aruco_ids=aruco_ids,
        aid_to_cabinet=aid_to_cabinet, nominal_cabinet=nominal_cabinet,
    )

    write_event(ProgressEvent(event="progress", stage="bundle_adjustment", percent=0.4, message="starting BA"))
    # bundle_adjust requires undistorted observations (we already undistorted
    # in _aggregate_observations, so dist_coeffs=zeros here).
    result = bundle_adjust(
        K=K, dist_coeffs=np.zeros(5),
        initial_points=initial_points, initial_cam_poses=initial_cams,
        observations=observations,
    )
    if not result.converged:
        write_event(ErrorEvent(
            event="error", code="ba_diverged",
            message=f"BA did not converge (rms={result.rms_reprojection_px:.2f}px after {result.iterations} iters)",
            fatal=True,
        ))
        return 1

    by_cabinet = _aggregate_ba_per_cabinet(
        aruco_ids, result.points, aid_to_cabinet, per_aruco_cams, result.point_covariances,
    )

    write_event(ProgressEvent(event="progress", stage="procrustes_align", percent=0.85, message="aligning"))
    try:
        if cmd.project.frame_strategy == "nominal_anchoring":
            src, dst = _select_anchors_a(by_cabinet, nominal_cabinet)
            anchor_aid_set: set[int] = set()
        else:
            src, dst, anchor_aid_set = _select_anchors_c(
                by_cabinet, aid_to_cabinet, cmd.project.frame_anchors or [],
                cmd.project.coordinate_frame,
            )
        R_align, t_align, align_rms_m = procrustes_rigid(src, dst)
    except ValueError as e:
        write_event(ErrorEvent(event="error", code="procrustes_failed", message=str(e), fatal=True))
        return 1

    rms_threshold = PROCRUSTES_RMS_THRESHOLD_M[cmd.project.frame_strategy]
    if align_rms_m > rms_threshold:
        write_event(ErrorEvent(
            event="error", code="procrustes_failed",
            message=(
                f"Procrustes alignment residual {align_rms_m * 1000:.1f}mm "
                f"exceeds {cmd.project.frame_strategy} threshold "
                f"{rms_threshold * 1000:.0f}mm — anchors / nominal model inconsistent"
            ),
            fatal=True,
        ))
        return 1

    measured_points: list[MeasuredPoint] = []
    for cab_xy, agg in by_cabinet.items():
        aligned_pos = (R_align @ agg["position"]) + t_align
        cov_world: np.ndarray | None = agg["covariance"]
        if cov_world is not None and np.isfinite(cov_world).all():
            cov_aligned = R_align @ cov_world @ R_align.T
            uncertainty = Uncertainty(covariance=cov_aligned.tolist())
        else:
            write_event(WarningEvent(
                event="warning", code="missing_covariance",
                message=f"cabinet V{cab_xy[0]:03d}_R{cab_xy[1]:03d} has no usable BA covariance; falling back to isotropic 5mm",
                cabinet=f"MAIN_V{cab_xy[0]:03d}_R{cab_xy[1]:03d}",
            ))
            uncertainty = Uncertainty(isotropic=0.005)
        measured_points.append(MeasuredPoint(
            name=f"MAIN_V{cab_xy[0]:03d}_R{cab_xy[1]:03d}",
            position=aligned_pos.tolist(),
            uncertainty=uncertainty,
            source=PointSource(visual_ba=PointSourceVisualBa(camera_count=int(agg["camera_count"]))),
        ))

    write_event(ProgressEvent(event="progress", stage="output", percent=1.0, message="emitting result"))
    write_event(ResultEvent(
        event="result",
        data=ResultData(
            measured_points=measured_points,
            ba_stats=BaStats(
                rms_reprojection_px=result.rms_reprojection_px,
                iterations=result.iterations,
                converged=True,
            ),
            frame_strategy_used=cmd.project.frame_strategy,
            procrustes_align_rms_m=align_rms_m,
        ),
    ))
    return 0


def _select_anchors_a(
    by_cabinet: dict[tuple[int, int], dict],
    nominal_cabinet: dict[tuple[int, int], tuple[float, float, float]],
) -> tuple[np.ndarray, np.ndarray]:
    """A mode: every observed cabinet's centroid against its nominal center."""
    src_list, dst_list = [], []
    for cab_xy, agg in by_cabinet.items():
        if cab_xy in nominal_cabinet:
            src_list.append(agg["position"])
            dst_list.append(nominal_cabinet[cab_xy])
    if len(src_list) < 3:
        raise ValueError(
            f"only {len(src_list)} observed cabinet centroids matched nominal "
            f"positions; need ≥ 3 for Procrustes"
        )
    return np.asarray(src_list, dtype=float), np.asarray(dst_list, dtype=float)


def _select_anchors_c(
    by_cabinet: dict[tuple[int, int], dict],
    aid_to_cabinet: dict[int, tuple[int, int]],
    frame_anchors: list[FrameAnchor],
    coordinate_frame,
) -> tuple[np.ndarray, np.ndarray, set[int]]:
    """C mode: 3 user-supplied anchors. Each anchor names a specific
    ArUco ID, and we use that marker's parent cabinet centroid as the
    source. The destination is the anchor's world position transformed
    into model frame via the project CoordinateFrame.
    """
    if len(frame_anchors) != 3:
        raise ValueError(
            f"three_points strategy requires exactly 3 anchors, got {len(frame_anchors)}"
        )
    distinct_aids = {a.aruco_id for a in frame_anchors}
    if len(distinct_aids) != 3:
        raise ValueError("frame_anchors must reference 3 distinct aruco_ids")

    src_list, dst_list = [], []
    used_aids: set[int] = set()
    cabinets_used: set[tuple[int, int]] = set()
    for anchor in frame_anchors:
        if anchor.aruco_id not in aid_to_cabinet:
            raise ValueError(
                f"frame_anchor aruco_id={anchor.aruco_id} not in pattern_meta"
            )
        cab_xy = aid_to_cabinet[anchor.aruco_id]
        declared = (anchor.cabinet_col, anchor.cabinet_row)
        if declared != cab_xy:
            # Defense-in-depth: total-station CSV rows are hand-entered and
            # easy to mismatch. Refuse rather than silently use the ID-derived
            # cabinet, which would anchor a different point than the report says.
            raise ValueError(
                f"frame_anchor aruco_id={anchor.aruco_id} maps to cabinet "
                f"V{cab_xy[0]:03d}_R{cab_xy[1]:03d} but the anchor declares "
                f"V{declared[0]:03d}_R{declared[1]:03d}"
            )
        if cab_xy not in by_cabinet:
            raise ValueError(
                f"frame_anchor aruco_id={anchor.aruco_id} (cabinet "
                f"V{cab_xy[0]:03d}_R{cab_xy[1]:03d}) was not observed in any image"
            )
        cabinets_used.add(cab_xy)
        src_list.append(by_cabinet[cab_xy]["position"])
        world = np.array(anchor.position_world, dtype=float)
        dst_list.append(_world_to_model(coordinate_frame, world))
        used_aids.add(anchor.aruco_id)

    if len(cabinets_used) != 3:
        # All anchors collapse to ≤ 2 distinct cabinet centroids → rank-deficient.
        raise ValueError(
            "three_points anchors must lie in 3 distinct cabinets; got "
            f"{len(cabinets_used)} unique cabinet(s) ({sorted(cabinets_used)})"
        )
    return (
        np.asarray(src_list, dtype=float),
        np.asarray(dst_list, dtype=float),
        used_aids,
    )
