# Precision Mode — Capture Gate + QA (L3 + compare-known + nominal-misfit) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Three small, independent verification/observability upgrades for precision mode: (1) `plan-capture --min-views` so the precision capture profile can demand ≥3 views/cabinet; (2) `compare-known` tolerance flags so a precision-grade acceptance run can tighten size/distance/angle gates from the CLI; (3) a `nominal_misfit` warning when the rigid `align_to_nominal` residual is large — surfacing the *non-absorbable* pitch/1:1 class (global isotropic scale / shape deviation) that the L1 cross-check (which targets the absorbable class) does not cover.

**Architecture:** L3 threads a `min_views` request field through the existing planner (default stays `gates.MIN_VIEWS = 2` to satisfy the gate-mirror test). compare-known thresholds are ALREADY honored by the Python sidecar (`CompareKnownInput.thresholds`); the only gap is the Rust/CLI never populating them — so this is pure Rust threading. The `nominal_misfit` warning is a one-constant threshold check in the shared `solve_and_emit` align block, firing only for `gauge_strategy=align_to_nominal` (SL), since `fix_root_cabinet` (charuco) has `align_rms = 0`.

**Tech Stack:** Python sidecar (`capture_planner`, `compare_known`, `reconstruct`, pytest) + Rust (`lmt-cli` clap, `lmt-app`, `adapter-visual-ba`).

**Source spec:** `docs/superpowers/specs/2026-06-04-precision-mode-design.md` §A.3 (L3), §A.4 (compare-known thresholds + pitch two-class), P5. **Branch:** `feat/precision-mode`.

**Scope note:** Plan 3 of 3. Independent of Plan 1 (intrinsics) and Plan 2 (subpixel). Part D closes the "non-absorbable pitch class" gap flagged in Plan 1's self-review.

---

## File Structure

| File | Responsibility | Change |
|---|---|---|
| `python-sidecar/src/lmt_vba_sidecar/ipc.py:527-558` | PlanCaptureInput | add `min_views: int = 2` |
| `python-sidecar/src/lmt_vba_sidecar/capture_planner/cmd.py:49-54` | plan request | thread `min_views` into score_kwargs |
| `python-sidecar/src/lmt_vba_sidecar/capture_planner/scoring.py:36` | scoring | `min_views` kwarg → coverage/bridging |
| `python-sidecar/src/lmt_vba_sidecar/capture_planner/visibility.py:124-155` | coverage gate | `min_views` kwarg, replace `gates.MIN_VIEWS` at :147 |
| `python-sidecar/src/lmt_vba_sidecar/capture_planner/optimize.py:48-64` | optimizer objective | thread `min_views` into `_score` |
| `python-sidecar/src/lmt_vba_sidecar/reconstruct.py:617-634` | align block | `nominal_misfit` warning |
| `crates/lmt-cli/src/cli.rs:458-487, :449-455` | clap | `--min-views` on PlanCapture; `--*-tol-*` on CompareKnown |
| `crates/lmt-cli/src/commands/visual.rs:773-811, :757-771` | transport | thread the flags |
| `crates/lmt-app/src/visual.rs:703-741, :788-800` | service | thread the flags |
| `crates/adapter-visual-ba/src/api.rs` (PlanCapture + CompareKnown args/payload) | IPC | new fields + payload keys |
| tests (planner, compare-known, sl_reconstruct, cli_e2e) | | new cases |

**Gate-mirror constraint:** `test_capture_planner_gates.py::test_gate_constants_mirror_*` asserts `gates.MIN_VIEWS == 2` and that it mirrors `reconstruct.check_observability(min_views=2,...)`. The `min_views` request param MUST default to `gates.MIN_VIEWS` (=2) so that test stays green.

---

