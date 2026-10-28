//! P0 determinism probe.
//!
//! Not gameplay. When a scenario sets `determinism_probe: true`, this system
//! draws counter-RNG values and runs great-circle maths every tick so that the
//! native-vs-WASM hash check covers `libm`, the RNG and `UnitId`-ordered
//! iteration. It moves only bare units (no platform).

use crate::clock::{RngSeed, SimClock};
use crate::components::{Kinematics, PlatformRef};
use crate::world::UnitIndex;
use bevy_ecs::prelude::*;
use core::f64::consts::TAU;
use serde::{Deserialize, Serialize};
use wb_core::geo::{LatLon, destination_point, initial_bearing, normalize_bearing};
use wb_core::{RngPurpose, rng, rng_u64};

/// Probe accumulator, part of the state hash.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
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

pub fn determinism_probe(world: &mut World) {
    let Some(mut probe) = world.get_resource::<ProbeState>().copied() else {
        return;
    };
    let clock = world.resource::<SimClock>();
    let tick = clock.tick;
    let dt_s = clock.dt_s();
    let seed = world.resource::<RngSeed>().0;

    // A random walk of the probe point.
    let bearing = rng(seed, RngPurpose::Probe, PROBE_ENTITY, tick, 0) * TAU;
    let dist = 1_000.0 + 9_000.0 * rng(seed, RngPurpose::Probe, PROBE_ENTITY, tick, 1);
    probe.point = destination_point(probe.point, bearing, dist);
    probe.acc = probe.acc.rotate_left(7) ^ rng_u64(seed, RngPurpose::Probe, PROBE_ENTITY, tick, 2);

    // Bare units drift with a jittered heading, visited in UnitId order.
    let units: Vec<_> = world
        .resource::<UnitIndex>()
        .0
        .iter()
        .map(|(id, e)| (*id, *e))
        .collect();
    for (id, e) in units {
        if world.get::<PlatformRef>(e).is_some() {
            continue;
        }
        let Some(mut k) = world.get_mut::<Kinematics>(e) else {
            continue;
        };
        let here = k.pos();
        let jitter = (rng(seed, RngPurpose::Probe, u64::from(id.0), tick, 0) - 0.5) * 0.02;
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
    world.insert_resource(probe);
}
