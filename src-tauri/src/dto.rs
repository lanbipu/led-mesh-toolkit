use lmt_core::{shape::CabinetArray, surface::QualityMetrics, surface::ReconstructedSurface};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentProject {
    pub id: i64,
    pub abs_path: String,
    pub display_name: String,
    pub last_opened_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub project: ProjectMeta,
    pub screens: BTreeMap<String, ScreenConfig>,
    pub coordinate_system: CoordinateSystemConfig,
    pub output: OutputConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMeta {
    pub name: String,
    pub unit: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<SurveyMethod>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SurveyMethod {
    M1,
    M2,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ShapeMode {
    Rectangle,
    Irregular,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BottomCompletionConfig {
    pub lowest_measurable_row: u32,
    pub fallback_method: String,
    pub assumed_height_mm: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinateSystemConfig {
    pub origin_point: String,
    pub x_axis_point: String,
    pub xy_plane_point: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputConfig {
    pub target: String,
    pub obj_filename: String,
    pub weld_vertices_tolerance_mm: f64,
    pub triangulate: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReconstructionResult {
    pub run_id: i64,
    pub surface: ReconstructedSurface,
    pub report_json_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstructionCardResult {
    /// HTML 字符串，前端 iframe srcdoc 渲染。PDF 通过单独的
    /// `save_instruction_pdf` 命令按用户选定的目标路径写盘。
    pub html_content: String,
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
