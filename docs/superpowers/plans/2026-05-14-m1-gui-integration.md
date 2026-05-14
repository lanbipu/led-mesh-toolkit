# M1 GUI Integration — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把已实现的 M1 全站仪 adapter（CSV → MeasuredPoints + 指示卡）接进 GUI，让用户能在 Import.vue 选 Trimble CSV、在 Instruct.vue 预览/下载指示卡，闭环跑通"原始测量数据 → reconstruct → export OBJ"。

**Architecture:** 不改 M1 adapter 内部代码；不改 GUI 端 `ProjectConfig` schema（M0.2 已用稳）；在 Tauri 层加一个 `commands/total_station.rs` 模块负责字段映射 + 调 adapter，新增 2 个 Tauri command（`import_total_station_csv` + `generate_instruction_card`）。前端 Import.vue / Instruct.vue 各加一个按钮，i18n 同步，examples 里补 raw CSV fixture。

**Tech Stack:** Rust（`lmt-adapter-total-station` + Tauri 2.x）、Vue 3 + Pinia + vue-i18n、Vitest、`cargo test`

---

## Pre-context — 现状速读

**M1 adapter 公开 API（不动）：**
- `parse_csv(path) -> Vec<RawPoint>`
- `load_project(path) -> ProjectConfig`（**M1 自己的 ProjectConfig**，跟 GUI 不同）
- `build_screen_measured_points_with_outcome(screen_id, &raw, screen_cfg) -> (MeasuredPoints, NameOutcome)`
- `build_screen_report(screen_id, &outcome, screen_cfg) -> ScreenReport`
- `instruction_card::html::generate_html(&card) -> String`
- `instruction_card::pdf::generate_pdf(&card, &path) -> Result<()>`

**GUI `ProjectConfig`（不动）和 M1 `ProjectConfig` 字段差异：**

| 字段 | GUI（`src-tauri/src/dto.rs`） | M1（`crates/.../project.rs`） |
|---|---|---|
| 坐标系字段 | `origin_point` / `x_axis_point` / `xy_plane_point` | `origin_grid_name` / `x_axis_grid_name` / `xy_plane_grid_name` |
| 缺角 | `irregular_mask: Vec<[u32; 2]>` | `absent_cells: Vec<(u32, u32)>` |
| Folded 字段名 | `fold_seams_at_columns` | `fold_seam_columns` |
| Curved | `{radius_mm, fold_seams_at_columns}` | `{radius_mm}` 无 fold |
| 其它 | `unit`、`pixels_per_cabinet`、`shape_mode`、`output` | 全无 |
| screens 容器 | `BTreeMap` | `HashMap` |

**新增 Tauri 命令的契约（在本计划里冻结）：**

```rust
#[tauri::command]
pub fn import_total_station_csv(
    project_abs_path: String,
    csv_path: String,
    screen_id: String,
) -> LmtResult<TotalStationImportResult>;

#[tauri::command]
pub fn generate_instruction_card(
    project_abs_path: String,
    screen_id: String,
) -> LmtResult<InstructionCardResult>;
```

> M1.1 实际只支持单 screen，但 API 把 `screen_id` 显式参数化，避免硬编码 `"MAIN"`；前端先从 `currentProject.config.screens` 取第一个，未来加 picker 不需改 API。

```rust
pub struct TotalStationImportResult {
    pub measurements_yaml_path: String,  // 相对 project_abs_path
    pub report_json_path: String,        // 相对 project_abs_path
    pub measured_count: usize,
    pub fabricated_count: usize,
    pub outlier_count: usize,
    pub missing_count: usize,
    pub warnings: Vec<String>,
}

pub struct InstructionCardResult {
    pub html_content: String,            // 给 iframe srcdoc 用
    pub pdf_path: String,                // 相对 project_abs_path
}
```

**路径约定：**
- 输入 CSV：用户在 dialog 里选，任意位置
- 输出 `measured.yaml`：`{project_abs_path}/measurements/measured.yaml`（**覆盖**已有的）
- 输出 import report：`{project_abs_path}/measurements/import_report.json`
- 输出 PDF：`{project_abs_path}/output/instruction.pdf`
- HTML 不落盘（前端 iframe 直接渲染）

---

## File Structure

**新建（5 个文件）：**
- `src-tauri/src/commands/total_station.rs` — Tauri commands + 调 adapter
- `src-tauri/src/commands/total_station_mapper.rs` — GUI ProjectConfig → M1 ProjectConfig
- `src-tauri/tests/total_station_test.rs` — Tauri-level 集成测试
- `examples/curved-flat/measurements/raw.csv` — 真实 CSV fixture（45 点，4×8 cabinet → 9×5 vertices）
- `examples/curved-arc/measurements/raw.csv` — 弧形 CSV fixture（17 点 vertex-numbering 已存在的 fixture 对照）

**修改（10 个文件）：**
- `src-tauri/Cargo.toml` — 加 `lmt-adapter-total-station` 依赖
- `src-tauri/src/error.rs` — `impl From<AdapterError> for LmtError`
- `src-tauri/src/dto.rs` — 加 `TotalStationImportResult` + `InstructionCardResult`
- `src-tauri/src/commands/mod.rs` — 注册 `total_station` 子模块
- `src-tauri/src/lib.rs` — 注册 2 个新 invoke handler
- `src/services/tauri.ts` — 加 wrapper + 类型
- `src/stores/reconstruction.ts` — 加 `importReport` 状态
- `src/views/Import.vue` — 加 "Load Trimble CSV" 按钮 + 显示 import report
- `src/views/Instruct.vue` — 改 stub 为可生成 + 预览 HTML/PDF
- `src/locales/zh.json` + `src/locales/en.json` — 文案

**最后改 README + 打 tag**（Task 12）。

---

## Phase 1 — Backend (Tauri layer)

### Task 1: 加 M1 adapter 依赖 + Error 转换

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/error.rs`
- Test: `src-tauri/src/error.rs` 内联单测

- [ ] **Step 1: 在 `src-tauri/Cargo.toml` 加 dependency**

定位 `[dependencies]` 区块（约 line 20+），在 `lmt-core` 那行下面加：

```toml
lmt-adapter-total-station = { path = "../crates/adapter-total-station" }
```

- [ ] **Step 2: 编译验证依赖加得对**

Run: `cargo build -p lmt-tauri 2>&1 | tail -20`
Expected: PASS（无新错；可能有 unused-import warning，忽略）。

- [ ] **Step 3: 在 `src-tauri/src/error.rs` 写失败测试**

先 `cat src-tauri/src/error.rs` 看 `LmtError` 现有 variants。一般会看到 `Io / Yaml / Json / NotFound / Other` 之类。在文件**末尾**追加：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use lmt_adapter_total_station::AdapterError;

    #[test]
    fn adapter_error_converts_to_lmt_error() {
        let adapter_err = AdapterError::InvalidInput("bad csv row".into());
        let lmt_err: LmtError = adapter_err.into();
        // 应该被分类成 Other / InvalidInput 类型，且消息保留
        let s = format!("{lmt_err}");
        assert!(s.contains("bad csv row"), "got: {s}");
    }
}
```

- [ ] **Step 4: 跑测试确认失败**

Run: `cargo test -p lmt-tauri --lib error::tests::adapter_error_converts_to_lmt_error -- --nocapture 2>&1 | tail -20`
Expected: FAIL（缺 `From<AdapterError>` impl）。

- [ ] **Step 5: 在 `src-tauri/src/error.rs` 加 From impl**

在文件**末尾**（`#[cfg(test)]` 之前）加：

```rust
impl From<lmt_adapter_total_station::AdapterError> for LmtError {
    fn from(e: lmt_adapter_total_station::AdapterError) -> Self {
        LmtError::Other(format!("{e}"))
    }
}
```

> 如果 `LmtError` 没有 `Other` variant，看现有 variants 选最贴近的（`Internal` / `Adapter`）；若都没有，加一个 `#[error("{0}")] Other(String)`。**写代码前先 grep 一次确认。**

- [ ] **Step 6: 跑测试确认通过**

Run: `cargo test -p lmt-tauri --lib error::tests::adapter_error_converts_to_lmt_error -- --nocapture 2>&1 | tail -10`
Expected: `1 passed`。

