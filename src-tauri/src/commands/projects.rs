pub use lmt_app::projects::{
    load_project_yaml_from_path, save_project_yaml_to_path, seed_example_to_dir,
};

use lmt_shared::data::{recent_projects, Db};
use lmt_shared::dto::{ProjectConfig, RecentProject};
use lmt_shared::error::{LmtError, LmtResult};
use std::path::Path;

#[tauri::command]
pub fn load_project_yaml(abs_path: String) -> LmtResult<ProjectConfig> {
    load_project_yaml_from_path(Path::new(&abs_path))
}

#[tauri::command]
pub fn save_project_yaml(abs_path: String, config: ProjectConfig) -> LmtResult<()> {
    save_project_yaml_to_path(Path::new(&abs_path), &config)
}

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
