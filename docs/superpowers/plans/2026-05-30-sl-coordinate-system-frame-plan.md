# 视觉/结构光重建落入项目坐标系 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 `reconstruct-structured-light → export pose-obj → disguise` 导出的 OBJ 落在项目 `coordinate_system` 设计帧、可 drop-in 替换源模型，朝向误差从 ~1° 降到重建噪声本底。

**Architecture:** 混合方案。**生产端**（Python sidecar）复活 `align_to_nominal`：把重建帧角点用固定置换 `P` 转 M0.1 约定，再用 Procrustes 把全部角点稳健配准到 nominal 设计网格（√N 压噪），report 落 M0.1。**消费端**（Rust `export pose-obj`）读 report 帧版本分支；`--coordinate-system` 时在**无噪声 nominal** 上由 `coordinate_system` 三网格名定设计帧重锚，再 `adapt_to_target`。帧定义在 lmt-core 抽共享 helper，与全站仪同源。

**Tech Stack:** Rust（lmt-core / lmt-shared / lmt-app / lmt-cli，nalgebra）、Python sidecar（numpy，pydantic ipc）、clap CLI、serde + schemars。

**Spec:** `docs/superpowers/specs/2026-05-30-sl-coordinate-system-frame-design.md`

---

## Context（自包含：假设无本仓库背景）

### 两个帧约定（已读源码核实）

- **重建帧**（pose report `corners_mm`；根箱体 `V000_R000` 局部帧）：`X=列, Y=行向上, Z=外法向`。来源 `reconstruct_cabinet_geometry`（normal=`R@[0,0,1]`）。现有 `export pose-obj` 把它当 "disguise-native +Y up/+Z outward"。
- **M0.1 / measured.yaml / 设计帧**（全站仪路径 + `adapt_to_target` 输入）：`X=列, Y=外法向, Z=行向上`。来源 Rust `expected_grid_positions` + `from_three_points` 的 M0.1 排列 `[b0,b2,-b1]`。
- 两者差一个 **Y↔Z 有号换轴**（det=+1 正常旋转）。两边网格原点都在 `V001_R001` 左下顶点。

### 命名/编号对账

- coordinate_system 网格名 `{screen}_V{c+1:03}_R{r+1:03}`（`shape_grid.rs:41`）：网格**顶点**、**1-based**、带屏幕前缀，`(cols+1)×(rows+1)` 个。
- pose report `cabinet_id` `V{col:03}_R{row:03}`（`reconstruct.py:249`）：**箱体**、**0-based**、无前缀。
- 顶点 `(vc,vr)`（0-based）= 箱体 `(vc,vr)` 的 BL 角；边缘顶点回退相邻箱体角。
- 解析：剥最长匹配屏幕前缀 → 解 `V/R` → 减 1（顶点 1-based）→ 顶点→箱体角。

### 关键文件（路径:行）

- `crates/core/src/coordinate.rs`：`CoordinateFrame{origin_world,basis}` :19-23；`from_three_points` :91-124；`world_to_model` :143；`validate_basis`（反序列化校验单位/正交/右手）:52-86。
- `crates/adapter-total-station/src/reference_frame.rs`：`build_frame_from_first_three` :24-91；M0.1 排列 `[b0,b2,-b1]` :85-89。
- `crates/adapter-total-station/src/shape_grid.rs`：`expected_grid_positions` :23-103（Flat :35-48 / Curved :49-80 / Folded→当平面 :81-99）；`GridExpected{name,model_position,col/row_zero_based}` :7-15。
- `crates/core/src/export/adapt.rs`：`adapt_to_target` :22（Disguise `(x,y,z)→(x,z,-y)`；Neutral identity）。
- `crates/core/src/export/build.rs`：disguise winding swap + UV U 镜像 :114-120。
- `crates/lmt-shared/src/dto.rs`：`CabinetPoseReportFile` :338-342；`CabinetPoseEntry{cabinet_id,corners_mm}` :345-349；`ExportPoseObjResult` :352-357；`VisualReconstructResult` :242-253。
- `crates/lmt-shared/src/schema.rs`：`add!(...)` 注册 + `dump_all()`（完整/incomplete 列表）。
- `crates/lmt-shared/src/envelope.rs`：`error_codes::PROCRUSTES_FAILED` :124；`exit_codes.rs:26`（exit 15）。
- `crates/lmt-app/src/export.rs`：`run_export_pose_obj` :153-159；`check_pose_obj_inputs` :291；`apply_canonical_frame` :458-502；`CabinetFrame::from_corners` :347-367；`parse_cabinet_col_row` :371-374；`infer_grid_dims` :381-395。
- `crates/lmt-app/src/projects.rs`：`load_project_yaml_from_path` :119。
- `crates/lmt-app/src/total_station_mapper.rs`：`map_to_adapter` :15-52；`check_grid_name_prefix`/`grid_suffix_valid`/`split_digits` :54-104。
- `crates/lmt-cli/src/cli.rs`：`ExportCmd::PoseObj` :273-290。
- `crates/lmt-cli/src/commands/export.rs`：`pose_obj` :137-213。
- `crates/lmt-cli/tests/cli_e2e.rs`：pose-obj 块 ~1592-1799；SL 块 ~2089-2159。
- `python-sidecar/src/lmt_vba_sidecar/reconstruct.py`：`solve_and_emit` :445；构建 poses/measured :574-633；写 report :636-642；`ResultData(... procrustes_align_rms_m=0.0)` :645-660；`_cabinet_id` :249；`ROOT_CABINET=(0,0)` :77。
- `python-sidecar/src/lmt_vba_sidecar/nominal.py`：`nominal_cabinet_centers_model_frame` :142；`_normals` :123；`_cabinet_center_model_m` :63；folded `raise` :81-85,115-119。
- `python-sidecar/src/lmt_vba_sidecar/procrustes.py`：`procrustes_rigid(src,dst)->(R,t,rms)` :23（SVD 无缩放 + det 修正）。
- `python-sidecar/src/lmt_vba_sidecar/ipc.py`：`FrameSpec(gauge_strategy …)` :400-408；`CabinetPose` :411-421；`CabinetPoseReport` :424-427；`ResultData.procrustes_align_rms_m` :311。
- `python-sidecar/src/lmt_vba_sidecar/sl_reconstruct.py`：`run_reconstruct_structured_light` :67；调 `solve_and_emit` :195。
- `docs/agents-cli.md`、`docs/contract-manifest.json`。

