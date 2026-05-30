//! M2 visual-BA adapter 的 service-layer helpers。
//!
//! Tauri GUI 的 `#[tauri::command]` 与 lmt-cli 的子命令都通过 thin shim 调用本
//! 文件的 `run_*` 函数。每个 `run_*` 是 SYNC(CLI 是同步的):内部建一个临时
//! tokio runtime,`block_on` adapter 的 async fn,然后把 adapter 的输出映射成
//! `lmt-shared` DTO。
//!
//! 单位约定见 adapter `MeasuredPointDto::into_ir`(IPC 用米,IR 用毫米/毫米²)。

use std::path::Path;

use lmt_adapter_visual_ba::api::{
    calibrate, calibrate_structured_light, compare_known, decode_structured_light, eval,
    generate_pattern, generate_structured_light, plan_capture, reconstruct,
    reconstruct_structured_light, simulate, CalibrateArgs, CalibrateStructuredLightArgs,
    CompareKnownArgs, DecodeStructuredLightArgs, EvalArgs, GeneratePatternArgs,
    GenerateStructuredLightArgs, PlanCaptureArgs, ReconstructArgs, ReconstructOut,
    ReconstructStructuredLightArgs, SimulateArgs,
};
use lmt_adapter_visual_ba::ipc;

use lmt_shared::dto::{
    CabinetPoseSummary, CabinetSizeCheck, CalibrateResult, CompareKnownResult,
    DecodeStructuredLightResult, EvalResult, GeneratePatternResult, GenerateStructuredLightResult,
    PairCheck, SimulateResult, VisualReconstructResult,
};
use lmt_shared::error::{LmtError, LmtResult};

use crate::projects::load_project_yaml_from_path;

/// A short-lived tokio runtime for `block_on`. The workspace tokio enables the
/// `rt` + `process` features (the adapter spawns the sidecar via tokio process).
fn rt() -> LmtResult<tokio::runtime::Runtime> {
    tokio::runtime::Runtime::new().map_err(|e| LmtError::Other(format!("tokio runtime: {e}")))
}

/// Map adapter `VbaError` → `LmtError`, preserving the sidecar's error code so
/// the CLI exit code is correct (see Task 1.6 error-code table). The `Protocol`
/// `code` string is exactly the snake_case `kind` of the matching `LmtError`
/// variant, so the envelope re-emits the same `error_codes::*` string.
fn map_vba_err(e: lmt_adapter_visual_ba::error::VbaError) -> LmtError {
    use lmt_adapter_visual_ba::error::VbaError as V;
    match e {
        V::Protocol { code, message } => match code.as_str() {
            "detection_failed" => LmtError::DetectionFailed(message),
            "ba_diverged" => LmtError::BaDiverged(message),
            "procrustes_failed" => LmtError::ProcrustesFailed(message),
            "intrinsics_invalid" => LmtError::IntrinsicsInvalid(message),
            "observability_failed" => LmtError::ObservabilityFailed(message),
            "decode_failed" => LmtError::DecodeFailed(message),
            "invalid_input" => LmtError::InvalidInput(message),
            "internal_error" | "internal" => LmtError::Other(message),
            other => LmtError::Other(format!("{other}: {message}")),
        },
        // The sync run_* helpers never pass a cancel token, so this arm is
        // permanently defensive — cancel is only reachable from async (Tauri)
        // callers.
        V::Cancelled => LmtError::Other("cancelled".into()),
        V::InvalidInput(m) => LmtError::InvalidInput(m),
        other => LmtError::Other(other.to_string()),
    }
}

/// Convert lmt-shared `ScreenConfig` → the adapter's `ipc::CabinetArray`.
/// Mirrors `export::build_cabinet_array` but targets the adapter's own ipc type
/// (which the sidecar wire contract uses) instead of `lmt_core::shape::CabinetArray`.
fn ipc_cabinet_array(screen_cfg: &lmt_shared::dto::ScreenConfig) -> ipc::CabinetArray {
    use lmt_shared::dto::ShapeMode;
    let [cols, rows] = screen_cfg.cabinet_count;
    let absent_cells = match screen_cfg.shape_mode {
        ShapeMode::Rectangle => Vec::new(),
        ShapeMode::Irregular => screen_cfg
            .irregular_mask
            .iter()
            .map(|&[c, r]| (c, r))
            .collect(),
    };
    ipc::CabinetArray {
        cols,
        rows,
        cabinet_size_mm: screen_cfg.cabinet_size_mm,
        absent_cells,
    }
}

/// Convert lmt-shared `ShapePriorConfig` → the adapter's `ipc::ShapePrior`.
fn ipc_shape_prior(screen_cfg: &lmt_shared::dto::ScreenConfig) -> ipc::ShapePrior {
    use lmt_shared::dto::ShapePriorConfig;
    match &screen_cfg.shape_prior {
        ShapePriorConfig::Flat => ipc::ShapePrior::Flat(ipc::FlatTag::Flat),
        ShapePriorConfig::Curved { radius_mm, .. } => ipc::ShapePrior::Curved {
            curved: ipc::CurvedShape {
                radius_mm: *radius_mm,
            },
        },
        ShapePriorConfig::Folded {
            fold_seams_at_columns,
        } => ipc::ShapePrior::Folded {
            folded: ipc::FoldedShape {
                fold_seam_columns: fold_seams_at_columns.clone(),
            },
        },
    }
}

/// Look up a screen in project.yaml or fail with `NotFound`.
fn load_screen<'a>(
    cfg: &'a lmt_shared::dto::ProjectConfig,
    screen_id: &str,
) -> LmtResult<&'a lmt_shared::dto::ScreenConfig> {
    cfg.screens
        .get(screen_id)
        .ok_or_else(|| LmtError::NotFound(format!("screen '{screen_id}' not in project")))
}

// ---------------------------------------------------------------------------
// reconstruct
// ---------------------------------------------------------------------------

/// Run the visual-BA reconstruction for one screen and persist the result to
/// `<project>/measurements/measured.yaml`.
///
/// The capture manifest references its own `screen_mapping` file, so we pass
/// `screen_mapping_path = None` and let the sidecar resolve it.
pub fn run_reconstruct(
    project_path: &Path,
    screen_id: &str,
    capture_manifest: &Path,
) -> LmtResult<VisualReconstructResult> {
    let cfg = load_project_yaml_from_path(project_path)?;
    let screen_cfg = load_screen(&cfg, screen_id)?;

    let project = ipc::ReconstructProject {
        screen_id: screen_id.to_string(),
        cabinet_array: ipc_cabinet_array(screen_cfg),
        shape_prior: ipc_shape_prior(screen_cfg),
    };

    // The sidecar writes the cabinet pose report here; the adapter reads it back
    // for the per-cabinet summaries.
    let measurements_dir = project_path.join("measurements");
    std::fs::create_dir_all(&measurements_dir)?;
    let pose_report_path = measurements_dir.join(format!("{screen_id}_cabinet_pose_report.json"));

    let args = ReconstructArgs {
        project,
        capture_manifest_path: capture_manifest.display().to_string(),
        screen_mapping_path: None,
        pose_report_path: pose_report_path.display().to_string(),
        progress_tx: None,
        cancel: None,
    };

    let out = rt()?.block_on(reconstruct(args)).map_err(map_vba_err)?;
    persist_reconstruct_result(project_path, screen_id, out)
}