- [ ] **Step 7: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/src/error.rs Cargo.lock
git commit -m "feat(tauri): wire lmt-adapter-total-station + AdapterError→LmtError conversion"
```

---

### Task 2: GUI → M1 ProjectConfig mapper

**Files:**
- Create: `src-tauri/src/commands/total_station_mapper.rs`
- Modify: `src-tauri/src/commands/mod.rs`

- [ ] **Step 1: 先写测试文件**

Create `src-tauri/src/commands/total_station_mapper.rs` with（**只写测试 + signature stub**）：

```rust
//! GUI `dto::ProjectConfig` → `lmt_adapter_total_station::ProjectConfig` 字段映射。
//!
//! 两边各自的 schema 独立演进（GUI 偏面向 UI，adapter 偏面向算法）。
//! 这个模块是唯一的桥。

use lmt_adapter_total_station::project as m1;

use crate::dto;
use crate::error::{LmtError, LmtResult};

pub fn map_to_adapter(cfg: &dto::ProjectConfig) -> LmtResult<m1::ProjectConfig> {
    todo!("implement in step 3")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn flat_screen() -> dto::ScreenConfig {
        dto::ScreenConfig {
            cabinet_count: [4, 2],
            cabinet_size_mm: [500.0, 500.0],
            pixels_per_cabinet: Some([256, 256]),
            shape_prior: dto::ShapePriorConfig::Flat,
            shape_mode: dto::ShapeMode::Rectangle,
            irregular_mask: vec![],
            bottom_completion: None,
        }
    }

    fn base_cfg(screen: dto::ScreenConfig) -> dto::ProjectConfig {
        let mut screens = BTreeMap::new();
        screens.insert("MAIN".into(), screen);
        dto::ProjectConfig {
            project: dto::ProjectMeta { name: "T".into(), unit: "mm".into() },
            screens,
            coordinate_system: dto::CoordinateSystemConfig {
                origin_point: "MAIN_V001_R001".into(),
                x_axis_point: "MAIN_V005_R001".into(),
                xy_plane_point: "MAIN_V001_R003".into(),
            },
            output: dto::OutputConfig {
                target: "disguise".into(),
                obj_filename: "{screen_id}.obj".into(),
                weld_vertices_tolerance_mm: 1.0,
                triangulate: true,
            },
        }
    }

    #[test]
    fn flat_screen_maps_minimal_fields() {
        let cfg = base_cfg(flat_screen());
        let m = map_to_adapter(&cfg).unwrap();

        assert_eq!(m.project.name, "T");
        assert_eq!(m.screens.len(), 1);
        let s = m.screens.get("MAIN").unwrap();
        assert_eq!(s.cabinet_count, [4, 2]);
        assert_eq!(s.cabinet_size_mm, [500.0, 500.0]);
        assert!(matches!(s.shape_prior, m1::ShapePriorConfig::Flat));
        assert!(s.absent_cells.is_empty());

        assert_eq!(m.coordinate_system.origin_grid_name, "MAIN_V001_R001");
        assert_eq!(m.coordinate_system.x_axis_grid_name, "MAIN_V005_R001");
        assert_eq!(m.coordinate_system.xy_plane_grid_name, "MAIN_V001_R003");
    }

    #[test]
    fn irregular_mask_to_absent_cells() {
        let mut s = flat_screen();
        s.shape_mode = dto::ShapeMode::Irregular;
        s.irregular_mask = vec![[0, 0], [3, 1]];
        let cfg = base_cfg(s);
        let m = map_to_adapter(&cfg).unwrap();
        let cells = &m.screens.get("MAIN").unwrap().absent_cells;
        assert_eq!(cells, &vec![(0u32, 0u32), (3u32, 1u32)]);
    }

    #[test]
    fn curved_without_folds_maps_to_curved() {
        let mut s = flat_screen();
        s.shape_prior = dto::ShapePriorConfig::Curved {
            radius_mm: 6000.0,
            fold_seams_at_columns: vec![],
        };
        let cfg = base_cfg(s);
        let m = map_to_adapter(&cfg).unwrap();
        match &m.screens.get("MAIN").unwrap().shape_prior {
            m1::ShapePriorConfig::Curved { radius_mm } => assert_eq!(*radius_mm, 6000.0),
            other => panic!("expected Curved, got {other:?}"),
        }
    }

    #[test]
    fn folded_renames_seam_field() {
        let mut s = flat_screen();
        s.shape_prior = dto::ShapePriorConfig::Folded { fold_seams_at_columns: vec![2, 4] };
        let cfg = base_cfg(s);
        let m = map_to_adapter(&cfg).unwrap();
        match &m.screens.get("MAIN").unwrap().shape_prior {
            m1::ShapePriorConfig::Folded { fold_seam_columns } => {
                assert_eq!(fold_seam_columns, &vec![2u32, 4u32]);
            }
            other => panic!("expected Folded, got {other:?}"),
        }
    }

    #[test]
    fn curved_with_folds_returns_error() {
        // Curved + 非空 fold_seams 在 M1 那边没有保留 radius 的表达；
        // 与其静默丢 radius 升级成 Folded，不如让用户显式选 shape_prior。
        let mut s = flat_screen();
        s.shape_prior = dto::ShapePriorConfig::Curved {
            radius_mm: 6000.0,
            fold_seams_at_columns: vec![3],
        };
        let cfg = base_cfg(s);
        let err = map_to_adapter(&cfg).unwrap_err();
        let msg = format!("{err}").to_lowercase();
        assert!(msg.contains("curved") && msg.contains("fold"), "got: {err}");
    }

    #[test]
    fn bottom_completion_passes_through() {
        let mut s = flat_screen();
        s.bottom_completion = Some(dto::BottomCompletionConfig {
            lowest_measurable_row: 2,
            fallback_method: "vertical".into(),
            assumed_height_mm: 500.0,
        });
        let cfg = base_cfg(s);
        let m = map_to_adapter(&cfg).unwrap();
        let bc = m
            .screens
            .get("MAIN")
            .unwrap()
            .bottom_completion
            .as_ref()
            .unwrap();
        assert_eq!(bc.lowest_measurable_row, 2);
    }

    #[test]
    fn validate_propagates() {
        // empty screens 应该被 adapter validate() 拒绝
        let mut cfg = base_cfg(flat_screen());
        cfg.screens.clear();
        let err = map_to_adapter(&cfg).unwrap_err();
        assert!(format!("{err}").to_lowercase().contains("no screens"));
    }
}
```

- [ ] **Step 2: 在 `commands/mod.rs` 只注册 mapper**（`total_station` 模块本身 Task 3 才创建）

`cat src-tauri/src/commands/mod.rs` 看现有内容，**只加一行**：

```rust
pub mod total_station_mapper;
```

> ⚠️ 不要现在 `pub mod total_station;` — 那个文件 Task 3 才会建，提前注册会编译失败。

- [ ] **Step 3: 跑测试确认全部失败**

Run: `cargo test -p lmt-tauri --lib commands::total_station_mapper -- --nocapture 2>&1 | tail -30`
Expected: 7 FAIL（`todo!()`，含 Self-Review 加的 bottom_completion 测试 + curved_with_folds_returns_error）。

- [ ] **Step 4: 实现 mapper**

替换 `map_to_adapter` 函数实现：

```rust
pub fn map_to_adapter(cfg: &dto::ProjectConfig) -> LmtResult<m1::ProjectConfig> {
    use std::collections::HashMap;

    let mut screens: HashMap<String, m1::ScreenConfig> = HashMap::new();
    for (id, s) in &cfg.screens {
        screens.insert(id.clone(), map_screen(s)?);
    }

    let m1_cfg = m1::ProjectConfig {
        project: m1::ProjectMeta { name: cfg.project.name.clone() },
        screens,
        coordinate_system: m1::CoordinateSystemConfig {
            origin_grid_name: cfg.coordinate_system.origin_point.clone(),
            x_axis_grid_name: cfg.coordinate_system.x_axis_point.clone(),
            xy_plane_grid_name: cfg.coordinate_system.xy_plane_point.clone(),
        },
    };

    m1_cfg.validate().map_err(|e| LmtError::Other(format!("{e}")))?;
    Ok(m1_cfg)
}

