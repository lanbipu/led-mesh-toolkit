"""Pure SL intrinsics solver (no IPC, no file IO) shared by calibrate-structured-light
and reconstruct-structured-light's --intrinsics auto. Gate failures raise
IntrinsicsRefused(code, msg); callers translate to an ErrorEvent or re-raise."""
from __future__ import annotations

from dataclasses import dataclass

import cv2
import numpy as np

from lmt_vba_sidecar.calibrate import FOCAL_BOUNDS_FRACTION

# Gate constants (mirror calibrate_sl.py:42-60 so behavior is unchanged after extraction).
COVERAGE_MIN_FRAC = 0.20
COPLANAR_RATIO_MIN = 1e-3
POSE_ROT_DIVERSITY_DEG = 5.0
PP_STDDEV_MAX_PX = 3.0
FOCAL_STDDEV_MAX_FRAC = 0.005
MIN_DOTS_PER_POSE = 4


class IntrinsicsRefused(Exception):
    def __init__(self, code: str, message: str):
        super().__init__(message)
        self.code = code
        self.message = message


@dataclass
class IntrinsicsResult:
    K: np.ndarray
    dist: np.ndarray
    rms: float
    focal_stddev_px: tuple[float, float]
    pp_stddev_px: tuple[float, float]
    distortion_model: str          # "radial2" | "full"
    coplanar_ratio: float
    rvecs: list


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
    allpts = np.concatenate([np.asarray(p).reshape(-1, 2) for p in image_points], axis=0)
    w = (allpts[:, 0].max() - allpts[:, 0].min()) / image_size[0]
    h = (allpts[:, 1].max() - allpts[:, 1].min()) / image_size[1]
    return float(min(w, h))


def solve_sl_intrinsics(object_points, image_points, image_size, *, max_rms_px: float) -> IntrinsicsResult:
    """Solve K + distortion from per-pose (object_points, image_points). Raises
    IntrinsicsRefused on any gate. Distortion model is fixed k1,k2 here (Task 1
    is a behavior-preserving extraction); Task 2 makes it adaptive."""
    if len(object_points) < 1:
        raise IntrinsicsRefused("observability_failed", f"no pose has >= {MIN_DOTS_PER_POSE} dots")
    all_obj = np.concatenate(object_points, axis=0)
    ratio = _coplanarity_ratio(all_obj)
    if ratio < COPLANAR_RATIO_MIN and len(object_points) < 3:
        raise IntrinsicsRefused("observability_failed",
                                f"near-coplanar target (ratio={ratio:.2e}) with only {len(object_points)} pose(s)")
    cover = _coverage_frac(image_points, image_size)
    if cover < COVERAGE_MIN_FRAC:
        raise IntrinsicsRefused("observability_failed", f"image coverage {cover:.2f} < {COVERAGE_MIN_FRAC}")

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
        raise IntrinsicsRefused("intrinsics_invalid", f"calibrateCamera failed: {e}")

    if len(rvecs) >= 2 and _max_pairwise_rot_deg(rvecs) < POSE_ROT_DIVERSITY_DEG:
        raise IntrinsicsRefused("observability_failed",
                                f"pose rotation diversity < {POSE_ROT_DIVERSITY_DEG} deg (near-duplicate captures)")
    if not (np.isfinite(K).all() and np.isfinite(dist).all() and np.isfinite(rms)):
        raise IntrinsicsRefused("intrinsics_invalid", f"calibration produced non-finite values (rms={rms})")
    fx, fy, cx, cy = float(K[0, 0]), float(K[1, 1]), float(K[0, 2]), float(K[1, 2])
    f_lo, f_hi = FOCAL_BOUNDS_FRACTION
    if not (f_lo * long_dim < fx < f_hi * long_dim) or not (f_lo * long_dim < fy < f_hi * long_dim):
        raise IntrinsicsRefused("intrinsics_invalid", f"focal ({fx:.1f},{fy:.1f}) outside plausible range")
    if not (0 < cx < image_size[0]) or not (0 < cy < image_size[1]):
        raise IntrinsicsRefused("intrinsics_invalid", f"principal point ({cx:.1f},{cy:.1f}) outside image")
    if rms > max_rms_px:
        raise IntrinsicsRefused("intrinsics_invalid", f"reproj RMS {rms:.2f}px exceeds gate {max_rms_px}px")
    std = np.asarray(std_int).flatten()
    pp_std = (float(std[2]), float(std[3]))
    foc_std = (float(std[0]), float(std[1]))
    if max(pp_std) > PP_STDDEV_MAX_PX:
        raise IntrinsicsRefused("observability_failed", f"principal-point std {pp_std} px > {PP_STDDEV_MAX_PX}")
    if max(foc_std) > FOCAL_STDDEV_MAX_FRAC * fx:
        raise IntrinsicsRefused("observability_failed", f"focal std {foc_std} px > {FOCAL_STDDEV_MAX_FRAC*100:.1f}%")

    return IntrinsicsResult(K=K, dist=dist, rms=float(rms), focal_stddev_px=foc_std,
                            pp_stddev_px=pp_std, distortion_model="radial2",
                            coplanar_ratio=ratio, rvecs=list(rvecs))
