//! M1 全站仪 CSV adapter 的 Tauri 入口。
//!
//! Pure helpers + thin `#[tauri::command]` wrappers。Helpers 受集成测试覆盖。

use std::path::Path;

use lmt_adapter_total_station::{
    builder::build_screen_measured_points_with_outcome,
    csv_parser::parse_csv,
    instruction_card::{html::generate_html, pdf::generate_pdf, InstructionCard},
    report_builder::build_screen_report,
};

use crate::commands::projects::load_project_yaml_from_path;
use crate::commands::total_station_mapper::map_to_adapter;
use crate::dto::{InstructionCardResult, TotalStationImportResult};
use crate::error::{LmtError, LmtResult};

/// 把 `csv_path` 的 Trimble CSV 转成 `{project}/measurements/measured.yaml`，
/// 同时写 `import_report.json`，返回 GUI 友好的 summary。
///
/// 已有 measured.yaml 会被备份成 `measured.yaml.bak`；写入失败时回滚。
pub fn run_import(
    project_abs_path: &Path,
    screen_id: &str,
    csv_path: &Path,
) -> LmtResult<TotalStationImportResult> {
    // 1. 读 GUI project.yaml，映射到 M1 ProjectConfig
    let gui_cfg = load_project_yaml_from_path(project_abs_path)?;
    let m1_cfg = map_to_adapter(&gui_cfg)?;
    let screen_cfg = m1_cfg
        .screens
        .get(screen_id)
        .ok_or_else(|| LmtError::NotFound(format!("screen '{screen_id}' not in project")))?;

    // 2. 解析 CSV
    let raw = parse_csv(csv_path)?;

    // 3. 跑 build + report（report 签名是 4 参数：screen_id, &mp, &outcome, &cfg）
    let (measured, outcome) =
        build_screen_measured_points_with_outcome(screen_id, &raw, screen_cfg)?;
    let report = build_screen_report(screen_id, &measured, &outcome, screen_cfg);

    // 4. 写文件（带 backup + rollback + cross-screen 防御）
    let measurements_dir = project_abs_path.join("measurements");
    std::fs::create_dir_all(&measurements_dir)?;
    let measured_yaml_path = measurements_dir.join("measured.yaml");
    let report_json_path = measurements_dir.join("import_report.json");
    let backup_path = measurements_dir.join("measured.yaml.bak");

    // 4a. 若已有 measured.yaml，检查它的 screen_id 跟本次导入是否匹配。
    //     M1.1 单 screen scope；多 screen 项目走同一 measured.yaml，但本次 import
    //     若覆盖的是另一个 screen 的测量数据，拒绝（避免无声毁掉别人的工作）。
    if measured_yaml_path.exists() {
        if let Some(existing_screen) = read_existing_screen_id(&measured_yaml_path) {
            if existing_screen != screen_id {
                return Err(LmtError::InvalidInput(format!(
                    "refusing to overwrite measured.yaml for screen {existing_screen:?} \
                     with an import targeting screen {screen_id:?}; remove the existing \
                     file first or import to the correct screen"
                )));
            }
        }
    }

    // 4b. 若已有 measured.yaml，rename 成 .bak（覆盖上一次的 .bak）。
    //     保留 .bak 作为上一版本快照——不在成功后删除，给用户一份 recovery copy。
    let did_backup = if measured_yaml_path.exists() {
        std::fs::rename(&measured_yaml_path, &backup_path)?;
        true
    } else {
        false
    };

    // 4c. 写新文件。任一步失败：删除可能落地的新 measured.yaml，再 restore .bak。
    let write_result = (|| -> LmtResult<()> {
        let yaml = serde_yaml::to_string(&measured)?;
        let tmp = measurements_dir.join("measured.yaml.tmp");
        std::fs::write(&tmp, yaml)?;
        std::fs::rename(&tmp, &measured_yaml_path)?;

        let report_json = serde_json::to_string_pretty(&report)?;
        let tmp = measurements_dir.join("import_report.json.tmp");
        std::fs::write(&tmp, report_json)?;
        std::fs::rename(&tmp, &report_json_path)?;
        Ok(())
    })();

    if let Err(e) = write_result {
        // Remove the half-written new file before restoring, otherwise rename(.bak, target)
        // can fail on platforms where rename refuses to overwrite (Windows).
        let _ = std::fs::remove_file(&measured_yaml_path);
        if did_backup {
            let _ = std::fs::rename(&backup_path, &measured_yaml_path);
        }
        return Err(e);
    }
    // Success: leave .bak in place as a versioned snapshot. The next successful
    // import will overwrite it with the now-current state.

    // 5. 返回 summary
    Ok(TotalStationImportResult {
        measurements_yaml_path: "measurements/measured.yaml".to_string(),
        report_json_path: "measurements/import_report.json".to_string(),
        measured_count: report.measured_count,
        fabricated_count: report.fabricated_count,
        outlier_count: report.outliers.len(),
        missing_count: report.missing.len(),
        warnings: report.warnings.clone(),
    })
}

