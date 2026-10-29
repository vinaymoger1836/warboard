//! ECS components (doc 02 §2): movement, fuel, groups, tasks and bases (P3),
//! emission control, satellites and scripted targets (P4).
//!
//! Components refer to each other by [`UnitId`] only, never by bevy `Entity`,
//! so a loaded world is identical to the saved one whatever the archetype
//! order (see `snapshot.rs`).

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use wb_content::compiled::{BaseIx, CountryIx, LoadoutIx, PlatformIx, SystemIx, WeaponIx};
use wb_content::source::Emcon;
use wb_content::source::realism::BaseKind;
use wb_core::UnitId;
use wb_core::geo::LatLon;

#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Identity {
    pub unit: UnitId,
    pub name: String,
}

/// Position and motion. Radians and SI units.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Kinematics {
    pub lat: f64,
    pub lon: f64,
    pub alt_m: f64,
    /// Radians clockwise from true north.
    pub heading: f64,
    pub speed_mps: f64,
    /// Vertical speed over the last tick.
    pub climb_mps: f64,
}

impl Kinematics {
    pub fn pos(&self) -> LatLon {
        LatLon::new(self.lat, self.lon)
    }
}

/// Side index (into `Sides`) and country of a platform, group or base.
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Allegiance {
    pub side: u8,
    pub country: CountryIx,
}

/// The content platform an entity is built from.
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlatformRef(pub PlatformIx);

/// Fuel state. Aircraft count kg; ships without a fuel mass count a fraction
/// of a full load (`max_kg` = 1).
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fuel {
    pub kg: f64,
    pub max_kg: f64,
    /// Fuel at which the unit must return (doc 02 §6), updated every second.
    pub bingo_kg: f64,
    /// Nearest suitable friendly base, refreshed every 30 s (staggered).
    pub bingo_base: Option<UnitId>,
    /// Reserve this unit should land with, recorded when bingo is called.
    pub reserve_at_bingo_kg: Option<f64>,
}

/// Weapons carried. Nothing fires before P5; the count drives winchester.
#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Magazine {
    pub loadout: Option<LoadoutIx>,
    pub rounds: Vec<(WeaponIx, u32)>,
    /// Rounds at load-out, so an unarmed unit is never "winchester".
    pub initial: u32,
}

impl Magazine {
    pub fn remaining(&self) -> u32 {
        self.rounds.iter().map(|(_, n)| n).sum()
    }
}

/// Membership of a group; slot 0 leads.
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GroupMember {
    pub group: UnitId,
    pub slot: u8,
}

/// Home base of a platform.
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BaseRef {
    pub home: Option<UnitId>,
}

/// Whether a platform is moving or stationed at a base.
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Posture {
    Moving,
    /// Landed or berthed. `ready` once the turnaround (refuel, rearm) is done.
    AtBase {
        base: UnitId,
        ready_at_ms: u64,
        ready: bool,
    },
}

/// Tanker state: fuel still available to give.
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TankerState {
    pub offload_left_kg: f64,
}

/// Where the task logic wants a lead to go this tick. Rebuilt every tick
/// before use, so it is neither saved nor hashed.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct Steer {
    pub heading: f64,
    pub speed_mps: f64,
    pub alt_m: f64,
}

/// Requested speed for a task.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum Speed {
    #[default]
    Cruise,
    Max,
}

/// A task, in engine units (radians, metres). Orders carry the degree form
/// (`orders::TaskOrder`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Task {
    /// No task: aircraft orbit where they are, ships stop.
    #[default]
    Idle,
    /// Fly or sail the waypoints, then hold at the last one.
    Move {
        waypoints: Vec<LatLon>,
        alt_m: Option<f64>,
        speed: Speed,
    },
    PatrolCircle {
        centre: LatLon,
        radius_m: f64,
        alt_m: Option<f64>,
        speed: Speed,
    },
    PatrolRoute {
        waypoints: Vec<LatLon>,
        looped: bool,
        alt_m: Option<f64>,
        speed: Speed,
    },
    Rtb {
        base: UnitId,
    },
    Land {
        base: UnitId,
    },
    /// Racetrack between two points; receivers come to the tanker.
    TankerTrack {
        a: LatLon,
        b: LatLon,
        alt_m: Option<f64>,
    },
    /// Fly to another base and make it home.
    Transfer {
        base: UnitId,
    },
}

