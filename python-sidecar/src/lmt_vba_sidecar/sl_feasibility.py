"""Feasibility model for structured-light screen reconstruction.

Models the ACTUAL reconstruction path so the gate is honest:
  1. project true 3D screen points into N views (true poses, true K)
  2. add Gaussian centroid noise
  3. ESTIMATE each camera pose with cv2.solvePnP against the nominal model
     (the as-built screen the pipeline assumes), using the CAMERA'S believed K
  4. triangulate with the ESTIMATED poses and believed K
This captures PnP pose error, intrinsic/calibration error, and nominal-deviation
error -- not just centroid noise. The definitive gate is re-confirmed by Phase 3's
full BA, but this is a valid stop/proceed screen before any production code.
"""
from __future__ import annotations

import cv2
import numpy as np

Pose = tuple[np.ndarray, np.ndarray]  # (R world->cam 3x3, t world->cam 3,)


def project_point(K: np.ndarray, R: np.ndarray, t: np.ndarray, X: np.ndarray) -> np.ndarray:
    xc = R @ X + t
    p = K @ xc
    return p[:2] / p[2]


def triangulate_multiview(K: np.ndarray, poses: list[Pose], pts2d: list[np.ndarray]) -> np.ndarray:
    if len(poses) < 2:
        raise ValueError("triangulation needs >= 2 camera poses")
    rows = []
    for (R, t), (x, y) in zip(poses, pts2d):
        P = K @ np.hstack([R, t.reshape(3, 1)])
        rows.append(x * P[2] - P[0])
        rows.append(y * P[2] - P[1])
    _, _, Vt = np.linalg.svd(np.asarray(rows))
    Xh = Vt[-1]
    return Xh[:3] / Xh[3]


def look_at_pose(cam_pos_mm: np.ndarray, target_mm: np.ndarray | None = None,
                 up: np.ndarray | None = None) -> Pose:
    target_mm = np.zeros(3) if target_mm is None else target_mm
    up = np.array([0.0, 1.0, 0.0]) if up is None else up
    z = target_mm - cam_pos_mm
    z = z / np.linalg.norm(z)
    x = np.cross(up, z)
    x = x / np.linalg.norm(x)
    y = np.cross(z, x)
    R = np.stack([x, y, z], axis=0)
    return R, -R @ cam_pos_mm


def solve_pnp_pose(K: np.ndarray, object_pts_mm: np.ndarray, image_pts: np.ndarray) -> Pose:
    """Estimate (R, t) from 3D-2D correspondences. SQPNP handles planar and
    general configurations without an initial guess (cv2 4.11)."""
    obj = np.ascontiguousarray(np.asarray(object_pts_mm, float).reshape(-1, 1, 3))
    img = np.ascontiguousarray(np.asarray(image_pts, float).reshape(-1, 1, 2))
    ok, rvec, tvec = cv2.solvePnP(obj, img, K, None, flags=cv2.SOLVEPNP_SQPNP)
    if not ok:
        raise ValueError("solvePnP failed")
    R, _ = cv2.Rodrigues(rvec)
    return R, tvec.reshape(3)
