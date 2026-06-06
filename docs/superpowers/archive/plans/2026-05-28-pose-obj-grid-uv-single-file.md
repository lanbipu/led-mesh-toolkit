# Path B 导出改造（整体网格 UV + 单文件）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 `lmt export pose-obj` 产出 xR/VP 可用的单个 OBJ：每箱体独立面片（不焊接）+ 一张整体 0-1 网格 UV（每块占自己格子）。

**Architecture:** 改造保持每箱体 1×1 surface 的现有几何管线（`panel_surface` → `surface_to_mesh_output(Neutral, weld=0)`），只把每块的 UV 从满幅 `[0,1]` 改成它在整屏网格里的格子；再把 N 块的 `MeshOutput` 拼成一个（顶点不去重、三角面索引偏移）写一次文件。列/行从 `cabinet_id`（`V<col>_R<row>`）解析，总列/行从全体 id 反推。这是跨 crate 的 breaking change（`--out-dir`→`--out`、`ExportPoseObjResult.files`→`file`），不留兼容 shim，所有调用点一次性同步改。

**Tech Stack:** Rust workspace（`lmt-core` / `lmt-app` / `lmt-cli` / `lmt-shared`），nalgebra，clap，assert_cmd（E2E）。

**Spec:** `docs/superpowers/specs/2026-05-28-pose-obj-grid-uv-single-file-design.md`

---

## File Structure

改动集中在 4 个 crate，全部 in-repo lockstep（已 grep 核实无 Tauri/前端调用）：

- `crates/lmt-app/src/export.rs` — 新增 `parse_cabinet_col_row` / `infer_grid_dims` / `merge_mesh_outputs` 三个纯函数；改 `panel_surface`（接 col/row/cols/rows，出格子 UV）；重写 `run_export_pose_obj`（单文件 + 合并）；`check_pose_obj_inputs` 加 id 可解析校验；更新本文件单测。
- `crates/lmt-shared/src/dto.rs` — `ExportPoseObjResult.files: Vec<String>` → `file: String`。
- `crates/lmt-shared/src/manifest.rs` — 命令字符串 `--out-dir <dir>` → `--out <path>`。
- `crates/lmt-cli/src/cli.rs` — `PoseObj` 子命令 `out_dir`（`--out-dir DIR`）→ `out`（`--out PATH`）。
- `crates/lmt-cli/src/commands/export.rs` — `pose_obj()` 改单文件路径、dry-run 预览文案、成功文案。
- `crates/lmt-cli/tests/cli_e2e.rs` — 5 个 pose-obj 用例改 `--out` + 单文件断言。
- `docs/agents-cli.md` + `docs/contract-manifest.json` — 命令表 + 快照同步。

每箱体仍是独立 4 顶点（不焊接），UV 与几何解耦——两者同时成立。

---

## Task 1: `cabinet_id` 解析 + 网格维度反推（纯函数）

新增两个纯函数，先写测试。它们暂时无调用方（Task 3 才接入），会有 dead_code 警告——**无害**，本仓不启用 `-D warnings`，`cargo test` 仍通过。

**Files:**
- Modify: `crates/lmt-app/src/export.rs`（在文件末尾 `#[cfg(test)] mod tests` **之前**加函数；测试加进现有 tests mod）

- [ ] **Step 1: 写失败测试**（加到 `crates/lmt-app/src/export.rs` 的 `mod tests` 内，紧跟现有测试之后、`}` 闭合之前）

```rust
    #[test]
    fn parse_cabinet_col_row_extracts_indices() {
        assert_eq!(parse_cabinet_col_row("V000_R000"), Some((0, 0)));
        assert_eq!(parse_cabinet_col_row("V012_R007"), Some((12, 7)));
        assert_eq!(parse_cabinet_col_row("V120_R024"), Some((120, 24)));
        // 不匹配 → None
        assert_eq!(parse_cabinet_col_row("../escape"), None);
        assert_eq!(parse_cabinet_col_row("a/b"), None);
        assert_eq!(parse_cabinet_col_row(""), None);
        assert_eq!(parse_cabinet_col_row("V000"), None);
    }

    #[test]
    fn infer_grid_dims_takes_max_plus_one() {
        // 1 列 × 2 行
        let ids = ["V000_R000", "V000_R001"];
        assert_eq!(infer_grid_dims(&ids).unwrap(), (1, 2));
        // 3 列 × 2 行
        let ids = ["V000_R000", "V002_R000", "V001_R001"];
        assert_eq!(infer_grid_dims(&ids).unwrap(), (3, 2));
        // 含不可解析 id → InvalidInput
        let ids = ["V000_R000", "bad"];
        assert!(matches!(infer_grid_dims(&ids), Err(LmtError::InvalidInput(_))));
    }
```

- [ ] **Step 2: 运行测试，确认编译失败（函数未定义）**

Run: `cargo test -p lmt-app parse_cabinet_col_row infer_grid_dims 2>&1 | tail -20`
Expected: 编译错误 `cannot find function parse_cabinet_col_row` / `infer_grid_dims`。

- [ ] **Step 3: 实现两个纯函数**（加到 `crates/lmt-app/src/export.rs`，放在 `fn panel_surface` 之前）