fn map_screen(s: &dto::ScreenConfig) -> LmtResult<m1::ScreenConfig> {
    let shape_prior = match &s.shape_prior {
        dto::ShapePriorConfig::Flat => m1::ShapePriorConfig::Flat,
        dto::ShapePriorConfig::Curved { radius_mm, fold_seams_at_columns } => {
            if fold_seams_at_columns.is_empty() {
                m1::ShapePriorConfig::Curved { radius_mm: *radius_mm }
            } else {
                return Err(LmtError::Other(
                    "shape_prior Curved with non-empty fold_seams_at_columns is not supported \
                     by M1 adapter (radius would be lost); pick pure Curved (drop seams) or \
                     switch to Folded".to_string(),
                ));
            }
        }
        dto::ShapePriorConfig::Folded { fold_seams_at_columns } => {
            m1::ShapePriorConfig::Folded {
                fold_seam_columns: fold_seams_at_columns.clone(),
            }
        }
    };

    let bottom_completion = s.bottom_completion.as_ref().map(|bc| m1::BottomCompletion {
        lowest_measurable_row: bc.lowest_measurable_row,
        fallback_method: m1::FallbackMethod::Vertical,
    });

    let absent_cells = s
        .irregular_mask
        .iter()
        .map(|c| (c[0], c[1]))
        .collect::<Vec<_>>();

    Ok(m1::ScreenConfig {
        cabinet_count: s.cabinet_count,
        cabinet_size_mm: s.cabinet_size_mm,
        shape_prior,
        bottom_completion,
        absent_cells,
    })
}
```

- [ ] **Step 5: 跑测试确认全过**

Run: `cargo test -p lmt-tauri --lib commands::total_station_mapper -- --nocapture 2>&1 | tail -20`
Expected: 7 passed。

- [ ] **Step 6: 跑 clippy 验证整洁**

Run: `cargo clippy -p lmt-tauri --lib --tests -- -D warnings 2>&1 | tail -10`
Expected: clean。

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/commands/total_station_mapper.rs src-tauri/src/commands/mod.rs
git commit -m "feat(tauri): GUI→M1 ProjectConfig mapper with 7 unit tests"
```

---

### Task 3: `import_total_station_csv` command

**Files:**
- Modify: `src-tauri/src/commands/total_station.rs`（新建于 Task 2 的 mod 注册）
- Modify: `src-tauri/src/dto.rs`
- Test: `src-tauri/src/commands/total_station.rs` 内联单测

- [ ] **Step 1: 在 `dto.rs` 加新 DTO**

`cat src-tauri/src/dto.rs` 末尾追加：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TotalStationImportResult {
    /// 相对 project_abs_path 的路径，e.g. "measurements/measured.yaml"
    pub measurements_yaml_path: String,
    /// 相对 project_abs_path 的路径
    pub report_json_path: String,
    pub measured_count: usize,
    pub fabricated_count: usize,
    pub outlier_count: usize,
    pub missing_count: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstructionCardResult {
    /// HTML 字符串，前端 iframe 用 srcdoc 渲染
    pub html_content: String,
    /// 相对 project_abs_path 的 PDF 路径
    pub pdf_path: String,
}
```

- [ ] **Step 2: 创建 `commands/total_station.rs` 并注册到 `mod.rs`**

先在 `src-tauri/src/commands/mod.rs` 追加一行（与 Task 2 加的 mapper 并列）：

```rust
pub mod total_station;
```

然后 create `src-tauri/src/commands/total_station.rs`:

```rust
//! M1 全站仪 CSV adapter 的 Tauri 入口。
//!
//! Pure helpers + thin `#[tauri::command]` wrappers。Helpers 受集成测试覆盖。

use std::path::Path;

use lmt_adapter_total_station::{
    builder::build_screen_measured_points_with_outcome,
    csv_parser::parse_csv,
    instruction_card::{html::generate_html, pdf::generate_pdf, InstructionCard},
    report_builder::build_screen_report,
};

use crate::commands::projects::load_project_yaml_from_path;
use crate::commands::total_station_mapper::map_to_adapter;
use crate::dto::{InstructionCardResult, TotalStationImportResult};
use crate::error::{LmtError, LmtResult};

/// 把 `csv_path` 的 Trimble CSV 转成 `{project}/measurements/measured.yaml`，
/// 同时写 `import_report.json`，返回 GUI 友好的 summary。
///
/// 已有 measured.yaml 会被备份成 `measured.yaml.bak`；写入失败时回滚。
pub fn run_import(
    project_abs_path: &Path,
    screen_id: &str,
    csv_path: &Path,
) -> LmtResult<TotalStationImportResult> {
    todo!("implement in step 4")
}

#[tauri::command]
pub fn import_total_station_csv(
    project_abs_path: String,
    csv_path: String,
    screen_id: String,
) -> LmtResult<TotalStationImportResult> {
    run_import(Path::new(&project_abs_path), &screen_id, Path::new(&csv_path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    /// 写一份最小化合法 project.yaml（4×2 cabinet，flat）+ 15 点 CSV。
    /// 4×2 cabinet → 5×3 vertices = 15 个点，全测无 fabricate。
    fn seed_project(dir: &Path) {
        let project_yaml = r#"
project:
  name: TS_Test
  unit: mm
screens:
  MAIN:
    cabinet_count: [4, 2]
    cabinet_size_mm: [500.0, 500.0]
    pixels_per_cabinet: [256, 256]
    shape_prior:
      type: flat
    shape_mode: rectangle
    irregular_mask: []
coordinate_system:
  origin_point: MAIN_V001_R001
  x_axis_point: MAIN_V005_R001
  xy_plane_point: MAIN_V001_R003
output:
  target: neutral
  obj_filename: "{screen_id}.obj"
  weld_vertices_tolerance_mm: 1.0
  triangulate: true
"#;
        fs::write(dir.join("project.yaml"), project_yaml).unwrap();
        fs::create_dir_all(dir.join("measurements")).unwrap();
    }

    fn write_csv(path: &Path) {
        // 第 1-3 个点是 reference (origin / x-axis / xy-plane)，后面 12 个填满 grid
        let csv = "\
name,x,y,z,note
1,0,0,0,origin
2,2000,0,0,x-axis
3,0,0,1000,xy-plane
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
";
        fs::write(path, csv).unwrap();
    }

    #[test]
    fn import_writes_measured_yaml_and_report() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        seed_project(project);
        let csv = project.join("measurements").join("raw.csv");
        write_csv(&csv);

        let result = run_import(project, "MAIN", &csv).unwrap();

        assert_eq!(result.measurements_yaml_path, "measurements/measured.yaml");
        assert_eq!(result.report_json_path, "measurements/import_report.json");
        assert_eq!(result.measured_count, 15);
        assert_eq!(result.fabricated_count, 0);
        assert_eq!(result.outlier_count, 0);
        assert_eq!(result.missing_count, 0);
        assert!(project.join("measurements/measured.yaml").is_file());
        assert!(project.join("measurements/import_report.json").is_file());
    }

    #[test]
    fn import_fails_when_project_yaml_missing() {
        let dir = tempdir().unwrap();
        let csv = dir.path().join("raw.csv");
        write_csv(&csv);
        let err = run_import(dir.path(), "MAIN", &csv).unwrap_err();
        assert!(format!("{err}").contains("project.yaml"), "got: {err}");
    }

    #[test]
    fn import_propagates_csv_parse_error() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        seed_project(project);
        let csv = project.join("raw.csv");
        fs::write(&csv, "garbage,not,a,csv\n").unwrap();
        let err = run_import(project, "MAIN", &csv).unwrap_err();
        let s = format!("{err}").to_lowercase();
        assert!(s.contains("instrument") || s.contains("csv") || s.contains("invalid"),
                "got: {err}");
    }

    #[test]
    fn import_fails_for_unknown_screen() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        seed_project(project);
        let csv = project.join("measurements").join("raw.csv");
        write_csv(&csv);
        let err = run_import(project, "FLOOR", &csv).unwrap_err();
        assert!(format!("{err}").contains("FLOOR"), "got: {err}");
    }

    #[test]
    fn second_import_cleans_up_backup_on_success() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        seed_project(project);
        let csv = project.join("measurements").join("raw.csv");
        write_csv(&csv);

        // 第 1 次导入建出 measured.yaml
        run_import(project, "MAIN", &csv).unwrap();
        assert!(project.join("measurements/measured.yaml").is_file());

        // 第 2 次导入：成功后 .bak 必须被清理
        run_import(project, "MAIN", &csv).unwrap();
        assert!(project.join("measurements/measured.yaml").is_file());
        assert!(
            !project.join("measurements/measured.yaml.bak").is_file(),
            "backup should be removed after successful re-import"
        );
    }
}
```

- [ ] **Step 3: 跑测试确认全部失败**

Run: `cargo test -p lmt-tauri --lib commands::total_station -- --nocapture 2>&1 | tail -30`
Expected: 5 FAIL（`todo!()`）。

- [ ] **Step 4: 实现 `run_import`**

替换 `run_import` 函数体（注意：`build_screen_report` 签名是 `(screen_id, &mp, &outcome, &cfg)` 四参数；`ScreenReport` 的 `missing` / `outliers` 是 `Vec<_>`，要 `.len()`）：

```rust
pub fn run_import(
    project_abs_path: &Path,
    screen_id: &str,
    csv_path: &Path,
) -> LmtResult<TotalStationImportResult> {
    // 1. 读 GUI project.yaml，映射到 M1 ProjectConfig
    let gui_cfg = load_project_yaml_from_path(project_abs_path)?;
    let m1_cfg = map_to_adapter(&gui_cfg)?;
    let screen_cfg = m1_cfg
        .screens
        .get(screen_id)
        .ok_or_else(|| LmtError::Other(format!("screen '{screen_id}' not in project")))?;

    // 2. 解析 CSV
    let raw = parse_csv(csv_path)?;

    // 3. 跑 build + report（report 签名是 4 参数：screen_id, &mp, &outcome, &cfg）
    let (measured, outcome) =
        build_screen_measured_points_with_outcome(screen_id, &raw, screen_cfg)?;
    let report = build_screen_report(screen_id, &measured, &outcome, screen_cfg);

    // 4. 写文件（带 backup + rollback）
    let measurements_dir = project_abs_path.join("measurements");
    std::fs::create_dir_all(&measurements_dir)?;
    let measured_yaml_path = measurements_dir.join("measured.yaml");
    let report_json_path = measurements_dir.join("import_report.json");
    let backup_path = measurements_dir.join("measured.yaml.bak");

    // 4a. 若已有 measured.yaml，先备份
    let did_backup = if measured_yaml_path.exists() {
        std::fs::rename(&measured_yaml_path, &backup_path)?;
        true
    } else {
        false
    };

    // 4b. 写新文件（写失败就 restore backup）
    let write_result = (|| -> LmtResult<()> {
        let yaml = serde_yaml::to_string(&measured)?;
        let tmp = measurements_dir.join("measured.yaml.tmp");
        std::fs::write(&tmp, yaml)?;
        std::fs::rename(&tmp, &measured_yaml_path)?;

        let report_json = serde_json::to_string_pretty(&report)?;
        let tmp = measurements_dir.join("import_report.json.tmp");
        std::fs::write(&tmp, report_json)?;
        std::fs::rename(&tmp, &report_json_path)?;
        Ok(())
    })();

    if let Err(e) = write_result {
        if did_backup {
            let _ = std::fs::rename(&backup_path, &measured_yaml_path);
        }
        return Err(e);
    }

    // 4c. 都成功 → 清理 backup
    if did_backup {
        let _ = std::fs::remove_file(&backup_path);
    }

    // 5. 返回 summary
    Ok(TotalStationImportResult {
        measurements_yaml_path: "measurements/measured.yaml".to_string(),
        report_json_path: "measurements/import_report.json".to_string(),
        measured_count: report.measured_count,
        fabricated_count: report.fabricated_count,
        outlier_count: report.outliers.len(),
        missing_count: report.missing.len(),
        warnings: report.warnings.clone(),
    })
}
```

> 已对照 `crates/adapter-total-station/src/report.rs` 的 `ScreenReport` 字段（`measured_count` / `fabricated_count` / `missing: Vec<MissingPoint>` / `outliers: Vec<OutlierPoint>` / `warnings: Vec<String>`）和 `report_builder.rs` 的 `build_screen_report(screen_id, &mp, &outcome, &cfg)` 签名。

- [ ] **Step 5: 跑测试确认全过**

Run: `cargo test -p lmt-tauri --lib commands::total_station -- --nocapture 2>&1 | tail -20`
Expected: 5 passed。

- [ ] **Step 6: clippy**

Run: `cargo clippy -p lmt-tauri --lib --tests -- -D warnings 2>&1 | tail -10`
Expected: clean。

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/dto.rs src-tauri/src/commands/total_station.rs
git commit -m "feat(tauri): import_total_station_csv (helper + 5 unit tests, with backup/rollback)"
```