### 置换 P（重建帧→M0.1）的处理原则

`P` 是固定 3×3 有号置换，满足 `m01 = P · recon`，由 `B_m01=B_recon·Π`（`Π` 来自 `[b0,b2,-b1]`）得 `P=Πᵀ`。**不硬抄定号**——由 Task 6 的单测钉死（`P@外法向==+Y`、`P@行向上==+Z`、`det=+1`）。候选 `(x,y,z)→(x,z,-y)` 仅作起点，以测试为准。

### 相位排序铁律

消费端（Task 2-4）必须先能读 `gauge_strategy` 帧版本，生产端（Task 5-6）才默认产 M0.1，否则中间态 `SL→export` 会把旧 canonical 套到 M0.1 错轴。每个 Task 结束时 `cargo test --workspace`（或对应 crate）必须绿。

### Worktree venv 坑（实施前必读）

跑 pytest 前确认 `python-sidecar/.venv` 不是软链主仓库（editable `.pth` 会把 import 解析回主 src，新模块 import 不到）。worktree 里建独立 venv + 手写 `.pth` 复用主 site-packages（见 memory `reference_worktree_venv_isolation`）。

---

## Task 1: lmt-core 共享帧 helper `from_three_points_m01`

把"`from_three_points` + M0.1 排列"抽到 lmt-core，全站仪 `reference_frame.rs` 改调它（行为不变）。

**Files:**
- Modify: `crates/core/src/coordinate.rs`（新增方法 + 测试）
- Modify: `crates/adapter-total-station/src/reference_frame.rs:72-90`

- [ ] **Step 1: 写失败测试**（`crates/core/src/coordinate.rs` 的 `#[cfg(test)] mod tests`）

```rust
#[test]
fn from_three_points_m01_axes_and_handedness() {
    use nalgebra::Vector3;
    // origin, +x (cols), +xy (up a column) —— 简单正交布置
    let o = Vector3::new(0.0, 0.0, 0.0);
    let x = Vector3::new(1.0, 0.0, 0.0);   // x_axis 沿 +X
    let xy = Vector3::new(0.0, 0.0, 1.0);  // xy_plane 沿 +Z（"上"）
    let f = CoordinateFrame::from_three_points_m01(o, x, xy).unwrap();
    // basis 列：col0=X(cols), col1=Y(normal), col2=Z(rows-up)
    let b = f.basis;
    // M0.1: col0 = +X
    assert!((b[0][0]-1.0).abs() < 1e-9 && b[0][1].abs()<1e-9 && b[0][2].abs()<1e-9);
    // 右手 + 正交（validate_basis 已有逻辑，这里直接复算 det=+1）
    let det = b[0][0]*(b[1][1]*b[2][2]-b[1][2]*b[2][1])
            - b[0][1]*(b[1][0]*b[2][2]-b[1][2]*b[2][0])
            + b[0][2]*(b[1][0]*b[2][1]-b[1][1]*b[2][0]);
    assert!((det-1.0).abs() < 1e-6, "det={det}");
}

#[test]
fn from_three_points_m01_matches_manual_permutation() {
    use nalgebra::Vector3;
    let o = Vector3::new(1.0, 2.0, 3.0);
    let x = Vector3::new(2.0, 2.0, 3.0);
    let xy = Vector3::new(1.0, 2.0, 4.0);
    let native = CoordinateFrame::from_three_points(o, x, xy).unwrap();
    let b = &native.basis;
    let expected = [b[0], b[2], [-b[1][0], -b[1][1], -b[1][2]]];
    let f = CoordinateFrame::from_three_points_m01(o, x, xy).unwrap();
    for i in 0..3 { for j in 0..3 {
        assert!((f.basis[i][j]-expected[i][j]).abs() < 1e-12);
    }}
    assert_eq!(f.origin_world, native.origin_world);
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p lmt-core from_three_points_m01 -- --nocapture`
Expected: 编译失败 / `no method named from_three_points_m01`。