```rust
/// 从 cabinet_id 解析末尾的 `V<col>_R<row>`（如 "V012_R007" → (12,7)）。
/// 容忍前缀（"MAIN_V012_R007" 也可）。不匹配返回 None。
fn parse_cabinet_col_row(cabinet_id: &str) -> Option<(u32, u32)> {
    let (head, row_str) = cabinet_id.rsplit_once("_R")?;
    let (_, col_str) = head.rsplit_once('V')?;
    Some((col_str.parse().ok()?, row_str.parse().ok()?))
}

/// 总列/行数 = max(col)+1 / max(row)+1。任一 id 不可解析 → InvalidInput。
fn infer_grid_dims(ids: &[&str]) -> LmtResult<(u32, u32)> {
    let mut max_col = 0u32;
    let mut max_row = 0u32;
    for id in ids {
        let (c, r) = parse_cabinet_col_row(id).ok_or_else(|| {
            LmtError::InvalidInput(format!("cabinet_id {id:?} not parseable as V<col>_R<row>"))
        })?;
        max_col = max_col.max(c);
        max_row = max_row.max(r);
    }
    Ok((max_col + 1, max_row + 1))
}
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test -p lmt-app parse_cabinet_col_row infer_grid_dims 2>&1 | tail -20`
Expected: 2 passed。（可能有 dead_code 警告，正常。）

- [ ] **Step 5: 提交**

```bash
git add crates/lmt-app/src/export.rs
git commit -m "feat(export): add cabinet_id parse + grid-dim inference helpers"
```

---

## Task 2: `merge_mesh_outputs`（拼合多块为一个网格）

**Files:**
- Modify: `crates/lmt-app/src/export.rs`（函数 + tests mod 内的测试）

- [ ] **Step 1: 写失败测试**（加到 `mod tests` 内）

```rust
    #[test]
    fn merge_mesh_outputs_concatenates_and_offsets_indices() {
        use lmt_core::surface::MeshOutput;
        let mk = |x: f64| MeshOutput {
            target: TargetSoftware::Neutral,
            vertices: vec![
                Vector3::new(x, 0.0, 0.0),
                Vector3::new(x + 1.0, 0.0, 0.0),
                Vector3::new(x, 1.0, 0.0),
                Vector3::new(x + 1.0, 1.0, 0.0),
            ],
            triangles: vec![[0, 1, 3], [0, 3, 2]],
            uv_coords: vec![
                nalgebra::Vector2::new(0.0, 0.0),
                nalgebra::Vector2::new(1.0, 0.0),
                nalgebra::Vector2::new(0.0, 1.0),
                nalgebra::Vector2::new(1.0, 1.0),
            ],
        };
        let merged = merge_mesh_outputs(TargetSoftware::Neutral, &[mk(0.0), mk(10.0)]);
        assert_eq!(merged.vertices.len(), 8);
        assert_eq!(merged.uv_coords.len(), 8);
        assert_eq!(merged.triangles.len(), 4);
        // 第二块的三角面索引整体 +4
        assert_eq!(merged.triangles[2], [4, 5, 7]);
        assert_eq!(merged.triangles[3], [4, 7, 6]);
        // 顶点保留各自坐标（不焊接）
        assert_eq!(merged.vertices[4].x, 10.0);
    }
```

- [ ] **Step 2: 运行测试，确认编译失败**

Run: `cargo test -p lmt-app merge_mesh_outputs 2>&1 | tail -20`
Expected: `cannot find function merge_mesh_outputs`。

- [ ] **Step 3: 实现**（加到 `crates/lmt-app/src/export.rs`，放在 `panel_surface` 之前；并确保文件顶部 `use` 含 `MeshOutput`）

先把第 4 行的 import 改为带上 `MeshOutput`：

```rust
use lmt_core::surface::{GridTopology, MeshOutput, QualityMetrics, ReconstructedSurface, TargetSoftware};
```

再加函数：

```rust
/// 把每块 cabinet 的 MeshOutput 拼成一个（顶点不去重=不焊接，三角面索引按累计偏移）。
fn merge_mesh_outputs(target: TargetSoftware, meshes: &[MeshOutput]) -> MeshOutput {
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    let mut uv_coords = Vec::new();
    for m in meshes {
        let offset = vertices.len() as u32;
        vertices.extend_from_slice(&m.vertices);
        uv_coords.extend_from_slice(&m.uv_coords);
        for t in &m.triangles {
            triangles.push([t[0] + offset, t[1] + offset, t[2] + offset]);
        }
    }
    MeshOutput { target, vertices, triangles, uv_coords }
}
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test -p lmt-app merge_mesh_outputs 2>&1 | tail -20`
Expected: 1 passed。

- [ ] **Step 5: 提交**

```bash
git add crates/lmt-app/src/export.rs
git commit -m "feat(export): add merge_mesh_outputs (concat panels, no welding)"
```

---

## Task 3: Cutover — 单文件 + 网格 UV（跨 crate 同步）

这是原子改动：`panel_surface` 签名、`run_export_pose_obj` 签名、DTO、manifest、CLI、E2E 必须一起改才能编译通过。按下面步骤改完后 `cargo test --workspace` 必须全绿再提交。

