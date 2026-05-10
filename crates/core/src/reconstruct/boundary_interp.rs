use nalgebra::Vector3;

use crate::error::CoreError;
use crate::measured_points::MeasuredPoints;
use crate::reconstruct::Reconstructor;
use crate::surface::{GridTopology, QualityMetrics, ReconstructedSurface};
use crate::uv::compute_grid_uv;

/// Boundary-interp reconstructor: requires full top + bottom rows
/// of vertex points. Interpolates the interior linearly between
/// the matched (col-aligned) top/bottom samples.
pub struct BoundaryInterpReconstructor;

impl Reconstructor for BoundaryInterpReconstructor {
    fn name(&self) -> &'static str {
        "boundary_interp"
    }

    fn applicable(&self, points: &MeasuredPoints) -> bool {
        // Skip irregular shapes (consistent with DirectLinkReconstructor —
        // masked topology is deferred).
        if !points.cabinet_array.absent_cells.is_empty() {
            return false;
        }

        let cols = points.cabinet_array.cols;
        let rows = points.cabinet_array.rows;

        // need every column's top + bottom vertex
        for c in 1..=(cols + 1) {
            let top_name = format!("{}_V{:03}_R{:03}", points.screen_id, c, rows + 1);
            let bot_name = format!("{}_V{:03}_R{:03}", points.screen_id, c, 1);
            if points.find(&top_name).is_none() || points.find(&bot_name).is_none() {
                return false;
            }
        }
        true
    }

    fn reconstruct(&self, points: &MeasuredPoints) -> Result<ReconstructedSurface, CoreError> {
        let cols = points.cabinet_array.cols;
        let rows = points.cabinet_array.rows;
        let topo = GridTopology { cols, rows };

        let mut vertices = vec![Vector3::zeros(); topo.vertex_count()];

        for c in 0..=cols {
            let top_name = format!("{}_V{:03}_R{:03}", points.screen_id, c + 1, rows + 1);
            let bot_name = format!("{}_V{:03}_R{:03}", points.screen_id, c + 1, 1);
            let top_pos = points
                .find(&top_name)
                .ok_or_else(|| CoreError::Reconstruction(format!("missing top {}", top_name)))?
                .position;
            let bot_pos = points
                .find(&bot_name)
                .ok_or_else(|| CoreError::Reconstruction(format!("missing bot {}", bot_name)))?
                .position;

            for r in 0..=rows {
                let t = r as f64 / rows as f64; // 0 = bottom, 1 = top
                let v = bot_pos * (1.0 - t) + top_pos * t;
                vertices[topo.vertex_index(c, r)] = v;
            }
        }

        // Validate any interior (non-top/bottom) measured grid points against
        // the interpolation result. Fill middle_max_dev_mm / middle_mean_dev_mm
        // and emit warnings when deviation exceeds the threshold.
        let prefix = format!("{}_V", points.screen_id);
        let mut max_dev_mm: f64 = 0.0;
        let mut sum_dev_mm: f64 = 0.0;
        let mut n_validated: usize = 0;
        let mut warnings: Vec<String> = Vec::new();
        const INTERIOR_DEV_WARN_MM: f64 = 10.0;

        for mp in &points.points {
            let Some(rest) = mp.name.strip_prefix(&prefix) else {
                continue;
            };
            let parts: Vec<&str> = rest.split("_R").collect();
            if parts.len() != 2 {
                continue;
            }
            let Ok(col_1based) = parts[0].parse::<u32>() else {
                continue;
            };
            let Ok(row_1based) = parts[1].parse::<u32>() else {
                continue;
            };
            if col_1based == 0 || row_1based == 0 {
                continue;
            }
            let col = col_1based - 1;
            let row = row_1based - 1;
            if col > cols || row > rows {
                continue;
            }
            // Skip top/bottom anchors — they're exactly reproduced by interpolation.
            if row == 0 || row == rows {
                continue;
            }

            let interpolated = vertices[topo.vertex_index(col, row)];
            let dev_m = (mp.position - interpolated).norm();
            let dev_mm = dev_m * 1000.0;
            max_dev_mm = max_dev_mm.max(dev_mm);
            sum_dev_mm += dev_mm;
            n_validated += 1;

            if dev_mm > INTERIOR_DEV_WARN_MM {
                warnings.push(format!(
                    "{} deviates {:.2}mm from boundary interpolation (>{}mm threshold)",
                    mp.name, dev_mm, INTERIOR_DEV_WARN_MM
                ));
            }
        }

        let mean_dev_mm = if n_validated > 0 {
            sum_dev_mm / n_validated as f64
        } else {
            0.0
        };

        // RMS estimate from input uncertainties (consistent with DirectLink approach).
        let estimated_rms_mm = if points.points.is_empty() {
            0.0
        } else {
            let n = points.points.len() as f64;
            let sum_sq: f64 = points
                .points
                .iter()
                .map(|p| p.uncertainty.sigma_approx().powi(2))
                .sum();
            (sum_sq / n).sqrt()
        };

        let uvs = compute_grid_uv(topo);
        let metrics = QualityMetrics {
            method: "boundary_interp".into(),
            measured_count: points.len(),
            expected_count: topo.vertex_count(),
            // BoundaryInterp adds interpolation error on top of measurement error;
            // bound below at the interpolation floor (5mm typical for 60×10 walls).
            estimated_rms_mm: estimated_rms_mm.max(5.0),
            middle_max_dev_mm: max_dev_mm,
            middle_mean_dev_mm: mean_dev_mm,
            warnings,
            ..Default::default()
        };

        Ok(ReconstructedSurface {
            screen_id: points.screen_id.clone(),
            topology: topo,
            vertices,
            uv_coords: uvs,
            quality_metrics: metrics,
        })
    }
}
