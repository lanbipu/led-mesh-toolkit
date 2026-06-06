# M1 — Total Station Adapter Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 `lmt-adapter-total-station` crate：从全站仪 CSV + 项目 YAML 生成 `lmt_core::MeasuredPoints` + 验证报告 + 现场指示卡（PDF + HTML）。完成后 GUI / CLI 拿到 `MeasuredPoints` 就能直接走 `auto_reconstruct → OutputTarget::export → OBJ`。

**Architecture:** 三阶段管线：
1. **解析** — CSV (instrument-numbered raw points) + YAML (ProjectConfig)
2. **转换** — 用 CSV 前 3 个点构造 `CoordinateFrame`，把所有 raw points 变换到模型坐标系，用 KD-tree 把仪器点号匹配到 grid name，按 `lowest_measurable_row` 用垂直延伸 fabricate fallback 底部点
3. **输出** — `MeasuredPoints` + `AdapterReport` JSON + 指示卡 PDF/HTML

复用 `lmt-core` 的 IR 类型（`MeasuredPoints`、`MeasuredPoint`、`PointSource`、`Uncertainty`、`CoordinateFrame`、`CabinetArray`、`ShapePrior`）+ 通过 workspace dep 共享 `kiddo` / `nalgebra` / `serde`。

**Scope (本 plan = M1.1):** 单一 MAIN screen + 单一 CoordinateFrame。多 screen attribution（FLOOR + 其他屏） 留给 M1.2 增量 plan。这样本 plan 任务数可控、能独立交付。

**Tech Stack:** Rust 1.85, `lmt-core`, `nalgebra`, `kiddo`, `serde` + `serde_yaml` + `serde_json`, `csv = "1"`, `printpdf = "0.7"`, `thiserror`.

**Spec 引用：** `docs/superpowers/specs/2026-05-10-led-mesh-toolkit-design.md`（第 4 节、第 8 节）

**前置条件：** M0.1 已完成（tag `m0.1-complete`，`lmt-core` 公共 API 冻结）。

---

## Phase 1 — Crate 准备

### Task 1: 升级 adapter-total-station Cargo.toml + 清空 placeholder

**Files:**
- Modify: `crates/adapter-total-station/Cargo.toml`
- Modify: `crates/adapter-total-station/src/lib.rs`

- [ ] **Step 1: 验证起点**

```bash
cd /Users/bip.lan/AIWorkspace/vp/led-mesh-toolkit
git status
. "$HOME/.cargo/env" && cargo build --workspace
```

预期：working tree clean；workspace build 成功。

- [ ] **Step 2: 改写 `crates/adapter-total-station/Cargo.toml`**

```toml
[package]
name = "lmt-adapter-total-station"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
authors.workspace = true
license.workspace = true
description = "Total-station CSV adapter for LMT (M1)"

[dependencies]
lmt-core = { path = "../core" }
nalgebra.workspace = true
kiddo.workspace = true
serde.workspace = true
serde_yaml.workspace = true
serde_json.workspace = true
thiserror.workspace = true
csv = "1"
printpdf = "0.7"

[dev-dependencies]
insta.workspace = true
pretty_assertions.workspace = true
tempfile = "3"
```

- [ ] **Step 3: 清空 `crates/adapter-total-station/src/lib.rs`**

```rust
//! Total-station CSV adapter (M1).
//!
//! Reads instrument-numbered CSV from a Trimble / Leica total station,
//! a project YAML config, and produces `lmt_core::MeasuredPoints` ready
//! for reconstruction + export, plus a JSON validation report and a
//! field instruction card (PDF + HTML).

pub mod error;

pub use error::AdapterError;
```

- [ ] **Step 4: 验证编译**

```bash
. "$HOME/.cargo/env" && cargo check -p lmt-adapter-total-station
```

预期：`error[E0583]: file not found for module 'error'` —— Task 2 才创建 `error.rs`。这步只验证 Cargo.toml deps 解析 OK。

- [ ] **Step 5: 临时绕开缺 module，跑 workspace check 验证 Cargo.toml 依赖正确**

把 `pub mod error;` 和 `pub use error::AdapterError;` 暂时注释掉（行首加 `//`）：

```rust
//! Total-station CSV adapter (M1).
//!
//! Reads instrument-numbered CSV from a Trimble / Leica total station,
//! a project YAML config, and produces `lmt_core::MeasuredPoints` ready
//! for reconstruction + export, plus a JSON validation report and a
//! field instruction card (PDF + HTML).

// pub mod error;
//
// pub use error::AdapterError;
```

```bash
. "$HOME/.cargo/env" && cargo check --workspace
```

预期：clean。然后**取消注释**（恢复 `pub mod error;` + `pub use error::AdapterError;`）。

- [ ] **Step 6: 提交（不含取消注释 — 只提交带注释的最小骨架版本，让 commit 始终可编译）**

把两行恢复成注释状态再提交：

```rust
// pub mod error;        ← keep commented
// pub use error::AdapterError;
```

```bash
git add crates/adapter-total-station/Cargo.toml crates/adapter-total-station/src/lib.rs Cargo.lock
git commit -m "feat(adapter-ts): bump dependencies for CSV/YAML/PDF/csv"
```

> **Note**：之所以以注释形式 commit `pub mod error;`，是因为 Task 2 才创建 error.rs；commit 顺序保持每步可编译。Task 2 会取消注释。

---

### Task 2: `AdapterError` 类型

**Files:**
- Create: `crates/adapter-total-station/src/error.rs`
- Modify: `crates/adapter-total-station/src/lib.rs` (uncomment `pub mod error`)
- Create: `crates/adapter-total-station/tests/error_test.rs`

- [ ] **Step 1: 写失败测试**

`crates/adapter-total-station/tests/error_test.rs`:

```rust
use lmt_adapter_total_station::AdapterError;

#[test]
fn adapter_error_displays_io_variant() {
    let io = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
    let e: AdapterError = io.into();
    let s = format!("{e}");
    assert!(s.contains("io error"));
    assert!(s.contains("file missing"));
}

#[test]
fn adapter_error_carries_invalid_input_detail() {
    let e = AdapterError::InvalidInput("bad column header".into());
    let s = format!("{e}");
    assert!(s.contains("bad column header"));
}

#[test]
fn adapter_error_wraps_core_error() {
    let core = lmt_core::CoreError::InvalidInput("origin coincides".into());
    let e: AdapterError = core.into();
    let s = format!("{e}");
    assert!(s.contains("core error"));
    assert!(s.contains("origin coincides"));
}
```

- [ ] **Step 2: 跑测试确认 fail**

```bash
. "$HOME/.cargo/env" && cargo test -p lmt-adapter-total-station --test error_test
```

预期：FAIL（unresolved import）。

