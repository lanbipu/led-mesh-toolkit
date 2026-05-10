use lmt_core::surface::{GridTopology, MeshOutput, QualityMetrics, ReconstructedSurface, TargetSoftware};
use nalgebra::{Vector2, Vector3};

#[test]
fn surface_construction_holds_consistent_sizes() {
    let cols = 4;
    let rows = 3;
    let n_verts = ((cols + 1) * (rows + 1)) as usize;

    let vertices: Vec<Vector3<f64>> = (0..n_verts).map(|i| Vector3::new(i as f64, 0.0, 0.0)).collect();
    let uvs: Vec<Vector2<f64>> = (0..n_verts).map(|_| Vector2::zeros()).collect();

    let surf = ReconstructedSurface {
        screen_id: "MAIN".into(),
        topology: GridTopology { cols, rows },
        vertices,
        uv_coords: uvs,
        quality_metrics: QualityMetrics::default(),
    };

    assert_eq!(surf.vertices.len(), n_verts);
    assert_eq!(surf.uv_coords.len(), n_verts);
}

#[test]
fn target_software_serializes_to_lowercase() {
    let s = serde_yaml::to_string(&TargetSoftware::Disguise).unwrap();
    assert!(s.contains("disguise"));
}

#[test]
fn mesh_output_default_target_neutral() {
    let mo = MeshOutput {
        target: TargetSoftware::Neutral,
        vertices: vec![],
        triangles: vec![],
        uv_coords: vec![],
    };
    assert_eq!(mo.target, TargetSoftware::Neutral);
}

#[test]
fn grid_topology_vertex_count_handles_boundaries() {
    assert_eq!(GridTopology { cols: 0, rows: 0 }.vertex_count(), 1);
    assert_eq!(GridTopology { cols: 0, rows: 3 }.vertex_count(), 4);
    assert_eq!(GridTopology { cols: 4, rows: 0 }.vertex_count(), 5);
    assert_eq!(GridTopology { cols: 500, rows: 500 }.vertex_count(), 251_001);
}

#[test]
fn grid_topology_vertex_index_is_row_major() {
    let topology = GridTopology { cols: 4, rows: 3 };
    assert_eq!(topology.vertex_index(0, 0), 0);
    assert_eq!(topology.vertex_index(4, 0), 4);
    assert_eq!(topology.vertex_index(0, 1), 5);
    assert_eq!(topology.vertex_index(3, 2), 13);
}

#[test]
fn reconstructed_surface_yaml_round_trips_vectors() {
    let surf = ReconstructedSurface {
        screen_id: "MAIN".into(),
        topology: GridTopology { cols: 1, rows: 1 },
        vertices: vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(1.0, 1.0, 0.0),
        ],
        uv_coords: vec![
            Vector2::new(0.0, 0.0),
            Vector2::new(1.0, 0.0),
            Vector2::new(0.0, 1.0),
            Vector2::new(1.0, 1.0),
        ],
        quality_metrics: QualityMetrics::default(),
    };

    let yaml = serde_yaml::to_string(&surf).unwrap();
    let decoded: ReconstructedSurface = serde_yaml::from_str(&yaml).unwrap();

    assert_eq!(decoded.screen_id, surf.screen_id);
    assert_eq!(decoded.topology.cols, surf.topology.cols);
    assert_eq!(decoded.topology.rows, surf.topology.rows);
    assert_eq!(decoded.vertices, surf.vertices);
    assert_eq!(decoded.uv_coords, surf.uv_coords);
}
