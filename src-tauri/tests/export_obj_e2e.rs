use lmt_tauri_lib::commands::export::run_export;
use lmt_tauri_lib::commands::reconstruct::{list_runs_for, run_reconstruction};
use lmt_tauri_lib::data::{open_in_memory, schema};
use std::io::Write;
use std::path::PathBuf;
use tempfile::TempDir;

fn copy_example(name: &str, dst: &std::path::Path) {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../examples")
        .join(name);
    fn cp(s: &std::path::Path, d: &std::path::Path) {
        std::fs::create_dir_all(d).unwrap();
        for e in std::fs::read_dir(s).unwrap() {
            let e = e.unwrap();
            let to = d.join(e.file_name());
            if e.path().is_dir() {
                cp(&e.path(), &to);
            } else {
                std::fs::copy(e.path(), &to).unwrap();
            }
        }
    }
    cp(&src, dst);
}

#[test]
fn reconstruct_then_export_writes_obj() {
    let proj = TempDir::new().unwrap();
    copy_example("curved-flat", proj.path());

    let db = open_in_memory().unwrap();
    {
        let mut c = db.lock().unwrap();
        schema::migrate(&mut c).unwrap();
    }

    let r = run_reconstruction(
        db.clone(),
        proj.path(),
        "MAIN",
        "measurements/measured.yaml",
    )
    .expect("reconstruction should succeed");

    let obj_path =
        run_export(db.clone(), r.run_id, "disguise", None).expect("export should succeed");

    assert!(
        std::path::Path::new(&obj_path).is_file(),
        "OBJ file not found at {obj_path}"
    );

    let content = std::fs::read_to_string(&obj_path).unwrap();
    assert!(
        content.contains("v "),
        "OBJ should contain vertex lines (v ...)"
    );
    assert!(
        content.contains("vt "),
        "OBJ should contain texture-coord lines (vt ...)"
    );
    assert!(
        content.contains("f "),
        "OBJ should contain face lines (f ...)"
    );

    println!("obj_path: {obj_path}");
    println!("obj size: {} bytes", content.len());
    println!(
        "v lines: {}",
        content.lines().filter(|l| l.starts_with("v ")).count()
    );
    println!(
        "f lines: {}",
        content.lines().filter(|l| l.starts_with("f ")).count()
    );
}

#[test]
fn two_runs_same_target_no_overwrite() {
    let proj = TempDir::new().unwrap();
    copy_example("curved-flat", proj.path());

    let db = open_in_memory().unwrap();
    {
        let mut c = db.lock().unwrap();
        schema::migrate(&mut c).unwrap();
    }

    let r1 = run_reconstruction(
        db.clone(),
        proj.path(),
        "MAIN",
        "measurements/measured.yaml",
    )
    .expect("first reconstruction should succeed");

    let r2 = run_reconstruction(
        db.clone(),
        proj.path(),
        "MAIN",
        "measurements/measured.yaml",
    )
    .expect("second reconstruction should succeed");

    let path_1 =
        run_export(db.clone(), r1.run_id, "disguise", None).expect("first export should succeed");
    let path_2 =
        run_export(db.clone(), r2.run_id, "disguise", None).expect("second export should succeed");

    assert_ne!(path_1, path_2, "two runs must produce different OBJ paths");
    assert!(
        std::path::Path::new(&path_1).is_file(),
        "first run OBJ must still exist on disk: {path_1}"
    );
    assert!(
        std::path::Path::new(&path_2).is_file(),
        "second run OBJ must still exist on disk: {path_2}"
    );

    // Verify DB rows point to their respective paths
    let runs = list_runs_for(db.clone(), proj.path().to_str().unwrap(), Some("MAIN"))
        .expect("list_runs_for should succeed");

    let row_1 = runs
        .iter()
        .find(|r| r.id == r1.run_id)
        .expect("run 1 should appear in listing");
    let row_2 = runs
        .iter()
        .find(|r| r.id == r2.run_id)
        .expect("run 2 should appear in listing");

    assert!(
        row_1
            .output_obj_path
            .as_deref()
            .map(|p| path_1.ends_with(p))
            .unwrap_or(false),
        "DB row for run1 should point to path_1, got: {:?}",
        row_1.output_obj_path
    );
    assert!(
        row_2
            .output_obj_path
            .as_deref()
            .map(|p| path_2.ends_with(p))
            .unwrap_or(false),
        "DB row for run2 should point to path_2, got: {:?}",
        row_2.output_obj_path
    );
}

