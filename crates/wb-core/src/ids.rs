//! Identifier newtypes.
//!
//! `UnitId` names a real entity and never leaves the engine for the other side.
//! `ContactId` names an entry in one side's picture. They are distinct types
//! with no conversion between them, so a contact can never be built from a
//! unit id by accident (CLAUDE.md rule 1, ADR-003).

use serde::{Deserialize, Serialize};

/// Stable id of an engine entity (platform, group, base, weapon).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(transparent)]
pub struct UnitId(pub u32);

/// Per-side id of a contact in a side's picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(transparent)]
pub struct ContactId(pub u32);

impl std::fmt::Display for UnitId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "U{}", self.0)
    }
}

impl std::fmt::Display for ContactId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "C{}", self.0)
    }
}