> **On the "parallax/oblique constraint" (Codex #4 — resolved as observability, NOT a new hard gate):**
> A verify pass RAN the planner: near-duplicate / fronto-parallel covering views do **not** pass the
> planner's final verdict, because the Monte-Carlo p95 residual already penalizes them ~1/sin(parallax)
> (measured: a 20mm-baseline pair → p95≈269mm → `pass=False`; only wide baselines clear the 3mm target;
> the candidate azimuth pool already spans −70°..70°). So **do NOT build a redundant hard parallax gate**
> (YAGNI) — the precision profile gets its parallax requirement from a tighter `--target-mm` + `--min-views`.
> The one real gap is **observability**: a count-`reconstructable` cabinet that fails on p95 lands in
> `unreachable_regions` with no hint that the cause is *baseline*, not *coverage*. Task 3b (end of
> Part A) adds that diagnostic. (This matches spec §A.3 as revised.)

## Part A — L3 `plan-capture --min-views` (Python threading)

### Task 1: Thread `min_views` through the planner objective (`_score`)

**Files:**
- Modify: `python-sidecar/src/lmt_vba_sidecar/capture_planner/optimize.py:48-64`
- Test: `python-sidecar/tests/test_capture_planner_optimize.py`

- [ ] **Step 1: Write the failing test** — `_score`'s view-deficit term must grow when `min_views` rises (a cabinet with 2 views is a deficit of 1 at `min_views=3`, 0 at `min_views=2`).

```python
# append to tests/test_capture_planner_optimize.py
from lmt_vba_sidecar.capture_planner.optimize import _score

def test_score_deficit_scales_with_min_views():
    report = {(0, 0): {"n_views": 2}, (1, 0): {"n_views": 4}}
    fails2, deficit2 = _score(report, n_cabinets=2, min_views=2)
    fails3, deficit3 = _score(report, n_cabinets=2, min_views=3)
    assert deficit2 == 0            # both cabinets meet 2 views
    assert deficit3 == 1            # cabinet (0,0) is 1 view short of 3
```

- [ ] **Step 2: Run to verify failure**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_capture_planner_optimize.py::test_score_deficit_scales_with_min_views -v`
Expected: FAIL — `_score()` takes no `min_views` kwarg (TypeError).

- [ ] **Step 3: Add `min_views` to `_score`** — `optimize.py:48` signature and the two `gates.MIN_VIEWS` references (`:56`, `:58`) become the param (default `gates.MIN_VIEWS`):

```python
# optimize.py:48
def _score(report, n_cabinets, *, min_views=gates.MIN_VIEWS):
    if report is None:
        return (n_cabinets, min_views * n_cabinets)        # was gates.MIN_VIEWS
    fails = sum(1 for v in report.values() if v["n_views"] < min_views)
    deficit = sum(max(0, min_views - v["n_views"]) for v in report.values())  # was gates.MIN_VIEWS
    return (fails, deficit)
```

And `optimize(...)` (`optimize.py:62`) gains `min_views=gates.MIN_VIEWS` and forwards it to every `_score(...)` call site within (and into `score_screen` via `score_kwargs`).

- [ ] **Step 4: Run to verify pass + planner regression**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_capture_planner_optimize.py tests/test_capture_planner_gates.py -v`
Expected: PASS (gate-mirror test stays green because default is `gates.MIN_VIEWS`).

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/capture_planner/optimize.py python-sidecar/tests/test_capture_planner_optimize.py
git commit -m "feat(planner): thread min_views into optimizer objective"
```

### Task 2: Thread `min_views` through `coverage_report` + request field + end-to-end

**Files:**
- Modify: `visibility.py:124-155`, `scoring.py:36`, `cmd.py:49-54`, `ipc.py:550`
- Test: `python-sidecar/tests/test_capture_planner_visibility.py`, `test_capture_planner_cmd.py`

- [ ] **Step 1: Write the failing coverage test** — reuse the geometry/camera builders already in `test_capture_planner_visibility.py` (open the file; its module-level helpers build a `ScreenGeometry` + a list of `Camera`). Construct a scene where one cabinet is covered by exactly 2 views; assert it is `reconstructable` at `min_views=2` and NOT at `min_views=3`.

```python
# append to tests/test_capture_planner_visibility.py — reuse this file's existing
# scene builders (the ones the current coverage tests already call).
from lmt_vba_sidecar.capture_planner.visibility import coverage_report

def test_coverage_min_views_param_tightens_reconstructable():
    geom, cams = _scene_with_two_view_cabinet()   # REUSE existing builder in this file;
    #   if no single builder yields a 2-view cabinet, compose from the existing
    #   geom/camera helpers so exactly one cabinet has 2 covering views.
    per2, _ = coverage_report(geom, cams, min_views=2)
    per3, _ = coverage_report(geom, cams, min_views=3)
    tgt2 = next(c for c in per2 if len(c.covering) == 2)
    tgt3 = next(c for c in per3 if (c.col, c.row) == (tgt2.col, tgt2.row))
    assert tgt2.reconstructable is True
    assert tgt3.reconstructable is False
```

- [ ] **Step 2: Run to verify failure**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_capture_planner_visibility.py::test_coverage_min_views_param_tightens_reconstructable -v`
Expected: FAIL — `coverage_report()` has no `min_views` kwarg.

- [ ] **Step 3: Add `min_views` to `coverage_report`** (`visibility.py:124`, replace `gates.MIN_VIEWS` at `:147`) and forward from `score_screen` (`scoring.py:36`, add `min_views=gates.MIN_VIEWS`, pass to `coverage_report`/`bridging_report` at `:39-43`) and from `cmd.run_plan_capture` score_kwargs (`cmd.py:49-54`, add `min_views=cmd.min_views`):

```python
# visibility.py:124
def coverage_report(geom, cams, *, margin_frac=0.05, incidence_max_deg=60.0, min_views=gates.MIN_VIEWS):
    ...
    reconstructable = (len(covering) >= min_views                 # was gates.MIN_VIEWS
                       and total_obs >= gates.MIN_POINTS_PER_CABINET)
```

```python
# ipc.py PlanCaptureInput — after incidence_max_deg (line 550):
    # Precision capture profile can demand >=3 views/cabinet (default mirrors gates.MIN_VIEWS).
    min_views: int = 2
```

```python
# cmd.py:49-54 score_kwargs — add:
                        min_views=cmd.min_views,
```

- [ ] **Step 4: Run + end-to-end** — also add a `run_plan_capture` assertion in `test_capture_planner_cmd.py` reusing that file's existing `PlanCaptureInput` builder, asserting `min_views=3` lowers `all_pass` (or flips a borderline cabinet's `reconstructable`) vs the default.

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_capture_planner_visibility.py tests/test_capture_planner_cmd.py tests/test_capture_planner_gates.py -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/capture_planner/ python-sidecar/src/lmt_vba_sidecar/ipc.py python-sidecar/tests/
git commit -m "feat(planner): plan-capture min_views request field threaded to coverage gate"
```

### Task 3: Rust `--min-views` flag on `plan-capture`

**Files:**
- Modify: `crates/lmt-cli/src/cli.rs:458-487`, `crates/lmt-cli/src/commands/visual.rs:36-59, :773-811`, `crates/lmt-app/src/visual.rs:788-800`, `crates/adapter-visual-ba/src/api.rs` (PlanCapture args + payload)

- [ ] **Step 1: Add the flag** — `cli.rs PlanCapture`, before line 487 `},`:

```rust
        /// 每箱体最少视角数(精准档传 3);默认 2,与 reconstruct 观测门一致。
        #[arg(long = "min-views", default_value_t = 2)]
        min_views: u32,
