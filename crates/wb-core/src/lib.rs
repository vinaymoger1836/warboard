//! Deterministic foundations shared by every engine crate.
//!
//! Everything here is a pure function of its inputs and gives bit-identical
//! results natively and in WebAssembly (CLAUDE.md rule 2, ADR-006).

pub mod collections;
pub mod geo;
pub mod ids;
pub mod math;
pub mod orbit;
pub mod rng;
pub mod sun;
pub mod units;

pub use ids::{ContactId, UnitId};
pub use rng::{RngPurpose, rng, rng_normal, rng_u64};
