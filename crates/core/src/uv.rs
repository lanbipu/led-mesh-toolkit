use nalgebra::Vector2;

use crate::surface::GridTopology;

/// Compute UV coordinates for a regular grid.
///
/// One UV cell per cabinet. UV origin (0,0) at the screen's bottom-left,
/// V increasing upward — disguise / 3ds Max convention (origin bottom-left).
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
            // V 随屏高度(row)递增：屏底(row 0)→V=0、屏顶→V=1，
            // 对齐 disguise / 3ds Max 的 UV 原点 (0,0) 在左下角约定。
            let v = row as f64 / topology.rows as f64;
            uvs.push(Vector2::new(u, v));
        }
    }

    uvs
}
