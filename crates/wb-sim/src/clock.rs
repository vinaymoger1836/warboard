//! Sim clock, the adaptive tick rule and the multi-rate helpers (doc 02 §3,
//! ADR-007).

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};

use crate::world::ForceFineTick;

/// Length of one tick. The rule depends only on state, so it is deterministic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TickLen {
    /// 100 ms while any weapon is in flight or a hostile is inside an engagement envelope.
    Fine,
    /// 1 s otherwise.
    Coarse,
}

impl TickLen {
    pub const fn ms(self) -> u32 {
        match self {
            TickLen::Fine => 100,
            TickLen::Coarse => 1_000,
        }
    }
}

#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SimClock {
    /// Ticks completed since war start.
    pub tick: u64,
    /// Sim time since war start, ms (start of the tick being run).
    pub sim_time_ms: u64,
    /// Length of the tick being run (or the last one run).
    pub tick_len: TickLen,
    /// War start, Unix epoch ms UTC.
    pub start_utc_ms: i64,
}

impl SimClock {
    /// Length of the current tick, seconds.
    pub fn dt_s(&self) -> f64 {
        f64::from(self.tick_len.ms()) / 1_000.0
    }

    /// Sim time at the end of the current tick, ms.
    pub fn end_ms(&self) -> u64 {
        self.sim_time_ms + u64::from(self.tick_len.ms())
    }

    /// True on the tick that crosses a multiple of `period_ms`: systems with
    /// their own rate (doc 02 §3 table) run then. `period_ms` must be ≥ 1 s,
    /// so it is a multiple of both tick lengths.
    pub fn due(&self, period_ms: u64) -> bool {
        self.end_ms() / period_ms > self.sim_time_ms / period_ms
    }

    /// Like [`Self::due`], shifted per unit so the work spreads over the
    /// period (`hash(UnitId) mod period`, doc 02 §3).
    pub fn due_staggered(&self, period_ms: u64, unit: u32) -> bool {
        let steps = (period_ms / 1_000).max(1);
        let off = (u64::from(unit).wrapping_mul(0x9E37_79B9) % steps) * 1_000;
        (self.end_ms() + off) / period_ms > (self.sim_time_ms + off) / period_ms
    }
}

#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RngSeed(pub u64);

/// Inputs to the adaptive tick rule. Weapons (P5) and engagement envelopes
/// (P5/P6) fill it in; nothing engages in P3.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct EngagementState {
    pub weapons_in_flight: u32,
    pub hostile_in_envelope: bool,
}

/// The adaptive tick rule.
pub fn tick_len_for(engagement: &EngagementState, force_fine: bool) -> TickLen {
    if force_fine || engagement.weapons_in_flight > 0 || engagement.hostile_in_envelope {
        TickLen::Fine
    } else {
        TickLen::Coarse
    }
}

/// Tick length the next `step` will use.
pub fn next_tick_len(world: &World) -> TickLen {
    tick_len_for(
        world.resource::<EngagementState>(),
        world.resource::<ForceFineTick>().0,
    )
}

/// Second stage of every tick (after orders).
pub fn choose_tick_len(world: &mut World) {
    let len = next_tick_len(world);
    world.resource_mut::<SimClock>().tick_len = len;
}

/// Last stage of every tick.
pub fn advance_clock(world: &mut World) {
    let mut clock = world.resource_mut::<SimClock>();
    clock.tick += 1;
    clock.sim_time_ms += u64::from(clock.tick_len.ms());
}
