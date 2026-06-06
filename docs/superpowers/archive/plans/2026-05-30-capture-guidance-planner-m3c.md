# Capture Guidance Planner — M3c Implementation Plan (HTML guidance card)

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans. Steps use checkbox (`- [ ]`).

**Goal:** Produce the on-site **visual** capture guidance — a self-contained HTML card (top-down plan view + front-elevation coverage heatmap + station table) — exposed as `lmt visual capture-card`, mirroring `total-station instruction-card` exactly (read_only; HTML string in the envelope; raw HTML to stdout in text mode so `... > card.html` works).

**Architecture:** `lmt-shared::dto::CaptureCardResult { html_content }` (mirror `InstructionCardResult`). `lmt-app::visual::render_capture_card(plan, geom) -> String` (pure HTML+inline-SVG; no deps, no CDN) + `run_capture_card(...)` (runs the planner via `run_plan_capture`, reloads geometry, renders). CLI `visual capture-card` subcommand. No new error code; no file writing (stdout only).

**Scope note:** spec §4⑦ + §8 M3. The card uses **2D orthographic inline-SVG** (plan + elevation), not embedded WebGL — self-contained, zero-dependency, printable (the agreed spec §4⑦ decision). Interactive 3D rides with the deferred GUI overlay consuming the same `CapturePlan`. CJK typography per `~/.claude/CLAUDE.md` (sans-serif stack, line-height ≥1.5, `<strong>`/`<em>`).

**Run env:** worktree root; `cargo build/test -p <crate>`; CLI E2E uses the worktree venv sidecar.

---

## File Structure

| File | Change |
| --- | --- |
| `crates/lmt-shared/src/dto.rs` | Add `CaptureCardResult { html_content: String }`. |
| `crates/lmt-shared/src/schema.rs` | `add!("CaptureCardResult", ...)`. |
| `crates/lmt-app/src/visual.rs` | Add `CardGeometry`, `render_capture_card` (pure), `run_capture_card`; reuse `run_plan_capture`. |
| `crates/lmt-cli/src/cli.rs` | Add `VisualCmd::CaptureCard { ... }` (same args as `PlanCapture`). |
| `crates/lmt-cli/src/commands/visual.rs` | Dispatch + `capture_card` fn (read_only; text mode → HTML to stdout, like `instruction_card`). |
| `crates/lmt-cli/tests/cli_e2e.rs` | E2E: HTML on stdout contains expected markers. |
| `docs/agents-cli.md` | Row for `visual capture-card` (read_only). |

---

## Task 1: `CaptureCardResult` DTO + schema

- [ ] **Step 1:** Add to `dto.rs`:
```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CaptureCardResult {
    /// Self-contained HTML (inline SVG, no external deps).
    pub html_content: String,
}
```
- [ ] **Step 2:** `schema.rs`: `add!("CaptureCardResult", dto::CaptureCardResult);` (next to `add!("CapturePlan", ...)`).
- [ ] **Step 3:** `cargo test -p lmt-shared` → PASS.
- [ ] **Step 4:** Commit: `feat(shared): CaptureCardResult DTO + schema`.

---

## Task 2: lmt-app `render_capture_card` + `run_capture_card`

**Files:** `crates/lmt-app/src/visual.rs`

- [ ] **Step 1: Write the failing test** (append to the `#[cfg(test)] mod tests`)

```rust
#[test]
fn render_capture_card_contains_plan_svg_and_table() {
    use lmt_shared::dto::{CabinetCoverage, CapturePlan, CaptureStation, UnreachableRegion};
    let plan = CapturePlan {
        stations: vec![CaptureStation {
            id: "S01".into(), position_mm: [250.0, 250.0, 3000.0],
            look_at_mm: [250.0, 250.0, 0.0], standoff_mm: 3000.0, height_mm: 250.0,
            role: "fan".into(), covers_cabinets: vec![[0, 0]],
        }],
        coverage: vec![
            CabinetCoverage { col: 0, row: 0, p95_residual_mm: Some(1.2), n_views: 4,
                total_observations: 64, reconstructable: true, low_observation: false,
                bridged: true, pass: true },
            CabinetCoverage { col: 1, row: 0, p95_residual_mm: None, n_views: 1,
                total_observations: 16, reconstructable: false, low_observation: false,
                bridged: false, pass: false },
        ],
        unreachable_regions: vec![UnreachableRegion { cabinets: vec![[1, 0]], reason: "x".into() }],
        all_pass: false, target_p95_residual_mm: 3.0,
    };
    let geom = CardGeometry { total_width_mm: 1000.0, total_height_mm: 500.0,
        radius_mm: None, cols: 2, rows: 1 };
    let html = render_capture_card(&plan, &geom, "Demo", "MAIN");
    assert!(html.starts_with("<!DOCTYPE html>"));
    assert!(html.contains("<svg"));               // plan + elevation SVG present
    assert!(html.contains("S01"));                // station listed
    assert!(html.contains("PingFang SC"));        // CJK font stack
    assert!(html.contains("1.2"));                // pass residual rendered
    assert!(html.contains("不可重建") || html.contains("✗"));  // unreconstructable flagged
    assert!(html.matches("<svg").count() >= 2);   // plan + elevation
    // no external deps
    assert!(!html.contains("http://") && !html.contains("https://") && !html.contains("cdn"));
}
```

- [ ] **Step 2:** Run → FAIL (`CardGeometry` / `render_capture_card` undefined).