**Files:**
- Modify: `crates/lmt-shared/src/dto.rs:331-337`
- Modify: `crates/lmt-shared/src/manifest.rs:118-119`
- Modify: `crates/lmt-app/src/export.rs`（`panel_surface` / `run_export_pose_obj` / `check_pose_obj_inputs` + 4 个旧单测）
- Modify: `crates/lmt-cli/src/cli.rs:259-278`
- Modify: `crates/lmt-cli/src/commands/export.rs:17-24, 138-214`
- Modify: `crates/lmt-cli/tests/cli_e2e.rs:1601-1764`

- [ ] **Step 1: 改 DTO**（`crates/lmt-shared/src/dto.rs`）

把：

```rust
/// `lmt export pose-obj` 结果：每块屏一个 OBJ。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExportPoseObjResult {
    pub target: String,
    pub cabinet_count: usize,
    pub files: Vec<String>,
}
```

改为：

```rust
/// `lmt export pose-obj` 结果：所有箱体合并为一个 OBJ。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExportPoseObjResult {
    pub target: String,
    pub cabinet_count: usize,
    pub file: String,
}
```

- [ ] **Step 2: 改 manifest 命令字符串**（`crates/lmt-shared/src/manifest.rs:118-119`）

把 `op("export.pose_obj", ...)` 那条的描述与用法字符串改为：

```rust
        op("export.pose_obj", "Merge a cabinet_pose_report.json into one world-frame OBJ: per-cabinet unwelded panels + one integral 0-1 grid UV (--root re-bases on a cabinet, --ground sits bottom edge at 0)",
           "lmt export pose-obj <pose_report> <target> --out <path> [--root <cabinet_id>] [--ground]", Destructive, true, false, false, Some("ExportPoseObjResult"), &[0, 2, 3, 4, 6]),
```

- [ ] **Step 3: 改 `panel_surface`（接 col/row/cols/rows，出格子 UV）**（`crates/lmt-app/src/export.rs:317`）

把现有 `fn panel_surface(cabinet_id, corners_mm)` 整体替换为：

```rust
/// 一块 cabinet 的 4 个世界系角点（mm，BL,BR,TR,TL）→ 1×1 ReconstructedSurface（米，原样）。
/// 顶点行主序 [(0,0),(1,0),(0,1),(1,1)]=[BL,BR,TL,TR]，故把 [BL,BR,TR,TL] 重排为索引 0,1,3,2。
/// UV：把 1×1 单位 UV 重映射到本块在整屏 (cols×rows) 网格里的格子。
fn panel_surface(
    cabinet_id: &str,
    corners_mm: &[[f64; 3]; 4],
    col: u32,
    row: u32,
    cols: u32,
    rows: u32,
) -> ReconstructedSurface {
    let m = |i: usize| {
        Vector3::new(
            corners_mm[i][0] / 1000.0,
            corners_mm[i][1] / 1000.0,
            corners_mm[i][2] / 1000.0,
        )
    };
    let topology = GridTopology { cols: 1, rows: 1 };
    let uv_coords = compute_grid_uv(topology)
        .into_iter()
        .map(|uv| {
            nalgebra::Vector2::new(
                (col as f64 + uv.x) / cols as f64,
                (row as f64 + uv.y) / rows as f64,
            )
        })
        .collect();
    ReconstructedSurface {
        screen_id: cabinet_id.to_string(),
        uv_coords,
        vertices: vec![m(0), m(1), m(3), m(2)],
        topology,
        quality_metrics: QualityMetrics {
            method: "pose_report_quad".into(),
            measured_count: 4,
            expected_count: 4,
            ..Default::default()
        },
        scatter_fit: None,
    }
}
```

- [ ] **Step 4: 重写 `run_export_pose_obj`（单文件 + 合并）**（`crates/lmt-app/src/export.rs:166-259`）

把现有 `pub fn run_export_pose_obj(... out_dir: &Path ...)` 整个函数体（含文档注释里 `out_dir` / 每屏一文件的描述）替换为：

