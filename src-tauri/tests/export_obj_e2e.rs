use lmt_tauri_lib::commands::export::run_export;
use lmt_tauri_lib::commands::reconstruct::run_reconstruction;
use lmt_tauri_lib::data::{open_in_memory, schema};
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

    let obj_path = run_export(db.clone(), r.run_id, "disguise")
        .expect("export should succeed");

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