```

Thread it: dispatch arm (`visual.rs:36-59`) destructures + passes `min_views`; `plan_capture` fn (`visual.rs:773-786`) gains `min_views: u32`, passes to `run_plan_capture` (`app/visual.rs:789`), which adds it to the PlanCapture IPC args + payload (`api.rs`, key `"min_views": args.min_views`).

- [ ] **Step 2: Build + E2E** — add a mock/real e2e (or extend the existing `plan_capture` e2e) asserting `--min-views 3` is accepted and changes coverage. plan-capture is `write_safe` (no `--yes`), returns a `CapturePlan` envelope.

Run: `cargo build --workspace && cargo test -p lmt-cli --test cli_e2e plan_capture 2>&1 | tail -6`
Expected: compiles; plan-capture e2e green.

- [ ] **Step 3: Commit**

```bash
git add crates/ python-sidecar/
git commit -m "feat(cli): plan-capture --min-views"
```

### Task 3b: per-cabinet `fail_reason` diagnostic (Codex #4 observability)

Surface WHY a cabinet fails — `low_coverage` (not enough views/points to even attempt) vs `low_parallax` (count-reconstructable but p95 over target = degenerate geometry). No new gate; the p95 verdict is unchanged.

**Files:**
- Modify: `python-sidecar/src/lmt_vba_sidecar/capture_planner/scoring.py` (where per-cabinet `pass` is computed)
- Modify: `python-sidecar/src/lmt_vba_sidecar/ipc.py` (`CabinetCoverageData` gains `fail_reason: str | None = None`)
- Modify: `crates/lmt-shared/src/dto.rs:633-644` (`CabinetCoverage` gains `fail_reason`)
- Test: `python-sidecar/tests/test_capture_planner_scoring.py`

- [ ] **Step 1: Write the failing test** — reuse `test_capture_planner_scoring.py`'s existing scene builder; a near-duplicate-view cabinet (count-reconstructable, high p95) → `fail_reason == "low_parallax"`; a <2-view cabinet → `"low_coverage"`; a passing cabinet → `None`.

```python
# append to tests/test_capture_planner_scoring.py (reuse this file's scene/camera builders)
def test_fail_reason_distinguishes_parallax_from_coverage():
    geom, cams = _two_near_duplicate_cams_scene()   # REUSE/compose existing builders so one
    #   cabinet is covered by 2 near-duplicate views (reconstructable=True, p95 >> target).
    per = score_screen(geom, cams, target_p95_residual_mm=3.0)
    tgt = next(c for c in per if c.reconstructable and not c.pass_)
    assert tgt.fail_reason == "low_parallax"
    uncovered = next((c for c in per if not c.reconstructable), None)
    if uncovered is not None:
        assert uncovered.fail_reason == "low_coverage"
    assert all(c.fail_reason is None for c in per if c.pass_)