---

### Task 4: `generate_instruction_card` command

**Files:**
- Modify: `src-tauri/src/commands/total_station.rs`

- [ ] **Step 1: 追加测试**

在 `src-tauri/src/commands/total_station.rs` 的 `mod tests` 内追加：

```rust
    #[test]
    fn generate_card_returns_html_and_pdf() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        seed_project(project);

        let result = run_generate_card(project, "MAIN").unwrap();

        // HTML 必须含项目名和屏体名
        assert!(result.html_content.contains("TS_Test"), "html: {}", result.html_content);
        assert!(result.html_content.contains("MAIN"));

        // PDF 文件落盘且以 %PDF- 开头
        assert_eq!(result.pdf_path, "output/instruction-MAIN.pdf");
        let pdf_bytes = fs::read(project.join("output/instruction-MAIN.pdf")).unwrap();
        assert!(pdf_bytes.starts_with(b"%PDF-"), "missing PDF magic header");
    }

    #[test]
    fn generate_card_fails_for_unknown_screen() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        seed_project(project);
        let err = run_generate_card(project, "FLOOR").unwrap_err();
        assert!(format!("{err}").contains("FLOOR"), "got: {err}");
    }
```

在文件顶部加 helper signature（与 `run_import` 并列）：

```rust
pub fn run_generate_card(
    project_abs_path: &Path,
    screen_id: &str,
) -> LmtResult<InstructionCardResult> {
    todo!("implement in step 3")
}

#[tauri::command]
pub fn generate_instruction_card(
    project_abs_path: String,
    screen_id: String,
) -> LmtResult<InstructionCardResult> {
    run_generate_card(Path::new(&project_abs_path), &screen_id)
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p lmt-tauri --lib commands::total_station::tests::generate -- --nocapture 2>&1 | tail -20`
Expected: 2 FAIL。

- [ ] **Step 3: 实现 `run_generate_card`**

替换函数体：

```rust
pub fn run_generate_card(
    project_abs_path: &Path,
    screen_id: &str,
) -> LmtResult<InstructionCardResult> {
    let gui_cfg = load_project_yaml_from_path(project_abs_path)?;
    let m1_cfg = map_to_adapter(&gui_cfg)?;
    let screen_cfg = m1_cfg
        .screens
        .get(screen_id)
        .ok_or_else(|| LmtError::Other(format!("screen '{screen_id}' not in project")))?;

    let card = InstructionCard {
        project_name: m1_cfg.project.name.clone(),
        screen_id: screen_id.to_string(),
        cfg: screen_cfg.clone(),
        origin_grid_name: m1_cfg.coordinate_system.origin_grid_name.clone(),
        x_axis_grid_name: m1_cfg.coordinate_system.x_axis_grid_name.clone(),
        xy_plane_grid_name: m1_cfg.coordinate_system.xy_plane_grid_name.clone(),
    };

    let html = generate_html(&card);

    let output_dir = project_abs_path.join("output");
    std::fs::create_dir_all(&output_dir)?;
    let pdf_filename = format!("instruction-{screen_id}.pdf");
    let pdf_abs = output_dir.join(&pdf_filename);
    generate_pdf(&card, &pdf_abs)?;

    Ok(InstructionCardResult {
        html_content: html,
        pdf_path: format!("output/{pdf_filename}"),
    })
}
```

