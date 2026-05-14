//! M1 全站仪 CSV adapter 的 Tauri 入口。
//!
//! Pure helpers + thin `#[tauri::command]` wrappers。Helpers 受集成测试覆盖。

use std::path::Path;

use lmt_adapter_total_station::{
    builder::build_screen_measured_points_with_outcome, csv_parser::parse_csv,
    report_builder::build_screen_report,
};

use crate::commands::projects::load_project_yaml_from_path;
use crate::commands::total_station_mapper::map_to_adapter;
use crate::dto::TotalStationImportResult;
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

    // 4. 写文件（带 backup + rollback）
    let measurements_dir = project_abs_path.join("measurements");
    std::fs::create_dir_all(&measurements_dir)?;
    let measured_yaml_path = measurements_dir.join("measured.yaml");
    let report_json_path = measurements_dir.join("import_report.json");
    let backup_path = measurements_dir.join("measured.yaml.bak");

    // 4a. 若已有 measured.yaml，先备份
    let did_backup = if measured_yaml_path.exists() {
        std::fs::rename(&measured_yaml_path, &backup_path)?;
        true
    } else {
        false
    };

    // 4b. 写新文件（写失败就 restore backup）
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
        if did_backup {
            let _ = std::fs::rename(&backup_path, &measured_yaml_path);
        }
        return Err(e);
    }

    // 4c. 都成功 → 清理 backup
    if did_backup {
        let _ = std::fs::remove_file(&backup_path);
    }

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

#[tauri::command]
pub fn import_total_station_csv(
    project_abs_path: String,
    csv_path: String,
    screen_id: String,
) -> LmtResult<TotalStationImportResult> {
    run_import(Path::new(&project_abs_path), &screen_id, Path::new(&csv_path))
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
    fn second_import_cleans_up_backup_on_success() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        seed_project(project);
        let csv = project.join("measurements").join("raw.csv");
        write_csv(&csv);

        run_import(project, "MAIN", &csv).unwrap();
        assert!(project.join("measurements/measured.yaml").is_file());

        run_import(project, "MAIN", &csv).unwrap();
        assert!(project.join("measurements/measured.yaml").is_file());
        assert!(
            !project.join("measurements/measured.yaml.bak").is_file(),
            "backup should be removed after successful re-import"
        );
    }
}