/// Persist a reconstruction's MeasuredPoints to `measurements/measured.yaml`
/// (backup + atomic write + cross-screen guard + ROLLBACK) and build the
/// VisualReconstructResult. Shared by run_reconstruct (charuco) and
/// run_reconstruct_structured_light. The reconstruction output is the expensive
/// product of a multi-minute BA run, so a half-written measured.yaml must never
/// leave the user with neither the old file nor the new result.
fn persist_reconstruct_result(
    project_path: &Path,
    screen_id: &str,
    out: ReconstructOut,
) -> LmtResult<VisualReconstructResult> {
    let measurements_dir = project_path.join("measurements");
    std::fs::create_dir_all(&measurements_dir)?;
    let measured_yaml_path = measurements_dir.join("measured.yaml");
    let backup_path = measurements_dir.join("measured.yaml.bak");

    crate::total_station::check_import_no_screen_conflict(project_path, screen_id)?;

    // If a previous measured.yaml exists, rename it to .bak (overwriting any prior
    // .bak), so we can restore it if the new write fails. Remove any prior .bak
    // first: std::fs::rename fails on Windows when the destination exists.
    let did_backup = if measured_yaml_path.exists() {
        let _ = std::fs::remove_file(&backup_path);
        std::fs::rename(&measured_yaml_path, &backup_path)?;
        true
    } else {
        false
    };

    let write_result = (|| -> LmtResult<()> {
        let yaml = serde_yaml::to_string(&out.measured_points)?;
        let tmp = measurements_dir.join("measured.yaml.tmp");
        std::fs::write(&tmp, yaml)?;
        std::fs::rename(&tmp, &measured_yaml_path)?;
        Ok(())
    })();

    if let Err(e) = write_result {
        // Remove the half-written new file before restoring, otherwise
        // rename(.bak → target) can fail on platforms where rename refuses to
        // overwrite (Windows).
        let _ = std::fs::remove_file(&measured_yaml_path);
        if did_backup {
            let _ = std::fs::rename(&backup_path, &measured_yaml_path);
        }
        return Err(e);
    }
    // Success: leave .bak in place as a versioned snapshot of the prior result.

    Ok(VisualReconstructResult {
        screen_id: screen_id.to_string(),
        measured_yaml_path: "measurements/measured.yaml".to_string(),
        pose_report_path: out.pose_report_path,
        cabinet_count: out.measured_points.points.len(),
        ba_rms_px: out.ba_rms_px,
        ba_observations_total: out.ba_observations_total,
        ba_observations_used: out.ba_observations_used,
        ba_rejected: out.ba_rejected,
        procrustes_align_rms_m: out.procrustes_align_rms_m,
        cabinets: out
            .cabinet_summaries
            .iter()
            .map(|s| CabinetPoseSummary {
                cabinet_id: s.cabinet_id.clone(),
                position_mm: s.position_mm,
                normal: s.normal,
                reprojection_rms_px: s.reprojection_rms_px,
                observed_views: s.observed_views,
                quality: s.quality.clone(),
            })
            .collect(),
    })
}

/// Multi-view structured-light reconstruction: N correspondence files (decode
/// output) + sl_meta + intrinsics → measured.yaml + cabinet_pose_report.json,
/// via the same model-constrained BA as `run_reconstruct`.
pub fn run_reconstruct_structured_light(
    project_path: &Path,
    screen_id: &str,
    sl_meta: &Path,
    intrinsics: &Path,
    correspondences: &[String],
) -> LmtResult<VisualReconstructResult> {
    let cfg = load_project_yaml_from_path(project_path)?;
    let screen_cfg = load_screen(&cfg, screen_id)?;
    let project = ipc::ReconstructProject {
        screen_id: screen_id.to_string(),
        cabinet_array: ipc_cabinet_array(screen_cfg),
        shape_prior: ipc_shape_prior(screen_cfg),
    };

    let measurements_dir = project_path.join("measurements");
    std::fs::create_dir_all(&measurements_dir)?;
    let pose_report_path = measurements_dir.join(format!("{screen_id}_cabinet_pose_report.json"));

    let args = ReconstructStructuredLightArgs {
        project,
        correspondence_paths: correspondences.to_vec(),
        sl_meta_path: sl_meta.display().to_string(),
        intrinsics_path: intrinsics.display().to_string(),
        pose_report_path: pose_report_path.display().to_string(),
        progress_tx: None,
        cancel: None,
    };

    let out = rt()?
        .block_on(reconstruct_structured_light(args))
        .map_err(map_vba_err)?;
    persist_reconstruct_result(project_path, screen_id, out)
}

// ---------------------------------------------------------------------------
// calibrate_structured_light
// ---------------------------------------------------------------------------

/// Calibrate camera intrinsics from multi-view structured-light correspondences.
/// Writes `<project>/calibration/<screen_id>_sl_intrinsics.json` (or `out` when
/// provided). Returns `Err(InvalidInput)` if the output file already exists and
/// `force` is false.
#[allow(clippy::too_many_arguments)]
pub fn run_calibrate_structured_light(
    project_path: &Path,
    screen_id: &str,
    sl_meta: &Path,
    correspondences: &[String],
    out: Option<&Path>,
    force: bool,
    max_rms_px: f64,
) -> LmtResult<CalibrateResult> {
    let cfg = load_project_yaml_from_path(project_path)?;
    let screen_cfg = load_screen(&cfg, screen_id)?;
    let project = ipc::ReconstructProject {
        screen_id: screen_id.to_string(),
        cabinet_array: ipc_cabinet_array(screen_cfg),
        shape_prior: ipc_shape_prior(screen_cfg),
    };

    let calibration_dir = project_path.join("calibration");
    std::fs::create_dir_all(&calibration_dir)?;
    let output_path = match out {
        Some(p) => p.to_path_buf(),
        None => calibration_dir.join(format!("{screen_id}_sl_intrinsics.json")),
    };
    if output_path.exists() && !force {
        return Err(LmtError::InvalidInput(format!(
            "would overwrite existing intrinsics {}; pass --force or --out",
            output_path.display()
        )));
    }

    let args = CalibrateStructuredLightArgs {
        project,
        correspondence_paths: correspondences.to_vec(),
        sl_meta_path: sl_meta.display().to_string(),
        output_path: output_path.display().to_string(),
        max_rms_px,
        progress_tx: None,
        cancel: None,
    };

    let out = rt()?
        .block_on(calibrate_structured_light(args))
        .map_err(map_vba_err)?;

    Ok(CalibrateResult {
        intrinsics_path: out.intrinsics_path,
        reproj_error_px: out.reproj_error_px,
        frames_used: out.frames_used,
    })
}

// ---------------------------------------------------------------------------
// calibrate
// ---------------------------------------------------------------------------

/// Parse `"9x9"` → `[9, 9]`. Both factors must be positive integers.
fn parse_inner_corners(s: &str) -> LmtResult<[u32; 2]> {
    let (a, b) = s
        .split_once(['x', 'X'])
        .ok_or_else(|| LmtError::InvalidInput(format!("inner corners must be WxH, got '{s}'")))?;
    let parse = |t: &str, which: &str| -> LmtResult<u32> {
        t.trim()
            .parse::<u32>()
            .map_err(|_| LmtError::InvalidInput(format!("inner corners {which} '{t}' invalid")))
            .and_then(|v| {
                if v == 0 {
                    Err(LmtError::InvalidInput(format!(
                        "inner corners {which} must be > 0"
                    )))
                } else {
                    Ok(v)
                }
            })
    };
    Ok([parse(a, "width")?, parse(b, "height")?])
}

