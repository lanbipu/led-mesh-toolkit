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


from lmt_vba_sidecar.capture_planner.geometry import CabinetGeom


@dataclass(frozen=True)
class CabinetCoverage:
    col: int
    row: int
    covering_cams: tuple        # cam indices with >= MIN_PNP_CORNERS visible points
    total_observations: int     # sum of visible points across covering cams
    reconstructable: bool       # >= MIN_VIEWS covering AND >= MIN_POINTS_PER_CABINET obs
    low_observation: bool       # reconstructable AND covering < QUALITY_MIN_VIEWS


def vis_count(cam: Camera, cabg: CabinetGeom, *, margin_frac=0.05,
              incidence_max_deg=60.0) -> int:
    return sum(
        1
        for p in cabg.sample_points_mm
        if point_visible(cam, p, cabg.normal, margin_frac=margin_frac,
                         incidence_max_deg=incidence_max_deg)
    )


def coverage_report(geom: ScreenGeometry, cams: list[Camera], *, margin_frac=0.05,
                    incidence_max_deg=60.0):
    """Return (per_cabinet: list[CabinetCoverage], counts: dict[(ci,(col,row))->int]).
    `counts` is the per-camera per-cabinet visible-point count, reused downstream
    (bridging, scoring)."""
    counts: dict[tuple[int, tuple[int, int]], int] = {}
    for ci, cam in enumerate(cams):
        for cabg in geom.cabinets:
            n = vis_count(cam, cabg, margin_frac=margin_frac,
                          incidence_max_deg=incidence_max_deg)
            if n:
                counts[(ci, (cabg.col, cabg.row))] = n

    per_cabinet: list[CabinetCoverage] = []
    for cabg in geom.cabinets:
        key = (cabg.col, cabg.row)
        covering = tuple(
            ci for ci in range(len(cams))
            if counts.get((ci, key), 0) >= gates.MIN_PNP_CORNERS
        )
        total_obs = sum(counts[(ci, key)] for ci in covering)
        reconstructable = (
            len(covering) >= gates.MIN_VIEWS
            and total_obs >= gates.MIN_POINTS_PER_CABINET
        )
        low_obs = reconstructable and len(covering) < gates.QUALITY_MIN_VIEWS
        per_cabinet.append(
            CabinetCoverage(cabg.col, cabg.row, covering, total_obs,
                            reconstructable, low_obs)
        )
    return per_cabinet, counts
