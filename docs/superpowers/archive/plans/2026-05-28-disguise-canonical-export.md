# disguise 标准摆法导出 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 `lmt export pose-obj ... disguise`(不带摆放参数)默认产出标准摆法的 OBJ:中心列朝向转正(发光面→+Z,对齐 Path A)+ 贴地 + 居中,逐箱体偏差 1:1 保留;退化墙 fail-fast 要 `--root`。

**Architecture:** 在 `run_export_pose_obj` 内新增分支:`root` 给了→手动(现状);`disguise` 且无 `root`→对 panel 角点施加标准摆法刚性变换(只转 yaw,保 +Y up=保真);否则(neutral)→原始(+可选 ground)。标准摆法 = 中心列发光面法向(Path A 弧中点 θ_mid 的稳健类比,≥180° 墙也成立)绕 +Y 转到 +Z + 贴地 + 居中。

**Tech Stack:** Rust(`lmt-app` / `lmt-core`),nalgebra,clap,assert_cmd(E2E)。

**Spec:** `docs/superpowers/specs/2026-05-28-disguise-export-canonical-command-design.md`

---

## File Structure

只动 Path B 导出,集中在:
- `crates/lmt-app/src/export.rs` — 新增 `center_column_forward` / `apply_canonical_frame` 两个纯函数(复用本文件已有的 `CabinetFrame`);`run_export_pose_obj` 加摆放分支;改 disguise 单测 + 加新单测。
- `crates/lmt-cli/tests/cli_e2e.rs` — 加 disguise 标准摆法 case + 退化报错 case。
- `docs/agents-cli.md` + `crates/lmt-shared/src/manifest.rs` — 描述同步(命令签名不变)。

无 DTO/schema/CLI 签名变更。退化错误复用 `LmtError::InvalidInput`。

---

## Task 1: `center_column_forward`(中心列前向)

新增纯函数:取墙中心列在位箱体的平均发光面法向、返回归一化水平分量(退化返回 None)。暂无调用方(Task 3 接入),dead_code 警告无害(本仓不启用 `-D warnings`)。

**Files:**
- Modify: `crates/lmt-app/src/export.rs`(函数加在 `fn panel_surface` 之前;测试加进 `#[cfg(test)] mod tests`)

- [ ] **Step 1: 写失败测试**(加进 `mod tests`)

```rust
    fn panel(col: u32, row: u32, corners: [[f64; 3]; 4]) -> (String, u32, u32, [[f64; 3]; 4]) {
        (format!("V{col:03}_R{row:03}"), col, row, corners)
    }
    /// 一块面向给定水平法向、竖直站立的箱体角点(BL,BR,TR,TL,mm)。
    fn facing_panel(col: u32, row: u32, nx: f64, nz: f64) -> (String, u32, u32, [[f64; 3]; 4]) {
        // right = up(+Y) × normal;normal=(nx,0,nz)
        let n = Vector3::new(nx, 0.0, nz).normalize();
        let up = Vector3::new(0.0, 1.0, 0.0);
        let right = up.cross(&n).normalize(); // 沿底边方向
        let (hw, hh) = (300.0, 170.0);
        let c = Vector3::new(col as f64 * 700.0, 0.0, 0.0); // 任意横向铺开
        let bl = c - right * hw - up * hh;
        let br = c + right * hw - up * hh;
        let tr = c + right * hw + up * hh;
        let tl = c - right * hw + up * hh;
        let v = |p: Vector3<f64>| [p.x, p.y, p.z];
        (format!("V{col:03}_R{row:03}"), col, row, [v(bl), v(br), v(tr), v(tl)])
    }

    #[test]
    fn center_column_forward_flat_wall() {
        // 3 列平墙全朝 +X → 中心列(col1)前向 ≈ +X
        let panels = vec![
            facing_panel(0, 0, 1.0, 0.0),
            facing_panel(1, 0, 1.0, 0.0),
            facing_panel(2, 0, 1.0, 0.0),
        ];
        let f = center_column_forward(&panels, 3).unwrap();
        assert!((f - Vector3::new(1.0, 0.0, 0.0)).norm() < 1e-6, "got {f:?}");
    }

    #[test]
    fn center_column_forward_handles_wraparound() {
        // 模拟 ≥180° 包角:法向铺满,平均会接近 0,但中心列(col1)朝 +Z 明确
        let panels = vec![
            facing_panel(0, 0, -1.0, 0.0),
            facing_panel(1, 0, 0.0, 1.0), // 中心列朝 +Z
            facing_panel(2, 0, 1.0, 0.0),
        ];
        let f = center_column_forward(&panels, 3).unwrap();
        assert!((f - Vector3::new(0.0, 0.0, 1.0)).norm() < 1e-6, "got {f:?}");
    }

    #[test]
    fn center_column_forward_degenerate_returns_none() {
        // 墙面朝上(法向≈+Y)→ 水平分量≈0 → None
        let flat_up = [[-300.0, 0.0, -170.0], [300.0, 0.0, -170.0], [300.0, 0.0, 170.0], [-300.0, 0.0, 170.0]];
        let panels = vec![panel(0, 0, flat_up)];
        assert!(center_column_forward(&panels, 1).is_none());
    }
```

