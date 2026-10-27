//! ECS components (doc 02 §2). P0 has only the two every unit needs.

use bevy_ecs::prelude::*;
use wb_core::UnitId;

#[derive(Component, Clone, Debug)]
pub struct Identity {
    pub unit: UnitId,
    pub name: String,
}

/// Position and motion. Radians and SI units.
#[derive(Component, Clone, Copy, Debug)]
pub struct Kinematics {
    pub lat: f64,
    pub lon: f64,
    pub alt_m: f64,
    /// Radians clockwise from true north.
    pub heading: f64,
    pub speed_mps: f64,
}
