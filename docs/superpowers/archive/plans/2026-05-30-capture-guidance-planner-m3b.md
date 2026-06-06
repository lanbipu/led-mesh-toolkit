# Capture Guidance Planner — M3b Implementation Plan (Rust CLI + DTO)

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans. Steps use checkbox (`- [ ]`).

**Goal:** Expose `plan_capture` through the Rust stack as `lmt visual plan-capture`, returning a `CapturePlan` in the CLI envelope, mirroring the existing `compare-known` (read-only, sidecar-backed, value-returning) path end-to-end.

**Architecture:** adapter `plan_capture` async fn + ipc Deserialize mirrors → `lmt-app::visual::run_plan_capture` (loads project.yaml, parses intrinsics/shell strings, calls adapter, converts to DTO) → `lmt-shared::dto::CapturePlan` (JsonSchema, in schema dump) → `lmt-cli` `visual plan-capture` subcommand → CLI E2E → Tauri shim.

**Scope note:** spec §6 (Rust slice). `plan-capture` is **read-only / write_safe** in M3b — it returns the plan as the envelope, writes nothing, needs no `--yes`/gate. The HTML card + `--html`/`--out` file writing is **M3c**. No new error code (`invalid_input` covers bad geometry/args; reuse existing `error_codes`).

**Run env:** worktree root. Build/test: `cargo build -p <crate>`, `cargo test -p <crate>`. The CLI E2E runs the real sidecar via `python-sidecar/.venv` (worktree venv → worktree sidecar code).

---

## File Structure

| File | Change |
| --- | --- |
| `crates/adapter-visual-ba/src/ipc.rs` | Add `PlanCaptureResultData` + `CaptureStationData` + `CabinetCoverageData` + `UnreachableRegionData` (Deserialize mirrors). |
| `crates/adapter-visual-ba/src/api.rs` | Add `PlanCaptureArgs` + `pub async fn plan_capture`. |
| `crates/adapter-visual-ba/src/lib.rs` | Re-export the new api fn / types if the crate re-exports (match existing). |
| `crates/lmt-shared/src/dto.rs` | Add `CapturePlan` + `CaptureStation` + `CabinetCoverage` + `UnreachableRegion` (Serialize/Deserialize/JsonSchema). |
| `crates/lmt-shared/src/schema.rs` | `add!("CapturePlan", dto::CapturePlan);` + assert in dump test. |
| `crates/lmt-app/src/visual.rs` | Add `run_plan_capture` + `parse_wxh` + `parse_range` helpers + adapter→DTO conversion. |
| `crates/lmt-cli/src/cli.rs` | Add `VisualCmd::PlanCapture { ... }` variant. |
| `crates/lmt-cli/src/commands/visual.rs` | Dispatch + `plan_capture` fn (read-only, `output::ok`/`err`). |
| `crates/lmt-cli/tests/cli_e2e.rs` | Happy + error-envelope E2E. |
| `src-tauri/src/commands/visual.rs` (or existing) | `#[tauri::command] plan_capture` thin shim + register in `lib.rs`. |

---

## Task 1: adapter ipc mirrors + `plan_capture` api fn

**Files:** `crates/adapter-visual-ba/src/ipc.rs`, `crates/adapter-visual-ba/src/api.rs`

- [ ] **Step 1: Add Deserialize mirrors to `ipc.rs`** (append near the other `*ResultData` types)

```rust
// --- plan_capture result mirror (matches sidecar PlanCaptureResultData) -----
#[derive(Debug, Clone, Deserialize)]
pub struct CaptureStationData {
    pub id: String,
    pub position_mm: [f64; 3],
    pub look_at_mm: [f64; 3],
    pub standoff_mm: f64,
    pub height_mm: f64,
    pub role: String,
    pub covers_cabinets: Vec<[u32; 2]>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CabinetCoverageData {
    pub col: u32,
    pub row: u32,
    pub p95_residual_mm: Option<f64>,   // null when not reconstructable
    pub n_views: u32,
    pub total_observations: u32,
    pub reconstructable: bool,
    pub low_observation: bool,
    pub bridged: bool,
    pub pass: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UnreachableRegionData {
    pub cabinets: Vec<[u32; 2]>,
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlanCaptureResultData {
    pub stations: Vec<CaptureStationData>,
    pub coverage: Vec<CabinetCoverageData>,
    pub unreachable_regions: Vec<UnreachableRegionData>,
    pub all_pass: bool,
    pub target_p95_residual_mm: f64,
}
```

