use crate::data::{runs, Db};
use crate::dto::{ReconstructionReport, ShapeMode};
use crate::error::{LmtError, LmtResult};
use lmt_core::export::build::surface_to_mesh_output;
use lmt_core::export::obj::write_obj;
use lmt_core::shape::CabinetArray;
use lmt_core::surface::TargetSoftware;
use std::path::{Path, PathBuf};

fn parse_target(s: &str) -> LmtResult<TargetSoftware> {
    match s {
        "disguise" => Ok(TargetSoftware::Disguise),
        "unreal" => Ok(TargetSoftware::Unreal),
        "neutral" => Ok(TargetSoftware::Neutral),
        other => Err(LmtError::InvalidInput(format!("unknown target: {other}"))),
    }
}

pub fn build_cabinet_array(screen_cfg: &crate::dto::ScreenConfig) -> LmtResult<CabinetArray> {
    let [cols, rows] = screen_cfg.cabinet_count;
    let cabinet_size_mm = screen_cfg.cabinet_size_mm;
    match screen_cfg.shape_mode {
        ShapeMode::Rectangle => Ok(CabinetArray::rectangle(cols, rows, cabinet_size_mm)),
        ShapeMode::Irregular => {
            let absent: Vec<(u32, u32)> = screen_cfg
                .irregular_mask
                .iter()
                .map(|&[c, r]| (c, r))
                .collect();
            Ok(CabinetArray::irregular(cols, rows, cabinet_size_mm, absent))
        }
    }
}

pub fn run_export(
    db: Db,
    run_id: i64,
    target: &str,
    dst_abs_path: Option<&std::path::Path>,
) -> LmtResult<String> {
    let target_enum = parse_target(target)?;

    let (project_path, report_rel) = {
        let conn = db.lock().unwrap();
        runs::get_report_path(&conn, run_id)?
    };

    let project_root = PathBuf::from(&project_path);
    let report_abs = project_root.join(&report_rel);
    let report: ReconstructionReport = serde_json::from_slice(&std::fs::read(&report_abs)?)?;

    // Use snapshotted values from the report — no re-read of project.yaml.
    let weld_m = report.weld_tolerance_mm / 1000.0;
    let mesh = surface_to_mesh_output(&report.surface, &report.cabinet_array, target_enum, weld_m)?;

    // Caller-chosen destination (from a save dialog) takes precedence; otherwise
    // fall back to the legacy `{project}/output/<screen>_<target>_run<id>.obj`.
    //
    // DB bookkeeping (`runs.output_obj_path`): project-relative when the
    // chosen path is under `{project}/`, else absolute. UI must handle both
    // (an absolute path here will not survive a cross-machine project move —
    // M1.1 scope, revisit when project archive/import is added).
    let (out_abs, out_rel_for_db) = match dst_abs_path {
        Some(p) => {
            let abs = ensure_obj_extension(p);
            let rel = abs
                .strip_prefix(&project_root)
                .ok()
                .map(|r| r.display().to_string())
                .unwrap_or_else(|| abs.display().to_string());
            (abs, rel)
        }
        None => {
            let rel = PathBuf::from("output")
                .join(format!("{}_{target}_run{run_id}.obj", report.screen_id));
            let abs = project_root.join(&rel);
            (abs, rel.display().to_string())
        }
    };
    if let Some(parent) = out_abs.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    write_obj(&mesh, &out_abs)?;

    {
        let conn = db.lock().unwrap();
        runs::update_export(&conn, run_id, target, &out_rel_for_db)?;
    }

    Ok(out_abs.display().to_string())
}

/// Append `.obj` if the path doesn't already end with that extension
/// (case-insensitive). Users who skip the dialog's filter and type
/// `mymesh` should still get a usable OBJ file.
fn ensure_obj_extension(p: &Path) -> PathBuf {
    match p.extension() {
        Some(ext) if ext.eq_ignore_ascii_case("obj") => p.to_path_buf(),
        _ => {
            let mut buf = p.as_os_str().to_os_string();
            buf.push(".obj");
            PathBuf::from(buf)
        }
    }
}

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