- [ ] **Step 2: 运行,确认编译失败**

Run: `cargo test -p lmt-app center_column_forward 2>&1 | tail -15`
Expected: `cannot find function center_column_forward`。

- [ ] **Step 3: 实现**(加在 `fn panel_surface` 之前)

```rust
/// 墙中心列在位箱体的平均发光面外法向,取归一化水平分量。
/// 中心列 = round((cols-1)/2);该列空则取最近非空列。Path A 弧中点 θ_mid 的稳健类比:
/// ≥180° 包角墙(全墙平均法向会抵消)用中心列仍能定出明确前向。
/// 水平分量近 0(墙面朝上/下等病态)→ None。
fn center_column_forward(
    panels: &[(String, u32, u32, [[f64; 3]; 4])],
    cols: u32,
) -> Option<Vector3<f64>> {
    if panels.is_empty() || cols == 0 {
        return None;
    }
    let c_mid = (cols - 1) / 2;
    let present: std::collections::BTreeSet<u32> = panels.iter().map(|(_, c, _, _)| *c).collect();
    let target_col = if present.contains(&c_mid) {
        c_mid
    } else {
        *present
            .iter()
            .min_by_key(|&&c| (c as i64 - c_mid as i64).abs())?
    };
    let mut sum = Vector3::zeros();
    let mut n = 0u32;
    for (_, c, _, cs) in panels.iter() {
        if *c == target_col {
            if let Some(f) = CabinetFrame::from_corners(cs) {
                sum += f.z;
                n += 1;
            }
        }
    }
    if n == 0 {
        return None;
    }
    let avg = sum / n as f64;
    let n_h = Vector3::new(avg.x, 0.0, avg.z);
    if n_h.norm() < 1e-6 {
        return None;
    }
    Some(n_h.normalize())
}
```

- [ ] **Step 4: 运行,确认通过**

Run: `cargo test -p lmt-app center_column_forward 2>&1 | tail -15`
Expected: 3 passed。

- [ ] **Step 5: 提交**

```bash
git add crates/lmt-app/src/export.rs
git commit -m "feat(export): add center_column_forward (robust wall facing)"
```

---

## Task 2: `apply_canonical_frame`(转正+贴地+居中)

**Files:**
- Modify: `crates/lmt-app/src/export.rs`

- [ ] **Step 1: 写失败测试**(加进 `mod tests`)