- [ ] **Step 2: Add the api fn to `api.rs`** (after `compare_known`)

```rust
// ---------------------------------------------------------------------------
// plan_capture
// ---------------------------------------------------------------------------

pub struct PlanCaptureArgs {
    pub project: ReconstructProject,
    pub image_size: [u32; 2],
    pub hfov_deg: Option<f64>,
    pub vfov_deg: Option<f64>,
    pub standoff_min_mm: f64,
    pub standoff_max_mm: f64,
    pub height_min_mm: f64,
    pub height_max_mm: f64,
    pub target_p95_residual_mm: f64,
    pub trials: u32,
    pub seed: u32,
    pub progress_tx: Option<mpsc::Sender<Event>>,
    pub cancel: Option<oneshot::Receiver<()>>,
}

pub async fn plan_capture(args: PlanCaptureArgs) -> VbaResult<PlanCaptureResultData> {
    let payload = json!({
        "command": "plan_capture",
        "version": 1,
        "project": &args.project,
        "intrinsics": {
            "image_size": [args.image_size[0], args.image_size[1]],
            "hfov_deg": args.hfov_deg,
            "vfov_deg": args.vfov_deg,
        },
        "shell": {
            "standoff_min_mm": args.standoff_min_mm,
            "standoff_max_mm": args.standoff_max_mm,
            "height_min_mm": args.height_min_mm,
            "height_max_mm": args.height_max_mm,
        },
        "target_p95_residual_mm": args.target_p95_residual_mm,
        "trials": args.trials,
        "seed": args.seed,
    });

    let value = run_sidecar(SidecarRequest {
        subcommand: "plan_capture".into(),
        payload,
        progress_tx: args.progress_tx,
        cancel: args.cancel,
    })
    .await?;

    serde_json::from_value(value).map_err(VbaError::BadEventJson)
}
```

Ensure `PlanCaptureResultData` (and nested) are imported/visible in `api.rs` (it already `use`s the ipc result types — add the new ones to that `use` if api.rs imports them explicitly; otherwise they resolve via `crate::ipc::`). Check the existing `use crate::ipc::{...}` block at the top of api.rs and add the new types.

- [ ] **Step 3: Re-export if needed** — if `lib.rs` re-exports api fns/types (e.g. `pub use api::compare_known;`), add `plan_capture` and `PlanCaptureArgs` / `PlanCaptureResultData` the same way.

- [ ] **Step 4: Build**

Run: `cargo build -p adapter-visual-ba`
Expected: compiles, 0 errors.

- [ ] **Step 5: Commit**

```bash
git add crates/adapter-visual-ba/src/ipc.rs crates/adapter-visual-ba/src/api.rs crates/adapter-visual-ba/src/lib.rs
git commit -m "feat(adapter): plan_capture sidecar call + result mirrors"
```

---

## Task 2: lmt-shared `CapturePlan` DTO + schema dump

**Files:** `crates/lmt-shared/src/dto.rs`, `crates/lmt-shared/src/schema.rs`

- [ ] **Step 1: Add DTO to `dto.rs`** (append)

```rust
// ── Capture guidance planner DTO ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CaptureStation {
    pub id: String,
    pub position_mm: [f64; 3],
    pub look_at_mm: [f64; 3],
    pub standoff_mm: f64,
    pub height_mm: f64,
    pub role: String,
    pub covers_cabinets: Vec<[u32; 2]>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CabinetCoverage {
    pub col: u32,
    pub row: u32,
    pub p95_residual_mm: Option<f64>,
    pub n_views: u32,
    pub total_observations: u32,
    pub reconstructable: bool,
    pub low_observation: bool,
    pub bridged: bool,
    pub pass: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UnreachableRegion {
    pub cabinets: Vec<[u32; 2]>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CapturePlan {
    pub stations: Vec<CaptureStation>,
    pub coverage: Vec<CabinetCoverage>,
    pub unreachable_regions: Vec<UnreachableRegion>,
    pub all_pass: bool,
    pub target_p95_residual_mm: f64,
}
```

