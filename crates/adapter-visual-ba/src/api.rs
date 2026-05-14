//! High-level public API.
//!
//! `reconstruct(args)` runs the sidecar `reconstruct` subcommand and
//! converts the `ResultData` into a `lmt_core::measured_points::MeasuredPoints`.
//! Caller is responsible for then driving the IR through `lmt_core::auto_reconstruct`.

use lmt_core::measured_points::MeasuredPoints;
use serde_json::json;
use tokio::sync::{mpsc, oneshot};

use crate::error::{VbaError, VbaResult};
use crate::ipc::{
    CabinetArray as IpcCabinetArray, CoordinateFrame as IpcCoordinateFrame, Event, Intrinsics,
    PatternMeta, ReconstructProject, ShapePrior as IpcShapePrior,
};
use crate::sidecar::{run_sidecar, SidecarRequest};

pub struct ReconstructArgs {
    pub project: ReconstructProject,
    pub images: Vec<String>,
    pub intrinsics: Intrinsics,
    pub pattern_meta: PatternMeta,
    pub progress_tx: Option<mpsc::Sender<Event>>,
    pub cancel: Option<oneshot::Receiver<()>>,
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

/// Pre-validate the project against the core IR's stricter rules so we fail
/// fast — before spawning a multi-minute sidecar run — when the basis is
/// non-orthonormal, dimensions are oversized, or sizes are non-positive.
fn validate_project_eagerly(p: &ReconstructProject) -> VbaResult<()> {
    ipc_to_ir_coord(&p.coordinate_frame)?;
    ipc_to_ir_cabinet(&p.cabinet_array)?;
    ipc_to_ir_shape(&p.shape_prior)?;
    Ok(())
}

pub async fn reconstruct(args: ReconstructArgs) -> VbaResult<MeasuredPoints> {
    validate_project_eagerly(&args.project)?;

    let payload = json!({
        "command": "reconstruct",
        "version": 1,
        "project": &args.project,
        "images": &args.images,
        "intrinsics": &args.intrinsics,
        "pattern_meta": &args.pattern_meta,
    });

    let result = run_sidecar(SidecarRequest {
        subcommand: "reconstruct".into(),
        payload,
        progress_tx: args.progress_tx,
        cancel: args.cancel,
    })
    .await?;

    let points: Vec<lmt_core::point::MeasuredPoint> = result
        .measured_points
        .into_iter()
        .map(|dto| dto.into_ir())
        .collect();

    Ok(MeasuredPoints {
        screen_id: args.project.screen_id,
        coordinate_frame: ipc_to_ir_coord(&args.project.coordinate_frame)?,
        cabinet_array: ipc_to_ir_cabinet(&args.project.cabinet_array)?,
        shape_prior: ipc_to_ir_shape(&args.project.shape_prior)?,
        points,
    })
}