```rust
/// Merge a `cabinet_pose_report.json` into ONE world-frame OBJ.
///
/// 几何：每块 cabinet 一块独立 quad（4 顶点，不焊接），世界坐标烘进顶点。
/// UV：一张整体 0-1 网格，每块占它在 (cols×rows) 网格里的格子（cols/rows 由 cabinet_id 反推）。
/// 几何按 `TargetSoftware::Neutral` 原样输出（pose report 已是 +Y up / +Z outward = disguise 约定，
/// 不套 core→target 适配器）。`target` 字符串校验+记录但不改轴。
///
/// `--root`：以该 cabinet 局部系为世界系（它轴对齐落原点），其余块保持真实相对位姿。
/// `--ground`：底边贴地（基准=root 块，未给 root 时=整体）。
pub fn run_export_pose_obj(
    pose_report_path: &Path,
    target: &str,
    out_file: &Path,
    root: Option<&str>,
    ground: bool,
) -> LmtResult<ExportPoseObjResult> {
    let _ = parse_target(target)?; // 校验 target；几何原样（Neutral）
    let report: CabinetPoseReportFile =
        serde_json::from_slice(&std::fs::read(pose_report_path)?)?;
    if report.cabinet_poses.is_empty() {
        return Err(LmtError::InvalidInput(
            "pose report has no cabinet_poses".into(),
        ));
    }

    // 网格维度（同时校验每个 cabinet_id 可解析）。
    let ids: Vec<&str> = report
        .cabinet_poses
        .iter()
        .map(|c| c.cabinet_id.as_str())
        .collect();
    let (cols, rows) = infer_grid_dims(&ids)?;

    // 可选 re-root：从基准 cabinet 推 world→local 变换。
    let frame = match root {
        None => None,
        Some(rid) => {
            let rc = report
                .cabinet_poses
                .iter()
                .find(|c| c.cabinet_id == rid)
                .ok_or_else(|| {
                    LmtError::NotFound(format!("--root cabinet '{rid}' not in pose report"))
                })?;
            Some(CabinetFrame::from_corners(&rc.corners_mm).ok_or_else(|| {
                LmtError::InvalidInput(format!(
                    "--root cabinet '{rid}' has degenerate corners (zero-area or collinear)"
                ))
            })?)
        }
    };

    // 每块：(id, col, row, 角点[已应用 re-root])。
    let mut panels: Vec<(String, u32, u32, [[f64; 3]; 4])> =
        Vec::with_capacity(report.cabinet_poses.len());
    for cab in &report.cabinet_poses {
        let (col, row) = parse_cabinet_col_row(&cab.cabinet_id).ok_or_else(|| {
            LmtError::InvalidInput(format!(
                "cabinet_id {:?} not parseable as V<col>_R<row>",
                cab.cabinet_id
            ))
        })?;
        let mut cs = cab.corners_mm;
        if let Some(f) = &frame {
            for c in cs.iter_mut() {
                *c = f.world_to_local(c);
            }
        }
        panels.push((cab.cabinet_id.clone(), col, row, cs));
    }

    // 可选 ground：Y 平移使底边到 0（基准=root 块，否则整体）。
    if ground {
        let min_y = panels
            .iter()
            .filter(|(id, _, _, _)| root.map_or(true, |r| id == r))
            .flat_map(|(_, _, _, cs)| cs.iter().map(|c| c[1]))
            .fold(f64::INFINITY, f64::min);
        if min_y.is_finite() {
            for (_, _, _, cs) in panels.iter_mut() {
                for c in cs.iter_mut() {
                    c[1] -= min_y;
                }
            }
        }
    }

    // 每块 → 1×1 surface（格子 UV）→ MeshOutput（Neutral 原样，weld 0）；再合并。
    let unit_array = CabinetArray::rectangle(1, 1, [1.0, 1.0]);
    let mut meshes = Vec::with_capacity(panels.len());
    for (cid, col, row, cs) in &panels {
        let surface = panel_surface(cid, cs, *col, *row, cols, rows);
        meshes.push(surface_to_mesh_output(
            &surface,
            &unit_array,
            TargetSoftware::Neutral,
            0.0,
        )?);
    }
    let combined = merge_mesh_outputs(TargetSoftware::Neutral, &meshes);

    let out = ensure_obj_extension(out_file);
    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    write_obj(&combined, &out)?;

    Ok(ExportPoseObjResult {
        target: target.to_string(),
        cabinet_count: panels.len(),
        file: out.display().to_string(),
    })
}
```

- [ ] **Step 5: `check_pose_obj_inputs` 加 id 可解析校验**（`crates/lmt-app/src/export.rs:264`）

在现有 `if report.cabinet_poses.is_empty() { ... }` 之后、`if let Some(rid) = root` 之前插入：

```rust
    // dry-run 与 execute 对齐：不可解析的 cabinet_id 现在就拒。
    let ids: Vec<&str> = report
        .cabinet_poses
        .iter()
        .map(|c| c.cabinet_id.as_str())
        .collect();
    infer_grid_dims(&ids)?;
```

- [ ] **Step 6: 更新 lmt-app 4 个旧单测为单文件 + 网格 UV**（`crates/lmt-app/src/export.rs` tests mod，行约 405-524）

把 `export_pose_obj_writes_one_world_frame_obj_per_cabinet` 替换为：

```rust
    #[test]
    fn export_pose_obj_writes_single_merged_obj_with_grid_uv() {
        let dir = tempdir().unwrap();
        let rp = dir.path().join("BENCH_cabinet_pose_report.json");
        std::fs::write(&rp, BENCH_REPORT).unwrap();
        let out = dir.path().join("wall.obj");

        let res = run_export_pose_obj(&rp, "neutral", &out, None, false).unwrap();
        assert_eq!(res.cabinet_count, 2);
        assert!(out.is_file());

        let text = std::fs::read_to_string(&out).unwrap();
        // 2 块 × 4 顶点 = 8 个 v；2 块 × 2 三角 = 4 个 f
        assert_eq!(text.lines().filter(|l| l.starts_with("v ")).count(), 8);
        assert_eq!(text.lines().filter(|l| l.starts_with("f ")).count(), 4);
        // neutral = 原样世界坐标（米）→ V000_R000 的 BL (-0.3,-0.17,0)
        assert!(text.contains("v -0.3 -0.17 0"), "got:\n{text}");

        // UV 是整体网格：BENCH 两块=V000_R000/V000_R001 → cols=1,rows=2
        // 不同 U 值 = cols+1 = 2；不同 V 值 = rows+1 = 3
        let us: std::collections::BTreeSet<String> = text
            .lines()
            .filter_map(|l| l.strip_prefix("vt "))
            .map(|l| l.split_whitespace().next().unwrap().to_string())
            .collect();
        let vs: std::collections::BTreeSet<String> = text
            .lines()
            .filter_map(|l| l.strip_prefix("vt "))
            .map(|l| l.split_whitespace().nth(1).unwrap().to_string())
            .collect();
        assert_eq!(us.len(), 2, "distinct U should be cols+1=2: {us:?}");
        assert_eq!(vs.len(), 3, "distinct V should be rows+1=3: {vs:?}");
    }
```