- [ ] **Step 2: Register in `schema.rs`** — add next to the other `add!(...)` calls:

```rust
    add!("CapturePlan", dto::CapturePlan);
```

And in the `dump_contains_known_types_and_incomplete_list` test, add `"CapturePlan"` to the asserted-present name list.

- [ ] **Step 3: Build + test**

Run: `cargo test -p lmt-shared`
Expected: PASS (schema dump test includes CapturePlan).

- [ ] **Step 4: Verify schema dump emits it**

Run: `cargo build -p lmt-cli && ./target/debug/lmt --json schema | python -c "import sys,json; d=json.load(sys.stdin); print('CapturePlan' in d['data']['schemas'])"`
Expected: `True` (adjust the JSON path to match the actual envelope shape if needed — the test in Step 3 is the source of truth).

- [ ] **Step 5: Commit**

```bash
git add crates/lmt-shared/src/dto.rs crates/lmt-shared/src/schema.rs
git commit -m "feat(shared): CapturePlan DTO + schema dump registration"
```

---

## Task 3: lmt-app `run_plan_capture` (load + parse + convert)

**Files:** `crates/lmt-app/src/visual.rs`

- [ ] **Step 1: Add parse helpers + `run_plan_capture`** (near `parse_inner_corners`)

```rust
/// Parse `"3840x2160"` → `[3840, 2160]`.
fn parse_wxh(s: &str) -> LmtResult<[u32; 2]> {
    let (a, b) = s
        .split_once(['x', 'X'])
        .ok_or_else(|| LmtError::InvalidInput(format!("image-size must be WxH, got '{s}'")))?;
    let p = |t: &str| t.trim().parse::<u32>()
        .map_err(|_| LmtError::InvalidInput(format!("image-size component '{t}' invalid")))
        .and_then(|v| if v == 0 {
            Err(LmtError::InvalidInput("image-size components must be > 0".into()))
        } else { Ok(v) });
    Ok([p(a)?, p(b)?])
}

/// Parse `"2000..12000"` → `(2000.0, 12000.0)`; min must be < max.
fn parse_range(s: &str, name: &str) -> LmtResult<(f64, f64)> {
    let (a, b) = s
        .split_once("..")
        .ok_or_else(|| LmtError::InvalidInput(format!("{name} must be MIN..MAX, got '{s}'")))?;
    let lo = a.trim().parse::<f64>()
        .map_err(|_| LmtError::InvalidInput(format!("{name} min '{a}' invalid")))?;
    let hi = b.trim().parse::<f64>()
        .map_err(|_| LmtError::InvalidInput(format!("{name} max '{b}' invalid")))?;
    if !(lo < hi) {
        return Err(LmtError::InvalidInput(format!("{name} needs MIN < MAX, got {lo}..{hi}")));
    }
    Ok((lo, hi))
}

#[allow(clippy::too_many_arguments)]
pub fn run_plan_capture(
    project_path: &Path,
    screen_id: &str,
    image_size: &str,
    hfov_deg: Option<f64>,
    vfov_deg: Option<f64>,
    standoff: &str,
    height: &str,
    target_p95_residual_mm: f64,
    trials: u32,
    seed: u32,
) -> LmtResult<lmt_shared::dto::CapturePlan> {
    use lmt_shared::dto::{CabinetCoverage, CapturePlan, CaptureStation, UnreachableRegion};

    if hfov_deg.is_some() == vfov_deg.is_some() {
        return Err(LmtError::InvalidInput(
            "pass exactly one of --hfov-deg / --vfov-deg".into(),
        ));
    }
    let image_size = parse_wxh(image_size)?;
    let (standoff_min_mm, standoff_max_mm) = parse_range(standoff, "standoff")?;
    let (height_min_mm, height_max_mm) = parse_range(height, "height")?;

    let cfg = load_project_yaml_from_path(project_path)?;
    let screen_cfg = load_screen(&cfg, screen_id)?;
    let project = ipc::ReconstructProject {
        screen_id: screen_id.to_string(),
        cabinet_array: ipc_cabinet_array(screen_cfg),
        shape_prior: ipc_shape_prior(screen_cfg),
    };

    let args = PlanCaptureArgs {
        project,
        image_size,
        hfov_deg,
        vfov_deg,
        standoff_min_mm,
        standoff_max_mm,
        height_min_mm,
        height_max_mm,
        target_p95_residual_mm,
        trials,
        seed,
        progress_tx: None,
        cancel: None,
    };
    let out = rt()?.block_on(plan_capture(args)).map_err(map_vba_err)?;

    Ok(CapturePlan {
        stations: out.stations.into_iter().map(|s| CaptureStation {
            id: s.id, position_mm: s.position_mm, look_at_mm: s.look_at_mm,
            standoff_mm: s.standoff_mm, height_mm: s.height_mm, role: s.role,
            covers_cabinets: s.covers_cabinets,
        }).collect(),
        coverage: out.coverage.into_iter().map(|c| CabinetCoverage {
            col: c.col, row: c.row, p95_residual_mm: c.p95_residual_mm,
            n_views: c.n_views, total_observations: c.total_observations,
            reconstructable: c.reconstructable, low_observation: c.low_observation,
            bridged: c.bridged, pass: c.pass,
        }).collect(),
        unreachable_regions: out.unreachable_regions.into_iter().map(|u| UnreachableRegion {
            cabinets: u.cabinets, reason: u.reason,
        }).collect(),
        all_pass: out.all_pass,
        target_p95_residual_mm: out.target_p95_residual_mm,
    })
}
```

