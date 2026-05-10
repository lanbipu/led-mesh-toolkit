use lmt_tauri_lib::commands::reconstruct::run_reconstruction;
use lmt_tauri_lib::data::{open_in_memory, schema};
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn end_to_end_yaml_to_report() {
    // arrange: copy curved-flat into a temporary project directory
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/curved-flat");
    let proj = TempDir::new().unwrap();
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
    cp_dir(&src, proj.path());

    let db = open_in_memory().unwrap();
    {
        let mut conn = db.lock().unwrap();
        schema::migrate(&mut conn).unwrap();
    }

    let result = run_reconstruction(
        db.clone(),
        proj.path(),
        "MAIN",
        "measurements/measured.yaml",
    )
    .expect("reconstruct ok");

    assert!(result.run_id > 0);
    let report_path = proj.path().join(&result.report_json_path);
    assert!(report_path.is_file(), "report json missing at {report_path:?}");
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&report_path).unwrap()).unwrap();
    assert!(
        json["surface"]["vertices"].is_array(),
        "expected surface.vertices to be an array"
    );
    assert!(
        json["quality_metrics"]["method"].is_string(),
        "expected quality_metrics.method to be a string"
    );

    println!("run_id: {}", result.run_id);
    println!("report: {}", result.report_json_path);
    println!("method: {}", json["quality_metrics"]["method"]);
    println!("vertices: {}", json["surface"]["vertices"].as_array().unwrap().len());
}
