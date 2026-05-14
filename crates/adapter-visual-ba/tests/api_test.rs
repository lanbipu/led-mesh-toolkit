use std::env;
use std::path::PathBuf;
use std::sync::Mutex;

use lmt_adapter_visual_ba::api::{reconstruct, ReconstructArgs};
use lmt_adapter_visual_ba::ipc::{
    CabinetArray, CoordinateFrame, FlatTag, FrameStrategy, Intrinsics, PatternMeta,
    ReconstructProject, ShapePrior,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn mock_path_with_result() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dir.join("tests/fixtures/mock_sidecar_with_point.sh")
}

#[tokio::test]
async fn reconstruct_returns_ir_measured_points() {
    let _guard = ENV_LOCK.lock().unwrap();
    env::set_var(
        "LMT_VBA_SIDECAR_PATH",
        mock_path_with_result().to_str().unwrap(),
    );
    let project = ReconstructProject {
        screen_id: "MAIN".into(),
        coordinate_frame: CoordinateFrame {
            origin_world: [0.0; 3],
            basis: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        },
        cabinet_array: CabinetArray {
            cols: 1,
            rows: 1,
            cabinet_size_mm: [500.0, 500.0],
            absent_cells: vec![],
        },
        shape_prior: ShapePrior::Flat(FlatTag::Flat),
        frame_strategy: FrameStrategy::NominalAnchoring,
        frame_anchors: None,
    };
    let intrinsics = Intrinsics {
        k: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        dist_coeffs: vec![0.0; 5],
        image_size: [1920, 1080],
    };
    let pattern_meta = PatternMeta {
        aruco_dict: "DICT_6X6_1000".into(),
        markers_per_cabinet: 64,
        checkerboard_inner_corners: 8,
        cabinets: vec![],
    };
    let args = ReconstructArgs {
        project,
        images: vec!["a.jpg".into()],
        intrinsics,
        pattern_meta,
        progress_tx: None,
        cancel: None,
    };
    let measured_points = reconstruct(args).await.unwrap();
    assert_eq!(measured_points.points.len(), 1);
    assert_eq!(measured_points.points[0].name, "MAIN_V000_R000");
    // covariance was 1e-6 m² → after into_ir conversion, 1.0 mm²
    match &measured_points.points[0].uncertainty {
        lmt_core::uncertainty::Uncertainty::Covariance3x3(m) => {
            assert!((m[(0, 0)] - 1.0).abs() < 1e-6);
        }
        _ => panic!("expected covariance"),
    }
    env::remove_var("LMT_VBA_SIDECAR_PATH");
}

#[tokio::test]
async fn invalid_basis_returns_error_not_panic() {
    use lmt_adapter_visual_ba::error::VbaError;
    let _guard = ENV_LOCK.lock().unwrap();
    env::set_var(
        "LMT_VBA_SIDECAR_PATH",
        mock_path_with_result().to_str().unwrap(),
    );
    let project = ReconstructProject {
        screen_id: "MAIN".into(),
        // Non-orthonormal basis: core IR validator must reject.
        coordinate_frame: CoordinateFrame {
            origin_world: [0.0; 3],
            basis: [[1.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
        },
        cabinet_array: CabinetArray {
            cols: 1,
            rows: 1,
            cabinet_size_mm: [500.0, 500.0],
            absent_cells: vec![],
        },
        shape_prior: ShapePrior::Flat(FlatTag::Flat),
        frame_strategy: FrameStrategy::NominalAnchoring,
        frame_anchors: None,
    };
    let intrinsics = Intrinsics {
        k: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        dist_coeffs: vec![0.0; 5],
        image_size: [1920, 1080],
    };
    let pattern_meta = PatternMeta {
        aruco_dict: "DICT_6X6_1000".into(),
        markers_per_cabinet: 64,
        checkerboard_inner_corners: 8,
        cabinets: vec![],
    };
    let args = ReconstructArgs {
        project,
        images: vec!["a.jpg".into()],
        intrinsics,
        pattern_meta,
        progress_tx: None,
        cancel: None,
    };
    let result = reconstruct(args).await;
    env::remove_var("LMT_VBA_SIDECAR_PATH");
    match result {
        Err(VbaError::InvalidInput(_)) => {}
        other => panic!("expected InvalidInput, got {other:?}"),
    }
}
