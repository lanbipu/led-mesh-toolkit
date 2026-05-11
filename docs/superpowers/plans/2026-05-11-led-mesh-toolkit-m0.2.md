# M0.2 — GUI Shell + Tauri 集成 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 m0.1-complete 之上加 GUI shell + Tauri 集成。结束状态：`pnpm tauri dev` 能从内置 example seed 出项目 → 在 `/design` 编辑 mask/refs/baseline → 在 `/preview` 看到 Three.js mesh → `/export` 写出 OBJ 文件可在 Disguise/Blender 加载。

**Architecture:** Vue 3 前端通过 Tauri command 调 Rust 后端，Rust 后端 `path` 引用 `lmt-core`（已 frozen）。YAML 是项目数据真源，SQLite 仅作 recent_projects + reconstruction_runs 索引。

**Tech Stack:** Tauri 2 / Vue 3 + TS / Vite / Tailwind / vue-i18n / Pinia / vue-router / vue-konva / Three.js / rusqlite (bundled) / serde_yaml / tokio / tracing.

**Spec 引用：** `docs/superpowers/specs/2026-05-11-led-mesh-toolkit-m0.2-design.md`

**前置：** git tag `m0.1-complete` 存在；`/Users/bip.lan/AIWorkspace/vp/ue-cache-manager` 可读（用于 component copy）。

---

## Phase 1 — Backend 基础（Cargo workspace + src-tauri 骨架）

### Task 1: 把 src-tauri 加入 Cargo workspace + 初始化 src-tauri 骨架

**Files:**
- Modify: `Cargo.toml` (workspace 根)
- Create: `src-tauri/Cargo.toml`
- Create: `src-tauri/build.rs`
- Create: `src-tauri/src/main.rs`
- Create: `src-tauri/src/lib.rs`
- Create: `src-tauri/tauri.conf.json`
- Create: `src-tauri/capabilities/default.json`

- [ ] **Step 1: 修改 workspace 根 `Cargo.toml`，把 `src-tauri` 加为第 4 个 member**

```toml
[workspace]
resolver = "2"
members = [
    "crates/core",
    "crates/adapter-total-station",
    "crates/adapter-visual-ba",
    "src-tauri",
]
```

并加 workspace deps（追加到现有 `[workspace.dependencies]`）：

```toml
tokio = { version = "1.36", features = ["full"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
rusqlite = { version = "0.31", features = ["bundled"] }
chrono = { version = "0.4", features = ["serde"] }
tempfile = "3.10"
```

- [ ] **Step 2: 写 `src-tauri/Cargo.toml`**

```toml
[package]
name = "lmt-tauri"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
authors.workspace = true
license.workspace = true
description = "LED Mesh Toolkit Tauri backend"

[lib]
name = "lmt_tauri_lib"
crate-type = ["staticlib", "cdylib", "rlib"]

[build-dependencies]
tauri-build = { version = "2.0.0", features = [] }

[dependencies]
lmt-core = { path = "../crates/core" }
tauri = { version = "2.0.0", features = [] }
serde.workspace = true
serde_json.workspace = true
serde_yaml.workspace = true
thiserror.workspace = true
tokio.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
rusqlite.workspace = true
chrono.workspace = true

[dev-dependencies]
tempfile.workspace = true

[profile.release]
opt-level = 3
lto = true
codegen-units = 1
strip = true
```

- [ ] **Step 3: 写 `src-tauri/build.rs`**

```rust
fn main() {
    tauri_build::build()
}
```

- [ ] **Step 4: 写 `src-tauri/src/main.rs`**

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    lmt_tauri_lib::run()
}
```

- [ ] **Step 5: 写最小 `src-tauri/src/lib.rs`（暂时只有 run 函数）**

```rust
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 6: 写 `src-tauri/tauri.conf.json`**

```json
{
  "$schema": "https://schema.tauri.app/config/2.0.0",
  "productName": "LED Mesh Toolkit",
  "version": "0.2.0",
  "identifier": "com.lanbipu.lmt",
  "build": {
    "beforeDevCommand": "pnpm dev",
    "beforeBuildCommand": "pnpm build",
    "devUrl": "http://localhost:5173",
    "frontendDist": "../dist"
  },
  "app": {
    "windows": [
      {
        "title": "LED Mesh Toolkit",
        "width": 1400,
        "height": 900,
        "minWidth": 1024,
        "minHeight": 600,
        "resizable": true
      }
    ],
    "security": {
      "csp": null
    }
  },
  "bundle": {
    "active": true,
    "targets": "all",
    "resources": {
      "../examples": "examples"
    }
  }
}
```

- [ ] **Step 7: 写 `src-tauri/capabilities/default.json`**

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Default capability for the app",
  "windows": ["main"],
  "permissions": ["core:default", "core:event:default", "core:path:default"]
}
```

- [ ] **Step 8: 验证 cargo build --workspace 通过（不含 frontend）**

```bash
cd /Users/bip.lan/AIWorkspace/vp/led-mesh-toolkit-m0.2
cargo build --workspace --exclude lmt-tauri
```

预期：lmt-core / adapter-* 编译成功（lmt-tauri 排除，因为 tauri-build 需要 frontend dist 才能 generate context；frontend 后续 phase 加）。

- [ ] **Step 9: Commit**

```bash
git add Cargo.toml src-tauri/
git commit -m "feat(tauri): scaffold src-tauri crate as workspace member"
```

---

### Task 2: 错误模型 LmtError + LmtResult

**Files:**
- Create: `src-tauri/src/error.rs`
- Test: 同文件内 `#[cfg(test)] mod tests`

- [ ] **Step 1: 写失败测试 `src-tauri/src/error.rs`**

```rust
use serde::Serialize;

#[derive(Debug, thiserror::Error, Serialize)]
#[serde(tag = "kind", content = "message")]
pub enum LmtError {
    #[error("io: {0}")]
    Io(String),
    #[error("yaml: {0}")]
    Yaml(String),
    #[error("core: {0}")]
    Core(String),
    #[error("db: {0}")]
    Db(String),
    #[error("not_found: {0}")]
    NotFound(String),
    #[error("invalid_input: {0}")]
    InvalidInput(String),
}

pub type LmtResult<T> = Result<T, LmtError>;

impl From<std::io::Error> for LmtError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

impl From<serde_yaml::Error> for LmtError {
    fn from(e: serde_yaml::Error) -> Self {
        Self::Yaml(e.to_string())
    }
}

impl From<serde_json::Error> for LmtError {
    fn from(e: serde_json::Error) -> Self {
        Self::Yaml(format!("json: {e}"))
    }
}

impl From<rusqlite::Error> for LmtError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Db(e.to_string())
    }
}

impl From<lmt_core::CoreError> for LmtError {
    fn from(e: lmt_core::CoreError) -> Self {
        Self::Core(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_with_kind_and_message() {
        let err = LmtError::NotFound("foo".into());
        let s = serde_json::to_string(&err).unwrap();
        assert_eq!(s, r#"{"kind":"not_found","message":"foo"}"#);
    }

    #[test]
    fn io_error_converts() {
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "x");
        let lmt: LmtError = io.into();
        assert!(matches!(lmt, LmtError::Io(_)));
    }
}
```

- [ ] **Step 2: 在 lib.rs 加 `pub mod error;`**

- [ ] **Step 3: 跑测试**

```bash
cargo test -p lmt-tauri --lib error
```

预期：2 个 test PASS。

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/error.rs src-tauri/src/lib.rs
git commit -m "feat(tauri): add LmtError + serde tagged serialization"
```

---

### Task 3: DTO struct 集（ProjectConfig / RecentProject / Reconstruction*）

**Files:**
- Create: `src-tauri/src/dto.rs`
- Create: `src-tauri/tests/dto_yaml_roundtrip.rs`

- [ ] **Step 1: 写 round-trip 失败测试 `src-tauri/tests/dto_yaml_roundtrip.rs`**

```rust
use lmt_tauri_lib::dto::*;

