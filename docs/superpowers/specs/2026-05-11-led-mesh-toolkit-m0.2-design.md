# LED Mesh Toolkit — M0.2 Design (GUI Shell + Tauri 集成)

> **状态**：Draft v1.0
> **日期**：2026-05-11
> **负责人**：lanbipu@gmail.com
> **前置**：m0.1-complete tag（lmt-core 全套 IR + 重建 + UV + 焊接 + 导出已 frozen）
> **后续**：M1 (adapter-total-station session) + M2 (adapter-visual-ba session) 并行启动

---

## 1. 范围

M0.2 = **Shell + 共用视图骨架 + 端到端 demo 管道**。

包含：
- Cargo workspace 接入 `src-tauri/`
- DB schema (rusqlite) + 2 张表
- Tauri 11 个 command + 错误模型
- Vue 3 前端 shell（router / 5 个 Pinia store / i18n / theme）
- 共用视图：`/`（Home）、`/design`、`/preview`、`/export`、`/runs`
- 自研组件：`CabinetGrid.vue`（vue-konva 2D 编辑器）+ `MeshPreview.vue`（Three.js 3D 预览）
- 从 UECM copy 共用 UI 组件（`LmtAppShell` / `LmtSidebar` / `LmtTopBar` / `LmtLogPanel` + primitives）
- 2 个内置 example 项目（curved-flat + curved-arc）+ `seed_example_project` command
- Tauri build 产物（macOS dmg + Windows exe）

不做：
- M1 / M2 实际 adapter 实现（各 session 自行）
- M1/M2 专属 view 的实现：`/import` / `/instruct` / `/charuco` / `/photoplan` 仅占位 stub
- E2E 测试自动化（Tauri webdriver 不上）
- 多窗口 / 多项目同时打开
- 项目 zip 导入导出
- 跨 view 的 undo / save 后 undo
- ts-rs 类型自动生成（手动同步）

---

## 2. 整体架构

### 2.1 数据流

```
[Vue 3 前端]                     [Tauri commands (Rust)]              [lmt-core (frozen)]
  AppShell + Router                load_project_yaml         ──→ serde_yaml ↔ ProjectConfig
  Pinia stores (5)                 save_project_yaml         
  vue-konva /design                list_recent_projects      ──→ rusqlite DB
  Three.js /preview                add_recent_project
  vue-i18n + ThemeToggle           load_measurements_yaml    ──→ MeasuredPoints
  services/tauri.ts                reconstruct_surface       ──→ auto_reconstruct()
                                   export_obj                ──→ surface_to_mesh_output
                                                                 + write_obj
                                   list_runs / get_run_report ──→ rusqlite DB
                                   seed_example_project      ──→ resources/examples/ 复制
```

### 2.2 数据真源

- **YAML 是项目数据真源**：项目 = folder，主配置在 `<folder>/project.yaml`。GUI 编辑 → 写回 YAML。
- **DB 是轻索引**：只存 recent_projects + reconstruction_runs。无 YAML 配置入 DB。
- **localStorage**：theme / lang。

### 2.3 IR 暴露策略

直接把 `lmt-core` 类型作为 Tauri command 的 args / return type。IR 已用 `#[serde(with=...)]` 把 `nalgebra::Vector3<f64>` 包成 `[f64; 3]`，TS 边对应 `[number, number, number]`。**不引入 DTO 中间层** — IR 已 frozen，再加一层等于双份维护。

仅 `ProjectConfig`（用户在 GUI/YAML 编辑的几何/形状/参考点配置）是 src-tauri 自有 DTO，不是 lmt-core 类型。`MeasuredPoints` 才是 IR。

### 2.4 Cargo workspace

把 `src-tauri/` 并入现有 workspace（第 4 个 member），`src-tauri/Cargo.toml` 通过 path 引用 `lmt-core`。

```toml
# Cargo.toml (root)
[workspace]
members = [
    "crates/core",
    "crates/adapter-total-station",
    "crates/adapter-visual-ba",
    "src-tauri",
]
```

```toml
# src-tauri/Cargo.toml
[dependencies]
lmt-core = { path = "../crates/core" }
tauri = { version = "2", features = [] }
rusqlite = { version = "0.31", features = ["bundled"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yaml = "0.9"          # 跟 workspace 一致
tokio = { version = "1", features = ["full"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
thiserror = "1"
chrono = { version = "0.4", features = ["serde"] }
```

---

## 3. 项目 folder + DB schema

### 3.1 项目 folder layout