> **再次注意**：`InstructionCard` 字段以 `crates/adapter-total-station/src/instruction_card/mod.rs` 现状为准；如果 `cfg` 字段名实际叫 `screen_cfg` 或 `screen`，调整即可。先 `cat` 看清楚。

- [ ] **Step 4: 跑测试确认全过**

Run: `cargo test -p lmt-tauri --lib commands::total_station -- --nocapture 2>&1 | tail -20`
Expected: 全部 7 个 passed（Task 3 的 5 个 + 本 Task 加的 2 个）。

并跑 lib 全量确认 Task 2 mapper 模块没退化：

Run: `cargo test -p lmt-tauri --lib 2>&1 | tail -15`
Expected: 含 7 个 mapper + 7 个 total_station + error::tests = 15 passed。

- [ ] **Step 5: clippy**

Run: `cargo clippy -p lmt-tauri --lib --tests -- -D warnings 2>&1 | tail -10`
Expected: clean。

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/commands/total_station.rs
git commit -m "feat(tauri): generate_instruction_card command (HTML+PDF)"
```

---

### Task 5: 注册 Tauri invoke handler + 端到端集成测试

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Create: `src-tauri/tests/total_station_test.rs`

- [ ] **Step 1: 注册 invoke handler**

`cat src-tauri/src/lib.rs` 看到 `tauri::generate_handler![ ... ]` 数组，在末尾加两行：

```rust
            commands::total_station::import_total_station_csv,
            commands::total_station::generate_instruction_card,
```

- [ ] **Step 2: 写集成测试**

Create `src-tauri/tests/total_station_test.rs`:

```rust
//! Tauri-layer 集成测试：直接调 pure helpers（不起 Tauri runtime），
//! 验证 GUI ProjectConfig → CSV → measured.yaml → reconstruct → OBJ
//! 全管线在 Tauri 入口处也能跑通。

use lmt_core::reconstruct::auto_reconstruct;
use lmt_tauri::commands::measurements::load_measurements_from_path;
use lmt_tauri::commands::total_station::{run_generate_card, run_import};
use std::fs;
use tempfile::tempdir;

fn seed(dir: &std::path::Path) {
    let yaml = r#"
project:
  name: E2E
  unit: mm
screens:
  MAIN:
    cabinet_count: [4, 2]
    cabinet_size_mm: [500.0, 500.0]
    pixels_per_cabinet: [256, 256]
    shape_prior:
      type: flat
    shape_mode: rectangle
    irregular_mask: []
coordinate_system:
  origin_point: MAIN_V001_R001
  x_axis_point: MAIN_V005_R001
  xy_plane_point: MAIN_V001_R003
output:
  target: neutral
  obj_filename: "{screen_id}.obj"
  weld_vertices_tolerance_mm: 1.0
  triangulate: true
"#;
    fs::write(dir.join("project.yaml"), yaml).unwrap();
    fs::create_dir_all(dir.join("measurements")).unwrap();

    let csv = "\
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
";
    fs::write(dir.join("measurements/raw.csv"), csv).unwrap();
}

#[test]
fn import_then_load_measured_yaml_then_reconstruct() {
    let dir = tempdir().unwrap();
    let project = dir.path();
    seed(project);
    let csv = project.join("measurements/raw.csv");

    let imp = run_import(project, "MAIN", &csv).unwrap();
    assert_eq!(imp.measured_count, 15);

    let mp_path = project.join(&imp.measurements_yaml_path);
    let mp = load_measurements_from_path(&mp_path).unwrap();
    assert_eq!(mp.points.len(), 15);

    let surface = auto_reconstruct(&mp).unwrap();
    assert_eq!(surface.quality_metrics.method, "direct_link");
    assert_eq!(surface.vertices.len(), 15);
}

#[test]
fn generate_card_writes_pdf_under_project() {
    let dir = tempdir().unwrap();
    let project = dir.path();
    seed(project);

    let card = run_generate_card(project, "MAIN").unwrap();
    assert!(card.html_content.contains("E2E"));
    assert!(card.html_content.contains("MAIN"));
    let pdf = fs::read(project.join(&card.pdf_path)).unwrap();
    assert!(pdf.starts_with(b"%PDF-"));
}
```

- [ ] **Step 3: 跑集成测试**

Run: `cargo test -p lmt-tauri --test total_station_test -- --nocapture 2>&1 | tail -30`
Expected: 2 passed。

> **注意**：`commands::total_station` / `commands::total_station_mapper` 必须 `pub mod` 在 `commands/mod.rs`，并且模块内函数 / `LmtTauri` lib 暴露。先 `cargo test -p lmt-tauri` build 一次确认编译通。若 `lmt-tauri` crate 名不一样，看 `src-tauri/Cargo.toml` `name = "..."` 字段对应。

- [ ] **Step 4: 全 workspace 跑一遍**

Run: `cargo test --workspace 2>&1 | tail -15`
Expected: 全绿。

Run: `cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -10`
Expected: clean。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/lib.rs src-tauri/tests/total_station_test.rs
git commit -m "feat(tauri): register M1 invoke handlers + e2e integration tests"
```

---

## Phase 2 — Frontend (Import flow)

### Task 6: TypeScript wrapper + Pinia state

**Files:**
- Modify: `src/services/tauri.ts`
- Modify: `src/stores/reconstruction.ts`

- [ ] **Step 1: 看现有 `tauri.ts` 类型导出**

Run: `cat src/services/tauri.ts | head -120`
预期能看到 `tauriApi` 对象 + 每个 invoke 一个方法。

- [ ] **Step 2: 加类型 + wrapper**

在 `tauri.ts` 合适位置（与其它类型并列）加：

```typescript
export interface TotalStationImportResult {
  measurementsYamlPath: string;
  reportJsonPath: string;
  measuredCount: number;
  fabricatedCount: number;
  outlierCount: number;
  missingCount: number;
  warnings: string[];
}

export interface InstructionCardResult {
  htmlContent: string;
  pdfPath: string;
}
```

在 `tauriApi` 对象里加方法：

```typescript
  async importTotalStationCsv(
    projectAbsPath: string,
    csvPath: string,
    screenId: string,
  ): Promise<TotalStationImportResult> {
    return invoke("import_total_station_csv", { projectAbsPath, csvPath, screenId });
  },

  async generateInstructionCard(
    projectAbsPath: string,
    screenId: string,
  ): Promise<InstructionCardResult> {
    return invoke("generate_instruction_card", { projectAbsPath, screenId });
  },
```

- [ ] **Step 3: 在 `reconstruction.ts` store 加 `importReport` 状态**

`cat src/stores/reconstruction.ts` 看现有 state。加 ref：

```typescript
import type { TotalStationImportResult } from "@/services/tauri";

// 在 state ref 区块加：
const importReport = ref<TotalStationImportResult | null>(null);

// 加 setter：
function setImportReport(r: TotalStationImportResult | null) {
  importReport.value = r;
}

// 在 return 暴露：
return {
  // ... 既有字段
  importReport,
  setImportReport,
};
```

- [ ] **Step 4: 看现有 store 测试，确认 pattern**

Run: `ls src/stores/__tests__/`
Run: `cat src/stores/__tests__/reconstruction.test.ts 2>/dev/null | head -30` （如果存在）

若 store 有单测，加一条：

```typescript
it("setImportReport stores result", () => {
  const store = useReconstructionStore();
  store.setImportReport({
    measurementsYamlPath: "measurements/measured.yaml",
    reportJsonPath: "measurements/import_report.json",
    measuredCount: 15,
    fabricatedCount: 0,
    outlierCount: 0,
    missingCount: 0,
    warnings: [],
  });
  expect(store.importReport?.measuredCount).toBe(15);
});
```

> 如果 `__tests__` 里没 reconstruction.test.ts，**跳过**（不要为这一项强行创建）。

- [ ] **Step 5: 跑前端测试**

Run: `pnpm vitest run 2>&1 | tail -15`
Expected: 全绿。

- [ ] **Step 6: 跑 typecheck**

Run: `pnpm exec vue-tsc -p tsconfig.app.json --noEmit 2>&1 | tail -10`
Expected: 0 errors。

- [ ] **Step 7: Commit**

```bash
git add src/services/tauri.ts src/stores/reconstruction.ts src/stores/__tests__/
git commit -m "feat(ui): tauri wrapper + reconstruction store for M1 import"
```