- [ ] **Step 3: Implement** (insert before the `#[cfg(test)]` module). A pure function building HTML with two inline SVGs (top-down plan with station dots + aim arrows; front elevation grid colored by coverage) + a station table + warnings. Coordinates mapped mm→px with a uniform scale. Color rule: `!reconstructable || !bridged` → red `#c62828`; `!pass` → orange `#ef6c00`; `low_observation` → amber `#f9a825`; else green `#2e7d32`. Includes `html_escape` (or reuse an existing one). `run_capture_card` reuses `run_plan_capture` for the plan, reloads the screen for `CardGeometry`, returns `CaptureCardResult { html_content }`.

  Key signatures:
```rust
pub struct CardGeometry {
    pub total_width_mm: f64,
    pub total_height_mm: f64,
    pub radius_mm: Option<f64>,
    pub cols: u32,
    pub rows: u32,
}

pub fn render_capture_card(
    plan: &lmt_shared::dto::CapturePlan,
    geom: &CardGeometry,
    project_name: &str,
    screen_id: &str,
) -> String { /* HTML + inline SVG */ }

#[allow(clippy::too_many_arguments)]
pub fn run_capture_card(
    project_path: &Path, screen_id: &str, image_size: &str,
    hfov_deg: Option<f64>, vfov_deg: Option<f64>, standoff: &str, height: &str,
    target_p95_residual_mm: f64, trials: u32, seed: u32,
) -> LmtResult<lmt_shared::dto::CaptureCardResult>;
```

- [ ] **Step 4:** Run test → PASS. `cargo test -p lmt-app render_capture_card`.
- [ ] **Step 5:** Commit: `feat(app): render_capture_card (plan + elevation SVG) + run_capture_card`.

---

## Task 3: CLI `visual capture-card` + E2E + docs

**Files:** `crates/lmt-cli/src/cli.rs`, `crates/lmt-cli/src/commands/visual.rs`, `crates/lmt-cli/tests/cli_e2e.rs`, `docs/agents-cli.md`

- [ ] **Step 1:** `cli.rs` — add `VisualCmd::CaptureCard { ... }` with the SAME args as `PlanCapture` (project_path, screen_id, --image-size, --hfov-deg/--vfov-deg, --standoff, --height, --target-mm, --trials, --seed). Doc: `side_effect: read_only`.

- [ ] **Step 2:** `commands/visual.rs` — dispatch + fn mirroring `instruction_card`:
```rust
fn capture_card(mode: Mode, /* same params */) -> i32 {
    match lmt_app::visual::run_capture_card(/* ... */) {
        Ok(c) => match mode {
            Mode::Human => { let _ = std::io::stdout().write_all(c.html_content.as_bytes()); exit_codes::OK }
            _ => output::ok(mode, c, |_| {}),
        },
        Err(e) => output::err(mode, ApiError::from(e)),
    }
}
```
(Match the exact `instruction_card` text-mode branch in `commands/total_station.rs:196`; reuse its `exit_codes` import.)

- [ ] **Step 3:** Build: `cargo build -p lmt-cli`.

- [ ] **Step 4:** E2E (append to cli_e2e.rs, reuse `write_min_project`):
```rust
#[test]
fn visual_capture_card_emits_self_contained_html() {
    let tmp = TempDir::new().unwrap();
    let proj = write_min_project(tmp.path());
    let assert = lmt()
        .args(["visual", "capture-card", proj.to_str().unwrap(), "MAIN",
               "--image-size", "1920x1080", "--hfov-deg", "60",
               "--standoff", "2000..4000", "--height", "400..2200", "--trials", "6"])
        .assert().success();
    let html = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(html.starts_with("<!DOCTYPE html>"));
    assert!(html.matches("<svg").count() >= 2);
    assert!(!html.contains("https://") && !html.contains("cdn"));
}
```

- [ ] **Step 5:** Run: `cargo test -p lmt-cli --test cli_e2e visual_capture_card` → PASS.

- [ ] **Step 6:** `docs/agents-cli.md` — add row:
`| lmt visual capture-card <project> <screen_id> <same flags as plan-capture> | read_only | Render the capture plan as a self-contained HTML guidance card (top-down plan view with station positions + aim arrows, front-elevation coverage heatmap, per-station table). HTML to stdout in text mode (... > card.html); --json wraps {html_content}. Runs the same planner as plan-capture. |`

- [ ] **Step 7:** Commit: `feat(cli): visual capture-card subcommand + E2E + docs`.

---

## Task 4: Workspace check + visual eyeball

- [ ] **Step 1:** `cargo test --workspace` → 0 failures.
- [ ] **Step 2:** Render a real card and eyeball it:
  `./target/debug/lmt visual capture-card examples/curved-flat MAIN --image-size 1920x1080 --hfov-deg 55 --standoff 3000..12000 --height 400..3000 --trials 8 > /tmp/card.html` — open and confirm plan/elevation/table render sensibly (stations on an arc in front of the wall; weak cabinets colored).
- [ ] **Step 3:** Contract self-check: `lmt visual capture-card --help`; schema dump contains `CaptureCardResult`.

---

## Self-Review (against spec §4⑦, §8 M3, CLAUDE.md)

- **Coverage:** HTML card (Task 2 render), CLI read_only command mirroring instruction-card (Task 3), DTO+schema (Task 1), E2E (Task 3), docs row (Task 3). 2D-SVG decision per spec §4⑦.
- **No file writing / no new error code** — stdout-only like instruction-card; consistent with the visual branch.
- **Remaining after M3c:** M4 (curved self-occlusion visibility (d) + strong-arc validation). M3 (full outside-exposure) then complete.
