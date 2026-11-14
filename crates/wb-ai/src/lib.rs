//! Enemy AI (doc 03 §11, ADR-015, ADR-046, ADR-049): strategic, operational and
//! tactical decisions for an AI-controlled side, from that side's picture only.
//! v2 (P12, `ops`) plans multi-phase operations with the same phase model a
//! player uses.
//!
//! The crate depends on `wb-core` alone. It cannot see the engine, the truth,
//! the content or the umpire view: the engine's adapter (`wb_sim::ai`) hands it
//! an [`AiPicture`] built from the side's own picture and turns its
//! [`Action`]s into ordinary orders of that side, validated like a player's.

pub mod brain;
pub mod ops;
pub mod picture;

pub use brain::{AiOp, AiState, Posture, Threat, ThreatKind, reaction_delay_ms, think};
pub use picture::{
    Action, Activity, AiPicture, Decision, Difficulty, Doctrine, Domain, Emcon, Layer, OpPlan,
    OpStatus, OwnBase, OwnGroup, OwnOperation, PhaseStatus, PlanAction, PlanCond, PlanPhase,
    PlanTask, Request, Role, Seen,
};
