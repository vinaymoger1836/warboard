//! ECS components (doc 02 §2): movement, fuel, groups, tasks and bases (P3),
//! emission control, satellites and scripted targets (P4), per-station
//! magazines, countermeasures, base stock and weapons in flight (P5).
//!
//! Components refer to each other by [`UnitId`] only, never by bevy `Entity`,
//! so a loaded world is identical to the saved one whatever the archetype
//! order (see `snapshot.rs`).

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use wb_content::compiled::{
    BaseIx, CountermeasureIx, LoadoutIx, NationIx, PlatformIx, SystemIx, WeaponIx,
};
use wb_content::source::Emcon;
use wb_content::source::realism::{BaseComponentKind, BaseKind, Subsystem};
use wb_content::source::system::ComponentRole;
use wb_content::source::world::LaunchDoctrine;
use wb_core::geo::LatLon;
use wb_core::{ContactId, UnitId};

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

/// Side index (into `Sides`) and nation of a platform, group or base.
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Allegiance {
    pub side: u8,
    pub nation: NationIx,
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

/// Rounds of one weapon on one station: `full` is the configured load (what
/// a rearm tops up to), `count` what is left.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StationLoad {
    /// Index into the platform's `stations`.
    pub station: u32,
    pub weapon: WeaponIx,
    pub full: u32,
    pub count: u32,
}

/// Weapons carried, per station (doc 03 §6). `loadout` names the template
/// the configuration came from; `None` for a custom or system magazine.
#[derive(Component, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Magazine {
    pub loadout: Option<LoadoutIx>,
    /// In (station, weapon) order.
    pub stations: Vec<StationLoad>,
}

impl Magazine {
    /// A full magazine from `(station, weapon, count)` items; entries for the
    /// same station and weapon merge.
    pub fn full(
        loadout: Option<LoadoutIx>,
        items: impl IntoIterator<Item = (u32, WeaponIx, u32)>,
    ) -> Self {
        let mut stations: Vec<StationLoad> = Vec::new();
        for (station, weapon, n) in items {
            match stations
                .iter_mut()
                .find(|s| s.station == station && s.weapon == weapon)
            {
                Some(s) => {
                    s.full += n;
                    s.count += n;
                }
                None => stations.push(StationLoad {
                    station,
                    weapon,
                    full: n,
                    count: n,
                }),
            }
        }
        stations.retain(|s| s.full > 0);
        stations.sort_by_key(|s| (s.station, s.weapon));
        Self { loadout, stations }
    }

    pub fn remaining(&self) -> u32 {
        self.stations.iter().map(|s| s.count).sum()
    }

    /// Rounds when full, so an unarmed unit is never "winchester".
    pub fn capacity(&self) -> u32 {
        self.stations.iter().map(|s| s.full).sum()
    }

    /// Rounds left per weapon, merged, in weapon order.
    pub fn rounds(&self) -> Vec<(WeaponIx, u32)> {
        let mut out: BTreeMap<WeaponIx, u32> = BTreeMap::new();
        for s in &self.stations {
            *out.entry(s.weapon).or_default() += s.count;
        }
        out.into_iter().collect()
    }
}

/// Chaff and flares left (doc 02 §11), from the platform; refilled at
/// turnaround. `(countermeasure, full, count)`.
#[derive(Component, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Countermeasures(pub Vec<(CountermeasureIx, u32, u32)>);

impl Countermeasures {
    pub fn full(p: &wb_content::compiled::Platform) -> Option<Self> {
        (!p.countermeasures.is_empty()).then(|| {
            Self(
                p.countermeasures
                    .iter()
                    .map(|(c, n)| (*c, *n, *n))
                    .collect(),
            )
        })
    }

    pub fn refill(&mut self) {
        for c in &mut self.0 {
            c.2 = c.1;
        }
    }
}

/// Membership of a group; slot 0 leads.
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GroupMember {
    pub group: UnitId,
    pub slot: u8,
}

