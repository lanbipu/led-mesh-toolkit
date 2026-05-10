use crate::data::{runs, Db};
use crate::dto::{ReconstructionReport, ShapeMode};
use crate::error::{LmtError, LmtResult};
use lmt_core::export::build::surface_to_mesh_output;
use lmt_core::export::obj::write_obj;
use lmt_core::shape::CabinetArray;
use lmt_core::surface::TargetSoftware;
use std::path::PathBuf;

fn parse_target(s: &str) -> LmtResult<TargetSoftware> {
    match s {
        "disguise" => Ok(TargetSoftware::Disguise),
        "unreal" => Ok(TargetSoftware::Unreal),
        "neutral" => Ok(TargetSoftware::Neutral),
        other => Err(LmtError::InvalidInput(format!("unknown target: {other}"))),
    }
}

fn build_cabinet_array(screen_cfg: &crate::dto::ScreenConfig) -> LmtResult<CabinetArray> {
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

pub fn run_export(db: Db, run_id: i64, target: &str) -> LmtResult<String> {
    let target_enum = parse_target(target)?;

    let (project_path, report_rel) = {
        let conn = db.lock().unwrap();
        runs::get_report_path(&conn, run_id)?
    };

    let project_root = PathBuf::from(&project_path);
    let report_abs = project_root.join(&report_rel);
    let report: ReconstructionReport = serde_json::from_slice(&std::fs::read(&report_abs)?)?;

    let yaml = std::fs::read_to_string(project_root.join("project.yaml"))?;
    let cfg: crate::dto::ProjectConfig = serde_yaml::from_str(&yaml)?;

    let screen_cfg = cfg
        .screens
        .get(&report.screen_id)
        .ok_or_else(|| LmtError::NotFound(format!("screen {} in project.yaml", report.screen_id)))?;

    let cabinet_array = build_cabinet_array(screen_cfg)?;
    let weld_m = cfg.output.weld_vertices_tolerance_mm / 1000.0;

    let mesh = surface_to_mesh_output(&report.surface, &cabinet_array, target_enum, weld_m)?;

    let out_rel = PathBuf::from("output").join(format!("{}_{target}.obj", report.screen_id));
    let out_abs = project_root.join(&out_rel);
    std::fs::create_dir_all(out_abs.parent().unwrap())?;
    write_obj(&mesh, &out_abs)?;

    {
        let conn = db.lock().unwrap();
        runs::update_export(&conn, run_id, target, &out_rel.display().to_string())?;
    }

    Ok(out_abs.display().to_string())
}

#[tauri::command]
pub fn export_obj(
    state: tauri::State<'_, Db>,
    run_id: i64,
    target: String,
) -> LmtResult<String> {
    run_export(state.inner().clone(), run_id, &target)
}
