use crate::data::{recent_projects, Db};
use crate::dto::{ProjectConfig, RecentProject};
use crate::error::{LmtError, LmtResult};
use std::path::{Path, PathBuf};

/// Pure helper used by command + integration tests.
pub fn seed_example_to_dir(
    examples_root: &Path,
    example_name: &str,
    target_dir: &Path,
) -> LmtResult<PathBuf> {
    let src = examples_root.join(example_name);
    if !src.is_dir() {
        return Err(LmtError::NotFound(format!(
            "example '{example_name}' (looked in {})",
            examples_root.display()
        )));
    }
    let dst = target_dir.join(example_name);
    copy_dir_recursive(&src, &dst)?;
    Ok(dst)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> LmtResult<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

// ── project.yaml load / save ──────────────────────────────────────────────────

/// Pure helper: read project.yaml from `abs_path/project.yaml`.
/// Returns `NotFound` if the file does not exist.
pub fn load_project_yaml_from_path(abs_path: &Path) -> LmtResult<ProjectConfig> {
    let yaml_path = abs_path.join("project.yaml");
    if !yaml_path.is_file() {
        return Err(LmtError::NotFound(yaml_path.display().to_string()));
    }
    let yaml = std::fs::read_to_string(&yaml_path)?;
    Ok(serde_yaml::from_str(&yaml)?)
}

/// Pure helper: write `config` to `abs_path/project.yaml` atomically (temp + rename).
pub fn save_project_yaml_to_path(abs_path: &Path, config: &ProjectConfig) -> LmtResult<()> {
    std::fs::create_dir_all(abs_path)?;
    let yaml = serde_yaml::to_string(config)?;
    let final_path = abs_path.join("project.yaml");
    let tmp_path = abs_path.join("project.yaml.tmp");
    std::fs::write(&tmp_path, yaml)?;
    std::fs::rename(&tmp_path, &final_path)?;
    Ok(())
}

#[tauri::command]
pub fn load_project_yaml(abs_path: String) -> LmtResult<ProjectConfig> {
    load_project_yaml_from_path(Path::new(&abs_path))
}

#[tauri::command]
pub fn save_project_yaml(abs_path: String, config: ProjectConfig) -> LmtResult<()> {
    save_project_yaml_to_path(Path::new(&abs_path), &config)
}

// ── recent_projects commands ──────────────────────────────────────────────────

#[tauri::command]
pub fn list_recent_projects(state: tauri::State<'_, Db>) -> LmtResult<Vec<RecentProject>> {
    let conn = state.lock().unwrap();
    recent_projects::list(&conn)
}

#[tauri::command]
pub fn add_recent_project(
    state: tauri::State<'_, Db>,
    abs_path: String,
    display_name: String,
) -> LmtResult<RecentProject> {
    let conn = state.lock().unwrap();
    recent_projects::upsert(&conn, &abs_path, &display_name)
}

#[tauri::command]
pub fn remove_recent_project(state: tauri::State<'_, Db>, id: i64) -> LmtResult<()> {
    let conn = state.lock().unwrap();
    recent_projects::delete(&conn, id)
}

// ── seed_example_project ──────────────────────────────────────────────────────

#[tauri::command]
pub fn seed_example_project(
    app: tauri::AppHandle,
    target_dir: String,
    example: String,
) -> LmtResult<String> {
    use tauri::{Emitter, Manager};
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| LmtError::Io(e.to_string()))?;
    let examples_root = resource_dir.join("examples");
    let out = seed_example_to_dir(&examples_root, &example, Path::new(&target_dir))?;
    let _ = app.emit(
        "project-seeded",
        serde_json::json!({"abs_path": out.display().to_string()}),
    );
    Ok(out.display().to_string())
}

#[cfg(test)]
mod project_yaml_method_tests {
    use super::*;
    use crate::dto::{ProjectConfig, ProjectMeta, SurveyMethod};
    use tempfile::tempdir;

    fn minimal_config(method: Option<SurveyMethod>) -> ProjectConfig {
        use crate::dto::{
            CoordinateSystemConfig, OutputConfig, ScreenConfig, ShapeMode, ShapePriorConfig,
        };
        use std::collections::BTreeMap;

        let mut screens = BTreeMap::new();
        screens.insert(
            "MAIN".to_string(),
            ScreenConfig {
                cabinet_count: [4, 2],
                cabinet_size_mm: [500.0, 500.0],
                pixels_per_cabinet: None,
                shape_prior: ShapePriorConfig::Flat,
                shape_mode: ShapeMode::Rectangle,
                irregular_mask: vec![],
                bottom_completion: None,
            },
        );
        ProjectConfig {
            project: ProjectMeta {
                name: "X".into(),
                unit: "mm".into(),
                method,
            },
            screens,
            coordinate_system: CoordinateSystemConfig {
                origin_point: "MAIN_V001_R001".into(),
                x_axis_point: "MAIN_V004_R001".into(),
                xy_plane_point: "MAIN_V001_R002".into(),
            },
            output: OutputConfig {
                target: "disguise".into(),
                obj_filename: "{screen_id}.obj".into(),
                weld_vertices_tolerance_mm: 1.0,
                triangulate: true,
            },
        }
    }

    #[test]
    fn load_save_roundtrip_with_method_m1() {
        let dir = tempdir().unwrap();
        let cfg = minimal_config(Some(SurveyMethod::M1));
        save_project_yaml_to_path(dir.path(), &cfg).unwrap();
        let loaded = load_project_yaml_from_path(dir.path()).unwrap();
        assert_eq!(loaded.project.method, Some(SurveyMethod::M1));
    }

    #[test]
    fn load_save_roundtrip_with_method_m2() {
        let dir = tempdir().unwrap();
        let cfg = minimal_config(Some(SurveyMethod::M2));
        save_project_yaml_to_path(dir.path(), &cfg).unwrap();
        let loaded = load_project_yaml_from_path(dir.path()).unwrap();
        assert_eq!(loaded.project.method, Some(SurveyMethod::M2));
    }

    #[test]
    fn load_legacy_yaml_without_method() {
        let dir = tempdir().unwrap();
        let legacy = r#"
project:
  name: Legacy
  unit: mm
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
        std::fs::write(dir.path().join("project.yaml"), legacy).unwrap();
        let loaded = load_project_yaml_from_path(dir.path()).unwrap();
        assert_eq!(loaded.project.method, None);
    }
}
