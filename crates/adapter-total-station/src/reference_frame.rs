use lmt_core::coordinate::CoordinateFrame;

use crate::error::AdapterError;
use crate::raw_point::RawPoint;

/// SOP-scale minimum baseline between origin and either reference marker (meters).
/// Picked at 50× the typical 2mm total-station instrument noise floor — far enough
/// that the resulting basis vector isn't dominated by measurement uncertainty.
const MIN_BASELINE_M: f64 = 0.1;

/// SOP-scale minimum perpendicular distance from the xy-plane reference to the
/// X axis (meters). Picked at half the baseline minimum so a near-collinear
/// xy-plane marker (e.g. user measured the wrong corner) is rejected before
/// it produces a noise-dominated Y axis.
const MIN_PERPENDICULAR_M: f64 = 0.05;

/// Use the first 3 raw points (by SOP: instrument_ids 1, 2, 3) as
/// origin / X-axis-ref / XY-plane-ref to construct a `CoordinateFrame`.
///
/// Errors if `raw.len() < 3`, the first three ids aren't 1/2/3 in order,
/// the baselines are below SOP-scale precision (`MIN_BASELINE_M` /
/// `MIN_PERPENDICULAR_M`), or `from_three_points` rejects (collinear /
/// coincident).
pub fn build_frame_from_first_three(raw: &[RawPoint]) -> Result<CoordinateFrame, AdapterError> {
    if raw.len() < 3 {
        return Err(AdapterError::InvalidInput(format!(
            "need at least 3 raw points, got {}",
            raw.len()
        )));
    }
    if raw[0].instrument_id != 1 || raw[1].instrument_id != 2 || raw[2].instrument_id != 3 {
        return Err(AdapterError::InvalidInput(format!(
            "first 3 raw points must have instrument_ids 1, 2, 3 in order; \
             got [{}, {}, {}]",
            raw[0].instrument_id, raw[1].instrument_id, raw[2].instrument_id
        )));
    }

    let origin = raw[0].position_meters();
    let x_axis = raw[1].position_meters();
    let xy_plane = raw[2].position_meters();

    let dx = x_axis - origin;
    if dx.norm() < MIN_BASELINE_M {
        return Err(AdapterError::InvalidInput(format!(
            "reference baseline origin→x_axis = {:.3}mm < SOP minimum {:.0}mm; \
             re-measure with the prism farther from origin",
            dx.norm() * 1000.0,
            MIN_BASELINE_M * 1000.0
        )));
    }
    let dxy = xy_plane - origin;
    if dxy.norm() < MIN_BASELINE_M {
        return Err(AdapterError::InvalidInput(format!(
            "reference baseline origin→xy_plane = {:.3}mm < SOP minimum {:.0}mm; \
             re-measure with the prism farther from origin",
            dxy.norm() * 1000.0,
            MIN_BASELINE_M * 1000.0
        )));
    }
    let perp = (dxy - dx * (dxy.dot(&dx) / dx.dot(&dx))).norm();
    if perp < MIN_PERPENDICULAR_M {
        return Err(AdapterError::InvalidInput(format!(
            "xy_plane reference is too close to the x-axis line \
             (perpendicular distance {:.3}mm < SOP minimum {:.0}mm); \
             pick a marker farther off-axis",
            perp * 1000.0,
            MIN_PERPENDICULAR_M * 1000.0
        )));
    }

    let native = CoordinateFrame::from_three_points(origin, x_axis, xy_plane)
        .map_err(AdapterError::Core)?;

    // M0.1 IR convention (per crates/core/tests/fixtures/curved_demo_points.yaml):
    //   model +X = cols, model +Z = rows-up, model +Y = screen normal.
    //
    // But `from_three_points` produces basis = [X_cols, Y_in_plane, Z_normal]
    // where Y_in_plane is the rows direction (with a sign depending on the
    // cross-product handedness; here Y_in_plane points rows-DOWN). To match
    // M0.1, we permute the basis so that:
    //   new_X = X_cols (unchanged)
    //   new_Y = Z_normal     (so model +Y is the normal)
    //   new_Z = -Y_in_plane  (so model +Z is rows-UP; negation keeps a right-handed basis)
    let b = &native.basis;
    let frame = CoordinateFrame {
        origin_world: native.origin_world,
        basis: [
            b[0],
            b[2],
            [-b[1][0], -b[1][1], -b[1][2]],
        ],
    };
    Ok(frame)
}
