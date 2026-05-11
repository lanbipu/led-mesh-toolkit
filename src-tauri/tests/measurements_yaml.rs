use lmt_tauri_lib::commands::measurements::load_measurements_from_path;
use std::path::PathBuf;

#[test]
fn loads_curved_flat_fixture() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../examples/curved-flat/measurements/measured.yaml");
    let points = load_measurements_from_path(&path).unwrap();
    assert_eq!(points.points.len(), 11);
    assert_eq!(points.screen_id, "MAIN");
}
