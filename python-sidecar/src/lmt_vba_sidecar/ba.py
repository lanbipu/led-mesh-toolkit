"""Bundle adjustment via scipy.optimize.least_squares.

Joint optimization over 3D points + camera extrinsics. Intrinsics are held
fixed (we trust the calibration step). Observations MUST be undistorted
upstream (e.g. via cv2.undistortPoints) — `bundle_adjust` rejects non-zero
`dist_coeffs` rather than silently fitting lens distortion into pose error.

Per-point covariance is opt-in via `compute_covariance=True` and bounded by
`MAX_COVARIANCE_PARAMS` (defends against dense pseudo-inverse blowup on
large reconstructions). When skipped or capped, the result has an empty
covariance dict; the caller logs a warning + falls back to isotropic.
"""
from __future__ import annotations

from dataclasses import dataclass

import cv2
import numpy as np
from scipy.optimize import least_squares
from scipy.sparse import lil_matrix


@dataclass
class BAResult:
    points: np.ndarray  # (P, 3)
    cam_poses: list[tuple[np.ndarray, np.ndarray]]
    rms_reprojection_px: float
    iterations: int
    converged: bool
    point_covariances: dict[int, np.ndarray]  # per-point 3×3 covariance


def _params_from_state(
    points: np.ndarray, cams: list[tuple[np.ndarray, np.ndarray]],
) -> np.ndarray:
    parts = []
    for R, t in cams:
        rvec, _ = cv2.Rodrigues(R)
        parts.append(np.concatenate([rvec.flatten(), t]))
    parts.append(points.flatten())
    return np.concatenate(parts)


def _state_from_params(
    params: np.ndarray, n_cams: int, n_points: int,
) -> tuple[list[tuple[np.ndarray, np.ndarray]], np.ndarray]:
    cams: list[tuple[np.ndarray, np.ndarray]] = []
    for i in range(n_cams):
        rvec = params[i * 6:i * 6 + 3]
        t = params[i * 6 + 3:i * 6 + 6]
        R, _ = cv2.Rodrigues(rvec)
        cams.append((R, t))
    points = params[n_cams * 6:].reshape(n_points, 3)
    return cams, points


def _residuals(
    params: np.ndarray, n_cams: int, n_points: int,
    K: np.ndarray, observations: list[tuple[int, int, np.ndarray]],
) -> np.ndarray:
    cams, points = _state_from_params(params, n_cams, n_points)
    res = np.zeros(len(observations) * 2)
    for k, (cam_i, pt_i, pix) in enumerate(observations):
        R, t = cams[cam_i]
        cam_pt = R @ points[pt_i] + t
        proj = K @ cam_pt
        proj = proj[:2] / proj[2]
        res[k * 2: k * 2 + 2] = proj - pix
    return res


def _build_sparsity(
    n_cams: int, n_points: int,
    observations: list[tuple[int, int, np.ndarray]],
) -> lil_matrix:
    m = len(observations) * 2
    n = n_cams * 6 + n_points * 3
    A = lil_matrix((m, n), dtype=int)
    for k, (cam_i, pt_i, _) in enumerate(observations):
        A[k * 2:k * 2 + 2, cam_i * 6:cam_i * 6 + 6] = 1
        A[k * 2:k * 2 + 2, n_cams * 6 + pt_i * 3:n_cams * 6 + pt_i * 3 + 3] = 1
    return A


MAX_COVARIANCE_PARAMS = 2400  # cap dense pinv at ~2400 params (~46MB matrix)


def bundle_adjust(
    *,
    K: np.ndarray, dist_coeffs: np.ndarray,
    initial_points: np.ndarray,
    initial_cam_poses: list[tuple[np.ndarray, np.ndarray]],
    observations: list[tuple[int, int, np.ndarray]],
    max_iters: int = 100,
    compute_covariance: bool = True,
) -> BAResult:
    if dist_coeffs is not None and np.asarray(dist_coeffs).size > 0:
        if not np.allclose(np.asarray(dist_coeffs), 0.0):
            raise ValueError(
                "bundle_adjust requires undistorted observations: "
                "non-zero dist_coeffs supplied. Run cv2.undistortPoints "
                "on observations upstream and pass dist_coeffs=zeros."
            )

    n_cams = len(initial_cam_poses)
    n_points = initial_points.shape[0]
    x0 = _params_from_state(initial_points, initial_cam_poses)
    sparsity = _build_sparsity(n_cams, n_points, observations)

    sol = least_squares(
        _residuals, x0,
        jac_sparsity=sparsity,
        args=(n_cams, n_points, K, observations),
        method="trf",
        max_nfev=max_iters,
        verbose=0,
    )

    cams, points = _state_from_params(sol.x, n_cams, n_points)
    rms = float(np.sqrt((sol.fun ** 2).reshape(-1, 2).sum(axis=1).mean()))

    point_covariances: dict[int, np.ndarray] = {}
    n_params = n_cams * 6 + n_points * 3
    if compute_covariance and sol.jac is not None and n_params <= MAX_COVARIANCE_PARAMS:
        try:
            J = sol.jac.toarray() if hasattr(sol.jac, "toarray") else np.asarray(sol.jac)
            sigma2 = float((sol.fun ** 2).sum() / max(1, J.shape[0] - J.shape[1]))
            cov = np.linalg.pinv(J.T @ J) * sigma2
            for pt_i in range(n_points):
                a = n_cams * 6 + pt_i * 3
                point_covariances[pt_i] = cov[a:a + 3, a:a + 3]
        except np.linalg.LinAlgError:
            pass

    return BAResult(
        points=points,
        cam_poses=cams,
        rms_reprojection_px=rms,
        iterations=int(sol.nfev),
        converged=bool(sol.success),
        point_covariances=point_covariances,
    )
