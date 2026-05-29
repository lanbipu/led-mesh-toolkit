//! `lmt visual ...` subcommands. Thin transport: parse → call lmt_app::visual → envelope.
//! No business logic here; all logic lives in lmt_app::visual.

use crate::cli::VisualCmd;
use crate::commands::util::{self, DestructiveDecision};
use crate::output::{self, Mode};
use lmt_shared::envelope::{error_codes, ApiError};
use std::io::Write as _;
use std::path::Path;

pub fn run(cmd: VisualCmd, mode: Mode, yes: bool, dry_run: bool) -> i32 {
    match cmd {
        VisualCmd::Reconstruct {
            project_path,
            screen_id,
            capture_manifest,
            images,
            method,
        } => reconstruct(
            mode,
            &project_path,
            &screen_id,
            capture_manifest,
            images,
            &method,
            yes,
            dry_run,
        ),
        VisualCmd::Simulate { config, out } => simulate(mode, &config, &out, yes, dry_run),
        VisualCmd::Eval {
            dataset,
            method,
            seed_matrix,
        } => eval(mode, &dataset, &method, seed_matrix),
        VisualCmd::CompareKnown { report, known } => compare_known(mode, &report, &known),
        VisualCmd::Calibrate {
            project_path,
            screen_id,
            checkerboard_dir,
            square_mm,
            inner,
        } => calibrate(
            mode,
            &project_path,
            &screen_id,
            &checkerboard_dir,
            square_mm,
            &inner,
            yes,
            dry_run,
        ),
        VisualCmd::GeneratePattern {
            project_path,
            screen_id,
            method,
            screen_mapping,
        } => generate_pattern(
            mode, &project_path, &screen_id, &method,
            screen_mapping.as_deref(), yes, dry_run,
        ),
        VisualCmd::GenerateStructuredLight {
            project_path,
            screen_id,
            dot_spacing,
            dot_radius,
            screen_mapping,
        } => generate_structured_light(
            mode, &project_path, &screen_id, dot_spacing, dot_radius,
            screen_mapping.as_deref(), yes, dry_run,
        ),
        VisualCmd::DecodeStructuredLight {
            input_path,
            sl_meta,
            out,
        } => decode_structured_light(mode, &input_path, &sl_meta, &out, yes, dry_run),
        VisualCmd::ReconstructStructuredLight {
            project_path,
            screen_id,
            sl_meta,
            intrinsics,
            correspondences,
        } => reconstruct_structured_light(
            mode, &project_path, &screen_id, &sl_meta, &intrinsics, &correspondences, yes, dry_run,
        ),
    }
}

