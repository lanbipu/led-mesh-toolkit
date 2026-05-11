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
