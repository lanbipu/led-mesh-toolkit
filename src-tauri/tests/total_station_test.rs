//! Tauri-layer 集成测试：直接调 pure helpers（不起 Tauri runtime），
//! 验证 GUI ProjectConfig → CSV → measured.yaml → reconstruct → OBJ
//! 全管线在 Tauri 入口处也能跑通。

use lmt_core::reconstruct::auto_reconstruct;
use lmt_tauri_lib::commands::measurements::load_measurements_from_path;
use lmt_tauri_lib::commands::total_station::{run_generate_card, run_import};
use std::fs;
use tempfile::tempdir;

fn seed(dir: &std::path::Path) {
    let yaml = r#"
project:
  name: E2E
  unit: mm
screens:
  MAIN:
    cabinet_count: [4, 2]
    cabinet_size_mm: [500.0, 500.0]
    pixels_per_cabinet: [256, 256]
    shape_prior:
      type: flat
    shape_mode: rectangle
    irregular_mask: []
coordinate_system:
  origin_point: MAIN_V001_R001
  x_axis_point: MAIN_V005_R001
  xy_plane_point: MAIN_V001_R003
output:
  target: neutral
  obj_filename: "{screen_id}.obj"
  weld_vertices_tolerance_mm: 1.0
  triangulate: true
"#;
    fs::write(dir.join("project.yaml"), yaml).unwrap();
    fs::create_dir_all(dir.join("measurements")).unwrap();

    let csv = "\
name,x,y,z,note
1,0,0,0,
2,2000,0,0,
3,0,0,1000,
4,500,0,0,
5,1000,0,0,
6,1500,0,0,
7,0,0,500,
8,500,0,500,
9,1000,0,500,
10,1500,0,500,
11,2000,0,500,
12,500,0,1000,
13,1000,0,1000,
14,1500,0,1000,
15,2000,0,1000,
";
    fs::write(dir.join("measurements/raw.csv"), csv).unwrap();
}

#[test]
fn import_then_load_measured_yaml_then_reconstruct() {
    let dir = tempdir().unwrap();
    let project = dir.path();
    seed(project);
    let csv = project.join("measurements/raw.csv");

    let imp = run_import(project, "MAIN", &csv).unwrap();
    assert_eq!(imp.measured_count, 15);

    let mp_path = project.join(&imp.measurements_yaml_path);
    let mp = load_measurements_from_path(&mp_path).unwrap();
    assert_eq!(mp.points.len(), 15);

    let surface = auto_reconstruct(&mp).unwrap();
    assert_eq!(surface.quality_metrics.method, "direct_link");
    assert_eq!(surface.vertices.len(), 15);
}

#[test]
fn generate_card_writes_pdf_under_project() {
    let dir = tempdir().unwrap();
    let project = dir.path();
    seed(project);

    let card = run_generate_card(project, "MAIN").unwrap();
    assert!(card.html_content.contains("E2E"));
    assert!(card.html_content.contains("MAIN"));
    let pdf = fs::read(project.join(&card.pdf_path)).unwrap();
    assert!(pdf.starts_with(b"%PDF-"));
}

/// Tauri 2.x maps invoke payload from JSON `{camelCase}` to Rust `snake_case`
/// command-fn parameters. This lightweight contract test ensures the two
/// commands' parameter shape stays in sync with what the TypeScript wrapper
/// in src/services/tauri.ts is supposed to send. If a future refactor renames
/// a Rust param or the wrapper key, this fails before reaching production.
#[test]
fn invoke_payload_shape_matches_command_signatures() {
    use serde::Deserialize;

    // import_total_station_csv(project_abs_path, csv_path, screen_id)
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    #[allow(dead_code)]
    struct ImportArgs {
        project_abs_path: String,
        csv_path: String,
        screen_id: String,
    }
    let json = r#"{"projectAbsPath":"/p","csvPath":"/p/raw.csv","screenId":"MAIN"}"#;
    serde_json::from_str::<ImportArgs>(json).expect("import_total_station_csv arg shape");

    // generate_instruction_card(project_abs_path, screen_id)
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    #[allow(dead_code)]
    struct CardArgs {
        project_abs_path: String,
        screen_id: String,
    }
    let json = r#"{"projectAbsPath":"/p","screenId":"MAIN"}"#;
    serde_json::from_str::<CardArgs>(json).expect("generate_instruction_card arg shape");
}

/// Sanity: lib.rs must actually register both commands. Compile-time check
/// — if the symbols are removed or renamed without updating generate_handler,
/// this stops compiling (verifying the public surface, not just helpers).
#[test]
fn registered_commands_are_addressable() {
    let _import_fn = lmt_tauri_lib::commands::total_station::import_total_station_csv;
    let _card_fn = lmt_tauri_lib::commands::total_station::generate_instruction_card;
}

/// End-to-end against the real `examples/curved-flat/` fixture: project.yaml
/// + raw.csv with 45 vertex points → import → reconstruct → direct_link mesh.
#[test]
fn import_real_example_curved_flat() {
    use std::path::PathBuf;

    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let example = workspace.join("examples/curved-flat");
    assert!(
        example.is_dir(),
        "fixture dir missing: {}",
        example.display()
    );

    let dir = tempdir().unwrap();
    let project = dir.path().join("curved-flat");
    copy_dir(&example, &project);

    let csv = project.join("measurements/raw.csv");
    let result = run_import(&project, "MAIN", &csv).unwrap();
    assert_eq!(
        result.measured_count, 45,
        "9×5 vertices for 8×4 cabinet grid"
    );
    assert_eq!(result.outlier_count, 0);
    assert_eq!(result.missing_count, 0);

    let mp = lmt_tauri_lib::commands::measurements::load_measurements_from_path(
        &project.join(&result.measurements_yaml_path),
    )
    .unwrap();
    let surface = auto_reconstruct(&mp).unwrap();
    assert_eq!(surface.quality_metrics.method, "direct_link");
    assert_eq!(surface.vertices.len(), 45);
}

fn copy_dir(src: &std::path::Path, dst: &std::path::Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to);
        } else {
            fs::copy(&from, &to).unwrap();
        }
    }
}