- [ ] **Step 3: 实现**（`crates/core/src/coordinate.rs`，加到 `impl CoordinateFrame`）

```rust
/// `from_three_points` 后套 M0.1 排列：basis = [b0, b2, -b1]
/// → model +X=cols, +Y=screen normal, +Z=rows-up（与全站仪 reference_frame 同约定）。
pub fn from_three_points_m01(
    origin: nalgebra::Vector3<f64>,
    x_axis: nalgebra::Vector3<f64>,
    xy_plane: nalgebra::Vector3<f64>,
) -> Result<Self, CoreError> {
    let native = Self::from_three_points(origin, x_axis, xy_plane)?;
    let b = &native.basis;
    Ok(CoordinateFrame {
        origin_world: native.origin_world,
        basis: [b[0], b[2], [-b[1][0], -b[1][1], -b[1][2]]],
    })
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p lmt-core from_three_points_m01`
Expected: 2 passed。

- [ ] **Step 5: 重构 reference_frame 调用它**（`crates/adapter-total-station/src/reference_frame.rs:72-90`）

把
```rust
let native = CoordinateFrame::from_three_points(origin, x_axis, xy_plane).map_err(AdapterError::Core)?;
let b = &native.basis;
let frame = CoordinateFrame { origin_world: native.origin_world, basis: [b[0], b[2], [-b[1][0], -b[1][1], -b[1][2]]] };
Ok(frame)
```
替换为
```rust
CoordinateFrame::from_three_points_m01(origin, x_axis, xy_plane).map_err(AdapterError::Core)
```

- [ ] **Step 6: 全站仪测试不回归**

Run: `cargo test -p lmt-adapter-total-station`
Expected: all passed（行为未变）。

- [ ] **Step 7: 提交**

```bash
git add crates/core/src/coordinate.rs crates/adapter-total-station/src/reference_frame.rs
git commit -m "feat(core): from_three_points_m01 shared helper; reference_frame reuses it"
```

---

## Task 2: lmt-shared `CabinetPoseReportFile` 读 `frame`（帧版本位）

让导出端能读 report 的 `gauge_strategy`；旧 report（无 `frame`）serde default 归 `fix_root_cabinet`。本 Task 只加字段，不改导出行为。

**Files:**
- Modify: `crates/lmt-shared/src/dto.rs:338-349`（+ 测试）
- Modify: `crates/lmt-shared/src/schema.rs`

- [ ] **Step 1: 写失败测试**（`crates/lmt-shared` 测试模块，dto.rs 末尾或 tests）

```rust
#[test]
fn pose_report_file_frame_defaults_to_fix_root() {
    let old = r#"{"schema_version":"visual_pose_report.v1","cabinet_poses":[]}"#;
    let r: CabinetPoseReportFile = serde_json::from_str(old).unwrap();
    assert!(matches!(r.frame.gauge_strategy, PoseReportGauge::FixRootCabinet));
}

#[test]
fn pose_report_file_reads_align_to_nominal() {
    let s = r#"{"schema_version":"visual_pose_report.v1",
        "frame":{"type":"screen_local","gauge_strategy":"align_to_nominal","units":"mm"},
        "cabinet_poses":[]}"#;
    let r: CabinetPoseReportFile = serde_json::from_str(s).unwrap();
    assert!(matches!(r.frame.gauge_strategy, PoseReportGauge::AlignToNominal));
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p lmt-shared pose_report_file_`
Expected: 编译失败（`PoseReportGauge` / `frame` 未定义）。

- [ ] **Step 3: 实现**（`crates/lmt-shared/src/dto.rs:338-342` 改 + 新增类型）

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PoseReportGauge {
    #[default]
    FixRootCabinet,
    AlignToNominal,
}

/// 读 cabinet_pose_report.json 的 `frame` 帧版本位（其余字段忽略）。
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct PoseReportFrame {
    #[serde(default)]
    pub gauge_strategy: PoseReportGauge,
}

// CabinetPoseReportFile 改为：
pub struct CabinetPoseReportFile {
    pub schema_version: String,
    #[serde(default)]
    pub frame: PoseReportFrame,
    pub cabinet_poses: Vec<CabinetPoseEntry>,
}
```
（确认 `CabinetPoseReportFile` 没有 `#[serde(deny_unknown_fields)]`，否则 report 里 `frame` 的其它字段会反序列化失败。）

- [ ] **Step 4: 注册 schema**（`crates/lmt-shared/src/schema.rs`，与既有 `add!` 同风格）

```rust
add!("PoseReportFrame", PoseReportFrame);
add!("PoseReportGauge", PoseReportGauge);
```
（纯类型，进 complete 列表；若有 `assert present` 测试，补上断言。）

- [ ] **Step 5: 跑测试确认通过**

Run: `cargo test -p lmt-shared`
Expected: 新 2 测试 + schema 测试 passed。

