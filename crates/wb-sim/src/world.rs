//! World-level resources: sides, the `UnitId` index and id allocation.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use wb_content::compiled::CountryIx;
use wb_core::UnitId;

/// First id the engine allocates itself (bases, split groups, generated
/// units). Scenario ids must stay below it.
pub const FIRST_ENGINE_ID: u32 = 1_000_000;

/// One side of the war: a coalition of countries.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SideInfo {
    pub name: String,
    /// Content country ids, e.g. `pol`.
    pub countries: Vec<String>,
    pub player: bool,
}

#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sides {
    pub sides: Vec<SideInfo>,
    /// Countries per side as content indices, sorted.
    pub country_ix: Vec<Vec<CountryIx>>,
}

impl Sides {
    pub fn side_of(&self, country: CountryIx) -> Option<u8> {
        self.country_ix
            .iter()
            .position(|cs| cs.binary_search(&country).is_ok())
            .map(|i| i as u8)
    }

    pub fn len(&self) -> usize {
        self.sides.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sides.is_empty()
    }
}

/// `UnitId` → entity. Derived from the world, rebuilt on load; ordered, so
/// iterating it visits units in `UnitId` order.
#[derive(Resource, Clone, Debug, Default)]
pub struct UnitIndex(pub BTreeMap<UnitId, Entity>);

impl UnitIndex {
    pub fn get(&self, id: UnitId) -> Option<Entity> {
        self.0.get(&id).copied()
    }
}

/// Hands out new ids in sequence. Saved, so ids continue after a load.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnitIdAlloc {
    pub next: u32,
}

impl UnitIdAlloc {
    pub fn take(&mut self) -> UnitId {
        let id = UnitId(self.next);
        self.next += 1;
        id
    }
}

/// Benchmark-only: run every tick at the fine length (doc 02 §16 budget is
/// per 100 ms tick). Not gameplay.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ForceFineTick(pub bool);

/// Spawns `bundle` for `id` and records it in the index.
pub fn spawn_unit(world: &mut World, id: UnitId, bundle: impl Bundle) -> Entity {
    let e = world.spawn(bundle).id();
    world.resource_mut::<UnitIndex>().0.insert(id, e);
    e
}

pub fn entity(world: &World, id: UnitId) -> Option<Entity> {
    world.resource::<UnitIndex>().get(id)
}
