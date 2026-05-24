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
    calibrate, compare_known, eval, generate_pattern, reconstruct, simulate, CalibrateArgs,
    CompareKnownArgs, EvalArgs, GeneratePatternArgs, ReconstructArgs, SimulateArgs,
};
use lmt_adapter_visual_ba::ipc;

use lmt_shared::dto::{
    CabinetPoseSummary, CabinetSizeCheck, CalibrateResult, CompareKnownResult, EvalResult,
    GeneratePatternResult, PairCheck, SimulateResult, VisualReconstructResult,
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

    // Persist MeasuredPoints to measured.yaml with the backup + atomic-write +
    // cross-screen-guard + ROLLBACK pattern from run_import (not run_import_scatter,
    // which omits rollback). The reconstruction output is the expensive product of
    // a multi-minute BA run, so a half-written measured.yaml must never leave the
    // user with neither the old file nor the new result.
    let measured_yaml_path = measurements_dir.join("measured.yaml");
    let backup_path = measurements_dir.join("measured.yaml.bak");

    crate::total_station::check_import_no_screen_conflict(project_path, screen_id)?;

    // If a previous measured.yaml exists, rename it to .bak (overwriting any prior
    // .bak), so we can restore it if the new write fails.
    let did_backup = if measured_yaml_path.exists() {
        std::fs::rename(&measured_yaml_path, &backup_path)?;
        true
    } else {
        false
    };

    // Write the new file. On any failure: remove the half-written measured.yaml,
    // then restore the previous version from .bak.
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

/// Generate ChArUco calibration patterns for one screen's cabinets, written to
/// `<project>/patterns/<screen_id>`.
pub fn run_generate_pattern(
    project_path: &Path,
    screen_id: &str,
    method: &str,
) -> LmtResult<GeneratePatternResult> {
    if method != "charuco" {
        return Err(LmtError::InvalidInput(format!(
            "unsupported pattern method '{method}' (only 'charuco')"
        )));
    }

    let cfg = load_project_yaml_from_path(project_path)?;
    let screen_cfg = load_screen(&cfg, screen_id)?;
    let cabinet_array = ipc_cabinet_array(screen_cfg);

    // screen_resolution = pixels_per_cabinet × cabinet_count. pixels_per_cabinet
    // is optional in the schema, so it's required for pattern generation.
    let ppc = screen_cfg.pixels_per_cabinet.ok_or_else(|| {
        LmtError::InvalidInput(format!(
            "screen '{screen_id}' has no pixels_per_cabinet; required for pattern generation"
        ))
    })?;
    let [cols, rows] = screen_cfg.cabinet_count;
    let screen_resolution = [ppc[0] * cols, ppc[1] * rows];

    let output_dir = project_path.join("patterns").join(screen_id);
    std::fs::create_dir_all(&output_dir)?;

    let args = GeneratePatternArgs {
        screen_id: screen_id.to_string(),
        cabinet_array,
        output_dir: output_dir.display().to_string(),
        screen_resolution,
        progress_tx: None,
        cancel: None,
    };

    let out = rt()?.block_on(generate_pattern(args)).map_err(map_vba_err)?;

    Ok(GeneratePatternResult {
        output_dir: out.output_dir,
        cabinet_count: out.cabinet_count as usize,
        markers_per_cabinet: out.markers_per_cabinet,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use tempfile::tempdir;

    // ── sidecar wrapper plumbing (mirrors adapter's simulate_eval_test) ────────

    /// Serialize env-var mutation across tests in this binary, since they share
    /// the process and all touch LMT_VBA_SIDECAR_PATH.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Path to the project's python-sidecar venv interpreter, computed from this
    /// crate's manifest dir (`crates/lmt-app` → `../../python-sidecar/.venv/bin`).
    /// We canonicalize only the parent `.venv/bin` dir and KEEP the `python`
    /// basename: launching via that path activates the venv's sys.path, while
    /// canonicalizing the file would resolve the symlink to the bare interpreter.
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
        let err = run_generate_pattern(dir.path(), "MAIN", "gray_code").unwrap_err();
        assert!(matches!(err, LmtError::InvalidInput(_)), "got: {err:?}");
        assert!(format!("{err}").contains("charuco"), "got: {err}");
    }

    #[test]
    fn generate_pattern_unknown_screen_is_not_found() {
        let dir = tempdir().unwrap();
        seed_project(dir.path());
        let err = run_generate_pattern(dir.path(), "FLOOR", "charuco").unwrap_err();
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