impl Task {
    /// Tasks a group takes off (or sails) for.
    pub fn is_sortie(&self) -> bool {
        matches!(
            self,
            Task::Move { .. }
                | Task::PatrolCircle { .. }
                | Task::PatrolRoute { .. }
                | Task::TankerTrack { .. }
        )
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Task::Idle => "idle",
            Task::Move { .. } => "move",
            Task::PatrolCircle { .. } => "patrol_circle",
            Task::PatrolRoute { .. } => "patrol_route",
            Task::Rtb { .. } => "rtb",
            Task::Land { .. } => "land",
            Task::TankerTrack { .. } => "tanker_track",
            Task::Transfer { .. } => "transfer",
        }
    }
}

/// Why a group is heading home.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum ReturnWhy {
    Order,
    Bingo,
    Winchester,
    Transfer,
}

/// What a group is doing right now, on top of its task.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Mode {
    /// Executing the task.
    #[default]
    Task,
    /// Diverted by bingo to a tanker.
    ToTanker {
        tanker: UnitId,
    },
    /// In contact with the tanker, taking fuel member by member; `served`
    /// members are done.
    Refuelling {
        tanker: UnitId,
        served: u8,
    },
    Return {
        base: UnitId,
        why: ReturnWhy,
    },
    AtBase {
        base: UnitId,
    },
}

/// Standing behaviours (doc 03 §5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Behaviour {
    pub bingo_rtb: bool,
    pub winchester_rtb: bool,
    /// After a bingo or winchester return, turn around and resume the task.
    pub resume_after_turnaround: bool,
}

impl Default for Behaviour {
    fn default() -> Self {
        Self {
            bingo_rtb: true,
            winchester_rtb: true,
            resume_after_turnaround: true,
        }
    }
}

/// A group (flight, ship, squadron): carries the task; its members keep
/// formation slots behind the lead (`members[0]`).
#[derive(Component, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub members: Vec<UnitId>,
    pub task: Task,
    pub mode: Mode,
    /// Next waypoint of a route or racetrack.
    pub wp: u32,
    pub on_station: bool,
    /// Take off as soon as every member is ready (a new task while at base).
    pub launch: bool,
    pub behaviour: Behaviour,
    /// The composite system (S-400 battalion) the group was deployed as.
    pub system: Option<SystemIx>,
}

impl Group {
    pub fn lead(&self) -> Option<UnitId> {
        self.members.first().copied()
    }
}

/// A base: real (from content) or placed by the player.
#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BaseSite {
    pub content: Option<BaseIx>,
    pub kind: BaseKind,
    /// Serves aircraft and ships respectively.
    pub air: bool,
    pub port: bool,
    pub at: LatLon,
    pub elevation_m: f64,
    pub runway_m: Option<f64>,
    pub aircraft_capacity: u32,
    pub ship_berths: u32,
    /// Usable from this sim time (activation after placement).
    pub active_at_ms: u64,
    pub active: bool,
    /// Platforms stationed here, sorted.
    pub stationed: Vec<UnitId>,
}

/// Emission control of a platform's radars (doc 02 §9, doc 03 §5 sites).
/// `On` emits, `Off` is silent (invisible to ESM, blind on radar), `Blink`
/// emits `emcon_blink_on_s` of every `emcon_blink_period_s`.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmconState(pub Emcon);

/// An imaging request on a satellite (doc 02 §7.3): the pass is found when
/// the order is accepted.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tasking {
    pub centre: LatLon,
    pub radius_m: f64,
    /// Sim time of the pass (closest approach), ms.
    pub pass_ms: u64,
}

/// A satellite: position from SGP4 every tick; taskings in pass order.
#[derive(Component, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SatelliteState {
    pub taskings: Vec<Tasking>,
}

/// Test-only straight-and-level mover that carries a weapon's signature
/// (scenario `scripted_targets`; P5 replaces it with real launches).
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScriptedTarget {
    pub weapon: WeaponIx,
}
