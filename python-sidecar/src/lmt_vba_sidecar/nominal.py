"""Compute nominal cabinet center positions in the model coordinate frame.

For frame_strategy="nominal_anchoring", these positions are the Procrustes
target. The reconstruct pipeline aggregates BA output per cabinet (centroid
of all detected markers in that cabinet) and aligns those cabinet centroids
to the nominals here. Aligning per-marker would force every marker in a
cabinet to share one target position, which is degenerate for any column-
or row-only grid (1×N or N×1) and discards in-cabinet geometry.

Curved shape priors deflect cabinet centers off the XY plane via a constant-
radius arc. Folded screens are not supported in M2 — they fail fast rather
than silently producing flat coordinates that look valid but encode the
wrong model frame.
"""
from __future__ import annotations

import math
from typing import Any

from lmt_vba_sidecar.ipc import (
    CabinetArray,
    ShapePriorCurved,
    ShapePriorFolded,
)


CURVED_RADIUS_MIN_RATIO = 0.6  # radius must be ≥ this × screen-half-width


def _validate_curved_radius(radius_mm: float, screen_half_width_mm: float) -> None:
    if not math.isfinite(radius_mm):
        raise ValueError(f"curved.radius_mm must be finite, got {radius_mm}")
    if radius_mm <= 0:
        raise ValueError(f"curved.radius_mm must be positive, got {radius_mm}")
    # Half-cylinder geometry needs radius > screen half-width or the arc
    # angle exceeds 90° and chord_x / radius starts to alias.
    min_radius = CURVED_RADIUS_MIN_RATIO * screen_half_width_mm
    if radius_mm < min_radius:
        raise ValueError(
            f"curved.radius_mm={radius_mm} is too small for screen "
            f"half-width {screen_half_width_mm} (need ≥ {min_radius:.1f})"
        )


def _curved_radius(shape_prior: Any) -> float:
    if isinstance(shape_prior, ShapePriorCurved):
        return shape_prior.curved.radius_mm
    return shape_prior["curved"]["radius_mm"]


def _is_curved(shape_prior: Any) -> bool:
    return isinstance(shape_prior, ShapePriorCurved) or (
        isinstance(shape_prior, dict) and "curved" in shape_prior
    )


def _is_folded(shape_prior: Any) -> bool:
    return isinstance(shape_prior, ShapePriorFolded) or (
        isinstance(shape_prior, dict) and "folded" in shape_prior
    )


def _cabinet_center_model_m(
    col: int, row: int, cab: CabinetArray, shape_prior: Any,
) -> tuple[float, float, float]:
    cw_mm, ch_mm = cab.cabinet_size_mm
    x_mm = (col + 0.5) * cw_mm
    y_mm = (row + 0.5) * ch_mm
    z_mm = 0.0

    if shape_prior == "flat":
        pass
    elif _is_curved(shape_prior):
        radius_mm = _curved_radius(shape_prior)
        total_w_mm = cab.cols * cw_mm
        _validate_curved_radius(radius_mm, total_w_mm / 2.0)
        chord_x_mm = x_mm - total_w_mm / 2.0
        angle = chord_x_mm / radius_mm
        x_mm = radius_mm * math.sin(angle) + total_w_mm / 2.0
        z_mm = radius_mm * (1.0 - math.cos(angle))
    elif _is_folded(shape_prior):
        raise ValueError(
            "shape_prior=folded is not supported in M2 (refinement deferred to M3); "
            "either approximate as flat or use a curved profile"
        )
    else:
        raise ValueError(f"unsupported shape_prior: {shape_prior!r}")

    return (x_mm / 1000.0, y_mm / 1000.0, z_mm / 1000.0)


def nominal_cabinet_centers_model_frame(
    cab: CabinetArray, shape_prior: Any,
) -> dict[tuple[int, int], tuple[float, float, float]]:
    """(col, row) → (x, y, z) cabinet center in model frame, meters.

    Reconstruct aggregates BA points per cabinet using `pattern_meta` to
    know which ArUco IDs belong to which cabinet, then aligns those
    aggregated centroids to these nominals via Procrustes.
    """
    centers: dict[tuple[int, int], tuple[float, float, float]] = {}
    absent = set(tuple(c) for c in cab.absent_cells)
    for row in range(cab.rows):
        for col in range(cab.cols):
            if (col, row) in absent:
                continue
            centers[(col, row)] = _cabinet_center_model_m(col, row, cab, shape_prior)
    return centers
