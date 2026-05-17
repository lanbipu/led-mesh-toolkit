pub use lmt_app::reconstruct::{list_runs_for, read_run_report, run_reconstruction};

use lmt_shared::data::Db;
use lmt_shared::dto::{ReconstructionResult, ReconstructionRun};
use lmt_shared::error::LmtResult;
use std::path::Path;

#[tauri::command]
pub fn reconstruct_surface(
    state: tauri::State<'_, Db>,
    project_path: String,
    screen_id: String,
    measurements_path: String,
) -> LmtResult<ReconstructionResult> {
    run_reconstruction(
        state.inner().clone(),
        Path::new(&project_path),
        &screen_id,
        &measurements_path,
    )
}

#[tauri::command]
pub fn list_runs(
    state: tauri::State<'_, Db>,
    project_path: String,
    screen_id: Option<String>,
) -> LmtResult<Vec<ReconstructionRun>> {
    list_runs_for(state.inner().clone(), &project_path, screen_id.as_deref())
}

#[tauri::command]
pub fn get_run_report(state: tauri::State<'_, Db>, run_id: i64) -> LmtResult<serde_json::Value> {
    read_run_report(state.inner().clone(), run_id)
}
