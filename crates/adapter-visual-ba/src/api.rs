//! High-level public API for the visual-BA adapter.
//!
//! One async fn per sidecar subcommand. Each builds the JSON payload, runs the
//! sidecar via [`run_sidecar`] (which returns the raw result `data`), and
//! deserializes it into the subcommand's concrete result type. The adapter
//! keeps its OWN ipc/result types (mirroring the sidecar); lmt-app maps them to
//! lmt-shared DTOs.

use std::path::Path;

use lmt_core::measured_points::MeasuredPoints;
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot};

use crate::error::{VbaError, VbaResult};
use crate::ipc::{
    CabinetArray as IpcCabinetArray, CabinetSummary, CoordinateFrame as IpcCoordinateFrame,
    EvalResultData, Event, ReconstructProject, ResultData, ShapePrior as IpcShapePrior,
    SimulateResultData,
};
use crate::sidecar::{run_sidecar, SidecarRequest};

// ---------------------------------------------------------------------------
// reconstruct
// ---------------------------------------------------------------------------

pub struct ReconstructArgs {
    pub project: ReconstructProject,
    pub capture_manifest_path: String,
    /// Optional override of the manifest's screen_mapping reference. `None`
    /// tells the sidecar to use the path the capture manifest points to.
    pub screen_mapping_path: Option<String>,
    /// Where the sidecar writes `cabinet_pose_report.json` (spec §9). The
    /// adapter reads it back to build `cabinet_summaries`.
    pub pose_report_path: String,
    pub progress_tx: Option<mpsc::Sender<Event>>,
    pub cancel: Option<oneshot::Receiver<()>>,
}

/// Output of [`reconstruct`]. `measured_points` is the primary product (cabinet
/// centers in screen-local frame); `cabinet_summaries` is a convenience digest
/// read back from the pose report on disk.
#[derive(Debug, Clone)]
pub struct ReconstructOut {
    pub measured_points: MeasuredPoints,
    pub pose_report_path: String,
    pub ba_rms_px: f64,
    pub cabinet_summaries: Vec<CabinetSummary>,
}

fn ipc_to_ir_coord(c: &IpcCoordinateFrame) -> VbaResult<lmt_core::coordinate::CoordinateFrame> {
    let json = serde_json::json!({
        "origin_world": c.origin_world,
        "basis": c.basis,
    });
    serde_json::from_value(json).map_err(|e| {
        VbaError::InvalidInput(format!("coordinate_frame failed core validation: {e}"))
    })
}

fn ipc_to_ir_cabinet(c: &IpcCabinetArray) -> VbaResult<lmt_core::shape::CabinetArray> {
    let json = serde_json::json!({
        "cols": c.cols,
        "rows": c.rows,
        "cabinet_size_mm": c.cabinet_size_mm,
        "absent_cells": c.absent_cells,
    });
    serde_json::from_value(json)
        .map_err(|e| VbaError::InvalidInput(format!("cabinet_array failed core validation: {e}")))
}

fn ipc_to_ir_shape(s: &IpcShapePrior) -> VbaResult<lmt_core::shape::ShapePrior> {
    let json = match s {
        IpcShapePrior::Flat(_) => serde_json::json!("flat"),
        IpcShapePrior::Curved { curved } => {
            serde_json::json!({"curved": {"radius_mm": curved.radius_mm}})
        }
        IpcShapePrior::Folded { folded } => {
            serde_json::json!({"folded": {"fold_seam_columns": folded.fold_seam_columns}})
        }
    };
    serde_json::from_value(json)
        .map_err(|e| VbaError::InvalidInput(format!("shape_prior failed core validation: {e}")))
}

/// Identity screen-local frame: origin at [0,0,0], basis = I. The
/// model-constrained reconstruction is already expressed in the root cabinet's
/// (screen-local) frame per spec §3, so the IR frame is identity.
fn identity_frame() -> VbaResult<lmt_core::coordinate::CoordinateFrame> {
    ipc_to_ir_coord(&IpcCoordinateFrame {
        origin_world: [0.0, 0.0, 0.0],
        basis: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
    })
}

/// Pre-validate the project against the core IR's stricter rules so we fail
/// fast — before spawning a multi-minute sidecar run — when dimensions are
/// oversized or sizes are non-positive. (Coordinate frame is no longer part of
/// the project; the output uses an identity screen-local frame.)
fn validate_project_eagerly(p: &ReconstructProject) -> VbaResult<()> {
    ipc_to_ir_cabinet(&p.cabinet_array)?;
    ipc_to_ir_shape(&p.shape_prior)?;
    Ok(())
}