- [ ] **Step 6: schema dump 含新类型**

Run: `cargo build && ./target/debug/lmt --json schema | jq '.PoseReportFrame, .PoseReportGauge'`
Expected: 两者非 null。

- [ ] **Step 7: 全工作区不回归**

Run: `cargo test --workspace`
Expected: all passed（导出行为此刻未变）。

- [ ] **Step 8: 提交**

```bash
git add crates/lmt-shared/src/dto.rs crates/lmt-shared/src/schema.rs
git commit -m "feat(shared): CabinetPoseReportFile reads frame.gauge_strategy (serde default fix_root_cabinet)"
```

---

## Task 3: 导出端按 `gauge_strategy` 分支（不加 flag）

`run_export_pose_obj` 读 `frame.gauge_strategy`：`fix_root_cabinet`→现有行为（**字节不变**）；`align_to_nominal`→跳过 `apply_canonical_frame`，几何视为已在 M0.1，按 target 走 `adapt_to_target` + disguise winding/UV。

**Files:**
- Modify: `crates/lmt-app/src/export.rs`（+ 测试，用合成 report 夹具）

- [ ] **Step 1: 写失败测试**（`crates/lmt-app/src/export.rs` 测试模块）

```rust
// 夹具：构造最小 report JSON 字符串，写临时文件，跑 run_export_pose_obj，读回 OBJ 断言。
#[test]
fn pose_obj_fix_root_unchanged_snapshot() {
    // 用一个已知 fix_root_cabinet（或无 frame）report → 导出 neutral，
    // 与 Task 前的黄金 OBJ 文本逐行相等（把当前输出存为 fixture 作快照基线）。
    let obj = export_to_string(FIXTURE_FIX_ROOT_JSON, "neutral", None, false);
    assert_eq!(obj, include_str!("../tests/fixtures/pose_obj_fix_root_neutral.obj"));
}

#[test]
fn pose_obj_align_to_nominal_disguise_uses_adapt_not_canonical() {
    // align_to_nominal report，corners 已在 M0.1。导出 disguise：
    // 断言某顶点 == adapt_to_target(corner, Disguise) = (x, z, -y)，
    // 且未发生 apply_canonical_frame 的居中/yaw（原点不被搬到质心）。
    let obj = export_to_string(FIXTURE_ALIGN_M01_JSON, "disguise", None, false);
    let v = first_vertex(&obj);
    let c = m01_corner_0(); // 夹具里已知 M0.1 角点
    assert_close(v, [c[0], c[2], -c[1]]);
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p lmt-app pose_obj_align_to_nominal`
Expected: FAIL（align 分支未实现，仍走 canonical）。

- [ ] **Step 3: 实现分支**（`crates/lmt-app/src/export.rs:153-286` `run_export_pose_obj`）

在读出 `report` 后取 `let gauge = report.frame.gauge_strategy;`，分流：
```rust
match gauge {
    PoseReportGauge::AlignToNominal => {
        // 几何已在 M0.1 设计帧：不调 apply_canonical_frame。
        // --root 在此模式无意义 → 若 root.is_some() 返回 InvalidInput。
        // 每顶点按 target：adapt_to_target(v, target_enum)；
        // disguise 时复用 build.rs 第 5 步：三角 winding swap + UV.u = 1-u。
        // --ground 仍可：贴地(min Y=0)。
    }
    PoseReportGauge::FixRootCabinet => {
        // 现有路径完全不变（canonical_disguise / --root / --ground / apply_canonical_frame）。
    }
}
```
注意：align 分支的 disguise 用 `adapt_to_target` 的 `(x,z,-y)`（M0.1→disguise），**不要**用旧 `flipY`（那是给重建帧的）。winding/UV 抽一个内部 fn 与 `fix_root_cabinet` 的 disguise 共用，避免两套。

- [ ] **Step 4: 跑测试确认通过 + 快照基线**

先生成快照基线（首次）：手动确认当前 `fix_root_cabinet` 输出正确后存入 `crates/lmt-app/tests/fixtures/pose_obj_fix_root_neutral.obj`。
Run: `cargo test -p lmt-app pose_obj`
Expected: 2 passed。

- [ ] **Step 5: 全工作区不回归**

Run: `cargo test --workspace`
Expected: all passed（真实 report 仍 fix_root_cabinet，align 分支靠夹具覆盖）。

- [ ] **Step 6: 提交**

```bash
git add crates/lmt-app/src/export.rs crates/lmt-app/tests/fixtures/
git commit -m "feat(app): export pose-obj branches on frame.gauge_strategy; align_to_nominal skips canonical guess"
```

---

## Task 4: 导出端 `--coordinate-system` 重锚 + Folded 守卫

加 `--coordinate-system <PROJECT_YAML>`：在 M0.1 nominal 上由 `coordinate_system` 三网格名定设计帧重锚。Folded 拒绝；`fix_root_cabinet`+flag 拒绝。

