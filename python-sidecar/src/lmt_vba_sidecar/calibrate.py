"""Camera intrinsics from a checkerboard image set."""
from __future__ import annotations

import json
import pathlib

import cv2
import numpy as np

from lmt_vba_sidecar.io_utils import write_event
from lmt_vba_sidecar.ipc import (
    BaStats,
    CalibrateInput,
    ErrorEvent,
    ProgressEvent,
    ResultData,
    ResultEvent,
)


def _build_object_points(inner: tuple[int, int], square_mm: float) -> np.ndarray:
    cols, rows = inner
    pts = np.zeros((cols * rows, 3), dtype=np.float32)
    pts[:, :2] = np.mgrid[0:cols, 0:rows].T.reshape(-1, 2) * square_mm
    return pts


def _atomic_write(path: pathlib.Path, content: str) -> None:
    """Write content to path atomically: write to <path>.tmp + rename."""
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(content)
    tmp.replace(path)


# Pose diversity / quality thresholds.
POSE_DIVERSITY_PX_RMS = 5.0  # mean pairwise corner-set RMS must exceed this
MAX_REPROJECTION_RMS_PX = 2.0  # cv2 RMS reprojection error gate
FOCAL_BOUNDS_FRACTION = (0.2, 5.0)  # fx/fy must lie within (frac × image_dim)


def _has_pose_diversity(img_points_list: list) -> bool:
    """True if the detected corner sets vary enough across frames.

    Computes mean pairwise RMS distance between corner arrays. A set of
    nearly-identical frames will have a near-zero value here.
    """
    arrays = [np.asarray(p).reshape(-1, 2) for p in img_points_list]
    if len(arrays) < 2:
        return False
    n = len(arrays)
    rms_total = 0.0
    pairs = 0
    for i in range(n):
        for j in range(i + 1, n):
            if arrays[i].shape != arrays[j].shape:
                continue
            diff = arrays[i] - arrays[j]
            rms_total += float(np.sqrt((diff ** 2).sum(axis=1).mean()))
            pairs += 1
    if pairs == 0:
        return False
    mean_pair_rms = rms_total / pairs
    return mean_pair_rms > POSE_DIVERSITY_PX_RMS


def _validate_calibration_outputs(
    K: np.ndarray, dist: np.ndarray, rms: float, image_size: tuple[int, int],
) -> str | None:
    """Return None if outputs look sane, else a user-facing error message."""
    if not np.isfinite(K).all() or not np.isfinite(dist).all() or not np.isfinite(rms):
        return f"calibration produced non-finite values (rms={rms})"
    fx = float(K[0, 0])
    fy = float(K[1, 1])
    cx = float(K[0, 2])
    cy = float(K[1, 2])
    img_w, img_h = image_size
    f_lo, f_hi = FOCAL_BOUNDS_FRACTION
    long_dim = max(img_w, img_h)
    if not (f_lo * long_dim < fx < f_hi * long_dim):
        return f"focal length fx={fx:.1f} px outside plausible range for {img_w}x{img_h}"
    if not (f_lo * long_dim < fy < f_hi * long_dim):
        return f"focal length fy={fy:.1f} px outside plausible range for {img_w}x{img_h}"
    if not (0 < cx < img_w) or not (0 < cy < img_h):
        return f"principal point ({cx:.1f}, {cy:.1f}) outside image {img_w}x{img_h}"
    if rms > MAX_REPROJECTION_RMS_PX:
        return f"reprojection RMS {rms:.2f}px exceeds quality gate {MAX_REPROJECTION_RMS_PX}px"
    return None


def run_calibrate(cmd: CalibrateInput) -> int:
    inner = (cmd.inner_corners[0], cmd.inner_corners[1])
    obj_template = _build_object_points(inner, cmd.square_size_mm)

    obj_points: list[np.ndarray] = []
    img_points: list[np.ndarray] = []
    image_size: tuple[int, int] | None = None

    for idx, path in enumerate(cmd.checkerboard_images):
        img = cv2.imread(path, cv2.IMREAD_GRAYSCALE)
        if img is None:
            write_event(ErrorEvent(
                event="error", code="image_load_failed",
                message=f"could not read image {path}", fatal=True,
            ))
            return 1
        if image_size is None:
            image_size = (img.shape[1], img.shape[0])
        elif (img.shape[1], img.shape[0]) != image_size:
            write_event(ErrorEvent(
                event="error", code="invalid_input",
                message=f"image {path} dim {img.shape[::-1]} differs from {image_size}",
                fatal=True,
            ))
            return 1
        found, corners = cv2.findChessboardCorners(img, inner, flags=cv2.CALIB_CB_ADAPTIVE_THRESH)
        if not found:
            write_event(ProgressEvent(
                event="progress", stage="detect_charuco",
                percent=(idx + 1) / len(cmd.checkerboard_images),
                message=f"checkerboard not found in {pathlib.Path(path).name}",
            ))
            continue
        criteria = (cv2.TERM_CRITERIA_EPS + cv2.TERM_CRITERIA_MAX_ITER, 30, 1e-3)
        corners_refined = cv2.cornerSubPix(img, corners, (11, 11), (-1, -1), criteria)
        obj_points.append(obj_template)
        img_points.append(corners_refined)
        write_event(ProgressEvent(
            event="progress", stage="detect_charuco",
            percent=(idx + 1) / len(cmd.checkerboard_images),
            message=f"{len(obj_points)}/{idx + 1} usable",
        ))

    if len(obj_points) < 5:
        write_event(ErrorEvent(
            event="error", code="detection_failed",
            message=f"only {len(obj_points)} of {len(cmd.checkerboard_images)} images yielded detectable checkerboards (need ≥ 5)",
            fatal=True,
        ))
        return 1

    # Pose diversity: if every pair of detected corner sets is nearly
    # identical, calibration has no baseline and `cv2.calibrateCamera`
    # would silently return meaningless intrinsics. Reject before solving.
    if not _has_pose_diversity(img_points):
        write_event(ErrorEvent(
            event="error", code="detection_failed",
            message=(
                "checkerboard pose diversity insufficient: all detected views "
                "appear nearly identical. Capture from varied angles/distances."
            ),
            fatal=True,
        ))
        return 1

    write_event(ProgressEvent(event="progress", stage="bundle_adjustment", percent=0.7, message="solving intrinsics"))
    rms, K, dist, _, _ = cv2.calibrateCamera(
        obj_points, img_points, image_size, None, None,
    )

    quality_err = _validate_calibration_outputs(K, dist, rms, image_size)
    if quality_err is not None:
        write_event(ErrorEvent(
            event="error", code="intrinsics_invalid",
            message=quality_err, fatal=True,
        ))
        return 1

    out_path = pathlib.Path(cmd.output_path)
    payload = json.dumps({
        "K": K.tolist(),
        "dist_coeffs": dist.flatten().tolist(),
        "image_size": list(image_size),
        "reproj_error_px": float(rms),
        "frames_used": len(obj_points),
    }, indent=2)
    _atomic_write(out_path, payload)

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