/// Calibrate camera intrinsics from a directory of checkerboard images.
/// Writes `<project>/calibration/<screen_id>_intrinsics.json`.
pub fn run_calibrate(
    project_path: &Path,
    screen_id: &str,
    checkerboard_dir: &Path,
    square_mm: f64,
    inner: &str,
) -> LmtResult<CalibrateResult> {
    let inner_corners = parse_inner_corners(inner)?;

    if !checkerboard_dir.is_dir() {
        return Err(LmtError::NotFound(format!(
            "checkerboard dir not found: {}",
            checkerboard_dir.display()
        )));
    }
    // Collect png/jpg/jpeg images, sorted for deterministic ordering.
    let mut images: Vec<String> = std::fs::read_dir(checkerboard_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|x| x.to_str())
                .map(|x| {
                    let x = x.to_ascii_lowercase();
                    x == "png" || x == "jpg" || x == "jpeg"
                })
                .unwrap_or(false)
        })
        .map(|p| p.display().to_string())
        .collect();
    images.sort();
    if images.is_empty() {
        return Err(LmtError::InvalidInput(format!(
            "no checkerboard images (png/jpg) found in {}",
            checkerboard_dir.display()
        )));
    }

    let calibration_dir = project_path.join("calibration");
    std::fs::create_dir_all(&calibration_dir)?;
    let output_path = calibration_dir.join(format!("{screen_id}_intrinsics.json"));

    let args = CalibrateArgs {
        checkerboard_images: images,
        inner_corners,
        square_size_mm: square_mm,
        output_path: output_path.display().to_string(),
        progress_tx: None,
        cancel: None,
    };

    let out = rt()?.block_on(calibrate(args)).map_err(map_vba_err)?;

    Ok(CalibrateResult {
        intrinsics_path: out.intrinsics_path,
        reproj_error_px: out.reproj_error_px,
        frames_used: out.frames_used,
    })
}

// ---------------------------------------------------------------------------
// generate_pattern
// ---------------------------------------------------------------------------

/// Resolve the framebuffer `[w, h]` for one screen.
///
/// In `--screen-mapping` mode the framebuffer is the bounding box of the
/// per-cabinet `input_rect_px` (cabinets may be unequal / gapped); `pixels_per_
/// cabinet` is not used. In uniform mode it is `pixels_per_cabinet × cabinet_count`.
///
/// Shared by `run_generate_pattern` and `run_generate_structured_light` (both
/// must agree on the screen resolution they pass to the sidecar).
fn compute_screen_resolution(
    sm_abs: &Option<std::path::PathBuf>,
    screen_cfg: &lmt_shared::dto::ScreenConfig,
    screen_id: &str,
) -> LmtResult<[u32; 2]> {
    match sm_abs {
        Some(p) => {
            let txt = std::fs::read_to_string(p).map_err(|e| {
                LmtError::InvalidInput(format!("screen_mapping '{}' unreadable: {e}", p.display()))
            })?;
            let v: serde_json::Value = serde_json::from_str(&txt).map_err(|e| {
                LmtError::InvalidInput(format!("screen_mapping '{}' invalid JSON: {e}", p.display()))
            })?;
            let cabs = v.get("cabinets").and_then(|c| c.as_array()).ok_or_else(|| {
                LmtError::InvalidInput("screen_mapping has no cabinets[]".into())
            })?;
            // Parse each coordinate via as_f64 (accepts both JSON int and float,
            // matching the Python side's int coercion) and reject negative /
            // non-finite values rather than silently treating them as 0. Sum in
            // u64 so a large rect can't overflow; cap the framebuffer at u32.
            let (mut max_w, mut max_h) = (0u64, 0u64);
            for c in cabs {
                let r = c.get("input_rect_px").and_then(|r| r.as_array()).ok_or_else(|| {
                    LmtError::InvalidInput("screen_mapping cabinet missing input_rect_px".into())
                })?;
                if r.len() != 4 {
                    return Err(LmtError::InvalidInput(
                        "input_rect_px must be [x, y, w, h]".into(),
                    ));
                }
                let g = |i: usize| -> Result<u64, LmtError> {
                    let f = r[i].as_f64().ok_or_else(|| {
                        LmtError::InvalidInput("input_rect_px values must be numbers".into())
                    })?;
                    if !f.is_finite() || f < 0.0 {
                        return Err(LmtError::InvalidInput(format!(
                            "input_rect_px values must be finite and non-negative, got {f}"
                        )));
                    }
                    Ok(f.round() as u64)
                };
                max_w = max_w.max(g(0)? + g(2)?);
                max_h = max_h.max(g(1)? + g(3)?);
            }
            if max_w > u32::MAX as u64 || max_h > u32::MAX as u64 {
                return Err(LmtError::InvalidInput(format!(
                    "screen_mapping framebuffer {max_w}x{max_h} exceeds u32 range"
                )));
            }
            Ok([max_w as u32, max_h as u32])
        }
        None => {
            let ppc = screen_cfg.pixels_per_cabinet.ok_or_else(|| {
                LmtError::InvalidInput(format!(
                    "screen '{screen_id}' has no pixels_per_cabinet; required for uniform pattern generation"
                ))
            })?;
            let [cols, rows] = screen_cfg.cabinet_count;
            Ok([ppc[0] * cols, ppc[1] * rows])
        }
    }
}

/// Generate ChArUco calibration patterns for one screen's cabinets, written to
/// `<project>/patterns/<screen_id>`.
pub fn run_generate_pattern(
    project_path: &Path,
    screen_id: &str,
    method: &str,
    screen_mapping_path: Option<&Path>,
) -> LmtResult<GeneratePatternResult> {
    if method != "charuco" {
        return Err(LmtError::InvalidInput(format!(
            "unsupported pattern method '{method}' (only 'charuco')"
        )));
    }

    let cfg = load_project_yaml_from_path(project_path)?;
    let screen_cfg = load_screen(&cfg, screen_id)?;
    let cabinet_array = ipc_cabinet_array(screen_cfg);

    // Resolve the screen_mapping path (project-root-relative if not absolute).
    let sm_abs = screen_mapping_path.map(|p| {
        if p.is_absolute() { p.to_path_buf() } else { project_path.join(p) }
    });

    let screen_resolution = compute_screen_resolution(&sm_abs, screen_cfg, screen_id)?;

    let output_dir = project_path.join("patterns").join(screen_id);
    std::fs::create_dir_all(&output_dir)?;

    let args = GeneratePatternArgs {
        screen_id: screen_id.to_string(),
        cabinet_array,
        output_dir: output_dir.display().to_string(),
        screen_resolution,
        screen_mapping_path: sm_abs.map(|p| p.display().to_string()),
        progress_tx: None,
        cancel: None,
    };

    let out = rt()?.block_on(generate_pattern(args)).map_err(map_vba_err)?;

    Ok(GeneratePatternResult {
        output_dir: out.output_dir,
        cabinet_count: out.cabinet_count as usize,
        total_markers: out.total_markers,
    })
}

// ---------------------------------------------------------------------------
// generate_structured_light
// ---------------------------------------------------------------------------

