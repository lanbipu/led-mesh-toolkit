//! LED Mesh Toolkit core library.
//!
//! Defines the Intermediate Representation (IR) and shared
//! reconstruction / UV / export pipeline used by both M1
//! (total-station) and M2 (visual photogrammetry) adapters.

pub mod coordinate;
pub mod error;
pub mod export;
pub mod measured_points;
pub mod point;
pub mod reconstruct;
pub mod shape;
pub mod surface;
pub mod triangulate;
pub mod uncertainty;
pub mod uv;
pub mod weld;

pub use error::CoreError;
