import math
import numpy as np
import pytest

from lmt_vba_sidecar.ipc import (
    CabinetArray, CabinetRect, CodeSpec, SequenceSpec,
    ShapePriorCurved, ShapePriorCurvedBody, StructuredLightDot, StructuredLightMeta,
)
from lmt_vba_sidecar.nominal import (
    nominal_dot_positions_world,
    nominal_cabinet_centers_model_frame,
    nominal_cabinet_normals_model_frame,
)
from lmt_vba_sidecar.sl_geometry import sl_local_mm


def _meta(cabinets, dots, screen_res=(1080, 540)):
    return StructuredLightMeta(
        schema_version=1, screen_id="MAIN", screen_resolution=list(screen_res),
        dot_radius_px=4,
        code=CodeSpec(data_bits=8, total_bits=9),
        sequence=SequenceSpec(n_code_frames=9, hold_ms=100, fps=30),
        cabinets=cabinets, dots=dots,
    )


def test_flat_dot_is_center_plus_local_offset():
    # One flat cabinet (0,0), 500x500mm, 540x540px -> pitch ~0.9259 mm/px.
    cab = CabinetArray(cols=1, rows=1, cabinet_size_mm=[500.0, 500.0])
    rect = CabinetRect(col=0, row=0, input_rect_px=[0, 0, 540, 540], pixel_pitch_mm=[500.0/540, 500.0/540])
    # A dot at the cabinet pixel center (u,v)=(270,270) -> local (0,0,0).
    dot = StructuredLightDot(id=0, u=270.0, v=270.0, cabinet=[0, 0])
    meta = _meta([rect], [dot], screen_res=(540, 540))
    world = nominal_dot_positions_world(meta, cab, "flat")
    center = np.array(nominal_cabinet_centers_model_frame(cab, "flat")[(0, 0)])
    assert np.allclose(world[0], center, atol=1e-9)


def test_flat_offset_dot_matches_sl_local_mm_translation():
    cab = CabinetArray(cols=1, rows=1, cabinet_size_mm=[500.0, 500.0])
    rect = CabinetRect(col=0, row=0, input_rect_px=[0, 0, 540, 540], pixel_pitch_mm=[500.0/540, 500.0/540])
    dot = StructuredLightDot(id=7, u=400.0, v=120.0, cabinet=[0, 0])
    meta = _meta([rect], [dot], screen_res=(540, 540))
    world = nominal_dot_positions_world(meta, cab, "flat")
    center = np.array(nominal_cabinet_centers_model_frame(cab, "flat")[(0, 0)])
    local_m = sl_local_mm((0, 0, 540, 540), 400.0, 120.0, 500.0/540, 500.0/540) / 1000.0
    assert np.allclose(world[7], center + local_m, atol=1e-9)


def test_curved_cabinet_dot_centroid_is_cabinet_center():
    # 3 cols curved; the centroid of a cabinet's dots == its nominal center,
    # and the dot-plane normal == the nominal normal (independent oracles).
    cab = CabinetArray(cols=3, rows=1, cabinet_size_mm=[500.0, 500.0])
    shape = ShapePriorCurved(curved=ShapePriorCurvedBody(radius_mm=4000.0))
    rects = [CabinetRect(col=c, row=0, input_rect_px=[c*540, 0, 540, 540], pixel_pitch_mm=[500.0/540, 500.0/540]) for c in range(3)]
    # 4 symmetric dots around each cabinet center -> centroid == center.
    dots, did = [], 0
    for c in range(3):
        for (u, v) in [(c*540+135, 135), (c*540+405, 135), (c*540+135, 405), (c*540+405, 405)]:
            dots.append(StructuredLightDot(id=did, u=float(u), v=float(v), cabinet=[c, 0])); did += 1
    meta = _meta(rects, dots, screen_res=(1620, 540))
    world = nominal_dot_positions_world(meta, cab, shape)
    centers = nominal_cabinet_centers_model_frame(cab, shape)
    normals = nominal_cabinet_normals_model_frame(cab, shape)
    for c in range(3):
        ids = [d.id for d in dots if d.cabinet == [c, 0]]
        pts = np.array([world[i] for i in ids])
        assert np.allclose(pts.mean(axis=0), np.array(centers[(c, 0)]), atol=1e-6)
        # Plane normal via SVD: smallest singular vector of centered points.
        u_, s_, vt = np.linalg.svd(pts - pts.mean(axis=0))
        n = vt[-1]
        nominal_n = np.array(normals[(c, 0)])
        assert abs(abs(np.dot(n, nominal_n)) - 1.0) < 1e-6  # parallel (sign-free)
