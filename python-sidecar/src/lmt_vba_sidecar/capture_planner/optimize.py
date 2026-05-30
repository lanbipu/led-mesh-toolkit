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

from lmt_vba_sidecar.capture_planner import gates
from lmt_vba_sidecar.capture_planner.geometry import ScreenGeometry
from lmt_vba_sidecar.capture_planner.visibility import Camera, coverage_report, look_at_camera
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
    counts: dict           # per-(cam_idx, (col,row)) visible-point count for `cameras`


def _score(report, n_cabinets) -> tuple:
    """Lexicographic greedy objective: failing cabinets first, then total view
    deficit (sum of how many covering views each cabinet still lacks to reach
    MIN_VIEWS). The deficit term lets the optimizer make progress on a cabinet
    that needs TWO new views: the first addition lowers the deficit even though
    the cabinet isn't reconstructable yet, so the greedy doesn't dead-stop and
    falsely report a reachable region as unreachable."""
    if report is None:
        return (n_cabinets, gates.MIN_VIEWS * n_cabinets)
    failing = sum(1 for v in report.values() if not v["pass"])
    deficit = sum(max(0, gates.MIN_VIEWS - v["n_views"]) for v in report.values())
    return (failing, deficit)


def optimize(geom: ScreenGeometry, K, image_size, shell: Shell, *, seed_cams=None,
             max_stations=24, n_standoff=2, n_height=3, n_azimuth=5,
             score_kwargs=None) -> OptimizeResult:
    score_kwargs = dict(score_kwargs or {})
    # The seed is part of the station budget: never return more than max_stations.
    cams = list(seed_cams or [])[:max_stations]
    pool = candidate_cameras(geom, K, image_size, shell, n_standoff=n_standoff,
                             n_height=n_height, n_azimuth=n_azimuth)
    n_cab = len(geom.cabinets)

    report = score_screen(geom, cams, **score_kwargs) if cams else None
    cur = _score(report, n_cab)

    # Greedy: each round, add the unused pool candidate that most improves the
    # objective. Selected candidates are removed from the pool so the same pose
    # can't be re-added (duplicate poses share no baseline yet coverage_report
    # would count them as independent views, faking reconstructability).
    while cur[0] > 0 and len(cams) < max_stations and pool:
        best, best_cam, best_report, best_idx = cur, None, report, -1
        for idx, cand in enumerate(pool):
            r = score_screen(geom, cams + [cand], **score_kwargs)
            s = _score(r, n_cab)
            if s < best:
                best, best_cam, best_report, best_idx = s, cand, r, idx
        if best_cam is None:        # no candidate improves the objective -> stop
            break
        cams.append(best_cam)
        pool.pop(best_idx)
        report, cur = best_report, best

    if report is None:
        report = score_screen(geom, cams, **score_kwargs) if cams else {
            (c.col, c.row): {"pass": False, "reconstructable": False,
                             "low_observation": False, "bridged": False,
                             "p95_mm": float("nan"), "median_mm": float("nan"),
                             "n_views": 0, "total_observations": 0}
            for c in geom.cabinets
        }
    # Final per-(cam, cabinet) visibility for the chosen cameras, so callers can
    # derive per-station covered-cabinet lists without re-running coverage.
    inc = score_kwargs.get("incidence_max_deg", 60.0)
    _, counts = coverage_report(geom, cams, incidence_max_deg=inc) if cams else ([], {})
    unreachable = [k for k, v in report.items() if not v["pass"]]
    return OptimizeResult(cams, report, unreachable, counts)
