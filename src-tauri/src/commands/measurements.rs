use crate::error::{LmtError, LmtResult};
use lmt_core::measured_points::MeasuredPoints;
use std::path::Path;

/// Pure helper: read `measured.yaml` from an absolute file path.
/// Returns `NotFound` if the file does not exist.
pub fn load_measurements_from_path(path: &Path) -> LmtResult<MeasuredPoints> {
    if !path.is_file() {
        return Err(LmtError::NotFound(path.display().to_string()));
    }
    let yaml = std::fs::read_to_string(path)?;
    Ok(serde_yaml::from_str(&yaml)?)
}

#[tauri::command]
pub fn load_measurements_yaml(path: String) -> LmtResult<MeasuredPoints> {
    load_measurements_from_path(Path::new(&path))
}
