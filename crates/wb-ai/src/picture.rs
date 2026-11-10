//! What the AI may know (doc 03 §11, ADR-015, ADR-046): its own side's units,
//! bases and requests, and the contacts of its own picture with their own
//! [`ContactId`]s, estimates only. The engine's adapter (`wb_sim::ai`) builds
//! this from the side's picture; nothing in it names another side's unit.
//! This crate cannot reach the engine at all: it depends on `wb-core` only.

use serde::{Deserialize, Serialize};
use wb_core::geo::LatLon;
use wb_core::{ContactId, UnitId};

/// AI difficulty: reaction delay and coordination depth, never information.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Difficulty {
    Easy,
    Normal,
    Hard,
}

/// The side's doctrine profile (content `doctrine`): weights, not rules.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Doctrine {
    /// 0–1: how readily the side goes on the offensive.
    pub aggression: f64,
    /// 0–1: how much risk it accepts for its aircraft.
    pub risk_tolerance: f64,
    /// 0–1: how strictly emitters stay off until needed.
    pub emcon_discipline: f64,
    /// Fraction of missiles held back for self-defence.
    pub missile_reserve_fraction: f64,
    /// Aircraft per CAP station.
    pub cap_aircraft: u32,
    /// Minutes a CAP group stays on station before it is relieved.
    pub cap_rotation_min: f64,
    /// Seconds the side needs to react to a new threat.
    pub reaction_time_s: f64,
    /// IADS layers, longest range first (`long`, `medium`, `short`).
    pub iads_layers: Vec<String>,
    /// Strike package template, e.g. `[sead, escort, strike, jammer]`.
    pub strike_package: Vec<String>,
}

/// What a group is good for, from its platform and what it carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Role {
    /// Aircraft with air-to-air weapons.
    Fighter,
    /// Aircraft with weapons against ships or ground targets.
    Strike,
    /// Aircraft with emitter-homing weapons.
    Sead,
    /// Stand-off jamming aircraft.
    Jammer,
    Aew,
    Tanker,
    /// Drones and other unarmed aircraft.
    Recon,
    Ship,
    /// A SAM site or battery.
    AirDefence,
    /// A radar site or vehicle.
    Radar,
    Satellite,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Domain {
    Air,
    Surface,
    Subsurface,
    Ground,
    Space,
}

/// What a group is doing now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Activity {
    /// At a base; `ready` once every member is turned round.
    AtBase {
        ready: bool,
    },
    /// Out with no task.
    Idle,
    Cap,
    Intercept,
    Strike,
    Suppress,
    Jam,
    /// Patrols, moves, tanker tracks and everything else ordered.
    Tasked,
    /// Heading home or to a tanker.
    Returning,
}

/// An own group.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OwnGroup {
    pub id: UnitId,
    pub role: Role,
    pub domain: Domain,
    pub at: LatLon,
    pub members: u32,
    pub activity: Activity,
    /// Lowest member fuel, 0–1 (1 for platforms without fuel).
    pub fuel: f64,
    /// Rounds by what they can hit.
    pub air_rounds: u32,
    pub surface_rounds: u32,
    pub ground_rounds: u32,
    /// Emitter-homing rounds.
    pub arm_rounds: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OwnBase {
    pub id: UnitId,
    pub at: LatLon,
    pub air: bool,
    pub port: bool,
}

/// A contact of the side's own picture: an estimate, never the truth.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Seen {
    pub id: ContactId,
    pub domain: Domain,
    /// Estimated position.
    pub at: LatLon,
    /// Identified hostile.
    pub hostile: bool,
    /// A weapon in flight (identified, or too fast to be anything else).
    pub weapon: bool,
    /// Estimated SAM threat radius of an identified type, metres (0: none known).
    pub threat_m: f64,
    /// The estimate lies inside the side's own territory.
    pub in_territory: bool,
    /// The contact has launched at the side's forces lately (return fire).
    pub fired_at_us: bool,
    /// Last-known marker, no longer refreshed.
    pub stale: bool,
    /// Estimated count range, high end.
    pub count_max: u16,
}

/// An open approval request of the side (doc 03 §7).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub id: u32,
    pub group: UnitId,
    pub contact: ContactId,
}

/// Everything one AI side knows at one moment.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AiPicture {
    pub side: u8,
    pub tick: u64,
    pub now_ms: u64,
    /// The war's seed, for the AI's own random draws.
    pub seed: u64,
    pub difficulty: Difficulty,
    pub doctrine: Doctrine,
    pub groups: Vec<OwnGroup>,
    pub bases: Vec<OwnBase>,
    pub contacts: Vec<Seen>,
    pub requests: Vec<Request>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Emcon {
    On,
    Off,
    Blink,
}

/// What the AI wants done. The adapter turns each into an ordinary order of
/// the side, validated like a player's.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Action {
    Cap {
        group: UnitId,
        centre: LatLon,
        radius_m: f64,
        commit_m: f64,
    },
    Intercept {
        group: UnitId,
        contact: ContactId,
    },
    Strike {
        group: UnitId,
        contact: ContactId,
    },
    Suppress {
        group: UnitId,
        contact: ContactId,
    },
    Jam {
        group: UnitId,
        centre: LatLon,
        radius_m: f64,
    },
    Rtb {
        group: UnitId,
    },
    Emcon {
        groups: Vec<UnitId>,
        emcon: Emcon,
    },
    Decide {
        request: u32,
        approve: bool,
    },
    Image {
        satellite: UnitId,
        centre: LatLon,
        radius_m: f64,
    },
}

/// The layer a decision came from (doc 03 §11).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Layer {
    Strategic,
    Operational,
    Tactical,
}

impl Layer {
    pub fn as_str(self) -> &'static str {
        match self {
            Layer::Strategic => "strategic",
            Layer::Operational => "operational",
            Layer::Tactical => "tactical",
        }
    }
}

/// One decision and why it was taken (CLAUDE.md rule 4).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Decision {
    pub layer: Layer,
    /// `None` for a decision that orders nothing (a posture change).
    pub action: Option<Action>,
    pub why: String,
}
