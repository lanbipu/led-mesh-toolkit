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
