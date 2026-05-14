//! IPC types mirroring `python-sidecar/schema/ipc.schema.json`.
//!
//! Any change here must also update the JSON Schema and the pydantic
//! models in `python-sidecar/src/lmt_vba_sidecar/ipc.py`.

use nalgebra::{Matrix3, Vector3};
use serde::{Deserialize, Serialize};

pub type Vec3 = [f64; 3];
pub type Mat3 = [[f64; 3]; 3];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinateFrame {
    pub origin_world: Vec3,
    pub basis: Mat3,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CabinetArray {
    pub cols: u32,
    pub rows: u32,
    pub cabinet_size_mm: [f64; 2],
    #[serde(default)]
    pub absent_cells: Vec<(u32, u32)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ShapePrior {
    Flat(FlatTag),
    Curved { curved: CurvedShape },
    Folded { folded: FoldedShape },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlatTag {
    Flat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurvedShape {
    pub radius_mm: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FoldedShape {
    pub fold_seam_columns: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameAnchor {
    pub cabinet_col: u32,
    pub cabinet_row: u32,
    pub aruco_id: u32,
    pub position_world: Vec3,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intrinsics {
    #[serde(rename = "K")]
    pub k: Mat3,
    pub dist_coeffs: Vec<f64>,
    pub image_size: [u32; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternMetaCabinet {
    pub col: u32,
    pub row: u32,
    pub aruco_id_start: u32,
    pub aruco_id_end: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternMeta {
    pub aruco_dict: String,
    pub markers_per_cabinet: u32,
    pub checkerboard_inner_corners: u32,
    pub cabinets: Vec<PatternMetaCabinet>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FrameStrategy {
    NominalAnchoring,
    ThreePoints,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconstructProject {
    pub screen_id: String,
    pub coordinate_frame: CoordinateFrame,
    pub cabinet_array: CabinetArray,
    pub shape_prior: ShapePrior,
    pub frame_strategy: FrameStrategy,
    pub frame_anchors: Option<Vec<FrameAnchor>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconstructInput {
    pub command: String,
    pub version: u32,
    pub project: ReconstructProject,
    pub images: Vec<String>,
    pub intrinsics: Intrinsics,
    pub pattern_meta: PatternMeta,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Uncertainty {
    Isotropic(f64),
    Covariance(Mat3),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointSourceVisualBa {
    pub camera_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointSource {
    pub visual_ba: PointSourceVisualBa,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeasuredPointDto {
    pub name: String,
    pub position: Vec3,
    pub uncertainty: Uncertainty,
    pub source: PointSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaStats {
    pub rms_reprojection_px: f64,
    pub iterations: u32,
    pub converged: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultData {
    pub measured_points: Vec<MeasuredPointDto>,
    pub ba_stats: BaStats,
    pub frame_strategy_used: FrameStrategy,
    // Forward compat: older sidecars (and the calibrate / generate_pattern
    // subcommands, which don't run Procrustes) may omit this. Default to 0.0
    // so Rust adapter doesn't reject otherwise-valid responses.
    #[serde(default)]
    pub procrustes_align_rms_m: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressEvent {
    pub stage: String,
    pub percent: f64,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WarningEvent {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub cabinet: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultEnvelope {
    pub data: ResultData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolErrorEvent {
    pub code: String,
    pub message: String,
    pub fatal: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    Progress(ProgressEvent),
    Warning(WarningEvent),
    Result(ResultEnvelope),
    Error(ProtocolErrorEvent),
}

impl MeasuredPointDto {
    /// Convert to the IR `MeasuredPoint`.
    ///
    /// **Unit boundary**: the IPC channel carries values in meters (matching
    /// the sidecar's BA / Procrustes math), but `lmt_core::uncertainty::Uncertainty`
    /// documents `Isotropic` as millimeters and `Covariance3x3` consequently in
    /// mm². Convert here so M1 (total-station, mm) and M2 (visual-BA) feed the
    /// downstream reconstruction metrics in identical units.
    ///
    /// Position is left in meters (matches `MeasuredPoint::position` docstring).
    pub fn into_ir(self) -> lmt_core::point::MeasuredPoint {
        let position = Vector3::new(self.position[0], self.position[1], self.position[2]);
        let uncertainty = match self.uncertainty {
            Uncertainty::Isotropic(sigma_m) => {
                lmt_core::uncertainty::Uncertainty::Isotropic(sigma_m * 1000.0)
            }
            Uncertainty::Covariance(m) => {
                // m² → mm² (1 m² = 1e6 mm²)
                let scale = 1.0e6;
                lmt_core::uncertainty::Uncertainty::Covariance3x3(Matrix3::new(
                    m[0][0] * scale,
                    m[0][1] * scale,
                    m[0][2] * scale,
                    m[1][0] * scale,
                    m[1][1] * scale,
                    m[1][2] * scale,
                    m[2][0] * scale,
                    m[2][1] * scale,
                    m[2][2] * scale,
                ))
            }
        };
        lmt_core::point::MeasuredPoint {
            name: self.name,
            position,
            uncertainty,
            source: lmt_core::point::PointSource::VisualBA {
                camera_count: self.source.visual_ba.camera_count,
            },
        }
    }
}