```
my-project/
├── project.yaml                     # 主配置 (cabinet_array / shape_prior / coordinate_system / output)
├── measurements/
│   ├── measured.yaml                # M0.2 demo 输入；M1/M2 adapter 也写这里
│   ├── points.csv                   # M1 全站仪原始 CSV (M1 session 加)
│   └── images/                      # M2 图像 (M2 session 加)
├── output/
│   ├── MAIN_disguise.obj
│   └── FLOOR_disguise.obj
├── reports/
│   └── 2026-05-11T15-23-04.json     # ReconstructionReport: surface + quality_metrics + 元数据
└── instruction-cards/               # M1 session 加，M0.2 不创建
```

M0.2 阶段只 touch `project.yaml` / `measurements/measured.yaml` / `output/*.obj` / `reports/*.json`。其他子目录留给 M1/M2 session。

### 3.2 内置示例项目（resources）

- `examples/curved-flat/`：平面屏 8×4 + 11 测点（满足 RBF ≥5 anchors + 4 corners + ≥1 interior 触发条件）
- `examples/curved-arc/`：弧形屏 16×6 + 测点

`examples/` 目录放在仓库根（与 `crates/` / `src-tauri/` / `src/` 同级）。`src-tauri/tauri.conf.json` 的 `bundle.resources` 配 `"../examples": "examples"`。运行时 Rust 端通过 `app.path().resource_dir()?.join("examples")` 取得源路径。`seed_example_project(target_dir, example_name)` 把 resources 内容复制到用户选的目录。

### 3.3 DB schema

DB 路径：`app.path().app_data_dir().join("lmt.sqlite")`。schema migration 启动时跑（同 UECM 模式）。

```sql
-- 001_recent_projects
CREATE TABLE IF NOT EXISTS recent_projects (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    abs_path TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    last_opened_at TEXT NOT NULL,           -- ISO 8601
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_recent_projects_last_opened
    ON recent_projects(last_opened_at DESC);

-- 002_reconstruction_runs
CREATE TABLE IF NOT EXISTS reconstruction_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_path TEXT NOT NULL,             -- 跟 recent_projects.abs_path 对应（不做 FK，项目可被删）
    screen_id TEXT NOT NULL,                -- "MAIN" / "FLOOR"
    measurements_path TEXT NOT NULL,        -- 相对项目根的输入路径
    method TEXT NOT NULL,                   -- "direct_link" / "radial_basis" / ...
    measured_count INTEGER NOT NULL,
    expected_count INTEGER NOT NULL,
    estimated_rms_mm REAL NOT NULL,
    estimated_p95_mm REAL NOT NULL,
    vertex_count INTEGER NOT NULL,
    output_obj_path TEXT,                   -- 相对项目根的 OBJ 路径；NULL 表示只重建未导出
    report_json_path TEXT NOT NULL,         -- 相对项目根的 report 路径（reconstruct 时必写）
    target TEXT,                            -- "disguise" / "unreal" / "neutral"；NULL 直到首次 export
    warnings_json TEXT NOT NULL,            -- JSON array of warning strings
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_runs_project_screen
    ON reconstruction_runs(project_path, screen_id, created_at DESC);
```

---

## 4. Tauri commands

### 4.1 Command 清单

| # | Command | Args | Returns | 用途 |
|---|---|---|---|---|
| 1 | `list_recent_projects` | — | `Vec<RecentProject>` | 启动页拉 recent 列表 |
| 2 | `add_recent_project` | `abs_path: String, display_name: String` | `RecentProject` | 创建/打开项目时 upsert |
| 3 | `remove_recent_project` | `id: i64` | `()` | 从 recent 列表移除（不删 folder） |
| 4 | `seed_example_project` | `target_dir: String, example: String` | `String` (创建的 folder 绝对路径) | 把 resources/examples/<example>/ 复制出去 |
| 5 | `load_project_yaml` | `abs_path: String` | `ProjectConfig` | 读 `<abs_path>/project.yaml` |
| 6 | `save_project_yaml` | `abs_path: String, config: ProjectConfig` | `()` | 原子写 `<abs_path>/project.yaml` (temp + rename) |
| 7 | `load_measurements_yaml` | `path: String` | `MeasuredPoints` | 读 measurements yaml，IR 类型 |
| 8 | `reconstruct_surface` | `project_path: String, screen_id: String, measurements_path: String` | `ReconstructionResult` | 调 `auto_reconstruct`，写 `reports/<ts>.json`（含 surface + quality），**不写 OBJ**，DB 登记 run（output_obj_path = NULL，target = NULL） |
| 9 | `export_obj` | `run_id: i64, target: TargetSoftware` | `String` (写出的 OBJ 绝对路径) | 从 DB 取 run → 读 reports JSON 拿 surface → 调 `surface_to_mesh_output` + `write_obj` 写 `output/<screen_id>_<target>.obj` → 更新 run.target + run.output_obj_path（同一 run 多次切 target 导出，最后一次胜出；不同 target 的 OBJ 文件名不冲突） |
| 10 | `list_runs` | `project_path: String, screen_id: Option<String>` | `Vec<ReconstructionRun>` | DB 查询历史 run |
| 11 | `get_run_report` | `run_id: i64` | `serde_json::Value` | 读 `reports/<ts>.json` 全文（含 surface + quality + warnings） |

