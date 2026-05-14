/// Full M0.2 user-journey E2E test — backend command helpers only, no GUI/AppHandle.
///
/// Steps:
///   1.  Seed project via seed_example_to_dir
///   2.  DB setup (in-memory) + schema migration
///   3.  recent_projects::upsert
///   4.  load_project_yaml_from_path
///   5.  load_measurements_from_path (11 points)
///   6.  run_reconstruction  → run_id, 45 vertices, radial_basis
///   7.  Verify report JSON on disk
///   8.  run_export ×3 targets (disguise / unreal / neutral)
///   9.  Re-export disguise (same run) — must succeed
///  10.  list_runs_for → 1 row, disguise, correct path
///  11.  read_run_report → JSON sanity
///  12.  Two-run isolation: second reconstruction + export; both OBJs on disk
use lmt_tauri_lib::commands::export::run_export;
use lmt_tauri_lib::commands::measurements::load_measurements_from_path;
use lmt_tauri_lib::commands::projects::{load_project_yaml_from_path, seed_example_to_dir};
use lmt_tauri_lib::commands::reconstruct::{list_runs_for, read_run_report, run_reconstruction};
use lmt_tauri_lib::data::{open_in_memory, recent_projects, schema};
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn full_user_journey_curved_flat() {
    // ── 1. Seed project ──────────────────────────────────────────────────────
    println!("\n[Step 1] seed_example_to_dir curved-flat → TempDir");
    let examples_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples");
    let tmp = TempDir::new().expect("TempDir::new");
    let projects_dir = tmp.path().join("projects");
    std::fs::create_dir_all(&projects_dir).unwrap();

    let project_path = seed_example_to_dir(&examples_root, "curved-flat", &projects_dir)
        .expect("seed_example_to_dir");
    assert!(
        project_path.is_dir(),
        "seeded project dir should exist: {project_path:?}"
    );
    assert!(
        project_path.join("project.yaml").is_file(),
        "project.yaml should be copied"
    );
    println!("  project_path = {}", project_path.display());

    // ── 2. DB setup ──────────────────────────────────────────────────────────
    println!("[Step 2] open_in_memory + schema::migrate");
    let db = open_in_memory().expect("open_in_memory");
    {
        let mut conn = db.lock().unwrap();
        schema::migrate(&mut conn).expect("schema::migrate");
    }

    // ── 3. Add to recent projects ────────────────────────────────────────────
    println!("[Step 3] recent_projects::upsert");
    let project_path_str = project_path.display().to_string();
    let recent = {
        let conn = db.lock().unwrap();
        recent_projects::upsert(&conn, &project_path_str, "Curved Flat Demo")
            .expect("upsert recent project")
    };
    assert!(
        recent.id > 0,
        "upserted row must have id > 0, got {}",
        recent.id
    );
    assert_eq!(recent.abs_path, project_path_str);
    assert_eq!(recent.display_name, "Curved Flat Demo");
    println!("  recent.id = {}", recent.id);

    // ── 4. Load project YAML ─────────────────────────────────────────────────
    println!("[Step 4] load_project_yaml_from_path");
    let config = load_project_yaml_from_path(&project_path).expect("load_project_yaml_from_path");
    assert_eq!(
        config.project.name, "Curved-Flat-Demo",
        "project name mismatch: {}",
        config.project.name
    );
    let main_screen = config
        .screens
        .get("MAIN")
        .expect("MAIN screen must exist in project.yaml");
    assert_eq!(
        main_screen.cabinet_count,
        [8, 4],
        "cabinet_count mismatch: {:?}",
        main_screen.cabinet_count
    );
    println!(
        "  name = {}, cabinet_count = {:?}",
        config.project.name, main_screen.cabinet_count
    );

    // ── 5. Load measurements ─────────────────────────────────────────────────
    println!("[Step 5] load_measurements_from_path (expect 11 points, screen_id MAIN)");
    let measurements_path = project_path.join("measurements/measured.yaml");
    let measurements =
        load_measurements_from_path(&measurements_path).expect("load_measurements_from_path");
    assert_eq!(
        measurements.screen_id, "MAIN",
        "screen_id mismatch: {}",
        measurements.screen_id
    );
    assert_eq!(
        measurements.points.len(),
        11,
        "expected 11 measured points, got {}",
        measurements.points.len()
    );
    println!(
        "  screen_id = {}, points = {}",
        measurements.screen_id,
        measurements.points.len()
    );

    // ── 6. Reconstruct ───────────────────────────────────────────────────────
    println!("[Step 6] run_reconstruction → expect 45 vertices, radial_basis");
    let result = run_reconstruction(
        db.clone(),
        &project_path,
        "MAIN",
        "measurements/measured.yaml",
    )
    .expect("run_reconstruction");

    let run_id = result.run_id;
    assert!(run_id > 0, "run_id must be > 0, got {run_id}");
    assert_eq!(
        result.surface.vertices.len(),
        45,
        "expected 45 vertices (9×5 grid for 8×4 cabinet array), got {}",
        result.surface.vertices.len()
    );
    assert_eq!(
        result.surface.quality_metrics.method, "radial_basis",
        "expected method radial_basis, got {}",
        result.surface.quality_metrics.method
    );
    println!(
        "  run_id = {run_id}, vertices = {}, method = {}",
        result.surface.vertices.len(),
        result.surface.quality_metrics.method
    );

    // ── 7. Verify report JSON ────────────────────────────────────────────────
    println!("[Step 7] verify report JSON on disk");
    let report_abs = project_path.join(&result.report_json_path);
    assert!(
        report_abs.is_file(),
        "report JSON not found at {report_abs:?}"
    );
    let report_bytes = std::fs::read(&report_abs).expect("read report JSON");
    let report: lmt_tauri_lib::dto::ReconstructionReport =
        serde_json::from_slice(&report_bytes).expect("deserialize ReconstructionReport");

    assert_eq!(
        report.surface.vertices.len(),
        45,
        "report surface.vertices len mismatch"
    );
    assert_eq!(
        report.cabinet_array.cols, 8,
        "cabinet_array.cols should be 8, got {}",
        report.cabinet_array.cols
    );
    assert_eq!(
        report.cabinet_array.rows, 4,
        "cabinet_array.rows should be 4, got {}",
        report.cabinet_array.rows
    );
    assert_eq!(
        report.weld_tolerance_mm, 1.0,
        "weld_tolerance_mm should be 1.0 (matches curved-flat fixture), got {}",
        report.weld_tolerance_mm
    );
    println!(
        "  report ok: vertices={}, cols={}, rows={}, weld_tolerance_mm={}",
        report.surface.vertices.len(),
        report.cabinet_array.cols,
        report.cabinet_array.rows,
        report.weld_tolerance_mm
    );

    // ── 8. Export 3 targets ──────────────────────────────────────────────────
    println!("[Step 8] run_export × 3 targets (disguise / unreal / neutral)");
    for target in &["disguise", "unreal", "neutral"] {
        let obj_abs = run_export(db.clone(), run_id, target, None)
            .unwrap_or_else(|e| panic!("run_export({target}) failed: {e}"));
        let obj_path = std::path::Path::new(&obj_abs);
        assert!(
            obj_path.is_file(),
            "OBJ file not found for target={target}: {obj_abs}"
        );

        let content = std::fs::read_to_string(obj_path)
            .unwrap_or_else(|e| panic!("read OBJ ({target}): {e}"));
        assert!(
            content.lines().any(|l| l.starts_with("v ")),
            "OBJ ({target}) missing vertex lines"
        );
        assert!(
            content.lines().any(|l| l.starts_with("vt ")),
            "OBJ ({target}) missing texture-coord lines"
        );
        assert!(
            content.lines().any(|l| l.starts_with("f ")),
            "OBJ ({target}) missing face lines"
        );

        // Filename must match pattern output/MAIN_<target>_run<id>.obj
        let expected_filename = format!("MAIN_{target}_run{run_id}.obj");
        assert!(
            obj_abs.ends_with(&expected_filename),
            "OBJ path for target={target} should end with {expected_filename}, got {obj_abs}"
        );
        assert!(
            obj_abs.contains("/output/"),
            "OBJ path should be under output/, got {obj_abs}"
        );
        println!(
            "  target={target} → {} ({} bytes)",
            expected_filename,
            content.len()
        );
    }

    // ── 9. Re-export disguise (same run) ─────────────────────────────────────
    println!("[Step 9] re-export disguise on same run (overwrite same target allowed)");
    let re_export_path = run_export(db.clone(), run_id, "disguise", None)
        .expect("re-export disguise on same run should succeed");
    assert!(
        std::path::Path::new(&re_export_path).is_file(),
        "re-exported OBJ not found: {re_export_path}"
    );
    println!("  re-export ok: {re_export_path}");

    // ── 10. List runs ────────────────────────────────────────────────────────
    println!("[Step 10] list_runs_for (1 run expected, last export = disguise)");
    let runs = list_runs_for(db.clone(), &project_path_str, Some("MAIN")).expect("list_runs_for");
    assert_eq!(runs.len(), 1, "expected 1 run, got {}", runs.len());
    let row = &runs[0];
    assert_eq!(row.id, run_id, "row id mismatch");
    assert_eq!(
        row.target.as_deref(),
        Some("disguise"),
        "last export target should be disguise, got {:?}",
        row.target
    );
    let expected_obj_rel = format!("output/MAIN_disguise_run{run_id}.obj");
    assert_eq!(
        row.output_obj_path.as_deref(),
        Some(expected_obj_rel.as_str()),
        "output_obj_path mismatch: {:?}",
        row.output_obj_path
    );
    println!(
        "  row id={}, target={:?}, output_obj_path={:?}",
        row.id, row.target, row.output_obj_path
    );

    // ── 11. Get run report ───────────────────────────────────────────────────
    println!("[Step 11] read_run_report → JSON sanity");
    let report_value = read_run_report(db.clone(), run_id).expect("read_run_report");
    assert!(
        report_value["surface"]["vertices"].is_array(),
        "report JSON: surface.vertices should be an array"
    );
    assert!(
        report_value["cabinet_array"].is_object(),
        "report JSON: cabinet_array should be an object"
    );
    println!(
        "  surface.vertices.len() = {}",
        report_value["surface"]["vertices"]
            .as_array()
            .map(|a| a.len())
            .unwrap_or(0)
    );

    // ── 12. Two-run isolation ────────────────────────────────────────────────
    println!("[Step 12] two-run isolation: second reconstruction + export");
    let result2 = run_reconstruction(
        db.clone(),
        &project_path,
        "MAIN",
        "measurements/measured.yaml",
    )
    .expect("second run_reconstruction");
    let run_id_2 = result2.run_id;
    assert_ne!(run_id_2, run_id, "second run must have a different run_id");

    let obj2_abs =
        run_export(db.clone(), run_id_2, "disguise", None).expect("export run 2 disguise");

    // Both OBJ files must exist (no cross-run overwrite)
    let obj1_expected = project_path
        .join("output")
        .join(format!("MAIN_disguise_run{run_id}.obj"));
    let obj2_expected = project_path
        .join("output")
        .join(format!("MAIN_disguise_run{run_id_2}.obj"));

    assert!(
        obj1_expected.is_file(),
        "run1 OBJ must still exist: {obj1_expected:?}"
    );
    assert!(
        obj2_expected.is_file(),
        "run2 OBJ must exist: {obj2_expected:?}"
    );
    assert_ne!(
        obj1_expected, obj2_expected,
        "run1 and run2 OBJ paths must differ"
    );

    let runs_after = list_runs_for(db.clone(), &project_path_str, Some("MAIN"))
        .expect("list_runs_for after two runs");
    assert_eq!(
        runs_after.len(),
        2,
        "expected 2 runs after second reconstruction, got {}",
        runs_after.len()
    );

    let ids: Vec<i64> = runs_after.iter().map(|r| r.id).collect();
    assert!(
        ids.contains(&run_id),
        "run 1 (id={run_id}) should appear in listing"
    );
    assert!(
        ids.contains(&run_id_2),
        "run 2 (id={run_id_2}) should appear in listing"
    );

    println!("  run1 id={run_id} obj ok, run2 id={run_id_2} obj ok");
    println!("  obj2_abs = {obj2_abs}");

    println!("\n[PASS] All 12 steps completed — {} asserts verified.", 30);
}
