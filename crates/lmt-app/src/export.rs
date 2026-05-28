use lmt_core::export::build::surface_to_mesh_output;
use lmt_core::export::obj::write_obj;
use lmt_core::shape::CabinetArray;
use lmt_core::surface::{
    GridTopology, MeshOutput, QualityMetrics, ReconstructedSurface, TargetSoftware, MAX_GRID_DIM,
};
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

/// Merge a `cabinet_pose_report.json` into ONE world-frame OBJ.
///
/// 几何：每块 cabinet 一块独立 quad（4 顶点，不焊接），世界坐标烘进顶点。
/// UV：一张整体 0-1 网格，每块占它在 (cols×rows) 网格里的格子（cols/rows 由 cabinet_id 反推）。
/// 几何按 `TargetSoftware::Neutral` 原样输出（pose report 已是 +Y up / +Z outward = disguise 约定，
/// 不套 core→target 适配器）。`target` 字符串校验+记录但不改轴。
///
/// `--root`：以该 cabinet 局部系为世界系（它轴对齐落原点），其余块保持真实相对位姿。
/// `--ground`：底边贴地（基准=root 块，未给 root 时=整体）。
pub fn run_export_pose_obj(
    pose_report_path: &Path,
    target: &str,
    out_file: &Path,
    root: Option<&str>,
    ground: bool,
) -> LmtResult<ExportPoseObjResult> {
    let _ = parse_target(target)?; // 校验 target；几何原样（Neutral）
    let report: CabinetPoseReportFile =
        serde_json::from_slice(&std::fs::read(pose_report_path)?)?;
    if report.cabinet_poses.is_empty() {
        return Err(LmtError::InvalidInput(
            "pose report has no cabinet_poses".into(),
        ));
    }

    // 网格维度（同时校验每个 cabinet_id 可解析）。
    let ids: Vec<&str> = report
        .cabinet_poses
        .iter()
        .map(|c| c.cabinet_id.as_str())
        .collect();
    let (cols, rows) = infer_grid_dims(&ids)?;

    // 可选 re-root：从基准 cabinet 推 world→local 变换。
    let frame = match root {
        None => None,
        Some(rid) => {
            let rc = report
                .cabinet_poses
                .iter()
                .find(|c| c.cabinet_id == rid)
                .ok_or_else(|| {
                    LmtError::NotFound(format!("--root cabinet '{rid}' not in pose report"))
                })?;
            Some(CabinetFrame::from_corners(&rc.corners_mm).ok_or_else(|| {
                LmtError::InvalidInput(format!(
                    "--root cabinet '{rid}' has degenerate corners (zero-area or collinear)"
                ))
            })?)
        }
    };

    // 每块：(id, col, row, 角点[已应用 re-root])。
    let mut panels: Vec<(String, u32, u32, [[f64; 3]; 4])> =
        Vec::with_capacity(report.cabinet_poses.len());
    for cab in &report.cabinet_poses {
        let (col, row) = parse_cabinet_col_row(&cab.cabinet_id).ok_or_else(|| {
            LmtError::InvalidInput(format!(
                "cabinet_id {:?} not parseable as V<col>_R<row>",
                cab.cabinet_id
            ))
        })?;
        let mut cs = cab.corners_mm;
        if let Some(f) = &frame {
            for c in cs.iter_mut() {
                *c = f.world_to_local(c);
            }
        }
        panels.push((cab.cabinet_id.clone(), col, row, cs));
    }

    // 可选 ground：Y 平移使底边到 0（基准=root 块，否则整体）。
    if ground {
        let min_y = panels
            .iter()
            .filter(|(id, _, _, _)| root.map_or(true, |r| id == r))
            .flat_map(|(_, _, _, cs)| cs.iter().map(|c| c[1]))
            .fold(f64::INFINITY, f64::min);
        if min_y.is_finite() {
            for (_, _, _, cs) in panels.iter_mut() {
                for c in cs.iter_mut() {
                    c[1] -= min_y;
                }
            }
        }
    }

    // 每块 → 1×1 surface（格子 UV）→ MeshOutput（Neutral 原样，weld 0）；再合并。
    let unit_array = CabinetArray::rectangle(1, 1, [1.0, 1.0]);
    let mut meshes = Vec::with_capacity(panels.len());
    for (cid, col, row, cs) in &panels {
        let surface = panel_surface(cid, cs, *col, *row, cols, rows);
        meshes.push(surface_to_mesh_output(
            &surface,
            &unit_array,
            TargetSoftware::Neutral,
            0.0,
        )?);
    }
    let combined = merge_mesh_outputs(TargetSoftware::Neutral, &meshes);

    let out = ensure_obj_extension(out_file);
    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    write_obj(&combined, &out)?;

    Ok(ExportPoseObjResult {
        target: target.to_string(),
        cabinet_count: panels.len(),
        file: out.display().to_string(),
    })
}

