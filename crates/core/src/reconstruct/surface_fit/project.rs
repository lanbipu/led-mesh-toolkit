use nalgebra::Vector3;

use crate::reconstruct::surface_fit::fit::{CylinderFit, PlaneFit};

/// 参数空间投影结果。`range = [min_a, max_a, min_b, max_b]`。
/// 圆柱: a=θ(rad), b=h(m，沿轴)。平面: a=u(m), b=v(m)。
pub struct Projection {
    pub range: [f64; 4],
    /// 平面专用：(origin, u_dir, v_dir)（世界系单位基）；圆柱为 None。
    pub plane_basis: Option<(Vector3<f64>, Vector3<f64>, Vector3<f64>)>,
}

pub fn project_cylinder(pts: &[Vector3<f64>], cyl: &CylinderFit) -> Projection {
    let (mut min_t, mut max_t) = (f64::INFINITY, f64::NEG_INFINITY);
    let (mut min_h, mut max_h) = (f64::INFINITY, f64::NEG_INFINITY);
    for &i in &cyl.inliers {
        let p = pts[i];
        let t = (p.y - cyl.center_xy.y).atan2(p.x - cyl.center_xy.x);
        let h = p.z;
        min_t = min_t.min(t);
        max_t = max_t.max(t);
        min_h = min_h.min(h);
        max_h = max_h.max(h);
    }
    Projection { range: [min_t, max_t, min_h, max_h], plane_basis: None }
}

/// 平面投影 + 定向：u 基取使 Δu:Δv 最接近 cols:rows 的方向，避免网格旋转/镜像。
pub fn project_plane(pts: &[Vector3<f64>], pl: &PlaneFit, cols: u32, rows: u32) -> Projection {
    let n = pl.normal;
    let seed = if n.x.abs() < 0.9 {
        Vector3::new(1.0, 0.0, 0.0)
    } else {
        Vector3::new(0.0, 1.0, 0.0)
    };
    let e1 = (seed - n * seed.dot(&n)).normalize();
    let e2 = n.cross(&e1).normalize();
    let proj = |e: &Vector3<f64>| {
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for &i in &pl.inliers {
            let s = (pts[i] - pl.centroid).dot(e);
            lo = lo.min(s);
            hi = hi.max(s);
        }
        (lo, hi)
    };
    let (e1lo, e1hi) = proj(&e1);
    let (e2lo, e2hi) = proj(&e2);
    let (d1, d2) = (e1hi - e1lo, e2hi - e2lo);
    let target = cols as f64 / rows as f64;
    let (u_dir, v_dir, urange, vrange) =
        if (d1 / d2 - target).abs() <= (d2 / d1 - target).abs() {
            (e1, e2, (e1lo, e1hi), (e2lo, e2hi))
        } else {
            (e2, e1, (e2lo, e2hi), (e1lo, e1hi))
        };
    let v_dir = if v_dir.z < 0.0 { -v_dir } else { v_dir };
    let u_dir = if u_dir.cross(&v_dir).dot(&n) < 0.0 { -u_dir } else { u_dir };
    let origin = pl.centroid + u_dir * urange.0 + v_dir * vrange.0;
    Projection {
        range: [urange.0, urange.1, vrange.0, vrange.1],
        plane_basis: Some((origin, u_dir, v_dir)),
    }
}

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
            let t = -1.0 + 2.0 * (k as f64 / 39.0);
            for &z in &[2.0_f64, 4.0_f64] {
                pts.push(Vector3::new(1.0 + r * t.cos(), 0.5 + r * t.sin(), z));
            }
        }
        let cyl = fit_cylinder(&pts).unwrap();
        let p = project_cylinder(&pts, &cyl);
        assert!((p.range[1] - p.range[0] - 2.0).abs() < 0.05);
        assert!((p.range[3] - p.range[2] - 2.0).abs() < 0.05);
    }

    #[test]
    fn plane_orientation_matches_cabinet_aspect() {
        let mut pts = vec![];
        for i in 0..9 {
            for j in 0..5 {
                pts.push(Vector3::new(i as f64 * 0.25, 0.0, j as f64 * 0.25));
            }
        }
        let pl = fit_plane(&pts).unwrap();
        let p = project_plane(&pts, &pl, 4, 2);
        let du = p.range[1] - p.range[0];
        let dv = p.range[3] - p.range[2];
        assert!((du / dv - 2.0).abs() < 0.1, "du={du} dv={dv}");
    }
}