/// Best-effort read of the cabinet pose report into summaries. A missing or
/// unreadable report is not fatal — the MeasuredPoints are the primary output —
/// so this returns an empty Vec rather than an error in that case. (The adapter
/// is a library: it writes nothing to stdout.)
fn read_cabinet_summaries(pose_report_path: &str) -> Vec<CabinetSummary> {
    let raw = match std::fs::read_to_string(pose_report_path) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let report: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    match report.get("cabinet_poses") {
        Some(poses) => serde_json::from_value(poses.clone()).unwrap_or_default(),
        None => Vec::new(),
    }
}

pub async fn reconstruct(args: ReconstructArgs) -> VbaResult<ReconstructOut> {
    validate_project_eagerly(&args.project)?;

    let mut payload = json!({
        "command": "reconstruct",
        "version": 1,
        "project": &args.project,
        "capture_manifest_path": &args.capture_manifest_path,
        "pose_report_path": &args.pose_report_path,
    });
    // Omit screen_mapping_path when None so the sidecar falls back to the
    // manifest's reference (its `None` default).
    if let Some(p) = &args.screen_mapping_path {
        payload["screen_mapping_path"] = json!(p);
    }

    let value = run_sidecar(SidecarRequest {
        subcommand: "reconstruct".into(),
        payload,
        progress_tx: args.progress_tx,
        cancel: args.cancel,
    })
    .await?;

    // A result we can't decode is a sidecar protocol violation, not caller
    // error → BadEventJson, not InvalidInput.
    let result: ResultData = serde_json::from_value(value).map_err(VbaError::BadEventJson)?;

    let ba_rms_px = result.ba_stats.rms_reprojection_px;
    let points: Vec<lmt_core::point::MeasuredPoint> = result
        .measured_points
        .into_iter()
        .map(|dto| dto.into_ir())
        .collect();

    let measured_points = MeasuredPoints {
        screen_id: args.project.screen_id.clone(),
        coordinate_frame: identity_frame()?,
        cabinet_array: ipc_to_ir_cabinet(&args.project.cabinet_array)?,
        shape_prior: ipc_to_ir_shape(&args.project.shape_prior)?,
        points,
        sampling_mode: lmt_core::sampling::SamplingMode::Grid,
    };

    let cabinet_summaries = read_cabinet_summaries(&args.pose_report_path);

    Ok(ReconstructOut {
        measured_points,
        pose_report_path: args.pose_report_path,
        ba_rms_px,
        cabinet_summaries,
    })
}

// ---------------------------------------------------------------------------
// calibrate
// ---------------------------------------------------------------------------

pub struct CalibrateArgs {
    pub checkerboard_images: Vec<String>,
    pub inner_corners: [u32; 2],
    pub square_size_mm: f64,
    pub output_path: String,
    pub progress_tx: Option<mpsc::Sender<Event>>,
    pub cancel: Option<oneshot::Receiver<()>>,
}

#[derive(Debug, Clone)]
pub struct CalibrateOut {
    pub intrinsics_path: String,
    pub reproj_error_px: f64,
    pub frames_used: u32,
}

pub async fn calibrate(args: CalibrateArgs) -> VbaResult<CalibrateOut> {
    let payload = json!({
        "command": "calibrate",
        "version": 1,
        "checkerboard_images": &args.checkerboard_images,
        "inner_corners": args.inner_corners,
        "square_size_mm": args.square_size_mm,
        "output_path": &args.output_path,
    });

    // calibrate's result event is a vestigial ResultData (`iterations` is
    // hard-coded to 0 in the sidecar — see calibrate.py), so it's NOT a
    // reliable source for the frame count. Run for side effects + error
    // surfacing, then read the authoritative values from the intrinsics JSON
    // the sidecar writes to `output_path` (it carries both `reproj_error_px`
    // and `frames_used = len(obj_points)`), mirroring how `generate_pattern`
    // reads pattern_meta.json.
    let _value = run_sidecar(SidecarRequest {
        subcommand: "calibrate".into(),
        payload,
        progress_tx: args.progress_tx,
        cancel: args.cancel,
    })
    .await?;

    #[derive(serde::Deserialize)]
    struct IntrinsicsFile {
        reproj_error_px: f64,
        frames_used: u32,
    }

    let intr: IntrinsicsFile = serde_json::from_str(
        &std::fs::read_to_string(&args.output_path)
            .map_err(|e| VbaError::InvalidInput(format!("intrinsics file unreadable: {e}")))?,
    )
    .map_err(|e| VbaError::InvalidInput(format!("intrinsics file decode failed: {e}")))?;

    Ok(CalibrateOut {
        intrinsics_path: args.output_path,
        reproj_error_px: intr.reproj_error_px,
        frames_used: intr.frames_used,
    })
}