```

- [ ] **Step 2: Run to verify failure**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_capture_planner_scoring.py::test_fail_reason_distinguishes_parallax_from_coverage -v`
Expected: FAIL — `CabinetCoverage` has no `fail_reason`.

- [ ] **Step 3: Compute `fail_reason`** — at the per-cabinet `pass` computation in `scoring.py` (`pass = bool(cov.reconstructable and bridged and (p95 <= target_p95_residual_mm))`):

```python
    if passed:
        fail_reason = None
    elif not (cov.reconstructable and bridged):
        fail_reason = "low_coverage"
    else:                                  # count-reconstructable but p95 over target
        fail_reason = "low_parallax"
```

Add `fail_reason: str | None = None` to `CabinetCoverageData` (`ipc.py`) and set it; add `#[serde(default)] pub fail_reason: Option<String>` to `CabinetCoverage` (`dto.rs:633-644`). `CabinetCoverage` is already registered in `schema.rs` — no new `add!`.

- [ ] **Step 4: Run + build + commit**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_capture_planner_scoring.py -v && cd .. && cargo build --workspace`
Expected: PASS + compiles.

```bash
git add python-sidecar/ crates/
git commit -m "feat(planner): per-cabinet fail_reason (low_parallax vs low_coverage)"
```

---

## Part B — compare-known tolerance flags (Rust threading; Python already honors)

### Task 4: `--max-size-mm/--max-dist-mm/--max-angle-deg` on `compare-known`

> **Flag names match the spec contract** (`docs/superpowers/specs/2026-06-04-precision-mode-design.md` §A.4 / item 6: `--max-size-mm` / `--max-dist-mm` / `--max-angle-deg`). Codex #8 caught an earlier `--size-tol-mm` draft that would have drifted from the spec/self-check/docs — DO NOT use `*-tol-*` names.

**Files:**
- Modify: `crates/lmt-cli/src/cli.rs:449-455`, `crates/lmt-cli/src/commands/visual.rs:757-771`, `crates/lmt-app/src/visual.rs:703-741`, `crates/adapter-visual-ba/src/api.rs:707-720`
- Test: `crates/lmt-cli/tests/cli_e2e.rs`

- [ ] **Step 1: Add the flags** — `cli.rs CompareKnown`, before line 455 `},`:

```rust
        /// size 误差阈值(mm),覆盖默认 2.0。
        #[arg(long = "max-size-mm")]
        max_size_mm: Option<f64>,
        /// 间距误差阈值(mm),覆盖默认 3.0。
        #[arg(long = "max-dist-mm")]
        max_dist_mm: Option<f64>,
        /// 夹角误差阈值(deg),覆盖默认 0.3。
        #[arg(long = "max-angle-deg")]
        max_angle_deg: Option<f64>,