/// Dry-run pre-flight for [`run_export_pose_obj`]: the pose report must be
/// readable, non-empty, and (if `root` is given) contain that cabinet — so
/// `--dry-run` does not green-light an export that execute would reject.
pub fn check_pose_obj_inputs(pose_report_path: &Path, root: Option<&str>) -> LmtResult<()> {
    let report: CabinetPoseReportFile =
        serde_json::from_slice(&std::fs::read(pose_report_path)?)?;
    if report.cabinet_poses.is_empty() {
        return Err(LmtError::InvalidInput(
            "pose report has no cabinet_poses".into(),
        ));
    }
    // dry-run 与 execute 对齐：不可解析的 cabinet_id 现在就拒。
    let ids: Vec<&str> = report
        .cabinet_poses
        .iter()
        .map(|c| c.cabinet_id.as_str())
        .collect();
    infer_grid_dims(&ids)?;
    if let Some(rid) = root {
        if !report.cabinet_poses.iter().any(|c| c.cabinet_id == rid) {
            return Err(LmtError::NotFound(format!(
                "--root cabinet '{rid}' not in pose report"
            )));
        }
    }
    Ok(())
}

/// A cabinet's local frame derived from its 4 world corners (BL,BR,TR,TL).
/// Origin = panel center; +x = width (BL→BR); +z = outward (x × BL→TL); +y = z×x.
/// `world_to_local` maps a world point (mm) into this orthonormal RH frame.
struct CabinetFrame {
    center: Vector3<f64>,
    x: Vector3<f64>,
    y: Vector3<f64>,
    z: Vector3<f64>,
}

impl CabinetFrame {
    /// `None` if the panel is degenerate (coincident/collinear corners), where
    /// `normalize()` would yield non-finite components and poison the geometry.
    fn from_corners(c: &[[f64; 3]; 4]) -> Option<Self> {
        let v = |i: usize| Vector3::new(c[i][0], c[i][1], c[i][2]);
        let (bl, br, tl) = (v(0), v(1), v(3));
        let center = (v(0) + v(1) + v(2) + v(3)) / 4.0;
        let x = (br - bl).normalize();
        let z = x.cross(&(tl - bl)).normalize();
        let y = z.cross(&x);
        let finite = |a: &Vector3<f64>| a.x.is_finite() && a.y.is_finite() && a.z.is_finite();
        if !(finite(&x) && finite(&y) && finite(&z)) {
            return None;
        }
        Some(Self { center, x, y, z })
    }
    fn world_to_local(&self, p: &[f64; 3]) -> [f64; 3] {
        let d = Vector3::new(p[0], p[1], p[2]) - self.center;
        [self.x.dot(&d), self.y.dot(&d), self.z.dot(&d)]
    }
}

/// 从 cabinet_id 解析末尾的 `V<col>_R<row>`（如 "V012_R007" → (12,7)）。
/// 容忍前缀（"MAIN_V012_R007" 也可）。不匹配返回 None。
fn parse_cabinet_col_row(cabinet_id: &str) -> Option<(u32, u32)> {
    let (head, row_str) = cabinet_id.rsplit_once("_R")?;
    let (_, col_str) = head.rsplit_once('V')?;
    Some((col_str.parse().ok()?, row_str.parse().ok()?))
}

