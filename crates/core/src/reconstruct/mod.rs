use crate::error::CoreError;
use crate::measured_points::MeasuredPoints;
use crate::surface::ReconstructedSurface;

pub mod boundary_interp;
pub mod direct;
pub mod nominal;
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

/// Pick the most accurate applicable reconstructor and run it.
/// Order: direct_link → radial_basis → boundary_interp → nominal.
pub fn auto_reconstruct(points: &MeasuredPoints) -> Result<ReconstructedSurface, CoreError> {
    // Order: direct_link → radial_basis → boundary_interp → nominal.
    // Rationale: radial_basis uses every interior anchor as a constraint
    // (yields exact anchor reproduction), while boundary_interp only uses
    // top+bottom and ignores interior measurements. When interior anchors
    // exist, prefer radial so they aren't silently dropped. Boundary still
    // wins in the boundary-only case (no interior anchors → radial
    // not-applicable), and stays useful as a manual API call.
    let strategies: Vec<Box<dyn Reconstructor>> = vec![
        Box::new(direct::DirectLinkReconstructor),
        Box::new(radial_basis::RadialBasisReconstructor),
        Box::new(boundary_interp::BoundaryInterpReconstructor),
        Box::new(nominal::NominalReconstructor),
    ];

    for s in &strategies {
        if s.applicable(points) {
            return s.reconstruct(points);
        }
    }

    Err(CoreError::Reconstruction(
        "no applicable reconstructor for this point set".into(),
    ))
}
