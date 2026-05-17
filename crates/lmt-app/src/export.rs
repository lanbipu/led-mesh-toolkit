use lmt_core::export::build::surface_to_mesh_output;
use lmt_core::export::obj::write_obj;
use lmt_core::shape::CabinetArray;
use lmt_core::surface::TargetSoftware;
use lmt_shared::data::{runs, Db};
use lmt_shared::dto::{ReconstructionReport, ShapeMode};
use lmt_shared::error::{LmtError, LmtResult};
use std::path::{Path, PathBuf};

fn parse_target(s: &str) -> LmtResult<TargetSoftware> {
    match s {
        "disguise" => Ok(TargetSoftware::Disguise),
        "unreal" => Ok(TargetSoftware::Unreal),
        "neutral" => Ok(TargetSoftware::Neutral),
        other => Err(LmtError::InvalidInput(format!("unknown target: {other}"))),
    }
}

pub fn build_cabinet_array(screen_cfg: &lmt_shared::dto::ScreenConfig) -> LmtResult<CabinetArray> {
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
            // `out_abs` 给 caller 返回时保持 raw(caller 看到自己给的 path
            // 形态,API 不变);但 strip_prefix 时用 canonical 版本跟
            // canonical project_root 比较——这样 macOS `/var/folders/...`
            // (raw)与 `/private/var/folders/...`(canonical)不会因
            // symlink 错位让 output_obj_path 退回 absolute。
            let abs_raw = ensure_obj_extension(p);
            if let Some(parent) = abs_raw.parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent)?;
                }
            }
            let canon_for_compare = match (abs_raw.parent(), abs_raw.file_name()) {
                (Some(parent), Some(file)) if !parent.as_os_str().is_empty() => {
                    let canon_parent =
                        std::fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
                    canon_parent.join(file)
                }
                _ => abs_raw.clone(),
            };
            // project_root 来自 DB:本 patch 之后写入是 canonical,但旧 row
            // 可能仍是 raw symlink。两种 abs(raw / canonical)各跟两种 root
            // (raw / canonical)都试一遍,任一组合 strip 成功就用它的
            // project-relative 表示,否则 fallback 到原始 absolute。
            let canon_root = std::fs::canonicalize(&project_root)
                .unwrap_or_else(|_| project_root.clone());
            let rel = [
                abs_raw.strip_prefix(&project_root).ok(),
                canon_for_compare.strip_prefix(&project_root).ok(),
                abs_raw.strip_prefix(&canon_root).ok(),
                canon_for_compare.strip_prefix(&canon_root).ok(),
            ]
            .into_iter()
            .flatten()
            .next()
            .map(|r| r.display().to_string())
            .unwrap_or_else(|| abs_raw.display().to_string());
            (abs_raw, rel)
        }
        None => {
            let rel = PathBuf::from("output")
                .join(format!("{}_{target}_run{run_id}.obj", report.screen_id));
            let abs = project_root.join(&rel);
            if let Some(parent) = abs.parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent)?;
                }
            }
            (abs, rel.display().to_string())
        }
    };
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
///
/// `pub` 让 lmt-cli 的 dry-run preview 跟 execute 一样的路径补全。
pub fn ensure_obj_extension(p: &Path) -> PathBuf {
    match p.extension() {
        Some(ext) if ext.eq_ignore_ascii_case("obj") => p.to_path_buf(),
        _ => {
            let mut buf = p.as_os_str().to_os_string();
            buf.push(".obj");
            PathBuf::from(buf)
        }
    }
}

/// 决定一次 OBJ 导出的最终绝对路径。run_export 与 lmt-cli 的 dry-run
/// preview 共享这一份解析,避免 dry-run 报错的目标。
///
/// - 给定 `dst_abs_path` 时:用 [`ensure_obj_extension`] 补 .obj 扩展名。
/// - 缺省时:回退到旧的 `<project>/output/<screen>_<target>_run<id>.obj`。
pub fn resolve_export_dst(
    project_root: &Path,
    screen_id: &str,
    target: &str,
    run_id: i64,
    dst_abs_path: Option<&Path>,
) -> PathBuf {
    match dst_abs_path {
        Some(p) => ensure_obj_extension(p),
        None => project_root
            .join("output")
            .join(format!("{screen_id}_{target}_run{run_id}.obj")),
    }
}

/// 从 reconstruction_runs 表读 `(project_path, screen_id)`,供 dry-run
/// 在不读 report.json 的情况下解析默认导出路径。
pub fn lookup_run_paths(db: Db, run_id: i64) -> LmtResult<(String, String)> {
    let conn = db.lock().unwrap();
    conn.query_row(
        "SELECT project_path, screen_id FROM reconstruction_runs WHERE id = ?1",
        [run_id],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
    )
    .map_err(|_| LmtError::NotFound(format!("run id {run_id}")))
}
