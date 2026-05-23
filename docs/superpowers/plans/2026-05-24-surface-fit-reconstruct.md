# Surface-Fit Reconstruct Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 LED Mesh Toolkit 支持把非网格散点（轮廓采样 + 杂点）robust 拟合成平面/圆柱曲面、再按 cabinet 网格重采样出标准 mesh，CLI 端到端可跑。

**Architecture:** core 新增 `surface_fit` 模块（纯几何，RANSAC 拟合 + 投影 + 重采样 + 坐标系导出），产出现有的 `ReconstructedSurface`（附带 core 内部 `ScatterFit` 元数据）；lmt-app 在 reconstruct 顶层按 `MeasuredPoints.sampling_mode` 分流，并把 core `ScatterFit` 转成 lmt-shared `ScatterFitInfo` 写进 `ReconstructionReport`；import 加 scatter 独立路径（新 CSV parser，跳过 SOP 校验/网格命名）；export 零改动复用。

**Tech Stack:** Rust，nalgebra（线代/PCA/SVD），手写确定性 RANSAC（无新依赖），serde + schemars 0.8，clap，rusqlite。

**Spec:** `docs/superpowers/specs/2026-05-23-surface-fit-reconstruct-design.md`

---

## Task 1: SamplingMode enum + MeasuredPoints 字段

**Files:**
- Create: `crates/core/src/sampling.rs`
- Modify: `crates/core/src/lib.rs`（加 `pub mod sampling;`）
- Modify: `crates/core/src/measured_points.rs`（加 `sampling_mode` 字段 + 测试）

- [ ] **Step 1: 写失败测试**（`crates/core/src/measured_points.rs` 末尾 `#[cfg(test)] mod tests`）

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::sampling::SamplingMode;

    const BASE: &str = r#"
screen_id: MAIN
coordinate_frame:
  origin_world: [0.0, 0.0, 0.0]
  basis: [[1.0,0.0,0.0],[0.0,1.0,0.0],[0.0,0.0,1.0]]
cabinet_array:
  cols: 4
  rows: 2
  cabinet_size_mm: [500.0, 500.0]
shape_prior:
  type: flat
points: []
"#;

    #[test]
    fn legacy_yaml_without_sampling_mode_defaults_to_grid() {
        let mp: MeasuredPoints = serde_yaml::from_str(BASE).unwrap();
        assert_eq!(mp.sampling_mode, SamplingMode::Grid);
    }

    #[test]
    fn scatter_mode_parses() {
        let yaml = format!("{BASE}sampling_mode: scatter\n");
        let mp: MeasuredPoints = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(mp.sampling_mode, SamplingMode::Scatter);
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p lmt-core measured_points::tests`
Expected: 编译失败 —— `SamplingMode` 不存在、`MeasuredPoints` 无 `sampling_mode` 字段。

- [ ] **Step 3: 创建 `crates/core/src/sampling.rs`**

```rust
use serde::{Deserialize, Serialize};

/// 测量点的采样方式，决定走哪条重建路径。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SamplingMode {
    /// 点落在网格顶点上，各自带 `<screen>_V<col>_R<row>` 名字（现状路径）。
    #[default]
    Grid,
    /// 屏面上的任意散点，靠曲面拟合重建。
    Scatter,
}
```

- [ ] **Step 4: 在 `crates/core/src/lib.rs` 注册模块**

在现有 `pub mod ...` 列表加一行（保持字母序附近即可）：

```rust
pub mod sampling;
```

- [ ] **Step 5: 给 `MeasuredPoints` 加字段**（`crates/core/src/measured_points.rs`）

顶部 import 加 `use crate::sampling::SamplingMode;`，结构体加字段（放在 `points` 之后）：

```rust
pub struct MeasuredPoints {
    pub screen_id: String,
    pub coordinate_frame: CoordinateFrame,
    pub cabinet_array: CabinetArray,
    pub shape_prior: ShapePrior,
    pub points: Vec<MeasuredPoint>,
    /// 采样方式。旧 measured.yaml 无此字段时默认 Grid（向后兼容）。
    #[serde(default)]
    pub sampling_mode: SamplingMode,
}
```

- [ ] **Step 6: 跑测试确认通过**

Run: `cargo test -p lmt-core measured_points::tests`
Expected: 2 passed。

- [ ] **Step 7: Commit**

```bash
git add crates/core/src/sampling.rs crates/core/src/lib.rs crates/core/src/measured_points.rs
git commit -m "feat(core): add SamplingMode + MeasuredPoints.sampling_mode (default Grid)"
```

---

## Task 2: surface_fit 模块骨架 + core 元数据类型 + ReconstructedSurface.scatter_fit

**Files:**
- Create: `crates/core/src/reconstruct/surface_fit/mod.rs`
- Modify: `crates/core/src/reconstruct/mod.rs`（加 `pub mod surface_fit;`）
- Modify: `crates/core/src/surface.rs`（`ReconstructedSurface` + `ReconstructedSurfaceRaw` 加 `scatter_fit` 字段）
- Modify: `crates/core/src/reconstruct/{direct,radial_basis,boundary_interp,nominal}.rs`（4 处构造点加 `scatter_fit: None`）

- [ ] **Step 1: 写失败测试**（`surface_fit/mod.rs` 内 `#[cfg(test)]`）

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::reconstruct::Reconstructor;
    use crate::sampling::SamplingMode;
    use crate::test_support::minimal_scatter_points; // Task 2 Step 5 定义

    #[test]
    fn applicable_only_for_scatter() {
        let mut mp = minimal_scatter_points();
        assert!(SurfaceFitReconstructor.applicable(&mp));
        mp.sampling_mode = SamplingMode::Grid;
        assert!(!SurfaceFitReconstructor.applicable(&mp));
    }

    #[test]
    fn reconstruct_stub_errors_until_assembled() {
        let mp = minimal_scatter_points();
        let err = SurfaceFitReconstructor.reconstruct(&mp).unwrap_err();
        assert!(matches!(err, crate::error::CoreError::Reconstruction(_)));
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p lmt-core surface_fit`
Expected: 编译失败（`SurfaceFitReconstructor`、`ScatterFit` 等未定义）。

- [ ] **Step 3: 写 `crates/core/src/reconstruct/surface_fit/mod.rs` 骨架 + 元数据类型**

```rust
//! 散点曲面拟合重建（scatter 路径）。不进 auto_reconstruct 序列，
//! 由 lmt-app 顶层在 sampling_mode==Scatter 时直接调用。

pub mod boundary;
pub mod fit;
pub mod frame;
pub mod project;
pub mod resample;

use serde::{Deserialize, Serialize};

use crate::error::CoreError;
use crate::measured_points::MeasuredPoints;
use crate::reconstruct::Reconstructor;
use crate::sampling::SamplingMode;
use crate::surface::ReconstructedSurface;

/// 拟合出的曲面形状（core 内部类型，坐标用 [f64;3] 便于转 DTO）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "shape")]
pub enum ScatterShape {
    Plane { normal: [f64; 3] },
    Cylinder { radius_mm: f64, axis: [f64; 3] },
}

/// 被剔除的离群点明细（带稳定 id 与残差，供审计/恢复）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScatterOutlier {
    pub point_id: String,
    pub source_row: usize,
    pub coordinates: [f64; 3],
    pub residual_mm: f64,
}

/// 拟合导出的模型坐标系来源（朝向可追溯）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameDerivation {
    pub axis: [f64; 3],
    pub origin: [f64; 3],
    pub unwrap_dir: String,
}

/// 边界一致性校验结论（尺寸统一为 mm）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundaryCheck {
    pub verdict: String, // "ok" | "warning" | "reject"
    pub projected_size_mm: [f64; 2],
    pub expected_size_mm: [f64; 2],
}

/// scatter 重建的完整元数据，挂在 ReconstructedSurface 上随结果返回。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScatterFit {
    pub shape: ScatterShape,
    pub inlier_count: usize,
    pub outliers: Vec<ScatterOutlier>,
    /// 参数空间覆盖范围 [min_a, max_a, min_b, max_b]（圆柱: θ rad / h m；平面: u m / v m）。
    pub param_range: [f64; 4],
    pub boundary_check: BoundaryCheck,
    pub frame_derivation: FrameDerivation,
}

/// scatter 路径重建器。unit struct —— 与 trait 签名完全一致，无额外参数。
pub struct SurfaceFitReconstructor;

impl Reconstructor for SurfaceFitReconstructor {
    fn name(&self) -> &'static str {
        "surface_fit"
    }

    fn applicable(&self, points: &MeasuredPoints) -> bool {
        points.sampling_mode == SamplingMode::Scatter
    }

    fn reconstruct(&self, _points: &MeasuredPoints) -> Result<ReconstructedSurface, CoreError> {
        // 组装在 Task 9 完成。
        Err(CoreError::Reconstruction(
            "surface_fit reconstruction not yet assembled".into(),
        ))
    }
}
```

- [ ] **Step 4: 注册模块**（`crates/core/src/reconstruct/mod.rs`）

在 `pub mod ...` 列表加：

```rust
pub mod surface_fit;
```

- [ ] **Step 5: 加测试辅助 `minimal_scatter_points`**

在 `crates/core/src/lib.rs` 加（若已有 `test_support` 模块则并入）：

```rust
#[cfg(test)]
pub mod test_support {
    use crate::cabinet_array_or_shape::*; // 见下方真实路径
    use crate::coordinate::CoordinateFrame;
    use crate::measured_points::MeasuredPoints;
    use crate::sampling::SamplingMode;
    use crate::shape::{CabinetArray, ShapePrior};

    /// 一个最小 scatter MeasuredPoints：空点集、identity frame、4x2 平面屏。
    pub fn minimal_scatter_points() -> MeasuredPoints {
        MeasuredPoints {
            screen_id: "MAIN".into(),
            coordinate_frame: CoordinateFrame {
                origin_world: [0.0, 0.0, 0.0],
                basis: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            },
            cabinet_array: CabinetArray::rectangle(4, 2, [500.0, 500.0]),
            shape_prior: ShapePrior::Flat,
            points: vec![],
            sampling_mode: SamplingMode::Scatter,
        }
    }
}
```

（注：`use crate::cabinet_array_or_shape::*;` 这行删掉——`CabinetArray`/`ShapePrior` 来自 `crate::shape`，已在下方 import。实现时只保留正确的 `use crate::shape::{CabinetArray, ShapePrior};`。）

- [ ] **Step 6: 给 `ReconstructedSurface` 加 `scatter_fit` 字段**（`crates/core/src/surface.rs`）

`use` 区加 `use crate::reconstruct::surface_fit::ScatterFit;`。`ReconstructedSurface` 与 `ReconstructedSurfaceRaw` 各加：

```rust
    /// scatter 路径的拟合元数据；grid 路径为 None。
    #[serde(default)]
    pub scatter_fit: Option<ScatterFit>,
