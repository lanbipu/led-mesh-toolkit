//! LED Mesh Toolkit core library.
//!
//! Defines the Intermediate Representation (IR) and shared
//! reconstruction / UV / export pipeline used by both M1
//! (total-station) and M2 (visual photogrammetry) adapters.

pub mod error;
pub mod uncertainty;
pub mod point;
pub mod coordinate;
pub mod shape;
pub mod measured_points;
pub mod surface;
pub mod uv;

pub use error::CoreError;