### 4.2 关键 DTO

`src-tauri/src/dto.rs`，前端 TypeScript 1:1 镜像在 `src/services/tauri.ts`：

```rust
#[derive(Serialize, Deserialize)]
pub struct RecentProject {
    pub id: i64,
    pub abs_path: String,
    pub display_name: String,
    pub last_opened_at: String,         // ISO 8601
}

#[derive(Serialize, Deserialize)]
pub struct ProjectConfig {
    pub project: ProjectMeta,
    pub screens: BTreeMap<String, ScreenConfig>,        // "MAIN" / "FLOOR"
    pub coordinate_system: CoordinateSystemConfig,
    pub output: OutputConfig,
}
// ScreenConfig 字段对应 spec §4.2 YAML：
//   cabinet_count / cabinet_size_mm / shape_prior / shape_mode /
//   irregular_mask / bottom_completion
// 用平实 plain struct，不复用 lmt-core 内部类型 ——
// YAML 是用户-facing 表达，跟 lmt-core 内部类型解耦。

#[derive(Serialize)]
pub struct ReconstructionResult {
    pub run_id: i64,
    pub surface: ReconstructedSurface,                  // 直接来自 lmt-core
    pub report_json_path: String,
}

#[derive(Serialize)]
pub struct ReconstructionRun {
    pub id: i64,
    pub screen_id: String,
    pub method: String,
    pub estimated_rms_mm: f64,
    pub vertex_count: i64,
    pub target: Option<String>,                         // NULL 直到首次 export_obj
    pub output_obj_path: Option<String>,                // NULL 直到首次 export_obj
    pub created_at: String,
}

// reports/<ts>.json 文件 schema（不入 DB，文件系统持久化）
#[derive(Serialize, Deserialize)]
pub struct ReconstructionReport {
    pub surface: ReconstructedSurface,                  // 来自 lmt-core，含 vertices + uv + topology
    pub quality_metrics: QualityMetrics,                // 来自 lmt-core
    pub project_path: String,
    pub screen_id: String,
    pub measurements_path: String,
    pub created_at: String,
}
```

`ProjectConfig` ≠ `MeasuredPoints`：
- `ProjectConfig` 是 GUI/YAML 用户-facing 表达（cabinet 阵列、形状先验、3 参考点角色名），存在 `src-tauri/dto.rs`
- `MeasuredPoints` 是 lmt-core 的 IR 类型（测点位置 + 不确定度 + source）
- `reconstruct_surface` command 内部把 ProjectConfig 的几何信息 + MeasuredPoints 拼装成 lmt-core 需要的输入

### 4.3 错误模型

```rust
// src-tauri/src/error.rs
#[derive(Debug, thiserror::Error, Serialize)]
#[serde(tag = "kind", content = "message")]
pub enum LmtError {
    #[error("io: {0}")]
    Io(String),
    #[error("yaml: {0}")]
    Yaml(String),
    #[error("core: {0}")]
    Core(String),                     // 包 lmt-core::CoreError
    #[error("db: {0}")]
    Db(String),
    #[error("not_found: {0}")]
    NotFound(String),
    #[error("invalid_input: {0}")]
    InvalidInput(String),
}
pub type LmtResult<T> = Result<T, LmtError>;
```

每个 command `-> LmtResult<T>`。前端 services/tauri.ts 把 invoke reject 解成 `{ kind, message }`，全局 toast。

### 4.4 进度推送

- `reconstruct_surface` 同步调用即可（M0.1 数据规模重建在毫秒级），不上 channel
- `seed_example_project` 完成后 `app.emit("project-seeded", { abs_path })`，前端 reactive
- M1/M2 加 `import_csv` / `run_bundle_adjustment` 时再上 channel + emit 模式（参考 UECM `batch-progress`）