---

### Task 7: `Import.vue` 加 "Load Trimble CSV" 入口

**Files:**
- Modify: `src/views/Import.vue`

- [ ] **Step 1: 在 `<script setup>` 加 import + handler**

打开 `src/views/Import.vue`，在既有 `<script setup>` 末尾加：

```typescript
async function loadCsv() {
  if (!proj.absPath) return;
  try {
    const file = await open({
      title: "Select total-station CSV",
      filters: [{ name: "CSV", extensions: ["csv"] }],
      defaultPath: `${proj.absPath}/measurements`,
    });
    if (!file) return;
    // M1.1 单 screen：取 config.screens 第一个 key（GUI 已经保证非空）；fallback MAIN
    const screenIds = Object.keys(proj.config?.screens ?? {});
    const screenId = screenIds[0] ?? "MAIN";
    const result = await tauriApi.importTotalStationCsv(proj.absPath, String(file), screenId);
    recon.setImportReport(result);
    recon.setMeasurementsPath(result.measurementsYamlPath);
    const summary = t("import.csvSummary", {
      m: result.measuredCount,
      f: result.fabricatedCount,
      o: result.outlierCount,
      x: result.missingCount,
    });
    if (result.warnings.length > 0) {
      ui.toast("warning", `${summary} · ${result.warnings.length} warning(s)`);
    } else {
      ui.toast("success", summary);
    }
  } catch (e) {
    ui.toast("error", `${e}`);
  }
}
```

- [ ] **Step 2: 在 `<template>` 加按钮**

定位现有 "Load measured.yaml" 按钮所在 `<div class="flex flex-wrap gap-2">`，在 **它后面**加第二个按钮：

```vue
<Button variant="outline" :disabled="!proj.absPath" @click="loadCsv">
  <LmtIcon name="file-spreadsheet" :size="14" />
  {{ t("import.loadCsv") }}
</Button>
```

> 假设 `LmtIcon` 用 lucide 图标库。如果 `file-spreadsheet` 不存在，换成 `upload` 或 `file-input`。先 `grep -rn "LmtIcon name=" src/components | head -10` 看可用 icon。

- [ ] **Step 3: 在 import-report 详情区下面加一段**

在 `<aside class="... ROADMAP">` 之前，加一个新 section 显示 import report（只在 `recon.importReport != null` 时显示）：

```vue
<section
  v-if="recon.importReport"
  class="rounded-lg border bg-card p-5"
>
  <p class="text-[11px] font-bold uppercase tracking-[0.18em] text-muted-foreground mb-3">
    {{ t("import.reportHeader") }}
  </p>
  <div class="grid grid-cols-2 gap-3 sm:grid-cols-4">
    <LmtKV :label="t('import.measured')" :value="String(recon.importReport.measuredCount)" />
    <LmtKV :label="t('import.fabricated')" :value="String(recon.importReport.fabricatedCount)" />
    <LmtKV :label="t('import.outliers')" :value="String(recon.importReport.outlierCount)" />
    <LmtKV :label="t('import.missing')" :value="String(recon.importReport.missingCount)" />
  </div>
  <ul v-if="recon.importReport.warnings.length > 0" class="mt-3 space-y-1 text-xs text-amber-500">
    <li v-for="(w, i) in recon.importReport.warnings" :key="i">⚠ {{ w }}</li>
  </ul>
</section>
```

- [ ] **Step 4: 更新 Roadmap 区块的 M1 状态**

把：

```vue
<li class="flex items-start gap-2">
  <LmtIcon name="circle-dot" :size="13" class="mt-0.5 text-status-info" />
  <span><span class="font-bold text-foreground">M1</span> — total-station CSV adapter</span>
</li>
```

改成：

```vue
<li class="flex items-start gap-2">
  <LmtIcon name="check-circle-2" :size="13" class="mt-0.5 text-status-healthy" />
  <span><span class="font-bold text-foreground">M1</span> — total-station CSV adapter</span>
</li>
```

- [ ] **Step 5: typecheck + 跑前端测试**

Run: `pnpm exec vue-tsc -p tsconfig.app.json --noEmit 2>&1 | tail -10`
Run: `pnpm vitest run 2>&1 | tail -10`
Expected: 都 clean。

- [ ] **Step 6: Commit**

```bash
git add src/views/Import.vue
git commit -m "feat(ui): Import.vue — Load Trimble CSV button + report card"
```

---

### Task 8: i18n 文案（zh + en）

**Files:**
- Modify: `src/locales/zh.json`
- Modify: `src/locales/en.json`

- [ ] **Step 1: 看现有 `import.*` 命名空间**

Run: `python3 -c "import json; d=json.load(open('src/locales/zh.json')); print(json.dumps(d['import'], ensure_ascii=False, indent=2))"`

- [ ] **Step 2: 在两个 `import` 命名空间下追加新 key**

`src/locales/zh.json` 的 `"import"` 对象里加：

```json
"loadCsv": "导入全站仪 CSV",
"csvSummary": "已导入 {m} 个点（fabricate {f} · outlier {o} · missing {x}）",
"reportHeader": "导入报告",
"measured": "已测",
"fabricated": "补出",
"outliers": "离群",
"missing": "缺失",
```

> 同样把 `"description"` 改成："从全站仪 CSV 或手写 measured.yaml 加载测量数据。"

`src/locales/en.json` 的 `"import"` 对象里加：

```json
"loadCsv": "Load Trimble CSV",
"csvSummary": "Imported {m} pts ({f} fabricated · {o} outliers · {x} missing)",
"reportHeader": "Import Report",
"measured": "Measured",
"fabricated": "Fabricated",
"outliers": "Outliers",
"missing": "Missing",
```

把 `"description"` 改成："Load measurements from Trimble CSV or hand-written measured.yaml."

- [ ] **Step 3: typecheck**

Run: `pnpm exec vue-tsc -p tsconfig.app.json --noEmit 2>&1 | tail -10`
Expected: clean。

- [ ] **Step 4: Commit**

```bash
git add src/locales/zh.json src/locales/en.json
git commit -m "i18n: zh+en strings for M1 CSV import flow"
```

---

## Phase 3 — Frontend (Instruction Card)

### Task 9: `Instruct.vue` 改写

**Files:**
- Modify: `src/views/Instruct.vue`
- Modify: `src/views/Instruct.vue`（同一文件，再次编辑无所谓）

- [ ] **Step 1: 整段重写 Instruct.vue**

Replace `src/views/Instruct.vue` whole content with:

```vue
<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useRoute } from "vue-router";
import { useI18n } from "vue-i18n";
import { open as openFile } from "@tauri-apps/plugin-shell";
import { useCurrentProjectStore } from "@/stores/currentProject";
import { useUiStore } from "@/stores/ui";
import { tauriApi } from "@/services/tauri";
import LmtPageHeader from "@/components/primitives/LmtPageHeader.vue";
import LmtIcon from "@/components/primitives/LmtIcon.vue";
import LmtStatusBadge from "@/components/primitives/LmtStatusBadge.vue";
import Button from "@/components/ui/Button.vue";

const { t } = useI18n();
const route = useRoute();
const proj = useCurrentProjectStore();
const ui = useUiStore();

const id = computed(() => Number(route.params.id));
const html = ref<string | null>(null);
const pdfPath = ref<string | null>(null);
const screenId = ref("MAIN");

onMounted(async () => {
  try {
    if (proj.id !== id.value) await proj.load(id.value);
  } catch (e) {
    ui.toast("error", `${e}`);
  }
});

async function generate() {
  if (!proj.absPath) return;
  try {
    const result = await tauriApi.generateInstructionCard(proj.absPath, screenId.value);
    html.value = result.htmlContent;
    pdfPath.value = result.pdfPath;
    ui.toast("success", t("instruct.generated"));
  } catch (e) {
    ui.toast("error", `${e}`);
  }
}

async function openPdf() {
  if (!proj.absPath || !pdfPath.value) return;
  const abs = `${proj.absPath}/${pdfPath.value}`;
  await openFile(abs);
}
</script>

<template>
  <div class="flex h-full flex-col gap-6 p-6">
    <LmtPageHeader
      :eyebrow="t('instruct.eyebrow')"
      :title="t('instruct.title')"
      :description="t('instruct.description')"
    >
      <template #actions>
        <LmtStatusBadge tone="healthy" label="M1" icon="check-circle-2" size="md" />
      </template>
    </LmtPageHeader>

    <section class="flex flex-wrap items-center gap-2 rounded-lg border bg-card p-4">
      <Button variant="default" :disabled="!proj.absPath" @click="generate">
        <LmtIcon name="printer" :size="14" />
        {{ t("instruct.generate") }}
      </Button>
      <Button variant="outline" :disabled="!pdfPath" @click="openPdf">
        <LmtIcon name="external-link" :size="14" />
        {{ t("instruct.openPdf") }}
      </Button>
      <span v-if="pdfPath" class="ml-2 text-xs text-muted-foreground font-mono">
        {{ pdfPath }}
      </span>
    </section>

    <section
      v-if="html"
      class="flex flex-1 flex-col rounded-lg border bg-card overflow-hidden"
    >
      <iframe
        :srcdoc="html"
        class="flex-1 w-full bg-white"
        sandbox=""
      />
    </section>

    <section
      v-else
      class="flex flex-1 flex-col items-center justify-center gap-3 rounded-lg border bg-hatched py-16 text-center"
    >
      <LmtIcon name="printer" :size="40" class="text-muted-foreground" />
      <p class="font-mono text-[11px] uppercase tracking-[0.18em] text-muted-foreground">
        {{ t("instruct.empty") }}
      </p>
      <p class="max-w-md text-sm text-muted-foreground">
        {{ t("instruct.description") }}
      </p>
    </section>
  </div>
</template>
```

