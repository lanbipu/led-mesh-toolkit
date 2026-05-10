use lmt_core::coordinate::CoordinateFrame;
use lmt_core::measured_points::MeasuredPoints;
use lmt_core::point::{MeasuredPoint, PointSource};
use lmt_core::reconstruct::direct::DirectLinkReconstructor;
use lmt_core::reconstruct::Reconstructor;
use lmt_core::shape::{CabinetArray, ShapePrior};
use lmt_core::uncertainty::Uncertainty;
use nalgebra::Vector3;

fn full_grid_3x2() -> MeasuredPoints {
    let frame = CoordinateFrame::from_three_points(
        Vector3::zeros(),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
    )
    .unwrap();

    // 3 col × 2 row cabinets → 4 × 3 = 12 vertices needed
    let mut pts = vec![];
    for r in 1..=3 {
        for c in 1..=4 {
            let x = (c - 1) as f64 * 0.5;
            let z = (r - 1) as f64 * 0.5;
            pts.push(MeasuredPoint {
                name: format!("MAIN_V{:03}_R{:03}", c, r),
                position: Vector3::new(x, 0.0, z),
                uncertainty: Uncertainty::Isotropic(2.0),
                source: PointSource::TotalStation,
            });
        }
    }

    MeasuredPoints {
        screen_id: "MAIN".into(),
        coordinate_frame: frame,
        cabinet_array: CabinetArray::rectangle(3, 2, [500.0, 500.0]),
        shape_prior: ShapePrior::Flat,
        points: pts,
    }
}

#[test]
fn direct_link_with_full_grid_returns_exact_positions() {
    let mp = full_grid_3x2();
    let r = DirectLinkReconstructor;
    assert!(r.applicable(&mp));

    let surface = r.reconstruct(&mp).unwrap();
    assert_eq!(surface.vertices.len(), 12);

    // (0,0) corner → (0,0,0)
    let i = surface.topology.vertex_index(0, 0);
    assert!((surface.vertices[i] - Vector3::new(0.0, 0.0, 0.0)).norm() < 1e-9);
    // top-right (3,2) → (1.5, 0, 1.0)
    let i = surface.topology.vertex_index(3, 2);
    assert!((surface.vertices[i] - Vector3::new(1.5, 0.0, 1.0)).norm() < 1e-9);
}

#[test]
fn direct_link_with_one_missing_is_not_applicable() {
    let mut mp = full_grid_3x2();
    mp.points.retain(|p| p.name != "MAIN_V002_R002");
    let r = DirectLinkReconstructor;
    assert!(!r.applicable(&mp));
}

#[test]
fn direct_link_rejects_irregular_cabinet_array() {
    let mut mp = full_grid_3x2();
    mp.cabinet_array = CabinetArray::irregular(3, 2, [500.0, 500.0], vec![(1, 0)]);
    let r = DirectLinkReconstructor;
    assert!(!r.applicable(&mp), "irregular CabinetArray should not be applicable for DirectLink");
}

#[test]
fn direct_link_estimated_rms_reflects_input_uncertainty() {
    // Override uncertainty to 5mm on all points
    let mut mp = full_grid_3x2();
    for p in &mut mp.points {
        p.uncertainty = Uncertainty::Isotropic(5.0);
    }
    let r = DirectLinkReconstructor;
    let surface = r.reconstruct(&mp).unwrap();
    // 12 points each with sigma=5 → RMS = 5
    assert!((surface.quality_metrics.estimated_rms_mm - 5.0).abs() < 1e-9);
}