---

## 5. 前端结构

### 5.1 Router

Hash history（同 UECM）：

```ts
const routes = [
  { path: "/",                          name: "home",      component: Home },         // recent + 新建/打开
  { path: "/projects/:id/design",       name: "design",    component: Design },       // ✅ M0.2 实现
  { path: "/projects/:id/preview",      name: "preview",   component: Preview },      // ✅ M0.2 实现
  { path: "/projects/:id/export",       name: "export",    component: Export },       // ✅ M0.2 实现
  { path: "/projects/:id/runs",         name: "runs",      component: Runs },         // ✅ M0.2 实现
  { path: "/projects/:id/import",       name: "import",    component: Import },       // ⚪️ partial stub (M0.2 仅 demo measured.yaml load — 见 §5.7)
  { path: "/projects/:id/instruct",     name: "instruct",  component: Instruct },     // ⚪️ stub (M1)
  { path: "/projects/:id/charuco",      name: "charuco",   component: Charuco },      // ⚪️ stub (M2)
  { path: "/projects/:id/photoplan",    name: "photoplan", component: Photoplan },    // ⚪️ stub (M2)
];
```

`:id` = `recent_projects.id`（数字）。store 解 id → abs_path。URL 不暴露绝对路径。

### 5.2 Pinia stores（5 个）

| Store | 持久化 | 关键字段 + actions |
|---|---|---|
| `useProjectsStore` | DB（recent_projects） | `recent: RecentProject[]`、`load()`、`createFromExample(example, targetDir)`、`openExisting(folder)`、`remove(id)` |
| `useCurrentProjectStore` | YAML（project.yaml） | `id`、`absPath`、`config: ProjectConfig`、`dirty: boolean`、`load(id)`、`save()`、`updateScreen(...)` |
| `useReconstructionStore` | in-memory | `currentSurface`、`currentRunId`、`status`、`reconstruct(screenId, measurementsPath)`、`exportObj(target)`、`recentRuns: ReconstructionRun[]`、`loadRuns()` |
| `useEditorStore` | in-memory（/design 内） | `mode: 'mask'\|'refs'\|'baseline'`、`selectedCells`、`undoStack`、`redoStack`、`commit()` 把变化写回 currentProject |
| `useUiStore` | localStorage（theme/lang）+ in-memory（log panel） | `logOpen`、`theme`、`lang`、`toasts: Toast[]` |

### 5.3 组件层级

```
App.vue
└── LmtAppShell.vue                      (copy from UECM)
    ├── LmtSidebar.vue                   (copy + 替换菜单项)
    ├── LmtTopBar.vue                    (copy + 项目名 + dirty 指示)
    ├── <RouterView />
    │   ├── Home.vue                     (recent 列表 + 创建按钮)
    │   ├── Design.vue                   (布局容器)
    │   │   ├── DesignToolbar.vue        (mode switcher + undo/redo + save)
    │   │   ├── ScreenPicker.vue         (MAIN / FLOOR 切换)
    │   │   └── CabinetGrid.vue          ⭐ vue-konva 主组件
    │   │       └── CabinetGridLegend.vue
    │   ├── Preview.vue                  (布局容器)
    │   │   ├── PreviewToolbar.vue       (target switcher + 重建/导出按钮)
    │   │   └── MeshPreview.vue          ⭐ Three.js 主组件
    │   ├── Export.vue                   (target 选择 + 导出 + run 状态)
    │   ├── Runs.vue                     (历史 run 表格)
    │   ├── Import.vue                   (M0.2 仅 "load demo measured.yaml")
    │   └── (M1/M2 stub views — 单 PageHeader + "M1/M2 implementation pending")
    └── LmtLogPanel.vue                  (copy)

components/primitives/                   Lmt-prefixed (copy from UECM):
  LmtButton, LmtInput, LmtKV, LmtPathInput, LmtPageHeader,
  LmtProgressBar, LmtStatusBadge, LmtStatusDot, LmtThemeToggle, LmtLanguageToggle
```

### 5.4 自研组件细节

**`CabinetGrid.vue`（vue-konva）**

- `<v-stage>` + `<v-layer>` + `v-for` 生成 cabinet rect
- 交互（按 `useEditorStore.mode` 派发）：
  - `mask` mode：点击 toggle present
  - `refs` mode：点击 cell 设为当前 ref 角色（origin / X-axis / XY-plane 三选一，工具栏切换当前角色）
  - `baseline` mode：点击行号 / 拖整行设 baseline_row
