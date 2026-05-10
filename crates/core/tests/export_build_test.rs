use lmt_core::export::build::surface_to_mesh_output;
use lmt_core::shape::CabinetArray;
use lmt_core::surface::{GridTopology, MeshOutput, QualityMetrics, ReconstructedSurface, TargetSoftware};
use lmt_core::uv::compute_grid_uv;
use nalgebra::Vector3;

fn sample_2x1_surface() -> ReconstructedSurface {
    let topo = GridTopology { cols: 2, rows: 1 };
    let vertices = vec![
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(0.5, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 0.5),
        Vector3::new(0.5, 0.0, 0.5),
        Vector3::new(1.0, 0.0, 0.5),
    ];
    let uvs = compute_grid_uv(topo);
    ReconstructedSurface {
        screen_id: "MAIN".into(),
        topology: topo,
        vertices,
        uv_coords: uvs,
        quality_metrics: QualityMetrics::default(),
    }
}

fn rect_2x1() -> CabinetArray {
    CabinetArray::rectangle(2, 1, [500.0, 500.0])
}

#[test]
fn neutral_output_preserves_vertex_count() {
    let s = sample_2x1_surface();
    let cab = rect_2x1();
    let mo: MeshOutput = surface_to_mesh_output(
        &s, &cab, TargetSoftware::Neutral, 0.001
    ).unwrap();
    assert_eq!(mo.vertices.len(), 6);
    assert_eq!(mo.uv_coords.len(), 6);
    assert_eq!(mo.triangles.len(), 4);
    assert_eq!(mo.target, TargetSoftware::Neutral);
}

#[test]
fn welding_drops_duplicates() {
    let topo = GridTopology { cols: 1, rows: 1 };
    let v = vec![
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 0.0),    // duplicate of vertex 0
        Vector3::new(1.0, 0.0, 1.0),
    ];
    let uvs = compute_grid_uv(topo);
    let s = ReconstructedSurface {
        screen_id: "MAIN".into(),
        topology: topo,
        vertices: v,
        uv_coords: uvs,
        quality_metrics: QualityMetrics::default(),
    };
    let cab = CabinetArray::rectangle(1, 1, [500.0, 500.0]);
    let mo = surface_to_mesh_output(&s, &cab, TargetSoftware::Neutral, 0.001).unwrap();
    assert_eq!(mo.vertices.len(), 3);  // 1 dup welded
}

#[test]
fn surface_to_mesh_returns_invalid_input_on_bad_tolerance() {
    let s = sample_2x1_surface();
    let cab = rect_2x1();

    let nan_result = surface_to_mesh_output(&s, &cab, TargetSoftware::Neutral, f64::NAN);
    assert!(nan_result.is_err());

    let inf_result = surface_to_mesh_output(&s, &cab, TargetSoftware::Neutral, f64::INFINITY);
    assert!(inf_result.is_err());

    let neg_result = surface_to_mesh_output(&s, &cab, TargetSoftware::Neutral, -0.001);
    assert!(neg_result.is_err());
}

#[test]
fn surface_to_mesh_returns_invalid_input_on_dim_mismatch() {
    let s = sample_2x1_surface();
    // topology is 2x1 but cabinet_array is 3x1
    let bad_cab = CabinetArray::rectangle(3, 1, [500.0, 500.0]);
    let result = surface_to_mesh_output(&s, &bad_cab, TargetSoftware::Neutral, 0.001);
    assert!(result.is_err());
}

#[test]
fn surface_to_mesh_disguise_limit_rejected_before_allocation() {
    // Build a surface with vertex count > DISGUISE_VERTEX_LIMIT.
    // Use cols=500, rows=500 → 251_001 vertices. Limit check should fire
    // before triangulate/weld run.
    let topo = GridTopology { cols: 500, rows: 500 };
    let n = topo.vertex_count();
    let vertices = vec![Vector3::zeros(); n];
    let uvs = vec![nalgebra::Vector2::zeros(); n];
    let s = ReconstructedSurface {
        screen_id: "MAIN".into(),
        topology: topo,
        vertices,
        uv_coords: uvs,
        quality_metrics: QualityMetrics::default(),
    };
    let cab = CabinetArray::rectangle(500, 500, [500.0, 500.0]);

    let result = surface_to_mesh_output(&s, &cab, TargetSoftware::Disguise, 0.001);
    assert!(result.is_err());
}

#[test]
fn unreal_target_reverses_triangle_winding() {
    // Same surface as neutral, but with Unreal target → indices [a,b,c] should become [a,c,b].
    let s = sample_2x1_surface();
    let cab = rect_2x1();
    let neutral = surface_to_mesh_output(&s, &cab, TargetSoftware::Neutral, 0.001).unwrap();
    let unreal = surface_to_mesh_output(&s, &cab, TargetSoftware::Unreal, 0.001).unwrap();

    assert_eq!(neutral.triangles.len(), unreal.triangles.len());
    // For each triangle, the winding should be reversed
    for (n_tri, u_tri) in neutral.triangles.iter().zip(unreal.triangles.iter()) {
        // [a, b, c] vs [a, c, b]
        assert_eq!(n_tri[0], u_tri[0]);
        assert_eq!(n_tri[1], u_tri[2]);
        assert_eq!(n_tri[2], u_tri[1]);
    }
}
