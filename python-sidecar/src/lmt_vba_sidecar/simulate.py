"""Geometric simulator (Level 0A). Builds true cabinet/camera poses and
noisy (cam, cabinet, local_mm, pixel) observations. Validates BA math only
-- NOT a substitute for real capture (no LED bloom/moire/rolling shutter).

Camera pitch must satisfy |pitch| < 90 deg; the look-at basis gimbal-locks
(divide-by-zero) at +/-90 deg. Phase 0 range is +/-20 deg, which is safe."""
from __future__ import annotations
from dataclasses import dataclass
import cv2
import numpy as np
from lmt_vba_sidecar.ipc import SimulateInput
from lmt_vba_sidecar.model_constrained_ba import Observation


@dataclass
class Scene:
    K: np.ndarray
    true_camera_poses: list[tuple[np.ndarray, np.ndarray]]
    true_cabinet_poses: dict[int, tuple[np.ndarray, np.ndarray]]
    cabinet_corners_local: dict[int, np.ndarray]  # idx -> (M,3) mm
    observations: list[Observation]
    n_cameras: int
    n_cabinets: int


def _board_corners_local(w_mm: float, h_mm: float, nx: int = 8, ny: int = 8) -> np.ndarray:
    """Active-surface center as origin; nx*ny inner-corner grid (mimics ChArUco)."""
    xs = (np.arange(nx) - (nx - 1) / 2) / (nx - 1) * w_mm
    ys = (np.arange(ny) - (ny - 1) / 2) / (ny - 1) * h_mm
    gx, gy = np.meshgrid(xs, ys)
    return np.stack([gx.ravel(), gy.ravel(), np.zeros(gx.size)], axis=1)


def build_scene(inp: SimulateInput) -> Scene:
    rng = np.random.default_rng(inp.seed)
    K = np.array(inp.intrinsics.K, float)
    cab = inp.scene.cabinet_array
    cw, ch = cab.cabinet_size_mm
    n_cab = cab.cols * cab.rows
    pitch_err = inp.noise.pixel_pitch_error_frac

    # Build cabinet poses on a 2D col x row grid. Cabinet j -> (col, row) in
    # row-major order; position t = [col*cw, row*ch, 0]. Yaw accumulates per
    # column (ang * col) so each column fans out around the Y-axis. With rows=1
    # this reduces to col==j (a single horizontal strip). absent_cells ignored.
    cabinet_poses: dict[int, tuple[np.ndarray, np.ndarray]] = {}
    corners_local: dict[int, np.ndarray] = {}
    ang = np.deg2rad(inp.scene.inter_board_angle_deg)
    for j in range(n_cab):
        col = j % cab.cols
        row = j // cab.cols
        R, _ = cv2.Rodrigues(np.array([0.0, ang * col, 0.0]))
        t = np.array([col * cw, row * ch, 0.0])
        cabinet_poses[j] = (R, t)
        # Apply pixel pitch error as uniform scale on the corner grid
        scale = 1.0 + pitch_err
        corners_local[j] = _board_corners_local(cw * scale, ch * scale)

    # Aim cameras at the centroid of all cabinet positions
    center = np.mean([t for _, t in cabinet_poses.values()], axis=0)
    cams: list[tuple[np.ndarray, np.ndarray]] = []
    for _ in range(inp.cameras.n_views):
        dist = rng.uniform(*inp.cameras.distance_mm_range)
        yaw = np.deg2rad(rng.uniform(*inp.cameras.yaw_deg_range))
        pitch = np.deg2rad(rng.uniform(*inp.cameras.pitch_deg_range))
        # Spherical offset from center; cameras look inward
        cam_pos = center + dist * np.array([
            np.sin(yaw) * np.cos(pitch),
            np.sin(pitch),
            -np.cos(yaw) * np.cos(pitch),
        ])
        fwd = center - cam_pos
        fwd /= np.linalg.norm(fwd)
        up = np.array([0.0, 1.0, 0.0])
        right = np.cross(up, fwd)
        right /= np.linalg.norm(right)
        up2 = np.cross(fwd, right)
        R = np.stack([right, up2, fwd])  # world-to-camera rotation
        t = -R @ cam_pos
        cams.append((R, t))

    # Generate observations: project each corner through each camera
    obs: list[Observation] = []
    for ci, (Rc, tc) in enumerate(cams):
        for j in range(n_cab):
            Rb, tb = cabinet_poses[j]
            for p in corners_local[j]:
                # Visibility dropout
                if rng.random() > inp.noise.visibility_frac:
                    continue
                xw = Rb @ p + tb
                xc = Rc @ xw + tc
                if xc[2] <= 0:
                    # TODO(phase1): clip to image bounds; real detector only sees in-frame corners
                    continue
                px = (K @ xc)[:2] / (K @ xc)[2]
                # Gaussian pixel noise
                if inp.noise.pixel_sigma > 0:
                    px = px + rng.normal(0, inp.noise.pixel_sigma, 2)
                # Gross outlier injection
                if inp.noise.outlier_frac > 0 and rng.random() < inp.noise.outlier_frac:
                    px = px + rng.normal(0, 50, 2)
                obs.append(Observation(
                    camera_idx=ci,
                    cabinet_idx=j,
                    p_local=p.copy(),
                    pixel=px,
                ))

    return Scene(
        K=K,
        true_camera_poses=cams,
        true_cabinet_poses=cabinet_poses,
        cabinet_corners_local=corners_local,
        observations=obs,
        n_cameras=len(cams),
        n_cabinets=n_cab,
    )