```

- [ ] **Step 2: Thread to the sidecar payload** — dispatch arm (`visual.rs:35`) passes the three options; `compare_known` fn (`visual.rs:757`) gains them and forwards to `run_compare_known` (`app/visual.rs:703`); `CompareKnownArgs` (`api.rs:707-712`) gains `pub max_size_mm: Option<f64>` etc.; the payload (`api.rs:715-720`) builds a `thresholds` object including ONLY the provided keys (mapping the CLI flag → the Python `DEFAULT_THRESHOLDS` keys `size_mm`/`distance_mm`/`angle_deg`):

```rust
// api.rs compare_known payload — build the optional thresholds map:
    let mut thresholds = serde_json::Map::new();
    if let Some(v) = args.max_size_mm { thresholds.insert("size_mm".into(), v.into()); }
    if let Some(v) = args.max_dist_mm { thresholds.insert("distance_mm".into(), v.into()); }
    if let Some(v) = args.max_angle_deg { thresholds.insert("angle_deg".into(), v.into()); }
    let payload = json!({
        "command": "compare_known", "version": 1,
        "report_path": &args.report_path, "known_path": &args.known_path,
        "thresholds": if thresholds.is_empty() { serde_json::Value::Null }
                      else { serde_json::Value::Object(thresholds) },
    });
```

(`CompareKnownInput.thresholds` is `dict[str,float] | None`, so `null` = use defaults — matches the existing Python contract. `CompareKnownResult.thresholds` echoes the applied values back. Note the CLI uses `--max-dist-mm` but the sidecar key is `distance_mm` — the mapping is in this payload builder.)

- [ ] **Step 3: E2E** — gated real-sidecar test (`#[ignore]`, clone the calibrate-style gated harness): write a `report.json` + `known.json` with a known 2mm distance error; assert default run `passed=true`, and `--max-dist-mm 1.0` run `passed=false` with `data.thresholds.distance_mm == 1.0`.

Run: `cargo build --workspace && cargo test -p lmt-cli --test cli_e2e compare_known 2>&1 | tail -6`
Expected: compiles; non-gated assertions green; gated test skipped without `LMT_VBA_SIDECAR_PATH`.

- [ ] **Step 4: Commit**

```bash
git add crates/
git commit -m "feat(cli): compare-known --size/distance/angle tolerance flags"
```

---

## Part C — `nominal_misfit` warning (non-absorbable pitch class, P5)

### Task 5: Warn when `align_to_nominal` residual is large

**Files:**
- Modify: `python-sidecar/src/lmt_vba_sidecar/reconstruct.py:612-634`
- Test: `python-sidecar/tests/test_sl_reconstruct.py`

