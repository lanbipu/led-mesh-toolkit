"""Expand a screen's nominal geometry into 3D cabinet centers, surface normals,
and per-cabinet sample points (model frame, millimetres).

The sample grid is the unit of visibility/coverage downstream: each cabinet is
sampled by a `sample_grid` (default 4x4) covering its active face, so coverage
can be judged per point against the observability gate (>=8 obs / >=4 per view)
rather than by a single cabinet-center test.
"""
from __future__ import annotations

from dataclasses import dataclass

import numpy as np

from lmt_vba_sidecar.ipc import CabinetArray
from lmt_vba_sidecar.nominal import (
    _curved_radius,
    _is_curved,
    nominal_cabinet_centers_model_frame,
    nominal_cabinet_normals_model_frame,
)


@dataclass(frozen=True)
class CabinetGeom:
    col: int
    row: int
    center_mm: np.ndarray        # (3,) model frame, mm
    normal: np.ndarray           # (3,) unit surface normal
    sample_points_mm: np.ndarray  # (K, 3) model frame, mm


@dataclass(frozen=True)
class ScreenGeometry:
    cabinets: list[CabinetGeom]
    radius_mm: float | None
    total_width_mm: float
    total_height_mm: float


def _tangent_basis(normal: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Orthonormal (right, up) spanning the cabinet face. World +Y is 'up';
    'right' = up x normal. For a flat (+z) face this is (+x, +y)."""
    up = np.array([0.0, 1.0, 0.0])
    right = np.cross(up, normal)
    right = right / np.linalg.norm(right)
    up_local = np.cross(normal, right)
    return right, up_local


def expand_screen(cab: CabinetArray, shape_prior, sample_grid=(4, 4)) -> ScreenGeometry:
    centers_m = nominal_cabinet_centers_model_frame(cab, shape_prior)
    normals = nominal_cabinet_normals_model_frame(cab, shape_prior)
    cw_mm, ch_mm = cab.cabinet_size_mm
    nx, ny = sample_grid
    us = np.linspace(-1.0, 1.0, nx) * (cw_mm / 2.0)
    vs = np.linspace(-1.0, 1.0, ny) * (ch_mm / 2.0)

    cabinets: list[CabinetGeom] = []
    for (col, row), c_m in centers_m.items():
        center_mm = np.asarray(c_m, float) * 1000.0
        normal = np.asarray(normals[(col, row)], float)
        right, up_local = _tangent_basis(normal)
        pts = [center_mm + u * right + v * up_local for v in vs for u in us]
        cabinets.append(
            CabinetGeom(col, row, center_mm, normal, np.asarray(pts, float))
        )

    cabinets.sort(key=lambda c: (c.row, c.col))
    radius = _curved_radius(shape_prior) if _is_curved(shape_prior) else None
    return ScreenGeometry(
        cabinets=cabinets,
        radius_mm=radius,
        total_width_mm=cab.cols * cw_mm,
        total_height_mm=cab.rows * ch_mm,
    )