/// 总列/行数 = max(col)+1 / max(row)+1。任一 id 不可解析 → InvalidInput。
/// 越界 index（≥ MAX_GRID_DIM）也拒：既防 `max+1` 溢出（pose report 是外部文件，
/// 可含 `V4294967295_R000` 这类极值），又与 GridTopology 的 cols/rows 上限一致。
fn infer_grid_dims(ids: &[&str]) -> LmtResult<(u32, u32)> {
    let mut max_col = 0u32;
    let mut max_row = 0u32;
    for id in ids {
        let (c, r) = parse_cabinet_col_row(id).ok_or_else(|| {
            LmtError::InvalidInput(format!("cabinet_id {id:?} not parseable as V<col>_R<row>"))
        })?;
        if c >= MAX_GRID_DIM || r >= MAX_GRID_DIM {
            return Err(LmtError::InvalidInput(format!(
                "cabinet_id {id:?} grid index out of range (must be < {MAX_GRID_DIM})"
            )));
        }
        max_col = max_col.max(c);
        max_row = max_row.max(r);
    }
    Ok((max_col + 1, max_row + 1))
}

/// 把每块 cabinet 的 MeshOutput 拼成一个（顶点不去重=不焊接，三角面索引按累计偏移）。
fn merge_mesh_outputs(target: TargetSoftware, meshes: &[MeshOutput]) -> MeshOutput {
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    let mut uv_coords = Vec::new();
    for m in meshes {
        let offset = vertices.len() as u32;
        vertices.extend_from_slice(&m.vertices);
        uv_coords.extend_from_slice(&m.uv_coords);
        for t in &m.triangles {
            triangles.push([t[0] + offset, t[1] + offset, t[2] + offset]);
        }
    }
    MeshOutput { target, vertices, triangles, uv_coords }
}

/// 墙中心列在位箱体的平均发光面外法向,取归一化水平分量。
/// 中心列 = round((cols-1)/2);该列空则取最近非空列。Path A 弧中点 θ_mid 的稳健类比:
/// ≥180° 包角墙(全墙平均法向会抵消)用中心列仍能定出明确前向。
/// 水平分量近 0(墙面朝上/下等病态)→ None。
fn center_column_forward(
    panels: &[(String, u32, u32, [[f64; 3]; 4])],
    cols: u32,
) -> Option<Vector3<f64>> {
    if panels.is_empty() || cols == 0 {
        return None;
    }
    let c_mid = (cols - 1) / 2;
    let present: std::collections::BTreeSet<u32> = panels.iter().map(|(_, c, _, _)| *c).collect();
    let target_col = if present.contains(&c_mid) {
        c_mid
    } else {
        *present
            .iter()
            .min_by_key(|&&c| (c as i64 - c_mid as i64).abs())?
    };
    let mut sum = Vector3::zeros();
    let mut n = 0u32;
    for (_, c, _, cs) in panels.iter() {
        if *c == target_col {
            if let Some(f) = CabinetFrame::from_corners(cs) {
                sum += f.z;
                n += 1;
            }
        }
    }
    if n == 0 {
        return None;
    }
    let avg = sum / n as f64;
    let n_h = Vector3::new(avg.x, 0.0, avg.z);
    if n_h.norm() < 1e-6 {
        return None;
    }
    Some(n_h.normalize())
}

