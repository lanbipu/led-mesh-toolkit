//! Test the PoC compare logic: read two MeasuredPoints sets, compute holdout RMS.

use std::process::Command;

#[test]
fn poc_compare_emits_holdout_rms_in_c_mode() {
    let out = Command::new(env!("CARGO_BIN_EXE_lmt-poc-compare"))
        .args([
            "--ground-truth", "tests/fixtures/poc_gt.json",
            "--measured", "tests/fixtures/poc_visual_c.json",
            "--frame-strategy", "three_points",
            "--anchor-ids", "MAIN_V000_R000_AR0,MAIN_V001_R000_AR64,MAIN_V000_R001_AR128",
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(report.get("holdout_rms_mm").is_some());
    assert!(report.get("holdout_p95_mm").is_some());
    assert!(report.get("anchor_residual_rms_mm").is_some());
    assert_eq!(report["frame_strategy"], "three_points");
}

#[test]
fn poc_compare_a_mode_uses_all_points() {
    let out = Command::new(env!("CARGO_BIN_EXE_lmt-poc-compare"))
        .args([
            "--ground-truth", "tests/fixtures/poc_gt.json",
            "--measured", "tests/fixtures/poc_visual_a.json",
            "--frame-strategy", "nominal_anchoring",
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(report.get("rms_mm").is_some());
    assert!(report.get("p95_mm").is_some());
}
