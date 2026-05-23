use nalgebra::{Matrix3, Vector3};

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
    let outliers = (0..pts.len()).filter(|i| !best.contains(i)).collect();
    Some(PlaneFit {
        normal,
        centroid,
        inliers: best,
        outliers,
    })
}

/// 协方差矩阵最小特征向量 = 平面法向。
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

#[cfg(test)]
mod tests {
    use super::*;
    use nalgebra::Vector3;

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