#[test]
fn export_uses_snapshot_after_project_yaml_changed() {
    let proj = TempDir::new().unwrap();
    copy_example("curved-flat", proj.path());

    let db = open_in_memory().unwrap();
    {
        let mut c = db.lock().unwrap();
        schema::migrate(&mut c).unwrap();
    }

    // Reconstruct once — snapshot captures original 8×4 cabinet_array.
    let r1 = run_reconstruction(
        db.clone(),
        proj.path(),
        "MAIN",
        "measurements/measured.yaml",
    )
    .expect("reconstruction should succeed");

    // Now mutate project.yaml: change cabinet_count from [8, 4] to [12, 6].
    let yaml_path = proj.path().join("project.yaml");
    let original_yaml = std::fs::read_to_string(&yaml_path).unwrap();
    let mutated_yaml = original_yaml.replace("cabinet_count: [8, 4]", "cabinet_count: [12, 6]");
    assert_ne!(
        original_yaml, mutated_yaml,
        "yaml mutation must have taken effect"
    );
    {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&yaml_path)
            .unwrap();
        f.write_all(mutated_yaml.as_bytes()).unwrap();
    }

    // Export must succeed using the snapshot (8×4), not the mutated yaml (12×6).
    let obj_path = run_export(db.clone(), r1.run_id, "disguise", None)
        .expect("export should succeed using snapshotted cabinet_array");

    let path = std::path::Path::new(&obj_path);
    assert!(path.is_file(), "OBJ file should exist at {obj_path}");

    let content = std::fs::read_to_string(path).unwrap();
    assert!(content.contains("v "), "OBJ should have vertex lines");
    assert!(content.contains("f "), "OBJ should have face lines");
}

#[test]
fn export_with_explicit_dst_path_writes_to_chosen_location() {
    // User-chosen path INSIDE project root → DB records project-relative path.
    let proj = TempDir::new().unwrap();
    copy_example("curved-flat", proj.path());

    let db = open_in_memory().unwrap();
    {
        let mut c = db.lock().unwrap();
        schema::migrate(&mut c).unwrap();
    }

    let r = run_reconstruction(
        db.clone(),
        proj.path(),
        "MAIN",
        "measurements/measured.yaml",
    )
    .unwrap();

    let chosen = proj.path().join("custom-folder").join("my-mesh.obj");
    let written = run_export(db.clone(), r.run_id, "disguise", Some(&chosen)).unwrap();
    assert_eq!(written, chosen.display().to_string());
    assert!(chosen.is_file(), "OBJ should be at chosen path");

    // DB should record a project-relative path (since chosen is under project root).
    let runs = list_runs_for(db.clone(), &proj.path().display().to_string(), Some("MAIN")).unwrap();
    let row = runs.iter().find(|r2| r2.id == r.run_id).unwrap();
    assert_eq!(
        row.output_obj_path.as_deref(),
        Some("custom-folder/my-mesh.obj"),
        "DB should record project-relative path; got {:?}",
        row.output_obj_path
    );
}

#[test]
fn export_appends_obj_extension_if_missing() {
    let proj = TempDir::new().unwrap();
    copy_example("curved-flat", proj.path());

    let db = open_in_memory().unwrap();
    {
        let mut c = db.lock().unwrap();
        schema::migrate(&mut c).unwrap();
    }

    let r = run_reconstruction(
        db.clone(),
        proj.path(),
        "MAIN",
        "measurements/measured.yaml",
    )
    .unwrap();

    // User typed "mymesh" with no extension.
    let chosen = proj.path().join("mymesh");
    let written = run_export(db.clone(), r.run_id, "disguise", Some(&chosen)).unwrap();
    assert!(written.ends_with(".obj"), "got: {written}");
    assert!(
        proj.path().join("mymesh.obj").is_file(),
        "OBJ at extended path missing"
    );
}

#[test]
fn export_to_path_outside_project_records_absolute() {
    // User picks a path on Desktop / external disk — DB records absolute path,
    // which is M1.1 documented behavior (revisit when project archive ships).
    let proj = TempDir::new().unwrap();
    copy_example("curved-flat", proj.path());

    let elsewhere = TempDir::new().unwrap();
    let chosen = elsewhere.path().join("external.obj");

    let db = open_in_memory().unwrap();
    {
        let mut c = db.lock().unwrap();
        schema::migrate(&mut c).unwrap();
    }
    let r = run_reconstruction(
        db.clone(),
        proj.path(),
        "MAIN",
        "measurements/measured.yaml",
    )
    .unwrap();

    run_export(db.clone(), r.run_id, "disguise", Some(&chosen)).unwrap();
    assert!(chosen.is_file());

    let runs = list_runs_for(db.clone(), &proj.path().display().to_string(), Some("MAIN")).unwrap();
    let row = runs.iter().find(|r2| r2.id == r.run_id).unwrap();
    let stored = row.output_obj_path.as_deref().unwrap_or("");
    assert!(
        std::path::Path::new(stored).is_absolute(),
        "expected absolute DB path for out-of-project export; got {stored:?}"
    );
}
