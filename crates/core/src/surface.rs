use nalgebra::{Vector2, Vector3};
use serde::{Deserialize, Serialize};

/// Maximum allowed cabinet count per axis (prevents pathological allocations
/// + overflow). 10_000 × 10_000 cabinets = 100M vertices upper bound, far
///   beyond any realistic LED screen.
pub const MAX_GRID_DIM: u32 = 10_000;

/// Grid topology for a single screen.
/// Vertex count = (cols + 1) * (rows + 1).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct GridTopology {
    pub cols: u32,
    pub rows: u32,
}

#[derive(Deserialize)]
struct GridTopologyRaw {
    cols: u32,
    rows: u32,
}

impl<'de> Deserialize<'de> for GridTopology {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = GridTopologyRaw::deserialize(d)?;
        if raw.cols > MAX_GRID_DIM {
            return Err(serde::de::Error::custom(format!(
                "GridTopology.cols {} exceeds MAX_GRID_DIM ({})",
                raw.cols, MAX_GRID_DIM
            )));
        }
        if raw.rows > MAX_GRID_DIM {
            return Err(serde::de::Error::custom(format!(
                "GridTopology.rows {} exceeds MAX_GRID_DIM ({})",
                raw.rows, MAX_GRID_DIM
            )));
        }
        Ok(Self { cols: raw.cols, rows: raw.rows })
    }
}

impl GridTopology {
    /// Total vertex count = (cols+1) * (rows+1). Panics on arithmetic overflow.
    pub fn vertex_count(&self) -> usize {
        let cols_p1 = (self.cols as usize)
            .checked_add(1)
            .expect("cols+1 overflow");
        let rows_p1 = (self.rows as usize)
            .checked_add(1)
            .expect("rows+1 overflow");
        cols_p1
            .checked_mul(rows_p1)
            .expect("vertex_count overflow")
    }

    /// Row-major index. Panics if (col, row) out of bounds when usize-multiplied.
    pub fn vertex_index(&self, col: u32, row: u32) -> usize {
        let cols_p1 = (self.cols as usize)
            .checked_add(1)
            .expect("cols+1 overflow");
        (row as usize)
            .checked_mul(cols_p1)
            .and_then(|r| r.checked_add(col as usize))
            .expect("vertex_index overflow")
    }
}

/// Diagnostic metrics produced by the reconstruction step.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QualityMetrics {
    pub method: String,
    pub middle_max_dev_mm: f64,
    pub middle_mean_dev_mm: f64,
    pub shape_fit_rms_mm: f64,
    pub measured_count: usize,
    pub expected_count: usize,
    pub missing: Vec<String>,
    pub outliers: Vec<String>,
    pub estimated_rms_mm: f64,
    pub estimated_p95_mm: f64,
    pub warnings: Vec<String>,
}

/// Reconstructed surface: grid of vertices in model frame, with UVs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconstructedSurface {
    pub screen_id: String,
    pub topology: GridTopology,
    /// (cols+1) × (rows+1) vertices, row-major: `vertex_index(col, row)`.
    #[serde(with = "vec_vector3_serde")]
    pub vertices: Vec<Vector3<f64>>,
    #[serde(with = "vec_vector2_serde")]
    pub uv_coords: Vec<Vector2<f64>>,
    pub quality_metrics: QualityMetrics,
}

/// Target export software (controls coordinate-frame + units).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetSoftware {
    /// Right-handed, +Y up, meters.
    Disguise,
    /// Left-handed, +Z up, centimeters.
    Unreal,
    /// Right-handed, +Z up, meters (raw model frame).
    Neutral,
}

/// Final mesh ready for export — already adapted to the target software.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshOutput {
    pub target: TargetSoftware,
    #[serde(with = "vec_vector3_serde")]
    pub vertices: Vec<Vector3<f64>>,
    pub triangles: Vec<[u32; 3]>,
    #[serde(with = "vec_vector2_serde")]
    pub uv_coords: Vec<Vector2<f64>>,
}

mod vec_vector3_serde {
    use nalgebra::Vector3;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(v: &[Vector3<f64>], s: S) -> Result<S::Ok, S::Error> {
        let arr: Vec<[f64; 3]> = v.iter().map(|p| [p.x, p.y, p.z]).collect();
        arr.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Vector3<f64>>, D::Error> {
        let arr: Vec<[f64; 3]> = Deserialize::deserialize(d)?;
        Ok(arr.into_iter().map(|a| Vector3::new(a[0], a[1], a[2])).collect())
    }
}

mod vec_vector2_serde {
    use nalgebra::Vector2;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(v: &[Vector2<f64>], s: S) -> Result<S::Ok, S::Error> {
        let arr: Vec<[f64; 2]> = v.iter().map(|p| [p.x, p.y]).collect();
        arr.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Vector2<f64>>, D::Error> {
        let arr: Vec<[f64; 2]> = Deserialize::deserialize(d)?;
        Ok(arr.into_iter().map(|a| Vector2::new(a[0], a[1])).collect())
    }
}