```rust
    /// 把整组 panel 角点施加任意 yaw(绕Y)+ 平移(模拟不同架站/根箱体)。
    fn perturb_yaw_translate(
        panels: &[(String, u32, u32, [[f64; 3]; 4])],
        yaw: f64,
        t: [f64; 3],
    ) -> Vec<(String, u32, u32, [[f64; 3]; 4])> {
        let (s, c) = yaw.sin_cos();
        panels
            .iter()
            .map(|(id, col, row, cs)| {
                let nc = cs.map(|p| {
                    let (x, z) = (p[0], p[2]);
                    [x * c - z * s + t[0], p[1] + t[1], x * s + z * c + t[2]]
                });
                (id.clone(), *col, *row, nc)
            })
            .collect()
    }

    #[test]
    fn apply_canonical_frame_faces_plus_z_grounded_centered() {
        // 平墙朝 +X → 标准摆法后中心列法向 ≈ +Z、min Y=0、水平质心=0
        let mut panels = vec![
            facing_panel(0, 0, 1.0, 0.0),
            facing_panel(1, 0, 1.0, 0.0),
            facing_panel(2, 0, 1.0, 0.0),
        ];
        apply_canonical_frame(&mut panels, 3).unwrap();
        // 中心列法向 → +Z
        let f = center_column_forward(&panels, 3).unwrap();
        assert!((f - Vector3::new(0.0, 0.0, 1.0)).norm() < 1e-6, "facing {f:?}");
        // 贴地 + 居中
        let all: Vec<[f64; 3]> = panels.iter().flat_map(|(_, _, _, cs)| cs.iter().copied()).collect();
        let min_y = all.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
        let mean_x = all.iter().map(|p| p[0]).sum::<f64>() / all.len() as f64;
        let mean_z = all.iter().map(|p| p[2]).sum::<f64>() / all.len() as f64;
        assert!(min_y.abs() < 1e-6, "min_y {min_y}");
        assert!(mean_x.abs() < 1e-6 && mean_z.abs() < 1e-6, "centroid ({mean_x},{mean_z})");
    }

    #[test]
    fn apply_canonical_frame_invariant_under_yaw_translation() {
        let base = vec![
            facing_panel(0, 0, 0.3, 1.0),
            facing_panel(1, 0, 0.0, 1.0),
            facing_panel(2, 0, -0.3, 1.0),
        ];
        let mut a = base.clone();
        apply_canonical_frame(&mut a, 3).unwrap();
        // 叠加任意 yaw + 平移后再标准摆法 → 与 a 逐顶点一致
        let mut b = perturb_yaw_translate(&base, 1.2345, [123.0, -45.0, 67.0]);
        apply_canonical_frame(&mut b, 3).unwrap();
        for ((_, _, _, ca), (_, _, _, cb)) in a.iter().zip(b.iter()) {
            for (pa, pb) in ca.iter().zip(cb.iter()) {
                for k in 0..3 {
                    assert!((pa[k] - pb[k]).abs() < 1e-6, "yaw+translate not invariant: {pa:?} vs {pb:?}");
                }
            }
        }
    }

    #[test]
    fn apply_canonical_frame_preserves_relative_geometry() {
        // 两块成已知夹角 → 标准摆法(刚性)后夹角不变
        let mut panels = vec![facing_panel(0, 0, 1.0, 0.0), facing_panel(1, 0, 0.0, 1.0)];
        let ang = |cs: &[[f64; 3]; 4]| CabinetFrame::from_corners(cs).unwrap().z;
        let before = ang(&panels[0].3).dot(&ang(&panels[1].3));
        apply_canonical_frame(&mut panels, 2).unwrap();
        let after = ang(&panels[0].3).dot(&ang(&panels[1].3));
        assert!((before - after).abs() < 1e-6, "relative angle changed: {before} -> {after}");
    }

    #[test]
    fn apply_canonical_frame_degenerate_errors() {
        // 墙面朝上 → 无法定向 → InvalidInput(不 panic)
        let flat_up = [[-300.0, 0.0, -170.0], [300.0, 0.0, -170.0], [300.0, 0.0, 170.0], [-300.0, 0.0, 170.0]];
        let mut panels = vec![panel(0, 0, flat_up)];
        assert!(matches!(apply_canonical_frame(&mut panels, 1), Err(LmtError::InvalidInput(_))));
    }
```

