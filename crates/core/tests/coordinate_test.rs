use lmt_core::coordinate::CoordinateFrame;
use nalgebra::Vector3;

#[test]
fn three_point_frame_is_orthonormal() {
    // Origin at (10,10,10) in raw frame
    // X axis ref at (12, 10, 10) → +X = (1, 0, 0)
    // XY plane ref at (10, 10, 13) → up = +Z, Y = +Z×+X = +Y? Wait, let's check.
    //
    // Cross: Z = (xy_ref - origin) × X
    //      = (0, 0, 3) × (1, 0, 0)
    //      = (0*0 - 3*0, 3*1 - 0*0, 0*0 - 0*1)
    //      = (0, 3, 0)
    // normalized → (0, 1, 0) = +Y
    // Y = Z × X = (0, 1, 0) × (1, 0, 0) = (0*0 - 0*0, 0*1 - 0*0, 0*0 - 1*1) = (0, 0, -1)
    //
    // So X=+X, "up"=+Y, last basis = -Z.
    let frame = CoordinateFrame::from_three_points(
        Vector3::new(10.0, 10.0, 10.0),
        Vector3::new(12.0, 10.0, 10.0),
        Vector3::new(10.0, 10.0, 13.0),
    )
    .unwrap();

    // Origin is moved to model (0,0,0)
    let origin_in_model = frame.world_to_model(&Vector3::new(10.0, 10.0, 10.0));
    assert!((origin_in_model.norm()) < 1e-9);

    // X-axis ref → (2, 0, 0) in model (distance preserved)
    let x_in_model = frame.world_to_model(&Vector3::new(12.0, 10.0, 10.0));
    assert!((x_in_model - Vector3::new(2.0, 0.0, 0.0)).norm() < 1e-9);
}

#[test]
fn collinear_three_points_returns_error() {
    let result = CoordinateFrame::from_three_points(
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(2.0, 0.0, 0.0), // on the X line
    );
    assert!(result.is_err());
}

#[test]
fn round_trip_world_model() {
    let frame = CoordinateFrame::from_three_points(
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
    )
    .unwrap();

    let world = Vector3::new(3.5, 1.2, -0.7);
    let model = frame.world_to_model(&world);
    let back = frame.model_to_world(&model);
    assert!((back - world).norm() < 1e-9);
}