**Files:**
- Create: `crates/lmt-app/src/design_frame.rs`（新 helper）
- Modify: `crates/lmt-app/src/export.rs`、`crates/lmt-app/src/lib.rs`（mod 声明）
- Modify: `crates/lmt-cli/src/cli.rs:273-290`、`crates/lmt-cli/src/commands/export.rs:137-213`
- Modify: `crates/lmt-cli/tests/cli_e2e.rs`（pose-obj 块）
- Modify: `docs/agents-cli.md`、`docs/contract-manifest.json`

- [ ] **Step 1: 写失败单测**（`crates/lmt-app/src/design_frame.rs` 测试模块）

```rust
#[test]
fn design_frame_resolves_default_grid_names() {
    // project.yaml: screen MAIN flat 4x3, coordinate_system 默认
    //   origin=MAIN_V001_R001, x_axis=MAIN_V004_R001, xy_plane=MAIN_V001_R002
    // 期望：origin 在 V001_R001 顶点(0,0,0)，X 沿列，Z 沿行向上。
    let f = design_frame_from_grid_names(&cfg, "MAIN").unwrap();
    assert_close(f.origin_world, [0.0, 0.0, 0.0]);
    // X 轴方向 ~ +列
    assert!(f.basis[0][0] > 0.99);
}

#[test]
fn design_frame_rejects_folded() {
    let err = design_frame_from_grid_names(&folded_cfg, "MAIN").unwrap_err();
    assert!(matches!(err, LmtError::InvalidInput(_)));
}

#[test]
fn design_frame_rejects_bad_grid_name() {
    let err = design_frame_from_grid_names(&cfg_bad_origin, "MAIN").unwrap_err();
    assert!(matches!(err, LmtError::InvalidInput(_)));
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p lmt-app design_frame`
Expected: 编译失败（`design_frame_from_grid_names` 未定义）。

- [ ] **Step 3: 实现 helper**（`crates/lmt-app/src/design_frame.rs`）

```rust
// 伪流程（用现有符号）：
// 1. cfg.coordinate_system: {origin_point, x_axis_point, xy_plane_point}（dto）
// 2. screen_id: 用 total_station_mapper 的最长前缀逻辑从 origin_point 推（首版要求三名同前缀）
// 3. let m1 = map_to_adapter(&cfg)?; let screen_cfg = m1.screens.get(screen_id)...
// 4. 若 screen_cfg.shape_prior 是 Folded → return InvalidInput("folded not supported")
// 5. let grid = expected_grid_positions(screen_id, screen_cfg)?;  // M0.1 nominal 顶点 name->pos
//    建 name->Vector3 map
// 6. 三名各：strip 前缀 → 解 V/R(1-based) → 用 "{screen}_V{v}_R{r}" 在 grid map 里查到顶点位置
//    （grid 的 name 就是 1-based 全名，可直接按名查，无需手动减1）
// 7. CoordinateFrame::from_three_points_m01(origin_pos, x_pos, xy_pos)  // Task 1
```
关键：`expected_grid_positions` 产的 `GridExpected.name` 本就是 `{screen}_V{c+1}_R{r+1}` 全名，**直接按 coordinate_system 里的全名查表**即可，省掉手动 1-based↔0-based 换算（对账逻辑仍写进注释）。

- [ ] **Step 4: 跑单测确认通过**

Run: `cargo test -p lmt-app design_frame`
Expected: 3 passed。

- [ ] **Step 5: 接入 run_export_pose_obj + 拒绝组合**（`export.rs`）

- `run_export_pose_obj` / `check_pose_obj_inputs` 加 `coordinate_system: Option<&Path>`。
- `AlignToNominal` 分支：`if let Some(p)=coordinate_system { let f=design_frame_from_grid_names(&load_project_yaml_from_path(p)?, screen)?; 对每角点 f.world_to_model(corner); }` 再 `adapt_to_target`。
- `FixRootCabinet` + `coordinate_system.is_some()` → `Err(InvalidInput("coordinate_system 重锚需要 align_to_nominal report"))`。

- [ ] **Step 6: CLI flag + dry-run 对齐**（`cli.rs` / `commands/export.rs`）

`cli.rs ExportCmd::PoseObj` 加：
```rust
#[arg(long, value_name = "PROJECT_YAML")]
coordinate_system: Option<std::path::PathBuf>,
```
`commands/export.rs::pose_obj`：`coordinate_system` 穿到 dry-run payload（加键）+ `check_pose_obj_inputs` + `run_export_pose_obj`（dry-run 与 execute 走同一预检）。

- [ ] **Step 7: E2E**（`crates/lmt-cli/tests/cli_e2e.rs` pose-obj 块）

加 case：
1. happy：align report + `--coordinate-system proj.yaml --yes` → envelope ok，OBJ 写出。
2. refuse：无 `--yes` → 拒（gate）。
3. dry-run：`--dry-run` payload 含 `coordinate_system` + `would_write`。
4. error：① 坏 yaml → not_found/invalid_input；② 网格名解析不到顶点 → invalid_input；③ Folded screen → invalid_input；④ `fix_root_cabinet` report + flag → invalid_input。

- [ ] **Step 8: 跑通**