/// Generate a structured-light dot sequence for one screen into
/// `<project>/patterns/<screen_id>/sl`. Mapping-aware: with `screen_mapping_path`
/// the framebuffer is the input_rect_px bounding box (mirrors `run_generate_pattern`).
pub fn run_generate_structured_light(
    project_path: &Path,
    screen_id: &str,
    // None = auto-derive per cabinet from its pixel resolution (sidecar).
    dot_spacing_px: Option<u32>,
    dot_radius_px: u32,
    // None = auto-derive per cabinet from its pixel resolution (sidecar).
    margin_px: Option<u32>,
    // None = auto: emit the TIFF `.seq` iff the project's output.target == "disguise".
    emit_tiff_seq: Option<bool>,
    screen_mapping_path: Option<&Path>,
) -> LmtResult<GenerateStructuredLightResult> {
    let cfg = load_project_yaml_from_path(project_path)?;
    let emit_tiff_seq = emit_tiff_seq.unwrap_or_else(|| cfg.output.target == "disguise");
    let screen_cfg = load_screen(&cfg, screen_id)?;
    let cabinet_array = ipc_cabinet_array(screen_cfg);

    // Resolve the screen_mapping path (project-root-relative if not absolute).
    let sm_abs = screen_mapping_path.map(|p| {
        if p.is_absolute() { p.to_path_buf() } else { project_path.join(p) }
    });
    let screen_resolution = compute_screen_resolution(&sm_abs, screen_cfg, screen_id)?;

    let output_dir = project_path.join("patterns").join(screen_id).join("sl");
    std::fs::create_dir_all(output_dir.parent().unwrap())?;

    let args = GenerateStructuredLightArgs {
        project_screen_id: screen_id.to_string(),
        cabinet_array,
        output_dir: output_dir.display().to_string(),
        screen_resolution,
        screen_mapping_path: sm_abs.map(|p| p.display().to_string()),
        dot_spacing_px,
        dot_radius_px,
        margin_px,
        emit_tiff_seq,
        progress_tx: None,
        cancel: None,
    };

    let out = rt()?.block_on(generate_structured_light(args)).map_err(map_vba_err)?;

    Ok(GenerateStructuredLightResult {
        output_dir: out.output_dir,
        n_dots: out.n_dots as usize,
        n_frames: out.n_frames as usize,
    })
}

// ---------------------------------------------------------------------------
// decode_structured_light
// ---------------------------------------------------------------------------

/// Decode a recorded structured-light capture (video or frame directory) into a
/// provenance-stamped screen↔camera correspondence file at `output_path`.
pub fn run_decode_structured_light(
    input_path: &Path,
    sl_meta_path: &Path,
    output_path: &Path,
    // None = sidecar default (0.85). Lower for non-black / partially-filled frames.
    sentinel_threshold: Option<f64>,
    // None = sidecar auto-derives the screen ROI from the temporal-activity map.
    screen_roi: Option<[u32; 4]>,
    // When true the sidecar also writes <output_path>.debug.png.
    emit_debug_image: bool,
) -> LmtResult<DecodeStructuredLightResult> {
    let args = DecodeStructuredLightArgs {
        input_path: input_path.display().to_string(),
        sl_meta_path: sl_meta_path.display().to_string(),
        output_path: output_path.display().to_string(),
        sentinel_threshold,
        screen_roi,
        emit_debug_image,
        progress_tx: None,
        cancel: None,
    };

    let out = rt()?.block_on(decode_structured_light(args)).map_err(map_vba_err)?;

    Ok(DecodeStructuredLightResult {
        output_path: out.output_path,
        n_dots_decoded: out.n_dots_decoded as usize,
    })
}

// ---------------------------------------------------------------------------
// simulate
// ---------------------------------------------------------------------------

/// Run a synthetic-dataset simulation. `config_path` is the
/// `{scene, cameras, intrinsics, noise, seed}` JSON object; `out_dir` is
/// injected as `out_dir` (overriding any value in the config).
pub fn run_simulate(config_path: &Path, out_dir: &Path) -> LmtResult<SimulateResult> {
    let raw = std::fs::read_to_string(config_path)?;
    let mut config: serde_json::Value = serde_json::from_str(&raw)?;
    let obj = config.as_object_mut().ok_or_else(|| {
        LmtError::InvalidInput("simulate config must be a JSON object".into())
    })?;
    obj.insert(
        "out_dir".to_string(),
        serde_json::Value::String(out_dir.display().to_string()),
    );

    let args = SimulateArgs {
        config,
        progress_tx: None,
        cancel: None,
    };

    let out = rt()?.block_on(simulate(args)).map_err(map_vba_err)?;

    Ok(SimulateResult {
        dataset_dir: out.dataset_dir,
        n_views: out.n_views,
        n_observations: out.n_observations,
        seed: out.seed,
    })
}

// ---------------------------------------------------------------------------
// eval
// ---------------------------------------------------------------------------

/// Evaluate a method against a simulated dataset across a seed matrix, returning
/// the worst-case error metrics.
pub fn run_eval(
    dataset_dir: &Path,
    method: &str,
    seed_matrix: Vec<i64>,
) -> LmtResult<EvalResult> {
    let args = EvalArgs {
        dataset_dir: dataset_dir.display().to_string(),
        method: method.to_string(),
        seed_matrix,
        progress_tx: None,
        cancel: None,
    };

    let out = rt()?.block_on(eval(args)).map_err(map_vba_err)?;

    Ok(EvalResult {
        method: out.method,
        seeds: out.seeds,
        max_size_error_mm: out.max_size_error_mm,
        max_distance_error_mm: out.max_distance_error_mm,
        max_angle_error_deg: out.max_angle_error_deg,
    })
}

// ---------------------------------------------------------------------------
// compare_known
// ---------------------------------------------------------------------------

/// Reconcile a reconstructed `cabinet_pose_report.json` against a user-filled
/// `known_geometry.json` (true monitor sizes + pairwise distances/angles).
/// Reads both files in the sidecar; writes nothing (write_safe).
pub fn run_compare_known(
    report_path: &Path,
    known_path: &Path,
) -> LmtResult<CompareKnownResult> {
    let args = CompareKnownArgs {
        report_path: report_path.display().to_string(),
        known_path: known_path.display().to_string(),
        progress_tx: None,
        cancel: None,
    };

    let out = rt()?.block_on(compare_known(args)).map_err(map_vba_err)?;

    Ok(CompareKnownResult {
        cabinets: out
            .cabinets
            .into_iter()
            .map(|c| CabinetSizeCheck {
                cabinet_id: c.cabinet_id,
                size_error_mm: c.size_error_mm,
                pass: c.pass,
            })
            .collect(),
        pairs: out
            .pairs
            .into_iter()
            .map(|p| PairCheck {
                a: p.a,
                b: p.b,
                distance_error_mm: p.distance_error_mm,
                angle_error_deg: p.angle_error_deg,
                distance_pass: p.distance_pass,
                angle_pass: p.angle_pass,
            })
            .collect(),
        passed: out.passed,
        thresholds: out.thresholds,
    })
}

// ── plan-capture ──────────────────────────────────────────────────────────────

/// Parse `"3840x2160"` → `[3840, 2160]`.
fn parse_wxh(s: &str) -> LmtResult<[u32; 2]> {
    let (a, b) = s
        .split_once(['x', 'X'])
        .ok_or_else(|| LmtError::InvalidInput(format!("image-size must be WxH, got '{s}'")))?;
    let p = |t: &str| {
        t.trim()
            .parse::<u32>()
            .map_err(|_| LmtError::InvalidInput(format!("image-size component '{t}' invalid")))
            .and_then(|v| {
                if v == 0 {
                    Err(LmtError::InvalidInput(
                        "image-size components must be > 0".into(),
                    ))
                } else {
                    Ok(v)
                }
            })
    };
    Ok([p(a)?, p(b)?])
}

