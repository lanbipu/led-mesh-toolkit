# Capture Guidance Planner — M3a Implementation Plan (sidecar subcommand)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Expose the M1+M2 planner through the sidecar's JSON protocol as a new `plan_capture` subcommand, so `echo <json> | python -m lmt_vba_sidecar plan_capture` returns a `CapturePlan` result event.

**Architecture:** New `ipc.py` input/result models following the existing `ResultData`/`ResultEvent` + `write_event` pattern; a new `capture_planner/cmd.py::run_plan_capture(cmd) -> int` that runs expand→seed→optimize→assemble; `__main__.py` registration in the three dispatch tables.

**Tech Stack:** Python 3.12, pydantic v2, M1+M2 capture_planner package.

**Scope note:** spec §5 (IPC contract) + §8 M3 (Python slice only). M3b (Rust adapter/DTO/CLI/E2E) and M3c (HTML card/docs/schema) follow. JSON cannot carry NaN, so non-reconstructable cabinets serialize `p95_residual_mm: null`.

**Run env:** worktree `python-sidecar/`, `./.venv/bin/python`.

---

## File Structure

| File | Responsibility |
| --- | --- |
| `python-sidecar/src/lmt_vba_sidecar/ipc.py` | Add `CaptureIntrinsicsSpec`, `ReachableShell`, `PlanCaptureInput`, `CaptureStationData`, `CabinetCoverageData`, `UnreachableRegionData`, `PlanCaptureResultData`, `PlanCaptureResultEvent`. |
| `python-sidecar/src/lmt_vba_sidecar/capture_planner/cmd.py` | `run_plan_capture(cmd) -> int`. |
| `python-sidecar/src/lmt_vba_sidecar/__main__.py` | Register `plan_capture` in the 3 dispatch tables. |
| `python-sidecar/tests/test_capture_planner_cmd.py` | Direct `run_plan_capture` test (capsys) + subprocess E2E (happy + error envelope). |

---

## Task 1: IPC models + `run_plan_capture` + dispatch wiring

**Files:**
- Modify: `python-sidecar/src/lmt_vba_sidecar/ipc.py` (append models)
- Create: `python-sidecar/src/lmt_vba_sidecar/capture_planner/cmd.py`
- Modify: `python-sidecar/src/lmt_vba_sidecar/__main__.py`
- Test: `python-sidecar/tests/test_capture_planner_cmd.py`

- [ ] **Step 1: Write the failing test**

Create `python-sidecar/tests/test_capture_planner_cmd.py`:

```python
import json

from lmt_vba_sidecar.ipc import PlanCaptureInput
from lmt_vba_sidecar.capture_planner.cmd import run_plan_capture


def _flat_input():
    return PlanCaptureInput.model_validate({
        "command": "plan_capture",
        "version": 1,
        "project": {
            "screen_id": "V000",
            "cabinet_array": {"cols": 2, "rows": 2, "cabinet_size_mm": [500.0, 500.0],
                              "absent_cells": []},
            "shape_prior": "flat",
        },
        "intrinsics": {"image_size": [1920, 1080], "hfov_deg": 60.0},
        "shell": {"standoff_min_mm": 2000.0, "standoff_max_mm": 4000.0,
                  "height_min_mm": 400.0, "height_max_mm": 2200.0},
        "target_p95_residual_mm": 4.0,
        "trials": 6, "n_fan": 5,
    })


def test_run_plan_capture_emits_result_event(capsys):
    rc = run_plan_capture(_flat_input())
    assert rc == 0
    line = capsys.readouterr().out.strip().splitlines()[-1]
    ev = json.loads(line)
    assert ev["event"] == "result"
    data = ev["data"]
    assert len(data["stations"]) >= 5                  # >= seed (fan+top+bottom)
    assert len(data["coverage"]) == 4                  # 2x2 cabinets
    assert data["all_pass"] is True
    assert data["unreachable_regions"] == []
    cov = data["coverage"][0]
    assert set(cov) >= {"col", "row", "p95_residual_mm", "reconstructable", "pass"}
    assert cov["pass"] is True
    st = data["stations"][0]
    assert set(st) >= {"id", "position_mm", "look_at_mm", "standoff_mm", "height_mm",
                       "role", "covers_cabinets"}
    # p95 must be a real number here (all reconstructable), JSON has no NaN
    assert isinstance(cov["p95_residual_mm"], (int, float))


def test_run_plan_capture_curved_radius_too_small_is_invalid_input(capsys):
    inp = _flat_input()
    inp = inp.model_copy(update={
        "project": inp.project.model_copy(update={
            "shape_prior": {"curved": {"radius_mm": 1.0}}})})  # << min ratio
    rc = run_plan_capture(inp)
    assert rc == 1
    line = capsys.readouterr().out.strip().splitlines()[-1]
    ev = json.loads(line)
    assert ev["event"] == "error"
    assert ev["code"] == "invalid_input"
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd python-sidecar && ./.venv/bin/python -m pytest tests/test_capture_planner_cmd.py -q`
Expected: FAIL with `ImportError: cannot import name 'PlanCaptureInput'`.