把 `export_pose_obj_disguise_target_equals_raw_world_frame` 中的输出读取改为单文件：

```rust
    #[test]
    fn export_pose_obj_disguise_target_equals_raw_world_frame() {
        let dir = tempdir().unwrap();
        let rp = dir.path().join("BENCH_cabinet_pose_report.json");
        std::fs::write(&rp, BENCH_REPORT).unwrap();
        let out = dir.path().join("wall_disguise.obj");

        let res = run_export_pose_obj(&rp, "disguise", &out, None, false).unwrap();
        assert_eq!(res.target, "disguise");
        assert_eq!(res.cabinet_count, 2);

        let text = std::fs::read_to_string(&out).unwrap();
        assert!(
            text.contains("v -0.3 -0.17 0"),
            "disguise output should equal raw world frame; got:\n{text}"
        );
        assert!(
            !text.contains("v -0.3 0 0.17"),
            "axis-swapped vertex found — adapter was wrongly applied; got:\n{text}"
        );
    }
```

把 `export_pose_obj_rejects_unsafe_cabinet_id` 替换为按"不可解析"拒（单文件下无路径穿越风险）：

```rust
    #[test]
    fn export_pose_obj_rejects_unparseable_cabinet_id() {
        // 单文件下 cabinet_id 不再是文件名；不可解析为 V<col>_R<row> 的 id → InvalidInput。
        for bad_id in &["../escape", "a/b", "", "V000", "RandomName"] {
            let dir = tempdir().unwrap();
            let report = format!(
                r#"{{
                  "schema_version": "visual_pose_report.v1",
                  "frame": {{}},
                  "cabinet_poses": [
                    {{"cabinet_id":{},
                     "corners_mm":[[-300,-170,0],[300,-170,0],[300,170,0],[-300,170,0]]}}
                  ]
                }}"#,
                serde_json::to_string(bad_id).unwrap()
            );
            let rp = dir.path().join("pose_report.json");
            std::fs::write(&rp, &report).unwrap();
            let out = dir.path().join("wall.obj");

            let result = run_export_pose_obj(&rp, "neutral", &out, None, false);
            assert!(
                matches!(result, Err(LmtError::InvalidInput(_))),
                "expected InvalidInput for cabinet_id={bad_id:?}, got {result:?}"
            );
        }
    }
```

把 `export_pose_obj_root_makes_reference_axis_aligned_and_grounded` 改为单文件 + 按 z≈0 过滤出基准块的 4 顶点：

```rust
    #[test]
    fn export_pose_obj_root_makes_reference_axis_aligned_and_grounded() {
        let dir = tempdir().unwrap();
        let rp = dir.path().join("BENCH_cabinet_pose_report.json");
        std::fs::write(&rp, BENCH_REPORT).unwrap();
        let out = dir.path().join("wall.obj");

        let res = run_export_pose_obj(&rp, "neutral", &out, Some("V000_R001"), true).unwrap();
        assert_eq!(res.cabinet_count, 2);

        let text = std::fs::read_to_string(&out).unwrap();
        let verts: Vec<[f64; 3]> = text
            .lines()
            .filter_map(|l| l.strip_prefix("v "))
            .map(|l| {
                let n: Vec<f64> = l.split_whitespace().map(|t| t.parse().unwrap()).collect();
                [n[0], n[1], n[2]]
            })
            .collect();
        assert_eq!(verts.len(), 8, "2 cabinets × 4 verts");
        // 基准块（V000_R001）re-root 后落在 XY 平面 → 取 z≈0 的 4 个顶点
        let refp: Vec<[f64; 3]> = verts.into_iter().filter(|v| v[2].abs() < 1e-3).collect();
        assert_eq!(refp.len(), 4, "reference panel should be the 4 z≈0 verts: {refp:?}");
        // ground：基准块底边 y=0
        let min_y = refp.iter().map(|v| v[1]).fold(f64::INFINITY, f64::min);
        assert!(min_y.abs() < 1e-3, "ground: ref min y should be 0, got {min_y}");
        // 高 ≈ 0.680 m
        let max_y = refp.iter().map(|v| v[1]).fold(f64::NEG_INFINITY, f64::max);
        assert!((max_y - 0.680).abs() < 0.01, "height ≈ 0.68m, got {max_y}");

        // 未知 --root → NotFound
        let err = run_export_pose_obj(&rp, "neutral", &out, Some("V999_R999"), false).unwrap_err();
        assert!(matches!(err, LmtError::NotFound(_)), "got {err:?}");
    }
```