/// The role of a component within a composite system (e.g. C2, FireControl, Launcher).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemMemberRole(pub ComponentRole);

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
    /// Chase a contact's estimate at max speed and engage it (pre-approved,
    /// ADR-029).
    Intercept {
        contact: ContactId,
    },
    /// Combat air patrol: a patrol circle that engages hostiles inside the
    /// commit range of its centre.
    Cap {
        centre: LatLon,
        radius_m: f64,
        commit_m: f64,
        alt_m: Option<f64>,
    },
    /// Close to launch range of a contact or point and fire (pre-approved,
    /// ADR-029); `weapon` and `count` narrow the shot. Home once fired.
    Strike {
        target: StrikeTarget,
        weapon: Option<WeaponIx>,
        count: Option<u32>,
        /// Rounds fired so far.
        fired: u32,
    },
    /// Stand-off jamming orbit (circle).
    Jam {
        centre: LatLon,
        radius_m: f64,
        alt_m: Option<f64>,
        speed: Speed,
    },
    /// Air-defence suppression (SEAD, doc 03 §5): pre-approved engagement of
    /// a radar site or contact with emitter-homing or precision weapons.
    Suppress {
        target: StrikeTarget,
        weapon: Option<WeaponIx>,
        count: Option<u32>,
        fired: u32,
    },
}

/// What a strike is aimed at.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum StrikeTarget {
    Contact(ContactId),
    Point(LatLon),
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
                | Task::Intercept { .. }
                | Task::Cap { .. }
                | Task::Strike { .. }
                | Task::Jam { .. }
                | Task::Suppress { .. }
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
            Task::Intercept { .. } => "intercept",
            Task::Cap { .. } => "cap",
            Task::Strike { .. } => "strike",
            Task::Jam { .. } => "jam",
            Task::Suppress { .. } => "suppress",
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
    /// A member was damaged: the mission is aborted (doc 02 §13).
    Damaged,
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

/// What an aware aircraft does about a weapon coming at it (doc 03 §5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum ThreatReaction {
    /// Hold course; countermeasures only.
    None,
    /// Turn to put the weapon on the beam, then chaff and flares.
    #[default]
    Beam,
}

/// Standing behaviours (doc 03 §5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Behaviour {
    pub bingo_rtb: bool,
    pub winchester_rtb: bool,
    /// After a bingo or winchester return, turn around and resume the task.
    pub resume_after_turnaround: bool,
    pub threat_reaction: ThreatReaction,
}

impl Default for Behaviour {
    fn default() -> Self {
        Self {
            bingo_rtb: true,
            winchester_rtb: true,
            resume_after_turnaround: true,
            threat_reaction: ThreatReaction::Beam,
        }
    }
}

/// An aware group turning away from a weapon until `until_ms` (set by the
/// weapons stage, read by the tasks stage).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Evade {
    pub heading: f64,
    pub until_ms: u64,
}

/// Launch doctrine of a group, overriding its nation's (doc 03 §5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoctrineOverride {
    pub launch: Option<LaunchDoctrine>,
    pub shots_per_target: Option<u32>,
    /// Rounds fired together at one target (a strike salvo).
    pub salvo: Option<u32>,
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
    pub evade: Option<Evade>,
    pub doctrine: DoctrineOverride,
    /// When C2 is lost, autonomous mode activates after this sim time (ms).
    pub autonomous_after_ms: Option<u64>,
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
    /// Munitions in store (not on aircraft or ships), by weapon.
    pub stock: BTreeMap<WeaponIx, u32>,
    /// Damaged components and when their repair ends, ms (doc 02 §13).
    pub damaged: BTreeMap<BaseComponentKind, u64>,
}

impl BaseSite {
    pub fn runway_cut(&self) -> bool {
        self.damaged.contains_key(&BaseComponentKind::Runway)
    }
}

/// Rules of engagement (doc 03 §7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum Roe {
    /// Weapons free.
    Alpha,
    /// Weapons tight: own territory and return fire.
    Bravo,
    /// Weapons hold: self-defence only; everything else needs approval.
    Charlie,
}

