pub use lmt_app::measurements::load_measurements_from_path;

use lmt_core::measured_points::MeasuredPoints;
use lmt_shared::error::LmtResult;
use std::path::Path;

#[tauri::command]
pub fn load_measurements_yaml(path: String) -> LmtResult<MeasuredPoints> {
    load_measurements_from_path(Path::new(&path))
}
