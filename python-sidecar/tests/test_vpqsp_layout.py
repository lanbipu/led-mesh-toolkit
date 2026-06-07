"""VP-QSP layout: marker grid sizing + p_local (+y-up, center-origin) convention.

The +y-up convention is load-bearing: it must match
screen_mapping.charuco_corner_local_mm so VP-QSP and ChArUco feed the shared BA
the same chirality. Feeding a y-down model recovers a mirrored cabinet pose.
"""
from __future__ import annotations

import numpy as np

from lmt_vba_sidecar.vpqsp_layout import (
    choose_marker_grid,
    marker_center_px,
    marker_local_mm,
)


def test_choose_marker_grid_square_and_min_markers():
    mx, my, mpx = choose_marker_grid((630, 630))
    assert mx == my  # square cabinet -> square marker grid
    assert mx * my >= 8  # clears reconstruct observability floor
    assert mpx > 0


def test_choose_marker_grid_aspect_scales_long_side():
    mx, my, _ = choose_marker_grid((1280, 360))  # wide cabinet
    assert mx > my  # more markers along the long (wide) side


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
