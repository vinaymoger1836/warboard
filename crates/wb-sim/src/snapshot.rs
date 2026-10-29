//! The canonical snapshot of a world: every unit's components in `UnitId`
//! order plus the simulation resources. The state hash is BLAKE3 over its
//! postcard encoding, and saves store it, so whatever a save restores is
//! exactly what the hash covers.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};

use crate::clock::{EngagementState, RngSeed, SimClock};
use crate::components::{
    Allegiance, BaseRef, BaseSite, EmconState, Fuel, Group, GroupMember, Identity, Kinematics,
    Magazine, PlatformRef, Posture, SatelliteState, ScriptedTarget, Steer, TankerState,
};
use crate::picture::SidePictures;
use crate::scenario::TestTerrain;
use crate::sensors::LosMemo;
use crate::events::EventLog;
use crate::orders::{OrderLog, OrderQueue};
use crate::probe::ProbeState;
use crate::rules::Rules;
use crate::world::{ForceFineTick, Sides, UnitIdAlloc, UnitIndex};

const MAGIC: &[u8; 8] = b"WB-STATE";
/// Bump when the snapshot layout changes: every hash changes with it.
pub const FORMAT_VERSION: u8 = 3;

/// All components of one unit, group or base.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnitRecord {
    pub identity: Identity,
    pub kinematics: Option<Kinematics>,
    pub allegiance: Option<Allegiance>,
    pub platform: Option<PlatformRef>,
    pub fuel: Option<Fuel>,
    pub magazine: Option<Magazine>,
    pub member: Option<GroupMember>,
    pub base_ref: Option<BaseRef>,
    pub posture: Option<Posture>,
    pub tanker: Option<TankerState>,
    pub group: Option<Group>,
    pub base: Option<BaseSite>,
    pub emcon: Option<EmconState>,
    pub satellite: Option<SatelliteState>,
    pub scripted: Option<ScriptedTarget>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub content_hash: [u8; 32],
    pub clock: SimClock,
    pub seed: RngSeed,
    pub sides: Sides,
    pub alloc: UnitIdAlloc,
    pub force_fine: ForceFineTick,
    pub probe: Option<ProbeState>,
    pub orders: OrderQueue,
    pub pictures: SidePictures,
    pub test_terrain: Option<TestTerrain>,
    pub units: Vec<UnitRecord>,
}

pub fn capture(world: &World) -> Snapshot {
    let units = world
        .resource::<UnitIndex>()
        .0
        .values()
        .filter_map(|&e| {
            Some(UnitRecord {
                identity: world.get::<Identity>(e)?.clone(),
                kinematics: world.get::<Kinematics>(e).copied(),
                allegiance: world.get::<Allegiance>(e).copied(),
                platform: world.get::<PlatformRef>(e).copied(),
                fuel: world.get::<Fuel>(e).copied(),
                magazine: world.get::<Magazine>(e).cloned(),
                member: world.get::<GroupMember>(e).copied(),
                base_ref: world.get::<BaseRef>(e).copied(),
                posture: world.get::<Posture>(e).copied(),
                tanker: world.get::<TankerState>(e).copied(),
                group: world.get::<Group>(e).cloned(),
                base: world.get::<BaseSite>(e).cloned(),
                emcon: world.get::<EmconState>(e).copied(),
                satellite: world.get::<SatelliteState>(e).cloned(),
                scripted: world.get::<ScriptedTarget>(e).copied(),
            })
        })
        .collect();
    Snapshot {
        content_hash: world.resource::<Rules>().content_hash,
        clock: world.resource::<SimClock>().clone(),
        seed: *world.resource::<RngSeed>(),
        sides: world.resource::<Sides>().clone(),
        alloc: *world.resource::<UnitIdAlloc>(),
        force_fine: *world.resource::<ForceFineTick>(),
        probe: world.get_resource::<ProbeState>().copied(),
        orders: world.resource::<OrderQueue>().clone(),
        pictures: world.resource::<SidePictures>().clone(),
        test_terrain: world.get_resource::<TestTerrain>().cloned(),
        units,
    }
}

/// Inserts the resources every world has; units come from the scenario or a
/// snapshot.
pub fn base_resources(world: &mut World, rules: Rules) {
    world.insert_resource(rules);
    world.insert_resource(EngagementState::default());
    world.insert_resource(OrderLog::default());
    world.insert_resource(EventLog::default());
    world.insert_resource(UnitIndex::default());
    world.insert_resource(LosMemo::default());
}

/// Builds a world from a snapshot (in `UnitId` order). The order log and
/// event log start empty; `save.rs` fills them.
pub fn restore(snap: Snapshot, rules: Rules) -> World {
    let mut world = World::new();
    base_resources(&mut world, rules);
    world.insert_resource(snap.clock);
    world.insert_resource(snap.seed);
    world.insert_resource(snap.force_fine);
    world.insert_resource(snap.sides);
    world.insert_resource(snap.alloc);
    world.insert_resource(snap.orders);
    world.insert_resource(snap.pictures);
    if let Some(t) = snap.test_terrain {
        world.insert_resource(t);
    }
    if let Some(p) = snap.probe {
        world.insert_resource(p);
    }
    for u in snap.units {
        let id = u.identity.unit;
        let mut e = world.spawn(u.identity);
        if let Some(c) = u.kinematics {
            e.insert(c);
        }
        if let Some(c) = u.allegiance {
            e.insert(c);
        }
        if let Some(c) = u.platform {
            e.insert((c, Steer::default()));
        }
        if let Some(c) = u.fuel {
            e.insert(c);
        }
        if let Some(c) = u.magazine {
            e.insert(c);
        }
        if let Some(c) = u.member {
            e.insert(c);
        }
        if let Some(c) = u.base_ref {
            e.insert(c);
        }
        if let Some(c) = u.posture {
            e.insert(c);
        }
        if let Some(c) = u.tanker {
            e.insert(c);
        }
        if let Some(c) = u.group {
            e.insert(c);
        }
        if let Some(c) = u.base {
            e.insert(c);
        }
        if let Some(c) = u.emcon {
            e.insert(c);
        }
        if let Some(c) = u.satellite {
            e.insert(c);
        }
        if let Some(c) = u.scripted {
            e.insert(c);
        }
        let ent = e.id();
        world.resource_mut::<UnitIndex>().0.insert(id, ent);
    }
    world
}

pub fn state_hash(world: &World) -> [u8; 32] {
    let bytes = postcard::to_stdvec(&capture(world)).expect("snapshot serializes");
    let mut h = blake3::Hasher::new();
    h.update(MAGIC);
    h.update(&[FORMAT_VERSION]);
    h.update(&bytes);
    *h.finalize().as_bytes()
}

pub fn to_hex(bytes: &[u8; 32]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::with_capacity(64), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}