/// Parse `"2000..12000"` → `(2000.0, 12000.0)`; min must be < max.
fn parse_range(s: &str, name: &str) -> LmtResult<(f64, f64)> {
    let (a, b) = s
        .split_once("..")
        .ok_or_else(|| LmtError::InvalidInput(format!("{name} must be MIN..MAX, got '{s}'")))?;
    let lo = a
        .trim()
        .parse::<f64>()
        .map_err(|_| LmtError::InvalidInput(format!("{name} min '{a}' invalid")))?;
    let hi = b
        .trim()
        .parse::<f64>()
        .map_err(|_| LmtError::InvalidInput(format!("{name} max '{b}' invalid")))?;
    if !(lo < hi) {
        return Err(LmtError::InvalidInput(format!(
            "{name} needs MIN < MAX, got {lo}..{hi}"
        )));
    }
    Ok((lo, hi))
}

#[allow(clippy::too_many_arguments)]
pub fn run_plan_capture(
    project_path: &Path,
    screen_id: &str,
    image_size: &str,
    hfov_deg: Option<f64>,
    vfov_deg: Option<f64>,
    standoff: &str,
    height: &str,
    target_p95_residual_mm: f64,
    trials: u32,
    seed: u32,
) -> LmtResult<lmt_shared::dto::CapturePlan> {
    use lmt_shared::dto::{CabinetCoverage, CapturePlan, CaptureStation, UnreachableRegion};

    if hfov_deg.is_some() == vfov_deg.is_some() {
        return Err(LmtError::InvalidInput(
            "pass exactly one of --hfov-deg / --vfov-deg".into(),
        ));
    }
    let image_size = parse_wxh(image_size)?;
    let (standoff_min_mm, standoff_max_mm) = parse_range(standoff, "standoff")?;
    let (height_min_mm, height_max_mm) = parse_range(height, "height")?;

    let cfg = load_project_yaml_from_path(project_path)?;
    let screen_cfg = load_screen(&cfg, screen_id)?;
    let project = ipc::ReconstructProject {
        screen_id: screen_id.to_string(),
        cabinet_array: ipc_cabinet_array(screen_cfg),
        shape_prior: ipc_shape_prior(screen_cfg),
    };

    let args = PlanCaptureArgs {
        project,
        image_size,
        hfov_deg,
        vfov_deg,
        standoff_min_mm,
        standoff_max_mm,
        height_min_mm,
        height_max_mm,
        target_p95_residual_mm,
        trials,
        seed,
        progress_tx: None,
        cancel: None,
    };
    let out = rt()?.block_on(plan_capture(args)).map_err(map_vba_err)?;

    Ok(CapturePlan {
        stations: out
            .stations
            .into_iter()
            .map(|s| CaptureStation {
                id: s.id,
                position_mm: s.position_mm,
                look_at_mm: s.look_at_mm,
                standoff_mm: s.standoff_mm,
                height_mm: s.height_mm,
                role: s.role,
                covers_cabinets: s.covers_cabinets,
            })
            .collect(),
        coverage: out
            .coverage
            .into_iter()
            .map(|c| CabinetCoverage {
                col: c.col,
                row: c.row,
                p95_residual_mm: c.p95_residual_mm,
                n_views: c.n_views,
                total_observations: c.total_observations,
                reconstructable: c.reconstructable,
                low_observation: c.low_observation,
                bridged: c.bridged,
                pass: c.pass,
            })
            .collect(),
        unreachable_regions: out
            .unreachable_regions
            .into_iter()
            .map(|u| UnreachableRegion {
                cabinets: u.cabinets,
                reason: u.reason,
            })
            .collect(),
        all_pass: out.all_pass,
        target_p95_residual_mm: out.target_p95_residual_mm,
    })
}

// ── capture guidance HTML card ────────────────────────────────────────────────

pub struct CardGeometry {
    pub total_width_mm: f64,
    pub total_height_mm: f64,
    pub radius_mm: Option<f64>,
    pub cols: u32,
    pub rows: u32,
}

