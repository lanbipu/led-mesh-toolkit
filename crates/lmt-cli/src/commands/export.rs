//! `lmt export ...` 子命令。

use crate::cli::ExportCmd;
use crate::commands::util::{self, DestructiveDecision};
use crate::output::{self, Mode};
use lmt_shared::envelope::{error_codes, ApiError};
use std::io::Write as _;
use std::path::{Path, PathBuf};

pub fn run(cmd: ExportCmd, mode: Mode, db_arg: Option<&Path>, yes: bool, dry_run: bool) -> i32 {
    match cmd {
        ExportCmd::Obj {
            run_id,
            target,
            dst,
        } => obj(mode, db_arg, run_id, &target, dst, yes, dry_run),
        ExportCmd::PoseObj {
            pose_report,
            target,
            out_dir,
            root,
            ground,
        } => pose_obj(mode, &pose_report, &target, &out_dir, root.as_deref(), ground, yes, dry_run),
    }
}

fn obj(
    mode: Mode,
    db_arg: Option<&Path>,
    run_id: i64,
    target: &str,
    dst: Option<PathBuf>,
    yes: bool,
    dry_run: bool,
) -> i32 {
    let decision = match util::gate_destructive(yes, dry_run, "export obj") {
        Ok(d) => d,
        Err(e) => return output::err(mode, e),
    };

    // dst 必须是 absolute——run_export 把它存进 reconstruction_runs.output_obj_path,
    // 该字段约定 "project-relative or absolute"。relative 字符串会让消费者分不清是
    // project-relative 还是 cwd-relative。我们在 CLI 边界 absolutize 一次,保证写
    // 进 DB 的字符串与调用方 cwd 无关。
    let dst: Option<PathBuf> = match dst {
        None => None,
        Some(p) => match util::absolutize(&p) {
            Ok(abs) => Some(abs),
            Err(e) => return output::err(mode, e),
        },
    };

    match decision {
        DestructiveDecision::DryRun => {
            // 校验:target 合法 + run 在(只读)DB 里存在。不读 report.json、
            // 不算 mesh、不创建/不 migrate DB——readonly 路径专用。
            //
            // 关键:would_write_dst 必须用跟 execute 一致的解析(走
            // lmt_app::export::resolve_export_dst),否则 dry-run preview
            // 报错的目标路径,agent 会基于 false-positive 同意导出。
            let target_known = matches!(target, "disguise" | "unreal" | "neutral");
            if !target_known {
                return output::err(
                    mode,
                    ApiError::new(
                        error_codes::INVALID_INPUT,
                        format!("unknown target: {target}"),
                    ),
                );
            }
            let db_opt = match util::open_db_readonly(db_arg) {
                Ok(o) => o,
                Err(e) => return output::err(mode, e),
            };
            let db_exists = db_opt.is_some();
            // 任何一个失败(DB 缺失 / run 不存在)都报 not_found,execute 必败。
            let (project_path, screen_id) = match db_opt {
                Some(db) => match lmt_app::export::lookup_run_paths(db, run_id) {
                    Ok(p) => p,
                    Err(e) => return output::err(mode, ApiError::from(e)),
                },
                None => {
                    return output::err(
                        mode,
                        ApiError::new(
                            error_codes::NOT_FOUND,
                            format!("run id {run_id} (db file does not exist)"),
                        ),
                    );
                }
            };
            let project_root = std::path::PathBuf::from(&project_path);
            let resolved = lmt_app::export::resolve_export_dst(
                &project_root,
                &screen_id,
                target,
                run_id,
                dst.as_deref(),
            );
            let payload = serde_json::json!({
                "dry_run": true,
                "run_id": run_id,
                "target": target,
                "would_write_dst": resolved.display().to_string(),
                "would_update_db_row": run_id,
                "db_exists": db_exists,
                "project_path": project_path,
                "screen_id": screen_id,
            });
            output::ok(mode, payload, |_| {
                let _ = writeln!(
                    std::io::stdout(),
                    "[dry-run] would export run #{run_id} target={target} to {}",
                    resolved.display()
                );
            })
        }
        DestructiveDecision::Execute => {
            let db = match util::open_db(db_arg) {
                Ok(d) => d,
                Err(e) => return output::err(mode, e),
            };
            match lmt_app::export::run_export(db, run_id, target, dst.as_deref()) {
                Ok(out_abs) => output::ok(
                    mode,
                    serde_json::json!({"written": out_abs, "run_id": run_id, "target": target}),
                    |_| {
                        let _ = writeln!(std::io::stdout(), "wrote {out_abs}");
                    },
                ),
                Err(e) => output::err(mode, ApiError::from(e)),
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn pose_obj(
    mode: Mode,
    pose_report: &str,
    target: &str,
    out_dir: &Path,
    root: Option<&str>,
    ground: bool,
    yes: bool,
    dry_run: bool,
) -> i32 {
    let decision = match util::gate_destructive(yes, dry_run, "export pose-obj") {
        Ok(d) => d,
        Err(e) => return output::err(mode, e),
    };
    match decision {
        DestructiveDecision::DryRun => {
            if !matches!(target, "disguise" | "unreal" | "neutral") {
                return output::err(
                    mode,
                    ApiError::new(
                        error_codes::INVALID_INPUT,
                        format!("unknown target: {target}"),
                    ),
                );
            }
            if !Path::new(pose_report).is_file() {
                return output::err(
                    mode,
                    ApiError::new(
                        error_codes::NOT_FOUND,
                        format!("pose report not found: {pose_report}"),
                    ),
                );
            }
            let payload = serde_json::json!({
                "dry_run": true,
                "pose_report": pose_report,
                "target": target,
                "root": root,
                "ground": ground,
                "would_write_under": out_dir.display().to_string(),
            });
            output::ok(mode, payload, |_| {
                let _ = writeln!(
                    std::io::stdout(),
                    "[dry-run] would export per-cabinet OBJ from {pose_report} into {}",
                    out_dir.display()
                );
            })
        }
        DestructiveDecision::Execute => {
            match lmt_app::export::run_export_pose_obj(
                Path::new(pose_report),
                target,
                out_dir,
                root,
                ground,
            ) {
                Ok(r) => output::ok(mode, r, |p| {
                    let _ = writeln!(
                        std::io::stdout(),
                        "wrote {} OBJ ({} target) under {}",
                        p.cabinet_count,
                        p.target,
                        out_dir.display()
                    );
                }),
                Err(e) => output::err(mode, ApiError::from(e)),
            }
        }
    }
}
