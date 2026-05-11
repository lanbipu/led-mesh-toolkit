//! Total-station CSV adapter (M1).
//!
//! Reads instrument-numbered CSV from a Trimble / Leica total station,
//! a project YAML config, and produces `lmt_core::MeasuredPoints` ready
//! for reconstruction + export, plus a JSON validation report and a
//! field instruction card (PDF + HTML).

pub mod csv_parser;
pub mod error;
pub mod geometric_naming;
pub mod project;
pub mod project_loader;
pub mod raw_point;
pub mod reference_frame;
pub mod report;
pub mod shape_grid;
pub mod transform;

pub use error::AdapterError;
pub use raw_point::RawPoint;