// ---------------------------------------------------------------------------
// generate_pattern
// ---------------------------------------------------------------------------

pub struct GeneratePatternArgs {
    pub screen_id: String,
    pub cabinet_array: IpcCabinetArray,
    pub output_dir: String,
    pub screen_resolution: [u32; 2],
    pub progress_tx: Option<mpsc::Sender<Event>>,
    pub cancel: Option<oneshot::Receiver<()>>,
}

#[derive(Debug, Clone)]
pub struct GeneratePatternOut {
    pub output_dir: String,
    pub cabinet_count: u32,
    pub markers_per_cabinet: u32,
}

pub async fn generate_pattern(args: GeneratePatternArgs) -> VbaResult<GeneratePatternOut> {
    let payload = json!({
        "command": "generate_pattern",
        "version": 1,
        "project": {
            "screen_id": &args.screen_id,
            "cabinet_array": &args.cabinet_array,
        },
        "output_dir": &args.output_dir,
        "screen_resolution": args.screen_resolution,
    });

    // generate_pattern's result event is an empty ResultData; the real product
    // is the files on disk. Run for the side effects + error surfacing, then
    // read the produced pattern_meta.json for counts.
    let _value = run_sidecar(SidecarRequest {
        subcommand: "generate_pattern".into(),
        payload,
        progress_tx: args.progress_tx,
        cancel: args.cancel,
    })
    .await?;

    let meta_path = Path::new(&args.output_dir).join("pattern_meta.json");
    let meta: crate::ipc::PatternMeta = serde_json::from_str(
        &std::fs::read_to_string(&meta_path)
            .map_err(|e| VbaError::InvalidInput(format!("pattern_meta.json unreadable: {e}")))?,
    )
    .map_err(|e| VbaError::InvalidInput(format!("pattern_meta.json decode failed: {e}")))?;

    Ok(GeneratePatternOut {
        output_dir: args.output_dir,
        cabinet_count: meta.cabinets.len() as u32,
        markers_per_cabinet: meta.markers_per_cabinet,
    })
}

// ---------------------------------------------------------------------------
// simulate
// ---------------------------------------------------------------------------

pub struct SimulateArgs {
    /// The `{scene, cameras, intrinsics, noise, seed, out_dir}` object. The
    /// adapter merges `command`/`version` in. (simulate config is large and
    /// owned by the caller, so it's passed through untyped.)
    ///
    /// The caller's config MUST NOT contain `command` or `version` keys: the
    /// merge writes them last, so a caller-supplied value would override the
    /// adapter's injected `"simulate"` / `1` and break the wire contract.
    pub config: Value,
    pub progress_tx: Option<mpsc::Sender<Event>>,
    pub cancel: Option<oneshot::Receiver<()>>,
}

pub async fn simulate(args: SimulateArgs) -> VbaResult<SimulateResultData> {
    // Merge command/version into the caller-supplied config object →
    // {"command":"simulate","version":1, ...config}.
    let mut payload = json!({"command": "simulate", "version": 1});
    let obj = payload
        .as_object_mut()
        .expect("payload literal is an object");
    let config = args
        .config
        .as_object()
        .ok_or_else(|| VbaError::InvalidInput("simulate config must be a JSON object".into()))?;
    for (k, v) in config {
        obj.insert(k.clone(), v.clone());
    }

    let value = run_sidecar(SidecarRequest {
        subcommand: "simulate".into(),
        payload,
        progress_tx: args.progress_tx,
        cancel: args.cancel,
    })
    .await?;

    // Undecodable result = sidecar protocol violation → BadEventJson.
    serde_json::from_value(value).map_err(VbaError::BadEventJson)
}

// ---------------------------------------------------------------------------
// eval
// ---------------------------------------------------------------------------

pub struct EvalArgs {
    pub dataset_dir: String,
    pub method: String,
    pub seed_matrix: Vec<i64>,
    pub progress_tx: Option<mpsc::Sender<Event>>,
    pub cancel: Option<oneshot::Receiver<()>>,
}

pub async fn eval(args: EvalArgs) -> VbaResult<EvalResultData> {
    let payload = json!({
        "command": "eval",
        "version": 1,
        "dataset_dir": &args.dataset_dir,
        "method": &args.method,
        "seed_matrix": &args.seed_matrix,
    });

    let value = run_sidecar(SidecarRequest {
        subcommand: "eval".into(),
        payload,
        progress_tx: args.progress_tx,
        cancel: args.cancel,
    })
    .await?;

    // Undecodable result = sidecar protocol violation → BadEventJson.
    serde_json::from_value(value).map_err(VbaError::BadEventJson)
}