- [ ] **Step 7: 改 CLI 子命令定义**（`crates/lmt-cli/src/cli.rs:259-278`）

把 `PoseObj { ... }` 里的 `out_dir` 块替换：

```rust
    /// 把 cabinet_pose_report.json 的所有箱体合并导出成一个世界坐标 OBJ。
    /// side_effect: destructive(写文件,需要 --yes 或 --dry-run)
    #[command(name = "pose-obj")]
    PoseObj {
        /// cabinet_pose_report.json 路径。
        pose_report: String,
        /// target software: disguise / unreal / neutral。
        target: String,
        /// 输出 OBJ 文件路径（所有箱体合并为一个文件）。
        #[arg(long, value_name = "PATH")]
        out: PathBuf,
        /// 以该 cabinet_id 为基准:把整个场景重定位到它的局部系(它轴对齐落在原点),
        /// 其余屏保持真实相对位姿。不传则用重建根 cabinet 的世界系。
        #[arg(long, value_name = "CABINET_ID")]
        root: Option<String>,
        /// 让下边缘贴地(最低 Y = 0),而非以中心为原点。给了 --root 时以基准屏下沿
        /// 为 0,其余屏保持真实相对高度(物理更低的屏可能 y<0)。
        #[arg(long)]
        ground: bool,
    },
```

- [ ] **Step 8: 改 CLI handler**（`crates/lmt-cli/src/commands/export.rs`）

`run()` 里的 `ExportCmd::PoseObj` 分支（行 17-24）改为：

```rust
        ExportCmd::PoseObj {
            pose_report,
            target,
            out,
            root,
            ground,
        } => pose_obj(mode, &pose_report, &target, &out, root.as_deref(), ground, yes, dry_run),
```

`fn pose_obj`（行 138-214）的签名 `out_dir: &Path` 改为 `out: &Path`，并把 DryRun / Execute 两支替换为：

```rust
    match decision {
        DestructiveDecision::DryRun => {
            if !matches!(target, "disguise" | "unreal" | "neutral") {
                return output::err(
                    mode,
                    ApiError::new(
                        error_codes::INVALID_INPUT,
                        format!("unknown target: {target}"),
                    ),
                );
            }
            if !Path::new(pose_report).is_file() {
                return output::err(
                    mode,
                    ApiError::new(
                        error_codes::NOT_FOUND,
                        format!("pose report not found: {pose_report}"),
                    ),
                );
            }
            if let Err(e) = lmt_app::export::check_pose_obj_inputs(Path::new(pose_report), root) {
                return output::err(mode, ApiError::from(e));
            }
            let resolved = lmt_app::export::ensure_obj_extension(out);
            let payload = serde_json::json!({
                "dry_run": true,
                "pose_report": pose_report,
                "target": target,
                "root": root,
                "ground": ground,
                "would_write": resolved.display().to_string(),
            });
            output::ok(mode, payload, |_| {
                let _ = writeln!(
                    std::io::stdout(),
                    "[dry-run] would export merged OBJ from {pose_report} to {}",
                    resolved.display()
                );
            })
        }
        DestructiveDecision::Execute => {
            match lmt_app::export::run_export_pose_obj(
                Path::new(pose_report),
                target,
                out,
                root,
                ground,
            ) {
                Ok(r) => output::ok(mode, r, |p| {
                    let _ = writeln!(
                        std::io::stdout(),
                        "wrote {} cabinets ({} target) into {}",
                        p.cabinet_count,
                        p.target,
                        p.file
                    );
                }),
                Err(e) => output::err(mode, ApiError::from(e)),
            }
        }
    }
```

- [ ] **Step 9: 更新 cli_e2e 的 5 个 pose-obj 用例**（`crates/lmt-cli/tests/cli_e2e.rs:1601-1764`）

逐个把 `--out-dir <out_dir>` 改成 `--out <out_file>`、把每屏文件断言改成单文件断言。替换 5 个测试：

