use lmt_tauri_lib::commands::projects::seed_example_to_dir;
use tempfile::TempDir;

#[test]
fn seeds_curved_flat_into_target_dir() {
    let src = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples");
    let dst = TempDir::new().unwrap();
    let out = seed_example_to_dir(&src, "curved-flat", dst.path()).unwrap();
    assert!(out.join("project.yaml").exists());
    assert!(out.join("measurements/measured.yaml").exists());
}

#[test]
fn rejects_unknown_example() {
    let src = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples");
    let dst = TempDir::new().unwrap();
    let err = seed_example_to_dir(&src, "nonexistent", dst.path()).unwrap_err();
    let msg = format!("{err:?}");
    assert!(
        msg.contains("not_found") || msg.contains("NotFound"),
        "got: {msg}"
    );
}
