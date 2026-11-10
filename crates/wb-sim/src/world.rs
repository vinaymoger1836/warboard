//! World-level resources: sides, the `UnitId` index and id allocation.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use wb_content::compiled::NationIx;
use wb_core::UnitId;

/// First id the engine allocates itself (bases, split groups, generated
/// units). Scenario ids must stay below it.
pub const FIRST_ENGINE_ID: u32 = 1_000_000;

/// Who gives a side's orders (doc 12, ADR-044).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum Controller {
    #[default]
    Human,
    Ai,
}

/// How hard an AI side plays: reaction delay and coordination depth, never
/// extra information (doc 03 §11).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum Difficulty {
    Easy,
    #[default]
    Normal,
    Hard,
}

/// The play mode follows from the controllers (doc 12): one human against
/// the AI, one person commanding both sides, two players taking turns on one
/// device, or two AIs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum PlayMode {
    VsAi,
    #[default]
    CommandBoth,
    HotSeat,
    Watch,
}

impl PlayMode {
    /// The mode two or more human sides get when the setup does not say:
    /// one human is `VsAi`, none is `Watch`, otherwise `CommandBoth`.
    pub fn derive(controllers: &[Controller]) -> Self {
        match controllers
            .iter()
            .filter(|c| **c == Controller::Human)
            .count()
        {
            0 => PlayMode::Watch,
            1 => PlayMode::VsAi,
            _ => PlayMode::CommandBoth,
        }
    }

    /// Whether the controllers fit the mode.
    pub fn fits(self, controllers: &[Controller]) -> Result<(), String> {
        let humans = controllers
            .iter()
            .filter(|c| **c == Controller::Human)
            .count();
        let ok = match self {
            PlayMode::Watch => humans == 0,
            PlayMode::VsAi => humans == 1,
            PlayMode::CommandBoth | PlayMode::HotSeat => humans >= 2,
        };
        if ok {
            Ok(())
        } else {
            Err(format!(
                "mode {self:?} does not fit {humans} human side{}",
                if humans == 1 { "" } else { "s" }
            ))
        }
    }
}

/// One side of the war: a coalition of nations.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SideInfo {
    pub name: String,
    /// Content nation ids, e.g. `ald`.
    pub nations: Vec<String>,
    /// The side a single-player client opens on.
    pub player: bool,
    pub controller: Controller,
    /// Only meaningful for an AI side.
    pub difficulty: Difficulty,
    /// Side default ROE and self-defence (doc 03 §7).
    pub roe: crate::components::Roe,
    pub self_defence: bool,
}

#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sides {
    pub sides: Vec<SideInfo>,
    pub mode: PlayMode,
    /// Nations per side as content indices, sorted.
    pub nation_ix: Vec<Vec<NationIx>>,
}

impl Sides {
    pub fn side_of(&self, nation: NationIx) -> Option<u8> {
        self.nation_ix
            .iter()
            .position(|cs| cs.binary_search(&nation).is_ok())
            .map(|i| i as u8)
    }

    pub fn len(&self) -> usize {
        self.sides.len()
    }

    pub fn controllers(&self) -> Vec<Controller> {
        self.sides.iter().map(|s| s.controller).collect()
    }

    pub fn is_ai(&self, side: u8) -> bool {
        self.sides
            .get(usize::from(side))
            .is_some_and(|s| s.controller == Controller::Ai)
    }

    /// Sides a person controls, in order.
    pub fn human_sides(&self) -> Vec<u8> {
        (0..self.sides.len() as u8)
            .filter(|s| !self.is_ai(*s))
            .collect()
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
