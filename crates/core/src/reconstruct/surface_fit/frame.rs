use nalgebra::Vector3;

use crate::coordinate::CoordinateFrame;
use crate::reconstruct::surface_fit::fit::CylinderFit;
use crate::reconstruct::surface_fit::project::Projection;
use crate::reconstruct::surface_fit::FrameDerivation;

/// M0.1 IR 坐标系：+X=列(周向)、+Y=法向(径向朝外)、+Z=行向上(竖直)。
/// origin = θmin 对应弧面上 h_min 点（即屏左下角）。
///
/// basis 列序 [X, Y, Z]，det = X·(Y×Z) = +1 由数学保证：
///   Y = radial = (cos θ0, sin θ0, 0)
///   Z = up     = (0, 0, 1)
///   X = radial × up = (sin θ0, -cos θ0, 0)
///   det = X·(Y×Z) = (sin θ0,−cos θ0,0)·(sin θ0,−cos θ0,0) = 1
pub fn derive_cylinder_frame(
    cyl: &CylinderFit,
    proj: &Projection,
) -> (CoordinateFrame, FrameDerivation) {
    let [t0, _t1, h0, _h1] = proj.range;

    let origin = Vector3::new(
        cyl.center_xy.x + cyl.radius_m * t0.cos(),
        cyl.center_xy.y + cyl.radius_m * t0.sin(),
        h0,
    );

    // +Y：法向（径向朝外），单位向量
    let radial = Vector3::new(t0.cos(), t0.sin(), 0.0);
    // +Z：竖直向上
    let up = Vector3::new(0.0, 0.0, 1.0);
    // +X：周向切线 = radial × up，保证右手系（det=+1）
    let x_col = radial.cross(&up).normalize();

    let basis = [
        [x_col.x, x_col.y, x_col.z],   // X = 周向
        [radial.x, radial.y, radial.z], // Y = 法向
        [up.x, up.y, up.z],             // Z = 竖直
    ];

    let frame = CoordinateFrame {
        origin_world: [origin.x, origin.y, origin.z],
        basis,
    };
    let deriv = FrameDerivation {
        axis: [0.0, 0.0, 1.0],
        origin: [origin.x, origin.y, origin.z],
        unwrap_dir: format!("theta {:.3}->{:.3}", proj.range[0], proj.range[1]),
    };
    (frame, deriv)
}

/// 平面坐标系：+X=u_dir(列)、+Y=v_dir(行)、+Z=法向。
/// project_plane 保证 (u×v)·n > 0，basis=[u,v,n] 的 det=(u×v)·n=+1，右手系。
///
/// 注：M0.1 IR 对平面写的 +Y=法向是笔误（[u,n,v] det=-1）；这里统一用
/// "两个切向量先，法向最后"的 [u,v,n] 排列，与坐标变换约定一致。
pub fn derive_plane_frame(
    normal: Vector3<f64>,
    proj: &Projection,
) -> (CoordinateFrame, FrameDerivation) {
    let (origin, u_dir, v_dir) = proj
        .plane_basis
        .expect("derive_plane_frame requires plane_basis from project_plane");

    // pca_smallest_axis 返回的 normal 符号不确定；project_plane 保证 (u×v)·n_passed_in>0
    // 并不一定——这里重新对齐：让 normal 与 u×v 同向（det=(u×v)·n>0=+1）。
    let normal = if u_dir.cross(&v_dir).dot(&normal) >= 0.0 {
        normal
    } else {
        -normal
    };

    let basis = [
        [u_dir.x, u_dir.y, u_dir.z],     // X = u（列方向）
        [v_dir.x, v_dir.y, v_dir.z],     // Y = v（行方向）
        [normal.x, normal.y, normal.z],   // Z = 法向
    ];

    let frame = CoordinateFrame {
        origin_world: [origin.x, origin.y, origin.z],
        basis,
    };
    let deriv = FrameDerivation {
        axis: [normal.x, normal.y, normal.z],
        origin: [origin.x, origin.y, origin.z],
        unwrap_dir: "planar".into(),
    };
    (frame, deriv)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reconstruct::surface_fit::fit::{fit_cylinder, fit_plane};
    use crate::reconstruct::surface_fit::project::{project_cylinder, project_plane};
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

        // serde 往返触发 CoordinateFrame 的自定义 Deserialize 校验：
        // 正交、单位长度、右手系（det=+1）
        let yaml = serde_yaml::to_string(&frame).unwrap();
        let back: crate::coordinate::CoordinateFrame = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(back.basis, frame.basis);

        // 圆柱轴应接近 Z 方向
        assert!(
            deriv.axis[2].abs() > 0.99,
            "axis Z component too small: {:?}",
            deriv.axis
        );
    }

    #[test]
    fn plane_frame_is_orthonormal_right_handed() {
        // 在 xz 平面上的矩形格点，法向 = Y (0,1,0)
        let mut pts = vec![];
        for i in 0..9 {
            for j in 0..5 {
                pts.push(Vector3::new(i as f64 * 0.25, 0.0, j as f64 * 0.25));
            }
        }
        let pl = fit_plane(&pts).unwrap();
        let proj = project_plane(&pts, &pl, 4, 2);
        let (frame, deriv) = derive_plane_frame(pl.normal, &proj);

        // serde 往返强制校验 basis 正交/单位/右手
        let yaml = serde_yaml::to_string(&frame).unwrap();
        let back: crate::coordinate::CoordinateFrame = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(back.basis, frame.basis);

        // 法向应接近 ±Y
        let n = deriv.axis;
        assert!(
            n[1].abs() > 0.99,
            "plane normal should be near Y axis: {:?}",
            n
        );
    }
}