> 假设 `@tauri-apps/plugin-shell` 已装；如果没有，先 `pnpm tauri add shell`，然后改 capabilities。检查命令：`grep '"@tauri-apps/plugin-shell"' package.json`。若没装，**跳过 `openPdf` 按钮**，改成显示 `pdfPath` 文本让用户自己 Finder 打开。

- [ ] **Step 2: 加 i18n key（zh+en）**

`src/locales/zh.json` `"instruct"` 命名空间里：

```json
"generate": "生成指示卡",
"openPdf": "打开 PDF",
"generated": "指示卡已生成",
"empty": "点上方按钮，从当前项目生成指示卡（HTML + PDF）",
```

把 `"description"` 改成："为现场测量员生成指示卡：屏体方位图 + 3 个参考点 + 全部目标点名编号。"
把 `"pending"` 这个 key（旧的）保留或删都行。

`src/locales/en.json` `"instruct"` 命名空间里：

```json
"generate": "Generate card",
"openPdf": "Open PDF",
"generated": "Instruction card generated",
"empty": "Click above to generate the instruction card (HTML + PDF).",
```

把 `"description"` 改成："Field instruction card: screen layout + reference points + target labels."

- [ ] **Step 3: typecheck + vitest**

Run: `pnpm exec vue-tsc -p tsconfig.app.json --noEmit 2>&1 | tail -10`
Run: `pnpm vitest run 2>&1 | tail -10`
Expected: clean。

- [ ] **Step 4: Commit**

```bash
git add src/views/Instruct.vue src/locales/
git commit -m "feat(ui): Instruct.vue — generate + preview instruction card"
```

---

## Phase 4 — Fixture + 验收

### Task 10: 造 `examples/curved-flat/measurements/raw.csv` fixture

**Files:**
- Create: `examples/curved-flat/measurements/raw.csv`

- [ ] **Step 1: 看 curved-flat project.yaml 几何**

Run: `cat examples/curved-flat/project.yaml`
Expected: 8×4 cabinet, 500mm × 500mm。意味着 9 × 5 = 45 个 vertex。
坐标系参考点：origin=MAIN_V001_R001, x_axis=MAIN_V008_R001, xy_plane=MAIN_V001_R004。

注意 `x_axis_point: MAIN_V008_R001` 但 8 列 cabinet 的 vertex 应该编号 V001..V009。`V008_R001` 是右下第二个 vertex，**不是**最右。这是既有项目设计选择，照搬。

- [ ] **Step 2: 生成 45 点 CSV**

整面屏 4000mm × 2000mm，9×5 grid，间距 500mm × 500mm。
3 个参考点在 instrument 坐标系：
- ID 1 (V001_R001, origin): (0, 0, 0)
- ID 2 (V008_R001, x_axis): (3500, 0, 0)  ← V008 是第 8 个 vertex（0-indexed 第 7），x = 7×500 = 3500
- ID 3 (V001_R004, xy_plane): (0, 0, 1500)  ← R004 是第 4 个 vertex，z = 3×500 = 1500

然后剩下 42 个点按 column-major 顺序（V001_R002, V001_R003, V001_R005, V002_R001..R005, ...）依次编号 4..45。

Create `examples/curved-flat/measurements/raw.csv` with：

```csv
name,x,y,z,note
1,0,0,0,origin V001_R001
2,3500,0,0,x-axis V008_R001
3,0,0,1500,xy-plane V001_R004
4,0,0,500,V001_R002
5,0,0,1000,V001_R003
6,0,0,2000,V001_R005
7,500,0,0,V002_R001
8,500,0,500,V002_R002
9,500,0,1000,V002_R003
10,500,0,1500,V002_R004
11,500,0,2000,V002_R005
12,1000,0,0,V003_R001
13,1000,0,500,V003_R002
14,1000,0,1000,V003_R003
15,1000,0,1500,V003_R004
16,1000,0,2000,V003_R005
17,1500,0,0,V004_R001
18,1500,0,500,V004_R002
19,1500,0,1000,V004_R003
20,1500,0,1500,V004_R004
21,1500,0,2000,V004_R005
22,2000,0,0,V005_R001
23,2000,0,500,V005_R002
24,2000,0,1000,V005_R003
25,2000,0,1500,V005_R004
26,2000,0,2000,V005_R005
27,2500,0,0,V006_R001
28,2500,0,500,V006_R002
29,2500,0,1000,V006_R003
30,2500,0,1500,V006_R004
31,2500,0,2000,V006_R005
32,3000,0,0,V007_R001
33,3000,0,500,V007_R002
34,3000,0,1000,V007_R003
35,3000,0,1500,V007_R004
36,3000,0,2000,V007_R005
37,3500,0,500,V008_R002
38,3500,0,1000,V008_R003
39,3500,0,1500,V008_R004
40,3500,0,2000,V008_R005
41,4000,0,0,V009_R001
42,4000,0,500,V009_R002
43,4000,0,1000,V009_R003
44,4000,0,1500,V009_R004
45,4000,0,2000,V009_R005
```

> 注意 ID 2 是 V008_R001（已在第 2 行），所以 36 行后跳过 V008_R001 直接写 V008_R002。检查：45 个 unique ID，无重复。

- [ ] **Step 3: 跑 Rust 验证 fixture 走 M1 全管线通过**

Run:
```bash
cargo test -p lmt-tauri --test total_station_test -- --nocapture 2>&1 | tail -10
```
Expected: 仍然 2 passed（不影响既有 e2e）。

加一个新的 fixture-based 集成测试到 `src-tauri/tests/total_station_test.rs` 末尾：

```rust
#[test]
fn import_real_example_curved_flat() {
    use std::path::PathBuf;
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf();
    let example = workspace.join("examples/curved-flat");

    // copy 到 temp（避免污染源目录）
    let dir = tempdir().unwrap();
    let project = dir.path().join("curved-flat");
    copy_dir(&example, &project);

    let csv = project.join("measurements/raw.csv");
    let result = run_import(&project, "MAIN", &csv).unwrap();
    assert_eq!(result.measured_count, 45, "9×5 vertices");
    assert_eq!(result.outlier_count, 0);
    assert_eq!(result.missing_count, 0);

    // reconstruct
    let mp = load_measurements_from_path(&project.join(&result.measurements_yaml_path)).unwrap();
    let surface = auto_reconstruct(&mp).unwrap();
    assert_eq!(surface.quality_metrics.method, "direct_link");
    assert_eq!(surface.vertices.len(), 45);
}

fn copy_dir(src: &std::path::Path, dst: &std::path::Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to);
        } else {
            std::fs::copy(&from, &to).unwrap();
        }
    }
}
```