- [ ] **Step 2: 运行,确认编译失败**

Run: `cargo test -p lmt-app apply_canonical_frame 2>&1 | tail -15`
Expected: `cannot find function apply_canonical_frame`。

- [ ] **Step 3: 实现**(加在 `center_column_forward` 之后)

```rust
/// 标准摆法:中心列前向绕 +Y 转到 +Z + 贴地(min Y=0)+ 居中(水平质心→原点)。
/// 只转 yaw、保 +Y up —— 真实倾斜(roll/pitch)刻意保留(保真,见 spec §3)。
/// 整组刚性变换,逐箱体相对几何不变。无法定向(中心列水平法向≈0)→ InvalidInput。
fn apply_canonical_frame(
    panels: &mut [(String, u32, u32, [[f64; 3]; 4])],
    cols: u32,
) -> LmtResult<()> {
    let fwd = center_column_forward(panels, cols).ok_or_else(|| {
        LmtError::InvalidInput(
            "cannot auto-orient: wall normal near-vertical or no usable cabinets; pass --root <cabinet_id>".into(),
        )
    })?;
    // θ = atan2(fwd.x, fwd.z);R_y(-θ) 把 fwd 转到 +Z:x'=x·cosθ - z·sinθ,z'=x·sinθ + z·cosθ。
    let theta = fwd.x.atan2(fwd.z);
    let (s, c) = theta.sin_cos();
    for (_, _, _, cs) in panels.iter_mut() {
        for p in cs.iter_mut() {
            let (x, z) = (p[0], p[2]);
            p[0] = x * c - z * s;
            p[2] = x * s + z * c;
        }
    }
    // 贴地 + 居中(全顶点)。
    let mut min_y = f64::INFINITY;
    let (mut sum_x, mut sum_z, mut n) = (0.0, 0.0, 0u32);
    for (_, _, _, cs) in panels.iter() {
        for p in cs.iter() {
            min_y = min_y.min(p[1]);
            sum_x += p[0];
            sum_z += p[2];
            n += 1;
        }
    }
    let (mean_x, mean_z) = (sum_x / n as f64, sum_z / n as f64);
    for (_, _, _, cs) in panels.iter_mut() {
        for p in cs.iter_mut() {
            p[0] -= mean_x;
            p[1] -= min_y;
            p[2] -= mean_z;
        }
    }
    Ok(())
}
```

- [ ] **Step 4: 运行,确认通过**

Run: `cargo test -p lmt-app apply_canonical_frame 2>&1 | tail -15`
Expected: 4 passed。

- [ ] **Step 5: 提交**

```bash
git add crates/lmt-app/src/export.rs
git commit -m "feat(export): add apply_canonical_frame (yaw-to-+Z + ground + center)"
```

---

## Task 3: 接入 `run_export_pose_obj` + 改单测 + E2E

**Files:**
- Modify: `crates/lmt-app/src/export.rs:160`(`parse_target` 捕获 enum)、`:215-229`(摆放分支)、disguise 单测
- Modify: `crates/lmt-cli/tests/cli_e2e.rs`

- [ ] **Step 1: 捕获 target enum**

把 `crates/lmt-app/src/export.rs:160` 的:
```rust
    let _ = parse_target(target)?; // 校验 target；几何原样（Neutral）
```
改为:
```rust
    let target_enum = parse_target(target)?; // 校验 target；几何原样（Neutral）
```

- [ ] **Step 2: 摆放分支(替换现有 ground 块)**

把 `crates/lmt-app/src/export.rs` 现有的 ground 块(`if ground { let min_y = panels.iter().filter(...)... }`,约 215-229 行)整段替换为:
```rust
    // 摆放：root→手动(+ground);disguise 无 root→标准摆法;否则(neutral)→原始(+可选 ground)。
    if root.is_none() && target_enum == TargetSoftware::Disguise {
        apply_canonical_frame(&mut panels, cols)?;
    } else if ground {
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
```