- 视觉：absent cell 半透明 + 灰色斜杠；ref cell 描红/绿/蓝边框；baseline 行画水平虚线
- 性能：120×20=2400 cell 单层渲染

**`MeshPreview.vue`（Three.js + 薄 Vue 包装）**

- `useTemplateRef` 拿 canvas
- `onMounted`：`new THREE.Scene/PerspectiveCamera/WebGLRenderer + OrbitControls`
- `watch(props.surface)`：重建 `BufferGeometry`
  - `position` attribute = vertices flatten
  - `index` buffer = triangles flatten
  - `uv` attribute = uv_coords flatten
- `onUnmounted`：dispose geometry / material / texture / `renderer.dispose()`，移除 OrbitControls listener

### 5.5 i18n

vue-i18n + locales/{en,zh}.json。M0.2 阶段全 view 走 `t('design.toolbar.mask')` 这种 key，避免硬编码中文。

### 5.6 Tauri service 层

`src/services/tauri.ts`：所有 invoke 包成 typed function。所有 IR 类型 + DTO 一份 TypeScript 定义文件，跟 `src-tauri/dto.rs` 手动同步。

### 5.7 /import view 的 M0.2 行为

仅展示一个 "Load measurements YAML" 文件选择器，调 `load_measurements_yaml` command，加载成功后把路径存到 `useReconstructionStore.measurementsPath`，并 toast 显示 "Loaded N measurements"。M0.2 不实现任何解析器逻辑。

M1 session 自然扩展为 "M1: Import CSV" 标签 + 全站仪 CSV 解析器；M2 session 扩展为 "M2: Import Images" 标签 + 图像处理入口。M0.2 只占 stub。

`/preview` 重建按钮的前置条件是 `useReconstructionStore.measurementsPath` 已设置；未设置则按钮 disabled，hover 提示 "Load measurements first (Import view)"。

---

## 6. /design 编辑器交互细节

### 6.1 Mode 切换

工具栏 3 个按钮（mask / refs / baseline）+ 快捷键 `M` / `R` / `B`。当前 mode 在按钮上高亮。鼠标悬浮按钮显示快捷键提示。

`refs` mode 内还要选当前 ref 角色（origin / X-axis / XY-plane）— 用 sub-toolbar，快捷键 `1` / `2` / `3`。

### 6.2 Undo/redo

Snapshot stack：
- `useEditorStore` 维护 `undoStack: EditorSnapshot[]` + `redoStack: EditorSnapshot[]`
- `EditorSnapshot` = `{ irregular_mask, refs, baseline_row }` 完整复制
- 每次编辑动作（toggle cell / set ref / set baseline）push 到 undoStack
- 最多保 50 步（FIFO 截断）
- Cmd/Ctrl+Z 弹 undoStack 顶 → 应用 → push redoStack；Cmd/Ctrl+Shift+Z 反向
- 仅 `/design` 页面内有效；切换 view 或 save 后清空 stack

### 6.3 Save 时序

- `useEditorStore.commit()` 把当前编辑器 state 写入 `useCurrentProjectStore.config`
- 用户点 Save → `useCurrentProjectStore.save()` → `save_project_yaml` command
- Save 成功 → 清 dirty + 清 undo/redo stack

---

## 7. 测试策略

| 层 | 工具 | M0.2 覆盖 |
|---|---|---|
| Rust unit (lmt-core) | cargo test | 已有，M0.2 不动 |
| Rust unit (src-tauri commands) | cargo test + tempfile | 每个 command ≥1 happy path + ≥1 error path（IO / YAML / DB） |
| Rust integration (DB) | cargo test + in-memory rusqlite | recent_projects upsert + runs 写查 |
| TypeScript unit (stores) | Vitest + @vue/test-utils | useEditorStore undo/redo、useCurrentProjectStore dirty 标记 |
| TypeScript unit (services) | Vitest + mock invoke | services/tauri.ts typed wrapper |
| 端到端（手动） | dev mode | 走 demo 项目：seed → load → reconstruct → preview → export OBJ → 在 Disguise / Blender 加载验证 |
| 跨平台 | macOS（开发）+ Windows（验收） | M0.2 验收必跑 Windows |

E2E 自动化不上（Playwright + Tauri webdriver 工作量在 M0.2 不值）。

---

## 8. 验收清单