Run: `cargo test -p lmt-app -p lmt-cli && ./target/debug/lmt export pose-obj --help`
Expected: 测试 passed；help 显示 `--coordinate-system`。

- [ ] **Step 9: 文档**

`docs/agents-cli.md` pose-obj 行加 `--coordinate-system` 说明；`docs/contract-manifest.json` export.pose_obj 参数同步。

- [ ] **Step 10: 提交**

```bash
git add crates/lmt-app/src/design_frame.rs crates/lmt-app/src/export.rs crates/lmt-app/src/lib.rs \
        crates/lmt-cli/src/cli.rs crates/lmt-cli/src/commands/export.rs crates/lmt-cli/tests/cli_e2e.rs \
        docs/agents-cli.md docs/contract-manifest.json
git commit -m "feat(cli): export pose-obj --coordinate-system re-anchors to project design frame; folded guard"
```

---

## Task 5: sidecar `nominal_cabinet_corners_m01` + 跨语言 golden

Python 出 M0.1 约定的 nominal **角点**（Procrustes 目标），用 golden 夹具与 Rust `expected_grid_positions` 锁一致。

**Files:**
- Modify: `python-sidecar/src/lmt_vba_sidecar/nominal.py`
- Create: `python-sidecar/tests/fixtures/nominal_m01_flat.json`、`..._curved.json`（顶点真值）
- Create: `python-sidecar/tests/test_nominal_m01.py`
- Modify/Create: `crates/adapter-total-station` 一致性测试（读同一夹具）

- [ ] **Step 1: 算夹具**

选 flat 4×3（cabinet 500×500mm）+ curved（同尺寸 + radius）两 screen。用 Rust `expected_grid_positions` 跑一次，把 `(name → [x,y,z] mm)` 顶点表导出存入 `tests/fixtures/nominal_m01_<case>.json`，人工核对原点在 V001_R001=(0,0,0)、X=列、Z=行、Y=法向。

- [ ] **Step 2: 写失败 Python 测试**（`python-sidecar/tests/test_nominal_m01.py`）

```python
import json, numpy as np
from lmt_vba_sidecar.nominal import nominal_cabinet_corners_m01
from lmt_vba_sidecar.ipc import CabinetArray

def _verts_from_corners(corners):  # {(c,r):(4,3)} -> {(vc,vr):xyz}, 取每箱体 BL + 边缘外推
    ...

def test_corners_match_rust_golden_flat():
    gold = json.load(open("tests/fixtures/nominal_m01_flat.json"))
    cab = CabinetArray(cols=4, rows=3, cabinet_size_mm=[500,500], absent_cells=[])
    verts = _verts_from_corners(nominal_cabinet_corners_m01(cab, "flat"))
    for name, xyz in gold.items():
        assert np.allclose(verts[name], xyz, atol=1e-6)

def test_folded_raises():
    cab = CabinetArray(cols=4, rows=3, cabinet_size_mm=[500,500], absent_cells=[])
    import pytest
    with pytest.raises(ValueError):
        nominal_cabinet_corners_m01(cab, {"folded": {...}})
```
（curved 同理一条。）

- [ ] **Step 3: 跑测试确认失败**

Run: `cd python-sidecar && .venv/bin/pytest tests/test_nominal_m01.py -q`
Expected: ImportError / 未定义。

- [ ] **Step 4: 实现**（`python-sidecar/src/lmt_vba_sidecar/nominal.py`）

```python
def nominal_cabinet_corners_m01(cab, shape_prior):
    """{(col,row): (4,3) corners [BL,BR,TR,TL]} in M0.1 约定:
    X=cols, Y=outward-normal, Z=rows-up, mm, 原点=V001_R001 左下顶点。
    公式镜像 Rust shape_grid.rs expected_grid_positions 的 Flat/Curved 顶点,
    每箱体取其 4 个网格顶点。folded -> raise ValueError(沿用 _cabinet_center_model_m 守卫)。"""
    # flat: 顶点 (c,r) = (c*cw, 0, r*ch)；BL=(c,r),BR=(c+1,r),TR=(c+1,r+1),TL=(c,r+1)
    # curved: 镜像 shape_grid.rs:49-80（half-cylinder, anchor 减 V001_R001 raw）
```
**不动** `nominal_cabinet_centers_model_frame` / `_normals`（BA 种子仍用重建约定）。

- [ ] **Step 5: 跑测试确认通过**

Run: `cd python-sidecar && .venv/bin/pytest tests/test_nominal_m01.py -q`
Expected: passed。

- [ ] **Step 6: Rust 侧读同一夹具一致性测试**（`crates/adapter-total-station` tests）

```rust
#[test]
fn expected_grid_matches_golden_flat() {
    let gold: HashMap<String,[f64;3]> = serde_json::from_str(include_str!(".../nominal_m01_flat.json")).unwrap();
    let g = expected_grid_positions("MAIN", &flat_cfg()).unwrap();
    for ge in g { let e = gold[&ge.name]; assert!((ge.model_position - Vector3::from(e)*0.001).norm() < 1e-9); }
}
```
（注意单位：Rust 是 m，夹具是 mm，换算清楚。）

