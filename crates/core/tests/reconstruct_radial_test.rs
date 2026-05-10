use lmt_core::coordinate::CoordinateFrame;
use lmt_core::measured_points::MeasuredPoints;
use lmt_core::point::{MeasuredPoint, PointSource};
use lmt_core::reconstruct::radial_basis::RadialBasisReconstructor;
use lmt_core::reconstruct::Reconstructor;
use lmt_core::shape::{CabinetArray, ShapePrior};
use lmt_core::uncertainty::Uncertainty;
use nalgebra::Vector3;

fn p(name: &str, x: f64, y: f64, z: f64) -> MeasuredPoint {
    MeasuredPoint {
        name: name.into(),
        position: Vector3::new(x, y, z),
        uncertainty: Uncertainty::Isotropic(2.0),
        source: PointSource::TotalStation,
    }
}

fn frame() -> CoordinateFrame {
    CoordinateFrame::from_three_points(
        Vector3::zeros(),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
    )
    .unwrap()
}

#[test]
fn radial_basis_reproduces_anchor_points_exactly() {
    // Sparse: 4 corners + 1 middle, in a 4×4 cabinet grid (5×5 vertices = 25)
    let mp = MeasuredPoints {
        screen_id: "MAIN".into(),
        coordinate_frame: frame(),
        cabinet_array: CabinetArray::rectangle(4, 4, [500.0, 500.0]),
        shape_prior: ShapePrior::Flat,
        points: vec![
            p("MAIN_V001_R001", 0.0, 0.0, 0.0),
            p("MAIN_V005_R001", 2.0, 0.0, 0.0),
            p("MAIN_V001_R005", 0.0, 0.0, 2.0),
            p("MAIN_V005_R005", 2.0, 0.0, 2.0),
            p("MAIN_V003_R003", 1.0, 0.0, 1.0),
        ],
    };

    let r = RadialBasisReconstructor;
    assert!(r.applicable(&mp));
    let surface = r.reconstruct(&mp).unwrap();
    assert_eq!(surface.vertices.len(), 25);

    // The anchor at (col=2, row=2) → (1, 0, 1) should be reproduced exactly
    let mid = surface.topology.vertex_index(2, 2);
    assert!((surface.vertices[mid] - Vector3::new(1.0, 0.0, 1.0)).norm() < 1e-3);
}

#[test]
fn radial_basis_with_only_4_corners_is_not_applicable() {
    // 4 corners alone is mathematically equivalent to bilinear; the
    // dispatcher should fall through to NominalReconstructor instead
    // of running RBF (which would shadow nominal forever).
    let mp = MeasuredPoints {
        screen_id: "MAIN".into(),
        coordinate_frame: frame(),
        cabinet_array: CabinetArray::rectangle(4, 4, [500.0, 500.0]),
        shape_prior: ShapePrior::Flat,
        points: vec![
            p("MAIN_V001_R001", 0.0, 0.0, 0.0),
            p("MAIN_V005_R001", 2.0, 0.0, 0.0),
            p("MAIN_V001_R005", 0.0, 0.0, 2.0),
            p("MAIN_V005_R005", 2.0, 0.0, 2.0),
        ],
    };
    let r = RadialBasisReconstructor;
    assert!(!r.applicable(&mp));
}

#[test]
fn radial_basis_needs_more_than_4_points() {
    let mp = MeasuredPoints {
        screen_id: "MAIN".into(),
        coordinate_frame: frame(),
        cabinet_array: CabinetArray::rectangle(4, 4, [500.0, 500.0]),
        shape_prior: ShapePrior::Flat,
        points: vec![p("MAIN_V001_R001", 0.0, 0.0, 0.0)],
    };
    let r = RadialBasisReconstructor;
    assert!(!r.applicable(&mp));
}

#[test]
fn radial_basis_rejects_clustered_anchors_without_corners() {
    // 5 anchors all clustered in middle, no corners → cannot extrapolate edges.
    let mp = MeasuredPoints {
        screen_id: "MAIN".into(),
        coordinate_frame: frame(),
        cabinet_array: CabinetArray::rectangle(4, 4, [500.0, 500.0]),
        shape_prior: ShapePrior::Flat,
        points: vec![
            p("MAIN_V002_R002", 0.5, 0.0, 0.5),
            p("MAIN_V003_R002", 1.0, 0.0, 0.5),
            p("MAIN_V004_R002", 1.5, 0.0, 0.5),
            p("MAIN_V002_R003", 0.5, 0.0, 1.0),
            p("MAIN_V003_R003", 1.0, 0.0, 1.0),
        ],
    };
    let r = RadialBasisReconstructor;
    assert!(
        !r.applicable(&mp),
        "should reject inputs without all 4 corners"
    );
}

#[test]
fn radial_basis_ignores_out_of_grid_anchor_names() {
    // 4 corners + 1 out-of-grid stray → only 4 in-grid unique anchors → not applicable.
    let mp = MeasuredPoints {
        screen_id: "MAIN".into(),
        coordinate_frame: frame(),
        cabinet_array: CabinetArray::rectangle(4, 4, [500.0, 500.0]),
        shape_prior: ShapePrior::Flat,
        points: vec![
            p("MAIN_V001_R001", 0.0, 0.0, 0.0),
            p("MAIN_V005_R001", 2.0, 0.0, 0.0),
            p("MAIN_V001_R005", 0.0, 0.0, 2.0),
            p("MAIN_V005_R005", 2.0, 0.0, 2.0),
            p("MAIN_V999_R999", 100.0, 0.0, 100.0), // out of grid
        ],
    };
    let r = RadialBasisReconstructor;
    assert!(
        !r.applicable(&mp),
        "out-of-grid stray must not count as anchor"
    );
}

#[test]
fn radial_basis_dedupes_repeated_anchor_names() {
    // 4 corners + same interior name twice = 5 raw, but only 4 unique → not applicable.
    let mp = MeasuredPoints {
        screen_id: "MAIN".into(),
        coordinate_frame: frame(),
        cabinet_array: CabinetArray::rectangle(4, 4, [500.0, 500.0]),
        shape_prior: ShapePrior::Flat,
        points: vec![
            p("MAIN_V001_R001", 0.0, 0.0, 0.0),
            p("MAIN_V005_R001", 2.0, 0.0, 0.0),
            p("MAIN_V001_R005", 0.0, 0.0, 2.0),
            p("MAIN_V005_R005", 2.0, 0.0, 2.0),
            p("MAIN_V001_R001", 0.0, 0.0, 0.0), // duplicate of corner
        ],
    };
    let r = RadialBasisReconstructor;
    assert!(
        !r.applicable(&mp),
        "duplicate names must not inflate anchor count"
    );
}