- [ ] **Step 1: Write the failing test** — inject a GLOBAL ISOTROPIC pitch scale (the non-absorbable class): scale all `p_local` truth uniformly so the as-built wall is rigidly unfittable to nominal; assert a `WarningEvent(code="nominal_misfit")` is emitted (and reconstruction still completes — it is a warning, not a refusal).

```python
# append to tests/test_sl_reconstruct.py
def test_nominal_misfit_warns_on_global_scale(tmp_path, capsys):
    meta_path = _gen_two_cabinet_meta(tmp_path)
    meta = json.loads(meta_path.read_text())
    intr_path, K = _write_intrinsics(tmp_path)
    rect_by_cr = {(c["col"], c["row"]): c["input_rect_px"] for c in meta["cabinets"]}
    pitch_by_cr = {(c["col"], c["row"]): c["pixel_pitch_mm"] for c in meta["cabinets"]}
    cab_by_id = {d["id"]: tuple(d["cabinet"]) for d in meta["dots"]}
    cab_world_t = {(0, 0): np.zeros(3), (1, 0): np.array([500.0, 0.0, 0.0])}
    scale = 1.01   # 1% global isotropic pitch error -> rigid Procrustes cannot absorb it
    truth = {}
    for d in meta["dots"]:
        cr = cab_by_id[d["id"]]
        pl = sl_local_mm(tuple(rect_by_cr[cr]), d["u"], d["v"], pitch_by_cr[cr][0], pitch_by_cr[cr][1])
        truth[d["id"]] = pl * scale + cab_world_t[cr]
    sha = hashlib.sha256(meta_path.read_bytes()).hexdigest()
    poses = [look_at_pose(np.array([px, 0.0, -3500.0]), np.array([250.0, 0.0, 0.0]))
             for px in (-1200.0, -400.0, 400.0, 1200.0)]
    rng = np.random.default_rng(0)
    corr_paths = []
    for vi, (R, t) in enumerate(poses):
        pts = [{"id": d["id"], "u": d["u"], "v": d["v"],
                **dict(zip(("x", "y"), (project_point(K, R, t, truth[d["id"]]) + rng.normal(0, 0.1, 2)).tolist()))}
               for d in meta["dots"]]
        cp = tmp_path / f"corr_{vi}.json"
        cp.write_text(json.dumps({"schema_version": 1, "screen_id": "MAIN", "sl_meta_sha256": sha,
            "screen_resolution": meta["screen_resolution"], "camera_image_size": [4000, 3000],
            "source_input": f"/cap/p{vi}.mp4", "points": pts}))
        corr_paths.append(str(cp))
    cmd = ReconstructStructuredLightInput.model_validate({
        "command": "reconstruct_structured_light", "version": 1,
        "project": {"screen_id": "MAIN", "cabinet_array": {"cols": 2, "rows": 1,
                    "absent_cells": [], "cabinet_size_mm": [500, 500]}},
        "correspondence_paths": corr_paths, "sl_meta_path": str(meta_path),
        "intrinsics_path": str(intr_path), "pose_report_path": str(tmp_path / "rep.json")})
    assert run_reconstruct_structured_light(cmd) == 0
    warns = [json.loads(l) for l in capsys.readouterr().out.splitlines()
             if l.strip() and json.loads(l).get("event") == "warning"]
    assert any(w["code"] == "nominal_misfit" for w in warns), warns
```

- [ ] **Step 2: Run to verify failure**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_sl_reconstruct.py::test_nominal_misfit_warns_on_global_scale -v`
Expected: FAIL — no `nominal_misfit` warning emitted today.

- [ ] **Step 3: Add the warning** — in `reconstruct.py`, after `align_rms_mm` is computed (`:633`, the `procrustes_rigid` return inside the `align_to_nominal` block), add a module constant and the check:

```python
# reconstruct.py — module constant near the other gate constants
NOMINAL_MISFIT_WARN_MM = 5.0   # rigid align residual above this => pitch/shape deviation, not pose

