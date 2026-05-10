use crate::error::CoreError;
use crate::measured_points::MeasuredPoints;
use crate::surface::ReconstructedSurface;

pub mod nominal;
pub mod direct;
pub mod boundary_interp;
pub mod radial_basis;

/// Strategy for reconstructing a continuous surface from sparse measured points.
pub trait Reconstructor {
    /// Whether this reconstructor can produce a result given the available measurements.
    fn applicable(&self, points: &MeasuredPoints) -> bool;

    /// Run reconstruction. Caller should call `applicable` first.
    fn reconstruct(&self, points: &MeasuredPoints) -> Result<ReconstructedSurface, CoreError>;

    /// Human-readable identifier for diagnostics.
    fn name(&self) -> &'static str;
}