// ---------------------------------------------------------------------------
// reconstruct
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn reconstruct(
    mode: Mode,
    project_path: &str,
    screen_id: &str,
    capture_manifest: Option<String>,
    images: Option<String>,
    method: &str,
    yes: bool,
    dry_run: bool,
) -> i32 {
    // Only charuco is implemented; structured-light is gated (spec §16).
    if method != "charuco" {
        return output::err(
            mode,
            ApiError::new(
                error_codes::UNSUPPORTED,
                "only --method charuco implemented (structured-light is gated, spec §16)",
            ),
        );
    }

    // Resolve manifest path from the two mutually-exclusive convenience args.
    let manifest = match (capture_manifest, images) {
        (Some(m), _) => m,
        (None, Some(_)) => {
            return output::err(
                mode,
                ApiError::new(
                    error_codes::UNSUPPORTED,
                    "--images convenience not yet wired; pass --capture-manifest",
                ),
            );
        }
        (None, None) => {
            return output::err(
                mode,
                ApiError::new(
                    error_codes::INVALID_INPUT,
                    "need --capture-manifest <json> (or --images <dir>)",
                ),
            );
        }
    };

    let decision = match util::gate_destructive(yes, dry_run, "visual reconstruct") {
        Ok(d) => d,
        Err(e) => return output::err(mode, e),
    };

    match decision {
        DestructiveDecision::DryRun => {
            // Match run_reconstruct's actual write targets: both files land under
            // <project>/measurements/. Use a vec! array (machine-parsable, no '+'
            // ambiguity), mirroring total_station's grid dry-run branch.
            let would_write = vec![
                format!("{project_path}/measurements/measured.yaml"),
                format!("{project_path}/measurements/{screen_id}_cabinet_pose_report.json"),
            ];
            let payload = serde_json::json!({
                "dry_run": true,
                "would_write": would_write,
                "capture_manifest": manifest,
            });
            output::ok(mode, payload, |_| {
                let _ = writeln!(
                    std::io::stdout(),
                    "[dry-run] would reconstruct screen {screen_id} from manifest {manifest}"
                );
            })
        }
        DestructiveDecision::Execute => {
            match lmt_app::visual::run_reconstruct(
                Path::new(project_path),
                screen_id,
                Path::new(&manifest),
            ) {
                Ok(r) => output::ok(mode, r, |p| {
                    let _ = writeln!(
                        std::io::stdout(),
                        "reconstructed {} cabinets (ba_rms={:.3}px)\n  measured: {}\n  poses: {}",
                        p.cabinet_count,
                        p.ba_rms_px,
                        p.measured_yaml_path,
                        p.pose_report_path
                    );
                }),
                Err(e) => output::err(mode, ApiError::from(e)),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// calibrate
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn calibrate(
    mode: Mode,
    project_path: &str,
    screen_id: &str,
    checkerboard_dir: &str,
    square_mm: f64,
    inner: &str,
    yes: bool,
    dry_run: bool,
) -> i32 {
    let decision = match util::gate_destructive(yes, dry_run, "visual calibrate") {
        Ok(d) => d,
        Err(e) => return output::err(mode, e),
    };

    match decision {
        DestructiveDecision::DryRun => {
            let payload = serde_json::json!({
                "dry_run": true,
                "would_write": format!("{project_path}/calibration/{screen_id}_intrinsics.json"),
                "checkerboard_dir": checkerboard_dir,
                "square_mm": square_mm,
                "inner": inner,
            });
            output::ok(mode, payload, |_| {
                let _ = writeln!(
                    std::io::stdout(),
                    "[dry-run] would calibrate screen {screen_id} from {checkerboard_dir}"
                );
            })
        }
        DestructiveDecision::Execute => {
            match lmt_app::visual::run_calibrate(
                Path::new(project_path),
                screen_id,
                Path::new(checkerboard_dir),
                square_mm,
                inner,
            ) {
                Ok(r) => output::ok(mode, r, |p| {
                    let _ = writeln!(
                        std::io::stdout(),
                        "calibrated: reproj={:.3}px frames={} → {}",
                        p.reproj_error_px,
                        p.frames_used,
                        p.intrinsics_path
                    );
                }),
                Err(e) => output::err(mode, ApiError::from(e)),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// generate_pattern
// ---------------------------------------------------------------------------

fn generate_pattern(
    mode: Mode,
    project_path: &str,
    screen_id: &str,
    method: &str,
    screen_mapping: Option<&str>,
    yes: bool,
    dry_run: bool,
) -> i32 {
    // Only charuco is supported.
    if method != "charuco" {
        return output::err(
            mode,
            ApiError::new(
                error_codes::UNSUPPORTED,
                format!("unsupported pattern method '{method}' (only 'charuco')"),
            ),
        );
    }

    let decision = match util::gate_destructive(yes, dry_run, "visual generate-pattern") {
        Ok(d) => d,
        Err(e) => return output::err(mode, e),
    };

    match decision {
        DestructiveDecision::DryRun => {
            let payload = serde_json::json!({
                "dry_run": true,
                "would_write": format!("{project_path}/patterns/{screen_id}/"),
                "method": method,
            });
            output::ok(mode, payload, |_| {
                let _ = writeln!(
                    std::io::stdout(),
                    "[dry-run] would generate {method} patterns for screen {screen_id}"
                );
            })
        }
        DestructiveDecision::Execute => {
            match lmt_app::visual::run_generate_pattern(
                Path::new(project_path),
                screen_id,
                method,
                screen_mapping.map(Path::new),
            ) {
                Ok(r) => output::ok(mode, r, |p| {
                    let _ = writeln!(
                        std::io::stdout(),
                        "generated {} cabinets, {} total markers → {}",
                        p.cabinet_count,
                        p.total_markers,
                        p.output_dir
                    );
                }),
                Err(e) => output::err(mode, ApiError::from(e)),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// generate_structured_light
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn generate_structured_light(
    mode: Mode,
    project_path: &str,
    screen_id: &str,
    dot_spacing: u32,
    dot_radius: u32,
    screen_mapping: Option<&str>,
    yes: bool,
    dry_run: bool,
) -> i32 {
    let decision = match util::gate_destructive(yes, dry_run, "visual generate-structured-light") {
        Ok(d) => d,
        Err(e) => return output::err(mode, e),
    };

    match decision {
        DestructiveDecision::DryRun => {
            let payload = serde_json::json!({
                "dry_run": true,
                "would_write": format!("{project_path}/patterns/{screen_id}/sl/"),
            });
            output::ok(mode, payload, |_| {
                let _ = writeln!(
                    std::io::stdout(),
                    "[dry-run] would generate structured-light sequence for screen {screen_id}"
                );
            })
        }
        DestructiveDecision::Execute => {
            match lmt_app::visual::run_generate_structured_light(
                Path::new(project_path),
                screen_id,
                dot_spacing,
                dot_radius,
                screen_mapping.map(Path::new),
            ) {
                Ok(r) => output::ok(mode, r, |p| {
                    let _ = writeln!(
                        std::io::stdout(),
                        "generated {} dots across {} frames → {}",
                        p.n_dots,
                        p.n_frames,
                        p.output_dir
                    );
                }),
                Err(e) => output::err(mode, ApiError::from(e)),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// decode_structured_light
// ---------------------------------------------------------------------------

fn decode_structured_light(
    mode: Mode,
    input_path: &str,
    sl_meta: &str,
    out: &str,
    yes: bool,
    dry_run: bool,
) -> i32 {
    let decision = match util::gate_destructive(yes, dry_run, "visual decode-structured-light") {
        Ok(d) => d,
        Err(e) => return output::err(mode, e),
    };

    match decision {
        DestructiveDecision::DryRun => {
            let payload = serde_json::json!({
                "dry_run": true,
                "would_write": out,
            });
            output::ok(mode, payload, |_| {
                let _ = writeln!(std::io::stdout(), "[dry-run] would decode → {out}");
            })
        }
        DestructiveDecision::Execute => {
            match lmt_app::visual::run_decode_structured_light(
                Path::new(input_path),
                Path::new(sl_meta),
                Path::new(out),
            ) {
                Ok(r) => output::ok(mode, r, |p| {
                    let _ = writeln!(
                        std::io::stdout(),
                        "decoded {} dots → {}",
                        p.n_dots_decoded,
                        p.output_path
                    );
                }),
                Err(e) => output::err(mode, ApiError::from(e)),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// reconstruct_structured_light
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn reconstruct_structured_light(
    mode: Mode,
    project_path: &str,
    screen_id: &str,
    sl_meta: &str,
    intrinsics: &str,
    correspondences: &[String],
    yes: bool,
    dry_run: bool,
) -> i32 {
    let decision =
        match util::gate_destructive(yes, dry_run, "visual reconstruct-structured-light") {
            Ok(d) => d,
            Err(e) => return output::err(mode, e),
        };

    match decision {
        DestructiveDecision::DryRun => {
            let would_write = vec![
                format!("{project_path}/measurements/measured.yaml"),
                format!("{project_path}/measurements/{screen_id}_cabinet_pose_report.json"),
            ];
            let payload = serde_json::json!({
                "dry_run": true,
                "would_write": would_write,
                "correspondences": correspondences,
                "sl_meta": sl_meta,
                "intrinsics": intrinsics,
            });
            output::ok(mode, payload, |_| {
                let _ = writeln!(
                    std::io::stdout(),
                    "[dry-run] would reconstruct screen {screen_id} from {} poses",
                    correspondences.len()
                );
            })
        }
        DestructiveDecision::Execute => {
            match lmt_app::visual::run_reconstruct_structured_light(
                Path::new(project_path),
                screen_id,
                Path::new(sl_meta),
                Path::new(intrinsics),
                correspondences,
            ) {
                Ok(r) => output::ok(mode, r, |p| {
                    let _ = writeln!(
                        std::io::stdout(),
                        "reconstructed {} cabinets (ba_rms={:.3}px)\n  measured: {}\n  poses: {}",
                        p.cabinet_count, p.ba_rms_px, p.measured_yaml_path, p.pose_report_path
                    );
                }),
                Err(e) => output::err(mode, ApiError::from(e)),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// simulate
// ---------------------------------------------------------------------------

fn simulate(mode: Mode, config: &str, out: &str, yes: bool, dry_run: bool) -> i32 {
    let decision = match util::gate_destructive(yes, dry_run, "visual simulate") {
        Ok(d) => d,
        Err(e) => return output::err(mode, e),
    };

    match decision {
        DestructiveDecision::DryRun => {
            let payload = serde_json::json!({
                "dry_run": true,
                "would_write": format!("{out}/scene.npz + meta.json"),
                "config": config,
            });
            output::ok(mode, payload, |_| {
                let _ = writeln!(
                    std::io::stdout(),
                    "[dry-run] would simulate dataset from config {config} → {out}"
                );
            })
        }
        DestructiveDecision::Execute => {
            match lmt_app::visual::run_simulate(Path::new(config), Path::new(out)) {
                Ok(r) => output::ok(mode, r, |p| {
                    let _ = writeln!(
                        std::io::stdout(),
                        "simulated {} views, {} obs → {}",
                        p.n_views,
                        p.n_observations,
                        p.dataset_dir
                    );
                }),
                Err(e) => output::err(mode, ApiError::from(e)),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// eval
// ---------------------------------------------------------------------------

fn eval(mode: Mode, dataset: &str, method: &str, seed_matrix: Vec<i64>) -> i32 {
    // eval is write_safe — no gate needed.
    match lmt_app::visual::run_eval(Path::new(dataset), method, seed_matrix) {
        Ok(r) => output::ok(mode, r, |p| {
            let _ = writeln!(
                std::io::stdout(),
                "eval {}: size={:.2}mm dist={:.2}mm angle={:.3}deg",
                p.method,
                p.max_size_error_mm,
                p.max_distance_error_mm,
                p.max_angle_error_deg
            );
        }),
        Err(e) => output::err(mode, ApiError::from(e)),
    }
}

// ---------------------------------------------------------------------------
// compare_known
// ---------------------------------------------------------------------------

fn compare_known(mode: Mode, report: &str, known: &str) -> i32 {
    // compare-known is write_safe (reads two JSON files, writes nothing) — no gate.
    match lmt_app::visual::run_compare_known(Path::new(report), Path::new(known)) {
        Ok(r) => output::ok(mode, r, |p| {
            let _ = writeln!(
                std::io::stdout(),
                "compare-known: passed={} ({} cabinets, {} pairs)",
                p.passed,
                p.cabinets.len(),
                p.pairs.len()
            );
        }),
        Err(e) => output::err(mode, ApiError::from(e)),
    }
}