- [ ] **Step 4: 跑测试**

Run: `cargo test -p lmt-tauri --test total_station_test -- --nocapture 2>&1 | tail -15`
Expected: 3 passed。

- [ ] **Step 5: Commit**

```bash
git add examples/curved-flat/measurements/raw.csv src-tauri/tests/total_station_test.rs
git commit -m "test(m1-gui): add curved-flat raw.csv fixture + e2e integration test"
```

---

### Task 11: dev-mode 手动验收

**Files:** 无新文件。

- [ ] **Step 1: 启动 dev**

Run: `pnpm tauri dev`（后台跑或独立 terminal）

- [ ] **Step 2: 走完整验收清单**

按下面顺序点，每步都得过：

1. Home 页 → "Create Curved-Flat (8×4)" → 选 `~/Desktop/lmt-test-m1/` → 自动跳 `/design`
2. `/design` 显示 8×4 grid → 点 Save（无修改也 toast "Saved"）
3. 跳 `/import` → 点 **"导入全站仪 CSV"** → 选 `~/Desktop/lmt-test-m1/curved-flat/measurements/raw.csv` → toast "Imported 45 pts (0 fabricated · 0 outliers · 0 missing)"
4. Import 页下方出现 Import Report 区块，4 个 KV 显示 45 / 0 / 0 / 0
5. 跳 `/instruct` → 点 **"生成指示卡"** → toast "指示卡已生成" → 页面下方 iframe 渲染 HTML（看得到 Curved-Flat-Demo 名 + MAIN + 屏体方位图 / 表格）
6. 点 **"打开 PDF"** → 系统 PDF Viewer 启动，A4 一张纸（看得到屏体方位图、3 个参考点、全部 45 个目标点编号）
7. 跳 `/preview` → 点 "Reconstruct" → method=direct_link → mesh 渲染
8. 点 "Export Disguise" → toast 出 OBJ 路径
9. 跳 `/runs` → 看到 1 行 run，点开看 report JSON

每步 OK 打 ✅。

- [ ] **Step 3: 异常路径验收**

10. 回 `/import`，再次"导入全站仪 CSV" → 选一个**没有 Trimble 格式**的 CSV（自己造 `bad.csv` 写 `a,b,c\n1,2,3\n`） → toast "error: instrument id ..." 或类似清晰报错（不能 panic / 空白页）
11. 回到 Finder 查 `~/Desktop/lmt-test-m1/curved-flat/measurements/` → **measured.yaml 仍然是上一份成功导入的内容**（说明 rollback 生效；同时不应该残留 `measured.yaml.bak`）
12. `/instruct` → 改 `screenId.value = "FLOOR"` 测试不存在的 screen（暂跳过，等 UI 加 screen-picker）

- [ ] **Step 4: 关 dev mode + 记录结果**

如果有 bug，在下面列出，然后**修了再继续 Task 12**。
如果无 bug，进 Task 12。

- [ ] **Step 5: Commit（仅有 fix 时）**

```bash
git add -A
git commit -m "fix(m1-gui): issues found during E2E smoke"
```

---

### Task 12: 全 workspace 验证 + README + tag

**Files:**
- Modify: `README.md`

- [ ] **Step 1: 全测试**

Run: `cargo test --workspace 2>&1 | tail -15`
Run: `cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -10`
Run: `cargo fmt --all -- --check 2>&1 | tail -5`
Run: `pnpm vitest run 2>&1 | tail -10`
Run: `pnpm exec vue-tsc -p tsconfig.app.json --noEmit 2>&1 | tail -10`

每条都得绿。

- [ ] **Step 2: 更新 README.md Status 块**

打开 `README.md`，替换 Status 块成：

```markdown
## Status

- M0.1 Rust core — done (tag `m0.1-complete`)
- M1.1 Total-station adapter — done (tag `m1.1-complete`)
- M0.2 GUI shell + Tauri integration — done (tag `m0.2-complete`)
- M1-GUI Total-station CSV in GUI — done (tag `m1-gui-complete`)
- M2 Visual photogrammetry adapter — Part A done, Part B blocked on field PoC
```

- [ ] **Step 3: Commit + tag**

```bash
git add README.md
git commit -m "docs: README — mark M1 GUI integration complete"
git tag -a m1-gui-complete -m "M1 total-station CSV adapter wired into GUI"
git log --oneline m0.2-complete..m1-gui-complete
```

预期：列出本计划全部 commit。

---

## Self-Review

**1. Spec coverage（spec § / 需求 → task 映射）：**

- "GUI 选 Trimble CSV → MeasuredPoints" → Task 3 + Task 7
- "GUI 看导入 report（measured / fabricated / outliers / missing）" → Task 3 (DTO) + Task 7 (UI)
- "GUI 生成 + 预览 + 下载指示卡" → Task 4 (PDF) + Task 9 (UI iframe)
- "字段映射 (origin_point ↔ origin_grid_name)" → Task 2
- "shape_prior 跨 schema 一致" → Task 2 (含 Flat/Curved/Folded/Curved+folds 4 个测试)
- "irregular_mask ↔ absent_cells" → Task 2 (irregular_mask_to_absent_cells)
- "bottom_completion 透传" → Task 2 (mapper) — **缺独立测试**，需在 Task 2 加一个
- "i18n zh+en" → Task 8
- "fixture demo" → Task 10
- "端到端集成测试" → Task 5 + Task 10
- "更新 README" → Task 12
- "tag" → Task 12

**修补：** Task 2 mapper 缺 `bottom_completion` 透传的测试。**在 Task 2 Step 1 的测试块里追加：**

```rust
    #[test]
    fn bottom_completion_passes_through() {
        let mut s = flat_screen();
        s.bottom_completion = Some(dto::BottomCompletionConfig {
            lowest_measurable_row: 2,
            fallback_method: "vertical".into(),
            assumed_height_mm: 500.0,
        });
        let cfg = base_cfg(s);
        let m = map_to_adapter(&cfg).unwrap();
        let bc = m.screens.get("MAIN").unwrap().bottom_completion.as_ref().unwrap();
        assert_eq!(bc.lowest_measurable_row, 2);
    }
```

测试个数从 6 → 7。Task 2 step 3 预期数字相应改成 7 FAIL，step 5 改成 7 passed。

**2. Placeholder scan：** 通读全文，搜 "TBD" / "TODO" / "implement later" / "fill in" / "appropriate" — 无匹配。每个 step 都有具体代码或具体命令。

**3. Type consistency：**
- `TotalStationImportResult` 在 Task 3 定义，Task 5 & 6 引用 → 字段名一致（measurementsYamlPath / reportJsonPath / measuredCount / fabricatedCount / outlierCount / missingCount / warnings）
- `InstructionCardResult` 在 Task 3 定义，Task 4 & 6 & 9 引用 → 字段名一致（htmlContent / pdfPath）
- `map_to_adapter(&cfg)` 在 Task 2 定义，Task 3 & 4 引用 → 签名一致
- `run_import` / `run_generate_card` 在 Task 3 / 4 定义，Task 5 集成测试引用 → 一致
- Frontend: `tauriApi.importTotalStationCsv` / `generateInstructionCard` 在 Task 6 定义，Task 7 / 9 调用 → 一致
- i18n key 在 Task 8 / 9 定义，view 引用 → 一致

OK，无类型不一致。

---

## Execution Handoff

按用户提供的两条工作流，本计划保存后可选：

**选项 A**：直接执行（subagent-driven，每 task 一个 fresh subagent，主 session review）

**选项 B**：每完成一个 task，调 `/codex:adversarial-review` skill 让 Codex review 该 task 的 diff，吸收有用反馈、修复，再进下一个 task

无论 A / B，使用 **superpowers:subagent-driven-development** 作为执行器；B 在每个 task 完成后追加一次 codex adversarial review pass。

---

## 跨 plan 依赖

| 依赖 | 状态 |
|---|---|
| M0.1 core（`m0.1-complete`） | done |
| M1.1 adapter（`m1.1-complete`） | done |
| M0.2 GUI（`m0.2-complete`） | done |
| M2 PoC | 无依赖，独立 |

完成后：M1 全站仪路径在 GUI 里闭环；M1.2 多屏 attribution 可作为下一 plan。
