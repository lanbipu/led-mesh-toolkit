use lmt_core::surface::GridTopology;
use lmt_core::uv::compute_grid_uv;
use nalgebra::Vector2;
use pretty_assertions::assert_eq;

#[test]
fn uv_for_2x2_grid_has_9_vertices() {
    let topo = GridTopology { cols: 2, rows: 2 };
    let uvs = compute_grid_uv(topo);
    assert_eq!(uvs.len(), 9);
}

#[test]
fn uv_corners_are_at_unit_square_corners() {
    let topo = GridTopology { cols: 4, rows: 3 };
    let uvs = compute_grid_uv(topo);

    // bottom-left vertex (col=0, row=0) → V is flipped, so this is (0, 1)
    assert_eq!(uvs[topo.vertex_index(0, 0)], Vector2::new(0.0, 1.0));
    // top-right vertex (col=4, row=3) → (1, 0)
    assert_eq!(uvs[topo.vertex_index(4, 3)], Vector2::new(1.0, 0.0));
    // top-left (col=0, row=3) → (0, 0)
    assert_eq!(uvs[topo.vertex_index(0, 3)], Vector2::new(0.0, 0.0));
    // bottom-right (col=4, row=0) → (1, 1)
    assert_eq!(uvs[topo.vertex_index(4, 0)], Vector2::new(1.0, 1.0));
}

#[test]
fn uv_step_matches_cabinet_size() {
    // For a 10×5 grid, each cabinet is 1/10 in U and 1/5 in V.
    let topo = GridTopology { cols: 10, rows: 5 };
    let uvs = compute_grid_uv(topo);

    let v00 = uvs[topo.vertex_index(0, 0)];
    let v10 = uvs[topo.vertex_index(1, 0)];
    let v01 = uvs[topo.vertex_index(0, 1)];

    assert!((v10.x - v00.x - 0.1).abs() < 1e-9);
    // V flipped: row+1 means V decreases
    assert!((v00.y - v01.y - 0.2).abs() < 1e-9);
}

#[test]
#[should_panic(expected = "cols must be > 0")]
fn uv_panics_on_zero_cols() {
    let topo = GridTopology { cols: 0, rows: 4 };
    let _ = compute_grid_uv(topo);
}

#[test]
#[should_panic(expected = "rows must be > 0")]
fn uv_panics_on_zero_rows() {
    let topo = GridTopology { cols: 4, rows: 0 };
    let _ = compute_grid_uv(topo);
}
