use crate::commands::measurements::load_measurements_from_path;
use crate::data::{runs, Db};
use crate::dto::{ReconstructionReport, ReconstructionResult};
use crate::error::{LmtResult};
use chrono::Utc;
use lmt_core::reconstruct::auto_reconstruct;
use std::path::{Path, PathBuf};

pub fn run_reconstruction(
    db: Db,
    project_path: &Path,
    screen_id: &str,
    measurements_rel_path: &str,
) -> LmtResult<ReconstructionResult> {
    let m_abs = project_path.join(measurements_rel_path);
    let measurements = load_measurements_from_path(&m_abs)?;
    let surface = auto_reconstruct(&measurements)?;
    let metrics = surface.quality_metrics.clone();

    let now = Utc::now();
    let stamp = now.format("%Y-%m-%dT%H-%M-%S%.3f").to_string();
    let report_rel = PathBuf::from("reports").join(format!("{stamp}.json"));
    let report_abs = project_path.join(&report_rel);
    std::fs::create_dir_all(report_abs.parent().unwrap())?;

    let report = ReconstructionReport {
        surface: surface.clone(),
        quality_metrics: metrics.clone(),
        project_path: project_path.display().to_string(),
        screen_id: screen_id.to_string(),
        measurements_path: measurements_rel_path.to_string(),
        created_at: now.to_rfc3339(),
    };
    let json =
        serde_json::to_vec_pretty(&report).map_err(|e| crate::error::LmtError::Yaml(format!("json: {e}")))?;
    std::fs::write(&report_abs, json)?;

    let warnings_json = serde_json::to_string(&metrics.warnings)
        .map_err(|e| crate::error::LmtError::Yaml(format!("json: {e}")))?;

    let run_id = {
        let conn = db.lock().unwrap();
        runs::insert(
            &conn,
            &runs::NewRun {
                project_path: project_path.display().to_string(),
                screen_id: screen_id.to_string(),
                measurements_path: measurements_rel_path.to_string(),
                method: metrics.method.clone(),
                measured_count: metrics.measured_count,
                expected_count: metrics.expected_count,
                estimated_rms_mm: metrics.estimated_rms_mm,
                estimated_p95_mm: metrics.estimated_p95_mm,
                vertex_count: surface.vertices.len(),
                report_json_path: report_rel.display().to_string(),
                warnings_json,
            },
        )?
    };

    Ok(ReconstructionResult {
        run_id,
        surface,
        report_json_path: report_rel.display().to_string(),
    })
}

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
