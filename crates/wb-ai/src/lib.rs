//! Enemy AI v1 (doc 03 §11, ADR-015, ADR-046): strategic, operational and
//! tactical decisions for an AI-controlled side, from that side's picture only.
//!
//! The crate depends on `wb-core` alone. It cannot see the engine, the truth,
//! the content or the umpire view: the engine's adapter (`wb_sim::ai`) hands it
//! an [`AiPicture`] built from the side's own picture and turns its
//! [`Action`]s into ordinary orders of that side, validated like a player's.

pub mod brain;
pub mod picture;

pub use brain::{AiState, Posture, Threat, ThreatKind, reaction_delay_ms, think};
pub use picture::{
    Action, Activity, AiPicture, Decision, Difficulty, Doctrine, Domain, Emcon, Layer, OwnBase,
    OwnGroup, Request, Role, Seen,
};