- [ ] **Step 3: 改 disguise 单测**(`export_pose_obj_disguise_target_equals_raw_world_frame`,约 506-526 行)

整体替换为(disguise 现在=标准摆法,不再=原始):
```rust
    #[test]
    fn export_pose_obj_disguise_applies_canonical_placement() {
        let dir = tempdir().unwrap();
        let rp = dir.path().join("BENCH_cabinet_pose_report.json");
        std::fs::write(&rp, BENCH_REPORT).unwrap();
        let out = dir.path().join("wall_disguise.obj");

        let res = run_export_pose_obj(&rp, "disguise", &out, None, false).unwrap();
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
        assert_eq!(verts.len(), 8);
        // 标准摆法:贴地 + 水平居中
        let min_y = verts.iter().map(|v| v[1]).fold(f64::INFINITY, f64::min);
        let mean_x = verts.iter().map(|v| v[0]).sum::<f64>() / 8.0;
        let mean_z = verts.iter().map(|v| v[2]).sum::<f64>() / 8.0;
        assert!(min_y.abs() < 1e-6, "grounded: min_y={min_y}");
        assert!(mean_x.abs() < 1e-6 && mean_z.abs() < 1e-6, "centered: ({mean_x},{mean_z})");
        // 不再是原始帧(原始 V000_R000 的 BL 是 -0.3,-0.17,0)
        assert!(!text.contains("v -0.3 -0.17 0"), "disguise should be canonical, not raw:\n{text}");
    }
```
(注:`neutral` 仍=原始,由现有 `export_pose_obj_writes_single_merged_obj_with_grid_uv` 用 neutral 锁定,不动。)

- [ ] **Step 4: 跑 lmt-app 全测**

Run: `cargo test -p lmt-app 2>&1 | tail -8`
Expected: 全绿(含新 disguise 单测 + Task1/2 helpers)。

- [ ] **Step 5: cli_e2e 加两个 case**(`crates/lmt-cli/tests/cli_e2e.rs`,加在现有 pose-obj E2E 区之后)

```rust
/// disguise 不带摆放参数 → 标准摆法:贴地 + 水平居中。
#[test]
fn export_pose_obj_disguise_canonical_no_flags() {
    let tmp = TempDir::new().unwrap();
    let report = tmp.path().join("cabinet_pose_report.json");
    std::fs::write(&report, pose_report_json()).unwrap();
    let out = tmp.path().join("wall.obj");

    lmt()
        .args(["--json", "--yes", "export", "pose-obj",
               report.to_str().unwrap(), "disguise", "--out", out.to_str().unwrap()])
        .assert()
        .success();

    let text = std::fs::read_to_string(&out).unwrap();
    let ys: Vec<f64> = text.lines().filter_map(|l| l.strip_prefix("v "))
        .map(|l| l.split_whitespace().nth(1).unwrap().parse::<f64>().unwrap()).collect();
    let min_y = ys.iter().cloned().fold(f64::INFINITY, f64::min);
    assert!(min_y.abs() < 1e-3, "disguise canonical must be grounded, got min_y={min_y}");
}

/// 退化墙(全部面朝上)→ disguise 无法定向 → 非零退出 + envelope ok=false + 不写文件。
#[test]
fn export_pose_obj_disguise_degenerate_errors() {
    let tmp = TempDir::new().unwrap();
    let report = tmp.path().join("cabinet_pose_report.json");
    // 两块都躺平(法向 ≈ +Y)→ 水平法向 0
    std::fs::write(&report, r#"{"schema_version":"visual_pose_report.v1","frame":{},"cabinet_poses":[
 {"cabinet_id":"V000_R000","corners_mm":[[-300,0,-170],[300,0,-170],[300,0,170],[-300,0,170]]},
 {"cabinet_id":"V001_R000","corners_mm":[[400,0,-170],[1000,0,-170],[1000,0,170],[400,0,170]]}]}"#).unwrap();
    let out = tmp.path().join("wall.obj");

    let assert = lmt()
        .args(["--json", "--yes", "export", "pose-obj",
               report.to_str().unwrap(), "disguise", "--out", out.to_str().unwrap()])
        .assert()
        .failure();
    let env: Value = serde_json::from_str(
        std::str::from_utf8(&assert.get_output().stderr).unwrap().trim_end()
    ).expect("stderr JSON envelope");
    assert_eq!(env["ok"], false, "degenerate must error: {env}");
    assert!(!out.exists(), "must not write file on degenerate");
}
```

