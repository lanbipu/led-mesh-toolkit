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


@dataclass
class OptimizeResult:
    cameras: list          # final list[Camera]
    report: dict           # score_screen output for the final set
    unreachable: list      # [(col,row), ...] cabinets that never pass


def _n_failing(report) -> int:
    return sum(1 for v in report.values() if not v["pass"])


def optimize(geom: ScreenGeometry, K, image_size, shell: Shell, *, seed_cams=None,
             max_stations=24, n_standoff=2, n_height=3, n_azimuth=5,
             score_kwargs=None) -> OptimizeResult:
    score_kwargs = dict(score_kwargs or {})
    cams = list(seed_cams or [])
    pool = candidate_cameras(geom, K, image_size, shell, n_standoff=n_standoff,
                             n_height=n_height, n_azimuth=n_azimuth)

    report = score_screen(geom, cams, **score_kwargs) if cams else None
    cur_fail = _n_failing(report) if report is not None else len(geom.cabinets)

    while cur_fail > 0 and len(cams) < max_stations:
        best_fail, best_cam, best_report = cur_fail, None, report
        for cand in pool:
            r = score_screen(geom, cams + [cand], **score_kwargs)
            f = _n_failing(r)
            if f < best_fail:
                best_fail, best_cam, best_report = f, cand, r
        if best_cam is None:        # no candidate improves coverage -> stop
            break
        cams.append(best_cam)
        report, cur_fail = best_report, best_fail

    if report is None:
        report = score_screen(geom, cams, **score_kwargs) if cams else {
            (c.col, c.row): {"pass": False, "reconstructable": False,
                             "low_observation": False, "bridged": False,
                             "p95_mm": float("nan"), "median_mm": float("nan"),
                             "n_views": 0, "total_observations": 0}
            for c in geom.cabinets
        }
    unreachable = [k for k, v in report.items() if not v["pass"]]
    return OptimizeResult(cams, report, unreachable)
