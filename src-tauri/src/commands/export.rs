pub use lmt_app::export::{build_cabinet_array, run_export};

use lmt_shared::data::Db;
use lmt_shared::error::LmtResult;

#[tauri::command]
pub fn export_obj(
    state: tauri::State<'_, Db>,
    run_id: i64,
    target: String,
    dst_abs_path: Option<String>,
) -> LmtResult<String> {
    let dst = dst_abs_path.as_deref().map(std::path::Path::new);
    run_export(state.inner().clone(), run_id, &target, dst)
}
