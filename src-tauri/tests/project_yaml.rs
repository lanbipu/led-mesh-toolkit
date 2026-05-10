use lmt_tauri_lib::commands::projects::{load_project_yaml_from_path, save_project_yaml_to_path};
use lmt_tauri_lib::dto::ProjectConfig;
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn save_then_load_round_trips() {
    let dir = TempDir::new().unwrap();
    let yaml = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/curved-flat/project.yaml"),
    )
    .unwrap();
    let cfg: ProjectConfig = serde_yaml::from_str(&yaml).unwrap();

    save_project_yaml_to_path(dir.path(), &cfg).unwrap();
    assert!(dir.path().join("project.yaml").exists());
    let loaded = load_project_yaml_from_path(dir.path()).unwrap();
    assert_eq!(loaded.project.name, cfg.project.name);
}

#[test]
fn load_missing_returns_not_found() {
    let dir = TempDir::new().unwrap();
    let err = load_project_yaml_from_path(dir.path()).unwrap_err();
    assert!(matches!(
        err,
        lmt_tauri_lib::error::LmtError::NotFound(_) | lmt_tauri_lib::error::LmtError::Io(_)
    ));
}