```rust
/// happy: 2-cabinet report → exit 0, envelope ok, cabinet_count==2, single OBJ exists.
#[test]
fn export_pose_obj_happy() {
    let tmp = TempDir::new().unwrap();
    let report = tmp.path().join("cabinet_pose_report.json");
    std::fs::write(&report, pose_report_json()).unwrap();
    let out = tmp.path().join("wall.obj");

    let assert = lmt()
        .args([
            "--json", "--yes", "export", "pose-obj",
            report.to_str().unwrap(), "neutral",
            "--out", out.to_str().unwrap(),
        ])
        .assert()
        .success();

    let env: Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    assert_eq!(env["ok"], true, "envelope ok: {env}");
    assert_eq!(env["data"]["cabinet_count"], 2, "cabinet_count: {env}");
    assert!(out.is_file(), "merged OBJ must exist");
    let text = std::fs::read_to_string(&out).unwrap();
    assert_eq!(text.lines().filter(|l| l.starts_with("v ")).count(), 8);
}

/// dry-run: output file must NOT be created, exit 0, dry_run==true in envelope.
#[test]
fn export_pose_obj_dry_run_writes_nothing() {
    let tmp = TempDir::new().unwrap();
    let report = tmp.path().join("cabinet_pose_report.json");
    std::fs::write(&report, pose_report_json()).unwrap();
    let out = tmp.path().join("wall.obj");

    let assert = lmt()
        .args([
            "--json", "--dry-run", "export", "pose-obj",
            report.to_str().unwrap(), "neutral",
            "--out", out.to_str().unwrap(),
        ])
        .assert()
        .success();

    let env: Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    assert_eq!(env["ok"], true, "envelope ok: {env}");
    assert_eq!(env["data"]["dry_run"], true, "dry_run flag: {env}");
    assert!(!out.exists(), "dry-run must not create the output file");
}

/// missing report → non-zero exit, envelope ok==false.
#[test]
fn export_pose_obj_missing_report_is_error() {
    let tmp = TempDir::new().unwrap();
    let missing = tmp.path().join("nope.json");
    let out = tmp.path().join("wall.obj");

    let assert = lmt()
        .args([
            "--json", "--yes", "export", "pose-obj",
            missing.to_str().unwrap(), "neutral",
            "--out", out.to_str().unwrap(),
        ])
        .assert()
        .failure();

    let out_assert = assert.get_output();
    assert_ne!(out_assert.status.code(), Some(0), "must be non-zero exit");
    let stderr = std::str::from_utf8(&out_assert.stderr).unwrap().trim_end();
    let env: Value = serde_json::from_str(stderr).expect("stderr must be JSON envelope");
    assert_eq!(env["ok"], false, "envelope ok must be false: {env}");
}

/// --root + --ground: reference panel axis-aligned (z≈0) with bottom edge at y=0.
#[test]
fn export_pose_obj_root_and_ground() {
    let tmp = TempDir::new().unwrap();
    let report = tmp.path().join("cabinet_pose_report.json");
    std::fs::write(&report, pose_report_json()).unwrap();
    let out = tmp.path().join("wall.obj");

    let assert = lmt()
        .args([
            "--json", "--yes", "export", "pose-obj",
            report.to_str().unwrap(), "neutral",
            "--out", out.to_str().unwrap(),
            "--root", "V000_R001", "--ground",
        ])
        .assert()
        .success();

    let env: Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    assert_eq!(env["ok"], true, "envelope ok: {env}");

    let text = std::fs::read_to_string(&out).unwrap();
    let verts: Vec<[f64; 3]> = text
        .lines()
        .filter_map(|l| l.strip_prefix("v "))
        .map(|l| {
            let n: Vec<f64> = l.split_whitespace().map(|t| t.parse().unwrap()).collect();
            [n[0], n[1], n[2]]
        })
        .collect();
    assert_eq!(verts.len(), 8);
    let refp: Vec<[f64; 3]> = verts.into_iter().filter(|v| v[2].abs() < 1e-3).collect();
    assert_eq!(refp.len(), 4, "reference panel = 4 z≈0 verts: {refp:?}");
    let min_y = refp.iter().map(|v| v[1]).fold(f64::INFINITY, f64::min);
    assert!(min_y.abs() < 1e-3, "ground: ref min y should be 0, got {min_y}");
}

/// dry-run must reject an unknown --root (parity with execute), not green-light it.
#[test]
fn export_pose_obj_dry_run_validates_root() {
    let tmp = TempDir::new().unwrap();
    let report = tmp.path().join("cabinet_pose_report.json");
    std::fs::write(&report, pose_report_json()).unwrap();
    let out = tmp.path().join("wall.obj");

    let assert = lmt()
        .args([
            "--json", "--dry-run", "export", "pose-obj",
            report.to_str().unwrap(), "neutral",
            "--out", out.to_str().unwrap(),
            "--root", "NONEXISTENT_CAB",
        ])
        .assert()
        .failure();

    let out_assert = assert.get_output();
    assert_ne!(out_assert.status.code(), Some(0), "unknown --root must fail dry-run");
    let stderr = std::str::from_utf8(&out_assert.stderr).unwrap().trim_end();
    let env: Value = serde_json::from_str(stderr).expect("stderr must be JSON envelope");
    assert_eq!(env["ok"], false, "envelope ok must be false: {env}");
    assert!(!out.exists(), "dry-run must not create the output file");
}
```

- [ ] **Step 10: 全量编译 + 测试**

Run: `cargo test --workspace 2>&1 | tail -30`
Expected: 全绿（含 lmt-app 单测 + cli_e2e 的 5 个 pose-obj 用例 + manifest 测试）。若有编译错误，按报错定位上面遗漏的调用点修正。

- [ ] **Step 11: 提交**

```bash
git add crates/lmt-shared/src/dto.rs crates/lmt-shared/src/manifest.rs \
        crates/lmt-app/src/export.rs \
        crates/lmt-cli/src/cli.rs crates/lmt-cli/src/commands/export.rs \
        crates/lmt-cli/tests/cli_e2e.rs
git commit -m "feat(export): pose-obj emits single merged OBJ with integral grid UV

Per-cabinet unwelded panels + one 0-1 grid UV (each cabinet = its cell);
--out-dir -> --out (single file); ExportPoseObjResult.files -> file.
Breaking change, no compat shim (per CLAUDE.md); all in-repo call sites updated."
```

---

## Task 4: docs + manifest 快照 + 自检

