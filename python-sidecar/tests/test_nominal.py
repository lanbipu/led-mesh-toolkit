"""Nominal cabinet center position tests per shape_prior."""
from __future__ import annotations

import pytest

from lmt_vba_sidecar.ipc import CabinetArray
from lmt_vba_sidecar.nominal import nominal_cabinet_centers_model_frame


def test_flat_2x1_grid_in_meters() -> None:
    cab = CabinetArray(cols=2, rows=1, cabinet_size_mm=[500.0, 500.0])
    centers = nominal_cabinet_centers_model_frame(cab, "flat")
    assert centers[(0, 0)] == pytest.approx((0.25, 0.25, 0.0), abs=1e-9)
    assert centers[(1, 0)] == pytest.approx((0.75, 0.25, 0.0), abs=1e-9)


def test_flat_skips_absent_cells() -> None:
    cab = CabinetArray(
        cols=2, rows=2, cabinet_size_mm=[500.0, 500.0],
        absent_cells=[(1, 1)],
    )
    centers = nominal_cabinet_centers_model_frame(cab, "flat")
    assert (1, 1) not in centers
    assert len(centers) == 3


def test_curved_lifts_z_off_plane_at_edges() -> None:
    cab = CabinetArray(cols=4, rows=1, cabinet_size_mm=[500.0, 500.0])
    centers = nominal_cabinet_centers_model_frame(
        cab, {"curved": {"radius_mm": 5000.0}},
    )
    zs = [centers[(c, 0)][2] for c in range(4)]
    # Outer columns should be lifted more than inner ones.
    assert max(zs) - min(zs) > 0.001


def test_unknown_shape_prior_rejected() -> None:
    cab = CabinetArray(cols=1, rows=1, cabinet_size_mm=[500.0, 500.0])
    with pytest.raises(ValueError, match="unsupported shape_prior"):
        nominal_cabinet_centers_model_frame(cab, {"bogus": {}})


def test_folded_fails_fast_not_silently_flat() -> None:
    cab = CabinetArray(cols=4, rows=1, cabinet_size_mm=[500.0, 500.0])
    with pytest.raises(ValueError, match="folded.*not supported"):
        nominal_cabinet_centers_model_frame(cab, {"folded": {"fold_seam_columns": [2]}})


def test_negative_curved_radius_rejected() -> None:
    cab = CabinetArray(cols=4, rows=1, cabinet_size_mm=[500.0, 500.0])
    with pytest.raises(ValueError, match="positive"):
        nominal_cabinet_centers_model_frame(cab, {"curved": {"radius_mm": -100.0}})


def test_zero_curved_radius_rejected() -> None:
    cab = CabinetArray(cols=4, rows=1, cabinet_size_mm=[500.0, 500.0])
    with pytest.raises(ValueError, match="positive"):
        nominal_cabinet_centers_model_frame(cab, {"curved": {"radius_mm": 0.0}})


def test_nonfinite_curved_radius_rejected() -> None:
    cab = CabinetArray(cols=4, rows=1, cabinet_size_mm=[500.0, 500.0])
    with pytest.raises(ValueError, match="finite"):
        nominal_cabinet_centers_model_frame(cab, {"curved": {"radius_mm": float("inf")}})


def test_curved_radius_too_small_for_screen_rejected() -> None:
    """If radius < ~half screen width, the arc angle exceeds 90° → unstable."""
    cab = CabinetArray(cols=20, rows=1, cabinet_size_mm=[500.0, 500.0])  # 10m wide
    with pytest.raises(ValueError, match="too small"):
        nominal_cabinet_centers_model_frame(cab, {"curved": {"radius_mm": 1000.0}})


def test_1x1_grid_returns_single_point() -> None:
    """1×1 grids yield a single nominal — caller (reconstruct) handles this
    by enforcing the 3-anchor minimum in Procrustes; nominal returns what
    the geometry says."""
    cab = CabinetArray(cols=1, rows=1, cabinet_size_mm=[500.0, 500.0])
    centers = nominal_cabinet_centers_model_frame(cab, "flat")
    assert len(centers) == 1
    assert (0, 0) in centers


import numpy as np

from lmt_vba_sidecar.ipc import CabinetArray
from lmt_vba_sidecar.nominal import nominal_cabinet_normals_model_frame


def _cab(cols, rows):
    return CabinetArray.model_validate(
        {"cols": cols, "rows": rows, "absent_cells": [], "cabinet_size_mm": [500, 500]}
    )


def test_flat_normals_all_face_plus_z():
    normals = nominal_cabinet_normals_model_frame(_cab(3, 1), "flat")
    assert set(normals.keys()) == {(0, 0), (1, 0), (2, 0)}
    for n in normals.values():
        np.testing.assert_allclose(n, [0.0, 0.0, 1.0], atol=1e-9)


def test_curved_normals_match_arc_tangent_and_are_unit():
    # Wide arc: cols=5, 500mm each => total 2500mm; radius generous so angle<90.
    cab = _cab(5, 1)
    shape = {"curved": {"radius_mm": 3000.0}}
    normals = nominal_cabinet_normals_model_frame(cab, shape)
    # Each normal is a unit vector with zero y-component (arc bends in x-z).
    for (col, _row), n in normals.items():
        n = np.asarray(n)
        assert abs(np.linalg.norm(n) - 1.0) < 1e-9
        assert abs(n[1]) < 1e-12
    # Left-of-center cabinet tilts so its normal has NEGATIVE x; right has POSITIVE.
    assert normals[(0, 0)][0] < 0.0
    assert normals[(4, 0)][0] > 0.0
    # Center-most cabinet (col 2, near arc center) faces nearly +z.
    assert normals[(2, 0)][2] > 0.99


def test_curved_normal_convention_matches_center_geometry():
    # The normal equals R_world_from_cab @ [0,0,1] for the arc rotation R_y(a)
    # that also places the cabinet center (x = R·sin a + W/2, z = R·(1−cos a)):
    # for angle = chord_x / radius, normal = R_y(a) @ [0,0,1] = [sin a, 0, cos a].
    import math
    cab = _cab(5, 1)
    radius = 3000.0
    normals = nominal_cabinet_normals_model_frame(cab, {"curved": {"radius_mm": radius}})
    cw = 500.0
    total_w = 5 * cw
    for col in range(5):
        x_mm = (col + 0.5) * cw
        chord_x = x_mm - total_w / 2.0
        a = chord_x / radius
        np.testing.assert_allclose(
            normals[(col, 0)], [math.sin(a), 0.0, math.cos(a)], atol=1e-9
        )