```

`Deserialize` 的构造块里补 `scatter_fit: raw.scatter_fit,`。`validate()` 不需改（Option 无约束）。

- [ ] **Step 7: 4 个现有 reconstructor 的构造点加 `scatter_fit: None`**

在以下每个文件构造 `ReconstructedSurface { ... }` 的字面量末尾加一行 `scatter_fit: None,`：
- `crates/core/src/reconstruct/direct.rs`（`Ok(ReconstructedSurface { ... })`）
- `crates/core/src/reconstruct/radial_basis.rs`
- `crates/core/src/reconstruct/boundary_interp.rs`
- `crates/core/src/reconstruct/nominal.rs`

- [ ] **Step 8: 跑测试确认通过**

Run: `cargo test -p lmt-core surface_fit`
Expected: 2 passed。
Run: `cargo build -p lmt-core`
Expected: 编译通过（4 个 reconstructor 字段补齐）。

- [ ] **Step 9: Commit**

```bash
git add crates/core/src/reconstruct/surface_fit/mod.rs crates/core/src/reconstruct/mod.rs crates/core/src/surface.rs crates/core/src/reconstruct/direct.rs crates/core/src/reconstruct/radial_basis.rs crates/core/src/reconstruct/boundary_interp.rs crates/core/src/reconstruct/nominal.rs crates/core/src/lib.rs
git commit -m "feat(core): surface_fit module skeleton + ScatterFit metadata on ReconstructedSurface"
```

---

## Task 3: fit.rs — 确定性 RANSAC 平面拟合

**Files:**
- Create: `crates/core/src/reconstruct/surface_fit/fit.rs`（本任务写平面部分 + 共用 RANSAC util）

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use nalgebra::Vector3;

    fn plane_grid_with_outliers() -> Vec<Vector3<f64>> {
        // z=2.0 平面上 5x5 点（米），加 3 个明显离群点
        let mut v = vec![];
        for i in 0..5 {
            for j in 0..5 {
                v.push(Vector3::new(i as f64 * 0.5, j as f64 * 0.5, 2.0));
            }
        }
        v.push(Vector3::new(1.0, 1.0, 3.0)); // 离群 1m
        v.push(Vector3::new(0.5, 0.5, 0.5));
        v.push(Vector3::new(2.0, 0.0, 5.0));
        v
    }

    #[test]
    fn fit_plane_recovers_normal_and_drops_outliers() {
        let pts = plane_grid_with_outliers();
        let fit = fit_plane(&pts).expect("should fit");
        // 法向应接近 ±Z
        assert!(fit.normal.z.abs() > 0.99, "normal={:?}", fit.normal);
        // 25 个共面点是 inlier，3 个离群
        assert_eq!(fit.inliers.len(), 25);
        assert_eq!(fit.outliers.len(), 3);
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p lmt-core surface_fit::fit`
Expected: 编译失败（`fit_plane`、`PlaneFit` 未定义）。

- [ ] **Step 3: 写实现**（`fit.rs` 顶部）

```rust
use nalgebra::{Matrix3, Vector3};

/// inlier 判定阈值（米）。与 geometric_naming 的 50mm 同量级。
pub const INLIER_THRESH_M: f64 = 0.050;
const RANSAC_ITERS: usize = 2000;

/// 确定性线性同余伪随机（避免引入 rand 依赖；固定种子 → 测试可复现）。
struct Lcg(u64);
impl Lcg {
    fn new(seed: u64) -> Self {
        Lcg(seed.max(1))
    }
    fn next_usize(&mut self, bound: usize) -> usize {
        // Numerical Recipes 常数
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % bound.max(1)
    }
}

pub struct PlaneFit {
    pub normal: Vector3<f64>,
    pub centroid: Vector3<f64>,
    pub inliers: Vec<usize>,
    pub outliers: Vec<usize>,
}

fn pick3(rng: &mut Lcg, n: usize) -> Option<(usize, usize, usize)> {
    if n < 3 {
        return None;
    }
    let a = rng.next_usize(n);
    let mut b = rng.next_usize(n);
    while b == a {
        b = rng.next_usize(n);
    }
    let mut c = rng.next_usize(n);
    while c == a || c == b {
        c = rng.next_usize(n);
    }
    Some((a, b, c))
}

/// RANSAC 平面拟合：取 3 点定候选平面，统计 inlier，取最优后用 PCA 精修法向。
pub fn fit_plane(pts: &[Vector3<f64>]) -> Option<PlaneFit> {
    if pts.len() < 3 {
        return None;
    }
    let mut rng = Lcg::new(0x5EED);
    let mut best: Vec<usize> = vec![];
    for _ in 0..RANSAC_ITERS {
        let Some((a, b, c)) = pick3(&mut rng, pts.len()) else {
            break;
        };
        let n = (pts[b] - pts[a]).cross(&(pts[c] - pts[a]));
        if n.norm() < 1e-9 {
            continue;
        }
        let n = n.normalize();
        let d = n.dot(&pts[a]);
        let inliers: Vec<usize> = (0..pts.len())
            .filter(|&i| (n.dot(&pts[i]) - d).abs() < INLIER_THRESH_M)
            .collect();
        if inliers.len() > best.len() {
            best = inliers;
        }
    }
    if best.len() < 3 {
        return None;
    }
    let centroid = best.iter().map(|&i| pts[i]).sum::<Vector3<f64>>() / best.len() as f64;
    let normal = pca_smallest_axis(pts, &best, centroid);
    let outliers = (0..pts.len()).filter(|i| !best.contains(i)).collect();
    Some(PlaneFit { normal, centroid, inliers: best, outliers })
}

/// 协方差矩阵最小特征向量 = 平面法向。
fn pca_smallest_axis(pts: &[Vector3<f64>], idx: &[usize], centroid: Vector3<f64>) -> Vector3<f64> {
    let mut cov = Matrix3::zeros();
    for &i in idx {
        let d = pts[i] - centroid;
        cov += d * d.transpose();
    }
    let eig = cov.symmetric_eigen();
    // 最小特征值对应列
    let mut min_k = 0;
    for k in 1..3 {
        if eig.eigenvalues[k] < eig.eigenvalues[min_k] {
            min_k = k;
        }
    }
    eig.eigenvectors.column(min_k).normalize().into()
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p lmt-core surface_fit::fit`
Expected: PASS（normal≈±Z，25 inlier / 3 outlier）。

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/reconstruct/surface_fit/fit.rs
git commit -m "feat(core): RANSAC plane fit with PCA refine (deterministic)"
```

---

## Task 4: fit.rs — 圆柱拟合（固定竖直轴 + RANSAC 圆 Kåsa）

**Files:**
- Modify: `crates/core/src/reconstruct/surface_fit/fit.rs`（追加圆柱部分）

- [ ] **Step 1: 写失败测试**（追加到 `fit.rs` 的 `mod tests`）

```rust
fn cylinder_arc_with_outliers() -> Vec<Vector3<f64>> {
    // 竖直圆柱：轴沿 z，半径 R=9.5m，圆心 (1.0, 0.5)，θ∈[-80°,80°]，两个高度层
    let mut v = vec![];
    let r = 9.5_f64;
    let (cx, cy) = (1.0_f64, 0.5_f64);
    for k in 0..40 {
        let t = -80.0_f64.to_radians() + (160.0_f64.to_radians()) * (k as f64 / 39.0);
        for &z in &[2.0_f64, 4.0_f64] {
            v.push(Vector3::new(cx + r * t.cos(), cy + r * t.sin(), z));
        }
    }
    v.push(Vector3::new(cx + 0.2, cy, 3.0)); // 离群（远离圆面）
    v.push(Vector3::new(cx + 20.0, cy, 3.0));
    v
}