# reconstruct.py — right after align_rms_mm is assigned (line ~633), still inside
# the `if gauge_strategy == "align_to_nominal":` block:
        if align_rms_mm > NOMINAL_MISFIT_WARN_MM:
            write_event(WarningEvent(
                event="warning", code="nominal_misfit",
                message=(f"align_to_nominal residual {align_rms_mm:.1f}mm > {NOMINAL_MISFIT_WARN_MM}mm: "
                         "suspected screen pitch scale / shape deviation, NOT a pose error")))
```

(`WarningEvent` and `write_event` are already imported in `reconstruct.py`. The check is inside the `align_to_nominal` branch, so `fix_root_cabinet`/charuco never triggers it.)

- [ ] **Step 4: Run to verify pass + reconstruct regression**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_sl_reconstruct.py tests/test_reconstruct.py -v`
Expected: PASS (existing aligned-frame tests have small residuals well under 5mm, so no spurious warning).

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/reconstruct.py python-sidecar/tests/test_sl_reconstruct.py
git commit -m "feat(sl): nominal_misfit warning when align_to_nominal residual is large (P5)"
```

### Task 6: Docs + final self-check

**Files:**
- Modify: `docs/agents-cli.md`

- [ ] **Step 1: Update rows** — `plan-capture` row gains `[--min-views <N>]` and notes the per-cabinet `fail_reason` (`low_parallax`/`low_coverage`); `compare-known` row gains `[--max-size-mm F] [--max-dist-mm F] [--max-angle-deg F]` (the spec contract names — NOT `*-tol-*`); `reconstruct-structured-light` row notes the `nominal_misfit` warning code (alongside the existing `cabinet_quality`/`high_rejection`). No error-code table change (warnings are free-form `WarningEvent.code`).
- [ ] **Step 2: Final workspace + sidecar check**

Run: `cargo test --workspace 2>&1 | tail -5 && cd python-sidecar && .venv/bin/python -m pytest tests/ 2>&1 | tail -5`
Expected: all green.
- [ ] **Step 3: Commit**

```bash
git add docs/agents-cli.md
git commit -m "docs(agents-cli): plan-capture --min-views, compare-known tolerances, nominal_misfit"
```

---

## Self-Review

**Spec coverage:** L3 `--min-views` threaded end-to-end with default preserved (Tasks 1-3 ✓, gate-mirror kept green); **Codex #4 resolved as observability** — no redundant hard parallax gate (the p95 verdict already rejects degenerate geometry, verified by running the planner), instead a per-cabinet `fail_reason` diagnostic (Task 3b ✓); compare-known CLI tolerances with **spec-matching `--max-*` names** (Task 4 ✓, Codex #8 — Python already honored the thresholds, Rust-only gap closed); P5 non-absorbable (isotropic-scale) pitch class via `nominal_misfit` (Task 5 ✓ — closes the gap flagged in Plan 1's self-review, and is the (a)-class guard of spec P6). The K-absorbable classes (b)(c) are Plan 1's cross-check; together the three classes × three guards cover §A.4 / P6.

**Placeholder scan:** real code in every code step. One test (Task 2 `coverage_report`) directs the implementer to reuse the existing scene builders in `test_capture_planner_visibility.py` rather than inlining a `ScreenGeometry`/`Camera` constructor I did not capture verbatim — the implementer must open that file and reuse/compose its helpers; the assertion logic is fully specified. The `_score` test (Task 1) and the `nominal_misfit` test (Task 5) are fully self-contained.

**Type consistency:** `min_views` default is `gates.MIN_VIEWS` (=2) at every Python layer (`_score`, `optimize`, `coverage_report`, `score_screen`, `PlanCaptureInput`) and `--min-views default_value_t = 2` on the Rust side — consistent, and required by the gate-mirror test. compare-known threshold keys (`size_mm`/`distance_mm`/`angle_deg`) match the Python `DEFAULT_THRESHOLDS` keys exactly. `nominal_misfit` warning code is a new free-form string (no enum change), consistent with the existing `cabinet_quality`/`high_rejection`/`missing_covariance` codes.