- [ ] **Step 3: 实现 `error.rs`**

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("csv parse error: {0}")]
    Csv(#[from] csv::Error),

    #[error("yaml parse error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("core error: {0}")]
    Core(#[from] lmt_core::CoreError),

    #[error("pdf generation: {0}")]
    Pdf(String),
}
```

- [ ] **Step 4: 取消 lib.rs 中 `pub mod error;` 注释**

`crates/adapter-total-station/src/lib.rs`:

```rust
//! Total-station CSV adapter (M1).

pub mod error;

pub use error::AdapterError;
```

- [ ] **Step 5: 验证 + commit**

```bash
. "$HOME/.cargo/env" && cargo test -p lmt-adapter-total-station --test error_test
. "$HOME/.cargo/env" && cargo clippy -p lmt-adapter-total-station --all-targets -- -D warnings
git add crates/adapter-total-station/src/error.rs crates/adapter-total-station/src/lib.rs crates/adapter-total-station/tests/error_test.rs
git commit -m "feat(adapter-ts): add AdapterError type with thiserror"
```

预期：3 tests pass, clippy clean。

---

## Phase 2 — 数据结构

### Task 3: `RawPoint` 类型

**Files:**
- Create: `crates/adapter-total-station/src/raw_point.rs`
- Modify: `crates/adapter-total-station/src/lib.rs`
- Create: `crates/adapter-total-station/tests/raw_point_test.rs`

- [ ] **Step 1: 写失败测试**

```rust
use lmt_adapter_total_station::raw_point::RawPoint;
use nalgebra::Vector3;

#[test]
fn raw_point_construction_holds_fields() {
    let p = RawPoint {
        instrument_id: 7,
        position_mm: Vector3::new(1234.5, 5678.9, 12345.0),
        note: Some("origin marker".into()),
    };
    assert_eq!(p.instrument_id, 7);
    assert_eq!(p.position_mm.x, 1234.5);
    assert_eq!(p.note.as_deref(), Some("origin marker"));
}

#[test]
fn raw_point_position_meters_converts_from_mm() {
    let p = RawPoint {
        instrument_id: 1,
        position_mm: Vector3::new(1000.0, 2000.0, 3000.0),
        note: None,
    };
    let m = p.position_meters();
    assert!((m.x - 1.0).abs() < 1e-9);
    assert!((m.y - 2.0).abs() < 1e-9);
    assert!((m.z - 3.0).abs() < 1e-9);
}
```

- [ ] **Step 2: 跑测试确认 fail**

```bash
. "$HOME/.cargo/env" && cargo test -p lmt-adapter-total-station --test raw_point_test
```

- [ ] **Step 3: 实现 `raw_point.rs`**

```rust
use nalgebra::Vector3;

/// One row from the total-station CSV export (instrument coordinates, mm).
///
/// `instrument_id` is the auto-incremented point number assigned by the
/// instrument (e.g. Trimble Access). Per the field SOP, the first 3 points
/// are the user-selected reference markers (origin / X-axis / XY-plane).
#[derive(Debug, Clone)]
pub struct RawPoint {
    pub instrument_id: u32,
    /// Position in instrument frame, **millimeters**.
    pub position_mm: Vector3<f64>,
    pub note: Option<String>,
}

impl RawPoint {
    /// Convert position to meters (matches `lmt-core` IR convention).
    pub fn position_meters(&self) -> Vector3<f64> {
        self.position_mm * 0.001
    }
}
```

加 `pub mod raw_point;` 到 `lib.rs`：

```rust
//! Total-station CSV adapter (M1).

pub mod error;
pub mod raw_point;

pub use error::AdapterError;
pub use raw_point::RawPoint;
```

- [ ] **Step 4: 验证 + commit**

```bash
. "$HOME/.cargo/env" && cargo test -p lmt-adapter-total-station --test raw_point_test
. "$HOME/.cargo/env" && cargo clippy -p lmt-adapter-total-station --all-targets -- -D warnings
git add crates/adapter-total-station/src/raw_point.rs crates/adapter-total-station/src/lib.rs crates/adapter-total-station/tests/raw_point_test.rs
git commit -m "feat(adapter-ts): add RawPoint type with mm→m conversion"
```

---

### Task 4: `ProjectConfig` YAML 类型

**Files:**
- Create: `crates/adapter-total-station/src/project.rs`
- Modify: `crates/adapter-total-station/src/lib.rs`
- Create: `crates/adapter-total-station/tests/project_test.rs`

- [ ] **Step 1: 写失败测试**

```rust
use lmt_adapter_total_station::project::{
    BottomCompletion, FallbackMethod, ProjectConfig, ScreenConfig, ShapePriorConfig,
};

#[test]
fn project_config_round_trips_curved_screen() {
    let yaml = r#"
project:
  name: Studio_A_Volume
screens:
  MAIN:
    cabinet_count: [120, 20]
    cabinet_size_mm: [500, 500]
    shape_prior:
      type: curved
      radius_mm: 30000
    bottom_completion:
      lowest_measurable_row: 5
      fallback_method: vertical
coordinate_system:
  origin_grid_name: MAIN_V001_R005
  x_axis_grid_name: MAIN_V120_R005
  xy_plane_grid_name: MAIN_V001_R020
"#;
    let cfg: ProjectConfig = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(cfg.project.name, "Studio_A_Volume");
    let main = cfg.screens.get("MAIN").unwrap();
    assert_eq!(main.cabinet_count, [120, 20]);
    assert_eq!(main.cabinet_size_mm, [500.0, 500.0]);
    match &main.shape_prior {
        ShapePriorConfig::Curved { radius_mm } => assert_eq!(*radius_mm, 30000.0),
        _ => panic!("expected Curved"),
    }
    let bc = main.bottom_completion.as_ref().unwrap();
    assert_eq!(bc.lowest_measurable_row, 5);
    assert!(matches!(bc.fallback_method, FallbackMethod::Vertical));
    assert_eq!(cfg.coordinate_system.origin_grid_name, "MAIN_V001_R005");
}

#[test]
fn project_config_flat_no_bottom_completion() {
    let yaml = r#"
project:
  name: TestFlat
screens:
  MAIN:
    cabinet_count: [4, 2]
    cabinet_size_mm: [500, 500]
    shape_prior:
      type: flat
coordinate_system:
  origin_grid_name: MAIN_V001_R001
  x_axis_grid_name: MAIN_V005_R001
  xy_plane_grid_name: MAIN_V001_R003
"#;
    let cfg: ProjectConfig = serde_yaml::from_str(yaml).unwrap();
    let main = cfg.screens.get("MAIN").unwrap();
    assert!(matches!(main.shape_prior, ShapePriorConfig::Flat));
    assert!(main.bottom_completion.is_none());
}
```

- [ ] **Step 2: 跑测试确认 fail**

```bash
. "$HOME/.cargo/env" && cargo test -p lmt-adapter-total-station --test project_test
```

- [ ] **Step 3: 实现 `project.rs`**

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub project: ProjectMeta,
    pub screens: HashMap<String, ScreenConfig>,
    pub coordinate_system: CoordinateSystemConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMeta {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenConfig {
    /// `[cols, rows]` in cabinets.
    pub cabinet_count: [u32; 2],
    /// Single cabinet `[width_mm, height_mm]`.
    pub cabinet_size_mm: [f64; 2],
    pub shape_prior: ShapePriorConfig,
    /// `None` → no bottom occlusion (lowest row is R001).
    #[serde(default)]
    pub bottom_completion: Option<BottomCompletion>,
    /// Cells absent in irregular shapes; `(col, row)` 0-based.
    #[serde(default)]
    pub absent_cells: Vec<(u32, u32)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum ShapePriorConfig {
    Flat,
    Curved {
        radius_mm: f64,
    },
    Folded {
        fold_seam_columns: Vec<u32>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BottomCompletion {
    pub lowest_measurable_row: u32,
    pub fallback_method: FallbackMethod,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FallbackMethod {
    /// R<lowest-1>..R001 = R<lowest>.position − k×cabinet_height (vertical extension).
    Vertical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinateSystemConfig {
    pub origin_grid_name: String,
    pub x_axis_grid_name: String,
    pub xy_plane_grid_name: String,
}
```

加 `pub mod project;` 到 `lib.rs`。

- [ ] **Step 4: 验证 + commit**

```bash
. "$HOME/.cargo/env" && cargo test -p lmt-adapter-total-station --test project_test
. "$HOME/.cargo/env" && cargo clippy -p lmt-adapter-total-station --all-targets -- -D warnings
git add crates/adapter-total-station/src/project.rs crates/adapter-total-station/src/lib.rs crates/adapter-total-station/tests/project_test.rs
git commit -m "feat(adapter-ts): add ProjectConfig YAML schema"
```

---

### Task 5: `AdapterReport` 类型

**Files:**
- Create: `crates/adapter-total-station/src/report.rs`
- Modify: `crates/adapter-total-station/src/lib.rs`
- Create: `crates/adapter-total-station/tests/report_test.rs`

- [ ] **Step 1: 写失败测试**

```rust
use lmt_adapter_total_station::report::{
    AdapterReport, AmbiguousMatch, MissingPoint, OutlierPoint, ScreenReport,
};

#[test]
fn screen_report_serializes_to_json() {
    let r = ScreenReport {
        screen_id: "MAIN".into(),
        expected_count: 277,
        measured_count: 273,
        fabricated_count: 0,
        missing: vec![MissingPoint { name: "MAIN_V015_R020".into() }],
        outliers: vec![OutlierPoint {
            instrument_id: 42,
            distance_to_nearest_mm: 87.3,
            nearest_grid_name: "MAIN_V010_R005".into(),
        }],
        ambiguous: vec![AmbiguousMatch {
            instrument_id: 51,
            candidates: vec!["MAIN_V005_R005".into(), "MAIN_V006_R005".into()],
        }],
        warnings: vec!["Top edge has 4 missing points".into()],
        estimated_rms_mm: 4.5,
    };
    let s = serde_json::to_string_pretty(&r).unwrap();
    assert!(s.contains("\"expected_count\": 277"));
    assert!(s.contains("MAIN_V015_R020"));
    assert!(s.contains("\"distance_to_nearest_mm\": 87.3"));
}

#[test]
fn adapter_report_contains_screens() {
    let r = AdapterReport {
        project_name: "Studio_A".into(),
        screens: vec![ScreenReport {
            screen_id: "MAIN".into(),
            expected_count: 4,
            measured_count: 4,
            fabricated_count: 0,
            missing: vec![],
            outliers: vec![],
            ambiguous: vec![],
            warnings: vec![],
            estimated_rms_mm: 2.0,
        }],
    };
    let s = serde_json::to_string(&r).unwrap();
    assert!(s.contains("\"project_name\":\"Studio_A\""));
    assert!(s.contains("\"screen_id\":\"MAIN\""));
}
```

- [ ] **Step 2: 跑测试确认 fail**

```bash
. "$HOME/.cargo/env" && cargo test -p lmt-adapter-total-station --test report_test
```

- [ ] **Step 3: 实现 `report.rs`**

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterReport {
    pub project_name: String,
    pub screens: Vec<ScreenReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenReport {
    pub screen_id: String,
    /// (cols+1) × (rows+1) total grid vertices expected for this screen.
    pub expected_count: usize,
    /// Number of grid vertices populated from CSV measurements (excludes fabricated).
    pub measured_count: usize,
    /// Number of grid vertices fabricated via bottom-occlusion fallback.
    pub fabricated_count: usize,
    /// Grid names that were neither measured nor fabricated.
    pub missing: Vec<MissingPoint>,
    /// Raw points whose nearest expected position is too far (likely a stray / wrong screen).
    pub outliers: Vec<OutlierPoint>,
    /// Raw points that match two or more expected positions within the tolerance.
    pub ambiguous: Vec<AmbiguousMatch>,
    pub warnings: Vec<String>,
    /// Aggregate uncertainty estimate (mm). Computed as RMS of input point sigmas.
    pub estimated_rms_mm: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissingPoint {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutlierPoint {
    pub instrument_id: u32,
    pub distance_to_nearest_mm: f64,
    pub nearest_grid_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmbiguousMatch {
    pub instrument_id: u32,
    /// Two or more grid names within the matching tolerance.
    pub candidates: Vec<String>,
}
```

加 `pub mod report;` 到 `lib.rs`。

- [ ] **Step 4: 验证 + commit**

```bash
. "$HOME/.cargo/env" && cargo test -p lmt-adapter-total-station --test report_test
. "$HOME/.cargo/env" && cargo clippy -p lmt-adapter-total-station --all-targets -- -D warnings
git add crates/adapter-total-station/src/report.rs crates/adapter-total-station/src/lib.rs crates/adapter-total-station/tests/report_test.rs
git commit -m "feat(adapter-ts): add AdapterReport / ScreenReport JSON schema"
```

---

## Phase 3 — 解析

### Task 6: CSV parser

**Files:**
- Create: `crates/adapter-total-station/src/csv_parser.rs`
- Modify: `crates/adapter-total-station/src/lib.rs`
- Create: `crates/adapter-total-station/tests/csv_test.rs`
- Create: `crates/adapter-total-station/tests/fixtures/sample.csv`

- [ ] **Step 1: 创建 CSV fixture**

```bash
mkdir -p crates/adapter-total-station/tests/fixtures
```

写 `crates/adapter-total-station/tests/fixtures/sample.csv`：

```
name,x,y,z,note
1,1234.5,5678.9,12345.6,
2,31234.5,5678.9,12340.0,
3,1234.5,5678.9,2345.6,origin marker
4,1734.5,5680.0,12345.5,
5,2234.5,5685.0,12345.4,
```

- [ ] **Step 2: 写失败测试**

`crates/adapter-total-station/tests/csv_test.rs`:

```rust
use lmt_adapter_total_station::csv_parser::parse_csv;
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests");
    p.push("fixtures");
    p.push(name);
    p
}

#[test]
fn parse_csv_returns_5_points_in_instrument_order() {
    let raw = parse_csv(&fixture("sample.csv")).unwrap();
    assert_eq!(raw.len(), 5);
    assert_eq!(raw[0].instrument_id, 1);
    assert_eq!(raw[1].instrument_id, 2);
    assert_eq!(raw[4].instrument_id, 5);
    assert!((raw[0].position_mm.x - 1234.5).abs() < 1e-9);
    assert!((raw[1].position_mm.x - 31234.5).abs() < 1e-9);
    assert_eq!(raw[2].note.as_deref(), Some("origin marker"));
    assert_eq!(raw[0].note, None);
}

#[test]
fn parse_csv_rejects_missing_file() {
    let result = parse_csv(&fixture("does-not-exist.csv"));
    assert!(result.is_err());
}

#[test]
fn parse_csv_rejects_non_numeric_id() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("bad.csv");
    std::fs::write(&p, "name,x,y,z,note\nabc,1.0,2.0,3.0,\n").unwrap();
    let result = parse_csv(&p);
    assert!(result.is_err());
}

#[test]
fn parse_csv_rejects_non_finite_coordinate() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("nan.csv");
    std::fs::write(&p, "name,x,y,z,note\n1,nan,0,0,\n").unwrap();
    let result = parse_csv(&p);
    assert!(result.is_err());
}
```

- [ ] **Step 3: 跑测试确认 fail**

```bash
. "$HOME/.cargo/env" && cargo test -p lmt-adapter-total-station --test csv_test
```

- [ ] **Step 4: 实现 `csv_parser.rs`**

```rust
use std::path::Path;

use nalgebra::Vector3;
use serde::Deserialize;

use crate::error::AdapterError;
use crate::raw_point::RawPoint;

#[derive(Debug, Deserialize)]
struct CsvRow {
    name: String,
    x: f64,
    y: f64,
    z: f64,
    #[serde(default)]
    note: String,
}

/// Parse a Trimble/Leica-style CSV export into raw points (mm).
///
/// Required columns: `name,x,y,z,note` (note may be empty).
/// `name` is parsed as a `u32` instrument id; the field SOP requires
/// the instrument to assign sequential numeric ids.
pub fn parse_csv(path: &Path) -> Result<Vec<RawPoint>, AdapterError> {
    let mut rdr = csv::Reader::from_path(path)?;
    let mut out = Vec::new();

    for row in rdr.deserialize() {
        let row: CsvRow = row?;
        let instrument_id: u32 = row.name.trim().parse().map_err(|e| {
            AdapterError::InvalidInput(format!(
                "expected numeric instrument id, got {:?}: {e}",
                row.name
            ))
        })?;
        if !row.x.is_finite() || !row.y.is_finite() || !row.z.is_finite() {
            return Err(AdapterError::InvalidInput(format!(
                "non-finite coordinate on point id {instrument_id}: ({}, {}, {})",
                row.x, row.y, row.z
            )));
        }
        let note = if row.note.trim().is_empty() {
            None
        } else {
            Some(row.note.trim().to_string())
        };
        out.push(RawPoint {
            instrument_id,
            position_mm: Vector3::new(row.x, row.y, row.z),
            note,
        });
    }

    Ok(out)
}
```

加 `pub mod csv_parser;` 到 `lib.rs`。

- [ ] **Step 5: 验证 + commit**

```bash
. "$HOME/.cargo/env" && cargo test -p lmt-adapter-total-station --test csv_test
. "$HOME/.cargo/env" && cargo clippy -p lmt-adapter-total-station --all-targets -- -D warnings
git add crates/adapter-total-station/src/csv_parser.rs crates/adapter-total-station/src/lib.rs crates/adapter-total-station/tests/csv_test.rs crates/adapter-total-station/tests/fixtures/sample.csv
git commit -m "feat(adapter-ts): add CSV parser with finite-coord + numeric-id validation"
```

---

### Task 7: YAML project parser

**Files:**
- Create: `crates/adapter-total-station/src/project_loader.rs`
- Modify: `crates/adapter-total-station/src/lib.rs`
- Modify: `crates/adapter-total-station/tests/project_test.rs`
- Create: `crates/adapter-total-station/tests/fixtures/sample_project.yaml`

- [ ] **Step 1: 创建 YAML fixture**

`crates/adapter-total-station/tests/fixtures/sample_project.yaml`:

```yaml
project:
  name: Studio_A_Volume
screens:
  MAIN:
    cabinet_count: [4, 2]
    cabinet_size_mm: [500, 500]
    shape_prior:
      type: flat
coordinate_system:
  origin_grid_name: MAIN_V001_R001
  x_axis_grid_name: MAIN_V005_R001
  xy_plane_grid_name: MAIN_V001_R003
```

- [ ] **Step 2: 追加测试到 `tests/project_test.rs`**

```rust
#[test]
fn load_project_from_path() {
    use lmt_adapter_total_station::project_loader::load_project;
    use std::path::PathBuf;

    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests/fixtures/sample_project.yaml");
    let cfg = load_project(&p).unwrap();
    assert_eq!(cfg.project.name, "Studio_A_Volume");
    assert!(cfg.screens.contains_key("MAIN"));
}
```

- [ ] **Step 3: 跑测试确认 fail**

```bash
. "$HOME/.cargo/env" && cargo test -p lmt-adapter-total-station --test project_test
```

- [ ] **Step 4: 实现 `project_loader.rs`**

```rust
use std::path::Path;

use crate::error::AdapterError;
use crate::project::ProjectConfig;

pub fn load_project(path: &Path) -> Result<ProjectConfig, AdapterError> {
    let s = std::fs::read_to_string(path)?;
    let cfg: ProjectConfig = serde_yaml::from_str(&s)?;
    Ok(cfg)
}
```

加 `pub mod project_loader;` 到 `lib.rs`。

- [ ] **Step 5: 验证 + commit**

```bash
. "$HOME/.cargo/env" && cargo test -p lmt-adapter-total-station --test project_test
. "$HOME/.cargo/env" && cargo clippy -p lmt-adapter-total-station --all-targets -- -D warnings
git add crates/adapter-total-station/src/project_loader.rs crates/adapter-total-station/src/lib.rs crates/adapter-total-station/tests/project_test.rs crates/adapter-total-station/tests/fixtures/sample_project.yaml
git commit -m "feat(adapter-ts): add YAML project loader from path"
```

---

## Phase 4 — 算法

### Task 8: 3 参考点 → `CoordinateFrame`

**Files:**
- Create: `crates/adapter-total-station/src/reference_frame.rs`
- Modify: `crates/adapter-total-station/src/lib.rs`
- Create: `crates/adapter-total-station/tests/reference_frame_test.rs`

- [ ] **Step 1: 写失败测试**

```rust
use lmt_adapter_total_station::raw_point::RawPoint;
use lmt_adapter_total_station::reference_frame::build_frame_from_first_three;
use nalgebra::Vector3;

fn rp(id: u32, x: f64, y: f64, z: f64) -> RawPoint {
    RawPoint {
        instrument_id: id,
        position_mm: Vector3::new(x, y, z),
        note: None,
    }
}

#[test]
fn build_frame_uses_first_three_points_in_meters() {
    // origin at (10000mm, 10000mm, 10000mm) = (10m, 10m, 10m)
    // x_axis_ref at (12000, 10000, 10000) → +X 2m away
    // xy_plane_ref at (10000, 10000, 13000) → up = +Y after Gram-Schmidt
    let raw = vec![
        rp(1, 10000.0, 10000.0, 10000.0),
        rp(2, 12000.0, 10000.0, 10000.0),
        rp(3, 10000.0, 10000.0, 13000.0),
        rp(4, 99999.0, 0.0, 0.0), // ignored
    ];
    let frame = build_frame_from_first_three(&raw).unwrap();

    let origin_in_model = frame.world_to_model(&Vector3::new(10.0, 10.0, 10.0));
    assert!(origin_in_model.norm() < 1e-9);

    let x_in_model = frame.world_to_model(&Vector3::new(12.0, 10.0, 10.0));
    assert!((x_in_model - Vector3::new(2.0, 0.0, 0.0)).norm() < 1e-9);
}

#[test]
fn build_frame_rejects_fewer_than_three_points() {
    let raw = vec![rp(1, 0.0, 0.0, 0.0), rp(2, 1.0, 0.0, 0.0)];
    let result = build_frame_from_first_three(&raw);
    assert!(result.is_err());
}

#[test]
fn build_frame_rejects_collinear_first_three() {
    let raw = vec![
        rp(1, 0.0, 0.0, 0.0),
        rp(2, 1000.0, 0.0, 0.0),
        rp(3, 2000.0, 0.0, 0.0),
        rp(4, 0.0, 0.0, 1000.0),
    ];
    let result = build_frame_from_first_three(&raw);
    assert!(result.is_err());
}

#[test]
fn build_frame_requires_instrument_ids_to_be_first_three() {
    // Plan SOP: first 3 points (by instrument_id 1, 2, 3) are reference markers.
    // If the input is not sorted, function should error.
    let raw = vec![
        rp(2, 0.0, 0.0, 0.0),
        rp(1, 1000.0, 0.0, 0.0),
        rp(3, 0.0, 0.0, 1000.0),
    ];
    let result = build_frame_from_first_three(&raw);
    assert!(result.is_err());
}
```

- [ ] **Step 2: 跑测试确认 fail**

```bash
. "$HOME/.cargo/env" && cargo test -p lmt-adapter-total-station --test reference_frame_test
```

- [ ] **Step 3: 实现 `reference_frame.rs`**

```rust
use lmt_core::coordinate::CoordinateFrame;
use lmt_core::CoreError;

use crate::error::AdapterError;
use crate::raw_point::RawPoint;

/// Use the first 3 raw points (by SOP: instrument_ids 1, 2, 3) as
/// origin / X-axis-ref / XY-plane-ref to construct a `CoordinateFrame`.
///
/// Errors if `raw.len() < 3` or if the first three points have ids
/// other than 1, 2, 3 in order, or if `from_three_points` rejects
/// (collinear / coincident).
pub fn build_frame_from_first_three(raw: &[RawPoint]) -> Result<CoordinateFrame, AdapterError> {
    if raw.len() < 3 {
        return Err(AdapterError::InvalidInput(format!(
            "need at least 3 raw points, got {}",
            raw.len()
        )));
    }
    if raw[0].instrument_id != 1 || raw[1].instrument_id != 2 || raw[2].instrument_id != 3 {
        return Err(AdapterError::InvalidInput(format!(
            "first 3 raw points must have instrument_ids 1, 2, 3 in order; \
             got [{}, {}, {}]",
            raw[0].instrument_id, raw[1].instrument_id, raw[2].instrument_id
        )));
    }

    // Convert mm→m before handing to lmt-core (which works in meters).
    let origin = raw[0].position_meters();
    let x_axis = raw[1].position_meters();
    let xy_plane = raw[2].position_meters();

    let frame = CoordinateFrame::from_three_points(origin, x_axis, xy_plane)
        .map_err(|e: CoreError| AdapterError::Core(e))?;
    Ok(frame)
}
```

加 `pub mod reference_frame;` 到 `lib.rs`。

- [ ] **Step 4: 验证 + commit**

```bash
. "$HOME/.cargo/env" && cargo test -p lmt-adapter-total-station --test reference_frame_test
. "$HOME/.cargo/env" && cargo clippy -p lmt-adapter-total-station --all-targets -- -D warnings
git add crates/adapter-total-station/src/reference_frame.rs crates/adapter-total-station/src/lib.rs crates/adapter-total-station/tests/reference_frame_test.rs
git commit -m "feat(adapter-ts): build CoordinateFrame from first 3 raw points"
```

---

### Task 9: 全局坐标变换

**Files:**
- Create: `crates/adapter-total-station/src/transform.rs`
- Modify: `crates/adapter-total-station/src/lib.rs`
- Create: `crates/adapter-total-station/tests/transform_test.rs`

- [ ] **Step 1: 写失败测试**

```rust
use lmt_adapter_total_station::raw_point::RawPoint;
use lmt_adapter_total_station::reference_frame::build_frame_from_first_three;
use lmt_adapter_total_station::transform::transform_to_model;
use nalgebra::Vector3;

fn rp(id: u32, x: f64, y: f64, z: f64) -> RawPoint {
    RawPoint {
        instrument_id: id,
        position_mm: Vector3::new(x, y, z),
        note: None,
    }
}

#[test]
fn transform_returns_one_position_per_input_point() {
    let raw = vec![
        rp(1, 0.0, 0.0, 0.0),
        rp(2, 1000.0, 0.0, 0.0),
        rp(3, 0.0, 0.0, 1000.0),
        rp(4, 500.0, 0.0, 500.0),
    ];
    let frame = build_frame_from_first_three(&raw).unwrap();
    let model = transform_to_model(&raw, &frame);
    assert_eq!(model.len(), 4);
    // Origin reference point (id=1) → (0, 0, 0)
    assert!(model[0].1.norm() < 1e-9);
    // X-axis reference (id=2) → (1, 0, 0)
    assert!((model[1].1 - Vector3::new(1.0, 0.0, 0.0)).norm() < 1e-9);
    // pair preserves instrument id
    assert_eq!(model[3].0, 4);
}
```

- [ ] **Step 2: 跑测试确认 fail**

```bash
. "$HOME/.cargo/env" && cargo test -p lmt-adapter-total-station --test transform_test
```

- [ ] **Step 3: 实现 `transform.rs`**

```rust
use nalgebra::Vector3;

use lmt_core::coordinate::CoordinateFrame;

use crate::raw_point::RawPoint;

/// Apply `frame.world_to_model` (with mm→m conversion) to every raw point.
/// Returns `(instrument_id, model_position)` pairs in input order.
pub fn transform_to_model(
    raw: &[RawPoint],
    frame: &CoordinateFrame,
) -> Vec<(u32, Vector3<f64>)> {
    raw.iter()
        .map(|p| (p.instrument_id, frame.world_to_model(&p.position_meters())))
        .collect()
}
```

加 `pub mod transform;` 到 `lib.rs`。

- [ ] **Step 4: 验证 + commit**

```bash
. "$HOME/.cargo/env" && cargo test -p lmt-adapter-total-station --test transform_test
. "$HOME/.cargo/env" && cargo clippy -p lmt-adapter-total-station --all-targets -- -D warnings
git add crates/adapter-total-station/src/transform.rs crates/adapter-total-station/src/lib.rs crates/adapter-total-station/tests/transform_test.rs
git commit -m "feat(adapter-ts): apply world→model transform to all raw points"
```

---

### Task 10: Shape grid expected positions

**Files:**
- Create: `crates/adapter-total-station/src/shape_grid.rs`
- Modify: `crates/adapter-total-station/src/lib.rs`
- Create: `crates/adapter-total-station/tests/shape_grid_test.rs`

- [ ] **Step 1: 写失败测试**

```rust
use lmt_adapter_total_station::project::{
    BottomCompletion, FallbackMethod, ScreenConfig, ShapePriorConfig,
};
use lmt_adapter_total_station::shape_grid::{
    expected_grid_positions, GridExpected,
};
use nalgebra::Vector3;

fn flat_screen(cols: u32, rows: u32) -> ScreenConfig {
    ScreenConfig {
        cabinet_count: [cols, rows],
        cabinet_size_mm: [500.0, 500.0],
        shape_prior: ShapePriorConfig::Flat,
        bottom_completion: None,
        absent_cells: vec![],
    }
}

#[test]
fn flat_4x2_grid_yields_15_expected_positions() {
    let cfg = flat_screen(4, 2);
    let grid = expected_grid_positions("MAIN", &cfg).unwrap();
    // (cols+1) × (rows+1) = 5 × 3 = 15
    assert_eq!(grid.len(), 15);

    // Bottom-left V001_R001 at (0, 0, 0) (origin assumed at lowest-left)
    let bl = grid.iter().find(|g| g.name == "MAIN_V001_R001").unwrap();
    assert!((bl.model_position - Vector3::new(0.0, 0.0, 0.0)).norm() < 1e-9);

    // Top-right V005_R003 at (2.0, 0, 1.0) for 4 col × 2 row × 0.5m cabinets
    let tr = grid.iter().find(|g| g.name == "MAIN_V005_R003").unwrap();
    assert!((tr.model_position - Vector3::new(2.0, 0.0, 1.0)).norm() < 1e-9);
}

#[test]
fn flat_grid_skips_absent_cells_neighborhood_keeps_corner() {
    let mut cfg = flat_screen(3, 3);
    // Mark center cabinet (1,1) absent. Its 4 corner vertices are still
    // present because each is shared with at least one present cabinet.
    cfg.absent_cells = vec![(1, 1)];
    let grid = expected_grid_positions("MAIN", &cfg).unwrap();
    // 4×4 = 16 vertices; one absent cabinet doesn't remove any vertex
    // (corners are shared). expected_grid_positions returns ALL grid
    // vertex positions; absent cabinets are reflected by absent_cells in
    // CabinetArray downstream — not by removing grid vertices here.
    assert_eq!(grid.len(), 16);
}

#[test]
fn curved_3x1_grid_arcs_along_x() {
    // Half-cylinder with very large radius behaves nearly flat — sanity check.
    let cfg = ScreenConfig {
        cabinet_count: [3, 1],
        cabinet_size_mm: [500.0, 500.0],
        shape_prior: ShapePriorConfig::Curved {
            radius_mm: 100_000.0, // 100m radius — gentle arc
        },
        bottom_completion: None,
        absent_cells: vec![],
    };
    let grid = expected_grid_positions("MAIN", &cfg).unwrap();
    assert_eq!(grid.len(), 8); // 4 × 2

    let bl = grid.iter().find(|g| g.name == "MAIN_V001_R001").unwrap();
    let br = grid.iter().find(|g| g.name == "MAIN_V004_R001").unwrap();
    // Arc length from V001 to V004 should be 3 × 0.5 = 1.5m;
    // chord length is slightly less than 1.5m for a 100m-radius arc.
    let chord = (br.model_position - bl.model_position).norm();
    assert!(chord < 1.5);
    assert!(chord > 1.499);
}

#[test]
fn bottom_completion_does_not_change_grid_size() {
    // The grid is always (cols+1)×(rows+1); bottom completion only
    // affects which rows are "fabricated" downstream — see fallback.rs.
    let cfg = ScreenConfig {
        cabinet_count: [4, 10],
        cabinet_size_mm: [500.0, 500.0],
        shape_prior: ShapePriorConfig::Flat,
        bottom_completion: Some(BottomCompletion {
            lowest_measurable_row: 5,
            fallback_method: FallbackMethod::Vertical,
        }),
        absent_cells: vec![],
    };
    let grid = expected_grid_positions("MAIN", &cfg).unwrap();
    assert_eq!(grid.len(), (4 + 1) * (10 + 1));
}
```

- [ ] **Step 2: 跑测试确认 fail**

```bash
. "$HOME/.cargo/env" && cargo test -p lmt-adapter-total-station --test shape_grid_test
```

- [ ] **Step 3: 实现 `shape_grid.rs`**

```rust
use nalgebra::Vector3;

use crate::error::AdapterError;
use crate::project::{ScreenConfig, ShapePriorConfig};

/// One expected grid vertex position with its grid name.
#[derive(Debug, Clone)]
pub struct GridExpected {
    pub name: String,
    /// Position in model frame (meters), assuming origin is at the
    /// bottom-left vertex of the screen (`V001_R001`).
    pub model_position: Vector3<f64>,
    pub col_zero_based: u32,
    pub row_zero_based: u32,
}

/// Compute the expected (nominal) position of every grid vertex for a
/// given screen, in model-frame meters, assuming the screen's origin is
/// at the bottom-left vertex (`V001_R001`).
///
/// The returned positions are used as targets for KD-tree nearest-neighbor
/// matching in `geometric_naming.rs`.
pub fn expected_grid_positions(
    screen_id: &str,
    cfg: &ScreenConfig,
) -> Result<Vec<GridExpected>, AdapterError> {
    let cols = cfg.cabinet_count[0];
    let rows = cfg.cabinet_count[1];
    let cw_m = cfg.cabinet_size_mm[0] * 0.001;
    let ch_m = cfg.cabinet_size_mm[1] * 0.001;

    let mut out = Vec::with_capacity(((cols + 1) * (rows + 1)) as usize);

    match &cfg.shape_prior {
        ShapePriorConfig::Flat => {
            for r in 0..=rows {
                for c in 0..=cols {
                    let x = c as f64 * cw_m;
                    let z = r as f64 * ch_m;
                    out.push(GridExpected {
                        name: format!("{screen_id}_V{:03}_R{:03}", c + 1, r + 1),
                        model_position: Vector3::new(x, 0.0, z),
                        col_zero_based: c,
                        row_zero_based: r,
                    });
                }
            }
        }
        ShapePriorConfig::Curved { radius_mm } => {
            // Half-cylinder centered on +Y of the chord, radius R.
            // The total horizontal extent is cols * cabinet_width.
            let r_m = radius_mm * 0.001;
            let total_width = cols as f64 * cw_m;
            // Half angle subtended by the chord at the center.
            let half_angle = (total_width / (2.0 * r_m)).asin();
            for r in 0..=rows {
                for c in 0..=cols {
                    // Linear interpolation in arc length:
                    // angle ranges from -half_angle to +half_angle as c goes 0..cols.
                    let t = c as f64 / cols as f64;
                    let theta = -half_angle + 2.0 * half_angle * t;
                    let x = r_m * theta.sin();
                    let y = r_m - r_m * theta.cos(); // bow inward (+Y)
                    let z = r as f64 * ch_m;
                    out.push(GridExpected {
                        name: format!("{screen_id}_V{:03}_R{:03}", c + 1, r + 1),
                        model_position: Vector3::new(x, y, z),
                        col_zero_based: c,
                        row_zero_based: r,
                    });
                }
            }
        }
        ShapePriorConfig::Folded { fold_seam_columns: _ } => {
            // M1.1: treat folded as flat for the nominal grid; the actual
            // fold geometry is recovered from measured points in the
            // reconstructor. Future enhancement: piecewise-flat by seam.
            for r in 0..=rows {
                for c in 0..=cols {
                    let x = c as f64 * cw_m;
                    let z = r as f64 * ch_m;
                    out.push(GridExpected {
                        name: format!("{screen_id}_V{:03}_R{:03}", c + 1, r + 1),
                        model_position: Vector3::new(x, 0.0, z),
                        col_zero_based: c,
                        row_zero_based: r,
                    });
                }
            }
        }
    }

    Ok(out)
}
```

加 `pub mod shape_grid;` 到 `lib.rs`。

- [ ] **Step 4: 验证 + commit**

```bash
. "$HOME/.cargo/env" && cargo test -p lmt-adapter-total-station --test shape_grid_test
. "$HOME/.cargo/env" && cargo clippy -p lmt-adapter-total-station --all-targets -- -D warnings
git add crates/adapter-total-station/src/shape_grid.rs crates/adapter-total-station/src/lib.rs crates/adapter-total-station/tests/shape_grid_test.rs
git commit -m "feat(adapter-ts): expected grid positions for flat/curved/folded priors"
```

---

### Task 11: 几何归名（KD-tree）

**Files:**
- Create: `crates/adapter-total-station/src/geometric_naming.rs`
- Modify: `crates/adapter-total-station/src/lib.rs`
- Create: `crates/adapter-total-station/tests/geometric_naming_test.rs`

- [ ] **Step 1: 写失败测试**

```rust
use lmt_adapter_total_station::geometric_naming::{
    name_points_geometrically, NameOutcome, NamingTolerances,
};
use lmt_adapter_total_station::shape_grid::GridExpected;
use nalgebra::Vector3;

fn ge(name: &str, x: f64, z: f64, c: u32, r: u32) -> GridExpected {
    GridExpected {
        name: name.into(),
        model_position: Vector3::new(x, 0.0, z),
        col_zero_based: c,
        row_zero_based: r,
    }
}

#[test]
fn matched_points_get_assigned_to_nearest_grid_name() {
    let expected = vec![
        ge("MAIN_V001_R001", 0.0, 0.0, 0, 0),
        ge("MAIN_V002_R001", 0.5, 0.0, 1, 0),
        ge("MAIN_V001_R002", 0.0, 0.5, 0, 1),
        ge("MAIN_V002_R002", 0.5, 0.5, 1, 1),
    ];
    let model = vec![
        (10u32, Vector3::new(0.001, 0.0, 0.0)),    // 1mm from V001_R001
        (11u32, Vector3::new(0.499, 0.0, 0.0)),    // 1mm from V002_R001
        (12u32, Vector3::new(0.001, 0.0, 0.500)),  // 0mm from V001_R002
    ];
    let outcome = name_points_geometrically(&model, &expected, &NamingTolerances::default());

    assert_eq!(outcome.matches.len(), 3);
    assert_eq!(outcome.matches.get(&10).unwrap().as_str(), "MAIN_V001_R001");
    assert_eq!(outcome.matches.get(&11).unwrap().as_str(), "MAIN_V002_R001");
    assert_eq!(outcome.matches.get(&12).unwrap().as_str(), "MAIN_V001_R002");
    assert!(outcome.outliers.is_empty());
    assert!(outcome.ambiguous.is_empty());
}

#[test]
fn point_too_far_from_any_grid_position_is_outlier() {
    let expected = vec![
        ge("MAIN_V001_R001", 0.0, 0.0, 0, 0),
        ge("MAIN_V002_R001", 0.5, 0.0, 1, 0),
    ];
    let model = vec![(99u32, Vector3::new(5.0, 0.0, 5.0))];
    let outcome = name_points_geometrically(&model, &expected, &NamingTolerances::default());
    assert_eq!(outcome.outliers.len(), 1);
    assert_eq!(outcome.outliers[0].instrument_id, 99);
    assert_eq!(outcome.outliers[0].nearest_grid_name, "MAIN_V002_R001");
    assert!(outcome.matches.is_empty());
}

#[test]
fn two_points_within_ambiguity_radius_of_same_target_are_ambiguous() {
    let expected = vec![ge("MAIN_V001_R001", 0.0, 0.0, 0, 0)];
    let model = vec![
        (10u32, Vector3::new(0.001, 0.0, 0.0)),
        (11u32, Vector3::new(0.002, 0.0, 0.0)),
    ];
    let outcome = name_points_geometrically(&model, &expected, &NamingTolerances::default());
    // Only one of the two can claim V001_R001; the other is reported ambiguous.
    let total = outcome.matches.len() + outcome.ambiguous.len() + outcome.outliers.len();
    assert_eq!(total, 2);
    assert!(outcome.ambiguous.len() >= 1 || outcome.matches.len() == 1);
}
```

- [ ] **Step 2: 跑测试确认 fail**

```bash
. "$HOME/.cargo/env" && cargo test -p lmt-adapter-total-station --test geometric_naming_test
```

- [ ] **Step 3: 实现 `geometric_naming.rs`**

```rust
use std::collections::HashMap;

use kiddo::float::distance::SquaredEuclidean;
use kiddo::float::kdtree::KdTree;
use nalgebra::Vector3;

use crate::shape_grid::GridExpected;

/// Per-screen tolerance configuration for matching raw points to
/// expected grid vertices.
pub struct NamingTolerances {
    /// Maximum distance (meters) from a raw point to its nearest expected
    /// grid position before the point is classified as an outlier.
    pub max_match_distance_m: f64,
    /// Distance (meters) within which a competing claim on the same grid
    /// vertex is reported as ambiguous instead of silently dropped.
    pub ambiguity_radius_m: f64,
}

impl Default for NamingTolerances {
    fn default() -> Self {
        Self {
            // 50mm — half a typical 100mm absent-cell margin; well above
            // total-station instrument noise (1-3mm) and below cabinet pitch (500mm).
            max_match_distance_m: 0.050,
            ambiguity_radius_m: 0.010, // 10mm
        }
    }
}

#[derive(Debug, Clone)]
pub struct OutlierEntry {
    pub instrument_id: u32,
    pub nearest_grid_name: String,
    pub distance_m: f64,
}

#[derive(Debug, Clone)]
pub struct AmbiguityEntry {
    pub instrument_id: u32,
    pub candidates: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct NameOutcome {
    pub matches: HashMap<u32, String>,
    pub outliers: Vec<OutlierEntry>,
    pub ambiguous: Vec<AmbiguityEntry>,
}

/// Match each transformed raw point (`instrument_id`, model-frame position)
/// to its nearest expected grid vertex via KD-tree nearest neighbor.
///
/// Reports outliers (no expected vertex within `max_match_distance_m`)
/// and ambiguities (two or more raw points claiming the same vertex
/// within `ambiguity_radius_m`).
pub fn name_points_geometrically(
    model_points: &[(u32, Vector3<f64>)],
    expected: &[GridExpected],
    tol: &NamingTolerances,
) -> NameOutcome {
    let mut tree: KdTree<f64, u64, 3, 32, u32> = KdTree::new();
    for (i, ge) in expected.iter().enumerate() {
        tree.add(
            &[ge.model_position.x, ge.model_position.y, ge.model_position.z],
            i as u64,
        );
    }

    let max_sq = tol.max_match_distance_m * tol.max_match_distance_m;

    // Phase 1 — find each raw point's nearest expected vertex (or outlier).
    let mut tentative: Vec<(u32, usize, f64)> = Vec::new(); // (id, expected_idx, dist)
    let mut outliers: Vec<OutlierEntry> = Vec::new();

    for (id, pos) in model_points {
        let q = [pos.x, pos.y, pos.z];
        let nearest = tree.nearest_one::<SquaredEuclidean>(&q);
        let dist_m = nearest.distance.sqrt();
        if nearest.distance > max_sq {
            outliers.push(OutlierEntry {
                instrument_id: *id,
                nearest_grid_name: expected[nearest.item as usize].name.clone(),
                distance_m: dist_m,
            });
        } else {
            tentative.push((*id, nearest.item as usize, dist_m));
        }
    }

    // Phase 2 — resolve competing claims.
    // Group tentative matches by expected index.
    let mut by_expected: HashMap<usize, Vec<(u32, f64)>> = HashMap::new();
    for (id, idx, d) in &tentative {
        by_expected.entry(*idx).or_default().push((*id, *d));
    }

    let mut matches: HashMap<u32, String> = HashMap::new();
    let mut ambiguous: Vec<AmbiguityEntry> = Vec::new();

    for (idx, mut claims) in by_expected {
        if claims.len() == 1 {
            matches.insert(claims[0].0, expected[idx].name.clone());
        } else {
            // Sort by distance; closest wins, others are ambiguous if within radius.
            claims.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
            let winner = claims[0];
            matches.insert(winner.0, expected[idx].name.clone());
            for runner_up in claims.iter().skip(1) {
                ambiguous.push(AmbiguityEntry {
                    instrument_id: runner_up.0,
                    candidates: vec![expected[idx].name.clone()],
                });
            }
        }
    }

    NameOutcome { matches, outliers, ambiguous }
}
```

加 `pub mod geometric_naming;` 到 `lib.rs`。

- [ ] **Step 4: 验证 + commit**

```bash
. "$HOME/.cargo/env" && cargo test -p lmt-adapter-total-station --test geometric_naming_test
. "$HOME/.cargo/env" && cargo clippy -p lmt-adapter-total-station --all-targets -- -D warnings
git add crates/adapter-total-station/src/geometric_naming.rs crates/adapter-total-station/src/lib.rs crates/adapter-total-station/tests/geometric_naming_test.rs
git commit -m "feat(adapter-ts): geometric naming via KD-tree (matches/outliers/ambiguous)"
```

---

### Task 12: 底部遮挡 fallback fabrication

**Files:**
- Create: `crates/adapter-total-station/src/fallback.rs`
- Modify: `crates/adapter-total-station/src/lib.rs`
- Create: `crates/adapter-total-station/tests/fallback_test.rs`

- [ ] **Step 1: 写失败测试**

```rust
use lmt_adapter_total_station::fallback::fabricate_bottom_rows;
use lmt_adapter_total_station::project::{
    BottomCompletion, FallbackMethod, ScreenConfig, ShapePriorConfig,
};
use nalgebra::Vector3;
use std::collections::HashMap;

fn flat_screen_with_fallback(
    cols: u32,
    rows: u32,
    lowest: u32,
) -> ScreenConfig {
    ScreenConfig {
        cabinet_count: [cols, rows],
        cabinet_size_mm: [500.0, 500.0],
        shape_prior: ShapePriorConfig::Flat,
        bottom_completion: Some(BottomCompletion {
            lowest_measurable_row: lowest,
            fallback_method: FallbackMethod::Vertical,
        }),
        absent_cells: vec![],
    }
}

#[test]
fn fabricate_bottom_uses_lowest_row_minus_height() {
    // 4×5 cabinet array, lowest_measurable_row=3 → fabricate R001 + R002
    // (vertex rows 0 + 1) from R003 (vertex row 2).
    let cfg = flat_screen_with_fallback(4, 5, 3);

    // measured: every column at vertex row 2 (R003) at z=1.0m
    let mut measured: HashMap<String, Vector3<f64>> = HashMap::new();
    for c in 1..=5u32 {
        measured.insert(
            format!("MAIN_V{:03}_R003", c),
            Vector3::new((c - 1) as f64 * 0.5, 0.0, 1.0),
        );
    }

    let fabricated = fabricate_bottom_rows("MAIN", &cfg, &measured).unwrap();

    // R001 + R002 × 5 columns = 10 fabricated points
    assert_eq!(fabricated.len(), 10);

    // R002 vertex 2 (col=1) should be 0.5m below R003 vertex
    let v2 = fabricated.get("MAIN_V002_R002").unwrap();
    assert!((v2 - Vector3::new(0.5, 0.0, 0.5)).norm() < 1e-9);
    let v1 = fabricated.get("MAIN_V001_R001").unwrap();
    assert!((v1 - Vector3::new(0.0, 0.0, 0.0)).norm() < 1e-9);
}

#[test]
fn fabricate_bottom_with_no_completion_returns_empty() {
    let cfg = ScreenConfig {
        cabinet_count: [4, 2],
        cabinet_size_mm: [500.0, 500.0],
        shape_prior: ShapePriorConfig::Flat,
        bottom_completion: None,
        absent_cells: vec![],
    };
    let measured: HashMap<String, Vector3<f64>> = HashMap::new();
    let fabricated = fabricate_bottom_rows("MAIN", &cfg, &measured).unwrap();
    assert!(fabricated.is_empty());
}

#[test]
fn fabricate_bottom_errors_when_anchor_row_missing() {
    let cfg = flat_screen_with_fallback(4, 5, 3);
    // Empty measured map → can't anchor on R003.
    let measured: HashMap<String, Vector3<f64>> = HashMap::new();
    let result = fabricate_bottom_rows("MAIN", &cfg, &measured);
    assert!(result.is_err());
}
```

- [ ] **Step 2: 跑测试确认 fail**

```bash
. "$HOME/.cargo/env" && cargo test -p lmt-adapter-total-station --test fallback_test
```

- [ ] **Step 3: 实现 `fallback.rs`**

```rust
use std::collections::HashMap;

use nalgebra::Vector3;

use crate::error::AdapterError;
use crate::project::{FallbackMethod, ScreenConfig};

/// Fabricate vertices for grid rows below `lowest_measurable_row` by
/// vertical extension from the lowest measured row.
///
/// Returns a map of fabricated `grid_name → model_position`. Empty if
/// `bottom_completion` is `None`.
///
/// **Convention**: `lowest_measurable_row` is a 1-based **vertex row**
/// (e.g. R005 in `MAIN_V001_R005`). The vertex grid has `rows + 1` rows
/// numbered R001..R<rows+1>; R001 is the bottom edge.
pub fn fabricate_bottom_rows(
    screen_id: &str,
    cfg: &ScreenConfig,
    measured: &HashMap<String, Vector3<f64>>,
) -> Result<HashMap<String, Vector3<f64>>, AdapterError> {
    let Some(bc) = &cfg.bottom_completion else {
        return Ok(HashMap::new());
    };
    let lowest = bc.lowest_measurable_row;
    if lowest <= 1 {
        return Ok(HashMap::new());
    }

    let cols = cfg.cabinet_count[0];
    let cabinet_height_m = cfg.cabinet_size_mm[1] * 0.001;

    let mut out = HashMap::new();

    match bc.fallback_method {
        FallbackMethod::Vertical => {
            // For each column, anchor on the measured R<lowest> vertex,
            // then push down by cabinet height for each missing row below.
            for c in 1..=(cols + 1) {
                let anchor_name = format!("{screen_id}_V{:03}_R{:03}", c, lowest);
                let anchor = measured.get(&anchor_name).ok_or_else(|| {
                    AdapterError::InvalidInput(format!(
                        "fallback anchor {} not in measured points; \
                         cannot fabricate rows R001..R{:03}",
                        anchor_name,
                        lowest - 1
                    ))
                })?;

                for r in 1..lowest {
                    // Distance below anchor = (lowest - r) cabinets in z.
                    let dz = (lowest as f64 - r as f64) * cabinet_height_m;
                    let pos = Vector3::new(anchor.x, anchor.y, anchor.z - dz);
                    out.insert(format!("{screen_id}_V{:03}_R{:03}", c, r), pos);
                }
            }
        }
    }

    Ok(out)
}
```

加 `pub mod fallback;` 到 `lib.rs`。

- [ ] **Step 4: 验证 + commit**

```bash
. "$HOME/.cargo/env" && cargo test -p lmt-adapter-total-station --test fallback_test
. "$HOME/.cargo/env" && cargo clippy -p lmt-adapter-total-station --all-targets -- -D warnings
git add crates/adapter-total-station/src/fallback.rs crates/adapter-total-station/src/lib.rs crates/adapter-total-station/tests/fallback_test.rs
git commit -m "feat(adapter-ts): bottom-occlusion fallback via vertical extension"
```

---

## Phase 5 — 整合

### Task 13: `build_screen_measured_points` 整合管线

**Files:**
- Create: `crates/adapter-total-station/src/builder.rs`
- Modify: `crates/adapter-total-station/src/lib.rs`
- Create: `crates/adapter-total-station/tests/builder_test.rs`

- [ ] **Step 1: 写失败测试**

```rust
use lmt_adapter_total_station::builder::build_screen_measured_points;
use lmt_adapter_total_station::project::{ScreenConfig, ShapePriorConfig};
use lmt_adapter_total_station::raw_point::RawPoint;
use lmt_core::PointSource;
use nalgebra::Vector3;

fn rp(id: u32, x_mm: f64, y_mm: f64, z_mm: f64) -> RawPoint {
    RawPoint {
        instrument_id: id,
        position_mm: Vector3::new(x_mm, y_mm, z_mm),
        note: None,
    }
}

fn flat_4x2() -> ScreenConfig {
    ScreenConfig {
        cabinet_count: [4, 2],
        cabinet_size_mm: [500.0, 500.0],
        shape_prior: ShapePriorConfig::Flat,
        bottom_completion: None,
        absent_cells: vec![],
    }
}

#[test]
fn builder_assigns_grid_names_and_returns_measured_points() {
    // SOP: ids 1, 2, 3 = origin / x-axis / xy-plane.
    // For a 4×2 flat screen with lowest=R001:
    //   origin = MAIN_V001_R001 (model 0,0,0)
    //   x_axis = MAIN_V005_R001 (model 2,0,0) → instrument at +X 2m from origin
    //   xy_plane = MAIN_V001_R003 (model 0,0,1) → instrument at +Z 1m from origin
    let raw = vec![
        rp(1, 1000.0, 1000.0, 1000.0),       // origin
        rp(2, 3000.0, 1000.0, 1000.0),       // x-axis ref, +2m X
        rp(3, 1000.0, 1000.0, 2000.0),       // xy-plane ref, +1m Z
        rp(4, 1500.0, 1000.0, 1000.0),       // expected → MAIN_V002_R001 (model 0.5,0,0)
        rp(5, 3000.0, 1000.0, 2000.0),       // expected → MAIN_V005_R003 (model 2,0,1)
    ];
    let cfg = flat_4x2();
    let mp = build_screen_measured_points("MAIN", &raw, &cfg).unwrap();

    // 5 raw → at most 5 entries (3 reference + 2 grid)
    assert!(mp.points.len() >= 3);

    let bl = mp.points.iter().find(|p| p.name == "MAIN_V001_R001").unwrap();
    assert!(matches!(bl.source, PointSource::TotalStation));
    assert!(bl.position.norm() < 1e-9);

    let v2 = mp.points.iter().find(|p| p.name == "MAIN_V002_R001").unwrap();
    assert!((v2.position - Vector3::new(0.5, 0.0, 0.0)).norm() < 1e-3);
}

#[test]
fn builder_with_bottom_completion_inserts_fabricated_rows() {
    use lmt_adapter_total_station::project::{BottomCompletion, FallbackMethod};

    let mut cfg = flat_4x2();
    cfg.cabinet_count = [4, 4];
    cfg.bottom_completion = Some(BottomCompletion {
        lowest_measurable_row: 3,
        fallback_method: FallbackMethod::Vertical,
    });

    let raw = vec![
        // Reference points (origin at R003 because lowest=3)
        rp(1, 1000.0, 1000.0, 2000.0), // origin = MAIN_V001_R003 (model 0,0,0)
        rp(2, 3000.0, 1000.0, 2000.0), // x-axis = MAIN_V005_R003 (model 2,0,0)
        rp(3, 1000.0, 1000.0, 3000.0), // xy-plane = MAIN_V001_R005 (model 0,0,1)
    ];
    let mp = build_screen_measured_points("MAIN", &raw, &cfg).unwrap();

    // R001 + R002 across 5 columns = 10 fabricated; plus 3 measured anchors.
    assert!(mp.points.iter().any(|p| p.name == "MAIN_V001_R001"));
    assert!(mp.points.iter().any(|p| p.name == "MAIN_V003_R002"));
}
```

- [ ] **Step 2: 跑测试确认 fail**

```bash
. "$HOME/.cargo/env" && cargo test -p lmt-adapter-total-station --test builder_test
```

- [ ] **Step 3: 实现 `builder.rs`**

```rust
use std::collections::HashMap;

use nalgebra::Vector3;

use crate::error::AdapterError;
use crate::fallback::fabricate_bottom_rows;
use crate::geometric_naming::{name_points_geometrically, NameOutcome, NamingTolerances};
use crate::project::ScreenConfig;
use crate::raw_point::RawPoint;
use crate::reference_frame::build_frame_from_first_three;
use crate::shape_grid::expected_grid_positions;
use crate::transform::transform_to_model;

use lmt_core::measured_points::MeasuredPoints;
use lmt_core::point::{MeasuredPoint, PointSource};
use lmt_core::shape::{CabinetArray, ShapePrior};
use lmt_core::uncertainty::Uncertainty;

/// Standard total-station instrument uncertainty (mm).
const INSTRUMENT_SIGMA_MM: f64 = 2.0;

/// End-to-end: raw CSV points + screen config → `MeasuredPoints` ready
/// for `lmt_core::reconstruct::auto_reconstruct`.
///
/// Pipeline:
/// 1. Build coordinate frame from first 3 raw points (SOP).
/// 2. Transform every raw point to model frame.
/// 3. Compute expected grid positions for the screen.
/// 4. Geometric name matching via KD-tree.
/// 5. Fabricate fallback bottom-row points if `bottom_completion` is set.
/// 6. Assemble `MeasuredPoints`.
///
/// Outliers and ambiguous matches are NOT included in the output points
/// (caller can inspect them via the `NameOutcome` returned from
/// `build_screen_measured_points_with_outcome` if needed — see Task 14).
pub fn build_screen_measured_points(
    screen_id: &str,
    raw: &[RawPoint],
    cfg: &ScreenConfig,
) -> Result<MeasuredPoints, AdapterError> {
    let (mp, _outcome) = build_screen_measured_points_with_outcome(screen_id, raw, cfg)?;
    Ok(mp)
}

/// Same as `build_screen_measured_points` but also returns the
/// `NameOutcome` (matches/outliers/ambiguous) for diagnostics.
pub fn build_screen_measured_points_with_outcome(
    screen_id: &str,
    raw: &[RawPoint],
    cfg: &ScreenConfig,
) -> Result<(MeasuredPoints, NameOutcome), AdapterError> {
    // 1. Coordinate frame.
    let frame = build_frame_from_first_three(raw)?;

    // 2. Transform every raw point.
    let model = transform_to_model(raw, &frame);

    // 3. Expected grid positions.
    let expected = expected_grid_positions(screen_id, cfg)?;

    // 4. Geometric naming.
    let outcome = name_points_geometrically(&model, &expected, &NamingTolerances::default());

    // Build name → model position map for matched raw points.
    let model_by_id: HashMap<u32, Vector3<f64>> =
        model.iter().map(|(id, p)| (*id, *p)).collect();

    let mut measured_by_name: HashMap<String, Vector3<f64>> = HashMap::new();
    for (id, name) in &outcome.matches {
        if let Some(pos) = model_by_id.get(id) {
            measured_by_name.insert(name.clone(), *pos);
        }
    }

    // 5. Fabricate fallback rows (if any).
    let fabricated = fabricate_bottom_rows(screen_id, cfg, &measured_by_name)?;

    // 6. Assemble MeasuredPoints.
    let mut points: Vec<MeasuredPoint> = Vec::new();
    for (name, pos) in &measured_by_name {
        points.push(MeasuredPoint {
            name: name.clone(),
            position: *pos,
            uncertainty: Uncertainty::Isotropic(INSTRUMENT_SIGMA_MM),
            source: PointSource::TotalStation,
        });
    }
    for (name, pos) in &fabricated {
        points.push(MeasuredPoint {
            name: name.clone(),
            position: *pos,
            // Fallback fabrications carry larger uncertainty (10mm).
            uncertainty: Uncertainty::Isotropic(10.0),
            source: PointSource::TotalStation,
        });
    }

    let cabinet_array = if cfg.absent_cells.is_empty() {
        CabinetArray::rectangle(
            cfg.cabinet_count[0],
            cfg.cabinet_count[1],
            cfg.cabinet_size_mm,
        )
    } else {
        CabinetArray::irregular(
            cfg.cabinet_count[0],
            cfg.cabinet_count[1],
            cfg.cabinet_size_mm,
            cfg.absent_cells.clone(),
        )
    };

    let shape_prior = match &cfg.shape_prior {
        crate::project::ShapePriorConfig::Flat => ShapePrior::Flat,
        crate::project::ShapePriorConfig::Curved { radius_mm } => ShapePrior::Curved {
            radius_mm: *radius_mm,
        },
        crate::project::ShapePriorConfig::Folded { fold_seam_columns } => ShapePrior::Folded {
            fold_seam_columns: fold_seam_columns.clone(),
        },
    };

    let mp = MeasuredPoints {
        screen_id: screen_id.to_string(),
        coordinate_frame: frame,
        cabinet_array,
        shape_prior,
        points,
    };

    Ok((mp, outcome))
}
```

加 `pub mod builder;` 到 `lib.rs`。

- [ ] **Step 4: 验证 + commit**

```bash
. "$HOME/.cargo/env" && cargo test -p lmt-adapter-total-station --test builder_test
. "$HOME/.cargo/env" && cargo clippy -p lmt-adapter-total-station --all-targets -- -D warnings
git add crates/adapter-total-station/src/builder.rs crates/adapter-total-station/src/lib.rs crates/adapter-total-station/tests/builder_test.rs
git commit -m "feat(adapter-ts): integrate full pipeline → MeasuredPoints + NameOutcome"
```

---

### Task 14: `AdapterReport` 生成

**Files:**
- Create: `crates/adapter-total-station/src/report_builder.rs`
- Modify: `crates/adapter-total-station/src/lib.rs`
- Create: `crates/adapter-total-station/tests/report_builder_test.rs`

- [ ] **Step 1: 写失败测试**

```rust
use lmt_adapter_total_station::builder::build_screen_measured_points_with_outcome;
use lmt_adapter_total_station::project::{ScreenConfig, ShapePriorConfig};
use lmt_adapter_total_station::raw_point::RawPoint;
use lmt_adapter_total_station::report_builder::build_screen_report;
use nalgebra::Vector3;

fn rp(id: u32, x_mm: f64, y_mm: f64, z_mm: f64) -> RawPoint {
    RawPoint {
        instrument_id: id,
        position_mm: Vector3::new(x_mm, y_mm, z_mm),
        note: None,
    }
}

#[test]
fn report_counts_measured_and_missing() {
    let raw = vec![
        rp(1, 1000.0, 1000.0, 1000.0), // origin
        rp(2, 3000.0, 1000.0, 1000.0), // x-axis
        rp(3, 1000.0, 1000.0, 2000.0), // xy-plane
        rp(4, 1500.0, 1000.0, 1000.0), // → MAIN_V002_R001
    ];
    let cfg = ScreenConfig {
        cabinet_count: [4, 2],
        cabinet_size_mm: [500.0, 500.0],
        shape_prior: ShapePriorConfig::Flat,
        bottom_completion: None,
        absent_cells: vec![],
    };
    let (mp, outcome) =
        build_screen_measured_points_with_outcome("MAIN", &raw, &cfg).unwrap();
    let report = build_screen_report("MAIN", &mp, &outcome, &cfg);

    // Expected = 5×3 = 15
    assert_eq!(report.expected_count, 15);
    // Measured = 4 named (V001_R001, V005_R001, V001_R003, V002_R001)
    assert_eq!(report.measured_count, 4);
    assert_eq!(report.fabricated_count, 0);
    // Missing = 15 - 4 = 11
    assert_eq!(report.missing.len(), 11);
    assert!(report.estimated_rms_mm > 0.0);
}

#[test]
fn report_records_outliers_when_point_too_far() {
    let raw = vec![
        rp(1, 1000.0, 1000.0, 1000.0),
        rp(2, 3000.0, 1000.0, 1000.0),
        rp(3, 1000.0, 1000.0, 2000.0),
        rp(4, 99999.0, 99999.0, 99999.0), // outlier
    ];
    let cfg = ScreenConfig {
        cabinet_count: [4, 2],
        cabinet_size_mm: [500.0, 500.0],
        shape_prior: ShapePriorConfig::Flat,
        bottom_completion: None,
        absent_cells: vec![],
    };
    let (mp, outcome) =
        build_screen_measured_points_with_outcome("MAIN", &raw, &cfg).unwrap();
    let report = build_screen_report("MAIN", &mp, &outcome, &cfg);

    assert_eq!(report.outliers.len(), 1);
    assert_eq!(report.outliers[0].instrument_id, 4);
}
```

- [ ] **Step 2: 跑测试确认 fail**

```bash
. "$HOME/.cargo/env" && cargo test -p lmt-adapter-total-station --test report_builder_test
```

- [ ] **Step 3: 实现 `report_builder.rs`**

```rust
use std::collections::HashSet;

use crate::geometric_naming::NameOutcome;
use crate::project::ScreenConfig;
use crate::report::{AmbiguousMatch, MissingPoint, OutlierPoint, ScreenReport};
use crate::shape_grid::expected_grid_positions;

use lmt_core::measured_points::MeasuredPoints;
use lmt_core::point::PointSource;

pub fn build_screen_report(
    screen_id: &str,
    mp: &MeasuredPoints,
    outcome: &NameOutcome,
    cfg: &ScreenConfig,
) -> ScreenReport {
    // Compute expected names.
    let expected = expected_grid_positions(screen_id, cfg).unwrap_or_default();
    let expected_count = expected.len();
    let expected_names: HashSet<String> = expected.iter().map(|g| g.name.clone()).collect();

    // Count measured (TotalStation source) vs fabricated.
    let mut measured_count = 0usize;
    let mut fabricated_count = 0usize;
    let mut present_names: HashSet<String> = HashSet::new();
    for p in &mp.points {
        present_names.insert(p.name.clone());
        match p.source {
            PointSource::TotalStation => {
                // Heuristic: instrument-uncertainty 2mm = direct, 10mm = fabricated.
                // Based on builder.rs convention.
                if let lmt_core::uncertainty::Uncertainty::Isotropic(s) = p.uncertainty {
                    if s <= 5.0 {
                        measured_count += 1;
                    } else {
                        fabricated_count += 1;
                    }
                } else {
                    measured_count += 1;
                }
            }
            _ => measured_count += 1,
        }
    }

    let missing: Vec<MissingPoint> = expected_names
        .difference(&present_names)
        .map(|n| MissingPoint { name: n.clone() })
        .collect();

    let outliers: Vec<OutlierPoint> = outcome
        .outliers
        .iter()
        .map(|o| OutlierPoint {
            instrument_id: o.instrument_id,
            distance_to_nearest_mm: o.distance_m * 1000.0,
            nearest_grid_name: o.nearest_grid_name.clone(),
        })
        .collect();

    let ambiguous: Vec<AmbiguousMatch> = outcome
        .ambiguous
        .iter()
        .map(|a| AmbiguousMatch {
            instrument_id: a.instrument_id,
            candidates: a.candidates.clone(),
        })
        .collect();

    let mut warnings: Vec<String> = Vec::new();
    if !outcome.outliers.is_empty() {
        warnings.push(format!(
            "{} outlier point(s) — possibly stray markers or wrong screen",
            outcome.outliers.len()
        ));
    }
    if fabricated_count > 0 {
        warnings.push(format!(
            "{fabricated_count} bottom-row vertices fabricated via vertical fallback; \
             accuracy ±5-15mm in fallback region"
        ));
    }
    if missing.len() > expected_count / 2 {
        warnings.push(format!(
            "Less than half the grid is populated ({}/{}); reconstruction may be unreliable",
            present_names.len(),
            expected_count
        ));
    }

    // Estimated RMS = sigma_approx aggregated across measured points.
    let estimated_rms_mm = if mp.points.is_empty() {
        0.0
    } else {
        let n = mp.points.len() as f64;
        let sum_sq: f64 = mp
            .points
            .iter()
            .map(|p| p.uncertainty.sigma_approx().powi(2))
            .sum();
        (sum_sq / n).sqrt()
    };

    ScreenReport {
        screen_id: screen_id.to_string(),
        expected_count,
        measured_count,
        fabricated_count,
        missing,
        outliers,
        ambiguous,
        warnings,
        estimated_rms_mm,
    }
}
```

加 `pub mod report_builder;` 到 `lib.rs`。

- [ ] **Step 4: 验证 + commit**

```bash
. "$HOME/.cargo/env" && cargo test -p lmt-adapter-total-station --test report_builder_test
. "$HOME/.cargo/env" && cargo clippy -p lmt-adapter-total-station --all-targets -- -D warnings
git add crates/adapter-total-station/src/report_builder.rs crates/adapter-total-station/src/lib.rs crates/adapter-total-station/tests/report_builder_test.rs
git commit -m "feat(adapter-ts): generate ScreenReport with missing/outliers/warnings"
```

---

## Phase 6 — 指示卡

### Task 15: HTML instruction card

**Files:**
- Create: `crates/adapter-total-station/src/instruction_card/mod.rs`
- Create: `crates/adapter-total-station/src/instruction_card/html.rs`
- Modify: `crates/adapter-total-station/src/lib.rs`
- Create: `crates/adapter-total-station/tests/instruction_html_test.rs`

- [ ] **Step 1: 写失败测试**

```rust
use lmt_adapter_total_station::instruction_card::html::generate_html;
use lmt_adapter_total_station::instruction_card::InstructionCard;
use lmt_adapter_total_station::project::{ScreenConfig, ShapePriorConfig};

#[test]
fn html_contains_project_name_and_screen_id() {
    let cfg = ScreenConfig {
        cabinet_count: [4, 2],
        cabinet_size_mm: [500.0, 500.0],
        shape_prior: ShapePriorConfig::Flat,
        bottom_completion: None,
        absent_cells: vec![],
    };
    let card = InstructionCard {
        project_name: "Studio_A".into(),
        screen_id: "MAIN".into(),
        cfg,
        origin_grid_name: "MAIN_V001_R001".into(),
        x_axis_grid_name: "MAIN_V005_R001".into(),
        xy_plane_grid_name: "MAIN_V001_R003".into(),
    };
    let html = generate_html(&card);
    assert!(html.contains("<title>"));
    assert!(html.contains("Studio_A"));
    assert!(html.contains("MAIN"));
    assert!(html.contains("MAIN_V001_R001"));
    assert!(html.contains("MAIN_V005_R001"));
    assert!(html.contains("MAIN_V001_R003"));
    // Should list all 15 grid points (5 × 3)
    assert!(html.matches("MAIN_V").count() >= 15);
}
```

- [ ] **Step 2: 跑测试确认 fail**

```bash
. "$HOME/.cargo/env" && cargo test -p lmt-adapter-total-station --test instruction_html_test
```

- [ ] **Step 3: 实现 `instruction_card/mod.rs` + `html.rs`**

`crates/adapter-total-station/src/instruction_card/mod.rs`:

```rust
use crate::project::ScreenConfig;

pub mod html;
pub mod pdf;

/// Data needed to render an instruction card (PDF + HTML share this).
#[derive(Debug, Clone)]
pub struct InstructionCard {
    pub project_name: String,
    pub screen_id: String,
    pub cfg: ScreenConfig,
    pub origin_grid_name: String,
    pub x_axis_grid_name: String,
    pub xy_plane_grid_name: String,
}
```

`crates/adapter-total-station/src/instruction_card/html.rs`:

```rust
use crate::instruction_card::InstructionCard;
use crate::shape_grid::expected_grid_positions;

/// Render an instruction card as standalone HTML.
pub fn generate_html(card: &InstructionCard) -> String {
    let grid =
        expected_grid_positions(&card.screen_id, &card.cfg).unwrap_or_default();
    let total = grid.len();

    let mut html = String::new();
    html.push_str("<!DOCTYPE html>\n");
    html.push_str("<html lang=\"zh\">\n<head>\n");
    html.push_str("<meta charset=\"utf-8\">\n");
    html.push_str(&format!(
        "<title>LED 屏建模指示卡 - {}</title>\n",
        html_escape(&card.project_name)
    ));
    html.push_str("<style>\n");
    html.push_str("body { font-family: 'PingFang SC', 'Microsoft YaHei', sans-serif; line-height: 1.5; max-width: 900px; margin: 2em auto; padding: 0 1em; }\n");
    html.push_str("h1 { border-bottom: 2px solid #333; padding-bottom: 0.3em; }\n");
    html.push_str("table { border-collapse: collapse; margin: 1em 0; width: 100%; }\n");
    html.push_str("th, td { border: 1px solid #999; padding: 4px 8px; text-align: left; }\n");
    html.push_str("th { background: #eee; }\n");
    html.push_str(".ref { background: #ffe4b5; }\n");
    html.push_str("</style>\n</head>\n<body>\n");

    html.push_str(&format!(
        "<h1>LED 屏建模 - 测量指示卡</h1>\n<p>项目：<b>{}</b> &nbsp;&nbsp; 屏体：<b>{}</b></p>\n",
        html_escape(&card.project_name),
        html_escape(&card.screen_id)
    ));
    html.push_str(&format!(
        "<p>箱体阵列：{} × {} &nbsp;&nbsp; 单箱体：{} × {} mm</p>\n",
        card.cfg.cabinet_count[0],
        card.cfg.cabinet_count[1],
        card.cfg.cabinet_size_mm[0],
        card.cfg.cabinet_size_mm[1]
    ));
    html.push_str(&format!(
        "<p>总测点数：{}（含 3 参考点）</p>\n",
        total
    ));

    html.push_str("<h2>第一步：3 个参考点（必须按仪器点号 1, 2, 3 顺序测量）</h2>\n<table>\n");
    html.push_str("<tr><th>仪器点号</th><th>角色</th><th>网格命名</th></tr>\n");
    html.push_str(&format!(
        "<tr class=\"ref\"><td>1</td><td>① Origin (0, 0, 0)</td><td>{}</td></tr>\n",
        html_escape(&card.origin_grid_name)
    ));
    html.push_str(&format!(
        "<tr class=\"ref\"><td>2</td><td>② X-axis</td><td>{}</td></tr>\n",
        html_escape(&card.x_axis_grid_name)
    ));
    html.push_str(&format!(
        "<tr class=\"ref\"><td>3</td><td>③ XY-plane</td><td>{}</td></tr>\n",
        html_escape(&card.xy_plane_grid_name)
    ));
    html.push_str("</table>\n");

    html.push_str("<h2>第二步：其他网格测点（仪器自动点号 4 起）</h2>\n<table>\n");
    html.push_str("<tr><th>网格命名</th><th>X (m)</th><th>Y (m)</th><th>Z (m)</th></tr>\n");
    let ref_names = [
        card.origin_grid_name.as_str(),
        card.x_axis_grid_name.as_str(),
        card.xy_plane_grid_name.as_str(),
    ];
    for ge in &grid {
        if ref_names.contains(&ge.name.as_str()) {
            continue;
        }
        html.push_str(&format!(
            "<tr><td>{}</td><td>{:.3}</td><td>{:.3}</td><td>{:.3}</td></tr>\n",
            html_escape(&ge.name),
            ge.model_position.x,
            ge.model_position.y,
            ge.model_position.z
        ));
    }
    html.push_str("</table>\n");

    html.push_str("<h2>现场操作要点</h2>\n<ul>\n");
    html.push_str("<li>先测 ①②③ 三个参考点（仪器点号 1-3）</li>\n");
    html.push_str("<li>其他点测量顺序无所谓（仪器点号 4 起递增）</li>\n");
    html.push_str("<li>测完导出 CSV，工具会自动按几何位置归名</li>\n");
    html.push_str("<li>漏测可补，工具会识别缺什么</li>\n");
    html.push_str("</ul>\n");
    html.push_str("</body>\n</html>\n");

    html
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
```

加 `pub mod instruction_card;` 到 `lib.rs`。同时**先创建** `crates/adapter-total-station/src/instruction_card/pdf.rs` 占位以避免编译错误：

```rust
//! PDF instruction card — implemented in Task 16.
```

- [ ] **Step 4: 验证 + commit**

```bash
. "$HOME/.cargo/env" && cargo test -p lmt-adapter-total-station --test instruction_html_test
. "$HOME/.cargo/env" && cargo clippy -p lmt-adapter-total-station --all-targets -- -D warnings
git add crates/adapter-total-station/src/instruction_card crates/adapter-total-station/src/lib.rs crates/adapter-total-station/tests/instruction_html_test.rs
git commit -m "feat(adapter-ts): HTML instruction card with grid table"
```

---

### Task 16: PDF instruction card（printpdf）

**Files:**
- Modify: `crates/adapter-total-station/src/instruction_card/pdf.rs`
- Create: `crates/adapter-total-station/tests/instruction_pdf_test.rs`

- [ ] **Step 1: 写失败测试**

```rust
use lmt_adapter_total_station::instruction_card::pdf::generate_pdf;
use lmt_adapter_total_station::instruction_card::InstructionCard;
use lmt_adapter_total_station::project::{ScreenConfig, ShapePriorConfig};
use tempfile::tempdir;

#[test]
fn pdf_writes_a_nonempty_file_starting_with_pdf_magic() {
    let cfg = ScreenConfig {
        cabinet_count: [4, 2],
        cabinet_size_mm: [500.0, 500.0],
        shape_prior: ShapePriorConfig::Flat,
        bottom_completion: None,
        absent_cells: vec![],
    };
    let card = InstructionCard {
        project_name: "Studio_A".into(),
        screen_id: "MAIN".into(),
        cfg,
        origin_grid_name: "MAIN_V001_R001".into(),
        x_axis_grid_name: "MAIN_V005_R001".into(),
        xy_plane_grid_name: "MAIN_V001_R003".into(),
    };
    let dir = tempdir().unwrap();
    let path = dir.path().join("card.pdf");
    generate_pdf(&card, &path).unwrap();

    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.len() > 1000, "PDF too small ({} bytes)", bytes.len());
    assert!(bytes.starts_with(b"%PDF-"), "missing PDF magic header");
}
```

- [ ] **Step 2: 跑测试确认 fail**

```bash
. "$HOME/.cargo/env" && cargo test -p lmt-adapter-total-station --test instruction_pdf_test
```

- [ ] **Step 3: 实现 `instruction_card/pdf.rs`**

```rust
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use printpdf::{Mm, PdfDocument};

use crate::error::AdapterError;
use crate::instruction_card::InstructionCard;
use crate::shape_grid::expected_grid_positions;

/// Render an instruction card to PDF (A4 portrait).
pub fn generate_pdf(card: &InstructionCard, path: &Path) -> Result<(), AdapterError> {
    let grid = expected_grid_positions(&card.screen_id, &card.cfg)
        .map_err(|e| AdapterError::Pdf(format!("grid: {e}")))?;

    let (doc, page1, layer1) =
        PdfDocument::new(format!("LMT — {}", card.project_name), Mm(210.0), Mm(297.0), "Layer 1");
    let layer = doc.get_page(page1).get_layer(layer1);
    let font = doc
        .add_builtin_font(printpdf::BuiltinFont::Helvetica)
        .map_err(|e| AdapterError::Pdf(e.to_string()))?;
    let bold = doc
        .add_builtin_font(printpdf::BuiltinFont::HelveticaBold)
        .map_err(|e| AdapterError::Pdf(e.to_string()))?;

    // Header
    layer.use_text(
        format!("LED Instruction Card — {}", card.project_name),
        14.0,
        Mm(20.0),
        Mm(280.0),
        &bold,
    );
    layer.use_text(
        format!(
            "Screen: {}    Cabinets: {}x{}    Cabinet size: {} x {} mm    Total points: {}",
            card.screen_id,
            card.cfg.cabinet_count[0],
            card.cfg.cabinet_count[1],
            card.cfg.cabinet_size_mm[0],
            card.cfg.cabinet_size_mm[1],
            grid.len()
        ),
        9.0,
        Mm(20.0),
        Mm(272.0),
        &font,
    );

    // Reference points block
    layer.use_text(
        "Reference points (instrument ids 1, 2, 3 — measure in order):",
        11.0,
        Mm(20.0),
        Mm(258.0),
        &bold,
    );
    layer.use_text(
        format!("  1) Origin     -> {}", card.origin_grid_name),
        9.0,
        Mm(20.0),
        Mm(250.0),
        &font,
    );
    layer.use_text(
        format!("  2) X-axis     -> {}", card.x_axis_grid_name),
        9.0,
        Mm(20.0),
        Mm(244.0),
        &font,
    );
    layer.use_text(
        format!("  3) XY-plane   -> {}", card.xy_plane_grid_name),
        9.0,
        Mm(20.0),
        Mm(238.0),
        &font,
    );

    // Grid table — paginated
    layer.use_text(
        "All grid points (instrument 4+ in any order):",
        11.0,
        Mm(20.0),
        Mm(225.0),
        &bold,
    );
    layer.use_text(
        "Name                 X(m)      Y(m)      Z(m)",
        9.0,
        Mm(20.0),
        Mm(218.0),
        &font,
    );

    let ref_names = [
        card.origin_grid_name.as_str(),
        card.x_axis_grid_name.as_str(),
        card.xy_plane_grid_name.as_str(),
    ];
    let mut y = 212.0_f64;
    let line_height = 4.5_f64;
    let bottom_margin = 25.0_f64;

    let mut current_layer = layer;
    for ge in &grid {
        if ref_names.contains(&ge.name.as_str()) {
            continue;
        }
        if y < bottom_margin {
            // New page
            let (new_page, new_layer) = doc.add_page(Mm(210.0), Mm(297.0), "Layer cont");
            current_layer = doc.get_page(new_page).get_layer(new_layer);
            y = 280.0;
        }
        current_layer.use_text(
            format!(
                "{:20} {:8.3}  {:8.3}  {:8.3}",
                ge.name, ge.model_position.x, ge.model_position.y, ge.model_position.z
            ),
            8.5,
            Mm(20.0),
            Mm(y),
            &font,
        );
        y -= line_height;
    }

    // Save
    let file = File::create(path)?;
    let mut buf = BufWriter::new(file);
    doc.save(&mut buf)
        .map_err(|e| AdapterError::Pdf(e.to_string()))?;

    Ok(())
}
```

- [ ] **Step 4: 验证 + commit**

```bash
. "$HOME/.cargo/env" && cargo test -p lmt-adapter-total-station --test instruction_pdf_test
. "$HOME/.cargo/env" && cargo clippy -p lmt-adapter-total-station --all-targets -- -D warnings
git add crates/adapter-total-station/src/instruction_card/pdf.rs crates/adapter-total-station/tests/instruction_pdf_test.rs
git commit -m "feat(adapter-ts): PDF instruction card via printpdf (A4 + paginated grid)"
```

---

## Phase 7 — 集成 + 收尾

### Task 17: End-to-end fixture test (CSV + YAML → MeasuredPoints → reconstruct → OBJ)

**Files:**
- Create: `crates/adapter-total-station/tests/end_to_end.rs`

- [ ] **Step 1: 写测试（TDD：测试整条 plumbing — 失败状态从前面 task 已经验证）**

```rust
use lmt_adapter_total_station::builder::build_screen_measured_points;
use lmt_adapter_total_station::csv_parser::parse_csv;
use lmt_adapter_total_station::project_loader::load_project;
use lmt_core::export::targets::{NeutralTarget, OutputTarget};
use lmt_core::reconstruct::auto_reconstruct;
use std::path::PathBuf;
use tempfile::tempdir;

fn fixture(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests/fixtures");
    p.push(name);
    p
}

#[test]
fn full_csv_to_obj_pipeline() {
    let raw = parse_csv(&fixture("e2e.csv")).unwrap();
    let cfg = load_project(&fixture("e2e.yaml")).unwrap();
    let main = cfg.screens.get("MAIN").unwrap();

    let mp = build_screen_measured_points("MAIN", &raw, main).unwrap();
    let surface = auto_reconstruct(&mp).expect("reconstruction succeeded");

    let dir = tempdir().unwrap();
    let path = dir.path().join("e2e.obj");
    NeutralTarget::default()
        .export(&surface, &mp.cabinet_array, &path)
        .unwrap();

    let obj = std::fs::read_to_string(&path).unwrap();
    assert!(obj.lines().filter(|l| l.starts_with("v ")).count() > 0);
    assert!(obj.lines().filter(|l| l.starts_with("f ")).count() > 0);
}
```

- [ ] **Step 2: 创建 fixtures**

`crates/adapter-total-station/tests/fixtures/e2e.yaml`:

```yaml
project:
  name: E2E_Test
screens:
  MAIN:
    cabinet_count: [4, 2]
    cabinet_size_mm: [500, 500]
    shape_prior:
      type: flat
coordinate_system:
  origin_grid_name: MAIN_V001_R001
  x_axis_grid_name: MAIN_V005_R001
  xy_plane_grid_name: MAIN_V001_R003
```

`crates/adapter-total-station/tests/fixtures/e2e.csv`：

For a 4×2 flat screen, total grid = 5×3 = 15 vertices. Provide all 15 measured at exact expected positions for a deterministic test (instrument ids 1–15, all in mm; ids 1/2/3 are origin/x/xy as required by SOP).

```
name,x,y,z,note
1,0,0,0,
2,2000,0,0,
3,0,0,1000,
4,500,0,0,
5,1000,0,0,
6,1500,0,0,
7,0,0,500,
8,500,0,500,
9,1000,0,500,
10,1500,0,500,
11,2000,0,500,
12,500,0,1000,
13,1000,0,1000,
14,1500,0,1000,
15,2000,0,1000,
```

> **Why all coords with y=0**: keeps the test focused on naming + reconstruction; no rotation involved. The reference frame from points 1, 2, 3 establishes origin at (0,0,0) world = (0,0,0) model, +X axis along world +X (because dx = 2000mm−0 = +X), and +Z axis along world +Z (dxy = z=1000mm gives Z up). The remaining 12 points sit on a 5×3 grid in z-x plane, so they'll be matched exactly.

- [ ] **Step 3: 验证 + commit**

```bash
. "$HOME/.cargo/env" && cargo test -p lmt-adapter-total-station --test end_to_end
. "$HOME/.cargo/env" && cargo test --workspace
. "$HOME/.cargo/env" && cargo clippy -p lmt-adapter-total-station --all-targets -- -D warnings
git add crates/adapter-total-station/tests/end_to_end.rs crates/adapter-total-station/tests/fixtures/e2e.yaml crates/adapter-total-station/tests/fixtures/e2e.csv
git commit -m "test(adapter-ts): end-to-end CSV+YAML → MeasuredPoints → OBJ"
```

---

### Task 18: Workspace fmt + clippy + tag

**Files:**
- Possibly modified by `cargo fmt` across `crates/adapter-total-station/`
- Create: `crates/adapter-total-station/README.md`
- Modify: `README.md` (project root)

- [ ] **Step 1: cargo test --workspace + clippy --workspace**

```bash
. "$HOME/.cargo/env" && cargo test --workspace
. "$HOME/.cargo/env" && cargo clippy --workspace --all-targets -- -D warnings
. "$HOME/.cargo/env" && cargo fmt --all -- --check
```

If `cargo fmt --check` reports drift, run `cargo fmt --all` and commit modified files (Rust + Cargo.toml only, exclude untracked `.claude/`, `CLAUDE.md`, `docs/`).

- [ ] **Step 2: 写 crate README**

`crates/adapter-total-station/README.md`:

```markdown
# lmt-adapter-total-station

M1 adapter: total-station CSV + project YAML → `lmt_core::MeasuredPoints` +
JSON validation report + instruction card (PDF + HTML).

## Single-screen scope (M1.1)

Currently supports **one screen per project** (`MAIN`). The first 3
CSV rows must be the user-selected reference points (origin / X-axis /
XY-plane), per the field SOP. Multi-screen attribution (FLOOR + others)
is M1.2.

## Public API

\`\`\`rust
use lmt_adapter_total_station::{
    csv_parser::parse_csv,
    project_loader::load_project,
    builder::build_screen_measured_points,
    report_builder::build_screen_report,
    instruction_card::{html::generate_html, pdf::generate_pdf, InstructionCard},
};
\`\`\`

## Pipeline

1. `parse_csv` — Trimble/Leica CSV (mm, instrument-numbered) → `Vec<RawPoint>`
2. `load_project` — YAML → `ProjectConfig`
3. `build_screen_measured_points` — first 3 raw points build coord frame;
   transform → KD-tree match → fabricate fallback bottom rows
4. `build_screen_report` — counts measured/missing/outliers/ambiguous; warns
5. Pass `MeasuredPoints` to `lmt_core::reconstruct::auto_reconstruct`,
   then `lmt_core::export::targets::{Disguise|Unreal|Neutral}Target::export`

## Spec

`docs/superpowers/specs/2026-05-10-led-mesh-toolkit-design.md` §4
```

- [ ] **Step 3: 在项目根 README 加 status 行**

Modify `README.md` (project root) — replace the Status block:

```markdown
## Status

- M0.1 Rust core — done (tag `m0.1-complete`)
- M1.1 Total-station adapter — done (tag `m1.1-complete`)
- M0.2 GUI shell + Tauri integration — pending
- M2 Visual photogrammetry adapter — pending
```

- [ ] **Step 4: 提交 + tag**

```bash
git add crates/adapter-total-station/README.md README.md
git commit -m "docs: M1.1 total-station adapter complete; update READMEs" -- crates/adapter-total-station/README.md README.md
git tag -a m1.1-complete -m "M1.1 total-station adapter complete"
git tag --list
git log --oneline -5
```

- [ ] **Step 5: 最终验证**

```bash
. "$HOME/.cargo/env" && cargo test --workspace
. "$HOME/.cargo/env" && cargo clippy --workspace --all-targets -- -D warnings
. "$HOME/.cargo/env" && cargo fmt --all -- --check
```

All three should be clean.

---

## 完成判定

完成 M1.1 后应满足：

- [ ] `cargo build --workspace` 全绿
- [ ] `cargo test --workspace` 全绿（M0.1 既有 ~120 tests + M1.1 新增 ~30+ tests）
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` clean
- [ ] `cargo fmt --all -- --check` clean
- [ ] `crates/adapter-total-station` 公共 API：`parse_csv`、`load_project`、`build_screen_measured_points`、`build_screen_report`、`generate_html`、`generate_pdf`、`AdapterError`、`AdapterReport`、`ScreenReport`、`InstructionCard`、`ProjectConfig` 等全部 export
- [ ] End-to-end test：fixture CSV + YAML → MeasuredPoints → auto_reconstruct → OBJ 全管线 PASS
- [ ] PDF 测试输出有效 `%PDF-` magic header
- [ ] HTML 测试输出含项目名 / 屏体名 / 全部 grid 点
- [ ] `git tag m1.1-complete` 已打

完成后：M0.2 GUI shell plan 可以独立 session 启动；M1.1 输出的 `MeasuredPoints` 已经能给 GUI 用。

---

## 跨 plan 依赖

| 当前 plan | 依赖 |
|---|---|
| **M1.1**（本文件）| M0.1 完成（消费 `lmt-core` API） |
| **M1.2** 多 screen attribution（增量） | M1.1 完成 |
| **M0.2** GUI shell | M0.1 完成；与 M1 / M2 并行 |
| **M2** 视觉反算 adapter | M0.1 完成；与 M1 并行 |

M1 和 M2 可以两个独立 Claude Code session 并行启动（M0.1 IR 已冻结）。
