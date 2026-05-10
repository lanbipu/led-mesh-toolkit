use nalgebra::Vector2;

use crate::surface::GridTopology;

/// Compute UV coordinates for a regular grid.
///
/// One UV cell per cabinet, V axis flipped (Disguise convention:
/// V increases upward, but bottom-left of texture has V=1).
///
/// Panics if `topology.cols == 0` or `topology.rows == 0`
/// (a zero-dimension grid would produce NaN UVs).
pub fn compute_grid_uv(topology: GridTopology) -> Vec<Vector2<f64>> {
    assert!(topology.cols > 0, "GridTopology.cols must be > 0");
    assert!(topology.rows > 0, "GridTopology.rows must be > 0");

    let n_cols_v = (topology.cols + 1) as usize;
    let n_rows_v = (topology.rows + 1) as usize;
    let mut uvs = Vec::with_capacity(n_cols_v * n_rows_v);

    for row in 0..n_rows_v {
        for col in 0..n_cols_v {
            let u = col as f64 / topology.cols as f64;
            let v = 1.0 - (row as f64 / topology.rows as f64);
            uvs.push(Vector2::new(u, v));
        }
    }

    uvs
}
