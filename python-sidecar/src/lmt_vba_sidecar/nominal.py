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


def _cabinet_normal_model(
    col: int, row: int, cab: CabinetArray, shape_prior: Any,
) -> tuple[float, float, float]:
    """Per-cabinet nominal surface normal (unit) in the model frame.

    Same convention as eval_runner.reconstruct_cabinet_geometry: the normal is
    the rotated local +z, i.e. R_world_from_cab @ [0,0,1]. Flat => +z
    everywhere. Curved => the arc rotation that places this cabinet's center
    via _cabinet_center_model_m (x = R·sin a + W/2, z = R·(1−cos a)) is R_y(a),
    so the normal is R_y(a) @ [0,0,1] = [sin a, 0, cos a]. Left-of-center
    cabinets (a < 0) tilt to −x; right-of-center (a > 0) tilt to +x.
    """
    if shape_prior == "flat":
        return (0.0, 0.0, 1.0)
    if _is_curved(shape_prior):
        cw_mm, _ch_mm = cab.cabinet_size_mm
        radius_mm = _curved_radius(shape_prior)
        total_w_mm = cab.cols * cw_mm
        _validate_curved_radius(radius_mm, total_w_mm / 2.0)
        x_mm = (col + 0.5) * cw_mm
        chord_x_mm = x_mm - total_w_mm / 2.0
        angle = chord_x_mm / radius_mm
        return (math.sin(angle), 0.0, math.cos(angle))
    if _is_folded(shape_prior):
        raise ValueError(
            "shape_prior=folded is not supported in M2 (refinement deferred to M3); "
            "either approximate as flat or use a curved profile"
        )
    raise ValueError(f"unsupported shape_prior: {shape_prior!r}")


def nominal_cabinet_normals_model_frame(
    cab: CabinetArray, shape_prior: Any,
) -> dict[tuple[int, int], tuple[float, float, float]]:
    """(col, row) -> nominal unit surface normal in the model frame.

    Used by reconstruct's IPPE two-branch disambiguation (Part C): each
    cabinet's planar-PnP mirror ambiguity is resolved by picking the branch
    whose model-frame normal best matches this nominal arc orientation.
    """
    normals: dict[tuple[int, int], tuple[float, float, float]] = {}
    absent = set(tuple(c) for c in cab.absent_cells)
    for row in range(cab.rows):
        for col in range(cab.cols):
            if (col, row) in absent:
                continue
            normals[(col, row)] = _cabinet_normal_model(col, row, cab, shape_prior)
    return normals


def nominal_cabinet_centers_model_frame(
    cab: CabinetArray, shape_prior: Any,
) -> dict[tuple[int, int], tuple[float, float, float]]:
    """(col, row) → (x, y, z) cabinet center in model frame, meters.

    Reconstruct uses these as the per-cabinet translation seeds that
    initialise model-constrained BA (the root cabinet fixes the gauge and BA
    refines every other cabinet pose from these seeds). The earlier
    Procrustes alignment of BA centroids to these nominals has been removed.
    """
    centers: dict[tuple[int, int], tuple[float, float, float]] = {}
    absent = set(tuple(c) for c in cab.absent_cells)
    for row in range(cab.rows):
        for col in range(cab.cols):
            if (col, row) in absent:
                continue
            centers[(col, row)] = _cabinet_center_model_m(col, row, cab, shape_prior)
    return centers


def _cabinet_R_y_model(col: int, row: int, cab: CabinetArray, shape_prior: Any) -> "np.ndarray":
    """R_world_from_cabinet for this cabinet (rigid tile). Flat => I; curved => R_y(alpha)
    where alpha is the arc angle of the cabinet center (consistent with
    _cabinet_normal_model: R_y(alpha).[0,0,1] = [sin a,0,cos a])."""
    import numpy as np
    if shape_prior == "flat":
        return np.eye(3)
    if _is_curved(shape_prior):
        cw_mm, _ch = cab.cabinet_size_mm
        radius_mm = _curved_radius(shape_prior)
        total_w_mm = cab.cols * cw_mm
        _validate_curved_radius(radius_mm, total_w_mm / 2.0)
        x_mm = (col + 0.5) * cw_mm
        angle = (x_mm - total_w_mm / 2.0) / radius_mm
        c, s = math.cos(angle), math.sin(angle)
        return np.array([[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]])
    if _is_folded(shape_prior):
        raise ValueError("shape_prior=folded is not supported in M2")
    raise ValueError(f"unsupported shape_prior: {shape_prior!r}")


def nominal_dot_positions_world(meta: Any, cab: CabinetArray, shape_prior: Any) -> "dict[int, np.ndarray]":
    """dot_id -> [x,y,z] (meters) in the model/design frame.

    world_m = center_m(col,row) + R_y(alpha).(sl_local_mm(rect,u,v,pitch)/1000).
    Flat => pure translation. Used by Step-1 SL calibration as the known 3D target.
    Raises ValueError (mapped to invalid_input) on unsupported shape or a dot whose
    cabinet is absent / not in meta.cabinets.
    """
    import numpy as np
    from lmt_vba_sidecar.sl_geometry import sl_local_mm

    centers = nominal_cabinet_centers_model_frame(cab, shape_prior)  # present cells only
    rect_by_cr = {(c.col, c.row): tuple(int(v) for v in c.input_rect_px) for c in meta.cabinets}
    pitch_by_cr = {(c.col, c.row): (float(c.pixel_pitch_mm[0]), float(c.pixel_pitch_mm[1])) for c in meta.cabinets}
    R_by_cr = {cr: _cabinet_R_y_model(cr[0], cr[1], cab, shape_prior) for cr in centers.keys()}

    out: dict[int, np.ndarray] = {}
    for d in meta.dots:
        cr = (int(d.cabinet[0]), int(d.cabinet[1]))
        if cr not in centers:
            raise ValueError(f"dot {d.id} references absent/unknown cabinet {cr}")
        if cr not in rect_by_cr:
            raise ValueError(f"dot {d.id} cabinet {cr} not in sl_meta.cabinets")
        local_m = sl_local_mm(rect_by_cr[cr], float(d.u), float(d.v), pitch_by_cr[cr][0], pitch_by_cr[cr][1]) / 1000.0
        out[int(d.id)] = np.asarray(centers[cr]) + R_by_cr[cr] @ local_m
    return out
