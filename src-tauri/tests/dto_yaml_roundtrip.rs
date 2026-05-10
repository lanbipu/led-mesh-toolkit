use lmt_tauri_lib::dto::*;

#[test]
fn project_config_yaml_roundtrip_matches_spec_fixture() {
    let yaml = r#"
project:
  name: "Studio_A_Volume"
  unit: "mm"
screens:
  MAIN:
    cabinet_count: [120, 20]
    cabinet_size_mm: [500, 500]
    pixels_per_cabinet: [256, 256]
    shape_prior:
      type: curved
      radius_mm: 30000
      fold_seams_at_columns: []
    shape_mode: rectangle
    irregular_mask: []
    bottom_completion:
      lowest_measurable_row: 5
      fallback_method: vertical
      assumed_height_mm: 2000
coordinate_system:
  origin_point: "MAIN_V001_R005"
  x_axis_point: "MAIN_V120_R005"
  xy_plane_point: "MAIN_V001_R020"
output:
  target: disguise
  obj_filename: "{screen_id}_mesh.obj"
  weld_vertices_tolerance_mm: 1.0
  triangulate: true
"#;

    let cfg: ProjectConfig = serde_yaml::from_str(yaml).expect("parse");
    assert_eq!(cfg.project.name, "Studio_A_Volume");
    assert_eq!(cfg.screens["MAIN"].cabinet_count, [120, 20]);
    assert_eq!(cfg.coordinate_system.origin_point, "MAIN_V001_R005");

    let back = serde_yaml::to_string(&cfg).expect("serialize");
    let cfg2: ProjectConfig = serde_yaml::from_str(&back).expect("reparse");
    assert_eq!(cfg2.project.name, cfg.project.name);
}
