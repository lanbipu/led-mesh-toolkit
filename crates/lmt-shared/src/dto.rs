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
