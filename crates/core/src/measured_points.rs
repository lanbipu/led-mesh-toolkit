use serde::{Deserialize, Serialize};

use crate::coordinate::CoordinateFrame;
use crate::point::MeasuredPoint;
use crate::shape::{CabinetArray, ShapePrior};

/// Top-level IR: all measured points for one screen plus its
/// coordinate frame and structural priors.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeasuredPoints {
    pub screen_id: String,
    pub coordinate_frame: CoordinateFrame,
    pub cabinet_array: CabinetArray,
    pub shape_prior: ShapePrior,
    pub points: Vec<MeasuredPoint>,
}

impl MeasuredPoints {
    /// Find a point by exact name. Returns `None` if not found.
    pub fn find(&self, name: &str) -> Option<&MeasuredPoint> {
        self.points.iter().find(|p| p.name == name)
    }

    /// Number of measured points present.
    pub fn len(&self) -> usize {
        self.points.len()
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }
}