- [ ] **Step 3a: Append IPC models to `ipc.py`**

Append to `python-sidecar/src/lmt_vba_sidecar/ipc.py`:

```python
# ---------------------------------------------------------------------------
# plan_capture — recommend camera capture stations for a screen
# ---------------------------------------------------------------------------

class CaptureIntrinsicsSpec(BaseModel):
    image_size: tuple[int, int]               # [w, h] px
    hfov_deg: float | None = None
    vfov_deg: float | None = None


class ReachableShell(BaseModel):
    standoff_min_mm: float
    standoff_max_mm: float
    height_min_mm: float
    height_max_mm: float


class PlanCaptureInput(BaseModel):
    command: Literal["plan_capture"]
    version: Literal[1]
    project: ReconstructProject               # screen_id + cabinet_array + shape_prior
    intrinsics: CaptureIntrinsicsSpec
    shell: ReachableShell
    target_p95_residual_mm: float = 3.0
    pixel_sigma_px: float = 0.3
    nominal_deviation_mm: float = 2.0
    focal_err_frac: float = 0.0
    incidence_max_deg: float = 60.0
    sample_grid: tuple[int, int] = (4, 4)
    n_fan: int = 5
    max_stations: int = 24
    n_standoff: int = 2
    n_height: int = 3
    n_azimuth: int = 7
    trials: int = 20
    seed: int = 0


class CaptureStationData(BaseModel):
    id: str
    position_mm: list[float]                  # [x, y, z] model frame
    look_at_mm: list[float]                   # optical axis hit on wall plane z=0
    standoff_mm: float
    height_mm: float
    role: str                                 # fan | top | bottom | added
    covers_cabinets: list[list[int]]          # [[col, row], ...]


class CabinetCoverageData(BaseModel):
    col: int
    row: int
    p95_residual_mm: float | None             # null when not reconstructable (no NaN in JSON)
    n_views: int
    total_observations: int
    reconstructable: bool
    low_observation: bool
    bridged: bool
    pass_: bool = Field(alias="pass")

    model_config = {"populate_by_name": True, "serialize_by_alias": True}


class UnreachableRegionData(BaseModel):
    cabinets: list[list[int]]
    reason: str


class PlanCaptureResultData(BaseModel):
    stations: list[CaptureStationData]
    coverage: list[CabinetCoverageData]
    unreachable_regions: list[UnreachableRegionData]
    all_pass: bool
    target_p95_residual_mm: float


class PlanCaptureResultEvent(BaseModel):
    event: Literal["result"]
    data: PlanCaptureResultData
```

- [ ] **Step 3b: Create `capture_planner/cmd.py`**