**Files:**
- Modify: `docs/agents-cli.md:39`
- Modify: `docs/contract-manifest.json`（重生成）

- [ ] **Step 1: 改 docs/agents-cli.md 命令表第 39 行**

把该行的命令与 Result 描述更新为（保持表格 4 列结构）：用法 `lmt export pose-obj <pose_report> <target> --out <path> [--root <cabinet_id>] [--ground]`；说明改为"把 `cabinet_pose_report.json` 的所有箱体合并导出成**一个**世界坐标 OBJ：每箱体独立面片（不焊接）+ 一张整体 0-1 网格 UV（每块占自己格子，导入 disguise 内容横铺整面墙）。几何按 `Neutral` 原样输出。`--root`/`--ground` 同前。"；Result 改为 `{target, cabinet_count, file}`。

- [ ] **Step 2: 重新构建 CLI 并重生成 manifest 快照**

Run:
```bash
cargo build -p lmt-cli && ./target/debug/lmt --json manifest | jq .data > docs/contract-manifest.json
git diff --stat docs/contract-manifest.json
```
Expected: `docs/contract-manifest.json` 仅在 `export.pose_obj` 那条的 usage 字符串处变化（`--out-dir`→`--out`），operation_id 不变。

- [ ] **Step 3: schema dump 自检（确认 DTO 改对）**

Run: `./target/debug/lmt --json schema | jq '.. | objects | select(.title? == "ExportPoseObjResult")'`
Expected: 输出含 `file` 属性、不含 `files`。

- [ ] **Step 4: 子命令帮助自检**

Run: `./target/debug/lmt export pose-obj --help`
Expected: 显示 `--out <PATH>`（无 `--out-dir`）。

- [ ] **Step 5: 端到端冒烟（真跑一次）**

Run:
```bash
T=$(mktemp -d); printf '%s' '{"schema_version":"visual_pose_report.v1","frame":{},"cabinet_poses":[{"cabinet_id":"V000_R000","corners_mm":[[-300,-170,0],[300,-170,0],[300,170,0],[-300,170,0]]},{"cabinet_id":"V001_R000","corners_mm":[[300,-170,0],[900,-170,0],[900,170,0],[300,170,0]]}]}' > "$T/r.json"
./target/debug/lmt --yes export pose-obj "$T/r.json" neutral --out "$T/wall.obj"
echo "--- distinct U (期望 3 = cols2+1) ---"; grep '^vt ' "$T/wall.obj" | awk '{print $2}' | sort -u | wc -l
echo "--- distinct V (期望 2 = rows1+1) ---"; grep '^vt ' "$T/wall.obj" | awk '{print $3}' | sort -u | wc -l
echo "--- v 数 (期望 8) ---"; grep -c '^v ' "$T/wall.obj"
```
Expected: distinct U = 3、distinct V = 2、v 数 = 8。

- [ ] **Step 6: 提交**

```bash
git add docs/agents-cli.md docs/contract-manifest.json
git commit -m "docs(cli): pose-obj single-file --out + integral grid UV; refresh manifest snapshot"
```

---

## Self-Review（计划对照 spec）

**1. Spec 覆盖：**
- spec §2 UV 网格化 → Task 3 Step 3（`panel_surface` 格子 UV）+ Task 1（解析/维度）。✓
- spec §3.1 单文件打包 → Task 2（merge）+ Task 3 Step 4。✓
- spec §3.2 朝向：保持绕序、不写法线、模拟测试兜底 → 沿用 `surface_to_mesh_output(Neutral)`，计划未引入法线，符合。✓（朝向正确性属手动验收，spec §5.3，不在自动化范围。）
- spec §4 CLI `--out` + 删旧模式 + DTO `file` → Task 3 Step 1/7/8。✓
- spec §4.1 调用点全改 → Task 3（dto/manifest/app/cli/e2e）+ Task 4（docs/snapshot）。✓
- spec §5.1 单测（解析/维度/格子UV/合并/异形/整体结构回归）→ Task 1、Task 2、Task 3 Step 6。**注：异形屏（缺块）未写专门单测**——补在下方。
- spec §5.2 E2E 四类 → Task 3 Step 9（happy/dry-run/missing-report=envelope；`export_pose_obj_rejects_unparseable_cabinet_id` 单测覆盖 refuse 类）。✓

**2. 占位符扫描：** 无 TBD/TODO；每个 code step 都给了完整代码。✓

**3. 类型一致性：** `parse_cabinet_col_row`/`infer_grid_dims`/`merge_mesh_outputs`/`panel_surface(col,row,cols,rows)`/`run_export_pose_obj(..out_file..)`/`ExportPoseObjResult{file}` 在各 task 用法一致。✓

**补：异形屏单测（加进 Task 1 或 Task 3 Step 6 的 tests mod）**

```rust
    #[test]
    fn infer_grid_dims_handles_absent_cells() {
        // 缺 V001_R000：dims 仍按 max+1 推（2 列 × 2 行）
        let ids = ["V000_R000", "V000_R001", "V001_R001"];
        assert_eq!(infer_grid_dims(&ids).unwrap(), (2, 2));
    }
```

---

## Execution Handoff

详见 spec 的成功判据；朝向/内容铺满靠用户在 disguise 的模拟测试做最终验收（spec §5.3）。