/// 一块 cabinet 的 4 个世界系角点（mm，BL,BR,TR,TL）→ 1×1 ReconstructedSurface（米，原样）。
/// 顶点行主序 [(0,0),(1,0),(0,1),(1,1)]=[BL,BR,TL,TR]，故把 [BL,BR,TR,TL] 重排为索引 0,1,3,2。
/// UV：把 1×1 单位 UV 重映射到本块在整屏 (cols×rows) 网格里的格子。
fn panel_surface(
    cabinet_id: &str,
    corners_mm: &[[f64; 3]; 4],
    col: u32,
    row: u32,
    cols: u32,
    rows: u32,
) -> ReconstructedSurface {
    let m = |i: usize| {
        Vector3::new(
            corners_mm[i][0] / 1000.0,
            corners_mm[i][1] / 1000.0,
            corners_mm[i][2] / 1000.0,
        )
    };
    let topology = GridTopology { cols: 1, rows: 1 };
    let uv_coords = compute_grid_uv(topology)
        .into_iter()
        .map(|uv| {
            nalgebra::Vector2::new(
                (col as f64 + uv.x) / cols as f64,
                (row as f64 + uv.y) / rows as f64,
            )
        })
        .collect();
    ReconstructedSurface {
        screen_id: cabinet_id.to_string(),
        uv_coords,
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

    fn panel(col: u32, row: u32, corners: [[f64; 3]; 4]) -> (String, u32, u32, [[f64; 3]; 4]) {
        (format!("V{col:03}_R{row:03}"), col, row, corners)
    }
    /// 一块面向给定水平法向、竖直站立的箱体角点(BL,BR,TR,TL,mm)。
    fn facing_panel(col: u32, row: u32, nx: f64, nz: f64) -> (String, u32, u32, [[f64; 3]; 4]) {
        // right = up(+Y) × normal;normal=(nx,0,nz)
        let n = Vector3::new(nx, 0.0, nz).normalize();
        let up = Vector3::new(0.0, 1.0, 0.0);
        let right = up.cross(&n).normalize(); // 沿底边方向
        let (hw, hh) = (300.0, 170.0);
        let c = Vector3::new(col as f64 * 700.0, 0.0, 0.0); // 任意横向铺开
        let bl = c - right * hw - up * hh;
        let br = c + right * hw - up * hh;
        let tr = c + right * hw + up * hh;
        let tl = c - right * hw + up * hh;
        let v = |p: Vector3<f64>| [p.x, p.y, p.z];
        (format!("V{col:03}_R{row:03}"), col, row, [v(bl), v(br), v(tr), v(tl)])
    }

    #[test]
    fn center_column_forward_flat_wall() {
        // 3 列平墙全朝 +X → 中心列(col1)前向 ≈ +X
        let panels = vec![
            facing_panel(0, 0, 1.0, 0.0),
            facing_panel(1, 0, 1.0, 0.0),
            facing_panel(2, 0, 1.0, 0.0),
        ];
        let f = center_column_forward(&panels, 3).unwrap();
        assert!((f - Vector3::new(1.0, 0.0, 0.0)).norm() < 1e-6, "got {f:?}");
    }

    #[test]
    fn center_column_forward_handles_wraparound() {
        // 模拟 ≥180° 包角:法向铺满,平均会接近 0,但中心列(col1)朝 +Z 明确
        let panels = vec![
            facing_panel(0, 0, -1.0, 0.0),
            facing_panel(1, 0, 0.0, 1.0), // 中心列朝 +Z
            facing_panel(2, 0, 1.0, 0.0),
        ];
        let f = center_column_forward(&panels, 3).unwrap();
        assert!((f - Vector3::new(0.0, 0.0, 1.0)).norm() < 1e-6, "got {f:?}");
    }

    #[test]
    fn center_column_forward_degenerate_returns_none() {
        // 墙面朝上(法向≈+Y)→ 水平分量≈0 → None
        let flat_up = [[-300.0, 0.0, -170.0], [300.0, 0.0, -170.0], [300.0, 0.0, 170.0], [-300.0, 0.0, 170.0]];
        let panels = vec![panel(0, 0, flat_up)];
        assert!(center_column_forward(&panels, 1).is_none());
    }

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
    fn export_pose_obj_writes_single_merged_obj_with_grid_uv() {
        let dir = tempdir().unwrap();
        let rp = dir.path().join("BENCH_cabinet_pose_report.json");
        std::fs::write(&rp, BENCH_REPORT).unwrap();
        let out = dir.path().join("wall.obj");

        let res = run_export_pose_obj(&rp, "neutral", &out, None, false).unwrap();
        assert_eq!(res.cabinet_count, 2);
        assert!(out.is_file());

        let text = std::fs::read_to_string(&out).unwrap();
        // 2 块 × 4 顶点 = 8 个 v；2 块 × 2 三角 = 4 个 f
        assert_eq!(text.lines().filter(|l| l.starts_with("v ")).count(), 8);
        assert_eq!(text.lines().filter(|l| l.starts_with("f ")).count(), 4);
        // neutral = 原样世界坐标（米）→ V000_R000 的 BL (-0.3,-0.17,0)
        assert!(text.contains("v -0.3 -0.17 0"), "got:\n{text}");

        // UV 是整体网格：BENCH 两块=V000_R000/V000_R001 → cols=1,rows=2
        // 不同 U 值 = cols+1 = 2；不同 V 值 = rows+1 = 3
        let us: std::collections::BTreeSet<String> = text
            .lines()
            .filter_map(|l| l.strip_prefix("vt "))
            .map(|l| l.split_whitespace().next().unwrap().to_string())
            .collect();
        let vs: std::collections::BTreeSet<String> = text
            .lines()
            .filter_map(|l| l.strip_prefix("vt "))
            .map(|l| l.split_whitespace().nth(1).unwrap().to_string())
            .collect();
        assert_eq!(us.len(), 2, "distinct U should be cols+1=2: {us:?}");
        assert_eq!(vs.len(), 3, "distinct V should be rows+1=3: {vs:?}");
    }

    #[test]
    fn export_pose_obj_disguise_target_equals_raw_world_frame() {
        let dir = tempdir().unwrap();
        let rp = dir.path().join("BENCH_cabinet_pose_report.json");
        std::fs::write(&rp, BENCH_REPORT).unwrap();
        let out = dir.path().join("wall_disguise.obj");

        let res = run_export_pose_obj(&rp, "disguise", &out, None, false).unwrap();
        assert_eq!(res.target, "disguise");
        assert_eq!(res.cabinet_count, 2);

        let text = std::fs::read_to_string(&out).unwrap();
        assert!(
            text.contains("v -0.3 -0.17 0"),
            "disguise output should equal raw world frame; got:\n{text}"
        );
        assert!(
            !text.contains("v -0.3 0 0.17"),
            "axis-swapped vertex found — adapter was wrongly applied; got:\n{text}"
        );
    }

    #[test]
    fn export_pose_obj_rejects_unparseable_cabinet_id() {
        // 单文件下 cabinet_id 不再是文件名；不可解析为 V<col>_R<row> 的 id → InvalidInput。
        for bad_id in &["../escape", "a/b", "", "V000", "RandomName"] {
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
            let out = dir.path().join("wall.obj");

            let result = run_export_pose_obj(&rp, "neutral", &out, None, false);
            assert!(
                matches!(result, Err(LmtError::InvalidInput(_))),
                "expected InvalidInput for cabinet_id={bad_id:?}, got {result:?}"
            );
        }
    }

    #[test]
    fn export_pose_obj_root_makes_reference_axis_aligned_and_grounded() {
        let dir = tempdir().unwrap();
        let rp = dir.path().join("BENCH_cabinet_pose_report.json");
        std::fs::write(&rp, BENCH_REPORT).unwrap();
        let out = dir.path().join("wall.obj");

        let res = run_export_pose_obj(&rp, "neutral", &out, Some("V000_R001"), true).unwrap();
        assert_eq!(res.cabinet_count, 2);

        let text = std::fs::read_to_string(&out).unwrap();
        let verts: Vec<[f64; 3]> = text
            .lines()
            .filter_map(|l| l.strip_prefix("v "))
            .map(|l| {
                let n: Vec<f64> = l.split_whitespace().map(|t| t.parse().unwrap()).collect();
                [n[0], n[1], n[2]]
            })
            .collect();
        assert_eq!(verts.len(), 8, "2 cabinets × 4 verts");
        // 基准块（V000_R001）re-root 后落在 XY 平面 → 取 z≈0 的 4 个顶点
        let refp: Vec<[f64; 3]> = verts.into_iter().filter(|v| v[2].abs() < 1e-3).collect();
        assert_eq!(refp.len(), 4, "reference panel should be the 4 z≈0 verts: {refp:?}");
        // ground：基准块底边 y=0
        let min_y = refp.iter().map(|v| v[1]).fold(f64::INFINITY, f64::min);
        assert!(min_y.abs() < 1e-3, "ground: ref min y should be 0, got {min_y}");
        // 高 ≈ 0.680 m
        let max_y = refp.iter().map(|v| v[1]).fold(f64::NEG_INFINITY, f64::max);
        assert!((max_y - 0.680).abs() < 0.01, "height ≈ 0.68m, got {max_y}");

        // 未知 --root → NotFound
        let err = run_export_pose_obj(&rp, "neutral", &out, Some("V999_R999"), false).unwrap_err();
        assert!(matches!(err, LmtError::NotFound(_)), "got {err:?}");
    }

    #[test]
    fn parse_cabinet_col_row_extracts_indices() {
        assert_eq!(parse_cabinet_col_row("V000_R000"), Some((0, 0)));
        assert_eq!(parse_cabinet_col_row("V012_R007"), Some((12, 7)));
        assert_eq!(parse_cabinet_col_row("V120_R024"), Some((120, 24)));
        // 不匹配 → None
        assert_eq!(parse_cabinet_col_row("../escape"), None);
        assert_eq!(parse_cabinet_col_row("a/b"), None);
        assert_eq!(parse_cabinet_col_row(""), None);
        assert_eq!(parse_cabinet_col_row("V000"), None);
    }

    #[test]
    fn infer_grid_dims_takes_max_plus_one() {
        let ids = ["V000_R000", "V000_R001"];
        assert_eq!(infer_grid_dims(&ids).unwrap(), (1, 2));
        let ids = ["V000_R000", "V002_R000", "V001_R001"];
        assert_eq!(infer_grid_dims(&ids).unwrap(), (3, 2));
        let ids = ["V000_R000", "bad"];
        assert!(matches!(infer_grid_dims(&ids), Err(LmtError::InvalidInput(_))));
    }

    #[test]
    fn infer_grid_dims_handles_absent_cells() {
        // 缺 V001_R000：dims 仍按 max+1 推（2 列 × 2 行）
        let ids = ["V000_R000", "V000_R001", "V001_R001"];
        assert_eq!(infer_grid_dims(&ids).unwrap(), (2, 2));
    }

    #[test]
    fn infer_grid_dims_rejects_out_of_range_index() {
        // u32::MAX 可解析但 max+1 会溢出 → 必须先拒（外部 pose report 是系统边界）。
        assert!(matches!(
            infer_grid_dims(&["V4294967295_R000"]),
            Err(LmtError::InvalidInput(_))
        ));
        // index == MAX_GRID_DIM 越界（合法上限是 < MAX_GRID_DIM）。
        assert!(matches!(
            infer_grid_dims(&["V000_R10000"]),
            Err(LmtError::InvalidInput(_))
        ));
    }

    #[test]
    fn merge_mesh_outputs_concatenates_and_offsets_indices() {
        use lmt_core::surface::MeshOutput;
        let mk = |x: f64| MeshOutput {
            target: TargetSoftware::Neutral,
            vertices: vec![
                Vector3::new(x, 0.0, 0.0),
                Vector3::new(x + 1.0, 0.0, 0.0),
                Vector3::new(x, 1.0, 0.0),
                Vector3::new(x + 1.0, 1.0, 0.0),
            ],
            triangles: vec![[0, 1, 3], [0, 3, 2]],
            uv_coords: vec![
                nalgebra::Vector2::new(0.0, 0.0),
                nalgebra::Vector2::new(1.0, 0.0),
                nalgebra::Vector2::new(0.0, 1.0),
                nalgebra::Vector2::new(1.0, 1.0),
            ],
        };
        let merged = merge_mesh_outputs(TargetSoftware::Neutral, &[mk(0.0), mk(10.0)]);
        assert_eq!(merged.vertices.len(), 8);
        assert_eq!(merged.uv_coords.len(), 8);
        assert_eq!(merged.triangles.len(), 4);
        assert_eq!(merged.triangles[2], [4, 5, 7]);
        assert_eq!(merged.triangles[3], [4, 7, 6]);
        assert_eq!(merged.vertices[4].x, 10.0);
    }
}
