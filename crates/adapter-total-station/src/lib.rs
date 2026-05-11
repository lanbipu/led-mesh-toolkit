//! Total-station CSV adapter (M1).
//!
//! Reads instrument-numbered CSV from a Trimble / Leica total station,
//! a project YAML config, and produces `lmt_core::MeasuredPoints` ready
//! for reconstruction + export, plus a JSON validation report and a
//! field instruction card (PDF + HTML).

pub mod error;
pub mod project;
pub mod raw_point;

pub use error::AdapterError;
pub use raw_point::RawPoint;
