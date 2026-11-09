//! Event log (doc 02 §15, doc 04 §7.3). Reports, alerts and the golden checks
//! are built from it. Each event names the sides that may see it.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use wb_content::source::realism::ContactLevel;
use wb_content::source::{Domain, Emcon};
use wb_core::{ContactId, UnitId};

use crate::clock::SimClock;
use crate::components::{JammerMode, ReturnWhy};
use crate::picture::SourceKind;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
// Externally tagged (`{"Landed": {...}}`): saves store events with postcard,
// which cannot read internally tagged enums.
pub enum EventKind {
    /// A group's task changed.
    TaskChanged {
        group: UnitId,
        task: String,
    },
    /// A strike or suppression ended before all its rounds were away, because
    /// no member can fire any more; `reason` says why (CLAUDE.md rule 4).
    StrikeAborted {
        group: UnitId,
        reason: String,
    },
    /// A group arrived on (or left) its station.
    StationChanged {
        group: UnitId,
        on: bool,
    },
    /// Bingo: the unit heads home; it should land with `reserve_kg`.
    BingoRtb {
        unit: UnitId,
        fuel_kg: f64,
        reserve_kg: f64,
        base: UnitId,
    },
    /// A group heads home for another reason.
    Return {
        group: UnitId,
        base: UnitId,
        why: ReturnWhy,
    },
    /// A group diverts to a tanker.
    ToTanker {
        group: UnitId,
        tanker: UnitId,
    },
    /// A receiver is full (or the tanker dry); fuel after refuelling.
    Refuelled {
        unit: UnitId,
        tanker: UnitId,
        fuel_kg: f64,
    },
    TookOff {
        unit: UnitId,
        base: UnitId,
    },
    Landed {
        unit: UnitId,
        base: UnitId,
        fuel_kg: f64,
    },
    TurnaroundDone {
        unit: UnitId,
        base: UnitId,
    },
    BaseActivated {
        base: UnitId,
    },
    /// The client should drop to ×1 or pause (doc 02 §3).
    AutoSlow {
        reason: String,
        cause: SlowCause,
    },
    /// A new contact in the side's picture; `observer` is the own unit whose
    /// sensor saw it. Carries contact data only (CLAUDE.md rule 1).
    ContactNew {
        contact: ContactId,
        domain: Domain,
        source: SourceKind,
        observer: UnitId,
    },
    /// Classification rose (doc 02 §8).
    ContactClassified {
        contact: ContactId,
        level: ContactLevel,
        source: SourceKind,
    },
    /// A group contact's count was resolved.
    ContactCount {
        contact: ContactId,
        min: u16,
        max: u16,
    },
    /// Not refreshed for the age-out time: now a last-known marker.
    ContactLost {
        contact: ContactId,
    },
    /// A satellite report arrived after its downlink delay (doc 02 §7.3).
    IntelReport {
        report: u32,
        satellite: UnitId,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        pass_ms: u64,
        imaged: bool,
        outcome: String,
        contacts: Vec<ContactId>,
    },
    /// A group's emission control changed.
    EmconChanged {
        group: UnitId,
        emcon: Emcon,
    },
    /// A group's jammer state changed (doc 02 §9).
    JammerChanged {
        group: UnitId,
        active: bool,
        mode: JammerMode,
    },
    /// A unit's loadout was changed at a base (`SetLoadout`); `loadout` is
    /// the template id, none for a custom one. A turnaround follows.
    LoadoutChanged {
        unit: UnitId,
        base: UnitId,
        loadout: Option<String>,
    },
    /// The base's stock could not fill a unit's stations: `short` rounds of
    /// `weapon` (content id) are missing.
    RearmShort {
        unit: UnitId,
        base: UnitId,
        weapon: String,
        short: u32,
    },
    /// A weapon left its shooter (ADR-027: the shooter's side only). `target`
    /// is the side's own contact or a point; none for test launches.
    WeaponLaunched {
        weapon: UnitId,
        shooter: UnitId,
        /// Content weapon id.
        weapon_type: String,
        /// Display name and category, for the event line (a close-in gun reads
        /// differently from a missile).
        weapon_name: String,
        category: wb_content::source::WeaponCategory,
        salvo: u32,
        tag: u32,
        target: Option<AimRef>,
        reason: crate::weapons::LaunchReason,
        /// Fired at a weapon (`INT-n`).
        intercept: bool,
    },
    /// A weapon's flight ended (ADR-027: the shooter's side only).
    WeaponTerminated {
        weapon: UnitId,
        shooter: UnitId,
        weapon_type: String,
        weapon_name: String,
        category: wb_content::source::WeaponCategory,
        tag: u32,
        /// The side's contact (or point) the weapon was fired at.
        target: Option<AimRef>,
        outcome: Outcome,
        factors: Factors,
        /// An interceptor (`INT-n`).
        intercept: bool,
    },
    /// A weapon contact judged aimed at an own unit (its predicted closest
    /// approach is inside `threat_cpa_m`).
    IncomingWeapon {
        contact: ContactId,
        unit: UnitId,
        time_to_impact_s: f64,
    },
    /// An own unit was hit (doc 02 §13). `points_pct` is what is left (ships
    /// and ground units); `subsystem` what the hit knocked out.
    Damage {
        unit: UnitId,
        points_pct: Option<f64>,
        subsystem: Option<String>,
        /// A damaged aircraft aborts its mission.
        aborted: bool,
    },
    /// An own unit was lost. `platform` is its content id, `cause` plain
    /// English from what the side knows (shot down, sunk, destroyed).
    UnitDestroyed {
        unit: UnitId,
        name: String,
        #[cfg_attr(feature = "ts", ts(optional))]
        platform: Option<String>,
        cause: String,
    },
    BaseComponentDamaged {
        base: UnitId,
        component: String,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        repaired_at_ms: u64,
    },
    BaseComponentRepaired {
        base: UnitId,
        component: String,
    },
    /// An engagement ROE does not allow, waiting for the player (doc 03 §7).
    RoeRequest {
        request: u32,
        group: UnitId,
        contact: ContactId,
        weapon_type: String,
        pk: f64,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        expires_ms: u64,
    },
    RoeDecision {
        request: u32,
        decision: Decision,
    },
    /// A hostile contact's estimate entered (or left) own territory.
    BorderCrossing {
        contact: ContactId,
        inbound: bool,
    },
    /// A satellite was tasked; `pass_ms` is the sim time of the pass.
    SatelliteTasked {
        satellite: UnitId,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        pass_ms: u64,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        report_ms: u64,
    },
    /// A composite site's C2 node was destroyed, dropping it into autonomous mode after `delay_s`.
    AutonomousMode {
        group: UnitId,
        delay_s: f64,
    },
    /// An operation was created, started, paused, resumed or ended (doc 03 §8).
    OperationChanged {
        operation: u32,
        name: String,
        state: crate::operations::OpState,
        #[cfg_attr(feature = "ts", ts(optional))]
        reason: Option<String>,
    },
    /// A phase of an operation started; `phase` is 0-based.
    PhaseStarted {
        operation: u32,
        phase: u32,
        name: String,
    },
    /// A phase ended (done or aborted) or was cancelled, and why.
    PhaseEnded {
        operation: u32,
        phase: u32,
        name: String,
        state: crate::operations::PhaseState,
        reason: String,
    },
    /// `side` ended the war (`EndWar`): the truth review unlocks (doc 03 §12).
    WarEnded {
        side: u8,
    },
    /// Truth only (no side sees it, `sides` 0): a weapon hit a unit. The truth
    /// review after the war reads it; side reports never do (CLAUDE.md rule 1).
    TruthHit {
        weapon: UnitId,
        weapon_type: String,
        shooter: UnitId,
        shooter_side: u8,
        target: UnitId,
        target_side: u8,
        target_name: String,
    },
    /// A salvo finished: counts of hits, misses, intercepts, decoys and jammed.
    SalvoReport {
        salvo: u32,
        shooter: UnitId,
        total: u32,
        hits: u32,
        misses: u32,
        intercepted: u32,
        decoyed: u32,
        jammed: u32,
    },
}

