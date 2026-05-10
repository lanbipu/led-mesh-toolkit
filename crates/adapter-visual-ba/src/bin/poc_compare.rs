//! PoC compare tool: load two MeasuredPoints (ground truth + visual BA),
//! compute RMS / 95th percentile error per point.
//!
//! In `three_points` mode, RMS / p95 are computed on the *holdout* set
//! (points NOT used as Procrustes anchors); anchor residuals are reported
//! separately. In `nominal_anchoring` mode, RMS uses all matched points.

use std::collections::HashSet;
use std::path::PathBuf;

use lmt_core::measured_points::MeasuredPoints;
use serde::Serialize;

#[derive(Debug)]
struct Args {
    ground_truth: PathBuf,
    measured: PathBuf,
    frame_strategy: String,
    anchor_ids: HashSet<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut gt: Option<PathBuf> = None;
    let mut me: Option<PathBuf> = None;
    let mut fs: Option<String> = None;
    let mut anchors: HashSet<String> = HashSet::new();
    let mut iter = std::env::args().skip(1);
    while let Some(a) = iter.next() {
        match a.as_str() {
            "--ground-truth" => gt = iter.next().map(PathBuf::from),
            "--measured" => me = iter.next().map(PathBuf::from),
            "--frame-strategy" => fs = iter.next(),
            "--anchor-ids" => {
                if let Some(v) = iter.next() {
                    for id in v.split(',') { anchors.insert(id.trim().to_string()); }
                }
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(Args {
        ground_truth: gt.ok_or("--ground-truth required")?,
        measured: me.ok_or("--measured required")?,
        frame_strategy: fs.ok_or("--frame-strategy required")?,
        anchor_ids: anchors,
    })
}

fn percentile(values: &mut Vec<f64>, p: f64) -> f64 {
    if values.is_empty() { return 0.0; }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let idx = ((values.len() as f64 - 1.0) * p).round() as usize;
    values[idx.min(values.len() - 1)]
}

fn rms(values: &[f64]) -> f64 {
    if values.is_empty() { return 0.0; }
    (values.iter().map(|v| v * v).sum::<f64>() / values.len() as f64).sqrt()
}

#[derive(Serialize)]
struct Report {
    frame_strategy: String,
    n_compared: usize,
    rms_mm: Option<f64>,
    p95_mm: Option<f64>,
    holdout_rms_mm: Option<f64>,
    holdout_p95_mm: Option<f64>,
    anchor_residual_rms_mm: Option<f64>,
    per_point_mm: Vec<(String, f64)>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args().map_err(|e| { eprintln!("{e}"); e })?;

    let gt: MeasuredPoints = serde_json::from_str(&std::fs::read_to_string(&args.ground_truth)?)?;
    let me: MeasuredPoints = serde_json::from_str(&std::fs::read_to_string(&args.measured)?)?;

    let mut per_point: Vec<(String, f64)> = Vec::new();
    for gp in &gt.points {
        if let Some(mp) = me.find(&gp.name) {
            let d = (gp.position - mp.position).norm() * 1000.0; // m → mm
            per_point.push((gp.name.clone(), d));
        }
    }

    let report = if args.frame_strategy == "three_points" {
        let mut holdout: Vec<f64> = Vec::new();
        let mut anchor: Vec<f64> = Vec::new();
        for (name, d) in &per_point {
            if args.anchor_ids.contains(name) {
                anchor.push(*d);
            } else {
                holdout.push(*d);
            }
        }
        let mut h = holdout.clone();
        Report {
            frame_strategy: args.frame_strategy,
            n_compared: per_point.len(),
            rms_mm: None,
            p95_mm: None,
            holdout_rms_mm: Some(rms(&holdout)),
            holdout_p95_mm: Some(percentile(&mut h, 0.95)),
            anchor_residual_rms_mm: Some(rms(&anchor)),
            per_point_mm: per_point,
        }
    } else {
        let mut all: Vec<f64> = per_point.iter().map(|(_, d)| *d).collect();
        Report {
            frame_strategy: args.frame_strategy,
            n_compared: per_point.len(),
            rms_mm: Some(rms(&all)),
            p95_mm: Some(percentile(&mut all, 0.95)),
            holdout_rms_mm: None,
            holdout_p95_mm: None,
            anchor_residual_rms_mm: None,
            per_point_mm: per_point,
        }
    };

    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