1. Cargo workspace 包含 src-tauri，`cargo build --workspace` 全绿
2. `pnpm tauri dev` 启动，HMR 工作
3. 启动页能从 2 个内置 example 之一创建项目，自动跳 `/design`
4. `/design`：120×20 grid 渲染流畅，3 mode 切换 + 快捷键、删 cell、选 3 ref、拖 baseline，undo/redo 工作；save 写回 project.yaml 跟 lmt-core test fixture 字段一致
5. `/preview`：Three.js mesh 渲染，OrbitControls 可旋转/缩放；切 target（disguise/unreal/neutral）geometry 重建（坐标系适配）
6. `/export`：写出 OBJ 文件，文件可在 Blender / Disguise Designer 加载，UV 正确
7. `/runs`：历史 run 表格显示，能展开看 quality report
8. `recent_projects` DB 持久化跨重启
9. 主题切换（dark/light）+ 语言切换（en/zh）正常
10. `pnpm tauri build` 在 macOS 出 dmg、Windows 出 exe 安装包
11. M1/M2 stub view 都路由可达，渲染 "pending implementation" 占位

---

## 9. 风险 + 缓解

| 风险 | 严重度 | 缓解 |
|---|---|---|
| Three.js 在 Tauri webview 里 dispose 不干净导致内存泄漏 | 中 | onUnmounted 严格 dispose geometry/material/texture/`renderer.dispose()`；浏览器 devtools 跑 memory profile |
| vue-konva 120×20=2400 rect 渲染卡顿 | 中 | 先按 spec §10.1 真实场景测；卡则降级到 Konva 原生（composable + new Stage/Layer 直接 add）— vue-konva 是 wrapper，不影响 Konva 性能本身 |
| Tauri 2 + rusqlite bundled feature 在 Windows MSVC 编译链 | 低 | UECM 已验证可行 |
| YAML schema 在 GUI 编辑后跟 lmt-core 测试 fixture 漂移 | 中 | src-tauri 的 ProjectConfig 序列化测试比对 lmt-core 现有 YAML fixture（serde round-trip），保证字段一致 |
| Cargo workspace 引入 src-tauri 后 lmt-core test 编译时间膨胀 | 低 | 用 `--package` 参数限定，CI 分两 job |
| TS 类型跟 Rust DTO 手动同步漂移 | 中 | dto.rs 顶部注释指向 `src/services/tauri.ts` 对应 interface；CI 加 grep check（确认两边字段名/数量一致）；后续可上 ts-rs |
| Three.js bundle 体积（~600KB） | 低 | 接受，desktop app 不是 web |

---

## 10. 显式不做（M0.2 范围外）

- M1 / M2 实际 adapter 实现
- E2E 测试自动化
- 多窗口 / 多项目同时打开
- 项目 zip 导入导出
- 跨 view 的 undo / save 后 undo
- ts-rs 类型自动生成

---

## 附录 A：关键决策追溯

| # | 决策 | 选择 |
|---|---|---|
| 1 | M0.2 范围 | Shell + 共用视图骨架 + demo 管道 |
| 2 | 项目数据真源 | YAML + DB 轻索引 |
| 3 | Tauri command 粒度 | 细粒度 + 前端编排 |
| 4 | Three.js Vue 集成 | 直接 + 薄 Vue 包装 |
| 5 | Konva Vue 集成 | vue-konva (declarative) |
| 6 | Pinia store 拆分 | 按数据域 + 持久化边界（5 个 store）|
| 7 | demo 数据来源 | MeasuredPoints YAML fixture（内置 example）|
| 8 | Undo/redo | Snapshot stack 限 /design 页面内 |
| 9 | 编辑器 mode 切换 | Toolbar + 快捷键 |
| 10 | UECM 复用 | Copy + rename 为 Lmt 前缀 |
| 11 | 项目 folder 结构 | 平铺 folder（不打包 zip）|
| 12 | src-tauri 与 workspace 关系 | 并入现有 workspace（第 4 个 member）|
| 13 | IR 暴露策略 | 直接暴露，不引入 DTO 中间层 |

---

## 附录 B：参考资料

- 上层 spec：`docs/superpowers/specs/2026-05-10-led-mesh-toolkit-design.md`（§7 GUI 视图、§2.3 技术栈）
- IR freeze notice：`docs/IR-FROZEN.md`
- UECM 项目（同栈 Tauri 2 + Vue 3 应用）：`/Users/bip.lan/AIWorkspace/vp/ue-cache-manager`
- M0.1 端到端 test：commit `3fccaa4` (`crates/core/tests/`)