#[test]
fn project_config_yaml_roundtrip_matches_spec_fixture() {
    let yaml = r#"
project:
  name: "Studio_A_Volume"
  unit: "mm"
screens:
  MAIN:
    cabinet_count: [120, 20]
    cabinet_size_mm: [500, 500]
    pixels_per_cabinet: [256, 256]
    shape_prior:
      type: curved
      radius_mm: 30000
      fold_seams_at_columns: []
    shape_mode: rectangle
    irregular_mask: []
    bottom_completion:
      lowest_measurable_row: 5
      fallback_method: vertical
      assumed_height_mm: 2000
coordinate_system:
  origin_point: "MAIN_V001_R005"
  x_axis_point: "MAIN_V120_R005"
  xy_plane_point: "MAIN_V001_R020"
output:
  target: disguise
  obj_filename: "{screen_id}_mesh.obj"
  weld_vertices_tolerance_mm: 1.0
  triangulate: true
"#;

    let cfg: ProjectConfig = serde_yaml::from_str(yaml).expect("parse");
    assert_eq!(cfg.project.name, "Studio_A_Volume");
    assert_eq!(cfg.screens["MAIN"].cabinet_count, [120, 20]);
    assert_eq!(cfg.coordinate_system.origin_point, "MAIN_V001_R005");

    let back = serde_yaml::to_string(&cfg).expect("serialize");
    let cfg2: ProjectConfig = serde_yaml::from_str(&back).expect("reparse");
    assert_eq!(cfg2.project.name, cfg.project.name);
}
```

- [ ] **Step 2: 跑测试确认失败（dto 模块不存在）**

```bash
cargo test -p lmt-tauri --test dto_yaml_roundtrip
```

预期：编译失败 `unresolved import lmt_tauri_lib::dto`。

- [ ] **Step 3: 实现 `src-tauri/src/dto.rs`**

```rust
use lmt_core::{
    measured_points::MeasuredPoints, surface::ReconstructedSurface,
    surface::QualityMetrics,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentProject {
    pub id: i64,
    pub abs_path: String,
    pub display_name: String,
    pub last_opened_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub project: ProjectMeta,
    pub screens: BTreeMap<String, ScreenConfig>,
    pub coordinate_system: CoordinateSystemConfig,
    pub output: OutputConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMeta {
    pub name: String,
    pub unit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenConfig {
    pub cabinet_count: [u32; 2],
    pub cabinet_size_mm: [f64; 2],
    #[serde(default)]
    pub pixels_per_cabinet: Option<[u32; 2]>,
    pub shape_prior: ShapePriorConfig,
    pub shape_mode: ShapeMode,
    #[serde(default)]
    pub irregular_mask: Vec<[u32; 2]>,
    #[serde(default)]
    pub bottom_completion: Option<BottomCompletionConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ShapePriorConfig {
    Flat,
    Curved {
        radius_mm: f64,
        #[serde(default)]
        fold_seams_at_columns: Vec<u32>,
    },
    Folded {
        fold_seams_at_columns: Vec<u32>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ShapeMode {
    Rectangle,
    Irregular,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BottomCompletionConfig {
    pub lowest_measurable_row: u32,
    pub fallback_method: String,
    pub assumed_height_mm: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinateSystemConfig {
    pub origin_point: String,
    pub x_axis_point: String,
    pub xy_plane_point: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputConfig {
    pub target: String,
    pub obj_filename: String,
    pub weld_vertices_tolerance_mm: f64,
    pub triangulate: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReconstructionResult {
    pub run_id: i64,
    pub surface: ReconstructedSurface,
    pub report_json_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconstructionRun {
    pub id: i64,
    pub screen_id: String,
    pub method: String,
    pub estimated_rms_mm: f64,
    pub vertex_count: i64,
    pub target: Option<String>,
    pub output_obj_path: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconstructionReport {
    pub surface: ReconstructedSurface,
    pub quality_metrics: QualityMetrics,
    pub project_path: String,
    pub screen_id: String,
    pub measurements_path: String,
    pub created_at: String,
}
```

并在 `src-tauri/src/lib.rs` 加 `pub mod dto;`。

- [ ] **Step 4: 跑测试**

```bash
cargo test -p lmt-tauri --test dto_yaml_roundtrip
```

预期：PASS。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/dto.rs src-tauri/src/lib.rs src-tauri/tests/dto_yaml_roundtrip.rs
git commit -m "feat(tauri): add ProjectConfig + Reconstruction DTOs (round-trip tested)"
```

---

## Phase 2 — Database (rusqlite)

### Task 4: rusqlite connection + schema migration 框架

**Files:**
- Create: `src-tauri/src/data/mod.rs`
- Create: `src-tauri/src/data/connection.rs`
- Create: `src-tauri/src/data/schema.rs`

- [ ] **Step 1: 写失败测试（schema migration 跑两次幂等）`src-tauri/src/data/schema.rs` 底部 mod tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn migrate_is_idempotent() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn).unwrap();
        migrate(&mut conn).unwrap(); // 跑第二次应该无副作用
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='recent_projects'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }
}
```

- [ ] **Step 2: 实现 `src-tauri/src/data/connection.rs`**

```rust
use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;

pub type Db = std::sync::Arc<Mutex<Connection>>;

pub fn open(path: &Path) -> rusqlite::Result<Db> {
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    Ok(std::sync::Arc::new(Mutex::new(conn)))
}

pub fn open_in_memory() -> rusqlite::Result<Db> {
    let conn = Connection::open_in_memory()?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    Ok(std::sync::Arc::new(Mutex::new(conn)))
}
```

- [ ] **Step 3: 实现 `src-tauri/src/data/schema.rs`**

```rust
use rusqlite::Connection;

const MIGRATIONS: &[(&str, &str)] = &[
    (
        "001_recent_projects",
        r#"
        CREATE TABLE IF NOT EXISTS recent_projects (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            abs_path TEXT NOT NULL UNIQUE,
            display_name TEXT NOT NULL,
            last_opened_at TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_recent_projects_last_opened
            ON recent_projects(last_opened_at DESC);
        "#,
    ),
    (
        "002_reconstruction_runs",
        r#"
        CREATE TABLE IF NOT EXISTS reconstruction_runs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_path TEXT NOT NULL,
            screen_id TEXT NOT NULL,
            measurements_path TEXT NOT NULL,
            method TEXT NOT NULL,
            measured_count INTEGER NOT NULL,
            expected_count INTEGER NOT NULL,
            estimated_rms_mm REAL NOT NULL,
            estimated_p95_mm REAL NOT NULL,
            vertex_count INTEGER NOT NULL,
            output_obj_path TEXT,
            report_json_path TEXT NOT NULL,
            target TEXT,
            warnings_json TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_runs_project_screen
            ON reconstruction_runs(project_path, screen_id, created_at DESC);
        "#,
    ),
];

pub fn migrate(conn: &mut Connection) -> rusqlite::Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            name TEXT PRIMARY KEY,
            applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )?;
    for (name, sql) in MIGRATIONS {
        let already: i64 = conn.query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE name = ?1",
            [name],
            |r| r.get(0),
        )?;
        if already > 0 {
            continue;
        }
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.execute("INSERT INTO schema_migrations(name) VALUES (?1)", [name])?;
        tx.commit()?;
    }
    Ok(())
}
```

- [ ] **Step 4: 写 `src-tauri/src/data/mod.rs`**

```rust
pub mod connection;
pub mod schema;

pub use connection::{open, open_in_memory, Db};
```

并在 lib.rs 加 `pub mod data;`。

- [ ] **Step 5: 跑测试**

```bash
cargo test -p lmt-tauri --lib data::schema
```

预期：PASS。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/data/ src-tauri/src/lib.rs
git commit -m "feat(tauri): add rusqlite migration framework + 2 base tables"
```

---

### Task 5: recent_projects CRUD

**Files:**
- Create: `src-tauri/src/data/recent_projects.rs`

- [ ] **Step 1: 写失败测试在文件底部**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{open_in_memory, schema};

    #[test]
    fn upsert_and_list() {
        let db = open_in_memory().unwrap();
        {
            let mut conn = db.lock().unwrap();
            schema::migrate(&mut conn).unwrap();
        }
        let conn = db.lock().unwrap();
        let p1 = upsert(&conn, "/a", "Alpha").unwrap();
        let p2 = upsert(&conn, "/b", "Beta").unwrap();
        let p1b = upsert(&conn, "/a", "Alpha v2").unwrap();
        assert_eq!(p1.id, p1b.id, "same path -> same id");
        let all = list(&conn).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].abs_path, "/a"); // last_opened_at DESC
    }

    #[test]
    fn delete_by_id() {
        let db = open_in_memory().unwrap();
        {
            let mut conn = db.lock().unwrap();
            schema::migrate(&mut conn).unwrap();
        }
        let conn = db.lock().unwrap();
        let p = upsert(&conn, "/x", "X").unwrap();
        delete(&conn, p.id).unwrap();
        assert!(list(&conn).unwrap().is_empty());
    }
}
```

- [ ] **Step 2: 实现 recent_projects.rs**

```rust
use crate::dto::RecentProject;
use crate::error::LmtResult;
use chrono::Utc;
use rusqlite::{params, Connection};

pub fn upsert(conn: &Connection, abs_path: &str, display_name: &str) -> LmtResult<RecentProject> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO recent_projects(abs_path, display_name, last_opened_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(abs_path) DO UPDATE SET
             display_name = excluded.display_name,
             last_opened_at = excluded.last_opened_at",
        params![abs_path, display_name, now],
    )?;
    let id: i64 = conn.query_row(
        "SELECT id FROM recent_projects WHERE abs_path = ?1",
        [abs_path],
        |r| r.get(0),
    )?;
    Ok(RecentProject {
        id,
        abs_path: abs_path.to_string(),
        display_name: display_name.to_string(),
        last_opened_at: now,
    })
}

pub fn list(conn: &Connection) -> LmtResult<Vec<RecentProject>> {
    let mut stmt = conn.prepare(
        "SELECT id, abs_path, display_name, last_opened_at
         FROM recent_projects ORDER BY last_opened_at DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(RecentProject {
            id: r.get(0)?,
            abs_path: r.get(1)?,
            display_name: r.get(2)?,
            last_opened_at: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn delete(conn: &Connection, id: i64) -> LmtResult<()> {
    conn.execute("DELETE FROM recent_projects WHERE id = ?1", [id])?;
    Ok(())
}
```

并在 `data/mod.rs` 加 `pub mod recent_projects;`。

- [ ] **Step 3: 跑测试**

```bash
cargo test -p lmt-tauri --lib data::recent_projects
```

预期：2 个 PASS。

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/data/recent_projects.rs src-tauri/src/data/mod.rs
git commit -m "feat(tauri): add recent_projects upsert/list/delete"
```

---

### Task 6: reconstruction_runs CRUD

**Files:**
- Create: `src-tauri/src/data/runs.rs`

- [ ] **Step 1: 写失败测试在文件底部**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{open_in_memory, schema};

    #[test]
    fn insert_list_update() {
        let db = open_in_memory().unwrap();
        {
            let mut conn = db.lock().unwrap();
            schema::migrate(&mut conn).unwrap();
        }
        let conn = db.lock().unwrap();
        let id = insert(
            &conn,
            &NewRun {
                project_path: "/p".into(),
                screen_id: "MAIN".into(),
                measurements_path: "measurements/m.yaml".into(),
                method: "direct_link".into(),
                measured_count: 100,
                expected_count: 100,
                estimated_rms_mm: 1.5,
                estimated_p95_mm: 3.0,
                vertex_count: 200,
                report_json_path: "reports/r.json".into(),
                warnings_json: "[]".into(),
            },
        )
        .unwrap();
        let runs = list_by_project(&conn, "/p", None).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].method, "direct_link");
        assert!(runs[0].output_obj_path.is_none());

        update_export(&conn, id, "disguise", "output/foo.obj").unwrap();
        let runs = list_by_project(&conn, "/p", Some("MAIN")).unwrap();
        assert_eq!(runs[0].target.as_deref(), Some("disguise"));
        assert_eq!(runs[0].output_obj_path.as_deref(), Some("output/foo.obj"));
    }
}
```

- [ ] **Step 2: 实现 runs.rs**

```rust
use crate::dto::ReconstructionRun;
use crate::error::LmtResult;
use rusqlite::{params, Connection};

pub struct NewRun {
    pub project_path: String,
    pub screen_id: String,
    pub measurements_path: String,
    pub method: String,
    pub measured_count: usize,
    pub expected_count: usize,
    pub estimated_rms_mm: f64,
    pub estimated_p95_mm: f64,
    pub vertex_count: usize,
    pub report_json_path: String,
    pub warnings_json: String,
}

pub fn insert(conn: &Connection, run: &NewRun) -> LmtResult<i64> {
    conn.execute(
        "INSERT INTO reconstruction_runs(
            project_path, screen_id, measurements_path, method,
            measured_count, expected_count, estimated_rms_mm, estimated_p95_mm,
            vertex_count, report_json_path, warnings_json
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![
            run.project_path,
            run.screen_id,
            run.measurements_path,
            run.method,
            run.measured_count as i64,
            run.expected_count as i64,
            run.estimated_rms_mm,
            run.estimated_p95_mm,
            run.vertex_count as i64,
            run.report_json_path,
            run.warnings_json,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_export(
    conn: &Connection,
    run_id: i64,
    target: &str,
    output_obj_path: &str,
) -> LmtResult<()> {
    let n = conn.execute(
        "UPDATE reconstruction_runs
         SET target = ?1, output_obj_path = ?2
         WHERE id = ?3",
        params![target, output_obj_path, run_id],
    )?;
    if n == 0 {
        return Err(crate::error::LmtError::NotFound(format!(
            "run id {run_id}"
        )));
    }
    Ok(())
}

pub fn list_by_project(
    conn: &Connection,
    project_path: &str,
    screen_id: Option<&str>,
) -> LmtResult<Vec<ReconstructionRun>> {
    let mut sql = String::from(
        "SELECT id, screen_id, method, estimated_rms_mm, vertex_count, target, output_obj_path, created_at
         FROM reconstruction_runs WHERE project_path = ?1",
    );
    if screen_id.is_some() {
        sql.push_str(" AND screen_id = ?2");
    }
    sql.push_str(" ORDER BY created_at DESC");
    let mut stmt = conn.prepare(&sql)?;
    let map = |r: &rusqlite::Row<'_>| {
        Ok(ReconstructionRun {
            id: r.get(0)?,
            screen_id: r.get(1)?,
            method: r.get(2)?,
            estimated_rms_mm: r.get(3)?,
            vertex_count: r.get(4)?,
            target: r.get(5)?,
            output_obj_path: r.get(6)?,
            created_at: r.get(7)?,
        })
    };
    let rows: Vec<_> = if let Some(s) = screen_id {
        stmt.query_map(params![project_path, s], map)?
            .collect::<rusqlite::Result<Vec<_>>>()?
    } else {
        stmt.query_map(params![project_path], map)?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    Ok(rows)
}

pub fn get_report_path(conn: &Connection, run_id: i64) -> LmtResult<(String, String)> {
    conn.query_row(
        "SELECT project_path, report_json_path FROM reconstruction_runs WHERE id = ?1",
        [run_id],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
    )
    .map_err(|_| crate::error::LmtError::NotFound(format!("run id {run_id}")))
}
```

并在 `data/mod.rs` 加 `pub mod runs;`。

- [ ] **Step 3: 跑测试**

```bash
cargo test -p lmt-tauri --lib data::runs
```

预期：PASS。

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/data/runs.rs src-tauri/src/data/mod.rs
git commit -m "feat(tauri): add reconstruction_runs CRUD (insert/update_export/list/get_report_path)"
```

---

## Phase 3 — Tauri commands

### Task 7: examples 固定数据集（curved-flat + curved-arc）

**Files:**
- Create: `examples/curved-flat/project.yaml`
- Create: `examples/curved-flat/measurements/measured.yaml`
- Create: `examples/curved-arc/project.yaml`
- Create: `examples/curved-arc/measurements/measured.yaml`

- [ ] **Step 1: 写 `examples/curved-flat/project.yaml`**（8×4 平面屏）

```yaml
project:
  name: Curved-Flat-Demo
  unit: mm
screens:
  MAIN:
    cabinet_count: [8, 4]
    cabinet_size_mm: [500, 500]
    pixels_per_cabinet: [256, 256]
    shape_prior:
      type: flat
    shape_mode: rectangle
    irregular_mask: []
coordinate_system:
  origin_point: MAIN_V001_R001
  x_axis_point: MAIN_V008_R001
  xy_plane_point: MAIN_V001_R004
output:
  target: disguise
  obj_filename: "{screen_id}_mesh.obj"
  weld_vertices_tolerance_mm: 1.0
  triangulate: true
```

- [ ] **Step 2: 写 `examples/curved-flat/measurements/measured.yaml`**（11 测点：4 corners + 7 anchors，触发 RBF 重建）

```yaml
points:
  - name: MAIN_V001_R001
    position: [0.0, 0.0, 0.0]
    uncertainty: { isotropic: 0.002 }
    source: total_station
  - name: MAIN_V008_R001
    position: [4.0, 0.0, 0.0]
    uncertainty: { isotropic: 0.002 }
    source: total_station
  - name: MAIN_V001_R004
    position: [0.0, 0.0, 2.0]
    uncertainty: { isotropic: 0.002 }
    source: total_station
  - name: MAIN_V008_R004
    position: [4.0, 0.0, 2.0]
    uncertainty: { isotropic: 0.002 }
    source: total_station
  - name: MAIN_V004_R001
    position: [2.0, 0.0, 0.0]
    uncertainty: { isotropic: 0.002 }
    source: total_station
  - name: MAIN_V004_R004
    position: [2.0, 0.0, 2.0]
    uncertainty: { isotropic: 0.002 }
    source: total_station
  - name: MAIN_V001_R002
    position: [0.0, 0.0, 0.667]
    uncertainty: { isotropic: 0.002 }
    source: total_station
  - name: MAIN_V008_R002
    position: [4.0, 0.0, 0.667]
    uncertainty: { isotropic: 0.002 }
    source: total_station
  - name: MAIN_V004_R002
    position: [2.0, 0.0, 0.667]
    uncertainty: { isotropic: 0.002 }
    source: total_station
  - name: MAIN_V002_R002
    position: [0.5, 0.0, 0.667]
    uncertainty: { isotropic: 0.002 }
    source: total_station
  - name: MAIN_V006_R002
    position: [2.5, 0.0, 0.667]
    uncertainty: { isotropic: 0.002 }
    source: total_station
coordinate_frame:
  origin: [0.0, 0.0, 0.0]
  x_axis: [1.0, 0.0, 0.0]
  z_axis: [0.0, 0.0, 1.0]
screen_id: MAIN
shape_prior:
  type: flat
cabinet_array:
  cols: 8
  rows: 4
  cabinet_size_mm: [500.0, 500.0]
  irregular_mask: []
```

> **注**：`measured.yaml` 的 schema 必须能被 `lmt_core::measured_points::MeasuredPoints` deserialize 通过。如发现字段名不匹配，先 `cargo test -p lmt-core` 看现有 fixture（`crates/core/tests/`）找规范字段，再调本文件。

- [ ] **Step 3: 写 `examples/curved-arc/project.yaml`**（16×6 弧形屏）

```yaml
project:
  name: Curved-Arc-Demo
  unit: mm
screens:
  MAIN:
    cabinet_count: [16, 6]
    cabinet_size_mm: [500, 500]
    pixels_per_cabinet: [256, 256]
    shape_prior:
      type: curved
      radius_mm: 12000
      fold_seams_at_columns: []
    shape_mode: rectangle
    irregular_mask: []
coordinate_system:
  origin_point: MAIN_V001_R001
  x_axis_point: MAIN_V016_R001
  xy_plane_point: MAIN_V001_R006
output:
  target: disguise
  obj_filename: "{screen_id}_mesh.obj"
  weld_vertices_tolerance_mm: 1.0
  triangulate: true
```

- [ ] **Step 4: 写 `examples/curved-arc/measurements/measured.yaml`**（≥9 测点，按 12m 半径弧形分布）

> 通过下面的 Python 脚本生成实际坐标（避免手算误差）：

```python
# scratch — 不入库，只为 plan 给出生成方法
import math
R = 12.0  # m
cab = 0.5  # m
half_arc = (16 * cab) / 2 / R  # rad
# 4 corners + 5 中段
def pos(col, row):
    s = (col - 1) * cab - 8 * cab + cab/2 + (cab/2)  # not used: simplify
    theta = ((col - 1 + 0.5 - 8) / 16) * (16 * cab) / R  # arc length / R
    x = R * math.sin(theta)
    y = R * (1 - math.cos(theta))
    z = (row - 1) * cab
    return (x, y, z)
```

实际写入 yaml 时取 `(col, row)` ∈ {(1,1),(16,1),(1,6),(16,6),(8,1),(8,6),(1,3),(16,3),(8,3)}（9 测点）。生成的 yaml 内容大致结构同 curved-flat 的 measured.yaml，仅 position 不同。

> **执行时操作**：在仓库 root 跑上面 Python（`python3 -c "..."`）算出 9 个 position，手贴进 yaml。

- [ ] **Step 5: 跑端到端 sanity check（手动 yaml 解析）**

```bash
cd /Users/bip.lan/AIWorkspace/vp/led-mesh-toolkit-m0.2
cargo test -p lmt-core --test '*' -- --nocapture 2>&1 | tail -20
```

预期：lmt-core 测试不受影响（fixture 在 examples/ 不在 crates/core/tests/）。

- [ ] **Step 6: 提交**

```bash
git add examples/
git commit -m "feat(examples): add curved-flat (8x4 flat) + curved-arc (16x6 R12m) demo projects"
```

---

### Task 8: seed_example_project command

**Files:**
- Create: `src-tauri/src/commands/mod.rs`
- Create: `src-tauri/src/commands/projects.rs`

- [ ] **Step 1: 写失败测试`src-tauri/tests/seed_example.rs`**

```rust
use lmt_tauri_lib::commands::projects::seed_example_to_dir;
use tempfile::TempDir;

#[test]
fn seeds_curved_flat_into_target_dir() {
    let src = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../examples");
    let dst = TempDir::new().unwrap();
    let out = seed_example_to_dir(&src, "curved-flat", dst.path()).unwrap();
    assert!(out.join("project.yaml").exists());
    assert!(out.join("measurements/measured.yaml").exists());
}

#[test]
fn rejects_unknown_example() {
    let src = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../examples");
    let dst = TempDir::new().unwrap();
    let err = seed_example_to_dir(&src, "nonexistent", dst.path()).unwrap_err();
    let msg = format!("{err:?}");
    assert!(msg.contains("not_found") || msg.contains("NotFound"), "got: {msg}");
}
```

- [ ] **Step 2: 实现 `src-tauri/src/commands/mod.rs`**

```rust
pub mod projects;
```

- [ ] **Step 3: 实现 `src-tauri/src/commands/projects.rs`**（暂只 seed，其他 task 追加）

```rust
use crate::dto::RecentProject;
use crate::error::{LmtError, LmtResult};
use std::path::{Path, PathBuf};

/// Pure helper used by command + integration tests.
pub fn seed_example_to_dir(
    examples_root: &Path,
    example_name: &str,
    target_dir: &Path,
) -> LmtResult<PathBuf> {
    let src = examples_root.join(example_name);
    if !src.is_dir() {
        return Err(LmtError::NotFound(format!(
            "example '{example_name}' (looked in {})",
            examples_root.display()
        )));
    }
    let dst = target_dir.join(example_name);
    copy_dir_recursive(&src, &dst)?;
    Ok(dst)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> LmtResult<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

#[tauri::command]
pub fn seed_example_project(
    app: tauri::AppHandle,
    target_dir: String,
    example: String,
) -> LmtResult<String> {
    use tauri::Manager;
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| LmtError::Io(e.to_string()))?;
    let examples_root = resource_dir.join("examples");
    let out = seed_example_to_dir(&examples_root, &example, Path::new(&target_dir))?;
    let _ = app.emit("project-seeded", serde_json::json!({"abs_path": out.display().to_string()}));
    Ok(out.display().to_string())
}
```

并在 `lib.rs` 加 `pub mod commands;`。

- [ ] **Step 4: 跑测试**

```bash
cargo test -p lmt-tauri --test seed_example
```

预期：2 个 PASS。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/commands/ src-tauri/src/lib.rs src-tauri/tests/seed_example.rs
git commit -m "feat(tauri): add seed_example_project (recursive copy + AppHandle resource resolve)"
```

---

### Task 9: load_project_yaml + save_project_yaml + recent_projects 三 command

**Files:**
- Modify: `src-tauri/src/commands/projects.rs` (追加)
- Create: `src-tauri/tests/project_yaml.rs`

- [ ] **Step 1: 写失败测试 `src-tauri/tests/project_yaml.rs`**

```rust
use lmt_tauri_lib::commands::projects::{load_project_yaml_from_path, save_project_yaml_to_path};
use lmt_tauri_lib::dto::ProjectConfig;
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn save_then_load_round_trips() {
    let dir = TempDir::new().unwrap();
    let yaml = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/curved-flat/project.yaml"),
    )
    .unwrap();
    let cfg: ProjectConfig = serde_yaml::from_str(&yaml).unwrap();

    save_project_yaml_to_path(dir.path(), &cfg).unwrap();
    assert!(dir.path().join("project.yaml").exists());
    let loaded = load_project_yaml_from_path(dir.path()).unwrap();
    assert_eq!(loaded.project.name, cfg.project.name);
}

#[test]
fn load_missing_returns_not_found() {
    let dir = TempDir::new().unwrap();
    let err = load_project_yaml_from_path(dir.path()).unwrap_err();
    assert!(matches!(
        err,
        lmt_tauri_lib::error::LmtError::NotFound(_) | lmt_tauri_lib::error::LmtError::Io(_)
    ));
}
```

- [ ] **Step 2: 在 `commands/projects.rs` 追加纯函数 + command 包装**

```rust
use crate::dto::ProjectConfig;

pub fn load_project_yaml_from_path(abs_path: &Path) -> LmtResult<ProjectConfig> {
    let yaml_path = abs_path.join("project.yaml");
    if !yaml_path.is_file() {
        return Err(LmtError::NotFound(yaml_path.display().to_string()));
    }
    let yaml = std::fs::read_to_string(&yaml_path)?;
    Ok(serde_yaml::from_str(&yaml)?)
}

pub fn save_project_yaml_to_path(abs_path: &Path, config: &ProjectConfig) -> LmtResult<()> {
    std::fs::create_dir_all(abs_path)?;
    let yaml = serde_yaml::to_string(config)?;
    let final_path = abs_path.join("project.yaml");
    let tmp_path = abs_path.join("project.yaml.tmp");
    std::fs::write(&tmp_path, yaml)?;
    std::fs::rename(&tmp_path, &final_path)?;
    Ok(())
}

#[tauri::command]
pub fn load_project_yaml(abs_path: String) -> LmtResult<ProjectConfig> {
    load_project_yaml_from_path(Path::new(&abs_path))
}

#[tauri::command]
pub fn save_project_yaml(abs_path: String, config: ProjectConfig) -> LmtResult<()> {
    save_project_yaml_to_path(Path::new(&abs_path), &config)
}
```

- [ ] **Step 3: 加 recent_projects 三 command（同文件）**

```rust
use crate::data::{recent_projects, Db};

#[tauri::command]
pub fn list_recent_projects(state: tauri::State<'_, Db>) -> LmtResult<Vec<RecentProject>> {
    let conn = state.lock().unwrap();
    recent_projects::list(&conn)
}

#[tauri::command]
pub fn add_recent_project(
    state: tauri::State<'_, Db>,
    abs_path: String,
    display_name: String,
) -> LmtResult<RecentProject> {
    let conn = state.lock().unwrap();
    recent_projects::upsert(&conn, &abs_path, &display_name)
}

#[tauri::command]
pub fn remove_recent_project(state: tauri::State<'_, Db>, id: i64) -> LmtResult<()> {
    let conn = state.lock().unwrap();
    recent_projects::delete(&conn, id)
}
```

- [ ] **Step 4: 跑测试**

```bash
cargo test -p lmt-tauri --test project_yaml
cargo test -p lmt-tauri --lib data::recent_projects
```

预期：所有 PASS。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/commands/projects.rs src-tauri/tests/project_yaml.rs
git commit -m "feat(tauri): add load/save project yaml + recent_projects commands"
```

---

### Task 10: load_measurements_yaml command

**Files:**
- Create: `src-tauri/src/commands/measurements.rs`
- Create: `src-tauri/tests/measurements_yaml.rs`

- [ ] **Step 1: 写失败测试 `src-tauri/tests/measurements_yaml.rs`**

```rust
use lmt_tauri_lib::commands::measurements::load_measurements_from_path;
use std::path::PathBuf;

#[test]
fn loads_curved_flat_fixture() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../examples/curved-flat/measurements/measured.yaml");
    let mp = load_measurements_from_path(&path).unwrap();
    assert_eq!(mp.points.len(), 11);
    assert_eq!(mp.screen_id, "MAIN");
}
```

- [ ] **Step 2: 实现 `commands/measurements.rs`**

```rust
use crate::error::{LmtError, LmtResult};
use lmt_core::measured_points::MeasuredPoints;
use std::path::Path;

pub fn load_measurements_from_path(path: &Path) -> LmtResult<MeasuredPoints> {
    if !path.is_file() {
        return Err(LmtError::NotFound(path.display().to_string()));
    }
    let yaml = std::fs::read_to_string(path)?;
    Ok(serde_yaml::from_str(&yaml)?)
}

#[tauri::command]
pub fn load_measurements_yaml(path: String) -> LmtResult<MeasuredPoints> {
    load_measurements_from_path(Path::new(&path))
}
```

并在 `commands/mod.rs` 加 `pub mod measurements;`。

- [ ] **Step 3: 跑测试**

```bash
cargo test -p lmt-tauri --test measurements_yaml
```

预期：PASS。如果失败，检查 `examples/curved-flat/measurements/measured.yaml` 字段名是否跟 `lmt_core::MeasuredPoints` 的 `Deserialize` 匹配；不匹配按 lmt-core 现有 fixture 调 example yaml。

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/commands/measurements.rs src-tauri/src/commands/mod.rs src-tauri/tests/measurements_yaml.rs
git commit -m "feat(tauri): add load_measurements_yaml command"
```

---

### Task 11: reconstruct_surface command（写 ReconstructionReport JSON + DB run）

**Files:**
- Create: `src-tauri/src/commands/reconstruct.rs`
- Create: `src-tauri/tests/reconstruct_e2e.rs`

- [ ] **Step 1: 写失败测试 `src-tauri/tests/reconstruct_e2e.rs`**

```rust
use lmt_tauri_lib::commands::reconstruct::run_reconstruction;
use lmt_tauri_lib::data::{open_in_memory, schema};
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn end_to_end_yaml_to_report() {
    // arrange: 把 curved-flat 复制到临时项目目录
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/curved-flat");
    let proj = TempDir::new().unwrap();
    fn cp_dir(s: &std::path::Path, d: &std::path::Path) {
        std::fs::create_dir_all(d).unwrap();
        for e in std::fs::read_dir(s).unwrap() {
            let e = e.unwrap();
            let to = d.join(e.file_name());
            if e.path().is_dir() {
                cp_dir(&e.path(), &to);
            } else {
                std::fs::copy(e.path(), &to).unwrap();
            }
        }
    }
    cp_dir(&src, proj.path());

    let db = open_in_memory().unwrap();
    {
        let mut conn = db.lock().unwrap();
        schema::migrate(&mut conn).unwrap();
    }

    let result = run_reconstruction(
        db.clone(),
        proj.path(),
        "MAIN",
        "measurements/measured.yaml",
    )
    .expect("reconstruct ok");

    assert!(result.run_id > 0);
    let report_path = proj.path().join(&result.report_json_path);
    assert!(report_path.is_file(), "report json missing");
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&report_path).unwrap()).unwrap();
    assert!(json["surface"]["vertices"].is_array());
    assert!(json["quality_metrics"]["method"].is_string());
}
```

- [ ] **Step 2: 实现 `commands/reconstruct.rs`**

```rust
use crate::commands::measurements::load_measurements_from_path;
use crate::data::{runs, Db};
use crate::dto::{ReconstructionReport, ReconstructionResult};
use crate::error::{LmtError, LmtResult};
use chrono::Utc;
use lmt_core::reconstruct::auto_reconstruct;
use std::path::{Path, PathBuf};

pub fn run_reconstruction(
    db: Db,
    project_path: &Path,
    screen_id: &str,
    measurements_rel_path: &str,
) -> LmtResult<ReconstructionResult> {
    let m_abs = project_path.join(measurements_rel_path);
    let measurements = load_measurements_from_path(&m_abs)?;
    let surface = auto_reconstruct(&measurements)?;
    let metrics = surface.quality_metrics.clone();

    let now = Utc::now();
    let stamp = now.format("%Y-%m-%dT%H-%M-%S%.3f").to_string();
    let report_rel = PathBuf::from("reports").join(format!("{stamp}.json"));
    let report_abs = project_path.join(&report_rel);
    std::fs::create_dir_all(report_abs.parent().unwrap())?;

    let report = ReconstructionReport {
        surface: surface.clone(),
        quality_metrics: metrics.clone(),
        project_path: project_path.display().to_string(),
        screen_id: screen_id.to_string(),
        measurements_path: measurements_rel_path.to_string(),
        created_at: now.to_rfc3339(),
    };
    let json = serde_json::to_vec_pretty(&report).map_err(|e| LmtError::Yaml(e.to_string()))?;
    std::fs::write(&report_abs, json)?;

    let run_id = {
        let conn = db.lock().unwrap();
        runs::insert(
            &conn,
            &runs::NewRun {
                project_path: project_path.display().to_string(),
                screen_id: screen_id.to_string(),
                measurements_path: measurements_rel_path.to_string(),
                method: metrics.method.clone(),
                measured_count: metrics.measured_count,
                expected_count: metrics.expected_count,
                estimated_rms_mm: metrics.estimated_rms_mm,
                estimated_p95_mm: metrics.estimated_p95_mm,
                vertex_count: surface.vertices.len(),
                report_json_path: report_rel.display().to_string(),
                warnings_json: serde_json::to_string(&metrics.warnings)
                    .map_err(|e| LmtError::Yaml(e.to_string()))?,
            },
        )?
    };

    Ok(ReconstructionResult {
        run_id,
        surface,
        report_json_path: report_rel.display().to_string(),
    })
}

#[tauri::command]
pub fn reconstruct_surface(
    state: tauri::State<'_, Db>,
    project_path: String,
    screen_id: String,
    measurements_path: String,
) -> LmtResult<ReconstructionResult> {
    run_reconstruction(
        state.inner().clone(),
        Path::new(&project_path),
        &screen_id,
        &measurements_path,
    )
}
```

并在 `commands/mod.rs` 加 `pub mod reconstruct;`。

- [ ] **Step 3: 跑测试**

```bash
cargo test -p lmt-tauri --test reconstruct_e2e -- --nocapture
```

预期：PASS。如果 `auto_reconstruct` 拒绝 fixture（数据不够），调 `examples/curved-flat/measurements/measured.yaml` 加测点。

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/commands/reconstruct.rs src-tauri/src/commands/mod.rs src-tauri/tests/reconstruct_e2e.rs
git commit -m "feat(tauri): reconstruct_surface command (yaml->report json + db run)"
```

---

### Task 12: export_obj command（从 run report 写出 OBJ）

**Files:**
- Create: `src-tauri/src/commands/export.rs`
- Create: `src-tauri/tests/export_obj_e2e.rs`

- [ ] **Step 1: 写失败测试 `src-tauri/tests/export_obj_e2e.rs`**

```rust
use lmt_tauri_lib::commands::export::run_export;
use lmt_tauri_lib::commands::reconstruct::run_reconstruction;
use lmt_tauri_lib::data::{open_in_memory, schema};
use std::path::PathBuf;
use tempfile::TempDir;

fn copy_example(name: &str, dst: &std::path::Path) {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../examples")
        .join(name);
    fn cp(s: &std::path::Path, d: &std::path::Path) {
        std::fs::create_dir_all(d).unwrap();
        for e in std::fs::read_dir(s).unwrap() {
            let e = e.unwrap();
            let to = d.join(e.file_name());
            if e.path().is_dir() {
                cp(&e.path(), &to);
            } else {
                std::fs::copy(e.path(), &to).unwrap();
            }
        }
    }
    cp(&src, dst);
}

#[test]
fn reconstruct_then_export_writes_obj() {
    let proj = TempDir::new().unwrap();
    copy_example("curved-flat", proj.path());

    let db = open_in_memory().unwrap();
    {
        let mut c = db.lock().unwrap();
        schema::migrate(&mut c).unwrap();
    }

    let r = run_reconstruction(
        db.clone(),
        proj.path(),
        "MAIN",
        "measurements/measured.yaml",
    )
    .unwrap();

    let obj_path = run_export(db.clone(), r.run_id, "disguise").unwrap();
    assert!(std::path::Path::new(&obj_path).is_file());
    let content = std::fs::read_to_string(&obj_path).unwrap();
    assert!(content.starts_with("# ") || content.starts_with("v "));
    assert!(content.contains("v "));
    assert!(content.contains("vt "));
    assert!(content.contains("f "));
}
```

- [ ] **Step 2: 实现 `commands/export.rs`**

```rust
use crate::data::{runs, Db};
use crate::dto::ReconstructionReport;
use crate::error::{LmtError, LmtResult};
use lmt_core::export::{surface_to_mesh_output, write_obj};
use lmt_core::measured_points::CabinetArray;
use lmt_core::surface::TargetSoftware;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;

fn parse_target(s: &str) -> LmtResult<TargetSoftware> {
    match s {
        "disguise" => Ok(TargetSoftware::Disguise),
        "unreal" => Ok(TargetSoftware::Unreal),
        "neutral" => Ok(TargetSoftware::Neutral),
        other => Err(LmtError::InvalidInput(format!("unknown target: {other}"))),
    }
}

pub fn run_export(db: Db, run_id: i64, target: &str) -> LmtResult<String> {
    let target_enum = parse_target(target)?;
    let (project_path, report_rel) = {
        let conn = db.lock().unwrap();
        runs::get_report_path(&conn, run_id)?
    };
    let project_root = PathBuf::from(&project_path);
    let report_abs = project_root.join(&report_rel);
    let report: ReconstructionReport =
        serde_json::from_slice(&std::fs::read(&report_abs)?)
            .map_err(|e| LmtError::Yaml(e.to_string()))?;

    // 读 project.yaml 拿 cabinet_array + weld tolerance
    let yaml = std::fs::read_to_string(project_root.join("project.yaml"))?;
    let cfg: crate::dto::ProjectConfig = serde_yaml::from_str(&yaml)?;
    let screen_cfg = cfg
        .screens
        .get(&report.screen_id)
        .ok_or_else(|| LmtError::NotFound(format!("screen {} in yaml", report.screen_id)))?;
    let cabinet_array = build_cabinet_array(screen_cfg)?;
    let weld_m = cfg.output.weld_vertices_tolerance_mm / 1000.0;

    let mesh = surface_to_mesh_output(&report.surface, &cabinet_array, target_enum, weld_m)?;

    let out_rel = PathBuf::from("output").join(format!("{}_{target}.obj", report.screen_id));
    let out_abs = project_root.join(&out_rel);
    std::fs::create_dir_all(out_abs.parent().unwrap())?;
    write_obj(&mesh, &out_abs)?;

    {
        let conn = db.lock().unwrap();
        runs::update_export(&conn, run_id, target, &out_rel.display().to_string())?;
    }
    Ok(out_abs.display().to_string())
}

fn build_cabinet_array(screen: &crate::dto::ScreenConfig) -> LmtResult<CabinetArray> {
    use crate::dto::ShapeMode;
    let cols = screen.cabinet_count[0];
    let rows = screen.cabinet_count[1];
    let size = screen.cabinet_size_mm;
    Ok(match screen.shape_mode {
        ShapeMode::Rectangle => CabinetArray::rectangle(cols, rows, size[0], size[1])?,
        ShapeMode::Irregular => {
            let mask: Vec<(u32, u32)> = screen
                .irregular_mask
                .iter()
                .map(|p| (p[0], p[1]))
                .collect();
            CabinetArray::irregular(cols, rows, size[0], size[1], &mask)?
        }
    })
}

#[tauri::command]
pub fn export_obj(
    state: tauri::State<'_, Db>,
    run_id: i64,
    target: String,
) -> LmtResult<String> {
    run_export(state.inner().clone(), run_id, &target)
}
```

并在 `commands/mod.rs` 加 `pub mod export;`。

> **检查点**：`CabinetArray::rectangle` / `irregular` 的实际签名以 `crates/core/src/measured_points.rs` 为准。如签名不同（例如返回 `Result` 类型不同 / 参数顺序不同），按 lmt-core API 实际写。

- [ ] **Step 3: 跑测试**

```bash
cargo test -p lmt-tauri --test export_obj_e2e -- --nocapture
```

预期：PASS，OBJ 文件生成、含 v / vt / f 行。

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/commands/export.rs src-tauri/src/commands/mod.rs src-tauri/tests/export_obj_e2e.rs
git commit -m "feat(tauri): export_obj command (yaml + report json -> mesh -> obj)"
```

---

### Task 13: list_runs + get_run_report commands

**Files:**
- Modify: `src-tauri/src/commands/reconstruct.rs` (追加)
- Create: `src-tauri/tests/runs_listing.rs`

- [ ] **Step 1: 写失败测试 `src-tauri/tests/runs_listing.rs`**

```rust
use lmt_tauri_lib::commands::reconstruct::{list_runs_for, read_run_report, run_reconstruction};
use lmt_tauri_lib::data::{open_in_memory, schema};
use std::path::PathBuf;
use tempfile::TempDir;

fn cp(s: &std::path::Path, d: &std::path::Path) {
    std::fs::create_dir_all(d).unwrap();
    for e in std::fs::read_dir(s).unwrap() {
        let e = e.unwrap();
        let to = d.join(e.file_name());
        if e.path().is_dir() {
            cp(&e.path(), &to);
        } else {
            std::fs::copy(e.path(), &to).unwrap();
        }
    }
}

#[test]
fn list_after_two_runs() {
    let proj = TempDir::new().unwrap();
    cp(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/curved-flat"),
        proj.path(),
    );
    let db = open_in_memory().unwrap();
    {
        let mut c = db.lock().unwrap();
        schema::migrate(&mut c).unwrap();
    }
    let r1 = run_reconstruction(db.clone(), proj.path(), "MAIN", "measurements/measured.yaml").unwrap();
    let r2 = run_reconstruction(db.clone(), proj.path(), "MAIN", "measurements/measured.yaml").unwrap();
    let listed = list_runs_for(db.clone(), &proj.path().display().to_string(), Some("MAIN")).unwrap();
    assert_eq!(listed.len(), 2);
    let report = read_run_report(db.clone(), r1.run_id).unwrap();
    assert!(report["surface"]["vertices"].is_array());
    let _ = r2;
}
```

- [ ] **Step 2: 在 `commands/reconstruct.rs` 追加**

```rust
use crate::data::runs;
use crate::dto::ReconstructionRun;
use std::path::PathBuf;

pub fn list_runs_for(
    db: Db,
    project_path: &str,
    screen_id: Option<&str>,
) -> LmtResult<Vec<ReconstructionRun>> {
    let conn = db.lock().unwrap();
    runs::list_by_project(&conn, project_path, screen_id)
}

pub fn read_run_report(db: Db, run_id: i64) -> LmtResult<serde_json::Value> {
    let (project_path, report_rel) = {
        let conn = db.lock().unwrap();
        runs::get_report_path(&conn, run_id)?
    };
    let p = PathBuf::from(&project_path).join(&report_rel);
    let bytes = std::fs::read(&p)?;
    Ok(serde_json::from_slice(&bytes).map_err(|e| LmtError::Yaml(e.to_string()))?)
}

#[tauri::command]
pub fn list_runs(
    state: tauri::State<'_, Db>,
    project_path: String,
    screen_id: Option<String>,
) -> LmtResult<Vec<ReconstructionRun>> {
    list_runs_for(state.inner().clone(), &project_path, screen_id.as_deref())
}

#[tauri::command]
pub fn get_run_report(state: tauri::State<'_, Db>, run_id: i64) -> LmtResult<serde_json::Value> {
    read_run_report(state.inner().clone(), run_id)
}
```

- [ ] **Step 3: 跑测试**

```bash
cargo test -p lmt-tauri --test runs_listing
```

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/commands/reconstruct.rs src-tauri/tests/runs_listing.rs
git commit -m "feat(tauri): list_runs + get_run_report commands"
```

---

### Task 14: lib.rs 注册 11 个 invoke handler + DB 启动初始化

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: 重写 `src-tauri/src/lib.rs`**

```rust
pub mod commands;
pub mod data;
pub mod dto;
pub mod error;

use std::path::PathBuf;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tauri::Builder::default()
        .setup(|app| {
            let db_path: PathBuf = app
                .path()
                .app_data_dir()
                .expect("failed to resolve app_data_dir")
                .join("lmt.sqlite");
            std::fs::create_dir_all(db_path.parent().unwrap())?;
            let db = data::open(&db_path)?;
            {
                let mut conn = db.lock().unwrap();
                data::schema::migrate(&mut conn)?;
            }
            app.manage(db);
            tracing::info!("LMT started, db at {}", db_path.display());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::projects::list_recent_projects,
            commands::projects::add_recent_project,
            commands::projects::remove_recent_project,
            commands::projects::seed_example_project,
            commands::projects::load_project_yaml,
            commands::projects::save_project_yaml,
            commands::measurements::load_measurements_yaml,
            commands::reconstruct::reconstruct_surface,
            commands::export::export_obj,
            commands::reconstruct::list_runs,
            commands::reconstruct::get_run_report,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 2: 跑 `cargo check -p lmt-tauri`**（不能 build context 因为缺 frontend，但 check 应过）

```bash
cargo check -p lmt-tauri
```

预期：通过（仅警告 unused; tauri-build 需 frontend 才能 generate context — 但 check 不触发 generate_context macro 编译）。

> 如果 `cargo check` 因 `tauri::generate_context!()` 失败，等 Phase 4（frontend 落地）后再 build。

- [ ] **Step 3: 提交**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(tauri): wire 11 commands + DB setup in tauri builder"
```

---

## Phase 4 — Frontend 基础

### Task 15: package.json + Vite + Tailwind + tsconfig

**Files:**
- Create: `package.json`
- Create: `vite.config.ts`
- Create: `tsconfig.json`
- Create: `tsconfig.app.json`
- Create: `tsconfig.node.json`
- Create: `tailwind.config.js`
- Create: `postcss.config.js`
- Create: `index.html`
- Create: `src/style.css`
- Create: `src/env.d.ts`

- [ ] **Step 1: 写 `package.json`**

```json
{
  "name": "lmt",
  "version": "0.2.0",
  "private": true,
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "vue-tsc -b && vite build",
    "preview": "vite preview",
    "tauri": "tauri",
    "test": "vitest run",
    "test:watch": "vitest",
    "typecheck": "vue-tsc -b --noEmit"
  },
  "dependencies": {
    "@tauri-apps/api": "^2.0.0",
    "@vueuse/core": "^14.3.0",
    "class-variance-authority": "^0.7.1",
    "clsx": "^2.1.1",
    "konva": "^9.3.0",
    "pinia": "^2.1.7",
    "reka-ui": "^2.9.6",
    "tailwind-merge": "^3.5.0",
    "three": "^0.165.0",
    "vue": "^3.4.21",
    "vue-i18n": "^9.14.4",
    "vue-konva": "^3.1.0",
    "vue-router": "^4.3.0"
  },
  "devDependencies": {
    "@tauri-apps/cli": "^2.0.0",
    "@types/node": "^20.0.0",
    "@types/three": "^0.165.0",
    "@vitejs/plugin-vue": "^5.0.4",
    "@vue/test-utils": "^2.4.5",
    "@vue/tsconfig": "^0.5.1",
    "autoprefixer": "^10.4.18",
    "happy-dom": "^14.3.0",
    "postcss": "^8.4.36",
    "tailwindcss": "^3.4.1",
    "tailwindcss-animate": "^1.0.7",
    "typescript": "^5.4.2",
    "vite": "^5.1.6",
    "vitest": "^1.4.0",
    "vue-tsc": "^2.0.6"
  }
}
```

- [ ] **Step 2: copy 配置文件从 UECM**（除 package.json，UECM 都能直接 copy）

```bash
cd /Users/bip.lan/AIWorkspace/vp/led-mesh-toolkit-m0.2
cp /Users/bip.lan/AIWorkspace/vp/ue-cache-manager/vite.config.ts .
cp /Users/bip.lan/AIWorkspace/vp/ue-cache-manager/tsconfig.json .
cp /Users/bip.lan/AIWorkspace/vp/ue-cache-manager/tsconfig.app.json .
cp /Users/bip.lan/AIWorkspace/vp/ue-cache-manager/tsconfig.node.json .
cp /Users/bip.lan/AIWorkspace/vp/ue-cache-manager/tailwind.config.js .
cp /Users/bip.lan/AIWorkspace/vp/ue-cache-manager/postcss.config.js .
cp /Users/bip.lan/AIWorkspace/vp/ue-cache-manager/index.html .
mkdir -p src
cp /Users/bip.lan/AIWorkspace/vp/ue-cache-manager/src/style.css src/
cp /Users/bip.lan/AIWorkspace/vp/ue-cache-manager/src/env.d.ts src/
```

修 `index.html` 把 title 改为 "LED Mesh Toolkit"。

- [ ] **Step 3: 修 `vite.config.ts`**（确认 alias `@` 指 `src/`，跟 UECM 一致）

```bash
grep -n "alias" vite.config.ts
```

预期：含 `'@': resolve(__dirname, './src')`。无需改。

- [ ] **Step 4: pnpm install**

```bash
pnpm install
```

预期：所有依赖安装成功。

- [ ] **Step 5: vue-tsc typecheck（应该跑通空 src）**

```bash
pnpm typecheck
```

预期：可能报 src 为空 — 暂时无视，下个 task 加 main.ts 后过。

- [ ] **Step 6: 提交**

```bash
git add package.json pnpm-lock.yaml vite.config.ts tsconfig.json tsconfig.app.json tsconfig.node.json tailwind.config.js postcss.config.js index.html src/style.css src/env.d.ts
git commit -m "feat(frontend): scaffold Vite + Vue 3 + TS + Tailwind + 3rd-party deps (three, konva, vue-konva)"
```

---

### Task 16: main.ts + App.vue + i18n locales

**Files:**
- Create: `src/main.ts`
- Create: `src/App.vue`
- Create: `src/locales/index.ts`
- Create: `src/locales/en.json`
- Create: `src/locales/zh.json`

- [ ] **Step 1: copy locales 框架从 UECM**

```bash
cp /Users/bip.lan/AIWorkspace/vp/ue-cache-manager/src/locales/index.ts src/locales/
```

写 `src/locales/en.json`（最小骨架，后续 task 加 key）：

```json
{
  "app": {
    "title": "LED Mesh Toolkit"
  },
  "nav": {
    "home": "Home",
    "design": "Design",
    "preview": "Preview",
    "export": "Export",
    "runs": "Runs",
    "import": "Import",
    "instruct": "Instruct (M1)",
    "charuco": "ChArUco (M2)",
    "photoplan": "Photo Plan (M2)"
  }
}
```

写 `src/locales/zh.json`：

```json
{
  "app": {
    "title": "LED Mesh 工具集"
  },
  "nav": {
    "home": "首页",
    "design": "设计",
    "preview": "预览",
    "export": "导出",
    "runs": "运行历史",
    "import": "导入",
    "instruct": "指示卡 (M1)",
    "charuco": "ChArUco (M2)",
    "photoplan": "拍摄规划 (M2)"
  }
}
```

如 UECM `locales/index.ts` 引用了 UECM 特有的 key，简化为：

```ts
import { createI18n } from "vue-i18n";
import en from "./en.json";
import zh from "./zh.json";

export const i18n = createI18n({
  legacy: false,
  locale: localStorage.getItem("lmt.lang") ?? "en",
  fallbackLocale: "en",
  messages: { en, zh },
});
```

- [ ] **Step 2: 写 `src/main.ts`**

```ts
import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import router from "./router";
import { i18n } from "./locales";
import "./style.css";

const app = createApp(App);
app.use(createPinia());
app.use(router);
app.use(i18n);
app.mount("#app");
```

- [ ] **Step 3: 写最小 `src/App.vue`**

```vue
<script setup lang="ts">
import LmtAppShell from "@/components/shell/LmtAppShell.vue";
</script>

<template>
  <LmtAppShell />
</template>
```

> Shell 文件下个 Task 写。

- [ ] **Step 4: 提交**

```bash
git add src/main.ts src/App.vue src/locales/
git commit -m "feat(frontend): main entry + vue-i18n bootstrap (en/zh stubs)"
```

---

### Task 17: Router + 9 routes + view stub 文件

**Files:**
- Create: `src/router/index.ts`
- Create: `src/views/Home.vue`
- Create: `src/views/Design.vue`
- Create: `src/views/Preview.vue`
- Create: `src/views/Export.vue`
- Create: `src/views/Runs.vue`
- Create: `src/views/Import.vue`
- Create: `src/views/Instruct.vue`
- Create: `src/views/Charuco.vue`
- Create: `src/views/Photoplan.vue`

- [ ] **Step 1: 写 `src/router/index.ts`**

```ts
import { createRouter, createWebHashHistory, type RouteRecordRaw } from "vue-router";
import Home from "@/views/Home.vue";
import Design from "@/views/Design.vue";
import Preview from "@/views/Preview.vue";
import Export from "@/views/Export.vue";
import Runs from "@/views/Runs.vue";
import Import from "@/views/Import.vue";
import Instruct from "@/views/Instruct.vue";
import Charuco from "@/views/Charuco.vue";
import Photoplan from "@/views/Photoplan.vue";

export const routes: RouteRecordRaw[] = [
  { path: "/", name: "home", component: Home },
  { path: "/projects/:id/design", name: "design", component: Design, props: true },
  { path: "/projects/:id/preview", name: "preview", component: Preview, props: true },
  { path: "/projects/:id/export", name: "export", component: Export, props: true },
  { path: "/projects/:id/runs", name: "runs", component: Runs, props: true },
  { path: "/projects/:id/import", name: "import", component: Import, props: true },
  { path: "/projects/:id/instruct", name: "instruct", component: Instruct, props: true },
  { path: "/projects/:id/charuco", name: "charuco", component: Charuco, props: true },
  { path: "/projects/:id/photoplan", name: "photoplan", component: Photoplan, props: true },
];

export default createRouter({
  history: createWebHashHistory(),
  routes,
});
```

- [ ] **Step 2: 9 个 view 文件先写最小 placeholder**

每个文件内容（`Home.vue` 例外，其他都用同模板替换 `Foo` 为 view 名）：

```vue
<script setup lang="ts">
import { useI18n } from "vue-i18n";
const { t } = useI18n();
</script>

<template>
  <div class="p-8">
    <h1 class="text-2xl font-bold">{{ t("nav.import") }}</h1>
    <p class="mt-4 text-muted-foreground">M0.2 stub. Implementation pending.</p>
  </div>
</template>
```

`Home.vue` 暂时用同样模板（详细实现在 Phase 6）。

> **注**：每个 view 修对应的 `t("nav.<key>")` 调用。

- [ ] **Step 3: 提交**

```bash
git add src/router/ src/views/
git commit -m "feat(frontend): router + 9 view stubs (M0.2 implementation in Phase 6)"
```

---

### Task 18: 从 UECM copy + rename Lmt 前缀的共用组件

**Files:**
- Create: `src/components/shell/LmtAppShell.vue`
- Create: `src/components/shell/LmtSidebar.vue`
- Create: `src/components/shell/LmtTopBar.vue`
- Create: `src/components/shell/LmtLogPanel.vue`
- Create: `src/components/primitives/*.vue` (10 个)
- Create: `src/components/ui/Button.vue`
- Create: `src/components/ui/Input.vue`
- Create: `src/composables/useColorMode.ts`
- Create: `src/composables/useLocale.ts`

- [ ] **Step 1: copy + 重命名脚本**

```bash
cd /Users/bip.lan/AIWorkspace/vp/led-mesh-toolkit-m0.2
mkdir -p src/components/shell src/components/primitives src/components/ui src/composables

# Shell
for f in AppShell Sidebar TopBar UecmLogPanel; do
  src_file="/Users/bip.lan/AIWorkspace/vp/ue-cache-manager/src/components/shell/${f/Uecm/Uecm}.vue"
done

# 简化：直接 cp + 改名 + 替换 token
cp /Users/bip.lan/AIWorkspace/vp/ue-cache-manager/src/components/shell/AppShell.vue src/components/shell/LmtAppShell.vue
cp /Users/bip.lan/AIWorkspace/vp/ue-cache-manager/src/components/shell/UecmSidebar.vue src/components/shell/LmtSidebar.vue
cp /Users/bip.lan/AIWorkspace/vp/ue-cache-manager/src/components/shell/UecmTopBar.vue src/components/shell/LmtTopBar.vue
cp /Users/bip.lan/AIWorkspace/vp/ue-cache-manager/src/components/shell/UecmLogPanel.vue src/components/shell/LmtLogPanel.vue

# Primitives — pick the ones we need
for name in Button Input; do
  cp /Users/bip.lan/AIWorkspace/vp/ue-cache-manager/src/components/ui/$name.vue src/components/ui/$name.vue
done

for f in PageHeader PathInput ProgressBar StatusBadge StatusDot ThemeToggle LanguageToggle KV; do
  cp /Users/bip.lan/AIWorkspace/vp/ue-cache-manager/src/components/primitives/Uecm$f.vue src/components/primitives/Lmt$f.vue
done

# Composables
cp /Users/bip.lan/AIWorkspace/vp/ue-cache-manager/src/composables/useColorMode.ts src/composables/
cp /Users/bip.lan/AIWorkspace/vp/ue-cache-manager/src/composables/useLocale.ts src/composables/

# Token replace within copied files
find src/components src/composables -type f \( -name "*.vue" -o -name "*.ts" \) -exec sed -i '' \
  -e 's/UecmSidebar/LmtSidebar/g' \
  -e 's/UecmTopBar/LmtTopBar/g' \
  -e 's/UecmLogPanel/LmtLogPanel/g' \
  -e 's/UecmPageHeader/LmtPageHeader/g' \
  -e 's/UecmPathInput/LmtPathInput/g' \
  -e 's/UecmProgressBar/LmtProgressBar/g' \
  -e 's/UecmStatusBadge/LmtStatusBadge/g' \
  -e 's/UecmStatusDot/LmtStatusDot/g' \
  -e 's/UecmThemeToggle/LmtThemeToggle/g' \
  -e 's/UecmLanguageToggle/LmtLanguageToggle/g' \
  -e 's/UecmKV/LmtKV/g' \
  -e 's/UecmIcon/LmtIcon/g' \
  -e 's/AppShell/LmtAppShell/g' \
  {} +
```

> **注**：sed 后视情况手工检查每个文件—— UECM 有 UecmIcon / UecmFilterChip / UecmCodeBlock 等本期不用的，不 copy；如果 sed 留下 dangling import 报错，按 typecheck 报错逐个删 import 行。

- [ ] **Step 2: 改 LmtSidebar 菜单项**为 LMT 路由

```vue
<!-- src/components/shell/LmtSidebar.vue -->
<script setup lang="ts">
import { computed } from "vue";
import { useRoute } from "vue-router";
import { useI18n } from "vue-i18n";

const route = useRoute();
const { t } = useI18n();

const projectId = computed(() => route.params.id as string | undefined);

const items = computed(() => {
  const id = projectId.value;
  if (!id) return [{ to: "/", label: t("nav.home") }];
  return [
    { to: "/", label: t("nav.home") },
    { to: `/projects/${id}/design`, label: t("nav.design") },
    { to: `/projects/${id}/import`, label: t("nav.import") },
    { to: `/projects/${id}/preview`, label: t("nav.preview") },
    { to: `/projects/${id}/export`, label: t("nav.export") },
    { to: `/projects/${id}/runs`, label: t("nav.runs") },
    { to: `/projects/${id}/instruct`, label: t("nav.instruct") },
    { to: `/projects/${id}/charuco`, label: t("nav.charuco") },
    { to: `/projects/${id}/photoplan`, label: t("nav.photoplan") },
  ];
});
</script>

<template>
  <nav class="flex w-56 flex-col gap-1 border-r bg-card p-3">
    <RouterLink
      v-for="it in items"
      :key="it.to"
      :to="it.to"
      class="rounded px-3 py-2 text-sm hover:bg-accent"
      active-class="bg-accent font-semibold"
    >
      {{ it.label }}
    </RouterLink>
  </nav>
</template>
```

- [ ] **Step 3: 跑 typecheck**

```bash
pnpm typecheck
```

预期：通过；如有 dangling import，逐个修。

- [ ] **Step 4: 跑 dev mode（Vite only，不 tauri dev）**

```bash
pnpm dev
```

打开 http://localhost:5173 确认页面渲染（应显示 LmtAppShell 空架 + Home 占位）。Ctrl-C 关停。

- [ ] **Step 5: 提交**

```bash
git add src/components/ src/composables/
git commit -m "feat(frontend): port shell + primitives from UECM (Lmt prefix)"
```

---

### Task 19: services/tauri.ts 类型化 invoke wrapper

**Files:**
- Create: `src/services/tauri.ts`
- Create: `src/services/__tests__/tauri.test.ts`

- [ ] **Step 1: 写失败测试 `src/services/__tests__/tauri.test.ts`**

```ts
import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";
import { tauriApi } from "../tauri";

describe("tauriApi", () => {
  beforeEach(() => vi.clearAllMocks());

  it("listRecentProjects calls invoke with no args", async () => {
    (invoke as any).mockResolvedValueOnce([]);
    const r = await tauriApi.listRecentProjects();
    expect(invoke).toHaveBeenCalledWith("list_recent_projects");
    expect(r).toEqual([]);
  });

  it("seedExampleProject passes target_dir + example", async () => {
    (invoke as any).mockResolvedValueOnce("/tmp/x/curved-flat");
    await tauriApi.seedExampleProject("/tmp/x", "curved-flat");
    expect(invoke).toHaveBeenCalledWith("seed_example_project", {
      targetDir: "/tmp/x",
      example: "curved-flat",
    });
  });

  it("reconstructSurface passes the 3 args", async () => {
    (invoke as any).mockResolvedValueOnce({ run_id: 1, surface: {}, report_json_path: "" });
    await tauriApi.reconstructSurface("/p", "MAIN", "m.yaml");
    expect(invoke).toHaveBeenCalledWith("reconstruct_surface", {
      projectPath: "/p",
      screenId: "MAIN",
      measurementsPath: "m.yaml",
    });
  });
});
```

- [ ] **Step 2: 实现 `src/services/tauri.ts`**

```ts
import { invoke } from "@tauri-apps/api/core";

export interface RecentProject {
  id: number;
  abs_path: string;
  display_name: string;
  last_opened_at: string;
}

export interface ProjectMeta {
  name: string;
  unit: string;
}
export type ShapePriorConfig =
  | { type: "flat" }
  | { type: "curved"; radius_mm: number; fold_seams_at_columns: number[] }
  | { type: "folded"; fold_seams_at_columns: number[] };
export interface BottomCompletionConfig {
  lowest_measurable_row: number;
  fallback_method: string;
  assumed_height_mm: number;
}
export interface ScreenConfig {
  cabinet_count: [number, number];
  cabinet_size_mm: [number, number];
  pixels_per_cabinet?: [number, number];
  shape_prior: ShapePriorConfig;
  shape_mode: "rectangle" | "irregular";
  irregular_mask: [number, number][];
  bottom_completion?: BottomCompletionConfig;
}
export interface CoordinateSystemConfig {
  origin_point: string;
  x_axis_point: string;
  xy_plane_point: string;
}
export interface OutputConfig {
  target: string;
  obj_filename: string;
  weld_vertices_tolerance_mm: number;
  triangulate: boolean;
}
export interface ProjectConfig {
  project: ProjectMeta;
  screens: Record<string, ScreenConfig>;
  coordinate_system: CoordinateSystemConfig;
  output: OutputConfig;
}

export interface ReconstructedSurface {
  screen_id: string;
  topology: { cols: number; rows: number };
  vertices: [number, number, number][];
  uv_coords: [number, number][];
  quality_metrics: QualityMetrics;
}
export interface QualityMetrics {
  method: string;
  middle_max_dev_mm: number;
  middle_mean_dev_mm: number;
  shape_fit_rms_mm: number;
  measured_count: number;
  expected_count: number;
  missing: string[];
  outliers: string[];
  estimated_rms_mm: number;
  estimated_p95_mm: number;
  warnings: string[];
}

export interface ReconstructionResult {
  run_id: number;
  surface: ReconstructedSurface;
  report_json_path: string;
}

export interface ReconstructionRun {
  id: number;
  screen_id: string;
  method: string;
  estimated_rms_mm: number;
  vertex_count: number;
  target: string | null;
  output_obj_path: string | null;
  created_at: string;
}

export interface MeasuredPoints {
  // 简化映射；实际字段以 lmt-core::measured_points::MeasuredPoints 为准
  points: Array<{
    name: string;
    position: [number, number, number];
    uncertainty: { isotropic: number } | { covariance: number[][] };
    source: "total_station" | { visual_ba: { camera_count: number } };
  }>;
  screen_id: string;
}

export type LmtError = { kind: string; message: string };

export const tauriApi = {
  listRecentProjects: () => invoke<RecentProject[]>("list_recent_projects"),
  addRecentProject: (absPath: string, displayName: string) =>
    invoke<RecentProject>("add_recent_project", { absPath, displayName }),
  removeRecentProject: (id: number) => invoke<void>("remove_recent_project", { id }),
  seedExampleProject: (targetDir: string, example: string) =>
    invoke<string>("seed_example_project", { targetDir, example }),
  loadProjectYaml: (absPath: string) => invoke<ProjectConfig>("load_project_yaml", { absPath }),
  saveProjectYaml: (absPath: string, config: ProjectConfig) =>
    invoke<void>("save_project_yaml", { absPath, config }),
  loadMeasurementsYaml: (path: string) =>
    invoke<MeasuredPoints>("load_measurements_yaml", { path }),
  reconstructSurface: (projectPath: string, screenId: string, measurementsPath: string) =>
    invoke<ReconstructionResult>("reconstruct_surface", {
      projectPath,
      screenId,
      measurementsPath,
    }),
  exportObj: (runId: number, target: string) =>
    invoke<string>("export_obj", { runId, target }),
  listRuns: (projectPath: string, screenId?: string) =>
    invoke<ReconstructionRun[]>("list_runs", { projectPath, screenId }),
  getRunReport: (runId: number) => invoke<unknown>("get_run_report", { runId }),
};
```

- [ ] **Step 3: 跑 vitest**

```bash
pnpm test src/services
```

预期：3 个 test PASS。

- [ ] **Step 4: 加 `vitest.config.ts`**（如不存在）

```bash
cp /Users/bip.lan/AIWorkspace/vp/ue-cache-manager/vitest.config.ts .
```

- [ ] **Step 5: 提交**

```bash
git add src/services/ vitest.config.ts
git commit -m "feat(frontend): typed tauri service wrapper + invoke arg shape tests"
```

---

## Phase 5 — Pinia stores

### Task 20: useUiStore + useProjectsStore

**Files:**
- Create: `src/stores/ui.ts`
- Create: `src/stores/projects.ts`
- Create: `src/stores/__tests__/projects.test.ts`

- [ ] **Step 1: 写 `src/stores/ui.ts`**

```ts
import { defineStore } from "pinia";
import { ref, watch } from "vue";

export const useUiStore = defineStore("ui", () => {
  const logOpen = ref(false);
  const theme = ref<"light" | "dark">((localStorage.getItem("lmt.theme") as any) ?? "dark");
  const lang = ref<"en" | "zh">((localStorage.getItem("lmt.lang") as any) ?? "en");
  const toasts = ref<Array<{ id: number; kind: "info" | "error" | "success"; msg: string }>>([]);
  let toastSeq = 0;

  watch(theme, (v) => {
    localStorage.setItem("lmt.theme", v);
    document.documentElement.classList.toggle("dark", v === "dark");
  }, { immediate: true });
  watch(lang, (v) => localStorage.setItem("lmt.lang", v));

  function toast(kind: "info" | "error" | "success", msg: string) {
    toasts.value.push({ id: ++toastSeq, kind, msg });
    setTimeout(() => {
      toasts.value = toasts.value.filter((t) => t.id !== toastSeq);
    }, 5000);
  }

  return { logOpen, theme, lang, toasts, toast };
});
```

- [ ] **Step 2: 写失败测试 `src/stores/__tests__/projects.test.ts`**

```ts
import { describe, it, expect, vi, beforeEach } from "vitest";
import { setActivePinia, createPinia } from "pinia";

vi.mock("@/services/tauri", () => ({
  tauriApi: {
    listRecentProjects: vi.fn(),
    addRecentProject: vi.fn(),
    removeRecentProject: vi.fn(),
    seedExampleProject: vi.fn(),
  },
}));

import { tauriApi } from "@/services/tauri";
import { useProjectsStore } from "../projects";

describe("useProjectsStore", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it("load fetches and stores recent", async () => {
    (tauriApi.listRecentProjects as any).mockResolvedValueOnce([
      { id: 1, abs_path: "/x", display_name: "X", last_opened_at: "2026" },
    ]);
    const s = useProjectsStore();
    await s.load();
    expect(s.recent).toHaveLength(1);
  });

  it("createFromExample seeds + adds + reloads", async () => {
    (tauriApi.seedExampleProject as any).mockResolvedValueOnce("/seeded/curved-flat");
    (tauriApi.addRecentProject as any).mockResolvedValueOnce({
      id: 7, abs_path: "/seeded/curved-flat", display_name: "Curved Flat", last_opened_at: "2026",
    });
    (tauriApi.listRecentProjects as any).mockResolvedValueOnce([
      { id: 7, abs_path: "/seeded/curved-flat", display_name: "Curved Flat", last_opened_at: "2026" },
    ]);
    const s = useProjectsStore();
    const created = await s.createFromExample("curved-flat", "/seeded");
    expect(created.id).toBe(7);
    expect(s.recent).toHaveLength(1);
  });
});
```

- [ ] **Step 3: 实现 `src/stores/projects.ts`**

```ts
import { defineStore } from "pinia";
import { ref } from "vue";
import { tauriApi, type RecentProject } from "@/services/tauri";

export const useProjectsStore = defineStore("projects", () => {
  const recent = ref<RecentProject[]>([]);
  const loading = ref(false);

  async function load() {
    loading.value = true;
    try {
      recent.value = await tauriApi.listRecentProjects();
    } finally {
      loading.value = false;
    }
  }

  async function createFromExample(example: string, targetDir: string) {
    const path = await tauriApi.seedExampleProject(targetDir, example);
    const displayName = `${example.replace(/-/g, " ")}`.replace(/\b\w/g, (c) => c.toUpperCase());
    const created = await tauriApi.addRecentProject(path, displayName);
    await load();
    return created;
  }

  async function openExisting(absPath: string, displayName: string) {
    const created = await tauriApi.addRecentProject(absPath, displayName);
    await load();
    return created;
  }

  async function remove(id: number) {
    await tauriApi.removeRecentProject(id);
    await load();
  }

  return { recent, loading, load, createFromExample, openExisting, remove };
});
```

- [ ] **Step 4: 跑测试**

```bash
pnpm test src/stores
```

- [ ] **Step 5: 提交**

```bash
git add src/stores/ui.ts src/stores/projects.ts src/stores/__tests__/projects.test.ts
git commit -m "feat(stores): ui + projects stores (theme/lang persist + recent CRUD)"
```

---

### Task 21: useCurrentProjectStore

**Files:**
- Create: `src/stores/currentProject.ts`
- Create: `src/stores/__tests__/currentProject.test.ts`

- [ ] **Step 1: 写测试**

```ts
import { describe, it, expect, vi, beforeEach } from "vitest";
import { setActivePinia, createPinia } from "pinia";

vi.mock("@/services/tauri", () => ({
  tauriApi: {
    listRecentProjects: vi.fn(),
    loadProjectYaml: vi.fn(),
    saveProjectYaml: vi.fn(),
  },
}));

import { tauriApi } from "@/services/tauri";
import { useCurrentProjectStore } from "../currentProject";

const sampleConfig = {
  project: { name: "X", unit: "mm" },
  screens: {
    MAIN: {
      cabinet_count: [8, 4] as [number, number],
      cabinet_size_mm: [500, 500] as [number, number],
      shape_prior: { type: "flat" } as const,
      shape_mode: "rectangle" as const,
      irregular_mask: [],
    },
  },
  coordinate_system: { origin_point: "MAIN_V001_R001", x_axis_point: "MAIN_V008_R001", xy_plane_point: "MAIN_V001_R004" },
  output: { target: "disguise", obj_filename: "{screen_id}.obj", weld_vertices_tolerance_mm: 1, triangulate: true },
};

describe("useCurrentProjectStore", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it("load by id resolves abs_path then loads yaml", async () => {
    (tauriApi.listRecentProjects as any).mockResolvedValueOnce([
      { id: 5, abs_path: "/p", display_name: "P", last_opened_at: "x" },
    ]);
    (tauriApi.loadProjectYaml as any).mockResolvedValueOnce(sampleConfig);
    const s = useCurrentProjectStore();
    await s.load(5);
    expect(s.absPath).toBe("/p");
    expect(s.config?.project.name).toBe("X");
    expect(s.dirty).toBe(false);
  });

  it("updateScreen sets dirty", async () => {
    (tauriApi.listRecentProjects as any).mockResolvedValueOnce([
      { id: 5, abs_path: "/p", display_name: "P", last_opened_at: "x" },
    ]);
    (tauriApi.loadProjectYaml as any).mockResolvedValueOnce(sampleConfig);
    const s = useCurrentProjectStore();
    await s.load(5);
    s.updateScreen("MAIN", { ...sampleConfig.screens.MAIN, cabinet_count: [10, 4] });
    expect(s.dirty).toBe(true);
  });

  it("save calls invoke + clears dirty", async () => {
    (tauriApi.listRecentProjects as any).mockResolvedValueOnce([
      { id: 5, abs_path: "/p", display_name: "P", last_opened_at: "x" },
    ]);
    (tauriApi.loadProjectYaml as any).mockResolvedValueOnce(sampleConfig);
    (tauriApi.saveProjectYaml as any).mockResolvedValueOnce(undefined);
    const s = useCurrentProjectStore();
    await s.load(5);
    s.updateScreen("MAIN", { ...sampleConfig.screens.MAIN, cabinet_count: [10, 4] });
    await s.save();
    expect(tauriApi.saveProjectYaml).toHaveBeenCalled();
    expect(s.dirty).toBe(false);
  });
});
```

- [ ] **Step 2: 实现 `src/stores/currentProject.ts`**

```ts
import { defineStore } from "pinia";
import { ref } from "vue";
import { tauriApi, type ProjectConfig, type ScreenConfig } from "@/services/tauri";

export const useCurrentProjectStore = defineStore("currentProject", () => {
  const id = ref<number | null>(null);
  const absPath = ref<string | null>(null);
  const config = ref<ProjectConfig | null>(null);
  const dirty = ref(false);
  const loading = ref(false);

  async function load(projectId: number) {
    loading.value = true;
    try {
      const recent = await tauriApi.listRecentProjects();
      const match = recent.find((p) => p.id === projectId);
      if (!match) throw new Error(`project ${projectId} not in recent`);
      id.value = projectId;
      absPath.value = match.abs_path;
      config.value = await tauriApi.loadProjectYaml(match.abs_path);
      dirty.value = false;
    } finally {
      loading.value = false;
    }
  }

  function updateScreen(screenId: string, screen: ScreenConfig) {
    if (!config.value) return;
    config.value = {
      ...config.value,
      screens: { ...config.value.screens, [screenId]: screen },
    };
    dirty.value = true;
  }

  function updateCoordinateSystem(cs: ProjectConfig["coordinate_system"]) {
    if (!config.value) return;
    config.value = { ...config.value, coordinate_system: cs };
    dirty.value = true;
  }

  function updateOutputTarget(target: string) {
    if (!config.value) return;
    config.value = { ...config.value, output: { ...config.value.output, target } };
    dirty.value = true;
  }

  async function save() {
    if (!absPath.value || !config.value) throw new Error("no project loaded");
    await tauriApi.saveProjectYaml(absPath.value, config.value);
    dirty.value = false;
  }

  return {
    id, absPath, config, dirty, loading,
    load, updateScreen, updateCoordinateSystem, updateOutputTarget, save,
  };
});
```

- [ ] **Step 3: 跑测试**

```bash
pnpm test src/stores
```

- [ ] **Step 4: 提交**

```bash
git add src/stores/currentProject.ts src/stores/__tests__/currentProject.test.ts
git commit -m "feat(stores): currentProject (load by id + dirty + save)"
```

---

### Task 22: useReconstructionStore

**Files:**
- Create: `src/stores/reconstruction.ts`
- Create: `src/stores/__tests__/reconstruction.test.ts`

- [ ] **Step 1: 写测试**

```ts
import { describe, it, expect, vi, beforeEach } from "vitest";
import { setActivePinia, createPinia } from "pinia";

vi.mock("@/services/tauri", () => ({
  tauriApi: {
    reconstructSurface: vi.fn(),
    exportObj: vi.fn(),
    listRuns: vi.fn(),
  },
}));

import { tauriApi } from "@/services/tauri";
import { useReconstructionStore } from "../reconstruction";

describe("useReconstructionStore", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it("setMeasurementsPath enables reconstruction", () => {
    const s = useReconstructionStore();
    expect(s.canReconstruct).toBe(false);
    s.setMeasurementsPath("measurements/m.yaml");
    expect(s.canReconstruct).toBe(true);
  });

  it("reconstruct stores surface + runId", async () => {
    (tauriApi.reconstructSurface as any).mockResolvedValueOnce({
      run_id: 42,
      surface: { vertices: [], uv_coords: [], topology: { cols: 1, rows: 1 }, screen_id: "MAIN", quality_metrics: {} as any },
      report_json_path: "reports/r.json",
    });
    const s = useReconstructionStore();
    s.setMeasurementsPath("m.yaml");
    await s.reconstruct("/p", "MAIN");
    expect(s.currentRunId).toBe(42);
    expect(s.currentSurface).toBeTruthy();
  });
});
```

- [ ] **Step 2: 实现 `src/stores/reconstruction.ts`**

```ts
import { defineStore } from "pinia";
import { computed, ref } from "vue";
import {
  tauriApi,
  type ReconstructedSurface,
  type ReconstructionRun,
} from "@/services/tauri";

export const useReconstructionStore = defineStore("reconstruction", () => {
  const measurementsPath = ref<string | null>(null);
  const currentSurface = ref<ReconstructedSurface | null>(null);
  const currentRunId = ref<number | null>(null);
  const status = ref<"idle" | "running" | "done" | "error">("idle");
  const recentRuns = ref<ReconstructionRun[]>([]);

  const canReconstruct = computed(() => measurementsPath.value !== null);

  function setMeasurementsPath(path: string) {
    measurementsPath.value = path;
  }

  async function reconstruct(projectPath: string, screenId: string) {
    if (!measurementsPath.value) throw new Error("no measurements loaded");
    status.value = "running";
    try {
      const r = await tauriApi.reconstructSurface(projectPath, screenId, measurementsPath.value);
      currentRunId.value = r.run_id;
      currentSurface.value = r.surface;
      status.value = "done";
      return r;
    } catch (e) {
      status.value = "error";
      throw e;
    }
  }

  async function exportObj(target: string) {
    if (!currentRunId.value) throw new Error("no run");
    return await tauriApi.exportObj(currentRunId.value, target);
  }

  async function loadRuns(projectPath: string, screenId?: string) {
    recentRuns.value = await tauriApi.listRuns(projectPath, screenId);
  }

  return {
    measurementsPath, currentSurface, currentRunId, status, recentRuns,
    canReconstruct,
    setMeasurementsPath, reconstruct, exportObj, loadRuns,
  };
});
```

- [ ] **Step 3: 跑测试**

```bash
pnpm test src/stores
```

- [ ] **Step 4: 提交**

```bash
git add src/stores/reconstruction.ts src/stores/__tests__/reconstruction.test.ts
git commit -m "feat(stores): reconstruction store (measurements path gate + run/export)"
```

---

### Task 23: useEditorStore (undo/redo snapshot)

**Files:**
- Create: `src/stores/editor.ts`
- Create: `src/stores/__tests__/editor.test.ts`

- [ ] **Step 1: 写测试**

```ts
import { describe, it, expect, beforeEach } from "vitest";
import { setActivePinia, createPinia } from "pinia";
import { useEditorStore } from "../editor";

describe("useEditorStore", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("toggleCell pushes snapshot, undo restores", () => {
    const s = useEditorStore();
    s.initFromScreen({
      cabinet_count: [4, 2],
      cabinet_size_mm: [500, 500],
      shape_prior: { type: "flat" },
      shape_mode: "rectangle",
      irregular_mask: [],
    });
    expect(s.isAbsent(0, 0)).toBe(false);
    s.toggleCell(0, 0);
    expect(s.isAbsent(0, 0)).toBe(true);
    s.undo();
    expect(s.isAbsent(0, 0)).toBe(false);
    s.redo();
    expect(s.isAbsent(0, 0)).toBe(true);
  });

  it("setRef stores per role + undoable", () => {
    const s = useEditorStore();
    s.initFromScreen({
      cabinet_count: [4, 2],
      cabinet_size_mm: [500, 500],
      shape_prior: { type: "flat" },
      shape_mode: "rectangle",
      irregular_mask: [],
    });
    s.setMode("refs");
    s.setRef("origin", "MAIN_V001_R001");
    s.setRef("x_axis", "MAIN_V004_R001");
    expect(s.refs.origin).toBe("MAIN_V001_R001");
    expect(s.refs.x_axis).toBe("MAIN_V004_R001");
    s.undo();
    expect(s.refs.x_axis).toBeNull();
  });

  it("undoStack truncates at 50", () => {
    const s = useEditorStore();
    s.initFromScreen({
      cabinet_count: [4, 2],
      cabinet_size_mm: [500, 500],
      shape_prior: { type: "flat" },
      shape_mode: "rectangle",
      irregular_mask: [],
    });
    for (let i = 0; i < 60; i++) s.toggleCell(i % 4, 0);
    expect(s.undoDepth).toBeLessThanOrEqual(50);
  });
});
```

- [ ] **Step 2: 实现 `src/stores/editor.ts`**

```ts
import { defineStore } from "pinia";
import { computed, ref } from "vue";
import type { ScreenConfig } from "@/services/tauri";

export type EditorMode = "mask" | "refs" | "baseline";
export type RefRole = "origin" | "x_axis" | "xy_plane";

interface Snapshot {
  mask: Set<string>; // "col,row"
  refs: { origin: string | null; x_axis: string | null; xy_plane: string | null };
  baselineRow: number | null;
}

const MAX_UNDO = 50;

function deepCopy(s: Snapshot): Snapshot {
  return {
    mask: new Set(s.mask),
    refs: { ...s.refs },
    baselineRow: s.baselineRow,
  };
}

export const useEditorStore = defineStore("editor", () => {
  const cols = ref(0);
  const rows = ref(0);
  const mode = ref<EditorMode>("mask");
  const currentRefRole = ref<RefRole>("origin");
  const mask = ref(new Set<string>());
  const refs = ref<Snapshot["refs"]>({ origin: null, x_axis: null, xy_plane: null });
  const baselineRow = ref<number | null>(null);
  const undoStack = ref<Snapshot[]>([]);
  const redoStack = ref<Snapshot[]>([]);

  function snapshot(): Snapshot {
    return {
      mask: new Set(mask.value),
      refs: { ...refs.value },
      baselineRow: baselineRow.value,
    };
  }

  function applySnapshot(s: Snapshot) {
    mask.value = new Set(s.mask);
    refs.value = { ...s.refs };
    baselineRow.value = s.baselineRow;
  }

  function pushUndo() {
    undoStack.value.push(snapshot());
    if (undoStack.value.length > MAX_UNDO) undoStack.value.shift();
    redoStack.value = [];
  }

  function initFromScreen(screen: ScreenConfig) {
    cols.value = screen.cabinet_count[0];
    rows.value = screen.cabinet_count[1];
    mask.value = new Set(screen.irregular_mask.map(([c, r]) => `${c},${r}`));
    refs.value = { origin: null, x_axis: null, xy_plane: null };
    baselineRow.value = null;
    undoStack.value = [];
    redoStack.value = [];
  }

  function setMode(m: EditorMode) {
    mode.value = m;
  }
  function setCurrentRefRole(r: RefRole) {
    currentRefRole.value = r;
  }

  function isAbsent(col: number, row: number): boolean {
    return mask.value.has(`${col},${row}`);
  }

  function toggleCell(col: number, row: number) {
    pushUndo();
    const k = `${col},${row}`;
    if (mask.value.has(k)) mask.value.delete(k);
    else mask.value.add(k);
    mask.value = new Set(mask.value); // trigger reactivity
  }

  function setRef(role: RefRole, name: string) {
    pushUndo();
    refs.value = { ...refs.value, [role]: name };
  }

  function setBaseline(row: number) {
    pushUndo();
    baselineRow.value = row;
  }

  function undo() {
    if (undoStack.value.length === 0) return;
    redoStack.value.push(snapshot());
    const s = undoStack.value.pop()!;
    applySnapshot(s);
  }
  function redo() {
    if (redoStack.value.length === 0) return;
    undoStack.value.push(snapshot());
    const s = redoStack.value.pop()!;
    applySnapshot(s);
  }

  function clearStacks() {
    undoStack.value = [];
    redoStack.value = [];
  }

  const undoDepth = computed(() => undoStack.value.length);
  const redoDepth = computed(() => redoStack.value.length);

  /** Commit current editor state back to a ScreenConfig shape (mask only;
   *  refs + baseline are owned by the wider project config). */
  function commitMaskToScreen(screen: ScreenConfig): ScreenConfig {
    return {
      ...screen,
      shape_mode: mask.value.size > 0 ? "irregular" : screen.shape_mode,
      irregular_mask: Array.from(mask.value).map((k) => {
        const [c, r] = k.split(",").map(Number);
        return [c, r] as [number, number];
      }),
    };
  }

  return {
    cols, rows, mode, currentRefRole, mask, refs, baselineRow,
    undoDepth, redoDepth,
    initFromScreen, setMode, setCurrentRefRole,
    isAbsent, toggleCell, setRef, setBaseline,
    undo, redo, clearStacks,
    commitMaskToScreen,
  };
});
```

- [ ] **Step 3: 跑测试**

```bash
pnpm test src/stores
```

- [ ] **Step 4: 提交**

```bash
git add src/stores/editor.ts src/stores/__tests__/editor.test.ts
git commit -m "feat(stores): editor (mode/mask/refs/baseline + 50-step snapshot undo)"
```

---

## Phase 6 — Views

### Task 24: Home.vue（recent + 创建按钮）

**Files:**
- Modify: `src/views/Home.vue`
- Create: `src/locales/en.json` + `zh.json` (追加 home key)

- [ ] **Step 1: 在 locales 追加**

en：
```json
"home": {
  "recent": "Recent Projects",
  "empty": "No recent projects. Click below to seed one from an example.",
  "create_curved_flat": "Create Curved-Flat (8x4)",
  "create_curved_arc": "Create Curved-Arc (16x6)",
  "open_existing": "Open Existing Folder…",
  "remove": "Remove from list"
}
```

zh：
```json
"home": {
  "recent": "最近项目",
  "empty": "无最近项目。点下方按钮从示例创建一个。",
  "create_curved_flat": "创建 Curved-Flat (8x4)",
  "create_curved_arc": "创建 Curved-Arc (16x6)",
  "open_existing": "打开已有文件夹…",
  "remove": "移出列表"
}
```

- [ ] **Step 2: 实现 `src/views/Home.vue`**

```vue
<script setup lang="ts">
import { onMounted } from "vue";
import { useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import { useProjectsStore } from "@/stores/projects";
import { useUiStore } from "@/stores/ui";
import { open } from "@tauri-apps/plugin-dialog";

const { t } = useI18n();
const router = useRouter();
const projects = useProjectsStore();
const ui = useUiStore();

onMounted(() => projects.load());

async function createExample(name: string) {
  try {
    const target = await open({ directory: true, title: "Choose where to seed example" });
    if (!target) return;
    const created = await projects.createFromExample(name, target as string);
    router.push(`/projects/${created.id}/design`);
  } catch (e) {
    ui.toast("error", `${e}`);
  }
}

async function openExisting() {
  try {
    const folder = await open({ directory: true, title: "Open project folder" });
    if (!folder) return;
    const name = String(folder).split(/[/\\]/).pop() ?? "Project";
    const created = await projects.openExisting(folder as string, name);
    router.push(`/projects/${created.id}/design`);
  } catch (e) {
    ui.toast("error", `${e}`);
  }
}
</script>

<template>
  <div class="p-8">
    <h1 class="text-2xl font-bold">{{ t("home.recent") }}</h1>
    <div v-if="projects.recent.length === 0" class="mt-6 text-muted-foreground">
      {{ t("home.empty") }}
    </div>
    <ul v-else class="mt-4 divide-y">
      <li v-for="p in projects.recent" :key="p.id" class="flex items-center gap-4 py-2">
        <RouterLink :to="`/projects/${p.id}/design`" class="flex-1 hover:underline">
          <div class="font-medium">{{ p.display_name }}</div>
          <div class="text-xs text-muted-foreground">{{ p.abs_path }}</div>
        </RouterLink>
        <button class="text-xs text-destructive" @click="projects.remove(p.id)">
          {{ t("home.remove") }}
        </button>
      </li>
    </ul>

    <div class="mt-8 flex flex-wrap gap-3">
      <button class="rounded bg-primary px-4 py-2 text-primary-foreground" @click="createExample('curved-flat')">
        {{ t("home.create_curved_flat") }}
      </button>
      <button class="rounded bg-primary px-4 py-2 text-primary-foreground" @click="createExample('curved-arc')">
        {{ t("home.create_curved_arc") }}
      </button>
      <button class="rounded border px-4 py-2" @click="openExisting">
        {{ t("home.open_existing") }}
      </button>
    </div>
  </div>
</template>
```

- [ ] **Step 3: capabilities 加 dialog 权限**

`src-tauri/capabilities/default.json` 加 `"dialog:allow-open"`：

```json
{
  "permissions": ["core:default", "core:event:default", "core:path:default", "dialog:allow-open"]
}
```

并在 `src-tauri/Cargo.toml` 加 `tauri-plugin-dialog = "2"`，在 `lib.rs` builder 加 `.plugin(tauri_plugin_dialog::init())`。在 `package.json` deps 加 `"@tauri-apps/plugin-dialog": "^2.0.0"`。

```bash
pnpm add @tauri-apps/plugin-dialog
```

- [ ] **Step 4: 提交**

```bash
git add src/views/Home.vue src/locales/ src-tauri/capabilities/default.json src-tauri/Cargo.toml src-tauri/src/lib.rs package.json pnpm-lock.yaml
git commit -m "feat(home): recent list + seed example flow + open existing folder"
```

---

### Task 25: CabinetGrid.vue (vue-konva 主组件)

**Files:**
- Create: `src/components/design/CabinetGrid.vue`
- Create: `src/components/design/CabinetGridLegend.vue`

- [ ] **Step 1: 实现 `CabinetGrid.vue`**

```vue
<script setup lang="ts">
import { computed } from "vue";
import { useEditorStore } from "@/stores/editor";

const props = defineProps<{
  cellPx?: number; // 视觉单位边长
}>();

const editor = useEditorStore();
const cell = computed(() => props.cellPx ?? Math.max(8, Math.min(24, Math.floor(960 / Math.max(1, editor.cols)))));

const stageWidth = computed(() => editor.cols * cell.value + 80);
const stageHeight = computed(() => editor.rows * cell.value + 80);

interface CellModel {
  col: number;
  row: number;
  x: number;
  y: number;
  absent: boolean;
  refRole: "origin" | "x_axis" | "xy_plane" | null;
  belowBaseline: boolean;
  name: string;
}

const cells = computed<CellModel[]>(() => {
  const out: CellModel[] = [];
  for (let r = 1; r <= editor.rows; r++) {
    for (let c = 1; c <= editor.cols; c++) {
      const name = `MAIN_V${String(c).padStart(3, "0")}_R${String(r).padStart(3, "0")}`;
      let role: CellModel["refRole"] = null;
      if (editor.refs.origin === name) role = "origin";
      else if (editor.refs.x_axis === name) role = "x_axis";
      else if (editor.refs.xy_plane === name) role = "xy_plane";
      out.push({
        col: c,
        row: r,
        // Konva y 朝下；R001 显示在底部，所以 y = (rows - r) * cell
        x: 40 + (c - 1) * cell.value,
        y: 40 + (editor.rows - r) * cell.value,
        absent: editor.isAbsent(c, r),
        refRole: role,
        belowBaseline: editor.baselineRow !== null && r < editor.baselineRow,
        name,
      });
    }
  }
  return out;
});

function fillFor(c: CellModel): string {
  if (c.absent) return "#3f3f46";
  if (c.belowBaseline) return "#1e293b";
  return "#0ea5e9";
}
function strokeFor(c: CellModel): string {
  if (c.refRole === "origin") return "#ef4444";
  if (c.refRole === "x_axis") return "#22c55e";
  if (c.refRole === "xy_plane") return "#3b82f6";
  return "#1e293b";
}
function strokeWidthFor(c: CellModel): number {
  return c.refRole !== null ? 3 : 1;
}

function onCellClick(c: CellModel) {
  if (editor.mode === "mask") {
    editor.toggleCell(c.col, c.row);
  } else if (editor.mode === "refs") {
    editor.setRef(editor.currentRefRole, c.name);
  } else if (editor.mode === "baseline") {
    editor.setBaseline(c.row);
  }
}
</script>

<template>
  <v-stage :config="{ width: stageWidth, height: stageHeight }">
    <v-layer>
      <v-rect
        v-for="c in cells"
        :key="c.name"
        :config="{
          x: c.x,
          y: c.y,
          width: cell - 1,
          height: cell - 1,
          fill: fillFor(c),
          stroke: strokeFor(c),
          strokeWidth: strokeWidthFor(c),
        }"
        @click="onCellClick(c)"
        @tap="onCellClick(c)"
      />
      <v-line
        v-if="editor.baselineRow !== null"
        :config="{
          points: [40, 40 + (editor.rows - editor.baselineRow + 1) * cell, 40 + editor.cols * cell, 40 + (editor.rows - editor.baselineRow + 1) * cell],
          stroke: '#fbbf24',
          strokeWidth: 2,
          dash: [6, 6],
        }"
      />
    </v-layer>
  </v-stage>
</template>
```

- [ ] **Step 2: 写 `CabinetGridLegend.vue`**

```vue
<script setup lang="ts">
import { useI18n } from "vue-i18n";
const { t } = useI18n();
</script>

<template>
  <div class="flex flex-wrap gap-4 text-xs">
    <span class="flex items-center gap-1"><span class="inline-block h-3 w-3 bg-sky-500"></span> {{ t("design.legend.present") }}</span>
    <span class="flex items-center gap-1"><span class="inline-block h-3 w-3 bg-zinc-700"></span> {{ t("design.legend.absent") }}</span>
    <span class="flex items-center gap-1"><span class="inline-block h-3 w-3 border-2 border-red-500"></span> {{ t("design.legend.origin") }}</span>
    <span class="flex items-center gap-1"><span class="inline-block h-3 w-3 border-2 border-green-500"></span> {{ t("design.legend.x_axis") }}</span>
    <span class="flex items-center gap-1"><span class="inline-block h-3 w-3 border-2 border-blue-500"></span> {{ t("design.legend.xy_plane") }}</span>
  </div>
</template>
```

加 locale 对应 key `design.legend.{present,absent,origin,x_axis,xy_plane}`。

- [ ] **Step 3: 在 `main.ts` 注册 vue-konva**

```ts
import VueKonva from "vue-konva";
app.use(VueKonva);
```

- [ ] **Step 4: 提交**

```bash
git add src/components/design/ src/main.ts src/locales/
git commit -m "feat(design): CabinetGrid (vue-konva) + Legend"
```

---

### Task 26: Design.vue + DesignToolbar + ScreenPicker + 快捷键

**Files:**
- Modify: `src/views/Design.vue`
- Create: `src/components/design/DesignToolbar.vue`
- Create: `src/components/design/ScreenPicker.vue`

- [ ] **Step 1: 实现 `ScreenPicker.vue`**

```vue
<script setup lang="ts">
import { computed } from "vue";
import { useCurrentProjectStore } from "@/stores/currentProject";
const props = defineProps<{ modelValue: string }>();
const emit = defineEmits<{ "update:modelValue": [v: string] }>();
const proj = useCurrentProjectStore();
const screens = computed(() => Object.keys(proj.config?.screens ?? {}));
</script>

<template>
  <select :value="modelValue" class="rounded border bg-background px-2 py-1" @change="emit('update:modelValue', ($event.target as HTMLSelectElement).value)">
    <option v-for="s in screens" :key="s" :value="s">{{ s }}</option>
  </select>
</template>
```

- [ ] **Step 2: 实现 `DesignToolbar.vue`**

```vue
<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { useEditorStore } from "@/stores/editor";
import { useCurrentProjectStore } from "@/stores/currentProject";
import { useUiStore } from "@/stores/ui";

const { t } = useI18n();
const editor = useEditorStore();
const proj = useCurrentProjectStore();
const ui = useUiStore();

async function save() {
  if (!proj.config) return;
  const screenId = Object.keys(proj.config.screens)[0];
  const next = editor.commitMaskToScreen(proj.config.screens[screenId]);
  proj.updateScreen(screenId, next);
  proj.updateCoordinateSystem({
    origin_point: editor.refs.origin ?? proj.config.coordinate_system.origin_point,
    x_axis_point: editor.refs.x_axis ?? proj.config.coordinate_system.x_axis_point,
    xy_plane_point: editor.refs.xy_plane ?? proj.config.coordinate_system.xy_plane_point,
  });
  await proj.save();
  editor.clearStacks();
  ui.toast("success", "Saved");
}
</script>

<template>
  <div class="flex items-center gap-2 border-b bg-card p-2">
    <button :class="['rounded px-3 py-1 text-sm', editor.mode === 'mask' && 'bg-accent']" @click="editor.setMode('mask')">{{ t("design.toolbar.mask") }} <kbd>M</kbd></button>
    <button :class="['rounded px-3 py-1 text-sm', editor.mode === 'refs' && 'bg-accent']" @click="editor.setMode('refs')">{{ t("design.toolbar.refs") }} <kbd>R</kbd></button>
    <button :class="['rounded px-3 py-1 text-sm', editor.mode === 'baseline' && 'bg-accent']" @click="editor.setMode('baseline')">{{ t("design.toolbar.baseline") }} <kbd>B</kbd></button>

    <div v-if="editor.mode === 'refs'" class="ml-4 flex gap-1">
      <button :class="['rounded border px-2 py-0.5 text-xs', editor.currentRefRole === 'origin' && 'bg-red-500 text-white']" @click="editor.setCurrentRefRole('origin')">Origin <kbd>1</kbd></button>
      <button :class="['rounded border px-2 py-0.5 text-xs', editor.currentRefRole === 'x_axis' && 'bg-green-500 text-white']" @click="editor.setCurrentRefRole('x_axis')">X-axis <kbd>2</kbd></button>
      <button :class="['rounded border px-2 py-0.5 text-xs', editor.currentRefRole === 'xy_plane' && 'bg-blue-500 text-white']" @click="editor.setCurrentRefRole('xy_plane')">XY-plane <kbd>3</kbd></button>
    </div>

    <div class="ml-auto flex gap-2">
      <button :disabled="editor.undoDepth === 0" class="rounded border px-2 py-1 text-xs disabled:opacity-50" @click="editor.undo()">Undo ({{ editor.undoDepth }})</button>
      <button :disabled="editor.redoDepth === 0" class="rounded border px-2 py-1 text-xs disabled:opacity-50" @click="editor.redo()">Redo ({{ editor.redoDepth }})</button>
      <button class="rounded bg-primary px-3 py-1 text-sm text-primary-foreground" :disabled="!proj.dirty" @click="save">Save</button>
    </div>
  </div>
</template>
```

- [ ] **Step 3: 实现 `Design.vue`**

```vue
<script setup lang="ts">
import { computed, onMounted, onBeforeUnmount, ref, watch } from "vue";
import { useRoute } from "vue-router";
import { useCurrentProjectStore } from "@/stores/currentProject";
import { useEditorStore } from "@/stores/editor";
import { useUiStore } from "@/stores/ui";
import CabinetGrid from "@/components/design/CabinetGrid.vue";
import CabinetGridLegend from "@/components/design/CabinetGridLegend.vue";
import DesignToolbar from "@/components/design/DesignToolbar.vue";
import ScreenPicker from "@/components/design/ScreenPicker.vue";

const route = useRoute();
const proj = useCurrentProjectStore();
const editor = useEditorStore();
const ui = useUiStore();
const id = computed(() => Number(route.params.id));
const currentScreenId = ref<string>("MAIN");

async function load() {
  await proj.load(id.value);
  if (proj.config) {
    const ids = Object.keys(proj.config.screens);
    currentScreenId.value = ids[0] ?? "MAIN";
    editor.initFromScreen(proj.config.screens[currentScreenId.value]);
  }
}

watch(currentScreenId, (next) => {
  if (proj.config?.screens[next]) editor.initFromScreen(proj.config.screens[next]);
});

function onKey(e: KeyboardEvent) {
  if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
  if (e.metaKey || e.ctrlKey) {
    if (e.key.toLowerCase() === "z" && !e.shiftKey) {
      e.preventDefault();
      editor.undo();
    } else if ((e.key.toLowerCase() === "z" && e.shiftKey) || e.key.toLowerCase() === "y") {
      e.preventDefault();
      editor.redo();
    }
    return;
  }
  if (e.key === "m" || e.key === "M") editor.setMode("mask");
  else if (e.key === "r" || e.key === "R") editor.setMode("refs");
  else if (e.key === "b" || e.key === "B") editor.setMode("baseline");
  else if (editor.mode === "refs") {
    if (e.key === "1") editor.setCurrentRefRole("origin");
    else if (e.key === "2") editor.setCurrentRefRole("x_axis");
    else if (e.key === "3") editor.setCurrentRefRole("xy_plane");
  }
}

onMounted(() => {
  load().catch((e) => ui.toast("error", `${e}`));
  window.addEventListener("keydown", onKey);
});
onBeforeUnmount(() => window.removeEventListener("keydown", onKey));
</script>

<template>
  <div class="flex h-full flex-col">
    <DesignToolbar />
    <div class="flex items-center gap-3 border-b bg-card px-4 py-2 text-sm">
      <span>Screen:</span>
      <ScreenPicker v-model="currentScreenId" />
      <CabinetGridLegend class="ml-auto" />
    </div>
    <div class="min-h-0 flex-1 overflow-auto p-4">
      <CabinetGrid />
    </div>
  </div>
</template>
```

加 locale `design.toolbar.{mask,refs,baseline}`、`design.legend.*`。

- [ ] **Step 4: 提交**

```bash
git add src/views/Design.vue src/components/design/ src/locales/
git commit -m "feat(design): page + toolbar + screen picker + keyboard shortcuts"
```

---

### Task 27: MeshPreview.vue (Three.js)

**Files:**
- Create: `src/components/preview/MeshPreview.vue`

- [ ] **Step 1: 实现 `MeshPreview.vue`**

```vue
<script setup lang="ts">
import { onMounted, onBeforeUnmount, ref, watch } from "vue";
import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import type { ReconstructedSurface } from "@/services/tauri";

const props = defineProps<{ surface: ReconstructedSurface | null }>();
const canvasRef = ref<HTMLCanvasElement | null>(null);

let scene: THREE.Scene | null = null;
let camera: THREE.PerspectiveCamera | null = null;
let renderer: THREE.WebGLRenderer | null = null;
let controls: OrbitControls | null = null;
let mesh: THREE.Mesh | null = null;
let raf = 0;

function buildGeometry(surface: ReconstructedSurface): THREE.BufferGeometry {
  const g = new THREE.BufferGeometry();
  const positions = new Float32Array(surface.vertices.flatMap((v) => [v[0], v[1], v[2]]));
  const uvs = new Float32Array(surface.uv_coords.flatMap((uv) => [uv[0], uv[1]]));
  g.setAttribute("position", new THREE.BufferAttribute(positions, 3));
  g.setAttribute("uv", new THREE.BufferAttribute(uvs, 2));
  // Build triangle index from grid topology (cols+1, rows+1)
  const cols = surface.topology.cols;
  const rows = surface.topology.rows;
  const idx: number[] = [];
  const idxAt = (c: number, r: number) => r * (cols + 1) + c;
  for (let r = 0; r < rows; r++) {
    for (let c = 0; c < cols; c++) {
      const a = idxAt(c, r);
      const b = idxAt(c + 1, r);
      const cc = idxAt(c + 1, r + 1);
      const d = idxAt(c, r + 1);
      idx.push(a, b, cc, a, cc, d);
    }
  }
  g.setIndex(idx);
  g.computeVertexNormals();
  return g;
}

function ensureScene() {
  if (!canvasRef.value) return;
  scene = new THREE.Scene();
  scene.background = new THREE.Color(0x111827);
  camera = new THREE.PerspectiveCamera(60, 1, 0.01, 1000);
  camera.position.set(8, 6, 8);
  renderer = new THREE.WebGLRenderer({ canvas: canvasRef.value, antialias: true });
  renderer.setPixelRatio(window.devicePixelRatio);
  controls = new OrbitControls(camera, canvasRef.value);
  controls.enableDamping = true;

  const grid = new THREE.GridHelper(10, 10, 0x444444, 0x222222);
  scene.add(grid);
  const dir = new THREE.DirectionalLight(0xffffff, 0.9);
  dir.position.set(5, 10, 5);
  scene.add(dir);
  scene.add(new THREE.AmbientLight(0xffffff, 0.4));

  const resize = () => {
    if (!canvasRef.value || !renderer || !camera) return;
    const w = canvasRef.value.clientWidth;
    const h = canvasRef.value.clientHeight;
    renderer.setSize(w, h, false);
    camera.aspect = w / h;
    camera.updateProjectionMatrix();
  };
  resize();
  window.addEventListener("resize", resize);

  const tick = () => {
    raf = requestAnimationFrame(tick);
    controls?.update();
    if (renderer && scene && camera) renderer.render(scene, camera);
  };
  tick();
}

function setMeshFromSurface(surface: ReconstructedSurface) {
  if (!scene) return;
  if (mesh) {
    scene.remove(mesh);
    mesh.geometry.dispose();
    (mesh.material as THREE.Material).dispose();
    mesh = null;
  }
  const g = buildGeometry(surface);
  const m = new THREE.MeshStandardMaterial({ color: 0x0ea5e9, side: THREE.DoubleSide, wireframe: false });
  mesh = new THREE.Mesh(g, m);
  scene.add(mesh);
  // 让 camera 看 mesh 中心
  const box = new THREE.Box3().setFromObject(mesh);
  const center = new THREE.Vector3();
  box.getCenter(center);
  controls?.target.copy(center);
}

onMounted(() => {
  ensureScene();
  if (props.surface) setMeshFromSurface(props.surface);
});

watch(
  () => props.surface,
  (v) => {
    if (v) setMeshFromSurface(v);
  },
);

onBeforeUnmount(() => {
  cancelAnimationFrame(raf);
  if (mesh) {
    mesh.geometry.dispose();
    (mesh.material as THREE.Material).dispose();
  }
  controls?.dispose();
  renderer?.dispose();
  scene = null;
  camera = null;
  renderer = null;
  controls = null;
  mesh = null;
});
</script>

<template>
  <canvas ref="canvasRef" class="h-full w-full" />
</template>
```

- [ ] **Step 2: 提交**

```bash
git add src/components/preview/MeshPreview.vue
git commit -m "feat(preview): MeshPreview Three.js component (BufferGeometry + OrbitControls + dispose)"
```

---

### Task 28: Preview.vue + PreviewToolbar.vue

**Files:**
- Modify: `src/views/Preview.vue`
- Create: `src/components/preview/PreviewToolbar.vue`

- [ ] **Step 1: 实现 `PreviewToolbar.vue`**

```vue
<script setup lang="ts">
import { useReconstructionStore } from "@/stores/reconstruction";
import { useCurrentProjectStore } from "@/stores/currentProject";
import { useUiStore } from "@/stores/ui";
import { useRoute } from "vue-router";

const recon = useReconstructionStore();
const proj = useCurrentProjectStore();
const ui = useUiStore();
const route = useRoute();

async function reconstructNow() {
  if (!proj.absPath) return;
  if (!recon.canReconstruct) {
    ui.toast("error", "Load measurements first (Import view)");
    return;
  }
  try {
    await recon.reconstruct(proj.absPath, "MAIN");
    ui.toast("success", "Reconstruction done");
  } catch (e) {
    ui.toast("error", `${e}`);
  }
}

async function exportNow(target: string) {
  try {
    const path = await recon.exportObj(target);
    ui.toast("success", `Wrote ${path}`);
  } catch (e) {
    ui.toast("error", `${e}`);
  }
}
</script>

<template>
  <div class="flex items-center gap-2 border-b bg-card p-2">
    <button :disabled="!recon.canReconstruct || recon.status === 'running'" class="rounded bg-primary px-3 py-1 text-sm text-primary-foreground disabled:opacity-50" @click="reconstructNow">
      {{ recon.status === "running" ? "Running…" : "Reconstruct" }}
    </button>
    <span class="ml-2 text-xs text-muted-foreground">Status: {{ recon.status }}</span>
    <div class="ml-auto flex gap-2">
      <button :disabled="!recon.currentRunId" class="rounded border px-3 py-1 text-sm disabled:opacity-50" @click="exportNow('disguise')">Export Disguise</button>
      <button :disabled="!recon.currentRunId" class="rounded border px-3 py-1 text-sm disabled:opacity-50" @click="exportNow('unreal')">Export Unreal</button>
      <button :disabled="!recon.currentRunId" class="rounded border px-3 py-1 text-sm disabled:opacity-50" @click="exportNow('neutral')">Export Neutral</button>
    </div>
  </div>
</template>
```

- [ ] **Step 2: 实现 `Preview.vue`**

```vue
<script setup lang="ts">
import { computed, onMounted } from "vue";
import { useRoute } from "vue-router";
import { useCurrentProjectStore } from "@/stores/currentProject";
import { useReconstructionStore } from "@/stores/reconstruction";
import PreviewToolbar from "@/components/preview/PreviewToolbar.vue";
import MeshPreview from "@/components/preview/MeshPreview.vue";

const route = useRoute();
const proj = useCurrentProjectStore();
const recon = useReconstructionStore();
const id = computed(() => Number(route.params.id));

onMounted(async () => {
  if (proj.id !== id.value) await proj.load(id.value);
});
</script>

<template>
  <div class="flex h-full flex-col">
    <PreviewToolbar />
    <div class="min-h-0 flex-1">
      <MeshPreview :surface="recon.currentSurface" />
    </div>
  </div>
</template>
```

- [ ] **Step 3: 提交**

```bash
git add src/views/Preview.vue src/components/preview/
git commit -m "feat(preview): toolbar (reconstruct + 3 export targets) + MeshPreview wiring"
```

---

### Task 29: Import.vue（M0.2 partial stub — load measured.yaml）

**Files:**
- Modify: `src/views/Import.vue`

- [ ] **Step 1: 实现**

```vue
<script setup lang="ts">
import { computed, onMounted } from "vue";
import { useRoute } from "vue-router";
import { useCurrentProjectStore } from "@/stores/currentProject";
import { useReconstructionStore } from "@/stores/reconstruction";
import { useUiStore } from "@/stores/ui";
import { tauriApi } from "@/services/tauri";
import { open } from "@tauri-apps/plugin-dialog";

const route = useRoute();
const proj = useCurrentProjectStore();
const recon = useReconstructionStore();
const ui = useUiStore();
const id = computed(() => Number(route.params.id));

onMounted(async () => {
  if (proj.id !== id.value) await proj.load(id.value);
});

async function loadMeasured() {
  if (!proj.absPath) return;
  try {
    const file = await open({
      title: "Select measured.yaml",
      filters: [{ name: "YAML", extensions: ["yaml", "yml"] }],
      defaultPath: `${proj.absPath}/measurements`,
    });
    if (!file) return;
    const mp = await tauriApi.loadMeasurementsYaml(String(file));
    const rel = String(file).startsWith(proj.absPath)
      ? String(file).slice(proj.absPath.length).replace(/^[\\/]+/, "")
      : String(file);
    recon.setMeasurementsPath(rel);
    ui.toast("success", `Loaded ${mp.points.length} measurements`);
  } catch (e) {
    ui.toast("error", `${e}`);
  }
}
</script>

<template>
  <div class="p-8">
    <h1 class="text-2xl font-bold">Import (M0.2 demo)</h1>
    <p class="mt-2 text-sm text-muted-foreground">
      M1 will add CSV import (total station). M2 will add image import (visual BA). For now, load a hand-written measured.yaml from your project.
    </p>
    <div class="mt-6 flex flex-col gap-2">
      <button class="w-fit rounded bg-primary px-4 py-2 text-primary-foreground" @click="loadMeasured">Load measured.yaml</button>
      <p class="text-xs text-muted-foreground">Current: {{ recon.measurementsPath ?? "(none)" }}</p>
    </div>
  </div>
</template>
```

- [ ] **Step 2: 提交**

```bash
git add src/views/Import.vue
git commit -m "feat(import): M0.2 partial stub — load measured.yaml from project folder"
```

---

### Task 30: Runs.vue + Export.vue + M1/M2 stub views

**Files:**
- Modify: `src/views/Runs.vue`
- Modify: `src/views/Export.vue`
- Modify: `src/views/Instruct.vue` / `Charuco.vue` / `Photoplan.vue`

- [ ] **Step 1: `Runs.vue`**

```vue
<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useRoute } from "vue-router";
import { useCurrentProjectStore } from "@/stores/currentProject";
import { useReconstructionStore } from "@/stores/reconstruction";
import { tauriApi } from "@/services/tauri";

const route = useRoute();
const proj = useCurrentProjectStore();
const recon = useReconstructionStore();
const id = computed(() => Number(route.params.id));
const expanded = ref<number | null>(null);
const reportCache = ref<Record<number, any>>({});

async function load() {
  if (proj.id !== id.value) await proj.load(id.value);
  if (proj.absPath) await recon.loadRuns(proj.absPath);
}
async function toggle(runId: number) {
  expanded.value = expanded.value === runId ? null : runId;
  if (expanded.value !== null && reportCache.value[runId] === undefined) {
    reportCache.value[runId] = await tauriApi.getRunReport(runId);
  }
}
onMounted(load);
</script>

<template>
  <div class="p-6">
    <h1 class="text-2xl font-bold">Reconstruction Runs</h1>
    <table class="mt-4 w-full text-sm">
      <thead class="border-b text-left">
        <tr>
          <th class="py-2">Created</th>
          <th>Screen</th>
          <th>Method</th>
          <th>RMS (mm)</th>
          <th>Vertices</th>
          <th>Target</th>
          <th>OBJ</th>
        </tr>
      </thead>
      <tbody>
        <template v-for="r in recon.recentRuns" :key="r.id">
          <tr class="cursor-pointer border-b hover:bg-accent" @click="toggle(r.id)">
            <td class="py-1">{{ r.created_at }}</td>
            <td>{{ r.screen_id }}</td>
            <td>{{ r.method }}</td>
            <td>{{ r.estimated_rms_mm.toFixed(2) }}</td>
            <td>{{ r.vertex_count }}</td>
            <td>{{ r.target ?? "—" }}</td>
            <td class="truncate text-xs">{{ r.output_obj_path ?? "—" }}</td>
          </tr>
          <tr v-if="expanded === r.id">
            <td colspan="7" class="bg-muted p-3">
              <pre class="text-xs">{{ JSON.stringify(reportCache[r.id], null, 2) }}</pre>
            </td>
          </tr>
        </template>
      </tbody>
    </table>
  </div>
</template>
```

- [ ] **Step 2: `Export.vue`**（简化为说明 + 跳转 Preview 按钮）

```vue
<script setup lang="ts">
import { useRouter, useRoute } from "vue-router";
const router = useRouter();
const route = useRoute();
</script>

<template>
  <div class="p-8">
    <h1 class="text-2xl font-bold">Export</h1>
    <p class="mt-2 text-sm text-muted-foreground">Export OBJ from the Preview view (the toolbar has 3 target buttons).</p>
    <button class="mt-4 rounded border px-3 py-1" @click="router.push(`/projects/${route.params.id}/preview`)">Go to Preview</button>
  </div>
</template>
```

- [ ] **Step 3: M1/M2 stub views**（`Instruct.vue` / `Charuco.vue` / `Photoplan.vue`）

每个文件：

```vue
<script setup lang="ts">
const props = defineProps<{ id?: string }>();
</script>

<template>
  <div class="p-8">
    <h1 class="text-2xl font-bold">Instruct (M1) — pending</h1>
    <p class="mt-2 text-sm text-muted-foreground">
      This view will be implemented by the M1 (total-station) session. M0.2 only provides the route.
    </p>
  </div>
</template>
```

(替换 title 为对应名字 / milestone)

- [ ] **Step 4: 提交**

```bash
git add src/views/
git commit -m "feat(views): runs/export pages + M1/M2 stubs"
```

---

## Phase 7 — 验收 + Build

### Task 31: 端到端 dev mode 手动验收

**Files:** 无新文件，仅 manual smoke。

- [ ] **Step 1: 启动 dev mode**

```bash
cd /Users/bip.lan/AIWorkspace/vp/led-mesh-toolkit-m0.2
pnpm tauri dev
```

预期：Tauri 窗口启动，Home 页显示空 recent + 3 个按钮。

- [ ] **Step 2: 验收清单（按顺序点）**

1. 点 "Create Curved-Flat (8x4)" → 选 `~/Desktop/lmt-test/` 目录 → 自动跳 `/design`，看到 8×4 grid
2. `/design`：按 M 切 mask mode → 点 cell → 半透明；按 Cmd+Z → 撤销；按 R → refs mode；按 1 → 选 origin role → 点 V001_R001 → 红框；按 2 → 选 x_axis → 点 V008_R001 → 绿框
3. 点 Save → 出 toast "Saved"
4. 跳 `/import` → 点 "Load measured.yaml" → 选 `<project>/measurements/measured.yaml` → toast "Loaded 11 measurements"
5. 跳 `/preview` → 点 "Reconstruct" → 状态 done → mesh 出现，OrbitControls 旋转/缩放
6. 点 "Export Disguise" → toast "Wrote /.../output/MAIN_disguise.obj"
7. 跳 `/runs` → 看到一行 run，点行展开看 report JSON
8. 用 Blender 打开导出的 OBJ → 看到正确 8×4 mesh + UV

- [ ] **Step 3: 文档化 verify 结果**

如果都 pass，本 task 完成；否则修对应代码。

- [ ] **Step 4: 提交（如有任何 fix）**

```bash
git add -A
git commit -m "fix: addresses found during E2E smoke"
```

无 fix 则跳过 commit。

---

### Task 32: Tauri build (macOS dmg) + Tag m0.2-complete

**Files:** 无。

- [ ] **Step 1: 跑 production build**

```bash
pnpm tauri build
```

预期：`src-tauri/target/release/bundle/dmg/*.dmg` 产物生成。

- [ ] **Step 2: 安装 dmg + 跑一次 E2E（同 Task 31 步骤简化版：seed + reconstruct + export）**

- [ ] **Step 3: 标 tag**

```bash
git tag m0.2-complete
git log --oneline m0.1-complete..m0.2-complete
```

预期：列出本 plan 全部 commit。

- [ ] **Step 4: 推 tag（按用户 git 配置；不强求）**

> 若用户用本地 worktree，`git push origin m0.2 && git push origin m0.2-complete` 留给用户决定。

---

## Self-Review

**Spec coverage 检查（spec § → task）：**
- §1 范围：所有 Phase 都实现；M1/M2 stub 见 Task 30。
- §2.1 数据流：Tasks 1, 8-13 实现 backend；Tasks 15-30 实现 frontend；Task 14 wiring。
- §2.2 真源：YAML 在 Task 7 (examples) + Task 9 (load/save)；DB 在 Tasks 4-6。
- §2.3 IR 暴露：Task 3 dto.rs + Task 19 services/tauri.ts 类型对应。
- §2.4 workspace：Task 1。
- §3.1 folder：Task 7。
- §3.2 examples + bundle.resources：Tasks 7 + 1。
- §3.3 schema：Tasks 4-6。
- §4.1 11 commands + §4.2 DTO + §4.3 error：Tasks 2-3, 8-14。
- §4.4 进度推送（M0.2 同步即可）：Task 11 reconstruct_surface 同步实现。
- §5.1 router：Task 17。
- §5.2 5 stores：Tasks 20-23。
- §5.3 组件层级：Tasks 18, 24-30。
- §5.4 自研组件：Tasks 25 (CabinetGrid)、27 (MeshPreview)。
- §5.5 i18n：Tasks 16, 24-26 各加 key。
- §5.6 service 层：Task 19。
- §5.7 /import M0.2 行为：Task 29。
- §6.1 mode 切换 + 快捷键：Task 26 (Design.vue 内 keyboard handler)。
- §6.2 undo/redo snapshot：Task 23 + Task 26 集成。
- §6.3 save 时序：Task 26 DesignToolbar.save() 内 commitMaskToScreen + save + clearStacks。
- §7 测试：Tasks 2, 3, 4-6, 8-13, 19-23 都含测试；E2E 见 Task 31-32。
- §8 验收 11 项：Task 31 步 2 列出。

**Placeholder scan：** 无 TBD/TODO；所有 step 都给完整代码或命令。Task 7 step 4 弧形坐标用 Python 脚本算出，执行时贴 yaml — 这是数据生成，不是占位。

**Type consistency 检查：**
- `ReconstructedSurface.topology = { cols, rows }` 在 Task 19 TS 定义、Task 27 buildGeometry 用法一致。
- `ScreenConfig.shape_mode = "rectangle" | "irregular"` 在 Tasks 3, 19, 23 三处定义一致。
- `editor store` 的 `commitMaskToScreen` 返回 `ScreenConfig`，跟 `proj.updateScreen` 接收类型一致（Task 26 toolbar.save 内）。
- `tauriApi.exportObj(runId, target)` 的 args 命名（runId / target）跟 Task 12 Rust 端 (`run_id`, `target`) 经 Tauri camelCase 转换一致。
- `ReconstructionResult.surface` 在 Task 11 Rust DTO 是 `ReconstructedSurface`，Task 19 TS interface 也是。

无 inline 修复需要。

---

## Execution Handoff

执行：用 `superpowers:subagent-driven-development` 一 task 一 fresh subagent，每 task commit 前调 codex 做 adversarial review（用户要求）。Codex CLI 通过 `codex:rescue` agent 调用，不是 slash command。

Plan 完整保存在本文件。