- [ ] **Step 6: 跑全 workspace**

Run: `cargo test --workspace 2>&1 | tail -12`
Expected: 全绿。

- [ ] **Step 7: 提交**

```bash
git add crates/lmt-app/src/export.rs crates/lmt-cli/tests/cli_e2e.rs
git commit -m "feat(export): disguise target auto-applies canonical placement

disguise (no --root) = center-column yaw->+Z + ground + center; neutral stays
raw; --root manual override. Degenerate wall fails fast requiring --root."
```

---

## Task 4: Path A 前向约定参照测试(钉死 +Z,F2)

确认 Path A(`surface_to_mesh_output(Disguise)`)的发光面也落 +Z——与 Task 2 锁定的 Path B `+Z` 同轴,两侧独立断言即钉死共享契约(Path A 权威)。

**Files:**
- Modify: `crates/lmt-app/src/export.rs`(`mod tests`)

- [ ] **Step 1: 写测试**

```rust
    #[test]
    fn path_a_disguise_front_face_is_plus_z() {
        // Path A model frame: 凸法向 +Y、列 +X、高 +Z。构一块平 1×1 panel(y=0 平面)。
        use lmt_core::export::build::surface_to_mesh_output;
        use lmt_core::surface::{GridTopology, QualityMetrics, ReconstructedSurface};
        let topo = GridTopology { cols: 1, rows: 1 };
        let surface = ReconstructedSurface {
            screen_id: "A".into(),
            topology: topo,
            // 行主序 (0,0),(1,0),(0,1),(1,1);y=0 → 法向 ±Y(凸=+Y)
            vertices: vec![
                Vector3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 1.0),
            ],
            uv_coords: compute_grid_uv(topo),
            quality_metrics: QualityMetrics::default(),
            scatter_fit: None,
        };
        let array = CabinetArray::rectangle(1, 1, [1.0, 1.0]);
        let mesh = surface_to_mesh_output(&surface, &array, TargetSoftware::Disguise, 0.0).unwrap();
        // 三角面 0 的几何法向(按 winding)= 发光/前面方向。
        let t = mesh.triangles[0];
        let v = |i: u32| mesh.vertices[i as usize];
        let nrm = (v(t[1]) - v(t[0])).cross(&(v(t[2]) - v(t[0]))).normalize();
        assert!(nrm.z > 0.9, "Path A disguise front face must be +Z, got {nrm:?}");
    }
```

- [ ] **Step 2: 运行**

Run: `cargo test -p lmt-app path_a_disguise_front_face_is_plus_z 2>&1 | tail -15`
Expected: PASS(若 FAIL=该断言揭示 Path A 实际落 -Z → 说明 Path B 的 `+Z` 目标要改成 -Z,即 `apply_canonical_frame` 转到 -Z;此时回到 Task 2 调整目标轴并更新两处断言。这正是 golden 的作用:Path A 权威。)

- [ ] **Step 3: 提交**

```bash
git add crates/lmt-app/src/export.rs
git commit -m "test(export): pin disguise front-face to +Z via Path A reference"
```

---

## Task 5: docs + manifest 描述

**Files:**
- Modify: `docs/agents-cli.md:39`、`crates/lmt-shared/src/manifest.rs:118-119`

