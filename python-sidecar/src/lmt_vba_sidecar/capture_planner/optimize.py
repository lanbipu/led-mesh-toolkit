"""Greedy capture-plan optimizer.

Warm-starts from the recipe seed, then repeatedly adds the shell candidate that
removes the most failing cabinets, until every cabinet passes or the station
budget / candidate pool is exhausted. Whatever still fails is reported as
`unreachable_regions` — honest 'no placement here meets target', not silence.
Add-only in M2 (prune/swap deferred).
"""
from __future__ import annotations

from dataclasses import dataclass

import numpy as np

from lmt_vba_sidecar.capture_planner.geometry import ScreenGeometry
from lmt_vba_sidecar.capture_planner.visibility import Camera, look_at_camera
from lmt_vba_sidecar.capture_planner.seed import Shell
from lmt_vba_sidecar.capture_planner.scoring import score_screen


def candidate_cameras(geom: ScreenGeometry, K, image_size, shell: Shell, *,
                      n_standoff=2, n_height=3, n_azimuth=5) -> list[Camera]:
    cx = geom.total_width_mm / 2.0
    cy = geom.total_height_mm / 2.0
    center = np.array([cx, cy, 0.0])
    standoffs = np.linspace(shell.standoff_min_mm, shell.standoff_max_mm, n_standoff)
    heights = np.linspace(shell.height_min_mm, shell.height_max_mm, n_height)
    # azimuth spread chosen so extremes stay in front of the wall (|a| < 80deg)
    azimuths = np.deg2rad(np.linspace(-70.0, 70.0, n_azimuth))
    cams: list[Camera] = []
    for d in standoffs:
        for a in azimuths:
            for hy in heights:
                pos = np.array([cx + d * np.sin(a), hy, d * np.cos(a)])
                cams.append(look_at_camera(K, pos, center, image_size))
    return cams
