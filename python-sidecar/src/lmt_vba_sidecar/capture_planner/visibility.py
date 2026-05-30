"""Per-sample-point visibility and observability-gate-aligned coverage.

Visibility is judged PER POINT (cheirality, in-frame, incidence) — never by a
single cabinet-center test. Coverage then aggregates point visibility to the
real reconstruction gate (see gates.py): a camera 'covers' a cabinet only if it
sees >= MIN_PNP_CORNERS of its sample points (so that view could seed a PnP
pose); a cabinet is 'reconstructable' only with >= MIN_VIEWS covering cameras
and >= MIN_POINTS_PER_CABINET total observations. This is deliberately
conservative vs reconstruct's bare gate (which counts >=1-obs views).
"""
from __future__ import annotations

from dataclasses import dataclass

import numpy as np

from lmt_vba_sidecar.sl_feasibility import look_at_pose
from lmt_vba_sidecar.capture_planner import gates
from lmt_vba_sidecar.capture_planner.geometry import ScreenGeometry


@dataclass(frozen=True)
class Camera:
    K: np.ndarray          # (3,3)
    R: np.ndarray          # (3,3) world->cam
    t: np.ndarray          # (3,) world->cam
    image_size: tuple      # (W, H)


def intrinsics_from_fov(image_size, hfov_deg=None, vfov_deg=None) -> np.ndarray:
    """Build a pinhole K from FOV + sensor resolution. Centered principal point,
    square pixels, zero skew. Exactly one of hfov_deg / vfov_deg is required."""
    w, h = image_size
    if (hfov_deg is None) == (vfov_deg is None):
        raise ValueError("pass exactly one of hfov_deg / vfov_deg")
    if hfov_deg is not None:
        f = (w / 2.0) / np.tan(np.deg2rad(hfov_deg) / 2.0)
    else:
        f = (h / 2.0) / np.tan(np.deg2rad(vfov_deg) / 2.0)
    return np.array([[f, 0.0, w / 2.0], [0.0, f, h / 2.0], [0.0, 0.0, 1.0]], float)


def look_at_camera(K, cam_pos_mm, target_mm, image_size, up=None) -> Camera:
    R, t = look_at_pose(np.asarray(cam_pos_mm, float), np.asarray(target_mm, float), up)
    return Camera(np.asarray(K, float), R, t, tuple(image_size))


def point_visible(cam: Camera, p_mm, normal, *, margin_frac=0.05,
                  incidence_max_deg=60.0) -> bool:
    p = np.asarray(p_mm, float)
    p_cam = cam.R @ p + cam.t
    if p_cam[2] <= 0.0:                                   # (a) cheirality
        return False
    uv = cam.K @ p_cam
    u, v = uv[0] / uv[2], uv[1] / uv[2]
    w, h = cam.image_size
    mx, my = margin_frac * w, margin_frac * h
    if not (mx <= u <= w - mx and my <= v <= h - my):     # (b) in-frame
        return False
    cam_center = -cam.R.T @ cam.t                          # (c) incidence
    to_cam = cam_center - p
    cos_inc = float(np.dot(np.asarray(normal, float), to_cam) / np.linalg.norm(to_cam))
    if cos_inc <= 0.0:                                     # back-facing
        return False
    return bool(np.degrees(np.arccos(np.clip(cos_inc, -1.0, 1.0))) <= incidence_max_deg)