- [ ] **Step 1: 改 docs/agents-cli.md 第 39 行**

在 `export pose-obj` 行的描述里补:「**`disguise` 默认=标准摆法**(中心列转正发光面→+Z + 居中 + 贴地,逐箱体偏差 1:1 保留);**`neutral`=原始帧**;**`--root` 手动覆盖**;无法定向的墙(法向近垂直)→ 报错要求 `--root`」。命令签名与 Result 不变。

- [ ] **Step 2: 改 manifest 描述串**(`crates/lmt-shared/src/manifest.rs:118`)

把 `op("export.pose_obj", "...", ...)` 的描述串(第一个字符串参数)更新为反映「disguise 默认标准摆法 / neutral 原始 / --root 覆盖」。用法字符串(第二个参数)**不变**(`--out <path>` 等)。

- [ ] **Step 3: 重生成 manifest 快照 + 自检**

Run:
```bash
cargo build -p lmt-cli && ./target/debug/lmt --json manifest | jq .data > docs/contract-manifest.json
git diff --stat docs/contract-manifest.json
./target/debug/lmt export pose-obj --help | head
```
Expected: contract-manifest 仅 `export.pose_obj` 描述变化;`--help` 仍显示 `--out`。

- [ ] **Step 4: 端到端冒烟(真跑标准摆法)**

Run:
```bash
T=$(mktemp -d); printf '%s' '{"schema_version":"visual_pose_report.v1","frame":{},"cabinet_poses":[{"cabinet_id":"V000_R000","corners_mm":[[-300,-170,0],[300,-170,0],[300,170,0],[-300,170,0]]},{"cabinet_id":"V001_R000","corners_mm":[[300,-170,0],[900,-170,0],[900,170,0],[300,170,0]]}]}' > "$T/r.json"
./target/debug/lmt --yes export pose-obj "$T/r.json" disguise --out "$T/wall.obj"
echo "--- min Y(期望≈0,贴地)---"; grep '^v ' "$T/wall.obj" | awk '{print $3}' | sort -n | head -1
```
Expected: min Y ≈ 0(标准摆法已贴地)。

- [ ] **Step 5: 提交**

```bash
git add docs/agents-cli.md crates/lmt-shared/src/manifest.rs docs/contract-manifest.json
git commit -m "docs(cli): document disguise canonical placement default"
```

---

## Self-Review(计划对照 spec)

**1. Spec 覆盖:**
- §2 target 驱动 → Task 3 Step 2 分支。✓
- §3 标准摆法(中心列前向/转 +Z/贴地居中/保偏差) → Task 1(前向)+ Task 2(变换)。✓
- §3.1 退化 fail-fast → Task 2(Err)+ Task 3 Step 5(E2E 错误信封)。✓
- §5 yaw+平移不变性 / 转正 / 保偏差 / 退化 / ≥180° 稳健 → Task 1+2 单测全覆盖。✓
- §5 Path A vs Path B golden(+Z) → Task 4(Path A +Z)+ Task 2(Path B +Z),两侧独立断言钉死。✓
- §5 disguise≠raw 行为变更 → Task 3 Step 3。✓
- §7 docs/manifest → Task 5。✓

**2. 占位符扫描:** 无 TBD/TODO;每个 code step 给了完整代码。✓

**3. 类型一致性:** `center_column_forward(&[panel], cols)->Option<Vector3>`、`apply_canonical_frame(&mut [panel], cols)->LmtResult<()>`、panel 元组 `(String,u32,u32,[[f64;3];4])`、`target_enum==TargetSoftware::Disguise` 各 task 一致。`CabinetFrame`/`compute_grid_uv`/`CabinetArray`/`surface_to_mesh_output` 均为本文件已 import 或可 import 的现有项。✓

**4. 歧义:** 中心列定义((cols-1)/2、最近非空列)、+Z 目标(Path A 权威,Task 4 揭示并可回切 -Z)、退化阈值 1e-6 均明确。✓