/// ROE override on a group or a unit; `None` inherits (unit → group → side).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoeSetting {
    pub roe: Option<Roe>,
    pub self_defence: Option<bool>,
}

/// Damage state of a platform (doc 02 §13): damage points for ships and
/// ground units, `damaged` for aircraft, knocked-out ship subsystems, and
/// when a ship sinks.
#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Health {
    pub points: f64,
    pub max: f64,
    pub damaged: bool,
    pub out: Vec<Subsystem>,
    pub sink_at_ms: Option<u64>,
}

impl Health {
    pub fn has_out(&self, s: Subsystem) -> bool {
        self.out.contains(&s)
    }
}

/// Emission control of a platform's radars (doc 02 §9, doc 03 §5 sites).
/// `On` emits, `Off` is silent (invisible to ESM, blind on radar), `Blink`
/// emits `emcon_blink_on_s` of every `emcon_blink_period_s`.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmconState(pub Emcon);

/// Operating mode of an electronic warfare jammer (doc 02 §9).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum JammerMode {
    #[default]
    SelfProtect,
    Escort,
    StandOff,
}

/// Jammer state of a platform (doc 02 §9): `active` whether emitting jamming,
/// and the active `mode`.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JammerState {
    pub active: bool,
    pub mode: JammerMode,
}

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

/// What a weapon was fired at.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum WeaponTarget {
    /// A contact of the shooter's side; `key` is its truth key (the target's
    /// group or the target), engine-internal.
    Track { contact: ContactId, key: UnitId },
    /// Coordinates (GPS/INS weapons, strikes on a point).
    Point { at: LatLon, alt_m: f64 },
    /// A truth unit, with perfect midcourse updates: tests and calibration
    /// only, never from an order.
    Truth(UnitId),
}

/// Energy phase of a weapon in flight (doc 02 §10).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum FlightPhase {
    Boost,
    Sustain,
    Coast,
    Cruise,
    Glide,
}

/// Terminal seeker state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SeekerState {
    /// Not yet within seeker range of the aimpoint, or no seeker at all.
    Off,
    /// Looking for a target; `lost_ms` is when it reached the aimpoint (or
    /// lost its lock) without one, for the self-destruct timer.
    Searching { lost_ms: Option<u64> },
    /// Homing on a truth target. `memory`: an anti-radiation seeker whose
    /// emitter went silent flies to where it last heard it.
    Locked { unit: UnitId, memory: bool },
}

/// Where midcourse guidance flies: a position estimate with its velocity at
/// `at_ms`, dead-reckoned to the current time.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AimPoint {
    pub at: LatLon,
    pub alt_m: f64,
    pub heading: f64,
    pub speed_mps: f64,
    pub climb_mps: f64,
    pub at_ms: u64,
}

/// A weapon in flight (doc 02 §2, §10). Its entity also has `Identity`,
/// `Allegiance` (the shooter's) and `Kinematics`.
#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WeaponFlight {
    pub weapon: WeaponIx,
    pub shooter: UnitId,
    pub salvo: u32,
    /// `M-n` / `INT-n` number, per side.
    pub tag: u32,
    pub target: WeaponTarget,
    pub aim: AimPoint,
    pub phase: FlightPhase,
    pub seeker: SeekerState,
    /// Flight-path angle, radians, climb positive.
    pub fpa: f64,
    pub launch_ms: u64,
    /// Distance from the shooter to the aimpoint at launch.
    pub launch_range_m: f64,
    /// Speed at launch and the burnout (or cruise) speed, m/s.
    pub v0_mps: f64,
    pub vmax_mps: f64,
    /// Ground distance flown.
    pub flown_m: f64,
    /// Speed lost to manoeuvring so far, m/s.
    pub manoeuvre_loss_mps: f64,
    /// Fired at a weapon (an interceptor, `INT-n`).
    pub intercept: bool,
    /// Countermeasure rounds the target dispensed against this weapon.
    pub cm_used: Vec<(CountermeasureIx, u32)>,
    /// The target knew of the weapon (picture or warning receiver) at some
    /// point in its flight.
    pub target_aware: bool,
}
