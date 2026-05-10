use lmt_tauri_lib::commands::reconstruct::{list_runs_for, read_run_report, run_reconstruction};
use lmt_tauri_lib::data::{open_in_memory, schema};
use std::path::PathBuf;
use tempfile::TempDir;

fn cp_dir(s: &std::path::Path, d: &std::path::Path) {
    std::fs::create_dir_all(d).unwrap();
    for e in std::fs::read_dir(s).unwrap() {
        let e = e.unwrap();
        let to = d.join(e.file_name());
        if e.path().is_dir() {
            cp_dir(&e.path(), &to);
        } else {
            std::fs::copy(e.path(), &to).unwrap();
        }
    }
}

#[test]
fn list_after_two_runs() {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/curved-flat");
    let proj = TempDir::new().unwrap();
    cp_dir(&src, proj.path());

    let db = open_in_memory().unwrap();
    {
        let mut conn = db.lock().unwrap();
        schema::migrate(&mut conn).unwrap();
    }

    let r1 = run_reconstruction(
        db.clone(),
        proj.path(),
        "MAIN",
        "measurements/measured.yaml",
    )
    .expect("first reconstruction ok");

    let r2 = run_reconstruction(
        db.clone(),
        proj.path(),
        "MAIN",
        "measurements/measured.yaml",
    )
    .expect("second reconstruction ok");

    let listed = list_runs_for(
        db.clone(),
        &proj.path().display().to_string(),
        Some("MAIN"),
    )
    .expect("list_runs_for ok");

    assert_eq!(listed.len(), 2, "expected 2 runs, got {}", listed.len());

    let report = read_run_report(db.clone(), r1.run_id).expect("read_run_report ok");
    assert!(
        report["surface"]["vertices"].is_array(),
        "expected surface.vertices to be an array"
    );

    // make sure all-project listing (no screen filter) also returns 2
    let all = list_runs_for(db.clone(), &proj.path().display().to_string(), None)
        .expect("list_runs_for (no filter) ok");
    assert_eq!(all.len(), 2);

    let _ = r2;
}