- [ ] **Step 7: 跑通**

Run: `cargo test -p lmt-adapter-total-station expected_grid_matches_golden`
Expected: passed。

- [ ] **Step 8: 提交**

```bash
git add python-sidecar/src/lmt_vba_sidecar/nominal.py python-sidecar/tests/ crates/adapter-total-station/
git commit -m "feat(sidecar): nominal_cabinet_corners_m01 + cross-language golden vs expected_grid_positions"
```

---

## Task 6: sidecar `solve_and_emit` 显式 `gauge_strategy` + align_to_nominal

`solve_and_emit` 加显式 `gauge_strategy`；SL 传 `align_to_nominal`（P + procrustes → M0.1 report），charuco 传 `fix_root_cabinet`（逐位不变）。消费端已就绪，端到端打通。

**Files:**
- Modify: `python-sidecar/src/lmt_vba_sidecar/reconstruct.py`（`solve_and_emit`）
- Modify: `python-sidecar/src/lmt_vba_sidecar/sl_reconstruct.py:195`（SL 调用传参）+ charuco 调用方传 `fix_root_cabinet`
- Create: `python-sidecar/tests/test_align_to_nominal.py`
- Modify: `crates/lmt-shared/src/dto.rs:242-253`（`VisualReconstructResult` 加字段）、`crates/lmt-app/src/visual.rs`

- [ ] **Step 1: 写 P 单测（钉死定号）**（`python-sidecar/tests/test_align_to_nominal.py`）

```python
import numpy as np
from lmt_vba_sidecar.reconstruct import recon_to_m01_permutation as P_of  # 返回 3x3

def test_P_maps_recon_axes_to_m01():
    P = P_of()
    assert np.allclose(P @ np.array([0,0,1.0]), [0,1,0])  # recon 外法向(+Z) -> M0.1 +Y
    assert np.allclose(P @ np.array([0,1,0.0]), [0,0,1])  # recon 行向上(+Y) -> M0.1 +Z
    assert np.isclose(np.linalg.det(P), 1.0)
```

- [ ] **Step 2: 写 align/charuco 行为测试**

```python
def test_charuco_unchanged_snapshot(tmp_path):
    # gauge_strategy="fix_root_cabinet" 跑一遍 → report+measured 与基线快照逐位一致
    ...

def test_sl_align_to_nominal_lands_m01(tmp_path):
    # gauge_strategy="align_to_nominal" → report.frame.gauge_strategy=="align_to_nominal"
    # 一块已知箱体 corners 在 M0.1（法向沿 +Y），procrustes_align_rms_m > 0 且 < 阈值
    ...

def test_align_too_few_cabinets_raises():
    # <3 箱体 → ProcrustesFailed（fatal error event / ValueError）
    ...
```

- [ ] **Step 3: 跑测试确认失败**

Run: `cd python-sidecar && .venv/bin/pytest tests/test_align_to_nominal.py -q`
Expected: 失败（`recon_to_m01_permutation`/参数未实现）。

- [ ] **Step 4: 实现**（`python-sidecar/src/lmt_vba_sidecar/reconstruct.py`）

```python
def recon_to_m01_permutation():
    # 固定有号置换 P，使 m01 = P @ recon；由 B_m01=B_recon·Π([b0,b2,-b1]) 得 P=Π.T。
    # 候选 np.array([[1,0,0],[0,0,1],[0,-1,0]])；以 Step1 单测为准（不符就改号）。
    return np.array([[1,0,0],[0,0,1],[0,-1,0]], float)

def solve_and_emit(..., gauge_strategy="fix_root_cabinet"):
    ...
    if gauge_strategy == "align_to_nominal":
        P = recon_to_m01_permutation()
        nominal = nominal_cabinet_corners_m01(cabinet_array, shape_prior)  # Task5
        # 收集所有箱体 world corners(重建帧) → P@corners → src(4N,3)
        # dst = 对应 nominal 角点(4N,3)，顺序与 src 对齐
        R_a, t_a, rms = procrustes_rigid(src_m01, dst)   # procrustes.py
        # 对每箱体: corners' = (R_a @ (P @ corner)) + t_a
        #           center' = R_a @ (P @ center) + t_a
        #           normal' = R_a @ (P @ normal)
        #           rotmat' = R_a @ P @ rotmat
        frame_spec = FrameSpec(type="screen_local", gauge_strategy="align_to_nominal", ...)
        result_frame_rms = rms
        measured_coord_frame = identity   # 点已在 M0.1 世界系（见开放项）
    else:
        # 现状不变
        frame_spec = FrameSpec(root_cabinet=list(ROOT_CABINET))  # fix_root_cabinet
        result_frame_rms = 0.0
    # 用 frame_spec 写 CabinetPoseReport(:636-642)；ResultData.procrustes_align_rms_m = result_frame_rms
```
变换必须在构建 `cabinet_poses`/`measured_points`（:574-633）**之前**，使 report 与 measured.yaml 都落 M0.1。

- [ ] **Step 5: 调用方传参**

