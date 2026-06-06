# Capture Guidance Planner — M2 Implementation Plan (seed + optimizer)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the M1 engine into an actual planner: a deterministic recipe **seed** layout (FOV-fill standoff, front fan, top/bottom stations) plus a greedy **optimizer** that warm-starts from the seed, adds shell candidates until every cabinet passes, and honestly reports `unreachable_regions` when it can't.

**Architecture:** Two new modules in `lmt_vba_sidecar.capture_planner`: `seed.py` (recipe → `list[Camera]`) and `optimize.py` (candidate shell pool + add-only greedy driven by M1's `score_screen`). Both consume the M1 `Camera`/`ScreenGeometry` types and the `coverage_report`/`score_screen` scorers unchanged.

**Tech Stack:** Python 3.12, numpy, M1 capture_planner package.

**Scope note:** spec milestone **M2** (`docs/superpowers/specs/2026-05-30-camera-capture-guidance-design.md` §4④⑤, §8). The optimizer is **add-only** in M2 (warm-start from seed, add candidates). Prune/swap of redundant cameras (§4⑤ "删/换") is deferred to a small M2.1 follow-up — add-only already converges and reports unreachable; pruning is an efficiency refinement, not correctness. Curved walls use the same center-aimed fan seed as flat in M2; arc-following seed refinement rides with M4's curved work.

**Run environment:** worktree `python-sidecar/`, isolated venv: `./.venv/bin/python -m pytest ...`.

---

## File Structure

| File | Responsibility |
| --- | --- |
| `python-sidecar/src/lmt_vba_sidecar/capture_planner/seed.py` | `Shell`, `fov_fill_standoff`, `seed_cameras` (recipe layout). |
| `python-sidecar/src/lmt_vba_sidecar/capture_planner/optimize.py` | `candidate_cameras` (shell pool), `optimize` (greedy + unreachable). |
| `python-sidecar/tests/test_capture_planner_seed.py` | Seed standoff/structure + coverage integration tests. |
| `python-sidecar/tests/test_capture_planner_optimize.py` | Greedy convergence + unreachable tests. |

---

## Task 1: Reachable shell + FOV-fill standoff + seed (`seed.py`)

**Files:**
- Create: `python-sidecar/src/lmt_vba_sidecar/capture_planner/seed.py`
- Test: `python-sidecar/tests/test_capture_planner_seed.py`

`Shell` carries the reachable standoff/height ranges (the v1 physical constraint). `fov_fill_standoff` solves for the distance that fits the wall in frame with margin (projected `fx*W_mm/standoff = fill*W_px`). `seed_cameras` places a horizontal front fan at mid height plus one top and one bottom station.

- [ ] **Step 1: Write the failing test**

Create `python-sidecar/tests/test_capture_planner_seed.py`:

```python
import numpy as np

from lmt_vba_sidecar.ipc import CabinetArray
from lmt_vba_sidecar.capture_planner.geometry import expand_screen
from lmt_vba_sidecar.capture_planner.visibility import intrinsics_from_fov, coverage_report
from lmt_vba_sidecar.capture_planner.seed import Shell, fov_fill_standoff, seed_cameras


def _wall(cols, rows):
    cab = CabinetArray(cols=cols, rows=rows, cabinet_size_mm=[500.0, 500.0], absent_cells=[])
    return expand_screen(cab, "flat", sample_grid=(4, 4))


def test_fov_fill_standoff_fits_width_with_margin():
    K = intrinsics_from_fov((1920, 1080), hfov_deg=60.0)
    # a 3 m wide wall, fill 0.8: projected width should be ~0.8*1920 px
    standoff = fov_fill_standoff(K, (1920, 1080), 3000.0, 1000.0, fill=0.8)
    proj_w = K[0, 0] * 3000.0 / standoff
    assert np.isclose(proj_w, 0.8 * 1920, rtol=1e-6)


def test_fov_fill_standoff_clamps_into_shell():
    K = intrinsics_from_fov((1920, 1080), hfov_deg=60.0)
    shell = Shell(standoff_min_mm=2000.0, standoff_max_mm=2500.0,
                  height_min_mm=300.0, height_max_mm=2500.0)
    # raw fit for a tiny wall would be < 2000 -> clamp up to standoff_min
    standoff = seed_cameras(_wall(1, 1), K, (1920, 1080), shell)[0].standoff_used_mm
    assert 2000.0 <= standoff <= 2500.0


def test_seed_has_fan_plus_top_and_bottom_at_shell_heights():
    K = intrinsics_from_fov((1920, 1080), hfov_deg=60.0)
    shell = Shell(2000.0, 8000.0, 300.0, 2600.0)
    geom = _wall(3, 2)
    cams = seed_cameras(geom, K, (1920, 1080), shell, n_fan=5)
    assert len(cams) == 5 + 2                       # fan + top + bottom
    ys = sorted(c.position_mm[1] for c in cams)
    assert np.isclose(ys[0], 300.0)                 # bottom station at height_min
    assert np.isclose(ys[-1], 2600.0)               # top station at height_max
    cy = geom.total_height_mm / 2.0
    fan_ys = [c.position_mm[1] for c in cams if abs(c.position_mm[1] - cy) < 1e-6]
    assert len(fan_ys) == 5                          # fan all at mid height


def test_seed_makes_small_flat_wall_mostly_reconstructable():
    K = intrinsics_from_fov((1920, 1080), hfov_deg=60.0)
    shell = Shell(2500.0, 8000.0, 300.0, 2600.0)
    geom = _wall(3, 2)
    cams = [c.camera for c in seed_cameras(geom, K, (1920, 1080), shell, n_fan=5)]
    per_cab, _ = coverage_report(geom, cams)
    n_ok = sum(1 for c in per_cab if c.reconstructable)
    assert n_ok >= 5                                 # >=5 of 6 cabinets reconstructable
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd python-sidecar && ./.venv/bin/python -m pytest tests/test_capture_planner_seed.py -q`
Expected: FAIL with `ModuleNotFoundError: ... capture_planner.seed`.

- [ ] **Step 3: Write minimal implementation**

Create `python-sidecar/src/lmt_vba_sidecar/capture_planner/seed.py`:

```python
"""Recipe seed layout: a deterministic, human-followable starting set of camera
stations. FOV-fill sets the standoff; a horizontal front fan covers the body;
one top and one bottom station target the edge rows (where residual is worst).
The optimizer (optimize.py) warm-starts from this and patches the rest.
"""
from __future__ import annotations

from dataclasses import dataclass

import numpy as np

from lmt_vba_sidecar.capture_planner.geometry import ScreenGeometry
from lmt_vba_sidecar.capture_planner.visibility import Camera, look_at_camera


@dataclass(frozen=True)
class Shell:
    standoff_min_mm: float
    standoff_max_mm: float
    height_min_mm: float
    height_max_mm: float


@dataclass(frozen=True)
class SeedStation:
    camera: Camera
    position_mm: np.ndarray
    standoff_used_mm: float
    role: str            # "fan" | "top" | "bottom"


def fov_fill_standoff(K, image_size, screen_w_mm, screen_h_mm, fill=0.8) -> float:
    """Distance at which the wall fills `fill` of the frame (tighter of w/h)."""
    w, h = image_size
    standoff_w = K[0, 0] * screen_w_mm / (fill * w)
    standoff_h = K[1, 1] * screen_h_mm / (fill * h)
    return max(standoff_w, standoff_h)


def _clamp(x, lo, hi):
    return max(lo, min(hi, x))


def seed_cameras(geom: ScreenGeometry, K, image_size, shell: Shell, *, n_fan=5,
                 fan_span_deg=40.0, fill=0.8) -> list[SeedStation]:
    cx = geom.total_width_mm / 2.0
    cy = geom.total_height_mm / 2.0
    standoff = _clamp(
        fov_fill_standoff(K, image_size, geom.total_width_mm, geom.total_height_mm, fill),
        shell.standoff_min_mm, shell.standoff_max_mm,
    )
    center = np.array([cx, cy, 0.0])

    stations: list[SeedStation] = []
    # horizontal front fan at mid height, on an arc of radius `standoff`
    angles = np.deg2rad(np.linspace(-fan_span_deg / 2, fan_span_deg / 2, n_fan))
    for a in angles:
        pos = center + np.array([standoff * np.sin(a), 0.0, standoff * np.cos(a)])
        stations.append(SeedStation(look_at_camera(K, pos, center, image_size),
                                    pos, standoff, "fan"))

    # top / bottom stations aimed at the edge-row centers
    top_target = np.array([cx, geom.total_height_mm, 0.0])
    bot_target = np.array([cx, 0.0, 0.0])
    top_pos = np.array([cx, shell.height_max_mm, standoff])
    bot_pos = np.array([cx, shell.height_min_mm, standoff])
    stations.append(SeedStation(look_at_camera(K, top_pos, top_target, image_size),
                                top_pos, standoff, "top"))
    stations.append(SeedStation(look_at_camera(K, bot_pos, bot_target, image_size),
                                bot_pos, standoff, "bottom"))
    return stations
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd python-sidecar && ./.venv/bin/python -m pytest tests/test_capture_planner_seed.py -v`
Expected: PASS (4 passed). If `test_seed_makes_small_flat_wall_mostly_reconstructable` is short of 5/6, widen `fan_span_deg` or raise standoff clamp in the test's shell — investigate before relaxing the assertion (the seed should cover a 3x2 wall).

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/capture_planner/seed.py \
        python-sidecar/tests/test_capture_planner_seed.py
git commit -m "feat(capture-planner): FOV-fill standoff + recipe seed layout (fan + top/bottom)"
```

---

## Task 2: Shell candidate pool (`optimize.py`)

**Files:**
- Create: `python-sidecar/src/lmt_vba_sidecar/capture_planner/optimize.py`
- Test: `python-sidecar/tests/test_capture_planner_optimize.py`

`candidate_cameras` samples the reachable shell (standoff × height × azimuth), each aimed at the wall center — the discrete menu the greedy optimizer chooses from.

- [ ] **Step 1: Write the failing test**

Create `python-sidecar/tests/test_capture_planner_optimize.py`:

```python
import numpy as np

from lmt_vba_sidecar.ipc import CabinetArray
from lmt_vba_sidecar.capture_planner.geometry import expand_screen
from lmt_vba_sidecar.capture_planner.visibility import intrinsics_from_fov
from lmt_vba_sidecar.capture_planner.seed import Shell
from lmt_vba_sidecar.capture_planner.optimize import candidate_cameras


def _wall(cols, rows):
    cab = CabinetArray(cols=cols, rows=rows, cabinet_size_mm=[500.0, 500.0], absent_cells=[])
    return expand_screen(cab, "flat", sample_grid=(4, 4))


def test_candidates_lie_within_the_shell():
    K = intrinsics_from_fov((1920, 1080), hfov_deg=60.0)
    shell = Shell(2000.0, 6000.0, 400.0, 2400.0)
    geom = _wall(2, 2)
    cams = candidate_cameras(geom, K, (1920, 1080), shell,
                             n_standoff=2, n_height=3, n_azimuth=5)
    assert len(cams) == 2 * 3 * 5
    cx = geom.total_width_mm / 2.0
    for cam in cams:
        pos = -cam.R.T @ cam.t            # camera center in world
        assert 400.0 - 1e-6 <= pos[1] <= 2400.0 + 1e-6      # height in shell
        standoff = np.linalg.norm([pos[0] - cx, pos[2]])    # radial dist in x-z
        assert 2000.0 - 1.0 <= standoff <= 6000.0 + 1.0     # standoff in shell
        assert pos[2] > 0                                    # in front of the wall
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd python-sidecar && ./.venv/bin/python -m pytest tests/test_capture_planner_optimize.py -q`
Expected: FAIL with `ModuleNotFoundError: ... capture_planner.optimize`.

- [ ] **Step 3: Write minimal implementation**

Create `python-sidecar/src/lmt_vba_sidecar/capture_planner/optimize.py`:

```python
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
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd python-sidecar && ./.venv/bin/python -m pytest tests/test_capture_planner_optimize.py -v`
Expected: PASS (1 passed).

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/capture_planner/optimize.py \
        python-sidecar/tests/test_capture_planner_optimize.py
git commit -m "feat(capture-planner): reachable-shell candidate camera pool"
```

---

## Task 3: Greedy optimize + unreachable report (`optimize.py`)

**Files:**
- Modify: `python-sidecar/src/lmt_vba_sidecar/capture_planner/optimize.py`
- Test: `python-sidecar/tests/test_capture_planner_optimize.py` (append)

- [ ] **Step 1: Write the failing tests (append)**

Append to `python-sidecar/tests/test_capture_planner_optimize.py`:

```python
from lmt_vba_sidecar.capture_planner.seed import seed_cameras
from lmt_vba_sidecar.capture_planner.optimize import optimize


def _score_kwargs():
    return dict(pixel_sigma=0.2, nominal_deviation_mm=0.5, trials=6,
               seed=0, target_p95_residual_mm=4.0)


def test_optimize_covers_a_reachable_flat_wall():
    K = intrinsics_from_fov((1920, 1080), hfov_deg=60.0)
    shell = Shell(2000.0, 4000.0, 400.0, 2200.0)
    geom = _wall(2, 2)
    seed = [s.camera for s in seed_cameras(geom, K, (1920, 1080), shell, n_fan=5)]
    result = optimize(geom, K, (1920, 1080), shell, seed_cams=seed,
                      max_stations=16, n_standoff=2, n_height=3, n_azimuth=5,
                      score_kwargs=_score_kwargs())
    assert result.unreachable == []
    assert all(v["pass"] for v in result.report.values())
    assert len(result.cameras) >= len(seed)        # warm-started, add-only


def test_optimize_reports_unreachable_when_shell_too_tight():
    K = intrinsics_from_fov((1920, 1080), hfov_deg=60.0)
    # a degenerate shell collapsed to a single near-frontal pencil: no two views
    # can ever form a baseline -> nothing reconstructable -> all unreachable.
    shell = Shell(3000.0, 3000.0, 1249.0, 1251.0)
    geom = _wall(2, 2)
    result = optimize(geom, K, (1920, 1080), shell, seed_cams=[],
                      max_stations=4, n_standoff=1, n_height=1, n_azimuth=1,
                      score_kwargs=_score_kwargs())
    assert len(result.unreachable) > 0
    assert not all(v["pass"] for v in result.report.values())
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd python-sidecar && ./.venv/bin/python -m pytest tests/test_capture_planner_optimize.py -k optimize -q`
Expected: FAIL with `ImportError: cannot import name 'optimize'`.

- [ ] **Step 3: Write minimal implementation (append to `optimize.py`)**

Append to `python-sidecar/src/lmt_vba_sidecar/capture_planner/optimize.py`:

```python
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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd python-sidecar && ./.venv/bin/python -m pytest tests/test_capture_planner_optimize.py -v`
Expected: PASS (3 passed). The greedy + Monte-Carlo is the slow path; if `test_optimize_covers_a_reachable_flat_wall` is slow, it is acceptable (seconds) — do not lower `trials` below 5 or pass/fail flickers.

- [ ] **Step 5: Run full capture-planner suite + sidecar regression**

Run: `cd python-sidecar && ./.venv/bin/python -m pytest tests/ -q`
Expected: PASS — all M1 + M2 + prior sidecar tests, 0 failures.

- [ ] **Step 6: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/capture_planner/optimize.py \
        python-sidecar/tests/test_capture_planner_optimize.py
git commit -m "feat(capture-planner): greedy add-only optimizer with unreachable reporting"
```

---

## Self-Review (against spec §4④⑤, §8 M2)

- **Spec coverage:** §4④ FOV-fill standoff + fan + top/bottom → Task 1. §4⑤ candidate shell → Task 2; greedy warm-started from seed + `unreachable_regions` → Task 3. Prune/swap (§4⑤ "删/换") explicitly deferred to M2.1 with rationale. Curved arc-following seed deferred to M4 (center-aimed fan used meanwhile).
- **Type consistency:** `Shell`, `SeedStation`, `OptimizeResult` defined once; `seed_cameras` returns `SeedStation` (with `.camera`, `.position_mm`, `.standoff_used_mm`); optimizer consumes raw `Camera` lists; `score_screen`/`coverage_report` reused from M1 unchanged.
- **Placeholders:** none — runnable code + exact commands throughout. Two tests carry an explicit "investigate before relaxing" note so a failing assertion triggers debugging, not silent weakening.

---

## Execution Handoff

After M2, **M3** wires it to the outside: sidecar subcommand `plan_capture`, `lmt-shared` `CapturePlan` DTO, `lmt-app` helper, Tauri shim, CLI `plan-capture`, E2E, self-contained HTML card, `docs/agents-cli.md`, schema dump. **M4** adds curved self-occlusion (visibility check (d)) + strong-arc validation.
