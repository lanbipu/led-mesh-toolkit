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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "shape")]
pub enum ScatterShape {
    Plane { normal: [f64; 3] },
    Cylinder { radius_mm: f64, axis: [f64; 3] },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScatterOutlier {
    pub point_id: String,
    pub source_row: usize,
    pub coordinates: [f64; 3],
    pub residual_mm: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameDerivation {
    pub axis: [f64; 3],
    pub origin: [f64; 3],
    pub unwrap_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundaryCheck {
    pub verdict: String,
    pub projected_size_mm: [f64; 2],
    pub expected_size_mm: [f64; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScatterFit {
    pub shape: ScatterShape,
    pub inlier_count: usize,
    pub outliers: Vec<ScatterOutlier>,
    pub param_range: [f64; 4],
    pub boundary_check: BoundaryCheck,
    pub frame_derivation: FrameDerivation,
}

pub struct SurfaceFitReconstructor;

impl Reconstructor for SurfaceFitReconstructor {
    fn name(&self) -> &'static str {
        "surface_fit"
    }
    fn applicable(&self, points: &MeasuredPoints) -> bool {
        points.sampling_mode == SamplingMode::Scatter
    }
    fn reconstruct(&self, _points: &MeasuredPoints) -> Result<ReconstructedSurface, CoreError> {
        Err(CoreError::Reconstruction(
            "surface_fit reconstruction not yet assembled".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reconstruct::Reconstructor;
    use crate::sampling::SamplingMode;
    use crate::test_support::minimal_scatter_points;

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