- `sl_reconstruct.py:195`：`solve_and_emit(..., gauge_strategy="align_to_nominal")`。
- charuco 调用方（`visual reconstruct` 路径调 `solve_and_emit` 处）：显式 `gauge_strategy="fix_root_cabinet"`。

- [ ] **Step 6: 跑 sidecar 测试**

Run: `cd python-sidecar && .venv/bin/pytest tests/test_align_to_nominal.py tests/test_nominal_m01.py -q`
Expected: passed（含 charuco 快照不变）。

- [ ] **Step 7: VisualReconstructResult 加 procrustes_align_rms_m**（Rust）

`dto.rs:242-253` 加 `pub procrustes_align_rms_m: f64,`（保 serde+JsonSchema）；`visual.rs` 组装时从 sidecar `ResultData.procrustes_align_rms_m` 填入。
Run: `cargo test -p lmt-shared -p lmt-app && ./target/debug/lmt --json schema | jq '.VisualReconstructResult.properties.procrustes_align_rms_m'`
Expected: 测试过；字段在 schema。

- [ ] **Step 8: 端到端冒烟**

跑一次真实 SL reconstruct（小数据集），确认输出 report `frame.gauge_strategy=="align_to_nominal"` 且 `export pose-obj --coordinate-system` 能消费。

- [ ] **Step 9: 提交**

```bash
git add python-sidecar/ crates/lmt-shared/src/dto.rs crates/lmt-app/src/visual.rs
git commit -m "feat(sidecar): solve_and_emit gauge_strategy; SL align_to_nominal (P+procrustes); charuco unchanged"
```

---

## Task 7: 集成、法向/winding golden、文档、自检

**Files:**
- Create: 集成/golden 测试（`crates/lmt-app` 或 `crates/lmt-cli` 端到端，或 sidecar+CLI 串联脚本）
- Modify: `docs/agents-cli.md`、`docs/contract-manifest.json`、`crates/lmt-shared/src/manifest.rs:118`

- [ ] **Step 1: 法向/winding golden（F1）**

flat + curved 各一面**已知位姿**合成墙 → align_to_nominal → `export pose-obj --coordinate-system disguise` → 断言导出 OBJ 的**面法向方向 + 三角 winding** 正确（不只比顶点）。专抓平面共面翻面二义。
Run: `cargo test -p lmt-cli pose_obj_normal_winding_golden`（或对应位置）
Expected: passed。

- [ ] **Step 2: 集成误差测试（验 √N 优于裸 3 点）**

合成已知墙 + 加噪 → 全链路 → 与已知设计模型逐点 3D 误差 < 阈值；并与"裸 3 点定帧"误差对比，断言全局拟合更优。

- [ ] **Step 3: 帧版本兼容（F2）回归**

旧 `fix_root_cabinet` report 无 flag 导出字节对照（Task 3 快照）+ 新 `align_to_nominal` report 带/不带 flag。
Run: `cargo test -p lmt-app pose_obj`
Expected: passed。

- [ ] **Step 4: 文档收口**

`docs/agents-cli.md`：pose-obj `--coordinate-system`、SL `align_to_nominal` 默认 + `procrustes_align_rms_m`、错误码表（`procrustes_failed` 已在）；`docs/contract-manifest.json` 同步；`manifest.rs:118` 若有 pose-obj 描述则更。

- [ ] **Step 5: 合并前自检（全过才算完成）**

```bash
cargo test --workspace
cd python-sidecar && .venv/bin/pytest && cd ..
cargo build && ./target/debug/lmt --json schema | jq '.PoseReportFrame, .VisualReconstructResult'
./target/debug/lmt export pose-obj --help     # --coordinate-system 注册
```
Expected: 全绿；schema 含新类型/字段；help 含新 flag。

- [ ] **Step 6: 验收对账（known-good）**

与同一面墙的全站仪 `export obj` diff，或与设计模型 diff，确认 drop-in 对位、误差远小于 5cm/顶端（memory: verify against known-good）。

- [ ] **Step 7: 提交**

```bash
git add docs/ crates/
git commit -m "test(sl): integration + normal/winding golden; docs/contract for coordinate_system frame"
```

---

## 整体验收标准

合成已知墙端到端 `SL → align_to_nominal → export pose-obj --coordinate-system → OBJ`，与设计模型逐点 3D 误差远小于旧猜测路径（~5cm/顶端）量级，法向/winding 正确，可在 disguise 里 drop-in 对位源模型。

## 开放项（实施时定，spec §11）

- `--coordinate-system` 多 screen 屏幕识别：默认从网格名前缀推；歧义时加 `--screen`。
- `MeasuredPoints.coordinate_frame` 在 align_to_nominal 下取 identity（暂定）vs M0.1 帧——与 measured.yaml 消费方对齐。
- charuco 迁移 align_to_nominal：本任务不做（显式参数隔离 + 锁死测试），独立 follow-up。
- visual measured.yaml 命名 `MAIN_V{0-based}` vs 全站仪 `{screen}_V{1-based}` 统一：记录，不阻塞。
- Folded 真实 nominal：follow-up（首版导出端 fail-fast 拒绝）。
