"""VP-QSP layout: marker grid sizing + p_local (+y-up, center-origin) convention.

The +y-up convention is load-bearing: it must match
screen_mapping.charuco_corner_local_mm so VP-QSP and ChArUco feed the shared BA
the same chirality. Feeding a y-down model recovers a mirrored cabinet pose.
"""
from __future__ import annotations

import numpy as np

import pytest

from lmt_vba_sidecar.vpqsp_codec import MAX_LOCAL, VpqspMarkerId, encode_marker
from lmt_vba_sidecar.vpqsp_layout import (
    DEFAULT_MARKER_FILL,
    MAX_MARKERS_PER_CABINET,
    choose_marker_grid,
    local_ids,
    marker_center_px,
    marker_local_mm,
)


def test_choose_marker_grid_square_and_min_markers():
    mx, my, mpx = choose_marker_grid((630, 630))
    assert mx == my  # square cabinet -> square marker grid
    assert mx * my >= 8  # clears reconstruct observability floor
    assert mpx > 0


@pytest.mark.parametrize("res", [(630, 630), (2560, 1440), (3840, 2160), (1280, 640)])
def test_marker_fills_cell_for_high_coverage(res):
    """Markers must fill ~DEFAULT_MARKER_FILL of each cell so the screen is well
    utilised (the operator's complaint was ~80% per-cell coverage). Guards Issue 4:
    bumping the fill maximises screen usage without moving centres (which would
    merge seam-adjacent markers on a seamless wall). The lower bound 0.85 also
    locks in that we did NOT regress below the new 0.9 target (minus rounding)."""
    mx, my, mpx = choose_marker_grid(res)
    cell_w = res[0] / mx
    cell_h = res[1] / my
    fill = mpx / min(cell_w, cell_h)
    assert fill >= 0.85, f"{res}: marker fills only {fill:.0%} of the cell"
    # Cross-check the fill tracks the DEFAULT_MARKER_FILL knob (within rounding).
    assert abs(fill - DEFAULT_MARKER_FILL) < 0.05


def test_choose_marker_grid_aspect_scales_long_side():
    mx, my, _ = choose_marker_grid((1280, 360))  # wide cabinet
    assert mx > my  # more markers along the long (wide) side


@pytest.mark.parametrize("res", [(1920, 360), (3840, 1080), (7680, 1080), (2560, 1440)])
def test_choose_marker_grid_caps_at_local_id_capacity(res):
    # A wide/large cabinet must not produce more markers than the 6-bit local_id
    # can address (MAX_LOCAL+1=64), or encode_marker overflows at generation time.
    mx, my, mpx = choose_marker_grid(res)
    assert mx >= 1 and my >= 1 and mpx > 0
    assert mx * my <= MAX_MARKERS_PER_CABINET == MAX_LOCAL + 1
    # Every local_id this grid yields must encode cleanly (no ValueError).
    for lid in local_ids(mx, my):
        encode_marker(VpqspMarkerId(0, 0, 0, lid))


def test_marker_local_mm_is_y_up_center_origin():
    res = (640, 640)
    pitch = (2.5, 2.5)
    mx, my = 4, 4
    # local_id 0 = top-left marker (marker_row 0, marker_col 0): -x (left), +y (up).
    p0 = marker_local_mm(0, markers_x=mx, markers_y=my, resolution_px=res, pixel_pitch_mm=pitch)
    assert p0[0] < 0 and p0[1] > 0 and p0[2] == 0.0
    # bottom-right marker: +x (right), -y (down).
    p_last = marker_local_mm(mx * my - 1, markers_x=mx, markers_y=my, resolution_px=res, pixel_pitch_mm=pitch)
    assert p_last[0] > 0 and p_last[1] < 0


def test_marker_local_mm_matches_charuco_convention():
    """marker_local_mm must apply the SAME pixel->mm transform as
    screen_mapping.charuco_corner_local_mm: x=(px-W/2)*pitch, y=(H/2-py)*pitch."""
    res = (630, 630)
    pitch = (600.0 / 630, 600.0 / 630)
    mx, my = 5, 5
    for lid in range(mx * my):
        cx, cy = marker_center_px(lid, markers_x=mx, markers_y=my, resolution_px=res)
        expected = np.array(
            [(cx - res[0] / 2) * pitch[0], (res[1] / 2 - cy) * pitch[1], 0.0]
        )
        got = marker_local_mm(lid, markers_x=mx, markers_y=my, resolution_px=res, pixel_pitch_mm=pitch)
        assert np.allclose(got, expected)


def test_marker_grid_is_symmetric_about_center():
    res = (640, 640)
    pitch = (2.5, 2.5)
    mx, my = 4, 4
    xs = [marker_local_mm(l, markers_x=mx, markers_y=my, resolution_px=res, pixel_pitch_mm=pitch)[0]
          for l in range(mx * my)]
    ys = [marker_local_mm(l, markers_x=mx, markers_y=my, resolution_px=res, pixel_pitch_mm=pitch)[1]
          for l in range(mx * my)]
    assert np.isclose(min(xs), -max(xs))
    assert np.isclose(min(ys), -max(ys))