/// What a weapon was fired at, as its own side knows it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum AimRef {
    Contact { contact: ContactId },
    Point { lat_deg: f64, lon_deg: f64 },
}

/// How a weapon's flight ended (doc 02 §11).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Hit,
    Miss,
    Decoyed,
    Jammed,
    OutOfEnergy,
    Intercepted,
    LostTrack,
    SelfDestruct,
}

/// The numbers behind an outcome, so a report can explain it (CLAUDE.md
/// rule 4). `miss_distance_m` and `endgame` are set when the weapon fuzed.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Factors {
    pub speed_mps: f64,
    pub time_of_flight_s: f64,
    pub launch_range_m: f64,
    pub flown_m: f64,
    pub miss_distance_m: Option<f64>,
    pub target_aware: bool,
    pub cm_used: u32,
    pub endgame: Option<Endgame>,
}

/// The endgame roll (doc 02 §11): `pk` is the clamped product of the
/// factors, the weapon hit when `roll < pk`. CEP weapons record their radial
/// miss instead of a Pk table.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Endgame {
    pub pk_base: f64,
    pub f_energy: f64,
    pub f_aspect: f64,
    pub f_maneuver: f64,
    pub f_cm: f64,
    pub f_ew: f64,
    pub f_env: f64,
    pub pk: f64,
    pub roll: f64,
    /// 0° head-on, 90° beam, 180° tail.
    pub aspect_deg: f64,
    pub target_g: f64,
    pub cep_m: Option<f64>,
    pub radial_miss_m: Option<f64>,
}

/// Why the engine asks the client to slow down (doc 02 §3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum SlowCause {
    Test,
    HostileContact,
    LaunchAgainstUs,
    RoeRequest,
    /// A phase of an operation started (doc 03 §9).
    PhaseStart,
}

/// What happened to an approval request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Approved,
    Denied,
    Expired,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Event {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub tick: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub sim_time_ms: u64,
    /// Bit `s` set: side `s` sees it.
    pub sides: u8,
    pub kind: EventKind,
}

#[derive(Resource, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EventLog(pub Vec<Event>);

impl EventLog {
    /// Events side `side` may see, from index `from`.
    pub fn visible(&self, side: u8, from: usize) -> impl Iterator<Item = &Event> {
        self.0
            .get(from..)
            .unwrap_or_default()
            .iter()
            .filter(move |e| e.sides & (1 << side) != 0)
    }
}

/// Appends a truth-only event (no side may see it; the truth review reads
/// it after the war).
pub fn emit_truth(world: &mut World, kind: EventKind) {
    let clock = world.resource::<SimClock>();
    let ev = Event {
        tick: clock.tick,
        sim_time_ms: clock.sim_time_ms,
        sides: 0,
        kind,
    };
    world.resource_mut::<EventLog>().0.push(ev);
}

/// Appends an event at the current tick.
pub fn emit(world: &mut World, side: u8, kind: EventKind) {
    let clock = world.resource::<SimClock>();
    let ev = Event {
        tick: clock.tick,
        sim_time_ms: clock.sim_time_ms,
        sides: 1 << side,
        kind,
    };
    world.resource_mut::<EventLog>().0.push(ev);
}