```python
"""sidecar 'plan_capture' entrypoint: project geometry + intrinsics + reachable
shell -> recipe seed -> greedy optimize -> CapturePlan result event."""
from __future__ import annotations

import math

import numpy as np

from lmt_vba_sidecar.io_utils import write_event
from lmt_vba_sidecar.ipc import (
    CabinetCoverageData,
    CaptureStationData,
    ErrorEvent,
    PlanCaptureInput,
    PlanCaptureResultData,
    PlanCaptureResultEvent,
    UnreachableRegionData,
)
from lmt_vba_sidecar.capture_planner import gates
from lmt_vba_sidecar.capture_planner.geometry import expand_screen
from lmt_vba_sidecar.capture_planner.visibility import coverage_report, intrinsics_from_fov
from lmt_vba_sidecar.capture_planner.seed import Shell, seed_cameras
from lmt_vba_sidecar.capture_planner.optimize import optimize


def _aim_point_on_wall(cam) -> np.ndarray:
    center = -cam.R.T @ cam.t
    axis = cam.R.T @ np.array([0.0, 0.0, 1.0])     # optical axis, world frame
    if abs(axis[2]) < 1e-9:
        return center
    return center + (-center[2] / axis[2]) * axis


def run_plan_capture(cmd: PlanCaptureInput) -> int:
    image_size = (int(cmd.intrinsics.image_size[0]), int(cmd.intrinsics.image_size[1]))
    try:
        K = intrinsics_from_fov(image_size, cmd.intrinsics.hfov_deg, cmd.intrinsics.vfov_deg)
        geom = expand_screen(cmd.project.cabinet_array, cmd.project.shape_prior,
                             tuple(cmd.sample_grid))
    except ValueError as exc:
        write_event(ErrorEvent(event="error", code="invalid_input",
                               message=str(exc), fatal=True))
        return 1

    shell = Shell(cmd.shell.standoff_min_mm, cmd.shell.standoff_max_mm,
                  cmd.shell.height_min_mm, cmd.shell.height_max_mm)
    seed_stations = seed_cameras(geom, K, image_size, shell, n_fan=cmd.n_fan)
    seed_cams = [s.camera for s in seed_stations]
    score_kwargs = dict(pixel_sigma=cmd.pixel_sigma_px,
                        nominal_deviation_mm=cmd.nominal_deviation_mm,
                        focal_err_frac=cmd.focal_err_frac,
                        incidence_max_deg=cmd.incidence_max_deg,
                        trials=cmd.trials, seed=cmd.seed,
                        target_p95_residual_mm=cmd.target_p95_residual_mm)
    result = optimize(geom, K, image_size, shell, seed_cams=seed_cams,
                      max_stations=cmd.max_stations, n_standoff=cmd.n_standoff,
                      n_height=cmd.n_height, n_azimuth=cmd.n_azimuth,
                      score_kwargs=score_kwargs)

    per_cab, counts = coverage_report(geom, result.cameras,
                                      incidence_max_deg=cmd.incidence_max_deg)

    roles = ([s.role for s in seed_stations]
             + ["added"] * (len(result.cameras) - len(seed_cams)))
    cx = geom.total_width_mm / 2.0
    stations = []
    for ci, cam in enumerate(result.cameras):
        pos = -cam.R.T @ cam.t
        aim = _aim_point_on_wall(cam)
        covers = [[c.col, c.row] for c in geom.cabinets
                  if counts.get((ci, (c.col, c.row)), 0) >= gates.MIN_PNP_CORNERS]
        stations.append(CaptureStationData(
            id=f"S{ci + 1:02d}",
            position_mm=[float(x) for x in pos],
            look_at_mm=[float(x) for x in aim],
            standoff_mm=float(math.hypot(pos[0] - cx, pos[2])),
            height_mm=float(pos[1]),
            role=roles[ci],
            covers_cabinets=covers,
        ))

    coverage = []
    for c in per_cab:
        v = result.report[(c.col, c.row)]
        p95 = v["p95_mm"]
        coverage.append(CabinetCoverageData(
            col=c.col, row=c.row,
            p95_residual_mm=(None if (p95 is None or math.isnan(p95)) else float(p95)),
            n_views=v["n_views"], total_observations=c.total_observations,
            reconstructable=v["reconstructable"], low_observation=v["low_observation"],
            bridged=v["bridged"], pass_=v["pass"],
        ))

    unreachable = []
    if result.unreachable:
        unreachable.append(UnreachableRegionData(
            cabinets=[[col, row] for (col, row) in result.unreachable],
            reason="no shell placement meets target (raise shell / split arc / add bridging)",
        ))

    write_event(PlanCaptureResultEvent(event="result", data=PlanCaptureResultData(
        stations=stations, coverage=coverage, unreachable_regions=unreachable,
        all_pass=(len(result.unreachable) == 0),
        target_p95_residual_mm=cmd.target_p95_residual_mm,
    )))
    return 0
```

- [ ] **Step 3c: Register in `__main__.py`**

In `python-sidecar/src/lmt_vba_sidecar/__main__.py`:

Add the parser line after the other `sub.add_parser(...)` calls:
```python
    sub.add_parser("plan_capture")
```

Add to `SUBCOMMAND_MODULES`:
```python
        "plan_capture": "lmt_vba_sidecar.capture_planner.cmd",
```

Add to `SUBCOMMAND_ENTRYPOINTS` (the input model must be importable — add `PlanCaptureInput` to the existing `from lmt_vba_sidecar.ipc import (...)` block at the top of `__main__.py`):
```python
        "plan_capture": ("run_plan_capture", PlanCaptureInput),
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd python-sidecar && ./.venv/bin/python -m pytest tests/test_capture_planner_cmd.py::test_run_plan_capture_emits_result_event tests/test_capture_planner_cmd.py::test_run_plan_capture_curved_radius_too_small_is_invalid_input -v`
Expected: PASS (2 passed).

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/ipc.py \
        python-sidecar/src/lmt_vba_sidecar/capture_planner/cmd.py \
        python-sidecar/src/lmt_vba_sidecar/__main__.py \
        python-sidecar/tests/test_capture_planner_cmd.py
git commit -m "feat(capture-planner): plan_capture sidecar subcommand (ipc + cmd + dispatch)"
```

---

## Task 2: Subprocess E2E (dispatch wiring + JSON protocol)

**Files:**
- Test: `python-sidecar/tests/test_capture_planner_cmd.py` (append)

Confirms `python -m lmt_vba_sidecar plan_capture` reads stdin JSON and writes a parseable result envelope (the real Rust-facing path).

- [ ] **Step 1: Write the failing test (append)**

Append to `python-sidecar/tests/test_capture_planner_cmd.py`:

```python
import subprocess
import sys


