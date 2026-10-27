//! P0 determinism probe.
//!
//! Not gameplay. When a scenario sets `determinism_probe: true`, this system
//! draws counter-RNG values and runs great-circle maths every tick so that the
//! native-vs-WASM hash check covers `libm`, the RNG and `UnitId`-ordered
//! iteration, not just an empty clock. Movement proper arrives in Phase 3.

use crate::clock::{RngSeed, SimClock};
use crate::components::{Identity, Kinematics};
use bevy_ecs::prelude::*;
use core::f64::consts::TAU;
use wb_core::geo::{LatLon, destination_point, initial_bearing, normalize_bearing};
use wb_core::{RngPurpose, rng, rng_u64};

/// Probe accumulator, part of the state hash.
#[derive(Resource, Clone, Copy, Debug)]
pub struct ProbeState {
    pub point: LatLon,
    pub acc: u64,
}

impl ProbeState {
    pub fn new() -> Self {
        Self {
            point: LatLon::from_deg(54.0, 20.0),
            acc: 0,
        }
    }
}

impl Default for ProbeState {
    fn default() -> Self {
        Self::new()
    }
}

/// Entity number the probe's own draws use (no unit has id 0 in practice).
const PROBE_ENTITY: u64 = u64::MAX;

pub fn determinism_probe(
    probe: Option<ResMut<ProbeState>>,
    clock: Res<SimClock>,
    seed: Res<RngSeed>,
    mut units: Query<(&Identity, &mut Kinematics)>,
) {
    let Some(mut probe) = probe else {
        return;
    };
    let tick = clock.tick;
    let dt_s = f64::from(clock.tick_len.ms()) / 1_000.0;

    // A random walk of the probe point.
    let bearing = rng(seed.0, RngPurpose::Probe, PROBE_ENTITY, tick, 0) * TAU;
    let dist = 1_000.0 + 9_000.0 * rng(seed.0, RngPurpose::Probe, PROBE_ENTITY, tick, 1);
    probe.point = destination_point(probe.point, bearing, dist);
    probe.acc =
        probe.acc.rotate_left(7) ^ rng_u64(seed.0, RngPurpose::Probe, PROBE_ENTITY, tick, 2);

    // Units drift with a jittered heading, visited in UnitId order (archetype
    // order must never matter). Each unit also turns slightly toward the probe
    // point, so results depend on bearing maths.
    let mut ordered: Vec<_> = units.iter_mut().collect();
    ordered.sort_by_key(|(id, _)| id.unit);
    for (id, mut k) in ordered {
        let here = LatLon::new(k.lat, k.lon);
        let jitter = (rng(seed.0, RngPurpose::Probe, u64::from(id.unit.0), tick, 0) - 0.5) * 0.02;
        let toward = initial_bearing(here, probe.point);
        let turn = (toward - k.heading).clamp(-0.01, 0.01);
        k.heading = normalize_bearing(k.heading + jitter + turn);
        let next = destination_point(here, k.heading, k.speed_mps * dt_s);
        k.lat = next.lat;
        k.lon = next.lon;
        probe.acc = probe
            .acc
            .wrapping_add(k.lat.to_bits() ^ k.lon.to_bits().rotate_left(17));
    }
}
