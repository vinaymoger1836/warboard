//! Sim clock and the adaptive tick rule (doc 02 §3, ADR-007).

use bevy_ecs::prelude::*;

/// Length of one tick. The rule depends only on state, so it is deterministic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

#[derive(Resource, Clone, Debug)]
pub struct SimClock {
    /// Ticks completed since war start.
    pub tick: u64,
    /// Sim time since war start, ms.
    pub sim_time_ms: u64,
    /// Length of the tick being run (or the last one run).
    pub tick_len: TickLen,
    /// War start, Unix epoch ms UTC.
    pub start_utc_ms: i64,
}

#[derive(Resource, Clone, Copy, Debug)]
pub struct RngSeed(pub u64);

/// Inputs to the adaptive tick rule. Stub in P0: weapons (P5) and engagement
/// envelopes (P5/P6) will fill it in.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct EngagementState {
    pub weapons_in_flight: u32,
    pub hostile_in_envelope: bool,
}

/// The adaptive tick rule.
pub fn tick_len_for(engagement: &EngagementState) -> TickLen {
    if engagement.weapons_in_flight > 0 || engagement.hostile_in_envelope {
        TickLen::Fine
    } else {
        TickLen::Coarse
    }
}

/// Tick length the next `step` will use.
pub fn next_tick_len(world: &World) -> TickLen {
    tick_len_for(world.resource::<EngagementState>())
}

/// First system of every tick.
pub fn choose_tick_len(engagement: Res<EngagementState>, mut clock: ResMut<SimClock>) {
    clock.tick_len = tick_len_for(&engagement);
}

/// Last system of every tick.
pub fn advance_clock(mut clock: ResMut<SimClock>) {
    clock.tick += 1;
    clock.sim_time_ms += u64::from(clock.tick_len.ms());
}
