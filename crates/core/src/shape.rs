use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Rectangular grid of cabinets, with optional irregular mask.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CabinetArray {
    pub cols: u32,
    pub rows: u32,
    /// Single cabinet size in millimeters: [width, height].
    pub cabinet_size_mm: [f64; 2],
    /// Cells that are explicitly absent (irregular shape).
    /// Keyed by (col, row), 0-based.
    #[serde(default)]
    pub absent_cells: HashSet<(u32, u32)>,
}

impl CabinetArray {
    /// Construct a complete rectangular array (no missing cells).
    pub fn rectangle(cols: u32, rows: u32, cabinet_size_mm: [f64; 2]) -> Self {
        Self {
            cols,
            rows,
            cabinet_size_mm,
            absent_cells: HashSet::new(),
        }
    }

    /// Construct an irregular array with explicitly absent cells.
    pub fn irregular(
        cols: u32,
        rows: u32,
        cabinet_size_mm: [f64; 2],
        absent: Vec<(u32, u32)>,
    ) -> Self {
        Self {
            cols,
            rows,
            cabinet_size_mm,
            absent_cells: absent.into_iter().collect(),
        }
    }

    /// Returns whether a given (col, row) cell exists in the screen.
    pub fn is_present(&self, col: u32, row: u32) -> bool {
        col < self.cols && row < self.rows && !self.absent_cells.contains(&(col, row))
    }

    /// Total physical size of the rectangular bounding box, in mm.
    pub fn total_size_mm(&self) -> [f64; 2] {
        [
            self.cabinet_size_mm[0] * self.cols as f64,
            self.cabinet_size_mm[1] * self.rows as f64,
        ]
    }
}

/// Prior knowledge about screen geometry.
///
/// Externally tagged: `flat` for unit variant, `curved: { radius_mm: N }` etc.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapePrior {
    Flat,
    /// Half-cylinder with constant radius.
    Curved { radius_mm: f64 },
    /// Multi-segment flat with folds at given column indices.
    Folded { fold_seam_columns: Vec<u32> },
}
