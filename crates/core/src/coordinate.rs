use nalgebra::{Matrix3, Vector3};
use serde::{Deserialize, Serialize};

use crate::error::CoreError;

/// 3-point method: origin + X-axis reference + XY-plane reference.
///
/// Internally builds an orthonormal basis via Gram-Schmidt:
///   X = normalize(P_x - P_origin)
///   Z = normalize((P_xy - P_origin) × X)
///   Y = Z × X
///
/// Stores world-frame origin + basis-as-rotation. Translation
/// from world to model is `R^T * (p - origin)`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinateFrame {
    pub origin_world: [f64; 3],
    pub basis: [[f64; 3]; 3], // columns: X, Y, Z (world frame)
}

impl CoordinateFrame {
    /// Build a coordinate frame from three world-frame points.
    /// Returns `CoreError::InvalidInput` if points are collinear or coincident.
    pub fn from_three_points(
        origin: Vector3<f64>,
        x_axis_ref: Vector3<f64>,
        xy_plane_ref: Vector3<f64>,
    ) -> Result<Self, CoreError> {
        let dx = x_axis_ref - origin;
        let dxy = xy_plane_ref - origin;

        if dx.norm() < 1e-9 {
            return Err(CoreError::InvalidInput(
                "x-axis reference coincides with origin".into(),
            ));
        }
        if dxy.norm() < 1e-9 {
            return Err(CoreError::InvalidInput(
                "xy-plane reference coincides with origin".into(),
            ));
        }

        let x = dx.normalize();
        let z_unnorm = dxy.cross(&x);
        if z_unnorm.norm() < 1e-9 {
            return Err(CoreError::InvalidInput(
                "three points are collinear".into(),
            ));
        }
        let z = z_unnorm.normalize();
        let y = z.cross(&x);

        let basis = [
            [x.x, x.y, x.z],
            [y.x, y.y, y.z],
            [z.x, z.y, z.z],
        ];

        Ok(Self {
            origin_world: [origin.x, origin.y, origin.z],
            basis,
        })
    }

    fn rotation(&self) -> Matrix3<f64> {
        // basis stored as [x_col, y_col, z_col]
        Matrix3::from_columns(&[
            Vector3::new(self.basis[0][0], self.basis[0][1], self.basis[0][2]),
            Vector3::new(self.basis[1][0], self.basis[1][1], self.basis[1][2]),
            Vector3::new(self.basis[2][0], self.basis[2][1], self.basis[2][2]),
        ])
    }

    fn origin(&self) -> Vector3<f64> {
        Vector3::new(self.origin_world[0], self.origin_world[1], self.origin_world[2])
    }

    /// Transform a world-frame point to model frame.
    pub fn world_to_model(&self, world: &Vector3<f64>) -> Vector3<f64> {
        self.rotation().transpose() * (world - self.origin())
    }

    /// Transform a model-frame point back to world.
    pub fn model_to_world(&self, model: &Vector3<f64>) -> Vector3<f64> {
        self.rotation() * model + self.origin()
    }
}
