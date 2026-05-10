//! LED Mesh Toolkit core library.
//!
//! Defines the Intermediate Representation (IR) and shared
//! reconstruction / UV / export pipeline used by both M1
//! (total-station) and M2 (visual photogrammetry) adapters.

pub mod error;
pub mod uncertainty;

pub use error::CoreError;
