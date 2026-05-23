use nalgebra::{Matrix3, Vector2, Vector3};

/// inlier 判定阈值（米）。与 geometric_naming 的 50mm 同量级。
pub const INLIER_THRESH_M: f64 = 0.050;
const RANSAC_ITERS: usize = 2000;

/// 确定性线性同余伪随机（避免引入 rand 依赖；固定种子 → 测试可复现）。
pub(crate) struct Lcg(u64);
impl Lcg {
    pub(crate) fn new(seed: u64) -> Self {
        Lcg(seed.max(1))
    }
    pub(crate) fn next_usize(&mut self, bound: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % bound.max(1)
    }
}

pub struct PlaneFit {
    pub normal: Vector3<f64>,
    pub centroid: Vector3<f64>,
    pub inliers: Vec<usize>,
    pub outliers: Vec<usize>,
}

pub(crate) fn pick3(rng: &mut Lcg, n: usize) -> Option<(usize, usize, usize)> {
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
    let centroid =
        best.iter().map(|&i| pts[i]).sum::<Vector3<f64>>() / best.len() as f64;
    let normal = pca_smallest_axis(pts, &best, centroid);
    let best_set: std::collections::HashSet<usize> = best.iter().copied().collect();
    let outliers = (0..pts.len()).filter(|i| !best_set.contains(i)).collect();
    Some(PlaneFit {
        normal,
        centroid,
        inliers: best,
        outliers,
    })
}

/// 协方差矩阵最小特征向量 = 平面法向。
// 假设 inlier>=3 且非全共线；共线时协方差秩为 1、最小特征向量不稳定，但 RANSAC 候选阶段已用叉积滤掉共线三点。
fn pca_smallest_axis(
    pts: &[Vector3<f64>],
    idx: &[usize],
    centroid: Vector3<f64>,
) -> Vector3<f64> {
    let mut cov = Matrix3::zeros();
    for &i in idx {
        let d = pts[i] - centroid;
        cov += d * d.transpose();
    }
    let eig = cov.symmetric_eigen();
    let mut min_k = 0;
    for k in 1..3 {
        if eig.eigenvalues[k] < eig.eigenvalues[min_k] {
            min_k = k;
        }
    }
    eig.eigenvectors.column(min_k).normalize().into()
}

pub struct CylinderFit {
    pub axis: Vector3<f64>,
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
    let best_set: std::collections::HashSet<usize> = best.iter().copied().collect();
    let outliers = (0..pts.len()).filter(|i| !best_set.contains(i)).collect();
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

#[cfg(test)]
mod tests {
    use super::*;
    use nalgebra::Vector3;

    fn cylinder_arc_with_outliers() -> Vec<Vector3<f64>> {
        let mut v = vec![];
        let r = 9.5_f64;
        let (cx, cy) = (1.0_f64, 0.5_f64);
        for k in 0..40 {
            let t = -80.0_f64.to_radians() + (160.0_f64.to_radians()) * (k as f64 / 39.0);
            for &z in &[2.0_f64, 4.0_f64] {
                v.push(Vector3::new(cx + r * t.cos(), cy + r * t.sin(), z));
            }
        }
        v.push(Vector3::new(cx + 0.2, cy, 3.0));
        v.push(Vector3::new(cx + 20.0, cy, 3.0));
        v
    }

    #[test]
    fn fit_cylinder_recovers_radius_and_drops_outliers() {
        let pts = cylinder_arc_with_outliers();
        let fit = fit_cylinder(&pts).expect("should fit");
        assert!((fit.radius_m - 9.5).abs() < 0.05, "radius={}", fit.radius_m);
        assert_eq!(fit.outliers.len(), 2);
        assert!(fit.axis.z.abs() > 0.99);
    }

    fn plane_grid_with_outliers() -> Vec<Vector3<f64>> {
        let mut v = vec![];
        for i in 0..5 {
            for j in 0..5 {
                v.push(Vector3::new(i as f64 * 0.5, j as f64 * 0.5, 2.0));
            }
        }
        v.push(Vector3::new(1.0, 1.0, 3.0));
        v.push(Vector3::new(0.5, 0.5, 0.5));
        v.push(Vector3::new(2.0, 0.0, 5.0));
        v
    }

    #[test]
    fn fit_plane_recovers_normal_and_drops_outliers() {
        let pts = plane_grid_with_outliers();
        let fit = fit_plane(&pts).expect("should fit");
        assert!(fit.normal.z.abs() > 0.99, "normal={:?}", fit.normal);
        assert_eq!(fit.inliers.len(), 25);
        assert_eq!(fit.outliers.len(), 3);
    }
}