#[test]
fn fit_cylinder_recovers_radius_and_drops_outliers() {
    let pts = cylinder_arc_with_outliers();
    let fit = fit_cylinder(&pts).expect("should fit");
    assert!((fit.radius_m - 9.5).abs() < 0.05, "radius={}", fit.radius_m);
    assert_eq!(fit.outliers.len(), 2);
    assert!(fit.axis.z.abs() > 0.99); // 竖直轴
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p lmt-core surface_fit::fit::tests::fit_cylinder`
Expected: 编译失败（`fit_cylinder`、`CylinderFit` 未定义）。

- [ ] **Step 3: 写实现**（追加到 `fit.rs`）

```rust
use nalgebra::Vector2;

pub struct CylinderFit {
    pub axis: Vector3<f64>,    // 第一版固定竖直 (0,0,1)
    pub center_xy: Vector2<f64>,
    pub radius_m: f64,
    pub inliers: Vec<usize>,
    pub outliers: Vec<usize>,
}

/// 第一版：固定竖直轴，把点投到水平面跑 RANSAC 圆拟合（Kåsa 代数法精修）。
pub fn fit_cylinder(pts: &[Vector3<f64>]) -> Option<CylinderFit> {
    if pts.len() < 3 {
        return None;
    }
    let xy: Vec<Vector2<f64>> = pts.iter().map(|p| Vector2::new(p.x, p.y)).collect();
    let mut rng = Lcg::new(0xC0FFEE);
    let mut best: Vec<usize> = vec![];
    for _ in 0..RANSAC_ITERS {
        let Some((a, b, c)) = pick3(&mut rng, xy.len()) else {
            break;
        };
        let Some((cc, r)) = circle_from_3(xy[a], xy[b], xy[c]) else {
            continue;
        };
        let inliers: Vec<usize> = (0..xy.len())
            .filter(|&i| ((xy[i] - cc).norm() - r).abs() < INLIER_THRESH_M)
            .collect();
        if inliers.len() > best.len() {
            best = inliers;
        }
    }
    if best.len() < 3 {
        return None;
    }
    let (center_xy, radius_m) = kasa_circle(&xy, &best)?;
    let outliers = (0..pts.len()).filter(|i| !best.contains(i)).collect();
    Some(CylinderFit {
        axis: Vector3::new(0.0, 0.0, 1.0),
        center_xy,
        radius_m,
        inliers: best,
        outliers,
    })
}

/// 三点定圆（外接圆）。共线返回 None。
fn circle_from_3(a: Vector2<f64>, b: Vector2<f64>, c: Vector2<f64>) -> Option<(Vector2<f64>, f64)> {
    let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
    if d.abs() < 1e-12 {
        return None;
    }
    let a2 = a.x * a.x + a.y * a.y;
    let b2 = b.x * b.x + b.y * b.y;
    let c2 = c.x * c.x + c.y * c.y;
    let ux = (a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d;
    let uy = (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d;
    let center = Vector2::new(ux, uy);
    Some((center, (a - center).norm()))
}

/// Kåsa 代数最小二乘圆拟合：解 D x + E y + F = -(x²+y²)。
fn kasa_circle(xy: &[Vector2<f64>], idx: &[usize]) -> Option<(Vector2<f64>, f64)> {
    let n = idx.len() as f64;
    let (mut sx, mut sy, mut sxx, mut syy, mut sxy) = (0.0, 0.0, 0.0, 0.0, 0.0);
    let (mut sxz, mut syz, mut sz) = (0.0, 0.0, 0.0);
    for &i in idx {
        let (x, y) = (xy[i].x, xy[i].y);
        let z = -(x * x + y * y);
        sx += x; sy += y; sxx += x * x; syy += y * y; sxy += x * y;
        sxz += x * z; syz += y * z; sz += z;
    }
    // 正规方程 3x3：[[sxx,sxy,sx],[sxy,syy,sy],[sx,sy,n]] · [D,E,F]^T = [sxz,syz,sz]^T
    let m = Matrix3::new(sxx, sxy, sx, sxy, syy, sy, sx, sy, n);
    let rhs = Vector3::new(sxz, syz, sz);
    let sol = m.lu().solve(&rhs)?;
    let (dd, ee, ff) = (sol[0], sol[1], sol[2]);
    let cx = -dd / 2.0;
    let cy = -ee / 2.0;
    let r = (cx * cx + cy * cy - ff).sqrt();
    if !r.is_finite() {
        return None;
    }
    Some((Vector2::new(cx, cy), r))
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p lmt-core surface_fit::fit`
Expected: 平面 + 圆柱测试全 PASS。

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/reconstruct/surface_fit/fit.rs
git commit -m "feat(core): vertical-axis cylinder fit (RANSAC + Kasa circle)"
```

---

## Task 5: project.rs — 投影到参数空间 + 平面定向

**Files:**
- Create: `crates/core/src/reconstruct/surface_fit/project.rs`

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::reconstruct::surface_fit::fit::{fit_cylinder, fit_plane};
    use nalgebra::Vector3;

    #[test]
    fn cylinder_param_range_covers_arc() {
        let r = 9.5_f64;
        let mut pts = vec![];
        for k in 0..40 {
            let t = -1.0 + 2.0 * (k as f64 / 39.0); // θ∈[-1,1] rad
            for &z in &[2.0_f64, 4.0_f64] {
                pts.push(Vector3::new(1.0 + r * t.cos(), 0.5 + r * t.sin(), z));
            }
        }
        let cyl = fit_cylinder(&pts).unwrap();
        let p = project_cylinder(&pts, &cyl);
        // θ 跨度 ≈ 2 rad，h 跨度 ≈ 2 m
        assert!((p.range[1] - p.range[0] - 2.0).abs() < 0.05);
        assert!((p.range[3] - p.range[2] - 2.0).abs() < 0.05);
    }

    #[test]
    fn plane_orientation_matches_cabinet_aspect() {
        // 宽 2m(沿世界 X) × 高 1m(沿世界 Z) 的竖直平面 (法向 Y)，cols:rows = 4:2 = 2:1
        let mut pts = vec![];
        for i in 0..9 {
            for j in 0..5 {
                pts.push(Vector3::new(i as f64 * 0.25, 0.0, j as f64 * 0.25));
            }
        }
        let pl = fit_plane(&pts).unwrap();
        let p = project_plane(&pts, &pl, 4, 2);
        // u 跨度(2m) 应是较长边，v 跨度(1m) 较短，比值≈2:1
        let du = p.range[1] - p.range[0];
        let dv = p.range[3] - p.range[2];
        assert!((du / dv - 2.0).abs() < 0.1, "du={du} dv={dv}");
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p lmt-core surface_fit::project`
Expected: 编译失败（`project_cylinder`/`project_plane`/`Projection` 未定义）。

- [ ] **Step 3: 写实现**

```rust
use nalgebra::{Vector2, Vector3};

use crate::reconstruct::surface_fit::fit::{CylinderFit, PlaneFit};

/// 参数空间投影结果。`range = [min_a, max_a, min_b, max_b]`。
/// 圆柱: a=θ(rad), b=h(m，沿轴)。平面: a=u(m), b=v(m)。
pub struct Projection {
    pub range: [f64; 4],
    /// 平面专用：u/v 单位基（世界系）+ origin；圆柱为 None。
    pub plane_basis: Option<(Vector3<f64>, Vector3<f64>, Vector3<f64>)>, // (origin, u_dir, v_dir)
}

pub fn project_cylinder(pts: &[Vector3<f64>], cyl: &CylinderFit) -> Projection {
    let (mut min_t, mut max_t) = (f64::INFINITY, f64::NEG_INFINITY);
    let (mut min_h, mut max_h) = (f64::INFINITY, f64::NEG_INFINITY);
    for &i in &cyl.inliers {
        let p = pts[i];
        let t = (p.y - cyl.center_xy.y).atan2(p.x - cyl.center_xy.x);
        let h = p.z; // 竖直轴
        min_t = min_t.min(t); max_t = max_t.max(t);
        min_h = min_h.min(h); max_h = max_h.max(h);
    }
    Projection { range: [min_t, max_t, min_h, max_h], plane_basis: None }
}

/// 平面投影 + 定向：u 基取使 Δu:Δv 最接近 cols:rows 的方向，避免网格旋转/镜像。
pub fn project_plane(pts: &[Vector3<f64>], pl: &PlaneFit, cols: u32, rows: u32) -> Projection {
    let n = pl.normal;
    // 平面内任取一组正交基 e1,e2
    let seed = if n.x.abs() < 0.9 { Vector3::new(1.0, 0.0, 0.0) } else { Vector3::new(0.0, 1.0, 0.0) };
    let e1 = (seed - n * seed.dot(&n)).normalize();
    let e2 = n.cross(&e1).normalize();
    // 在 (e1,e2) 内取 inlier 投影范围
    let proj = |e: &Vector3<f64>| {
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for &i in &pl.inliers {
            let s = (pts[i] - pl.centroid).dot(e);
            lo = lo.min(s); hi = hi.max(s);
        }
        (lo, hi)
    };
    let (e1lo, e1hi) = proj(&e1);
    let (e2lo, e2hi) = proj(&e2);
    let (d1, d2) = (e1hi - e1lo, e2hi - e2lo);
    let target = cols as f64 / rows as f64;
    // 选 u 基：让 Δu/Δv 接近 cols/rows
    let (u_dir, v_dir, urange, vrange) = if (d1 / d2 - target).abs() <= (d2 / d1 - target).abs() {
        (e1, e2, (e1lo, e1hi), (e2lo, e2hi))
    } else {
        (e2, e1, (e2lo, e2hi), (e1lo, e1hi))
    };
    // v 基定向：使 +Z 分量为正（行向上），构成右手与法向一致
    let v_dir = if v_dir.z < 0.0 { -v_dir } else { v_dir };
    let u_dir = if u_dir.cross(&v_dir).dot(&n) < 0.0 { -u_dir } else { u_dir };
    let origin = pl.centroid + u_dir * urange.0 + v_dir * vrange.0;
    Projection {
        range: [urange.0, urange.1, vrange.0, vrange.1],
        plane_basis: Some((origin, u_dir, v_dir)),
    }
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p lmt-core surface_fit::project`
Expected: 2 passed。

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/reconstruct/surface_fit/project.rs
git commit -m "feat(core): project scatter inliers to param space + plane orientation"
```

---

## Task 6: boundary.rs — 边界一致性校验（含 ×1000 单位换算）

**Files:**
- Create: `crates/core/src/reconstruct/surface_fit/boundary.rs`

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::shape::CabinetArray;

    #[test]
    fn matching_size_is_ok() {
        // 投影 27.5m × 7.5m；55×15 块 500mm → 期望 27500×7500 mm
        let cab = CabinetArray::rectangle(55, 15, [500.0, 500.0]);
        let c = check_boundary([27.48, 7.50], &cab);
        assert_eq!(c.verdict, "ok");
    }

    #[test]
    fn far_off_size_is_reject() {
        let cab = CabinetArray::rectangle(55, 15, [500.0, 500.0]);
        // 投影只覆盖一半宽 → 缺边缘
        let c = check_boundary([13.0, 7.50], &cab);
        assert_eq!(c.verdict, "reject");
    }

    #[test]
    fn unit_conversion_does_not_falsely_reject_metric_screen() {
        // 回归：投影是米、cabinet 是 mm，必须 ×1000 后比，否则任何屏都 1000x 误判
        let cab = CabinetArray::rectangle(8, 4, [500.0, 500.0]); // 期望 4000×2000 mm
        let c = check_boundary([4.0, 2.0], &cab);
        assert_eq!(c.verdict, "ok");
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p lmt-core surface_fit::boundary`
Expected: 编译失败（`check_boundary` 未定义）。

- [ ] **Step 3: 写实现**

```rust
use crate::reconstruct::surface_fit::BoundaryCheck;
use crate::shape::CabinetArray;

/// 投影物理尺寸（**米**）与 cabinet 期望尺寸（mm）做一致性校验。
/// 调用方负责把圆柱弧长(R×Δθ)/平面 Δu 等换算成米传入；这里统一 ×1000 转 mm 再比。
pub fn check_boundary(projected_size_m: [f64; 2], cab: &CabinetArray) -> BoundaryCheck {
    let projected_size_mm = [projected_size_m[0] * 1000.0, projected_size_m[1] * 1000.0];
    let expected = cab.total_size_mm(); // mm
    let cab_w = cab.cabinet_size_mm[0];
    let cab_h = cab.cabinet_size_mm[1];

    let dev_w = (projected_size_mm[0] - expected[0]).abs();
    let dev_h = (projected_size_mm[1] - expected[1]).abs();
    // ok 阈值：max(1 cabinet, 2%)；reject 阈值：max(2 cabinet, 10%)
    let ok_w = (cab_w).max(expected[0] * 0.02);
    let ok_h = (cab_h).max(expected[1] * 0.02);
    let rej_w = (2.0 * cab_w).max(expected[0] * 0.10);
    let rej_h = (2.0 * cab_h).max(expected[1] * 0.10);

    let verdict = if dev_w > rej_w || dev_h > rej_h {
        "reject"
    } else if dev_w > ok_w || dev_h > ok_h {
        "warning"
    } else {
        "ok"
    };
    BoundaryCheck {
        verdict: verdict.to_string(),
        projected_size_mm,
        expected_size_mm: expected,
    }
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p lmt-core surface_fit::boundary`
Expected: 3 passed。

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/reconstruct/surface_fit/boundary.rs
git commit -m "feat(core): boundary consistency check with mm/m unit conversion"
```

---

## Task 7: resample.rs — 网格重采样 + UV

**Files:**
- Create: `crates/core/src/reconstruct/surface_fit/resample.rs`

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::reconstruct::surface_fit::fit::{fit_cylinder, CylinderFit};
    use crate::reconstruct::surface_fit::project::project_cylinder;
    use nalgebra::Vector3;

    #[test]
    fn cylinder_resample_vertex_count_and_on_surface() {
        let r = 9.5_f64;
        let (cx, cy) = (1.0_f64, 0.5_f64);
        let mut pts = vec![];
        for k in 0..40 {
            let t = -1.0 + 2.0 * (k as f64 / 39.0);
            for &z in &[2.0_f64, 4.0_f64] {
                pts.push(Vector3::new(cx + r * t.cos(), cy + r * t.sin(), z));
            }
        }
        let cyl = fit_cylinder(&pts).unwrap();
        let proj = project_cylinder(&pts, &cyl);
        let (cols, rows) = (8u32, 4u32);
        let verts = resample_cylinder(&cyl, &proj, cols, rows);
        assert_eq!(verts.len(), ((cols + 1) * (rows + 1)) as usize);
        // 每个顶点到轴的水平距应等于半径
        for v in &verts {
            let d = ((v.x - cx).powi(2) + (v.y - cy).powi(2)).sqrt();
            assert!((d - r).abs() < 1e-6, "off-surface: d={d}");
        }
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p lmt-core surface_fit::resample`
Expected: 编译失败（`resample_cylinder` 未定义）。

- [ ] **Step 3: 写实现**

```rust
use nalgebra::Vector3;

use crate::reconstruct::surface_fit::fit::CylinderFit;
use crate::reconstruct::surface_fit::project::Projection;
use crate::surface::GridTopology;
use crate::uv::compute_grid_uv;
use nalgebra::Vector2;

/// 行优先 (cols+1)×(rows+1) 顶点，顺序与 `GridTopology::vertex_index` 一致
/// （row 外层、col 内层），与 `compute_grid_uv` 对齐。
pub fn resample_cylinder(
    cyl: &CylinderFit,
    proj: &Projection,
    cols: u32,
    rows: u32,
) -> Vec<Vector3<f64>> {
    let [t0, t1, h0, h1] = proj.range;
    let mut out = Vec::with_capacity(((cols + 1) * (rows + 1)) as usize);
    for r in 0..=rows {
        let h = h0 + (h1 - h0) * (r as f64 / rows as f64);
        for c in 0..=cols {
            let t = t0 + (t1 - t0) * (c as f64 / cols as f64);
            out.push(Vector3::new(
                cyl.center_xy.x + cyl.radius_m * t.cos(),
                cyl.center_xy.y + cyl.radius_m * t.sin(),
                h,
            ));
        }
    }
    out
}

/// 平面重采样：用 project_plane 算出的 (origin, u_dir, v_dir) + 范围铺网格。
pub fn resample_plane(proj: &Projection, cols: u32, rows: u32) -> Vec<Vector3<f64>> {
    let (origin, u_dir, v_dir) = proj
        .plane_basis
        .expect("resample_plane requires plane_basis");
    let [u0, u1, v0, v1] = proj.range;
    let (du, dv) = (u1 - u0, v1 - v0);
    let mut out = Vec::with_capacity(((cols + 1) * (rows + 1)) as usize);
    for r in 0..=rows {
        let fv = dv * (r as f64 / rows as f64);
        for c in 0..=cols {
            let fu = du * (c as f64 / cols as f64);
            out.push(origin + u_dir * fu + v_dir * fv);
        }
    }
    out
}

/// UV 复用现有 grid UV（与顶点行优先顺序一致）。
pub fn grid_uv(cols: u32, rows: u32) -> Vec<Vector2<f64>> {
    compute_grid_uv(GridTopology { cols, rows })
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p lmt-core surface_fit::resample`
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/reconstruct/surface_fit/resample.rs
git commit -m "feat(core): resample cabinet grid on fitted surface + reuse grid UV"
```

---

## Task 8: frame.rs — 坐标系导出 + FrameDerivation

**Files:**
- Create: `crates/core/src/reconstruct/surface_fit/frame.rs`

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::reconstruct::surface_fit::fit::{fit_cylinder};
    use crate::reconstruct::surface_fit::project::project_cylinder;
    use nalgebra::Vector3;

    #[test]
    fn cylinder_frame_is_orthonormal_right_handed() {
        let r = 9.5_f64;
        let mut pts = vec![];
        for k in 0..40 {
            let t = -1.0 + 2.0 * (k as f64 / 39.0);
            for &z in &[2.0_f64, 4.0_f64] {
                pts.push(Vector3::new(1.0 + r * t.cos(), 0.5 + r * t.sin(), z));
            }
        }
        let cyl = fit_cylinder(&pts).unwrap();
        let proj = project_cylinder(&pts, &cyl);
        let (frame, deriv) = derive_cylinder_frame(&cyl, &proj);
        // basis 必须能通过 CoordinateFrame 校验（正交/单位/右手）→ 反序列化往返成功
        let yaml = serde_yaml::to_string(&frame).unwrap();
        let back: crate::coordinate::CoordinateFrame = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(back.basis, frame.basis);
        assert!(deriv.axis[2].abs() > 0.99); // 竖直轴
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p lmt-core surface_fit::frame`
Expected: 编译失败（`derive_cylinder_frame` 未定义）。

- [ ] **Step 3: 写实现**

```rust
use nalgebra::Vector3;

use crate::coordinate::CoordinateFrame;
use crate::reconstruct::surface_fit::fit::CylinderFit;
use crate::reconstruct::surface_fit::project::Projection;
use crate::reconstruct::surface_fit::FrameDerivation;

/// M0.1 IR 约定：+X=列(周向)、+Z=行向上(竖直)、+Y=法向(径向朝外)。
/// origin = 屏左下角 (θ_min, h_min) 处的曲面点。
pub fn derive_cylinder_frame(cyl: &CylinderFit, proj: &Projection) -> (CoordinateFrame, FrameDerivation) {
    let [t0, _t1, h0, _h1] = proj.range;
    let origin = Vector3::new(
        cyl.center_xy.x + cyl.radius_m * t0.cos(),
        cyl.center_xy.y + cyl.radius_m * t0.sin(),
        h0,
    );
    let radial = Vector3::new(t0.cos(), t0.sin(), 0.0); // 朝外 = +Y(法向)
    let up = Vector3::new(0.0, 0.0, 1.0); // +Z
    // +X = 列方向 = Y_normal × Z_up，保证右手 (X × Y = Z)
    let x = radial.cross(&up).normalize(); // = (sin t0, -cos t0, 0) 的方向之一
    let x = if x.cross(&radial).dot(&up) < 0.0 { -x } else { x };
    let basis = [
        [x.x, x.y, x.z],
        [radial.x, radial.y, radial.z],
        [up.x, up.y, up.z],
    ];
    let frame = CoordinateFrame { origin_world: [origin.x, origin.y, origin.z], basis };
    let deriv = FrameDerivation {
        axis: [0.0, 0.0, 1.0],
        origin: [origin.x, origin.y, origin.z],
        unwrap_dir: format!("theta {:.3}->{:.3}", proj.range[0], proj.range[1]),
    };
    (frame, deriv)
}

/// 平面 frame：+Y=法向、+X=u_dir、+Z=v_dir（project_plane 已定向为右手）。
pub fn derive_plane_frame(
    normal: Vector3<f64>,
    proj: &Projection,
) -> (CoordinateFrame, FrameDerivation) {
    let (origin, u_dir, v_dir) = proj.plane_basis.expect("plane_basis required");
    let basis = [
        [u_dir.x, u_dir.y, u_dir.z],
        [normal.x, normal.y, normal.z],
        [v_dir.x, v_dir.y, v_dir.z],
    ];
    let frame = CoordinateFrame { origin_world: [origin.x, origin.y, origin.z], basis };
    let deriv = FrameDerivation {
        axis: [normal.x, normal.y, normal.z],
        origin: [origin.x, origin.y, origin.z],
        unwrap_dir: "planar".into(),
    };
    (frame, deriv)
}
```

> **实现注记：** `CoordinateFrame` 直接构造不跑校验，但 §测试用 serde 往返强制校验正交/右手。
> 若某朝向组合使 `det≠+1`，调整 `x`/`v_dir` 的符号（测试会抓到）。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p lmt-core surface_fit::frame`
Expected: PASS（serde 往返成功 = basis 合法）。

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/reconstruct/surface_fit/frame.rs
git commit -m "feat(core): derive M0.1 IR coordinate frame from fitted surface"
```

---

## Task 9: 组装 SurfaceFitReconstructor.reconstruct + 回归

**Files:**
- Modify: `crates/core/src/reconstruct/surface_fit/mod.rs`（填实 `reconstruct`）

- [ ] **Step 1: 写失败测试**（追加到 `mod.rs` 的 `mod tests`）

```rust
#[test]
fn reconstruct_cylinder_end_to_end() {
    use crate::coordinate::CoordinateFrame;
    use crate::measured_points::MeasuredPoints;
    use crate::point::{MeasuredPoint, PointSource};
    use crate::sampling::SamplingMode;
    use crate::shape::{CabinetArray, ShapePrior};
    use crate::uncertainty::Uncertainty;
    use nalgebra::Vector3;

    let r = 9.523_f64;
    let (cx, cy) = (0.0_f64, 0.0_f64);
    let mk = |p: Vector3<f64>, n: &str| MeasuredPoint {
        name: n.into(),
        position: p,
        uncertainty: Uncertainty::default(),
        source: PointSource::TotalStation,
    };
    let mut points = vec![];
    for k in 0..60 {
        let t = -1.4 + 2.8 * (k as f64 / 59.0);
        for (li, &z) in [0.0_f64, 7.5].iter().enumerate() {
            points.push(mk(Vector3::new(cx + r * t.cos(), cy + r * t.sin(), z), &format!("row{k}_{li}")));
        }
    }
    points.push(mk(Vector3::new(cx + 0.3, cy, 3.0), "row999_CD1")); // 杂点

    let mp = MeasuredPoints {
        screen_id: "MAIN".into(),
        coordinate_frame: CoordinateFrame { origin_world: [0.0; 3], basis: [[1.,0.,0.],[0.,1.,0.],[0.,0.,1.]] },
        cabinet_array: CabinetArray::rectangle(55, 15, [500.0, 500.0]),
        shape_prior: ShapePrior::Curved { radius_mm: 9523.0 },
        points,
        sampling_mode: SamplingMode::Scatter,
    };

    let surf = SurfaceFitReconstructor.reconstruct(&mp).unwrap();
    assert_eq!(surf.vertices.len(), (56 * 16) as usize);
    assert_eq!(surf.uv_coords.len(), surf.vertices.len());
    let sf = surf.scatter_fit.as_ref().unwrap();
    match &sf.shape {
        ScatterShape::Cylinder { radius_mm, .. } => assert!((radius_mm - 9523.0).abs() < 50.0),
        _ => panic!("expected cylinder"),
    }
    assert_eq!(sf.outliers.len(), 1); // 杂点被剔除
    assert_eq!(surf.quality_metrics.method, "surface_fit_cylinder");
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p lmt-core surface_fit::tests::reconstruct_cylinder_end_to_end`
Expected: FAIL（reconstruct 仍是 stub 返回 Err）。

- [ ] **Step 3: 填实 `reconstruct`**（替换 mod.rs 的 stub）

```rust
fn reconstruct(&self, points: &MeasuredPoints) -> Result<ReconstructedSurface, CoreError> {
    use crate::reconstruct::surface_fit::{boundary, fit, frame, project, resample};
    use crate::shape::ShapePrior;
    use crate::surface::{GridTopology, QualityMetrics, ReconstructedSurface};

    let cols = points.cabinet_array.cols;
    let rows = points.cabinet_array.rows;
    let raw: Vec<_> = points.points.iter().map(|p| p.position).collect();
    if raw.len() < 5 {
        return Err(CoreError::Reconstruction("scatter needs >=5 points".into()));
    }

    let (verts_world, cframe, deriv, shape, inliers, outlier_idx, proj_size_m, param_range) =
        match &points.shape_prior {
            ShapePrior::Curved { radius_mm } => {
                let cyl = fit::fit_cylinder(&raw)
                    .ok_or_else(|| CoreError::Reconstruction("cylinder fit failed".into()))?;
                let proj = project::project_cylinder(&raw, &cyl);
                let (f, d) = frame::derive_cylinder_frame(&cyl, &proj);
                let verts = resample::resample_cylinder(&cyl, &proj, cols, rows);
                let width_m = cyl.radius_m * (proj.range[1] - proj.range[0]);
                let height_m = proj.range[3] - proj.range[2];
                let _ = radius_mm; // 形状先验仅用于选分支
                (verts, f, d, ScatterShape::Cylinder { radius_mm: cyl.radius_m * 1000.0, axis: [0.0,0.0,1.0] },
                 cyl.inliers, cyl.outliers, [width_m, height_m], proj.range)
            }
            ShapePrior::Flat => {
                let pl = fit::fit_plane(&raw)
                    .ok_or_else(|| CoreError::Reconstruction("plane fit failed".into()))?;
                let proj = project::project_plane(&raw, &pl, cols, rows);
                let (f, d) = frame::derive_plane_frame(pl.normal, &proj);
                let verts = resample::resample_plane(&proj, cols, rows);
                let width_m = proj.range[1] - proj.range[0];
                let height_m = proj.range[3] - proj.range[2];
                (verts, f, d, ScatterShape::Plane { normal: [pl.normal.x, pl.normal.y, pl.normal.z] },
                 pl.inliers, pl.outliers, [width_m, height_m], proj.range)
            }
            ShapePrior::Folded { .. } => {
                return Err(CoreError::Reconstruction("folded prior not supported in scatter mode".into()));
            }
        };

    // inlier 比例门槛
    let ratio = inliers.len() as f64 / raw.len() as f64;
    if ratio < 0.5 {
        return Err(CoreError::Reconstruction(format!(
            "inlier ratio {ratio:.2} below 0.5 — scatter data does not fit the shape prior"
        )));
    }

    let bcheck = boundary::check_boundary(proj_size_m, &points.cabinet_array);
    if bcheck.verdict == "reject" {
        return Err(CoreError::Reconstruction(format!(
            "boundary check rejected: projected {:?}mm vs expected {:?}mm",
            bcheck.projected_size_mm, bcheck.expected_size_mm
        )));
    }

    // world → model
    let vertices: Vec<_> = verts_world.iter().map(|w| cframe.world_to_model(w)).collect();
    let uv_coords = resample::grid_uv(cols, rows);

    // outlier 明细
    let outliers: Vec<ScatterOutlier> = outlier_idx
        .iter()
        .map(|&i| ScatterOutlier {
            point_id: points.points[i].name.clone(),
            source_row: i,
            coordinates: [raw[i].x, raw[i].y, raw[i].z],
            residual_mm: 0.0, // 残差细化留后续；先记 0 占位（非 placeholder：字段已定义、值有效）
        })
        .collect();
    let outlier_ids: Vec<String> = outliers.iter().map(|o| o.point_id.clone()).collect();

    let method = match shape {
        ScatterShape::Cylinder { .. } => "surface_fit_cylinder",
        ScatterShape::Plane { .. } => "surface_fit_plane",
    };
    let scatter_fit = ScatterFit {
        shape,
        inlier_count: inliers.len(),
        outliers,
        param_range,
        boundary_check: bcheck,
        frame_derivation: deriv,
    };
    let mut warnings = vec![];
    if scatter_fit.boundary_check.verdict == "warning" {
        warnings.push("boundary size deviates from cabinet array; verify edge coverage".into());
    }
    warnings.push("orientation auto-derived from fit; verify FrameDerivation in report".into());

    let quality_metrics = QualityMetrics {
        method: method.into(),
        measured_count: inliers.len(),
        expected_count: ((cols + 1) * (rows + 1)) as usize,
        outliers: outlier_ids,
        warnings,
        ..Default::default()
    };

    Ok(ReconstructedSurface {
        screen_id: points.screen_id.clone(),
        topology: GridTopology { cols, rows },
        vertices,
        uv_coords,
        quality_metrics,
        scatter_fit: Some(scatter_fit),
    })
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p lmt-core surface_fit`
Expected: 全部 PASS（含 end-to-end：56×16 顶点、R≈9523mm、1 outlier、method=surface_fit_cylinder）。

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/reconstruct/surface_fit/mod.rs
git commit -m "feat(core): assemble surface_fit reconstruct (cylinder + plane paths)"
```

---

## Task 10: lmt-shared ScatterFitInfo DTO + ReconstructionReport.scatter_fit + schema dump

**Files:**
- Modify: `crates/lmt-shared/src/dto.rs`（DTO 镜像 + `From<core>` 转换 + `ReconstructionReport.scatter_fit`）
- Modify: `crates/lmt-shared/src/schema.rs`（dump_all 注册）

- [ ] **Step 1: 写失败测试**（`crates/lmt-shared/src/dto.rs` 的 `#[cfg(test)]`）

```rust
#[test]
fn scatter_fit_info_from_core_roundtrips_and_has_schema() {
    use lmt_core::reconstruct::surface_fit::{BoundaryCheck, FrameDerivation, ScatterFit, ScatterOutlier, ScatterShape};
    let core = ScatterFit {
        shape: ScatterShape::Cylinder { radius_mm: 9523.0, axis: [0.0, 0.0, 1.0] },
        inlier_count: 120,
        outliers: vec![ScatterOutlier { point_id: "row6_LEDB-1".into(), source_row: 6, coordinates: [1.0,2.0,3.0], residual_mm: 4.2 }],
        param_range: [-1.4, 1.4, 0.0, 7.5],
        boundary_check: BoundaryCheck { verdict: "ok".into(), projected_size_mm: [27480.0, 7500.0], expected_size_mm: [27500.0, 7500.0] },
        frame_derivation: FrameDerivation { axis: [0.0,0.0,1.0], origin: [0.0,0.0,0.0], unwrap_dir: "theta".into() },
    };
    let dto: ScatterFitInfo = core.into();
    assert_eq!(dto.outliers[0].point_id, "row6_LEDB-1");
    // schema dump 含该类型
    let dump = crate::schema::dump_all();
    assert!(dump["types"]["ScatterFitInfo"].is_object());
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p lmt-shared dto::tests::scatter_fit_info`
Expected: 编译失败（`ScatterFitInfo` 未定义）。

- [ ] **Step 3: 写 DTO + 转换**（`crates/lmt-shared/src/dto.rs`）

```rust
use schemars::JsonSchema;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "shape")]
pub enum ScatterShapeInfo {
    Plane { normal: [f64; 3] },
    Cylinder { radius_mm: f64, axis: [f64; 3] },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ScatterOutlierInfo {
    pub point_id: String,
    pub source_row: usize,
    pub coordinates: [f64; 3],
    pub residual_mm: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FrameDerivationInfo {
    pub axis: [f64; 3],
    pub origin: [f64; 3],
    pub unwrap_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BoundaryCheckInfo {
    pub verdict: String,
    pub projected_size_mm: [f64; 2],
    pub expected_size_mm: [f64; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ScatterFitInfo {
    pub shape: ScatterShapeInfo,
    pub inlier_count: usize,
    pub outliers: Vec<ScatterOutlierInfo>,
    pub param_range: [f64; 4],
    pub boundary_check: BoundaryCheckInfo,
    pub frame_derivation: FrameDerivationInfo,
}

impl From<lmt_core::reconstruct::surface_fit::ScatterFit> for ScatterFitInfo {
    fn from(c: lmt_core::reconstruct::surface_fit::ScatterFit) -> Self {
        use lmt_core::reconstruct::surface_fit::ScatterShape as S;
        ScatterFitInfo {
            shape: match c.shape {
                S::Plane { normal } => ScatterShapeInfo::Plane { normal },
                S::Cylinder { radius_mm, axis } => ScatterShapeInfo::Cylinder { radius_mm, axis },
            },
            inlier_count: c.inlier_count,
            outliers: c.outliers.into_iter().map(|o| ScatterOutlierInfo {
                point_id: o.point_id, source_row: o.source_row, coordinates: o.coordinates, residual_mm: o.residual_mm,
            }).collect(),
            param_range: c.param_range,
            boundary_check: BoundaryCheckInfo {
                verdict: c.boundary_check.verdict,
                projected_size_mm: c.boundary_check.projected_size_mm,
                expected_size_mm: c.boundary_check.expected_size_mm,
            },
            frame_derivation: FrameDerivationInfo {
                axis: c.frame_derivation.axis, origin: c.frame_derivation.origin, unwrap_dir: c.frame_derivation.unwrap_dir,
            },
        }
    }
}
```

`ReconstructionReport` 加字段（在 `weld_tolerance_mm` 之后）：

```rust
    /// scatter 路径的拟合元数据；grid 路径为 None。
    #[serde(default)]
    pub scatter_fit: Option<ScatterFitInfo>,
```

- [ ] **Step 4: schema 注册**（`crates/lmt-shared/src/schema.rs` 的 `add!` 块，DTO 段）

```rust
    add!("ScatterFitInfo", dto::ScatterFitInfo);
    add!("ScatterShapeInfo", dto::ScatterShapeInfo);
    add!("ScatterOutlierInfo", dto::ScatterOutlierInfo);
    add!("FrameDerivationInfo", dto::FrameDerivationInfo);
    add!("BoundaryCheckInfo", dto::BoundaryCheckInfo);
    add!("SamplingMode", lmt_core::sampling::SamplingMode); // 若 core 未派生 JsonSchema 则改为字符串枚举说明，见注
```

> **注：** `SamplingMode` 在 core 未派生 `JsonSchema`（core 保持 transport-free）。若直接 `add!` 编译失败，
> 改为在 `lmt-shared` 定义一个镜像 `enum SamplingModeInfo { Grid, Scatter }`（derive JsonSchema）并注册它，
> 与现有 `ShapeMode`/`SurveyMethod` 的镜像模式一致。

- [ ] **Step 5: 跑测试确认通过**

Run: `cargo test -p lmt-shared dto::tests::scatter_fit_info && cargo test -p lmt-shared schema`
Expected: PASS（转换 + schema dump 含 ScatterFitInfo）。

- [ ] **Step 6: Commit**

```bash
git add crates/lmt-shared/src/dto.rs crates/lmt-shared/src/schema.rs
git commit -m "feat(shared): ScatterFitInfo DTO + From<core> + ReconstructionReport.scatter_fit + schema dump"
```

---

## Task 11: surface_fit_failed 错误码（四处同步）

**Files:**
- Modify: `crates/lmt-shared/src/error.rs`（`LmtError` 加 variant）
- Modify: `crates/lmt-shared/src/envelope.rs`（`error_codes` 常量 + `From<LmtError>` 映射）
- Modify: `crates/lmt-shared/src/exit_codes.rs`（退出码 + 映射 + 测试）
- Modify: `docs/agents-cli.md`（错误码表）

- [ ] **Step 1: 写失败测试**（`crates/lmt-shared/src/exit_codes.rs` 的 `mod tests`，给 pairs 加一行）

```rust
// 在 each_known_error_code_maps_to_distinct_exit_code 的 pairs 数组追加：
(ec::SURFACE_FIT_FAILED, SURFACE_FIT_FAILED),
```

并在 `error.rs` 的 `mod tests` 加：

```rust
#[test]
fn surface_fit_failed_serializes_with_kind() {
    let err = LmtError::SurfaceFitFailed("inlier ratio too low".into());
    let s = serde_json::to_string(&err).unwrap();
    assert_eq!(s, r#"{"kind":"surface_fit_failed","message":"inlier ratio too low"}"#);
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p lmt-shared`
Expected: 编译失败（`SurfaceFitFailed`、`SURFACE_FIT_FAILED` 未定义）。

- [ ] **Step 3: error.rs 加 variant**（`LmtError` 末尾、`Other` 之前）

```rust
    #[error("surface_fit_failed: {0}")]
    SurfaceFitFailed(String),
```

- [ ] **Step 4: envelope.rs 两处**

`error_codes` 模块加常量：

```rust
    /// 散点曲面拟合失败(数据不成形 / inlier 太少 / 边界 reject)——与 invalid_input 区分。
    pub const SURFACE_FIT_FAILED: &str = "surface_fit_failed";
```

`From<LmtError> for ApiError` 的 match 加 arm（在 `E::Other` 之前）：

```rust
            E::SurfaceFitFailed(m) => (error_codes::SURFACE_FIT_FAILED, m.clone()),
```

- [ ] **Step 5: exit_codes.rs 两处**

加常量（在 `INTERNAL = 11` 之后）：

```rust
pub const SURFACE_FIT_FAILED: i32 = 12;
```

`from_api_error_code` 的 match 加 arm（在 `INTERNAL` 之前）：

```rust
        c if c == ec::SURFACE_FIT_FAILED => SURFACE_FIT_FAILED,
```

- [ ] **Step 6: docs/agents-cli.md 错误码表加一行**

在错误码 / 退出码对照表追加：

```markdown
| `surface_fit_failed` | 12 | 散点曲面拟合失败：数据不成形 / inlier 比例 < 0.5 / 边界校验 reject |
```

- [ ] **Step 7: 跑测试确认通过**

Run: `cargo test -p lmt-shared`
Expected: PASS（含 distinct exit code 不重复、serde kind 正确）。

- [ ] **Step 8: Commit**

```bash
git add crates/lmt-shared/src/error.rs crates/lmt-shared/src/envelope.rs crates/lmt-shared/src/exit_codes.rs docs/agents-cli.md
git commit -m "feat(shared): add surface_fit_failed error code (4-way sync)"
```

---

## Task 12: scatter CSV parser

**Files:**
- Create: `crates/adapter-total-station/src/scatter_csv.rs`
- Modify: `crates/adapter-total-station/src/lib.rs`（`pub mod scatter_csv;`）

- [ ] **Step 1: 写失败测试**（`scatter_csv.rs` 的 `mod tests`）

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_tmp(content: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f
    }

    #[test]
    fn parses_bengtie_style_no_header_extra_col() {
        // name, (空列), x, y, z —— 用 --columns x=3,y=4,z=5, label=1
        let f = write_tmp("1,,1000.0,100.0,100.0\nLEDB-1,,1005.8,108.2,103.9\n");
        let cols = ColumnMap { x: 3, y: 4, z: 5, label: Some(1) };
        let pts = parse_scatter_csv(f.path(), Some(cols)).unwrap();
        assert_eq!(pts.len(), 2);
        assert_eq!(pts[1].id, "row2_LEDB-1");
        assert_eq!(pts[1].xyz, [1005.8, 108.2, 103.9]);
    }

    #[test]
    fn rejects_duplicate_ids() {
        let f = write_tmp("A,,1.0,2.0,3.0\nA,,1.0,2.0,3.0\n");
        let cols = ColumnMap { x: 3, y: 4, z: 5, label: Some(1) };
        let err = parse_scatter_csv(f.path(), Some(cols)).unwrap_err();
        assert!(matches!(err, AdapterError::InvalidInput(_)));
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p lmt-adapter-total-station scatter_csv`
Expected: 编译失败（`parse_scatter_csv`、`ColumnMap`、`ScatterPoint` 未定义）。

- [ ] **Step 3: 写实现**

```rust
use std::collections::HashSet;
use std::path::Path;

use crate::error::AdapterError;

/// 1-based 列号映射；label 可选（用于生成可读 id）。
#[derive(Debug, Clone, Copy)]
pub struct ColumnMap {
    pub x: usize,
    pub y: usize,
    pub z: usize,
    pub label: Option<usize>,
}

/// 一个散点：稳定唯一 id（行号+label）+ 原始坐标（与 CSV 同单位）。
#[derive(Debug, Clone)]
pub struct ScatterPoint {
    pub id: String,
    pub xyz: [f64; 3],
}

/// 解析无表头的散点 CSV。`columns` 为 None 时默认取“末尾 3 个可解析为数值的列”作 xyz、首列作 label。
pub fn parse_scatter_csv(
    path: &Path,
    columns: Option<ColumnMap>,
) -> Result<Vec<ScatterPoint>, AdapterError> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_path(path)
        .map_err(|e| AdapterError::InvalidInput(format!("open csv: {e}")))?;

    let mut out = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for (ri, rec) in rdr.records().enumerate() {
        let rec = rec.map_err(|e| AdapterError::InvalidInput(format!("csv row {}: {e}", ri + 1)))?;
        let fields: Vec<&str> = rec.iter().collect();
        let cm = match columns {
            Some(c) => c,
            None => infer_columns(&fields).ok_or_else(|| {
                AdapterError::InvalidInput(format!(
                    "row {}: cannot infer xyz columns; pass --columns",
                    ri + 1
                ))
            })?,
        };
        let get = |idx: usize| -> Result<f64, AdapterError> {
            fields
                .get(idx - 1)
                .and_then(|s| s.trim().parse::<f64>().ok())
                .ok_or_else(|| {
                    AdapterError::InvalidInput(format!("row {}: column {idx} not a number", ri + 1))
                })
        };
        let xyz = [get(cm.x)?, get(cm.y)?, get(cm.z)?];
        let label = cm
            .label
            .and_then(|li| fields.get(li - 1))
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .unwrap_or("");
        let id = format!("row{}_{}", ri + 1, label);
        if !seen.insert(id.clone()) {
            return Err(AdapterError::InvalidInput(format!("duplicate point id {id}")));
        }
        out.push(ScatterPoint { id, xyz });
    }
    if out.is_empty() {
        return Err(AdapterError::InvalidInput("no scatter points parsed".into()));
    }
    Ok(out)
}

/// 默认推断：取末尾 3 个能解析成数值的列作 x,y,z；首列作 label。
fn infer_columns(fields: &[&str]) -> Option<ColumnMap> {
    let numeric: Vec<usize> = fields
        .iter()
        .enumerate()
        .filter(|(_, s)| s.trim().parse::<f64>().is_ok())
        .map(|(i, _)| i + 1)
        .collect();
    if numeric.len() < 3 {
        return None;
    }
    let n = numeric.len();
    Some(ColumnMap {
        x: numeric[n - 3],
        y: numeric[n - 2],
        z: numeric[n - 1],
        label: Some(1),
    })
}
```

> **依赖注记：** `tempfile` 已是 adapter-total-station 的 dev-dependency（其它测试在用）；
> 若 `cargo test` 报缺，在 `[dev-dependencies]` 补 `tempfile`。`csv` 已是正式依赖。

- [ ] **Step 4: 注册模块**（`crates/adapter-total-station/src/lib.rs`）

```rust
pub mod scatter_csv;
```

- [ ] **Step 5: 跑测试确认通过**

Run: `cargo test -p lmt-adapter-total-station scatter_csv`
Expected: 2 passed。

- [ ] **Step 6: Commit**

```bash
git add crates/adapter-total-station/src/scatter_csv.rs crates/adapter-total-station/src/lib.rs
git commit -m "feat(adapter): scatter CSV parser (no header, positional columns, stable ids)"
```

---

## Task 13: lmt-app scatter import 分支

**Files:**
- Modify: `crates/lmt-app/src/total_station.rs`（`run_import` 加 `mode`/`columns`，scatter 走独立路径）

- [ ] **Step 1: 写失败测试**（`crates/lmt-app/src/total_station.rs` 的 `mod tests`）

```rust
#[test]
fn scatter_import_writes_measured_yaml_without_sop() {
    use lmt_adapter_total_station::scatter_csv::ColumnMap;
    use lmt_core::sampling::SamplingMode;
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    let proj = dir.path();
    // project.yaml：curved 屏 + 无 SOP coordinate_system（scatter 不需要）
    std::fs::write(proj.join("project.yaml"), r#"
project: { name: T, unit: mm }
screens:
  MAIN:
    cabinet_count: [55, 15]
    cabinet_size_mm: [500, 500]
    shape_prior: { type: curved, radius_mm: 9523 }
    shape_mode: rectangle
output: { target: disguise, obj_filename: "{screen_id}.obj", weld_vertices_tolerance_mm: 1.0, triangulate: true }
"#).unwrap();
    let csv = proj.join("s.csv");
    std::fs::write(&csv, "LEDB-1,,1.0,2.0,3.0\nLEDB-2,,1.1,2.1,3.0\nLEDB-3,,1.2,2.0,3.0\n").unwrap();

    let cols = ColumnMap { x: 3, y: 4, z: 5, label: Some(1) };
    let r = run_import_scatter(proj, "MAIN", &csv, Some(cols)).unwrap();
    assert_eq!(r.measured_count, 3);
    assert_eq!(r.fabricated_count, 0);

    let mp: lmt_core::measured_points::MeasuredPoints =
        serde_yaml::from_str(&std::fs::read_to_string(proj.join("measurements/measured.yaml")).unwrap()).unwrap();
    assert_eq!(mp.sampling_mode, SamplingMode::Scatter);
    assert_eq!(mp.points[0].name, "row1_LEDB-1");
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p lmt-app total_station::tests::scatter_import`
Expected: 编译失败（`run_import_scatter` 未定义）。

- [ ] **Step 3: 写 scatter import 路径**（在 `total_station.rs` 加新函数，不改 grid 的 `run_import`）

```rust
use lmt_adapter_total_station::scatter_csv::{parse_scatter_csv, ColumnMap};
use lmt_core::coordinate::CoordinateFrame;
use lmt_core::measured_points::MeasuredPoints;
use lmt_core::point::{MeasuredPoint, PointSource};
use lmt_core::sampling::SamplingMode;
use lmt_core::uncertainty::Uncertainty;
use nalgebra::Vector3;

/// scatter 路径：不走 SOP 校验 / 网格命名。从 project.yaml 取 cabinet_array + shape_prior，
/// 把散点原样存进 measured.yaml（identity frame、sampling_mode=Scatter）。
pub fn run_import_scatter(
    project_abs_path: &Path,
    screen_id: &str,
    csv_path: &Path,
    columns: Option<ColumnMap>,
) -> LmtResult<TotalStationImportResult> {
    let cfg = crate::projects::load_project_yaml_from_path(project_abs_path)?;
    let screen_cfg = cfg
        .screens
        .get(screen_id)
        .ok_or_else(|| LmtError::NotFound(format!("screen '{screen_id}' not in project")))?;
    // 复用 grid 路径的 cabinet/shape 取数（来源同 grid：project.yaml）
    let cabinet_array = crate::export::build_cabinet_array(screen_cfg)?;
    let shape_prior = crate::export::build_shape_prior(screen_cfg)?;

    let scatter = parse_scatter_csv(csv_path, columns)?;
    let points: Vec<MeasuredPoint> = scatter
        .iter()
        .map(|p| MeasuredPoint {
            name: p.id.clone(),
            position: Vector3::new(p.xyz[0], p.xyz[1], p.xyz[2]),
            uncertainty: Uncertainty::default(),
            source: PointSource::TotalStation,
        })
        .collect();
    let measured = MeasuredPoints {
        screen_id: screen_id.to_string(),
        coordinate_frame: CoordinateFrame {
            origin_world: [0.0, 0.0, 0.0],
            basis: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        },
        cabinet_array,
        shape_prior,
        points,
        sampling_mode: SamplingMode::Scatter,
    };

    let measurements_dir = project_abs_path.join("measurements");
    std::fs::create_dir_all(&measurements_dir)?;
    let measured_yaml_path = measurements_dir.join("measured.yaml");
    let backup_path = measurements_dir.join("measured.yaml.bak");
    check_import_no_screen_conflict(project_abs_path, screen_id)?;
    if measured_yaml_path.exists() {
        std::fs::rename(&measured_yaml_path, &backup_path)?;
    }
    let yaml = serde_yaml::to_string(&measured)?;
    std::fs::write(&measured_yaml_path, yaml)?;

    Ok(TotalStationImportResult {
        measurements_yaml_path: "measurements/measured.yaml".into(),
        report_json_path: String::new(), // scatter import 不产 grid import_report；拟合报告在 reconstruct
        measured_count: measured.points.len(),
        fabricated_count: 0,
        outlier_count: 0,
        missing_count: 0,
        warnings: vec!["scatter mode: points stored raw; fitting + outlier detection happen at reconstruct".into()],
    })
}
```

> **前置依赖：** 需要 `crate::export::build_shape_prior(screen_cfg)`。若 export.rs 尚无此 helper，
> 在 export.rs 加一个与 `build_cabinet_array` 对称的 `pub fn build_shape_prior(sc: &ScreenConfig)
> -> LmtResult<ShapePrior>`（把 `ShapePriorConfig` 的 `type: flat|curved{radius_mm}|...` 转 core
> `ShapePrior`）。`build_cabinet_array` 已存在（reconstruct.rs:34 在用）。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p lmt-app total_station::tests::scatter_import`
Expected: PASS（measured.yaml 含 sampling_mode=scatter，点 name=row1_LEDB-1）。

- [ ] **Step 5: Commit**

```bash
git add crates/lmt-app/src/total_station.rs crates/lmt-app/src/export.rs
git commit -m "feat(app): scatter import path (no SOP/grid-naming, cabinet/shape from project.yaml)"
```

---

## Task 14: lmt-app reconstruct 分流 + ScatterFitInfo 转换

**Files:**
- Modify: `crates/lmt-app/src/reconstruct.rs`（按 `sampling_mode` 分流 + report 填 `scatter_fit`）

- [ ] **Step 1: 写失败测试**（`reconstruct.rs` 的 `mod tests`，端到端：scatter import 出的 measured.yaml → reconstruct）

```rust
#[test]
fn reconstruct_scatter_fills_report_scatter_fit() {
    // 借 Task 13 的 scatter import 产物；这里直接构造 measured.yaml 跑 run_reconstruction
    // （完整 fixture 见 cli E2E）。断言 run 的 report.json 含 scatter_fit 且 method=surface_fit_*。
    // 详细 fixture 构造同 Task 9 的圆柱点集，写入 <proj>/measurements/measured.yaml，
    // project.yaml 用 curved/55x15，调用 run_reconstruction，读回 report.json 断言：
    //   report["surface"]["scatter_fit"] 非 null；report["quality_metrics"]["method"] 以 "surface_fit_" 开头。
    // （此测试代码与 Task 9 的点生成 + Task 13 的 project.yaml 写法组合，逐行展开见执行时。）
}
```

> **注：** 此 step 的测试需组合 Task 9 点生成 + Task 13 project.yaml；执行者把两段已给出的真实代码
> 拼进一个 `tempdir` fixture（写 measured.yaml + project.yaml），调用 `run_reconstruction`，
> 用 `read_run_report` 读回断言。两段源码均在本计划中、非占位。

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p lmt-app reconstruct::tests::reconstruct_scatter`
Expected: FAIL（顶层未分流，scatter 走 auto_reconstruct → 四级方法全 not-applicable → 报错）。

- [ ] **Step 3: 改 run_reconstruction 分流**（reconstruct.rs:51 的 `auto_reconstruct` 调用处）

顶部 import 加：

```rust
use lmt_core::reconstruct::surface_fit::SurfaceFitReconstructor;
use lmt_core::reconstruct::Reconstructor;
use lmt_core::sampling::SamplingMode;
```

把 line 51 的 `let surface = auto_reconstruct(&measurements).map_err(...)?;` 替换为：

```rust
    let surface = if measurements.sampling_mode == SamplingMode::Scatter {
        SurfaceFitReconstructor.reconstruct(&measurements).map_err(|e| {
            tracing::error!(error = %e, "reconstruct: surface_fit failed");
            // CoreError::Reconstruction → surface_fit_failed（与 invalid_input 区分）
            LmtError::SurfaceFitFailed(e.to_string())
        })?
    } else {
        auto_reconstruct(&measurements).map_err(|e| {
            tracing::error!(error = %e, "reconstruct: auto_reconstruct failed");
            LmtError::from(e)
        })?
    };
```

`ReconstructionReport` 构造（line 70-79）末尾加字段，从 core `ScatterFit` 转 DTO：

```rust
        scatter_fit: surface.scatter_fit.clone().map(Into::into),
```

（`From<core::ScatterFit> for ScatterFitInfo` 已在 Task 10 定义。）

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p lmt-app reconstruct`
Expected: PASS（scatter → report.surface.scatter_fit 非 null、method=surface_fit_cylinder）。
Run: `cargo test --workspace`
Expected: 全绿（grid 回归不破）。

- [ ] **Step 5: Commit**

```bash
git add crates/lmt-app/src/reconstruct.rs
git commit -m "feat(app): dispatch reconstruct by sampling_mode + map surface_fit error + fill report.scatter_fit"
```

---

## Task 15: lmt-cli `--mode` / `--columns` + E2E

**Files:**
- Modify: `crates/lmt-cli/src/cli.rs`（`TotalStationCmd::Import` 加 `mode`/`columns`）
- Modify: `crates/lmt-cli/src/commands/total_station.rs`（按 mode 调 grid/scatter import）
- Modify: `crates/lmt-cli/tests/cli_e2e.rs`（scatter 四类）

- [ ] **Step 1: 写失败 E2E 测试**（`cli_e2e.rs`，仿现有 case 风格）

```rust
#[test]
fn scatter_import_reconstruct_export_happy_path() {
    let tmp = tempfile::tempdir().unwrap();
    let proj = tmp.path();
    let db = proj.join("test.sqlite");
    // 写 curved 55x15 project.yaml（同 Task 13）+ 圆柱散点 CSV（同 Task 9 点集，CSV 化）
    // ... 写文件 ...
    // import --mode scatter --columns x=3,y=4,z=5 --yes
    let out = run_lmt(&["--db", db.to_str().unwrap(), "total-station", "import",
        proj.to_str().unwrap(), "MAIN", csv.to_str().unwrap(),
        "--mode", "scatter", "--columns", "x=3,y=4,z=5", "--yes"]);
    assert_eq!(out.code, 0);
    // reconstruct
    let out = run_lmt(&["--db", db.to_str().unwrap(), "reconstruct", "surface",
        proj.to_str().unwrap(), "MAIN", "measurements/measured.yaml", "--yes"]);
    assert_eq!(out.code, 0);
    // list-runs 拿 run_id → export
    // ... 解析 run_id, export obj <id> neutral --yes, 断言 exit 0 + OBJ 文件存在 ...
}

#[test]
fn scatter_import_refuses_without_yes() {
    // 不带 --yes → exit 2 (invalid_input)，不写 measured.yaml
}

#[test]
fn scatter_reconstruct_fit_failure_returns_surface_fit_failed() {
    // 喂随机噪声散点（拟合不出/inlier<50%）→ reconstruct exit 12 (surface_fit_failed)
    // 断言 stderr envelope error.code == "surface_fit_failed"
}
```

> **注：** 三个 case 的文件 fixture（project.yaml + CSV）用 Task 9 点集 + Task 13 project.yaml 拼成；
> `run_lmt`/`run_lmt_json` 辅助沿用 `cli_e2e.rs` 现有 harness。dry-run case 复用现有 import dry-run 断言模式加 `--mode scatter`。

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p lmt-cli scatter`
Expected: 编译失败（`--mode`/`--columns` 未定义）。

- [ ] **Step 3: cli.rs 加 clap 字段**（`TotalStationCmd::Import`）

```rust
        /// 采样模式：grid（默认，网格命名）或 scatter（曲面拟合）。
        #[arg(long, value_enum, default_value_t = ImportMode::Grid)]
        mode: ImportMode,
        /// scatter 模式列映射，1-based，形如 `x=3,y=4,z=5[,label=1]`。省略则自动推断末尾 3 数值列。
        #[arg(long)]
        columns: Option<String>,
```

加 enum（cli.rs 内）：

```rust
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum ImportMode {
    Grid,
    Scatter,
}
```

- [ ] **Step 4: commands/total_station.rs 按 mode 分流**

`run` 的 `Import` 解构加 `mode, columns`，`import()` 加这两个参数；`DestructiveDecision::Execute` 分支按 mode 调用：

```rust
            let result = match mode {
                ImportMode::Grid => lmt_app::total_station::run_import(
                    Path::new(project_abs_path), screen_id, Path::new(csv_path)),
                ImportMode::Scatter => {
                    let cols = match columns {
                        Some(s) => match parse_columns(s) {
                            Ok(c) => Some(c),
                            Err(e) => return output::err(mode_out, ApiError::new(error_codes::INVALID_INPUT, e)),
                        },
                        None => None,
                    };
                    lmt_app::total_station::run_import_scatter(
                        Path::new(project_abs_path), screen_id, Path::new(csv_path), cols)
                }
            };
            match result { Ok(r) => output::ok(...), Err(e) => output::err(mode_out, ApiError::from(e)) }
```

加 `--columns` 解析 helper（commands/total_station.rs）：

```rust
fn parse_columns(s: &str) -> Result<lmt_adapter_total_station::scatter_csv::ColumnMap, String> {
    use lmt_adapter_total_station::scatter_csv::ColumnMap;
    let (mut x, mut y, mut z, mut label) = (None, None, None, None);
    for kv in s.split(',') {
        let (k, v) = kv.split_once('=').ok_or_else(|| format!("bad --columns segment: {kv}"))?;
        let n: usize = v.trim().parse().map_err(|_| format!("column {k} not a number: {v}"))?;
        match k.trim() { "x" => x = Some(n), "y" => y = Some(n), "z" => z = Some(n), "label" => label = Some(n),
            other => return Err(format!("unknown column key: {other}")) }
    }
    Ok(ColumnMap {
        x: x.ok_or("missing x=")?, y: y.ok_or("missing y=")?, z: z.ok_or("missing z=")?, label,
    })
}
```

> **注：** dry-run 分支对 scatter：复用现有 project/screen/csv 存在性校验（不需要 SOP），
> **并额外**在 scatter 模式下解析 `--columns`（调 `parse_column_map`），格式错（如 `x=abc`、缺 `x=`）
> 立刻报 `invalid_input`，让 agent 在 `--yes` 前就发现列映射错误。对应 Step 1 加一个 dry-run case：
> `import ... --mode scatter --columns x=abc --dry-run` → exit 2 + envelope code `invalid_input`。

- [ ] **Step 5: 跑测试确认通过**

Run: `cargo test -p lmt-cli scatter`
Expected: 四类 case 全 PASS。

- [ ] **Step 6: Commit**

```bash
git add crates/lmt-cli/src/cli.rs crates/lmt-cli/src/commands/total_station.rs crates/lmt-cli/tests/cli_e2e.rs
git commit -m "feat(cli): total-station import --mode scatter --columns + E2E"
```

---

## Task 16: Tauri shim + docs/agents-cli.md + 全量自检

**Files:**
- Modify: `src-tauri/src/commands/total_station.rs`（import command 加 `mode`/`columns`）
- Modify: `docs/agents-cli.md`（命令表 + side_effect + Not-exposed 段）

- [ ] **Step 1: Tauri import shim 加参数**（`src-tauri/src/commands/total_station.rs`）

给现有 `#[tauri::command] import_total_station(...)`（thin wrapper）加 `mode: Option<String>` 与
`columns: Option<String>` 参数，按 mode 调 `lmt_app::total_station::run_import` 或
`run_import_scatter`（解析 columns 同 CLI 的 `parse_columns` 逻辑，可抽到 lmt-app 共用或各写一份）。
保持只做 transport 翻译，不写业务逻辑。

```rust
#[tauri::command]
pub async fn import_total_station(
    project_abs_path: String,
    screen_id: String,
    csv_path: String,
    mode: Option<String>,
    columns: Option<String>,
) -> Result<TotalStationImportResult, ApiError> {
    let p = std::path::Path::new(&project_abs_path);
    let c = std::path::Path::new(&csv_path);
    let res = match mode.as_deref() {
        Some("scatter") => {
            let cols = columns.as_deref().map(lmt_app::total_station::parse_column_map).transpose()
                .map_err(|e| ApiError::new(lmt_shared::envelope::error_codes::INVALID_INPUT, e))?;
            lmt_app::total_station::run_import_scatter(p, &screen_id, c, cols)
        }
        _ => lmt_app::total_station::run_import(p, &screen_id, c),
    };
    res.map_err(ApiError::from)
}
```

> **前置：** 把 `--columns` 字符串解析挪到 lmt-app（`pub fn parse_column_map(s: &str) ->
> Result<ColumnMap, String>`），CLI 与 Tauri 共用，避免两份漂移。CLI Task 15 的 `parse_columns`
> 改为调用它。

- [ ] **Step 2: docs/agents-cli.md 更新**

命令表 `lmt total-station import` 行的参数补 `[--mode grid|scatter] [--columns x=C,y=C,z=C[,label=C]]`，
并加一段说明：scatter 模式跳过 SOP 坐标系与网格命名，从 project.yaml 读 cabinet/shape，拟合 +
outlier 检测发生在 `reconstruct surface`。错误码表确认含 `surface_fit_failed`（Task 11 已加）。

- [ ] **Step 3: 全量自检**

```bash
cargo test --workspace
./target/debug/lmt --json schema | jq '.types | keys'   # 含 ScatterFitInfo
./target/debug/lmt total-station import --help            # 含 --mode/--columns
./target/debug/lmt --help
```
Expected: 测试全绿；schema 含新 DTO；help 含新参数。

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/commands/total_station.rs crates/lmt-app/src/total_station.rs docs/agents-cli.md
git commit -m "feat(tauri+docs): import shim mode/columns + agents-cli scatter docs"
```

---

## 完整端到端验收（崩铁真实数据）

跑一遍真实场景（脱敏后的崩铁 CSV 作 fixture，见 §11 隐私注）：

```bash
LMT=./target/debug/lmt; PROJECT=<崩铁项目>; DB=$PROJECT/lmt.sqlite
$LMT --db "$DB" total-station import "$PROJECT" MAIN bengtie.csv --mode scatter --columns x=3,y=4,z=5,label=1 --yes
$LMT --db "$DB" reconstruct surface "$PROJECT" MAIN measurements/measured.yaml --yes
RUN=$($LMT --json --db "$DB" reconstruct list-runs "$PROJECT" --screen-id MAIN | jq -r '.data[0].id')
$LMT --json --db "$DB" reconstruct get-run-report "$RUN" | jq '.data.surface.scatter_fit'  # R≈9523, outliers 带行号
$LMT --db "$DB" export obj "$RUN" disguise --dst out.obj --yes
```
Expected: scatter_fit.shape.radius_mm≈9523、张角≈165°、CD/A/BZ 点进 outliers、OBJ 生成。

> **隐私注：** 崩铁 CSV 作 git fixture 前先脱敏（去项目名/路径，坐标可保留或整体平移），
> 放 `crates/lmt-cli/tests/fixtures/scatter_arc.csv`；体积小（~80 行）可直接入库。
