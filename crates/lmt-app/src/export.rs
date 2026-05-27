use lmt_core::export::build::surface_to_mesh_output;
use lmt_core::export::obj::write_obj;
use lmt_core::shape::CabinetArray;
use lmt_core::surface::{GridTopology, QualityMetrics, ReconstructedSurface, TargetSoftware};
use lmt_core::uv::compute_grid_uv;
use lmt_shared::data::{runs, Db};
use lmt_shared::dto::{CabinetPoseReportFile, ExportPoseObjResult, ReconstructionReport, ShapeMode};
use lmt_shared::error::{LmtError, LmtResult};
use nalgebra::Vector3;
use std::path::{Path, PathBuf};

fn parse_target(s: &str) -> LmtResult<TargetSoftware> {
    match s {
        "disguise" => Ok(TargetSoftware::Disguise),
        "unreal" => Ok(TargetSoftware::Unreal),
        "neutral" => Ok(TargetSoftware::Neutral),
        other => Err(LmtError::InvalidInput(format!("unknown target: {other}"))),
    }
}

pub fn build_shape_prior(
    screen_cfg: &lmt_shared::dto::ScreenConfig,
) -> LmtResult<lmt_core::shape::ShapePrior> {
    use lmt_shared::dto::ShapePriorConfig;
    Ok(match &screen_cfg.shape_prior {
        ShapePriorConfig::Flat => lmt_core::shape::ShapePrior::Flat,
        ShapePriorConfig::Curved { radius_mm, .. } => {
            lmt_core::shape::ShapePrior::Curved { radius_mm: *radius_mm }
        }
        ShapePriorConfig::Folded { fold_seams_at_columns } => lmt_core::shape::ShapePrior::Folded {
            fold_seam_columns: fold_seams_at_columns.clone(),
        },
    })
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

/// Export one world-frame OBJ per cabinet from a `cabinet_pose_report.json`.
///
/// ## Geometry frame
///
/// Vertices are emitted in the **screen-local world frame** as supplied by the
/// visual pipeline: right-handed, **+Y up** (panel height axis), **+Z outward**,
/// metres. This frame is disguise-native and imports upright without any further
/// rotation.
///
/// The core-model→target axis adapter (`surface_to_mesh_output` with
/// `target=Disguise/Unreal`) is intentionally **NOT** applied. The visual world
/// frame is already a target-agnostic world frame, and Path A's adapters are
/// designed for the internal +Z-up core model frame: applying them here would
/// reflect/mis-orient each panel (det −1 mismatch). Geometry is therefore always
/// built with `TargetSoftware::Neutral` (identity transform, raw world coords).
///
/// ## `target` argument
///
/// The `target` string is **validated** (unknown values are rejected) and
/// **recorded** in `ExportPoseObjResult.target`, but does not currently remap
/// axes or scale units — the world frame is already in the correct convention
/// for all supported targets.
pub fn run_export_pose_obj(
    pose_report_path: &Path,
    target: &str,
    out_dir: &Path,
) -> LmtResult<ExportPoseObjResult> {
    // Validate target string but do NOT use the enum for geometry: see doc comment.
    let _ = parse_target(target)?;
    let report: CabinetPoseReportFile =
        serde_json::from_slice(&std::fs::read(pose_report_path)?)?;
    if report.cabinet_poses.is_empty() {
        return Err(LmtError::InvalidInput(
            "pose report has no cabinet_poses".into(),
        ));
    }
    std::fs::create_dir_all(out_dir)?;
    let mut files = Vec::with_capacity(report.cabinet_poses.len());
    for cab in &report.cabinet_poses {
        // Sanitize cabinet_id before using it as a filename component.
        // Must be non-empty, consist only of [A-Za-z0-9._-], and not be "." or "..".
        if cab.cabinet_id.is_empty()
            || !cab.cabinet_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
            || cab.cabinet_id == "."
            || cab.cabinet_id == ".."
        {
            return Err(LmtError::InvalidInput(format!(
                "unsafe cabinet_id in pose report: {:?}",
                cab.cabinet_id
            )));
        }
        let surface = panel_surface(&cab.cabinet_id, &cab.corners_mm);
        // 单 quad → 1×1；cabinet_size 只参与 absent-cell 逻辑（此处无），填占位值。
        // Always use Neutral (identity) so vertices stay in the raw world frame —
        // see doc comment on run_export_pose_obj.
        let cabinet_array = CabinetArray::rectangle(1, 1, [1.0, 1.0]);
        let mesh = surface_to_mesh_output(&surface, &cabinet_array, TargetSoftware::Neutral, 0.0)?;
        let out = out_dir.join(format!("{}_{target}.obj", cab.cabinet_id));
        write_obj(&mesh, &out)?;
        files.push(out.display().to_string());
    }
    Ok(ExportPoseObjResult {
        target: target.to_string(),
        cabinet_count: files.len(),
        files,
    })
}

/// 一块 cabinet 的 4 个世界系角点（mm，BL,BR,TR,TL）→ 1×1 ReconstructedSurface（米）。
/// 网格顶点行主序 [(0,0),(1,0),(0,1),(1,1)]=[BL,BR,TL,TR]，故把 [BL,BR,TR,TL] 重排为
/// [BL,BR,TL,TR]（索引 0,1,3,2）。
fn panel_surface(cabinet_id: &str, corners_mm: &[[f64; 3]; 4]) -> ReconstructedSurface {
    let m = |i: usize| {
        Vector3::new(
            corners_mm[i][0] / 1000.0,
            corners_mm[i][1] / 1000.0,
            corners_mm[i][2] / 1000.0,
        )
    };
    let topology = GridTopology { cols: 1, rows: 1 };
    ReconstructedSurface {
        screen_id: cabinet_id.to_string(),
        uv_coords: compute_grid_uv(topology),
        vertices: vec![m(0), m(1), m(3), m(2)],
        topology,
        quality_metrics: QualityMetrics {
            method: "pose_report_quad".into(),
            measured_count: 4,
            expected_count: 4,
            ..Default::default()
        },
        scatter_fit: None,
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const BENCH_REPORT: &str = r#"{
          "schema_version": "visual_pose_report.v1",
          "frame": {},
          "cabinet_poses": [
            {"cabinet_id":"V000_R000",
             "corners_mm":[[-300,-170,0],[300,-170,0],[300,170,0],[-300,170,0]]},
            {"cabinet_id":"V000_R001",
             "corners_mm":[[321,-391,-4],[793,-376,-1117],[803,303,-1104],[331,289,8]]}
          ]
        }"#;

    #[test]
    fn export_pose_obj_writes_one_world_frame_obj_per_cabinet() {
        let dir = tempdir().unwrap();
        let rp = dir.path().join("BENCH_cabinet_pose_report.json");
        std::fs::write(&rp, BENCH_REPORT).unwrap();
        let out_dir = dir.path().join("out");

        let res = run_export_pose_obj(&rp, "neutral", &out_dir).unwrap();
        assert_eq!(res.cabinet_count, 2);
        assert_eq!(res.files.len(), 2);

        let obj0 = out_dir.join("V000_R000_neutral.obj");
        let obj1 = out_dir.join("V000_R001_neutral.obj");
        assert!(obj0.is_file() && obj1.is_file());

        let text0 = std::fs::read_to_string(&obj0).unwrap();
        assert_eq!(text0.lines().filter(|l| l.starts_with("v ")).count(), 4);
        assert_eq!(text0.lines().filter(|l| l.starts_with("f ")).count(), 2);
        // neutral = raw world frame in meters → BL corner (-0.3,-0.17,0) baked into geometry
        assert!(text0.contains("v -0.3 -0.17 0"), "got:\n{text0}");
    }

    #[test]
    fn export_pose_obj_disguise_target_equals_raw_world_frame() {
        // Fix 1 regression lock: "disguise" must produce the same raw world-frame
        // geometry as "neutral". The axis adapter must NOT be applied to the visual
        // world frame (which is already in disguise convention). If the adapter were
        // applied, BL would become v -0.3 0 0.17 (x,z,-y swap) — wrong.
        let dir = tempdir().unwrap();
        let rp = dir.path().join("BENCH_cabinet_pose_report.json");
        std::fs::write(&rp, BENCH_REPORT).unwrap();
        let out_dir = dir.path().join("out_disguise");

        let res = run_export_pose_obj(&rp, "disguise", &out_dir).unwrap();
        assert_eq!(res.target, "disguise");
        assert_eq!(res.cabinet_count, 2);

        let obj0 = out_dir.join("V000_R000_disguise.obj");
        assert!(obj0.is_file());
        let text0 = std::fs::read_to_string(&obj0).unwrap();
        // Must equal the raw world frame — NOT the axis-swapped v -0.3 0 0.17
        assert!(
            text0.contains("v -0.3 -0.17 0"),
            "disguise output should equal raw world frame; got:\n{text0}"
        );
        assert!(
            !text0.contains("v -0.3 0 0.17"),
            "axis-swapped vertex found — adapter was wrongly applied; got:\n{text0}"
        );
    }

    #[test]
    fn export_pose_obj_rejects_unsafe_cabinet_id() {
        // Fix 3: cabinet_id from external JSON must be sanitized before use in path.
        for bad_id in &["../escape", "a/b", "", ".", ".."] {
            let dir = tempdir().unwrap();
            let report = format!(
                r#"{{
                  "schema_version": "visual_pose_report.v1",
                  "frame": {{}},
                  "cabinet_poses": [
                    {{"cabinet_id":{},
                     "corners_mm":[[-300,-170,0],[300,-170,0],[300,170,0],[-300,170,0]]}}
                  ]
                }}"#,
                serde_json::to_string(bad_id).unwrap()
            );
            let rp = dir.path().join("pose_report.json");
            std::fs::write(&rp, &report).unwrap();
            let out_dir = dir.path().join("out");

            let result = run_export_pose_obj(&rp, "neutral", &out_dir);
            assert!(
                matches!(result, Err(LmtError::InvalidInput(_))),
                "expected InvalidInput for cabinet_id={bad_id:?}, got {result:?}"
            );
            // Nothing should be written outside out_dir (out_dir itself may be created)
            let escaped = dir.path().join("escape");
            assert!(!escaped.exists(), "path traversal file written for cabinet_id={bad_id:?}");
        }
    }
}
