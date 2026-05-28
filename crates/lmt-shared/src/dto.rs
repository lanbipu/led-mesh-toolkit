use lmt_core::{shape::CabinetArray, surface::QualityMetrics, surface::ReconstructedSurface};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RecentProject {
    pub id: i64,
    pub abs_path: String,
    pub display_name: String,
    pub last_opened_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProjectConfig {
    pub project: ProjectMeta,
    pub screens: BTreeMap<String, ScreenConfig>,
    pub coordinate_system: CoordinateSystemConfig,
    pub output: OutputConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProjectMeta {
    pub name: String,
    pub unit: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<SurveyMethod>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SurveyMethod {
    M1,
    M2,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ScreenConfig {
    pub cabinet_count: [u32; 2],
    pub cabinet_size_mm: [f64; 2],
    #[serde(default)]
    pub pixels_per_cabinet: Option<[u32; 2]>,
    pub shape_prior: ShapePriorConfig,
    pub shape_mode: ShapeMode,
    #[serde(default)]
    pub irregular_mask: Vec<[u32; 2]>,
    #[serde(default)]
    pub bottom_completion: Option<BottomCompletionConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ShapePriorConfig {
    Flat,
    Curved {
        radius_mm: f64,
        #[serde(default)]
        fold_seams_at_columns: Vec<u32>,
    },
    Folded {
        fold_seams_at_columns: Vec<u32>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ShapeMode {
    Rectangle,
    Irregular,
}

/// lmt-core 的 `SamplingMode` 镜像，用于 schema dump（core 不派生 JsonSchema）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SamplingModeInfo {
    Grid,
    Scatter,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BottomCompletionConfig {
    pub lowest_measurable_row: u32,
    pub fallback_method: String,
    pub assumed_height_mm: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CoordinateSystemConfig {
    pub origin_point: String,
    pub x_axis_point: String,
    pub xy_plane_point: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OutputConfig {
    pub target: String,
    pub obj_filename: String,
    pub weld_vertices_tolerance_mm: f64,
    pub triangulate: bool,
}

// ── Scatter-fit DTO types ─────────────────────────────────────────────────────

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
            outliers: c
                .outliers
                .into_iter()
                .map(|o| ScatterOutlierInfo {
                    point_id: o.point_id,
                    source_row: o.source_row,
                    coordinates: o.coordinates,
                    residual_mm: o.residual_mm,
                })
                .collect(),
            param_range: c.param_range,
            boundary_check: BoundaryCheckInfo {
                verdict: c.boundary_check.verdict,
                projected_size_mm: c.boundary_check.projected_size_mm,
                expected_size_mm: c.boundary_check.expected_size_mm,
            },
            frame_derivation: FrameDerivationInfo {
                axis: c.frame_derivation.axis,
                origin: c.frame_derivation.origin,
                unwrap_dir: c.frame_derivation.unwrap_dir,
            },
        }
    }
}

// ── Reconstruction types ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct ReconstructionResult {
    pub run_id: i64,
    pub surface: ReconstructedSurface,
    pub report_json_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReconstructionRun {
    pub id: i64,
    pub screen_id: String,
    pub method: String,
    pub estimated_rms_mm: f64,
    pub vertex_count: i64,
    pub target: Option<String>,
    pub output_obj_path: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconstructionReport {
    pub surface: ReconstructedSurface,
    pub quality_metrics: QualityMetrics,
    pub project_path: String,
    pub screen_id: String,
    pub measurements_path: String,
    pub created_at: String,
    /// Cabinet array snapshot captured at reconstruction time.
    /// Export uses this instead of re-reading project.yaml.
    pub cabinet_array: CabinetArray,
    /// Weld tolerance (mm) snapshot captured at reconstruction time.
    pub weld_tolerance_mm: f64,
    /// Scatter 路径的拟合元数据；grid 路径为 None。
    #[serde(default)]
    pub scatter_fit: Option<ScatterFitInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TotalStationImportResult {
    /// 相对 project_abs_path 的路径，e.g. "measurements/measured.yaml"
    pub measurements_yaml_path: String,
    /// 相对 project_abs_path 的路径
    pub report_json_path: String,
    pub measured_count: usize,
    pub fabricated_count: usize,
    pub outlier_count: usize,
    pub missing_count: usize,
    pub warnings: Vec<String>,
}

// ── Visual reconstruction (camera-branch) DTO types ──────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CabinetPoseSummary {
    pub cabinet_id: String,
    pub position_mm: [f64; 3],
    pub normal: [f64; 3],
    pub reprojection_rms_px: f64,
    pub observed_views: u32,
    pub quality: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct VisualReconstructResult {
    pub screen_id: String,
    pub measured_yaml_path: String,
    pub pose_report_path: String,
    pub cabinet_count: usize,
    pub ba_rms_px: f64,
    pub cabinets: Vec<CabinetPoseSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SimulateResult {
    pub dataset_dir: String,
    pub n_views: u32,
    pub n_observations: u32,
    pub seed: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EvalResult {
    pub method: String,
    pub max_size_error_mm: f64,
    pub max_distance_error_mm: f64,
    pub max_angle_error_deg: f64,
    pub seeds: Vec<i64>,
}

/// Per-cabinet size reconciliation: reconstructed size (from corners) vs known.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CabinetSizeCheck {
    pub cabinet_id: String,
    pub size_error_mm: f64,
    #[serde(rename = "pass")]
    pub pass: bool,
}

/// Per-pair distance/angle reconciliation against known monitor geometry.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PairCheck {
    pub a: String,
    pub b: String,
    pub distance_error_mm: f64,
    pub angle_error_deg: f64,
    pub distance_pass: bool,
    pub angle_pass: bool,
}

/// Result of reconciling a cabinet_pose_report against known monitor geometry
/// (size from corners, distance from positions, angle from normals).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CompareKnownResult {
    pub cabinets: Vec<CabinetSizeCheck>,
    pub pairs: Vec<PairCheck>,
    pub passed: bool,
    pub thresholds: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CalibrateResult {
    pub intrinsics_path: String,
    pub reproj_error_px: f64,
    pub frames_used: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GeneratePatternResult {
    pub output_dir: String,
    pub cabinet_count: usize,
    /// Total ArUco markers across all cabinets (per-cabinet counts vary in v2).
    pub total_markers: u32,
}

/// `lmt visual generate-structured-light` 结果：点阵序列生成到
/// `patterns/<screen_id>/sl/`(frames + sequence.mp4 + sl_meta.json)。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GenerateStructuredLightResult {
    pub output_dir: String,
    pub n_dots: usize,
    pub n_frames: usize,
}

/// `lmt visual decode-structured-light` 结果：解码出的屏幕↔相机对应文件路径
/// 与解码成功的点数。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DecodeStructuredLightResult {
    pub output_path: String,
    pub n_dots_decoded: usize,
}

// ── Pose-OBJ export DTO types ─────────────────────────────────────────────────

/// 读 `cabinet_pose_report.json`（visual reconstruct 产出）用的精简视图，
/// 只取导出 OBJ 需要的字段。完整 schema 见 python-sidecar 的 `CabinetPoseReport`。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CabinetPoseReportFile {
    pub schema_version: String,
    pub cabinet_poses: Vec<CabinetPoseEntry>,
}

/// 单块 cabinet 的 4 个世界系角点（mm，顺序 BL,BR,TR,TL）。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CabinetPoseEntry {
    pub cabinet_id: String,
    pub corners_mm: [[f64; 3]; 4],
}

/// `lmt export pose-obj` 结果：所有箱体合并为一个 OBJ。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExportPoseObjResult {
    pub target: String,
    pub cabinet_count: usize,
    pub file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstructionCardResult {
    /// HTML 字符串，前端 iframe srcdoc 渲染。PDF 通过单独的
    /// `save_instruction_pdf` 命令按用户选定的目标路径写盘。
    pub html_content: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visual_dtos_roundtrip() {
        // VisualReconstructResult
        let cabinet = CabinetPoseSummary {
            cabinet_id: "MAIN_V001_R001".into(),
            position_mm: [100.0, 200.0, 300.0],
            normal: [0.0, 0.0, 1.0],
            reprojection_rms_px: 0.42,
            observed_views: 8,
            quality: "good".into(),
        };
        let vr = VisualReconstructResult {
            screen_id: "MAIN".into(),
            measured_yaml_path: "measurements/measured.yaml".into(),
            pose_report_path: "measurements/pose_report.json".into(),
            cabinet_count: 1,
            ba_rms_px: 0.35,
            cabinets: vec![cabinet],
        };
        let json = serde_json::to_string(&vr).unwrap();
        assert!(json.contains("\"screen_id\":\"MAIN\""));
        assert!(json.contains("\"ba_rms_px\":0.35"));
        let back: VisualReconstructResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.screen_id, "MAIN");
        assert_eq!(back.cabinets[0].cabinet_id, "MAIN_V001_R001");
        assert_eq!(back.cabinets[0].observed_views, 8);

        // SimulateResult
        let sim = SimulateResult {
            dataset_dir: "/tmp/sim".into(),
            n_views: 12,
            n_observations: 480,
            seed: 42,
        };
        let sim_json = serde_json::to_string(&sim).unwrap();
        let sim_back: SimulateResult = serde_json::from_str(&sim_json).unwrap();
        assert_eq!(sim_back.seed, 42);

        // EvalResult
        let eval = EvalResult {
            method: "visual".into(),
            max_size_error_mm: 1.5,
            max_distance_error_mm: 2.0,
            max_angle_error_deg: 0.3,
            seeds: vec![1, 2, 3],
        };
        let eval_json = serde_json::to_string(&eval).unwrap();
        let eval_back: EvalResult = serde_json::from_str(&eval_json).unwrap();
        assert_eq!(eval_back.seeds, vec![1, 2, 3]);

        // CalibrateResult
        let cal = CalibrateResult {
            intrinsics_path: "intrinsics.yaml".into(),
            reproj_error_px: 0.25,
            frames_used: 30,
        };
        let cal_json = serde_json::to_string(&cal).unwrap();
        let cal_back: CalibrateResult = serde_json::from_str(&cal_json).unwrap();
        assert_eq!(cal_back.frames_used, 30);

        // GeneratePatternResult
        let gp = GeneratePatternResult {
            output_dir: "/tmp/patterns".into(),
            cabinet_count: 12,
            total_markers: 480,
        };
        let gp_json = serde_json::to_string(&gp).unwrap();
        let gp_back: GeneratePatternResult = serde_json::from_str(&gp_json).unwrap();
        assert_eq!(gp_back.cabinet_count, 12);

        // Verify schemas are generated without panic (schemars::schema_for! is compile-time;
        // exercising dump_all() covers this at runtime).
        let dump = crate::schema::dump_all();
        for name in [
            "VisualReconstructResult",
            "CabinetPoseSummary",
            "SimulateResult",
            "EvalResult",
            "CalibrateResult",
            "GeneratePatternResult",
        ] {
            assert!(
                dump["types"][name].is_object(),
                "schema missing for {name}"
            );
        }
    }

    #[test]
    fn scatter_fit_info_from_core_roundtrips_and_has_schema() {
        use lmt_core::reconstruct::surface_fit::{
            BoundaryCheck, FrameDerivation, ScatterFit, ScatterOutlier, ScatterShape,
        };
        let core = ScatterFit {
            shape: ScatterShape::Cylinder {
                radius_mm: 9523.0,
                axis: [0.0, 0.0, 1.0],
            },
            inlier_count: 120,
            outliers: vec![ScatterOutlier {
                point_id: "row6_LEDB-1".into(),
                source_row: 6,
                coordinates: [1.0, 2.0, 3.0],
                residual_mm: 4.2,
            }],
            param_range: [-1.4, 1.4, 0.0, 7.5],
            boundary_check: BoundaryCheck {
                verdict: "ok".into(),
                projected_size_mm: [27480.0, 7500.0],
                expected_size_mm: [27500.0, 7500.0],
            },
            frame_derivation: FrameDerivation {
                axis: [0.0, 0.0, 1.0],
                origin: [0.0, 0.0, 0.0],
                unwrap_dir: "theta".into(),
            },
        };
        let dto: ScatterFitInfo = core.into();
        assert_eq!(dto.outliers[0].point_id, "row6_LEDB-1");
        let dump = crate::schema::dump_all();
        assert!(dump["types"]["ScatterFitInfo"].is_object());
    }
}

#[cfg(test)]
mod method_tests {
    use super::*;

    fn parse(yaml: &str) -> ProjectConfig {
        serde_yaml::from_str(yaml).unwrap()
    }

    const BASE: &str = r#"
project:
  name: Test
  unit: mm
{method_line}
screens:
  MAIN:
    cabinet_count: [4, 2]
    cabinet_size_mm: [500, 500]
    shape_prior:
      type: flat
    shape_mode: rectangle
    irregular_mask: []
coordinate_system:
  origin_point: MAIN_V001_R001
  x_axis_point: MAIN_V004_R001
  xy_plane_point: MAIN_V001_R002
output:
  target: disguise
  obj_filename: "{screen_id}.obj"
  weld_vertices_tolerance_mm: 1.0
  triangulate: true
"#;

    fn build(method_line: &str) -> String {
        BASE.replace("{method_line}", method_line)
    }

    #[test]
    fn method_missing_yaml_parses_as_none() {
        let cfg = parse(&build(""));
        assert_eq!(cfg.project.method, None);
    }

    #[test]
    fn method_null_yaml_parses_as_none() {
        let cfg = parse(&build("  method: null"));
        assert_eq!(cfg.project.method, None);
    }

    #[test]
    fn method_m1_yaml_roundtrips() {
        let cfg = parse(&build("  method: m1"));
        assert_eq!(cfg.project.method, Some(SurveyMethod::M1));
        let s = serde_yaml::to_string(&cfg).unwrap();
        assert!(s.contains("method: m1"), "serialized form: {}", s);
    }

    #[test]
    fn method_m2_yaml_roundtrips() {
        let cfg = parse(&build("  method: m2"));
        assert_eq!(cfg.project.method, Some(SurveyMethod::M2));
        let s = serde_yaml::to_string(&cfg).unwrap();
        assert!(s.contains("method: m2"), "serialized form: {}", s);
    }

    #[test]
    fn method_invalid_value_errors() {
        let result: Result<ProjectConfig, _> = serde_yaml::from_str(&build("  method: m3"));
        assert!(result.is_err());
    }

    #[test]
    fn none_omitted_on_serialize() {
        let cfg = parse(&build(""));
        let s = serde_yaml::to_string(&cfg).unwrap();
        assert!(!s.contains("method:"), "expected method field omitted, got: {}", s);
    }
}