/// Lightweight YAML scan for the top-level `screen_id:` field. Avoids deserializing
/// the entire `MeasuredPoints` blob when all we want is the screen ID.
/// Returns `None` if the file is unreadable or missing the field.
fn read_existing_screen_id(path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("screen_id:") {
            let value = rest.trim().trim_matches('"').trim_matches('\'').to_string();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

#[tauri::command]
pub fn import_total_station_csv(
    project_abs_path: String,
    csv_path: String,
    screen_id: String,
) -> LmtResult<TotalStationImportResult> {
    run_import(
        Path::new(&project_abs_path),
        &screen_id,
        Path::new(&csv_path),
    )
}

/// Build the `InstructionCard` payload from a GUI project.yaml.
/// Shared by HTML-render and PDF-save paths.
fn build_card(project_abs_path: &Path, screen_id: &str) -> LmtResult<InstructionCard> {
    let gui_cfg = load_project_yaml_from_path(project_abs_path)?;
    let m1_cfg = map_to_adapter(&gui_cfg)?;
    let screen_cfg = m1_cfg
        .screens
        .get(screen_id)
        .ok_or_else(|| LmtError::NotFound(format!("screen '{screen_id}' not in project")))?
        .clone();

    Ok(InstructionCard {
        project_name: m1_cfg.project.name.clone(),
        screen_id: screen_id.to_string(),
        cfg: screen_cfg,
        origin_grid_name: m1_cfg.coordinate_system.origin_grid_name.clone(),
        x_axis_grid_name: m1_cfg.coordinate_system.x_axis_grid_name.clone(),
        xy_plane_grid_name: m1_cfg.coordinate_system.xy_plane_grid_name.clone(),
    })
}

/// Render the instruction card HTML (for iframe preview). Does NOT write a PDF.
/// PDF is written separately via [`run_save_pdf`] at a user-chosen path.
pub fn run_generate_card(
    project_abs_path: &Path,
    screen_id: &str,
) -> LmtResult<InstructionCardResult> {
    let card = build_card(project_abs_path, screen_id)?;
    Ok(InstructionCardResult {
        html_content: generate_html(&card),
    })
}

/// Append `.pdf` if the path doesn't already end with that extension
/// (case-insensitive). Users who skip the dialog's filter and type
/// `report` should still get a usable PDF file.
fn ensure_pdf_extension(p: &Path) -> std::path::PathBuf {
    match p.extension() {
        Some(ext) if ext.eq_ignore_ascii_case("pdf") => p.to_path_buf(),
        _ => {
            let mut buf = p.as_os_str().to_os_string();
            buf.push(".pdf");
            std::path::PathBuf::from(buf)
        }
    }
}

/// Render the instruction card PDF to a user-chosen absolute path.
/// Atomic: writes to a sibling `<dst>.<pid>.tmp` first, then renames;
/// cleans up the tmp on failure. Returns the final destination on success.
pub fn run_save_pdf(
    project_abs_path: &Path,
    screen_id: &str,
    dst_pdf_path: &Path,
) -> LmtResult<String> {
    if dst_pdf_path.as_os_str().is_empty() {
        return Err(LmtError::InvalidInput(
            "destination PDF path must not be empty".into(),
        ));
    }

    // Build card BEFORE touching the filesystem, so a bad project.yaml /
    // missing screen doesn't leave empty parent dirs behind.
    let card = build_card(project_abs_path, screen_id)?;

    let dst = ensure_pdf_extension(dst_pdf_path);
    if let Some(parent) = dst.parent() {
        if !parent.as_os_str().is_empty() && !parent.is_dir() {
            std::fs::create_dir_all(parent)?;
        }
    }

    // Sibling tmp with PID suffix — never collides with an unrelated
    // `*.pdf.tmp` the user might happen to have, and stays on the same
    // filesystem so the final rename is atomic on POSIX.
    let mut tmp_os = dst.as_os_str().to_os_string();
    tmp_os.push(format!(".{}.tmp", std::process::id()));
    let tmp = std::path::PathBuf::from(tmp_os);

    if let Err(e) = generate_pdf(&card, &tmp) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    if let Err(e) = std::fs::rename(&tmp, &dst) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(dst.display().to_string())
}

#[tauri::command]
pub fn generate_instruction_card(
    project_abs_path: String,
    screen_id: String,
) -> LmtResult<InstructionCardResult> {
    run_generate_card(Path::new(&project_abs_path), &screen_id)
}

#[tauri::command]
pub fn save_instruction_pdf(
    project_abs_path: String,
    screen_id: String,
    dst_pdf_path: String,
) -> LmtResult<String> {
    run_save_pdf(
        Path::new(&project_abs_path),
        &screen_id,
        Path::new(&dst_pdf_path),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    /// 写一份最小化合法 project.yaml（4×2 cabinet，flat）+ 15 点 CSV。
    /// 4×2 cabinet → 5×3 vertices = 15 个点，全测无 fabricate。
    fn seed_project(dir: &Path) {
        let project_yaml = r#"
project:
  name: TS_Test
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
        fs::write(dir.join("project.yaml"), project_yaml).unwrap();
        fs::create_dir_all(dir.join("measurements")).unwrap();
    }

    fn write_csv(path: &Path) {
        // 第 1-3 个点是 reference (origin / x-axis / xy-plane)，后面 12 个填满 grid
        let csv = "\
name,x,y,z,note
1,0,0,0,origin
2,2000,0,0,x-axis
3,0,0,1000,xy-plane
4,500,0,0,
5,1000,0,0,
6,1500,0,0,
7,0,0,500,
8,500,0,500,
9,1000,0,500,
10,1500,0,500,
11,2000,0,500,
12,500,0,1000,
13,1000,0,1000,
14,1500,0,1000,
15,2000,0,1000,
";
        fs::write(path, csv).unwrap();
    }

    #[test]
    fn import_writes_measured_yaml_and_report() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        seed_project(project);
        let csv = project.join("measurements").join("raw.csv");
        write_csv(&csv);

        let result = run_import(project, "MAIN", &csv).unwrap();

        assert_eq!(result.measurements_yaml_path, "measurements/measured.yaml");
        assert_eq!(result.report_json_path, "measurements/import_report.json");
        assert_eq!(result.measured_count, 15);
        assert_eq!(result.fabricated_count, 0);
        assert_eq!(result.outlier_count, 0);
        assert_eq!(result.missing_count, 0);
        assert!(project.join("measurements/measured.yaml").is_file());
        assert!(project.join("measurements/import_report.json").is_file());
    }

    #[test]
    fn import_fails_when_project_yaml_missing() {
        let dir = tempdir().unwrap();
        let csv = dir.path().join("raw.csv");
        write_csv(&csv);
        let err = run_import(dir.path(), "MAIN", &csv).unwrap_err();
        assert!(format!("{err}").contains("project.yaml"), "got: {err}");
    }

    #[test]
    fn import_propagates_csv_parse_error() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        seed_project(project);
        let csv = project.join("raw.csv");
        fs::write(&csv, "garbage,not,a,csv\n").unwrap();
        let err = run_import(project, "MAIN", &csv).unwrap_err();
        let s = format!("{err}").to_lowercase();
        assert!(
            s.contains("instrument") || s.contains("csv") || s.contains("invalid"),
            "got: {err}"
        );
    }

    #[test]
    fn import_fails_for_unknown_screen() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        seed_project(project);
        let csv = project.join("measurements").join("raw.csv");
        write_csv(&csv);
        let err = run_import(project, "FLOOR", &csv).unwrap_err();
        assert!(format!("{err}").contains("FLOOR"), "got: {err}");
    }

    #[test]
    fn second_import_preserves_backup_as_versioned_snapshot() {
        // Successful re-import keeps .bak around as the previous version,
        // giving the user a recovery copy of whatever was overwritten.
        let dir = tempdir().unwrap();
        let project = dir.path();
        seed_project(project);
        let csv = project.join("measurements").join("raw.csv");
        write_csv(&csv);

        run_import(project, "MAIN", &csv).unwrap();
        let first_content = fs::read_to_string(project.join("measurements/measured.yaml")).unwrap();

        run_import(project, "MAIN", &csv).unwrap();
        assert!(project.join("measurements/measured.yaml").is_file());
        let bak = project.join("measurements/measured.yaml.bak");
        assert!(
            bak.is_file(),
            "backup must survive as previous-version snapshot"
        );
        let bak_content = fs::read_to_string(&bak).unwrap();
        assert_eq!(
            bak_content, first_content,
            ".bak should be the prior measured.yaml"
        );
    }

    #[test]
    fn import_refuses_to_overwrite_different_screens_measurements() {
        // Seed measured.yaml as if it belongs to a different screen (FLOOR),
        // then attempt to import for MAIN. Should error without touching the file.
        let dir = tempdir().unwrap();
        let project = dir.path();
        seed_project(project);
        let csv = project.join("measurements").join("raw.csv");
        write_csv(&csv);

        let stale =
            "screen_id: FLOOR\ncoordinate_frame:\n  origin_world: [0.0, 0.0, 0.0]\npoints: []\n";
        fs::write(project.join("measurements/measured.yaml"), stale).unwrap();

        let err = run_import(project, "MAIN", &csv).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("FLOOR"), "got: {err}");
        assert!(msg.contains("MAIN"), "got: {err}");

        // Existing file must be untouched (no .bak should have been created).
        let still = fs::read_to_string(project.join("measurements/measured.yaml")).unwrap();
        assert_eq!(still, stale, "file must not be overwritten on refusal");
        assert!(
            !project.join("measurements/measured.yaml.bak").is_file(),
            "no backup should have been created when import was refused"
        );
    }

    #[test]
    fn generate_card_returns_html_no_pdf_side_effect() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        seed_project(project);

        let result = run_generate_card(project, "MAIN").unwrap();
        assert!(
            result.html_content.contains("TS_Test"),
            "html: {}",
            result.html_content
        );
        assert!(result.html_content.contains("MAIN"));

        // No PDF / output dir side-effect — that's save_instruction_pdf's job.
        let output = project.join("output");
        if output.exists() {
            let entries: Vec<_> = fs::read_dir(&output).unwrap().flatten().collect();
            assert!(
                entries.is_empty(),
                "generate_card must not write output/ artifacts; found {entries:?}"
            );
        }
    }

    #[test]
    fn generate_card_fails_for_unknown_screen() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        seed_project(project);
        let err = run_generate_card(project, "FLOOR").unwrap_err();
        assert!(format!("{err}").contains("FLOOR"), "got: {err}");
    }

    #[test]
    fn save_pdf_writes_to_chosen_path() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        seed_project(project);

        let dst = dir.path().join("custom-name.pdf");
        let out = run_save_pdf(project, "MAIN", &dst).unwrap();
        assert_eq!(out, dst.display().to_string());

        let pdf = fs::read(&dst).unwrap();
        assert!(pdf.starts_with(b"%PDF-"), "missing PDF magic header");
        let tmp = dst.with_extension("pdf.tmp");
        assert!(!tmp.exists(), "leftover .tmp file");
    }

    #[test]
    fn save_pdf_creates_missing_parent_dir() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        seed_project(project);

        let dst = dir.path().join("deeply/nested/output/card.pdf");
        run_save_pdf(project, "MAIN", &dst).unwrap();
        assert!(dst.is_file());
    }

    #[test]
    fn save_pdf_fails_for_unknown_screen() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        seed_project(project);

        let dst = dir.path().join("x.pdf");
        let err = run_save_pdf(project, "FLOOR", &dst).unwrap_err();
        assert!(format!("{err}").contains("FLOOR"), "got: {err}");
        assert!(!dst.exists(), "no PDF should be written on failure");
    }

    #[test]
    fn save_pdf_rejects_empty_dst() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        seed_project(project);

        let dst = std::path::PathBuf::new();
        let err = run_save_pdf(project, "MAIN", &dst).unwrap_err();
        assert!(
            format!("{err}").to_lowercase().contains("empty"),
            "got: {err}"
        );
    }

    #[test]
    fn save_pdf_appends_missing_extension() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        seed_project(project);

        // User types "report" with no extension — backend should append .pdf.
        let dst_no_ext = dir.path().join("report");
        let written = run_save_pdf(project, "MAIN", &dst_no_ext).unwrap();
        assert!(written.ends_with(".pdf"), "got: {written}");
        let actual = dir.path().join("report.pdf");
        assert!(actual.is_file());
        let head = fs::read(&actual).unwrap();
        assert!(head.starts_with(b"%PDF-"));
    }

    #[test]
    fn save_pdf_does_not_create_dirs_on_invalid_project() {
        // Test that mkdir doesn't happen if build_card fails (e.g. unknown screen).
        let dir = tempdir().unwrap();
        let project = dir.path();
        seed_project(project);

        let dst = dir.path().join("would_be_created/deeper/x.pdf");
        let err = run_save_pdf(project, "FLOOR", &dst).unwrap_err();
        assert!(format!("{err}").contains("FLOOR"), "got: {err}");
        assert!(
            !dir.path().join("would_be_created").exists(),
            "parent dirs must not be created when build_card fails"
        );
    }

    #[test]
    fn save_pdf_overwrites_existing_atomically() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        seed_project(project);

        // Pre-populate the destination with sentinel bytes to verify the
        // second write actually replaces them (not just no-ops).
        let dst = dir.path().join("out.pdf");
        fs::write(&dst, b"OLD-CONTENT-NOT-A-PDF").unwrap();

        run_save_pdf(project, "MAIN", &dst).unwrap();
        let bytes = fs::read(&dst).unwrap();
        assert!(
            bytes.starts_with(b"%PDF-"),
            "stale content was kept: {:?}",
            &bytes[..8]
        );

        // Second write also goes through atomically; no .tmp left over.
        run_save_pdf(project, "MAIN", &dst).unwrap();
        assert!(dst.is_file());
        let pid_tmp =
            std::path::PathBuf::from(format!("{}.{}.tmp", dst.display(), std::process::id()));
        assert!(!pid_tmp.exists(), "leftover .tmp at {pid_tmp:?}");
    }

    #[test]
    fn rollback_on_write_failure_restores_previous_measured_yaml() {
        // Simulate a mid-import write failure by pre-creating import_report.json
        // as a directory — the rename target then collides, write_result fails,
        // and rollback must restore the original measured.yaml from .bak.
        let dir = tempdir().unwrap();
        let project = dir.path();
        seed_project(project);
        let csv = project.join("measurements").join("raw.csv");
        write_csv(&csv);

        // First successful import to seed measured.yaml.
        run_import(project, "MAIN", &csv).unwrap();
        let original = fs::read_to_string(project.join("measurements/measured.yaml")).unwrap();

        // Booby-trap import_report.json as a directory; rename(tmp → final) will fail.
        fs::remove_file(project.join("measurements/import_report.json")).unwrap();
        fs::create_dir(project.join("measurements/import_report.json")).unwrap();

        let err = run_import(project, "MAIN", &csv).unwrap_err();
        assert!(!format!("{err}").is_empty());

        // measured.yaml must still match the pre-import state.
        let restored = fs::read_to_string(project.join("measurements/measured.yaml")).unwrap();
        assert_eq!(
            restored, original,
            "rollback must restore previous measured.yaml content"
        );
    }
}
