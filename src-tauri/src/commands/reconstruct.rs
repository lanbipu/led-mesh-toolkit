use crate::commands::export::build_cabinet_array;
use crate::commands::measurements::load_measurements_from_path;
use crate::data::{runs, Db};
use crate::dto::{ReconstructionReport, ReconstructionResult};
use crate::error::{LmtError, LmtResult};
use chrono::Utc;
use lmt_core::reconstruct::auto_reconstruct;
use std::path::{Path, PathBuf};

pub fn run_reconstruction(
    db: Db,
    project_path: &Path,
    screen_id: &str,
    measurements_rel_path: &str,
) -> LmtResult<ReconstructionResult> {
    // Load project.yaml to snapshot cabinet_array and weld_tolerance at this moment.
    let yaml = std::fs::read_to_string(project_path.join("project.yaml"))?;
    let cfg: crate::dto::ProjectConfig =
        serde_yaml::from_str(&yaml).map_err(|e| LmtError::Yaml(format!("project.yaml: {e}")))?;
    let screen_cfg = cfg
        .screens
        .get(screen_id)
        .ok_or_else(|| LmtError::NotFound(format!("screen {screen_id} in project.yaml")))?;
    let cabinet_array = build_cabinet_array(screen_cfg)?;
    let weld_tolerance_mm = cfg.output.weld_vertices_tolerance_mm;

    let m_abs = project_path.join(measurements_rel_path);
    let measurements = load_measurements_from_path(&m_abs)?;
    tracing::info!(
        project_path = %project_path.display(),
        screen_id = %screen_id,
        measurements_abs = %m_abs.display(),
        points_count = measurements.points.len(),
        measurements_screen_id = %measurements.screen_id,
        cabinet_cols = measurements.cabinet_array.cols,
        cabinet_rows = measurements.cabinet_array.rows,
        shape_prior = ?measurements.shape_prior,
        first_point = measurements.points.first().map(|p| p.name.as_str()).unwrap_or("(empty)"),
        "reconstruct: loaded measurements",
    );
    let surface = auto_reconstruct(&measurements).map_err(|e| {
        tracing::error!(
            error = %e,
            points_count = measurements.points.len(),
            cabinet_cols = measurements.cabinet_array.cols,
            cabinet_rows = measurements.cabinet_array.rows,
            shape_prior = ?measurements.shape_prior,
            "reconstruct: auto_reconstruct failed",
        );
        LmtError::from(e)
    })?;
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
        cabinet_array,
        weld_tolerance_mm,
    };
    let json = serde_json::to_vec_pretty(&report)
        .map_err(|e| crate::error::LmtError::Yaml(format!("json: {e}")))?;
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

pub fn list_runs_for(
    db: Db,
    project_path: &str,
    screen_id: Option<&str>,
) -> LmtResult<Vec<crate::dto::ReconstructionRun>> {
    let conn = db.lock().unwrap();
    runs::list_by_project(&conn, project_path, screen_id)
}

pub fn read_run_report(db: Db, run_id: i64) -> LmtResult<serde_json::Value> {
    let (project_path, report_rel) = {
        let conn = db.lock().unwrap();
        runs::get_report_path(&conn, run_id)?
    };
    let report_abs = PathBuf::from(&project_path).join(&report_rel);
    let bytes = std::fs::read(&report_abs)?;
    serde_json::from_slice(&bytes).map_err(|e| crate::error::LmtError::Yaml(format!("json: {e}")))
}

#[tauri::command]
pub fn list_runs(
    state: tauri::State<'_, Db>,
    project_path: String,
    screen_id: Option<String>,
) -> LmtResult<Vec<crate::dto::ReconstructionRun>> {
    list_runs_for(state.inner().clone(), &project_path, screen_id.as_deref())
}

#[tauri::command]
pub fn get_run_report(state: tauri::State<'_, Db>, run_id: i64) -> LmtResult<serde_json::Value> {
    read_run_report(state.inner().clone(), run_id)
}