def _flat_payload():
    return {
        "command": "plan_capture", "version": 1,
        "project": {"screen_id": "V000",
                    "cabinet_array": {"cols": 2, "rows": 2,
                                      "cabinet_size_mm": [500.0, 500.0], "absent_cells": []},
                    "shape_prior": "flat"},
        "intrinsics": {"image_size": [1920, 1080], "hfov_deg": 60.0},
        "shell": {"standoff_min_mm": 2000.0, "standoff_max_mm": 4000.0,
                  "height_min_mm": 400.0, "height_max_mm": 2200.0},
        "target_p95_residual_mm": 4.0, "trials": 6, "n_fan": 5,
    }


def test_subprocess_plan_capture_happy():
    proc = subprocess.run(
        [sys.executable, "-m", "lmt_vba_sidecar", "plan_capture"],
        input=json.dumps(_flat_payload()), capture_output=True, text=True,
    )
    assert proc.returncode == 0, proc.stderr
    ev = json.loads(proc.stdout.strip().splitlines()[-1])
    assert ev["event"] == "result"
    assert ev["data"]["all_pass"] is True
    # output must be valid JSON end-to-end (no bare NaN tokens)
    assert "NaN" not in proc.stdout


def test_subprocess_plan_capture_invalid_input_envelope():
    payload = _flat_payload()
    payload["project"]["shape_prior"] = {"curved": {"radius_mm": 1.0}}
    proc = subprocess.run(
        [sys.executable, "-m", "lmt_vba_sidecar", "plan_capture"],
        input=json.dumps(payload), capture_output=True, text=True,
    )
    assert proc.returncode == 1
    ev = json.loads(proc.stdout.strip().splitlines()[-1])
    assert ev["event"] == "error"
    assert ev["code"] == "invalid_input"
```

- [ ] **Step 2: Run test to verify it fails**

If Task 1 is complete this should already PASS (dispatch is wired). Run it to confirm wiring:
Run: `cd python-sidecar && ./.venv/bin/python -m pytest tests/test_capture_planner_cmd.py -k subprocess -v`
Expected: PASS (2 passed). If FAIL with "not yet implemented", the `__main__.py` registration (Task 1 Step 3c) is incomplete — fix it.

- [ ] **Step 3: Run full sidecar regression**

Run: `cd python-sidecar && ./.venv/bin/python -m pytest tests/ -q`
Expected: PASS — all prior + capture-planner tests, 0 failures.

- [ ] **Step 4: Verify the subcommand is registered**

Run: `cd python-sidecar && echo '{"command":"plan_capture","version":1,"project":{"screen_id":"V0","cabinet_array":{"cols":2,"rows":1,"cabinet_size_mm":[500,500],"absent_cells":[]},"shape_prior":"flat"},"intrinsics":{"image_size":[1920,1080],"hfov_deg":60},"shell":{"standoff_min_mm":2000,"standoff_max_mm":4000,"height_min_mm":400,"height_max_mm":2200},"trials":6}' | ./.venv/bin/python -m lmt_vba_sidecar plan_capture | python -m json.tool`
Expected: pretty-printed result envelope with `stations` / `coverage` / `all_pass`.

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/tests/test_capture_planner_cmd.py
git commit -m "test(capture-planner): plan_capture subprocess E2E (happy + invalid-input envelope)"
```

---

## Self-Review (against spec §5, §8 M3 Python slice)

- **Spec coverage:** §5 `PlanCaptureInput` (intrinsics FOV→K, shell, params) → Task 1 ipc. CapturePlan result (stations/coverage/unreachable/all_pass) → Task 1 cmd. Subcommand dispatch → Task 1 Step 3c, verified Task 2.
- **NaN handling:** explicit — `p95_residual_mm` is `float | None`, NaN→None in cmd.py, asserted "no NaN in stdout" in the subprocess test.
- **Type consistency:** input reuses `ReconstructProject` (existing); `pass_`/alias mirrors `CabinetSizeCheck`; result event mirrors `CompareKnownResultEvent`. cmd.py consumes M1/M2 `expand_screen`/`seed_cameras`/`optimize`/`coverage_report` unchanged.
- **Placeholders:** none.

---

## Execution Handoff

- **M3b** — Rust: `adapter-visual-ba` (`api.rs` `plan_capture` async fn + `ipc.rs` mirrors), `lmt-shared` `CapturePlan` DTO (+JsonSchema, schema dump), `lmt-app` `capture_plan.rs` helper, `lmt-cli` `plan-capture` subcommand + E2E (happy/refuse/dry-run/envelope), Tauri shim.
- **M3c** — self-contained HTML card (plan view + elevation heatmap + station list) + `docs/agents-cli.md` + final schema/contract checks.