fn card_esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Render a self-contained HTML capture-guidance card: a top-down plan SVG
/// (screen footprint + station dots + aim arrows), a front-elevation coverage
/// heatmap SVG (cabinet grid colored by reconstructability/residual), a station
/// table, and unreachable-region warnings. No external dependencies.
pub fn render_capture_card(
    plan: &lmt_shared::dto::CapturePlan,
    geom: &CardGeometry,
    project_name: &str,
    screen_id: &str,
) -> String {
    use std::fmt::Write as _;
    let cov = |c: u32, r: u32| plan.coverage.iter().find(|x| x.col == c && x.row == r);

    // ---- top-down plan SVG (X horizontal, Z depth: 0 = wall, + toward cameras) ----
    // Fan / side stations sit at x < 0 or x > total_width_mm, so the viewBox
    // x-range must span every station position, not just the screen [0, width],
    // or those dots/arrows get clipped off the SVG.
    let mut x_lo = 0.0_f64;
    let mut x_hi = geom.total_width_mm;
    for s in &plan.stations {
        x_lo = x_lo.min(s.position_mm[0]);
        x_hi = x_hi.max(s.position_mm[0]);
    }
    let x_extent = (x_hi - x_lo).max(1.0);
    let max_z = plan
        .stations
        .iter()
        .map(|s| s.position_mm[2])
        .fold(0.0_f64, f64::max)
        .max(geom.total_width_mm * 0.3)
        * 1.12;
    let span = x_extent.max(max_z).max(1.0);
    let pad = 36.0_f64;
    let sc = 720.0_f64 / span;
    let sx = |x: f64| pad + (x - x_lo) * sc;
    let sz = |z: f64| pad + z * sc;
    let svg_w = pad * 2.0 + x_extent * sc;
    let svg_h = pad * 2.0 + max_z * sc;

    let mut plan_svg = String::new();
    write!(plan_svg, "<svg viewBox=\"0 0 {:.0} {:.0}\" width=\"100%\" style=\"max-width:760px;height:auto;background:#0f1722;border-radius:8px\">", svg_w, svg_h).ok();
    if let Some(radius) = geom.radius_mm {
        let w = geom.total_width_mm;
        let mut pts = String::new();
        for i in 0..=40 {
            let x = w * (i as f64) / 40.0;
            let a = (x - w / 2.0) / radius;
            let z = radius * (1.0 - a.cos());
            write!(pts, "{:.1},{:.1} ", sx(x), sz(z)).ok();
        }
        write!(plan_svg, "<polyline points=\"{}\" fill=\"none\" stroke=\"#38bdf8\" stroke-width=\"3\"/>", pts).ok();
    } else {
        write!(plan_svg, "<line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"#38bdf8\" stroke-width=\"3\"/>", sx(0.0), sz(0.0), sx(geom.total_width_mm), sz(0.0)).ok();
    }
    write!(plan_svg, "<text x=\"{:.1}\" y=\"{:.1}\" fill=\"#7dd3fc\" font-size=\"13\">屏幕 {:.1} m</text>", sx(0.0), sz(0.0) - 8.0, geom.total_width_mm / 1000.0).ok();
    for s in &plan.stations {
        let (px, pz) = (sx(s.position_mm[0]), sz(s.position_mm[2]));
        let (ax, az) = (sx(s.look_at_mm[0]), sz(s.look_at_mm[2]));
        let (dx, dz) = (ax - px, az - pz);
        let len = (dx * dx + dz * dz).sqrt().max(1.0);
        write!(plan_svg, "<line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"#fbbf24\" stroke-width=\"2\"/>", px, pz, px + dx / len * 26.0, pz + dz / len * 26.0).ok();
        let color = match s.role.as_str() {
            "top" => "#a78bfa",
            "bottom" => "#34d399",
            "added" => "#f472b6",
            _ => "#fbbf24",
        };
        write!(plan_svg, "<circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"5\" fill=\"{}\"/>", px, pz, color).ok();
        write!(plan_svg, "<text x=\"{:.1}\" y=\"{:.1}\" fill=\"#e2e8f0\" font-size=\"12\">{}</text>", px + 7.0, pz + 4.0, card_esc(&s.id)).ok();
    }
    plan_svg.push_str("</svg>");

    // ---- front elevation heatmap SVG (row 0 at bottom) ----
    let (cell, gap) = (56.0_f64, 4.0_f64);
    let ew = geom.cols as f64 * (cell + gap) + gap;
    let eh = geom.rows as f64 * (cell + gap) + gap;
    let mut elev_svg = String::new();
    write!(elev_svg, "<svg viewBox=\"0 0 {:.0} {:.0}\" width=\"100%\" style=\"max-width:{:.0}px;height:auto\">", ew, eh, ew.min(760.0)).ok();
    for r in 0..geom.rows {
        for c in 0..geom.cols {
            let x = gap + c as f64 * (cell + gap);
            let y = gap + (geom.rows - 1 - r) as f64 * (cell + gap);
            let (fill, label) = match cov(c, r) {
                Some(cv) => {
                    let color = if !cv.reconstructable || !cv.bridged {
                        "#c62828"
                    } else if !cv.pass {
                        "#ef6c00"
                    } else if cv.low_observation {
                        "#f9a825"
                    } else {
                        "#2e7d32"
                    };
                    let lab = match cv.p95_residual_mm {
                        Some(p) => format!("{:.1}", p),
                        None => "✗".to_string(),
                    };
                    (color, lab)
                }
                None => ("#37474f", "—".to_string()),
            };
            write!(elev_svg, "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.0}\" height=\"{:.0}\" rx=\"4\" fill=\"{}\"/>", x, y, cell, cell, fill).ok();
            write!(elev_svg, "<text x=\"{:.1}\" y=\"{:.1}\" fill=\"#fff\" font-size=\"13\" text-anchor=\"middle\">{}</text>", x + cell / 2.0, y + cell / 2.0 + 5.0, label).ok();
        }
    }
    elev_svg.push_str("</svg>");

    let banner = if plan.all_pass {
        "<div class=\"banner ok\">全部箱体达标 ✓</div>".to_string()
    } else {
        let n: usize = plan.unreachable_regions.iter().map(|u| u.cabinets.len()).sum();
        format!("<div class=\"banner bad\">{n} 个箱体未达标 ✗</div>")
    };

    let mut rows_html = String::new();
    for s in &plan.stations {
        write!(
            rows_html,
            "<tr><td>{}</td><td>{}</td><td>{:.2}</td><td>{:.2}</td><td>{:.0}</td><td>{}</td></tr>",
            card_esc(&s.id), card_esc(&s.role), s.standoff_mm / 1000.0,
            s.height_mm / 1000.0, s.look_at_mm[0], s.covers_cabinets.len()
        ).ok();
    }

    let mut warn_html = String::new();
    if !plan.unreachable_regions.is_empty() {
        warn_html.push_str("<div class=\"warn\"><strong>不可重建 / 不可达区域</strong><ul>");
        for u in &plan.unreachable_regions {
            let cells: Vec<String> =
                u.cabinets.iter().map(|c| format!("({},{})", c[0], c[1])).collect();
            write!(warn_html, "<li>{} — {}</li>", card_esc(&u.reason), cells.join(", ")).ok();
        }
        warn_html.push_str("</ul></div>");
    }

    let shape = match geom.radius_mm {
        Some(r) => format!(" &nbsp; 弧半径 {:.0} mm", r),
        None => " &nbsp; 平面".to_string(),
    };

    format!(
"<!DOCTYPE html>
<html lang=\"zh\"><head><meta charset=\"utf-8\">
<title>采集指导 - {proj} / {scr}</title>
<style>
body {{ font-family: 'PingFang SC', 'Microsoft YaHei', sans-serif; line-height: 1.6; max-width: 880px; margin: 2em auto; padding: 0 1em; color:#1f2937; }}
h1 {{ border-bottom: 2px solid #333; padding-bottom: .3em; }}
h2 {{ margin-top: 1.4em; }}
table {{ border-collapse: collapse; margin: 1em 0; width: 100%; }}
th, td {{ border: 1px solid #cbd5e1; padding: 6px 10px; text-align: left; }}
th {{ background: #f1f5f9; }}
.banner {{ padding: .6em 1em; border-radius: 6px; font-weight: 600; margin: .8em 0; }}
.banner.ok {{ background:#dcfce7; color:#166534; }}
.banner.bad {{ background:#fee2e2; color:#991b1b; }}
.legend span {{ display:inline-block; padding:2px 8px; border-radius:4px; color:#fff; margin-right:6px; font-size:13px; }}
.warn {{ background:#fff7ed; border:1px solid #fed7aa; border-radius:6px; padding:.6em 1em; margin:1em 0; }}
.note {{ color:#64748b; font-size:13px; }}
</style></head><body>
<h1>LED 屏采集指导卡</h1>
<p>项目：<strong>{proj}</strong> &nbsp; 屏体：<strong>{scr}</strong> &nbsp; 箱体阵列：{cols} × {rows}{shape}</p>
{banner}
<h2>俯视机位图（屏幕在上，相机在下方）</h2>
{plan_svg}
<h2>正视覆盖热力图（按箱体重建残差着色）</h2>
<p class=\"legend\"><span style=\"background:#2e7d32\">达标</span><span style=\"background:#f9a825\">低观测</span><span style=\"background:#ef6c00\">超目标</span><span style=\"background:#c62828\">不可重建/断链</span></p>
{elev_svg}
<h2>推荐机位清单</h2>
<table><tr><th>机位</th><th>类型</th><th>后退(m)</th><th>架高(m)</th><th>对准 X(mm)</th><th>覆盖箱体数</th></tr>{rows_html}</table>
{warn_html}
<p class=\"note\">残差为指导性预测（视野感知三角化可行性，非完整 BA）；目标 p95 ≤ {target:.1} mm。坐标为相对屏幕的模型系。</p>
</body></html>",
        proj = card_esc(project_name), scr = card_esc(screen_id),
        cols = geom.cols, rows = geom.rows, shape = shape, banner = banner,
        plan_svg = plan_svg, elev_svg = elev_svg, rows_html = rows_html,
        warn_html = warn_html, target = plan.target_p95_residual_mm,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn run_capture_card(
    project_path: &Path,
    screen_id: &str,
    image_size: &str,
    hfov_deg: Option<f64>,
    vfov_deg: Option<f64>,
    standoff: &str,
    height: &str,
    target_p95_residual_mm: f64,
    trials: u32,
    seed: u32,
) -> LmtResult<lmt_shared::dto::CaptureCardResult> {
    let plan = run_plan_capture(
        project_path, screen_id, image_size, hfov_deg, vfov_deg, standoff, height,
        target_p95_residual_mm, trials, seed,
    )?;
    let cfg = load_project_yaml_from_path(project_path)?;
    let screen_cfg = load_screen(&cfg, screen_id)?;
    let [cols, rows] = screen_cfg.cabinet_count;
    let [cw, ch] = screen_cfg.cabinet_size_mm;
    let radius_mm = match &screen_cfg.shape_prior {
        lmt_shared::dto::ShapePriorConfig::Curved { radius_mm, .. } => Some(*radius_mm),
        _ => None,
    };
    let geom = CardGeometry {
        total_width_mm: cols as f64 * cw,
        total_height_mm: rows as f64 * ch,
        radius_mm,
        cols,
        rows,
    };
    let html = render_capture_card(&plan, &geom, &cfg.project.name, screen_id);
    Ok(lmt_shared::dto::CaptureCardResult { html_content: html })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn render_capture_card_contains_plan_svg_and_table() {
        use lmt_shared::dto::{CabinetCoverage, CapturePlan, CaptureStation, UnreachableRegion};
        let plan = CapturePlan {
            stations: vec![
                CaptureStation {
                    id: "S01".into(),
                    position_mm: [250.0, 250.0, 3000.0],
                    look_at_mm: [250.0, 250.0, 0.0],
                    standoff_mm: 3000.0,
                    height_mm: 250.0,
                    role: "fan".into(),
                    covers_cabinets: vec![[0, 0]],
                },
                // a fan station LEFT of the 1000mm-wide wall (x < 0) — must not
                // be clipped off the SVG viewBox.
                CaptureStation {
                    id: "S02".into(),
                    position_mm: [-600.0, 250.0, 2000.0],
                    look_at_mm: [500.0, 250.0, 0.0],
                    standoff_mm: 2300.0,
                    height_mm: 250.0,
                    role: "fan".into(),
                    covers_cabinets: vec![[0, 0]],
                },
            ],
            coverage: vec![
                CabinetCoverage {
                    col: 0, row: 0, p95_residual_mm: Some(1.2), n_views: 4,
                    total_observations: 64, reconstructable: true, low_observation: false,
                    bridged: true, pass: true,
                },
                CabinetCoverage {
                    col: 1, row: 0, p95_residual_mm: None, n_views: 1,
                    total_observations: 16, reconstructable: false, low_observation: false,
                    bridged: false, pass: false,
                },
            ],
            unreachable_regions: vec![UnreachableRegion {
                cabinets: vec![[1, 0]],
                reason: "x".into(),
            }],
            all_pass: false,
            target_p95_residual_mm: 3.0,
        };
        let geom = CardGeometry {
            total_width_mm: 1000.0,
            total_height_mm: 500.0,
            radius_mm: None,
            cols: 2,
            rows: 1,
        };
        let html = render_capture_card(&plan, &geom, "Demo", "MAIN");
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<svg"));
        assert!(html.contains("S01"));
        assert!(html.contains("PingFang SC"));
        assert!(html.contains("1.2"));
        assert!(html.contains("不可重建") || html.contains("✗"));
        assert!(html.matches("<svg").count() >= 2);
        assert!(!html.contains("http://") && !html.contains("https://") && !html.contains("cdn"));
        // the x<0 station must not be clipped: no negative SVG coordinates.
        assert!(!html.contains("cx=\"-"), "station clipped off the plan viewBox");
        assert!(!html.contains("x1=\"-") && !html.contains("x2=\"-"));
    }

    // ── sidecar wrapper plumbing (mirrors adapter's simulate_eval_test) ────────
    //
    // The real-sidecar round-trips below rely on a POSIX `.sh` wrapper and a
    // venv interpreter at `.venv/bin/python`, so they are `#[cfg(unix)]`-only.
    // On Windows the venv lives under `.venv/Scripts/` and there is no `.sh`
    // runner; these tests are excluded from compilation there (Windows CI
    // covers pytest + the cross-platform tests below + the packaging smoke).

    #[cfg(unix)]
    use std::path::PathBuf;
    #[cfg(unix)]
    use std::sync::Mutex;

    /// Serialize env-var mutation across tests in this binary, since they share
    /// the process and all touch LMT_VBA_SIDECAR_PATH.
    #[cfg(unix)]
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Path to the project's python-sidecar venv interpreter, computed from this
    /// crate's manifest dir (`crates/lmt-app` → `../../python-sidecar/.venv/bin`).
    /// We canonicalize only the parent `.venv/bin` dir and KEEP the `python`
    /// basename: launching via that path activates the venv's sys.path, while
    /// canonicalizing the file would resolve the symlink to the bare interpreter.
    #[cfg(unix)]
    fn sidecar_python() -> Option<PathBuf> {
        let bin =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../python-sidecar/.venv/bin");
        let bin = bin.canonicalize().ok()?;
        let py = bin.join("python");
        if py.is_file() {
            Some(py)
        } else {
            None
        }
    }

    /// Write a `sh` wrapper that execs `python -m lmt_vba_sidecar "$@"`, chmod
    /// 0o755; locate_sidecar requires an existing FILE, so we point the env var
    /// at the script (not the bare interpreter).
    #[cfg(unix)]
    fn write_wrapper(dir: &Path, python: &Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let wrapper = dir.join("lmt-vba-sidecar");
        let script = format!(
            "#!/bin/sh\nexec \"{}\" -m lmt_vba_sidecar \"$@\"\n",
            python.display()
        );
        std::fs::write(&wrapper, script).expect("write wrapper");
        let mut perms = std::fs::metadata(&wrapper).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&wrapper, perms).expect("chmod wrapper");
        wrapper
    }

    // ── real-sidecar test: simulate → eval ─────────────────────────────────────

    #[cfg(unix)]
    #[test]
    fn simulate_then_eval_roundtrip() {
        let _guard = ENV_LOCK.lock().unwrap();

        let python = match sidecar_python() {
            Some(p) => p,
            None => {
                eprintln!("skipping simulate_then_eval_roundtrip: python-sidecar venv not found");
                return;
            }
        };

        let tmp = tempdir().expect("tmpdir");
        let wrapper = write_wrapper(tmp.path(), &python);
        std::env::set_var("LMT_VBA_SIDECAR_PATH", wrapper.to_str().unwrap());

        // Write a simulate config; out_dir is injected by run_simulate so we
        // leave it out here (the helper overwrites it anyway).
        let config = serde_json::json!({
            "scene": {
                "cabinet_array": {"cols": 2, "rows": 1, "cabinet_size_mm": [600, 340]},
                "shape_prior": "flat",
                "inter_board_angle_deg": 10.0
            },
            "cameras": {
                "n_views": 20,
                "distance_mm_range": [1500, 3000],
                "yaw_deg_range": [-40, 40],
                "pitch_deg_range": [-20, 20]
            },
            "intrinsics": {
                "K": [[2000, 0, 960], [0, 2000, 540], [0, 0, 1]],
                "dist_coeffs": [0, 0, 0, 0, 0],
                "image_size": [1920, 1080]
            },
            "noise": {"pixel_sigma": 0.3, "visibility_frac": 0.8},
            "seed": 2
        });
        let config_path = tmp.path().join("sim_config.json");
        std::fs::write(&config_path, serde_json::to_string(&config).unwrap()).unwrap();
        let dataset_dir = tmp.path().join("dataset");

        let sim = run_simulate(&config_path, &dataset_dir);
        let sim = match sim {
            Ok(s) => s,
            Err(e) => {
                std::env::remove_var("LMT_VBA_SIDECAR_PATH");
                panic!("run_simulate failed: {e}");
            }
        };
        assert_eq!(sim.n_views, 20, "n_views");
        assert_eq!(
            sim.dataset_dir,
            dataset_dir.display().to_string(),
            "dataset_dir echoes injected out_dir"
        );
        // scene.npz must exist on disk.
        assert!(
            dataset_dir.join("scene.npz").is_file(),
            "scene.npz missing in {}",
            dataset_dir.display()
        );

        let ev = run_eval(&dataset_dir, "charuco", vec![2]);
        std::env::remove_var("LMT_VBA_SIDECAR_PATH");

        let ev = ev.expect("run_eval should succeed");
        assert_eq!(ev.method, "charuco");
        assert!(
            ev.max_distance_error_mm < 3.0,
            "max_distance_error_mm = {} should be < 3.0",
            ev.max_distance_error_mm
        );
    }

    // ── real-sidecar test: compare_known ───────────────────────────────────────

    #[cfg(unix)]
    #[test]
    fn compare_known_roundtrip() {
        let _guard = ENV_LOCK.lock().unwrap();

        let python = match sidecar_python() {
            Some(p) => p,
            None => {
                eprintln!("skipping compare_known_roundtrip: python-sidecar venv not found");
                return;
            }
        };

        let tmp = tempdir().expect("tmpdir");
        let wrapper = write_wrapper(tmp.path(), &python);

        let report = serde_json::json!({
            "schema_version": "visual_pose_report.v1",
            "frame": {},
            "cabinet_poses": [
                {
                    "cabinet_id": "V000_R000",
                    "position_mm": [0, 0, 0],
                    "normal": [0, 0, 1],
                    "rotation_matrix": [[1, 0, 0], [0, 1, 0], [0, 0, 1]],
                    "corners_mm": [[-300, -170, 0], [300, -170, 0], [300, 170, 0], [-300, 170, 0]],
                    "reprojection_rms_px": 0.4,
                    "observed_views": 7,
                    "observed_points": 120,
                    "quality": "ok"
                },
                {
                    "cabinet_id": "V001_R000",
                    "position_mm": [702, 0, 0],
                    "normal": [0.0, 0.0, 1.0],
                    "rotation_matrix": [[1, 0, 0], [0, 1, 0], [0, 0, 1]],
                    "corners_mm": [[-300, -170, 0], [300, -170, 0], [300, 170, 0], [-300, 170, 0]],
                    "reprojection_rms_px": 0.4,
                    "observed_views": 7,
                    "observed_points": 120,
                    "quality": "ok"
                }
            ]
        });
        let known = serde_json::json!({
            "cabinets": {"V000_R000": {"size_mm": [600, 340]}, "V001_R000": {"size_mm": [600, 340]}},
            "pairs": [{"a": "V000_R000", "b": "V001_R000", "distance_mm": 700.0, "angle_deg": 0.0}]
        });
        let report_path = tmp.path().join("report.json");
        let known_path = tmp.path().join("known.json");
        std::fs::write(&report_path, serde_json::to_string(&report).unwrap()).unwrap();
        std::fs::write(&known_path, serde_json::to_string(&known).unwrap()).unwrap();

        std::env::set_var("LMT_VBA_SIDECAR_PATH", wrapper.to_str().unwrap());
        let res = run_compare_known(&report_path, &known_path);
        std::env::remove_var("LMT_VBA_SIDECAR_PATH");

        let res = res.expect("run_compare_known should succeed");
        assert!(res.passed, "2mm distance within default 3mm threshold");
        assert_eq!(res.pairs.len(), 1);
        assert!(
            (res.pairs[0].distance_error_mm - 2.0).abs() < 1e-6,
            "distance_error_mm = {} should be 2.0",
            res.pairs[0].distance_error_mm
        );
    }

    // ── error paths (no sidecar) ────────────────────────────────────────────────

    fn seed_project(dir: &Path) {
        let project_yaml = r#"
project:
  name: VBA_Test
  unit: mm
screens:
  MAIN:
    cabinet_count: [4, 2]
    cabinet_size_mm: [500.0, 500.0]
    pixels_per_cabinet: [256, 256]
    shape_prior:
      type: flat
    shape_mode: rectangle
    irregular_mask: []
coordinate_system:
  origin_point: MAIN_V001_R001
  x_axis_point: MAIN_V005_R001
  xy_plane_point: MAIN_V001_R003
output:
  target: neutral
  obj_filename: "{screen_id}.obj"
  weld_vertices_tolerance_mm: 1.0
  triangulate: true
"#;
        std::fs::write(dir.join("project.yaml"), project_yaml).unwrap();
    }

    #[test]
    fn reconstruct_unknown_screen_is_not_found() {
        let dir = tempdir().unwrap();
        seed_project(dir.path());
        let manifest = dir.path().join("capture_manifest.json");
        let err = run_reconstruct(dir.path(), "FLOOR", &manifest).unwrap_err();
        assert!(matches!(err, LmtError::NotFound(_)), "got: {err:?}");
        assert!(format!("{err}").contains("FLOOR"), "got: {err}");
    }

    #[test]
    fn reconstruct_missing_project_yaml_errors() {
        let dir = tempdir().unwrap();
        let manifest = dir.path().join("capture_manifest.json");
        let err = run_reconstruct(dir.path(), "MAIN", &manifest).unwrap_err();
        assert!(format!("{err}").contains("project.yaml"), "got: {err}");
    }

    #[test]
    fn generate_pattern_rejects_non_charuco_method() {
        let dir = tempdir().unwrap();
        seed_project(dir.path());
        let err = run_generate_pattern(dir.path(), "MAIN", "gray_code", None).unwrap_err();
        assert!(matches!(err, LmtError::InvalidInput(_)), "got: {err:?}");
        assert!(format!("{err}").contains("charuco"), "got: {err}");
    }

    #[test]
    fn generate_pattern_unknown_screen_is_not_found() {
        let dir = tempdir().unwrap();
        seed_project(dir.path());
        let err = run_generate_pattern(dir.path(), "FLOOR", "charuco", None).unwrap_err();
        assert!(matches!(err, LmtError::NotFound(_)), "got: {err:?}");
    }

    #[test]
    fn calibrate_missing_dir_is_not_found() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("does_not_exist");
        let err = run_calibrate(dir.path(), "MAIN", &missing, 25.0, "9x9").unwrap_err();
        assert!(matches!(err, LmtError::NotFound(_)), "got: {err:?}");
    }

    #[test]
    fn calibrate_bad_inner_corners_is_invalid_input() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("imgs")).unwrap();
        let err =
            run_calibrate(dir.path(), "MAIN", &dir.path().join("imgs"), 25.0, "nope").unwrap_err();
        assert!(matches!(err, LmtError::InvalidInput(_)), "got: {err:?}");
    }

    #[test]
    fn parse_inner_corners_ok_and_errors() {
        assert_eq!(parse_inner_corners("9x9").unwrap(), [9, 9]);
        assert_eq!(parse_inner_corners("7X5").unwrap(), [7, 5]);
        assert!(parse_inner_corners("9").is_err());
        assert!(parse_inner_corners("0x5").is_err());
    }
}