Add `PlanCaptureArgs` and `plan_capture` to the `use adapter_visual_ba::...` import block at the top of visual.rs (where `CompareKnownArgs`, `compare_known` are imported).

- [ ] **Step 2: Build**

Run: `cargo build -p lmt-app`
Expected: compiles. (Fix import paths if `PlanCaptureArgs`/`plan_capture` aren't yet exported — Task 1 Step 3.)

- [ ] **Step 3: Commit**

```bash
git add crates/lmt-app/src/visual.rs
git commit -m "feat(app): run_plan_capture (load project + parse intrinsics/shell + adapter→DTO)"
```

---

## Task 4: lmt-cli `visual plan-capture` subcommand

**Files:** `crates/lmt-cli/src/cli.rs`, `crates/lmt-cli/src/commands/visual.rs`

- [ ] **Step 1: Add the clap variant to `cli.rs`** (inside `enum VisualCmd`)

```rust
    /// 采集指导:几何+内参→推荐机位 plan(逐箱体覆盖/残差)。side_effect: write_safe
    #[command(name = "plan-capture")]
    PlanCapture {
        /// 项目根目录。
        project_path: String,
        /// screen id。
        screen_id: String,
        /// 传感器分辨率 WxH,例如 3840x2160。
        #[arg(long = "image-size")]
        image_size: String,
        /// 水平 FOV(度);与 --vfov-deg 二选一。
        #[arg(long = "hfov-deg")]
        hfov_deg: Option<f64>,
        /// 垂直 FOV(度);与 --hfov-deg 二选一。
        #[arg(long = "vfov-deg")]
        vfov_deg: Option<f64>,
        /// 后退距离区间 MIN..MAX(mm),例如 3000..12000。
        #[arg(long)]
        standoff: String,
        /// 架高区间 MIN..MAX(mm),例如 400..3000。
        #[arg(long)]
        height: String,
        /// 每箱体 p95 3D 残差目标(mm)。
        #[arg(long = "target-mm", default_value_t = 3.0)]
        target_mm: f64,
        /// Monte-Carlo 试验次数。
        #[arg(long, default_value_t = 20)]
        trials: u32,
        /// RNG 种子。
        #[arg(long, default_value_t = 0)]
        seed: u32,
    },
```

- [ ] **Step 2: Dispatch + impl in `commands/visual.rs`**

In the `run(cmd, mode, yes, dry_run)` match, add:
```rust
        VisualCmd::PlanCapture {
            project_path, screen_id, image_size, hfov_deg, vfov_deg,
            standoff, height, target_mm, trials, seed,
        } => plan_capture(
            mode, &project_path, &screen_id, &image_size, hfov_deg, vfov_deg,
            &standoff, &height, target_mm, trials, seed,
        ),
```

Add the fn (read-only → no gate, mirrors `compare_known`):
```rust
#[allow(clippy::too_many_arguments)]
fn plan_capture(
    mode: Mode,
    project_path: &str,
    screen_id: &str,
    image_size: &str,
    hfov_deg: Option<f64>,
    vfov_deg: Option<f64>,
    standoff: &str,
    height: &str,
    target_mm: f64,
    trials: u32,
    seed: u32,
) -> i32 {
    // plan-capture is write_safe (computes a plan, writes nothing) — no gate.
    match lmt_app::visual::run_plan_capture(
        Path::new(project_path), screen_id, image_size, hfov_deg, vfov_deg,
        standoff, height, target_mm, trials, seed,
    ) {
        Ok(p) => output::ok(mode, p, |plan| {
            let _ = writeln!(
                std::io::stdout(),
                "plan-capture: {} stations, all_pass={} ({} unreachable region(s))",
                plan.stations.len(), plan.all_pass, plan.unreachable_regions.len()
            );
        }),
        Err(e) => output::err(mode, ApiError::from(e)),
    }
}
```

- [ ] **Step 3: Build**

Run: `cargo build -p lmt-cli`
Expected: compiles.

- [ ] **Step 4: Manual smoke**

Run: `./target/debug/lmt --json visual plan-capture examples/curved-flat MAIN --image-size 1920x1080 --hfov-deg 55 --standoff 3000..12000 --height 400..3000 --trials 6 | python -c "import sys,json;d=json.load(sys.stdin);print(d['ok'], len(d['data']['stations']), d['data']['all_pass'])"`
Expected: `True <n> <bool>`. (If the example screen id isn't `MAIN`, use the correct id from `examples/curved-flat/project.yaml`.)

- [ ] **Step 5: Commit**

```bash
git add crates/lmt-cli/src/cli.rs crates/lmt-cli/src/commands/visual.rs
git commit -m "feat(cli): visual plan-capture subcommand (read-only, returns CapturePlan)"
```

---

## Task 5: CLI E2E (happy + error envelope)

**Files:** `crates/lmt-cli/tests/cli_e2e.rs`

- [ ] **Step 1: Add E2E tests** (mirror the existing happy/error patterns; uses the real sidecar via worktree venv)

```rust
#[test]
fn visual_plan_capture_returns_plan() {
    let tmp = TempDir::new().unwrap();
    let proj = seed_project(tmp.path(), "curved-flat");
    let assert = lmt()
        .args([
            "--json", "visual", "plan-capture",
            proj.to_str().unwrap(), "MAIN",
            "--image-size", "1920x1080", "--hfov-deg", "55",
            "--standoff", "3000..12000", "--height", "400..3000",
            "--trials", "6",
        ])
        .assert()
        .success();
    let env: Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    assert_eq!(env["ok"], true);
    assert!(env["data"]["stations"].as_array().unwrap().len() >= 5);
    assert!(env["data"]["coverage"].as_array().unwrap().len() >= 1);
    // valid JSON end-to-end (no NaN leaked through)
    assert!(!std::str::from_utf8(&assert.get_output().stdout).unwrap().contains("NaN"));
}

#[test]
fn visual_plan_capture_bad_screen_is_error_envelope() {
    let tmp = TempDir::new().unwrap();
    let proj = seed_project(tmp.path(), "curved-flat");
    let assert = lmt()
        .args([
            "--json", "visual", "plan-capture",
            proj.to_str().unwrap(), "BOGUS",
            "--image-size", "1920x1080", "--hfov-deg", "55",
            "--standoff", "3000..12000", "--height", "400..3000",
            "--trials", "6",
        ])
        .assert()
        .failure();
    let stderr = std::str::from_utf8(&assert.get_output().stderr).unwrap().trim_end();
    let env: Value = serde_json::from_str(stderr).expect("stderr must be JSON envelope");
    assert_eq!(env["ok"], false);
    assert!(env["error"]["code"].as_str().unwrap_or("").len() > 0);
}

#[test]
fn visual_plan_capture_bad_image_size_is_invalid_input() {
    let tmp = TempDir::new().unwrap();
    let proj = seed_project(tmp.path(), "curved-flat");
    let assert = lmt()
        .args([
            "--json", "visual", "plan-capture",
            proj.to_str().unwrap(), "MAIN",
            "--image-size", "nonsense", "--hfov-deg", "55",
            "--standoff", "3000..12000", "--height", "400..3000",
        ])
        .assert()
        .failure();
    let stderr = std::str::from_utf8(&assert.get_output().stderr).unwrap().trim_end();
    let env: Value = serde_json::from_str(stderr).unwrap();
    assert_eq!(env["error"]["code"], "invalid_input");
}
```

Before writing: confirm the screen id in `examples/curved-flat/project.yaml` (grep `screens:`); if not `MAIN`, use the actual id in all three tests.

- [ ] **Step 2: Run E2E**

Run: `cargo test -p lmt-cli --test cli_e2e visual_plan_capture`
Expected: 3 passed. (These spawn the real sidecar; allow a few seconds.)

- [ ] **Step 3: Commit**

```bash
git add crates/lmt-cli/tests/cli_e2e.rs
git commit -m "test(cli): plan-capture E2E (happy + bad-screen + bad-image-size envelopes)"
```

---

## Task 6: Tauri shim

**Files:** `src-tauri/src/commands/visual.rs` (or the existing visual commands module), `src-tauri/src/lib.rs`

- [ ] **Step 1: Add the thin command** (transport translation only)

```rust
#[tauri::command]
pub fn plan_capture(
    project_path: String,
    screen_id: String,
    image_size: String,
    hfov_deg: Option<f64>,
    vfov_deg: Option<f64>,
    standoff: String,
    height: String,
    target_mm: f64,
    trials: u32,
    seed: u32,
) -> LmtResult<lmt_shared::dto::CapturePlan> {
    lmt_app::visual::run_plan_capture(
        std::path::Path::new(&project_path), &screen_id, &image_size,
        hfov_deg, vfov_deg, &standoff, &height, target_mm, trials, seed,
    )
}
```

(Match the module's existing import style and where other `visual::*` commands live; if there is no visual commands module, place it alongside the closest existing sidecar-backed command and follow that file's conventions.)

- [ ] **Step 2: Register in `lib.rs`** — add `commands::visual::plan_capture,` (adjust path to the module used) to the `tauri::generate_handler![...]` list.

- [ ] **Step 3: Build**

Run: `cargo build -p lmt-tauri` (or the tauri crate name — check `src-tauri/Cargo.toml` `[package] name`).
Expected: compiles.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/commands/ src-tauri/src/lib.rs
git commit -m "feat(tauri): plan_capture command shim"
```

---

## Task 7: Workspace check

- [ ] **Step 1: Full workspace test**

Run: `cargo test --workspace`
Expected: PASS, 0 failures (existing + new plan-capture E2E + schema test).

- [ ] **Step 2: Contract self-check (CLAUDE.md)**

Run:
```bash
./target/debug/lmt --json schema | python -c "import sys,json;d=json.load(sys.stdin);print('CapturePlan' in json.dumps(d))"
./target/debug/lmt visual plan-capture --help
```
Expected: `True`; help text renders with all flags.

- [ ] **Step 3: Commit (if any fixups)** — otherwise done.

---

## Self-Review (against spec §6, CLAUDE.md CLI contract)

- **Coverage:** lmt-app helper (Task 3), adapter fn (Task 1), CLI subcommand (Task 4), CLI E2E happy+refuse-style+envelope (Task 5), Tauri shim (Task 6), DTO + schema dump (Task 2). `docs/agents-cli.md` update → **M3c** (with the HTML card, since the command's side_effect class changes when `--html`/`--out` land).
- **Deviations from spec §6, noted:** (1) `plan-capture` is read-only in M3b → no `gate_destructive`/`--dry-run`/`output_exists`; those arrive with `--out`/`--html` file-writing in M3c. (2) No new error code — `invalid_input` covers bad args/geometry (matches existing `error_codes`).
- **Type consistency:** adapter ipc mirror ↔ lmt-shared DTO have identical field names/types; lmt-app converts 1:1; `pass`/`Option<f64>` (null p95) consistent across all three layers.

---

## Execution Handoff

**M3c** — self-contained HTML card (`lmt-app` pure fn `render_capture_card(&CapturePlan, screen geometry) -> String`: SVG plan view + elevation heatmap + station table), `--html`/`--out` writing on the CLI (then it becomes `write_safe`→gated, add `--yes`/`--dry-run`), `docs/agents-cli.md` row + side_effect, final contract self-check. **M4** — curved self-occlusion (visibility (d)) + strong-arc validation.
