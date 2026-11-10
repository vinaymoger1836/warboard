//! Orders (doc 04 §7.2, CLAUDE.md rule 5): typed commands with the tick they
//! apply at, validated by the engine, logged, and rejected with a reason.
//!
//! The wire form ([`Order`]) uses degrees and `[lon, lat]` pairs. Validation
//! turns it into a [`Resolved`] command (unit ids expanded, bases resolved,
//! radians); the queue and the order log hold that form, and replays apply it
//! without validating again, so a replay never depends on the region packs.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use wb_content::compiled::{LoadoutItem, LoadoutIx, NationIx, WeaponIx};
use wb_content::source::realism::BaseKind;
use wb_content::source::world::LaunchDoctrine;
use wb_core::geo::{LatLon, distance_m, intermediate_point};
use wb_core::units::{km_to_m, rad_to_deg};
use wb_core::{ContactId, UnitId};
use wb_geo::{Geo, TerrainClass};

use crate::bases::{self, BaseInfo};
use crate::clock::SimClock;
use crate::components::{
    Allegiance, BaseSite, Behaviour, EmconState, Group, GroupMember, Identity, JammerMode,
    JammerState, Kinematics, Magazine, Mode, PlatformRef, Posture, ReturnWhy, Roe, RoeSetting,
    SatelliteState, Speed, StrikeTarget, Task, Tasking, ThreatReaction,
};
use crate::events::{EventKind, emit};
use crate::placement;
use crate::rules::Rules;
use crate::world::{Sides, UnitIdAlloc, entity, spawn_unit};
use wb_content::source::{Domain, Emcon, PayloadKind, StationKind};

/// `[lon, lat]` in degrees (doc 04 §7.2).
pub type LonLat = [f64; 2];

/// Most waypoints one task accepts.
const MAX_WAYPOINTS: usize = 64;
/// Ship legs are checked for land every this many metres.
const WATER_SAMPLE_M: f64 = 1_000.0;

/// A base named by unit id or by content id (scenarios).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(untagged)]
pub enum BaseTarget {
    Id(u32),
    Content(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskOrder {
    Move {
        waypoints: Vec<LonLat>,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        alt_m: Option<f64>,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        speed: Option<Speed>,
    },
    PatrolCircle {
        centre: LonLat,
        radius_km: f64,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        alt_m: Option<f64>,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        speed: Option<Speed>,
    },
    PatrolRoute {
        waypoints: Vec<LonLat>,
        #[serde(rename = "loop", default = "yes")]
        looped: bool,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        alt_m: Option<f64>,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        speed: Option<Speed>,
    },
    Rtb {
        /// Nearest suitable base if absent.
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        base: Option<BaseTarget>,
    },
    Land {
        base: BaseTarget,
    },
    TankerTrack {
        a: LonLat,
        b: LonLat,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        alt_m: Option<f64>,
    },
    Transfer {
        base: BaseTarget,
    },
    /// Chase a contact and engage it (pre-approved, ADR-029).
    Intercept {
        contact: u32,
    },
    /// Combat air patrol: patrol circle, engage hostiles inside the commit
    /// range of its centre.
    Cap {
        centre: LonLat,
        radius_km: f64,
        commit_km: f64,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        alt_m: Option<f64>,
    },
    /// Attack a contact or a point (pre-approved, ADR-029); `weapon` and
    /// `count` narrow the shot.
    Strike {
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        contact: Option<u32>,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        point: Option<LonLat>,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        weapon: Option<String>,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        count: Option<u32>,
    },
    /// Stand-off jamming orbit (circle).
    Jam {
        centre: LonLat,
        radius_km: f64,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        alt_m: Option<f64>,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        speed: Option<Speed>,
    },
    /// Air-defence suppression (SEAD, doc 03 §5).
    Suppress {
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        contact: Option<u32>,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        point: Option<LonLat>,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        weapon: Option<String>,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        count: Option<u32>,
    },
}

fn yes() -> bool {
    true
}

/// An ROE to set, or back to the inherited one (doc 03 §7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum RoeChoice {
    Alpha,
    Bravo,
    Charlie,
    Inherit,
}

impl RoeChoice {
    fn roe(self) -> Option<Roe> {
        match self {
            RoeChoice::Alpha => Some(Roe::Alpha),
            RoeChoice::Bravo => Some(Roe::Bravo),
            RoeChoice::Charlie => Some(Roe::Charlie),
            RoeChoice::Inherit => None,
        }
    }
}

/// Weapons to fit at a base (doc 03 §6): a loadout template of the
/// platform, or stations filled by hand.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LoadoutChoice {
    /// Content loadout id.
    Template {
        id: String,
    },
    Custom {
        stations: Vec<StationItem>,
    },
}

/// Rounds of a weapon on a station, by content ids.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct StationItem {
    pub station: String,
    pub weapon: String,
    pub count: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "type")]
pub enum Command {
    /// Units or groups; a part of a group splits off into a new group.
    AssignTask {
        units: Vec<u32>,
        task: TaskOrder,
    },
    SetBehaviour {
        units: Vec<u32>,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        bingo_rtb: Option<bool>,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        winchester_rtb: Option<bool>,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        resume_after_turnaround: Option<bool>,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        threat_reaction: Option<ThreatReaction>,
    },
    PlaceBase {
        kind: BaseKind,
        lat_deg: f64,
        lon_deg: f64,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        name: Option<String>,
    },
    /// Raises an `AutoSlow` event; exercises the client plumbing (tests).
    TestAutoSlow {
        reason: String,
    },
    /// Destroys a unit; exercises casualty handling and IADS autonomous mode (tests).
    TestDestroyUnit {
        unit: u32,
    },
    /// Emission control for units or groups (doc 03 §5, doc 02 §9).
    SetEmcon {
        units: Vec<u32>,
        emcon: Emcon,
    },
    /// Electronic warfare jammer control (doc 02 §9).
    SetJammer {
        units: Vec<u32>,
        on: bool,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        mode: Option<JammerMode>,
    },
    /// Image an area on the satellite's next pass (doc 03 §5 space tasks).
    TaskSatellite {
        satellite: u32,
        centre: LonLat,
        radius_km: f64,
    },
    /// Change the weapons of units at a base, drawn from its stock; a
    /// turnaround follows (doc 03 §6).
    SetLoadout {
        units: Vec<u32>,
        loadout: LoadoutChoice,
    },
    /// ROE of the side (no `units`), or of groups and units (doc 03 §7).
    SetRoe {
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        units: Option<Vec<u32>>,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        roe: Option<RoeChoice>,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        self_defence: Option<bool>,
    },
    /// Allows exactly one engagement of a queued request.
    ApproveEngagement {
        request: u32,
    },
    DenyEngagement {
        request: u32,
    },
    /// A new operation (doc 03 §8), state planned; `replace` swaps out a
    /// planned one (the planner's edit). The operation's id is the order's
    /// sequence number (the acknowledgement's `seq`).
    CreateOperation {
        operation: crate::operations::OperationSpec,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        replace: Option<u32>,
        /// Start it as soon as it is created (scenarios, "create and start").
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        start: Option<bool>,
    },
    StartOperation {
        operation: u32,
    },
    PauseOperation {
        operation: u32,
    },
    ResumeOperation {
        operation: u32,
    },
    /// Running phases abort (aircraft home, ships stop), waiting ones are cancelled.
    AbortOperation {
        operation: u32,
    },
    /// The player's Go for a phase with a manual trigger; `phase` is 0-based.
    PhaseGo {
        operation: u32,
        phase: u32,
    },
    /// Ends a running phase now (a manual end).
    EndPhase {
        operation: u32,
        phase: u32,
    },
    /// Ends the war: the truth review unlocks (doc 03 §12–13).
    EndWar,
    /// Deploy yourself (doc 03 §2): station `count` of a platform or system
    /// of your inventory at one of your preset bases, with the clock held.
    /// Takes effect when accepted. Aircraft and ships need a suitable base
    /// with room; sites and ground forces are set up beside the base.
    DeployForce {
        /// Content id of the platform or system.
        force: String,
        count: u32,
        base: BaseTarget,
        /// Content loadout id; the platform's default when absent.
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        loadout: Option<String>,
    },
    /// The side has stationed everything it wants to; when every deploying
    /// side has finished, the clock may run.
    FinishDeployment,
    /// Launch doctrine of groups, overriding their nation's (doc 03 §5).
    SetDoctrine {
        units: Vec<u32>,
        #[serde(default)]
        #[cfg_attr(
            feature = "ts",
            ts(optional, type = "\"max_range\" | \"no_escape_zone\" | \"close\"")
        )]
        launch: Option<LaunchDoctrine>,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        shots_per_target: Option<u32>,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        salvo: Option<u32>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Order {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub tick: u64,
    pub side: u8,
    #[serde(flatten)]
    pub command: Command,
}

/// A validated command, as queued, logged and replayed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Resolved {
    Assign {
        groups: Vec<GroupPick>,
        task: Task,
    },
    Behaviour {
        groups: Vec<GroupPick>,
        bingo_rtb: Option<bool>,
        winchester_rtb: Option<bool>,
        resume_after_turnaround: Option<bool>,
        threat_reaction: Option<ThreatReaction>,
    },
    PlaceBase {
        site: BaseSite,
        nation: NationIx,
        name: String,
    },
    AutoSlow {
        reason: String,
    },
    TestDestroyUnit {
        unit: UnitId,
    },
    Emcon {
        groups: Vec<GroupPick>,
        emcon: Emcon,
    },
    Jammer {
        groups: Vec<GroupPick>,
        on: bool,
        mode: Option<JammerMode>,
    },
    TaskSatellite {
        satellite: UnitId,
        tasking: Tasking,
        report_ms: u64,
    },
    /// Each unit's new magazine, full; `template` is the content id.
    Loadout {
        units: Vec<(UnitId, Magazine)>,
        template: Option<String>,
    },
    /// `targets` empty: the side default.
    Roe {
        targets: Vec<UnitId>,
        roe: Option<RoeChoice>,
        self_defence: Option<bool>,
    },
    Decide {
        request: u32,
        approve: bool,
    },
    Doctrine {
        groups: Vec<GroupPick>,
        launch: Option<LaunchDoctrine>,
        shots_per_target: Option<u32>,
        salvo: Option<u32>,
    },
    Operation(crate::operations::OpCommand),
    EndWar,
    /// Applied when accepted (ADR-046), never queued.
    Deploy {
        force: wb_content::compiled::ForceIx,
        nation: NationIx,
        count: u32,
        base: UnitId,
        loadout: Option<LoadoutIx>,
    },
    FinishDeploy,
}

/// Members of one group an order applies to; all members means the group
/// itself, fewer means they split off.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GroupPick {
    pub group: UnitId,
    pub members: Vec<UnitId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QueuedOrder {
    pub seq: u64,
    pub tick: u64,
    /// The tick it was submitted before: a replay queues it again then.
    pub submitted: u64,
    pub side: u8,
    pub cmd: Resolved,
}

/// Accepted orders not yet applied, in (tick, seq) order.
#[derive(Resource, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct OrderQueue {
    pub pending: Vec<QueuedOrder>,
    pub next_seq: u64,
}

/// Every accepted order since war start (doc 04 §7.1 command log).
#[derive(Resource, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct OrderLog(pub Vec<QueuedOrder>);

/// What `submit` returns on success.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct OrderAck {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub seq: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub tick: u64,
    /// A plain-English note for the player, e.g. the next satellite pass.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Where an order comes from (doc 12, ADR-044). Only the engine's own AI
/// adapter submits as `Ai`; clients are always `Client`, and a client may not
/// command an AI-controlled side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// A person at a client.
    Client,
    /// A scenario's scripted orders (tests, goldens, quick starts).
    Scenario,
    /// The AI of the order's side.
    Ai,
}

/// Why an order was refused, for tests and the umpire's log. Clients only
/// ever see [`Rejection::reason`], which reads the same for an unknown id and
/// for another side's id, so a rejection is no oracle for enemy ids
/// (rule 1, ADR-026).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RejectKind {
    /// Not valid for some other reason; the text says why.
    Invalid,
    /// Names a unit, group, base or satellite of another side.
    OtherSideUnit(u32),
    /// Names a contact id that exists only in another side's picture.
    OtherSideContact(u32),
    /// Names another side's approval request.
    OtherSideRequest(u32),
    /// Names another side's operation.
    OtherSideOperation(u32),
    /// The side is controlled by the AI.
    AiControlled,
}

impl RejectKind {
    /// Names something that belongs to another side.
    pub fn is_other_side(&self) -> bool {
        matches!(
            self,
            RejectKind::OtherSideUnit(_)
                | RejectKind::OtherSideContact(_)
                | RejectKind::OtherSideRequest(_)
                | RejectKind::OtherSideOperation(_)
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rejection {
    /// Plain English, safe to show the player.
    pub reason: String,
    pub kind: RejectKind,
}

/// Validates and queues a client order; the reason if rejected.
pub fn submit(world: &mut World, geo: Option<&Geo>, order: &Order) -> Result<OrderAck, String> {
    submit_from(world, geo, order, Origin::Client).map_err(|r| r.reason)
}

/// Like [`submit`], with the origin and a typed rejection.
pub fn submit_from(
    world: &mut World,
    geo: Option<&Geo>,
    order: &Order,
    origin: Origin,
) -> Result<OrderAck, Rejection> {
    let sides = world.resource::<Sides>();
    if origin == Origin::Client && usize::from(order.side) < sides.len() && sides.is_ai(order.side)
    {
        return Err(Rejection {
            reason: format!(
                "Order rejected: {} is controlled by the AI",
                sides.sides[usize::from(order.side)].name
            ),
            kind: RejectKind::AiControlled,
        });
    }
    let mut cmd = match validate(world, geo, order) {
        Ok(c) => c,
        Err(reason) => {
            let kind = classify(world, order);
            return Err(Rejection { reason, kind });
        }
    };
    let seq = world.resource::<OrderQueue>().next_seq;
    if let Resolved::Operation(crate::operations::OpCommand::Create { op, .. }) = &mut cmd {
        op.id = seq as u32;
    }
    let note = match &cmd {
        Resolved::Operation(crate::operations::OpCommand::Start { op }) => {
            let ops = world.resource::<crate::operations::Operations>();
            ops.get(*op).map(|o| {
                let n = o.phases.len();
                format!(
                    "{} starts with {n} phase{}",
                    o.name,
                    if n == 1 { "" } else { "s" }
                )
            })
        }
        Resolved::TaskSatellite {
            tasking, report_ms, ..
        } => {
            let start = world.resource::<SimClock>().start_utc_ms;
            Some(format!(
                "Next pass {}, report by {}",
                fmt_utc(start + tasking.pass_ms as i64),
                fmt_utc(start + *report_ms as i64)
            ))
        }
        // A strike that cannot fire at once says why (CLAUDE.md rule 4).
        Resolved::Assign { groups, task } => groups.first().and_then(|pick| {
            let rules = world.resource::<Rules>().clone();
            let g = world.get::<Group>(entity(world, pick.group)?)?;
            crate::engage::strike_status(world, &rules, order.side, g, task)
        }),
        _ => None,
    };
    let submitted = world.resource::<SimClock>().tick;
    // Deployment orders take effect now: the clock is held, so no tick would
    // apply them. They are logged but not queued.
    if matches!(cmd, Resolved::Deploy { .. } | Resolved::FinishDeploy) {
        let queued = QueuedOrder {
            seq,
            tick: order.tick,
            submitted,
            side: order.side,
            cmd: cmd.clone(),
        };
        world.resource_mut::<OrderQueue>().next_seq = seq + 1;
        apply(world, order.side, cmd);
        world.resource_mut::<OrderLog>().0.push(queued);
        return Ok(OrderAck {
            seq,
            tick: order.tick,
            note: None,
        });
    }
    let mut q = world.resource_mut::<OrderQueue>();
    q.next_seq = seq + 1;
    let queued = QueuedOrder {
        seq,
        tick: order.tick,
        submitted,
        side: order.side,
        cmd,
    };
    q.pending.push(queued.clone());
    q.pending.sort_by_key(|o| (o.tick, o.seq));
    world.resource_mut::<OrderLog>().0.push(queued);
    Ok(OrderAck {
        seq,
        tick: order.tick,
        note,
    })
}

/// Works out, after a rejection, whether the order named something of another
/// side. The player never sees the answer (the text is the same for unknown
/// ids); tests and the order log do.
fn classify(world: &World, order: &Order) -> RejectKind {
    let side = order.side;
    let unit = |raw: u32| -> Option<RejectKind> {
        let e = entity(world, UnitId(raw))?;
        let owner = world.get::<Allegiance>(e)?.side;
        (owner != side).then_some(RejectKind::OtherSideUnit(raw))
    };
    let units = |ids: &[u32]| ids.iter().find_map(|&r| unit(r));
    let contact = |raw: u32| -> Option<RejectKind> {
        let pics = world.resource::<crate::picture::SidePictures>();
        let own = pics
            .sides
            .get(usize::from(side))
            .is_some_and(|p| p.get(ContactId(raw)).is_some());
        let other = pics
            .sides
            .iter()
            .enumerate()
            .any(|(i, p)| i != usize::from(side) && p.get(ContactId(raw)).is_some());
        (!own && other).then_some(RejectKind::OtherSideContact(raw))
    };
    let base = |b: &BaseTarget| match b {
        BaseTarget::Id(id) => unit(*id),
        BaseTarget::Content(_) => None,
    };
    let task = |t: &TaskOrder| match t {
        TaskOrder::Rtb { base: Some(b) } => base(b),
        TaskOrder::Land { base: b } | TaskOrder::Transfer { base: b } => base(b),
        TaskOrder::Intercept { contact: c } => contact(*c),
        TaskOrder::Strike {
            contact: Some(c), ..
        }
        | TaskOrder::Suppress {
            contact: Some(c), ..
        } => contact(*c),
        _ => None,
    };
    let found = match &order.command {
        Command::AssignTask { units: u, task: t } => units(u).or_else(|| task(t)),
        Command::SetBehaviour { units: u, .. }
        | Command::SetEmcon { units: u, .. }
        | Command::SetJammer { units: u, .. }
        | Command::SetLoadout { units: u, .. }
        | Command::SetDoctrine { units: u, .. } => units(u),
        Command::SetRoe { units: Some(u), .. } => units(u),
        Command::TestDestroyUnit { unit: u } => unit(*u),
        Command::DeployForce { base: b, .. } => base(b),
        Command::TaskSatellite { satellite, .. } => unit(*satellite),
        Command::ApproveEngagement { request } | Command::DenyEngagement { request } => world
            .resource::<crate::roe::RoeRequests>()
            .open
            .iter()
            .any(|r| r.id == *request && r.side != side)
            .then_some(RejectKind::OtherSideRequest(*request)),
        Command::StartOperation { operation }
        | Command::PauseOperation { operation }
        | Command::ResumeOperation { operation }
        | Command::AbortOperation { operation }
        | Command::PhaseGo { operation, .. }
        | Command::EndPhase { operation, .. } => world
            .resource::<crate::operations::Operations>()
            .get(*operation)
            .is_some_and(|o| o.side != side)
            .then_some(RejectKind::OtherSideOperation(*operation)),
        Command::CreateOperation { operation, .. } => operation
            .phases
            .iter()
            .flat_map(|p| &p.tasks)
            .find_map(|t| units(&t.units).or_else(|| task(&t.task))),
        _ => None,
    };
    found.unwrap_or(RejectKind::Invalid)
}

/// `DD HH:MMZ` (day of month and UTC time): passes are at most two days ahead.
fn fmt_utc(ms: i64) -> String {
    let s = ms.div_euclid(1_000);
    let day = s.div_euclid(86_400);
    let (h, m) = (s.rem_euclid(86_400) / 3_600, s.rem_euclid(3_600) / 60);
    // Day of month from the civil date.
    let z = day + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    format!("{d:02} {h:02}:{m:02}Z")
}

fn validate(world: &mut World, geo: Option<&Geo>, order: &Order) -> Result<Resolved, String> {
    let now_tick = world.resource::<SimClock>().tick;
    if order.tick < now_tick {
        return Err(format!(
            "Order is for tick {}, which has already run (now {now_tick})",
            order.tick
        ));
    }
    let n_sides = world.resource::<Sides>().len();
    if usize::from(order.side) >= n_sides {
        return Err(format!("There is no side {}", order.side));
    }
    let side = order.side;
    match &order.command {
        Command::AssignTask { units, task } => {
            let groups = pick_groups(world, side, units)?;
            let task = resolve_task(world, geo, side, &groups, task)?;
            Ok(Resolved::Assign { groups, task })
        }
        Command::SetBehaviour {
            units,
            bingo_rtb,
            winchester_rtb,
            resume_after_turnaround,
            threat_reaction,
        } => Ok(Resolved::Behaviour {
            groups: pick_groups(world, side, units)?,
            bingo_rtb: *bingo_rtb,
            winchester_rtb: *winchester_rtb,
            resume_after_turnaround: *resume_after_turnaround,
            threat_reaction: *threat_reaction,
        }),
        Command::SetRoe {
            units,
            roe,
            self_defence,
        } => {
            if roe.is_none() && self_defence.is_none() {
                return Err("Nothing to change: give an ROE or self-defence".to_owned());
            }
            let targets = match units {
                None => {
                    if *roe == Some(RoeChoice::Inherit) {
                        return Err("The side default cannot inherit; choose A, B or C".to_owned());
                    }
                    Vec::new()
                }
                Some(ids) => {
                    if ids.is_empty() {
                        return Err("No units selected".to_owned());
                    }
                    let mut out = Vec::new();
                    for &raw in ids {
                        let unknown = || format!("Unknown unit {raw}");
                        let e = entity(world, UnitId(raw)).ok_or_else(unknown)?;
                        if world.get::<Allegiance>(e).map(|a| a.side) != Some(side)
                            || (world.get::<Group>(e).is_none()
                                && world.get::<GroupMember>(e).is_none())
                        {
                            return Err(unknown());
                        }
                        out.push(UnitId(raw));
                    }
                    out.sort();
                    out.dedup();
                    out
                }
            };
            Ok(Resolved::Roe {
                targets,
                roe: *roe,
                self_defence: *self_defence,
            })
        }
        Command::ApproveEngagement { request } | Command::DenyEngagement { request } => {
            let open = world
                .resource::<crate::roe::RoeRequests>()
                .open
                .iter()
                .any(|r| r.id == *request && r.side == side && !r.approved);
            if !open {
                return Err(format!("Request {request} is no longer open"));
            }
            Ok(Resolved::Decide {
                request: *request,
                approve: matches!(order.command, Command::ApproveEngagement { .. }),
            })
        }
        Command::SetDoctrine {
            units,
            launch,
            shots_per_target,
            salvo,
        } => {
            if shots_per_target.is_some_and(|n| !(1..=8).contains(&n))
                || salvo.is_some_and(|n| !(1..=64).contains(&n))
            {
                return Err("Shots per target must be 1–8 and a salvo 1–64".to_owned());
            }
            Ok(Resolved::Doctrine {
                groups: pick_groups(world, side, units)?,
                launch: *launch,
                shots_per_target: *shots_per_target,
                salvo: *salvo,
            })
        }
        Command::DeployForce {
            force,
            count,
            base,
            loadout,
        } => crate::deployment::validate(world, side, force, *count, base, loadout.as_deref()),
        Command::FinishDeployment => {
            if world
                .resource::<crate::deployment::Deployment>()
                .active
                .contains(&side)
            {
                Ok(Resolved::FinishDeploy)
            } else {
                Err("Your side is not deploying".to_owned())
            }
        }
        Command::CreateOperation {
            operation,
            replace,
            start,
        } => {
            let op = crate::operations::build(world, geo, side, 0, operation)?;
            let cmd = crate::operations::OpCommand::Create {
                op: Box::new(op),
                replace: *replace,
                start: start.unwrap_or(false),
            };
            crate::operations::validate(world, side, &cmd)?;
            Ok(Resolved::Operation(cmd))
        }
        Command::StartOperation { operation }
        | Command::PauseOperation { operation }
        | Command::ResumeOperation { operation }
        | Command::AbortOperation { operation } => {
            use crate::operations::OpCommand as C;
            let op = *operation;
            let cmd = match &order.command {
                Command::StartOperation { .. } => C::Start { op },
                Command::PauseOperation { .. } => C::Pause { op },
                Command::ResumeOperation { .. } => C::Resume { op },
                _ => C::Abort { op },
            };
            crate::operations::validate(world, side, &cmd)?;
            Ok(Resolved::Operation(cmd))
        }
        Command::PhaseGo { operation, phase } | Command::EndPhase { operation, phase } => {
            use crate::operations::OpCommand as C;
            let (op, phase) = (*operation, *phase);
            let cmd = if matches!(order.command, Command::PhaseGo { .. }) {
                C::Go { op, phase }
            } else {
                C::EndPhase { op, phase }
            };
            crate::operations::validate(world, side, &cmd)?;
            Ok(Resolved::Operation(cmd))
        }
        Command::EndWar => {
            if world.resource::<crate::reports::War>().ended.is_some() {
                return Err("The war has already ended".to_owned());
            }
            Ok(Resolved::EndWar)
        }
        Command::PlaceBase {
            kind,
            lat_deg,
            lon_deg,
            name,
        } => {
            let at = point([*lon_deg, *lat_deg])?;
            let rules = world.resource::<Rules>().clone();
            let sides = world.resource::<Sides>().clone();
            let now = world.resource::<SimClock>().sim_time_ms;
            let active = bases::list(world);
            let placed = placement::validate(&rules, &sides, side, *kind, at, &active, geo, now)?;
            let queued = world.resource::<OrderQueue>().pending.iter().any(|o| {
                matches!(&o.cmd, Resolved::PlaceBase { site, .. }
                    if site.content.is_some() && site.content == placed.site.content)
            });
            if let (true, Some(real)) = (queued, &placed.real_name) {
                return Err(format!("{real} is already one of your bases"));
            }
            let name = name
                .clone()
                .or(placed.real_name)
                .unwrap_or_else(|| bases::kind_name(*kind).to_owned());
            Ok(Resolved::PlaceBase {
                site: placed.site,
                nation: placed.nation,
                name,
            })
        }
        Command::TestAutoSlow { reason } => Ok(Resolved::AutoSlow {
            reason: reason.clone(),
        }),
        Command::TestDestroyUnit { unit } => Ok(Resolved::TestDestroyUnit {
            unit: UnitId(*unit),
        }),
        Command::SetEmcon { units, emcon } => Ok(Resolved::Emcon {
            groups: pick_groups(world, side, units)?,
            emcon: *emcon,
        }),
        Command::SetJammer { units, on, mode } => Ok(Resolved::Jammer {
            groups: pick_groups(world, side, units)?,
            on: *on,
            mode: *mode,
        }),
        Command::TaskSatellite {
            satellite,
            centre,
            radius_km,
        } => {
            let unknown = || format!("Unknown satellite {satellite}");
            let e = entity(world, UnitId(*satellite)).ok_or_else(unknown)?;
            if world.get::<Allegiance>(e).map(|a| a.side) != Some(side)
                || world.get::<SatelliteState>(e).is_none()
            {
                return Err(unknown());
            }
            let rules = world.resource::<Rules>().clone();
            let pix = world.get::<PlatformRef>(e).ok_or_else(unknown)?.0;
            let name = world
                .get::<Identity>(e)
                .map_or(String::new(), |i| i.name.clone());
            let (_, pl) = crate::satellites::payload(&rules, pix).ok_or_else(unknown)?;
            if pl.kind == PayloadKind::Sigint {
                return Err(format!(
                    "{name} is a SIGINT satellite: it reports emitters on every pass and takes no imaging tasks"
                ));
            }
            let at = point(*centre)?;
            if !radius_km.is_finite() || *radius_km <= 0.0 || km_to_m(*radius_km) > pl.swath_m / 2.0
            {
                return Err(format!(
                    "Area radius must be between 0 and {:.0} km (half of {name}'s strip)",
                    pl.swath_m / 2_000.0
                ));
            }
            let clock = world.resource::<SimClock>();
            let from_ms = clock.sim_time_ms
                + order.tick.saturating_sub(clock.tick) * u64::from(clock.tick_len.ms());
            let from_utc = clock.start_utc_ms as f64 + from_ms as f64;
            let horizon_h = rules.realism().satellites.tasking_horizon_s / 3_600.0;
            let pass_utc =
                crate::satellites::next_pass(&rules, pix, at, km_to_m(*radius_km), from_utc)
                    .ok_or_else(|| {
                        format!("{name} has no pass over that area in the next {horizon_h:.0} h")
                    })?;
            let pass_ms = (pass_utc - clock.start_utc_ms as f64).max(0.0) as u64;
            let report_ms = pass_ms + (crate::satellites::downlink_s(&rules, pl) * 1_000.0) as u64;
            Ok(Resolved::TaskSatellite {
                satellite: UnitId(*satellite),
                tasking: Tasking {
                    centre: at,
                    radius_m: km_to_m(*radius_km),
                    pass_ms,
                },
                report_ms,
            })
        }
        Command::SetLoadout { units, loadout } => {
            let units = resolve_loadout(world, side, units, loadout)?;
            let template = match loadout {
                LoadoutChoice::Template { id } => Some(id.clone()),
                LoadoutChoice::Custom { .. } => None,
            };
            Ok(Resolved::Loadout { units, template })
        }
    }
}

/// Checks a loadout change for every unit picked, in this order: the unit is
/// at a base, the loadout fits its stations, VLS cells are only changed at a
/// base that reloads them, and the base's stock covers the rounds added
/// (rounds taken off go back to stock).
fn resolve_loadout(
    world: &World,
    side: u8,
    units: &[u32],
    choice: &LoadoutChoice,
) -> Result<Vec<(UnitId, Magazine)>, String> {
    let rules = world.resource::<Rules>();
    let db = &rules.db;
    let mut members: Vec<UnitId> = pick_groups(world, side, units)?
        .into_iter()
        .flat_map(|p| p.members)
        .collect();
    members.sort();
    members.dedup();
    // Base -> weapon -> rounds drawn (negative: returned).
    let mut net: BTreeMap<UnitId, BTreeMap<WeaponIx, i64>> = BTreeMap::new();
    let mut out = Vec::new();
    for m in members {
        let e = entity(world, m).ok_or("Unknown unit")?;
        let name = world.get::<Identity>(e).map_or("", |i| i.name.as_str());
        let pix = world
            .get::<PlatformRef>(e)
            .ok_or_else(|| format!("{name} carries no weapons"))?
            .0;
        let p = rules.platform(pix);
        let Some(Posture::AtBase { base, .. }) = world.get::<Posture>(e).copied() else {
            return Err(match p.domain {
                Domain::Air => format!("Loadout locked: {name} is airborne"),
                Domain::Surface | Domain::Subsurface => format!("Loadout locked: {name} is at sea"),
                Domain::Ground | Domain::Space => {
                    format!("Loadout locked: {name} is not at a base")
                }
            });
        };
        let new = match choice {
            LoadoutChoice::Template { id } => {
                let lix = db
                    .loadouts
                    .iter()
                    .position(|l| db.str(l.id) == id)
                    .map(|i| LoadoutIx(i as u32))
                    .ok_or_else(|| format!("Unknown loadout `{id}`"))?;
                let lo = &db.loadouts[lix.0 as usize];
                if lo.platform != pix {
                    return Err(format!(
                        "{} is not a loadout for the {}",
                        db.str(lo.name),
                        db.str(p.short_name)
                    ));
                }
                db.check_loadout(pix, &lo.items)?;
                bases::template_magazine(rules, lix)
            }
            LoadoutChoice::Custom { stations } => {
                let items = stations
                    .iter()
                    .map(|s| {
                        let station = p
                            .stations
                            .iter()
                            .position(|st| db.str(st.id) == s.station)
                            .ok_or_else(|| {
                                format!(
                                    "The {} has no station `{}`",
                                    db.str(p.short_name),
                                    s.station
                                )
                            })?;
                        let weapon = db
                            .weapons
                            .iter()
                            .position(|w| db.str(w.id) == s.weapon)
                            .ok_or_else(|| format!("Unknown weapon `{}`", s.weapon))?;
                        Ok(LoadoutItem {
                            station: station as u32,
                            weapon: WeaponIx(weapon as u32),
                            count: s.count,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                db.check_loadout(pix, &items)?;
                Magazine::full(None, items.iter().map(|i| (i.station, i.weapon, i.count)))
            }
        };
        let old = world.get::<Magazine>(e).cloned().unwrap_or_default();
        let be = entity(world, base).ok_or("Unknown base")?;
        let site = world.get::<BaseSite>(be).ok_or("Unknown base")?;
        let base_name = world.get::<Identity>(be).map_or("", |i| i.name.as_str());
        let vls_changed = p.stations.iter().enumerate().any(|(i, st)| {
            let on = |m: &Magazine, full: bool| -> Vec<(WeaponIx, u32)> {
                m.stations
                    .iter()
                    .filter(|s| s.station == i as u32)
                    .map(|s| (s.weapon, if full { s.full } else { s.count }))
                    .filter(|(_, n)| *n > 0)
                    .collect()
            };
            st.kind == StationKind::Vls && on(&old, false) != on(&new, true)
        });
        if vls_changed && !bases::vls_reload(rules, site.kind) {
            return Err(format!(
                "{base_name} cannot reload VLS cells; {name} needs a naval base"
            ));
        }
        let n = net.entry(base).or_default();
        for s in &old.stations {
            *n.entry(s.weapon).or_default() -= i64::from(s.count);
        }
        for s in &new.stations {
            *n.entry(s.weapon).or_default() += i64::from(s.full);
        }
        out.push((m, new));
    }
    for (base, weapons) in net {
        let be = entity(world, base).ok_or("Unknown base")?;
        let site = world.get::<BaseSite>(be).ok_or("Unknown base")?;
        let base_name = world.get::<Identity>(be).map_or("", |i| i.name.as_str());
        for (w, need) in weapons {
            let have = i64::from(site.stock.get(&w).copied().unwrap_or(0));
            if need > have {
                return Err(format!(
                    "Not enough {} at {base_name}: {need} needed, {have} in stock",
                    db.str(db.weapons[w.0 as usize].name)
                ));
            }
        }
    }
    Ok(out)
}

/// `[lon, lat]` degrees to a point, checking the ranges.
pub(crate) fn point(p: LonLat) -> Result<LatLon, String> {
    let [lon, lat] = p;
    if !(lat.is_finite() && lon.is_finite()) || !(-89.0..=89.0).contains(&lat) {
        return Err(format!("Latitude {lat} is outside ±89°"));
    }
    if !(-180.0..=180.0).contains(&lon) {
        return Err(format!("Longitude {lon} is outside ±180°"));
    }
    Ok(LatLon::from_deg(lat, lon))
}

pub(crate) fn fmt_point(p: LatLon) -> String {
    let (lat, lon) = (rad_to_deg(p.lat), rad_to_deg(p.lon));
    format!(
        "{:.2}°{} {:.2}°{}",
        lat.abs(),
        if lat >= 0.0 { 'N' } else { 'S' },
        lon.abs(),
        if lon >= 0.0 { 'E' } else { 'W' }
    )
}

/// Expands unit and group ids of `side` into group picks, in `UnitId` order.
/// Unknown and enemy ids get the same message, so the reply reveals nothing.
pub(crate) fn pick_groups(
    world: &World,
    side: u8,
    units: &[u32],
) -> Result<Vec<GroupPick>, String> {
    if units.is_empty() {
        return Err("No units selected".to_owned());
    }
    if units.len() > 256 {
        return Err("Too many units in one order (256 at most)".to_owned());
    }
    let mut picks: Vec<GroupPick> = Vec::new();
    for &raw in units {
        let id = UnitId(raw);
        let unknown = || format!("Unknown unit {raw}");
        let e = entity(world, id).ok_or_else(unknown)?;
        if world.get::<Allegiance>(e).map(|a| a.side) != Some(side) {
            return Err(unknown());
        }
        let (gid, members) = if let Some(g) = world.get::<Group>(e) {
            (id, g.members.clone())
        } else if let Some(m) = world.get::<GroupMember>(e) {
            (m.group, vec![id])
        } else {
            let name = world.get::<Identity>(e).map_or("", |i| i.name.as_str());
            return Err(format!("{name} cannot take a task"));
        };
        match picks.iter_mut().find(|p| p.group == gid) {
            Some(p) => p.members.extend(members),
            None => picks.push(GroupPick {
                group: gid,
                members,
            }),
        }
    }
    for p in &mut picks {
        p.members.sort();
        p.members.dedup();
    }
    picks.sort_by_key(|p| p.group);
    Ok(picks)
}

pub(crate) fn resolve_task(
    world: &mut World,
    geo: Option<&Geo>,
    side: u8,
    picks: &[GroupPick],
    order: &TaskOrder,
) -> Result<Task, String> {
    let rules = world.resource::<Rules>().clone();
    let mut platforms = Vec::new();
    let mut lead_pos = None;
    for p in picks {
        for m in &p.members {
            let e = entity(world, *m).ok_or("Unknown unit")?;
            platforms.push(world.get::<PlatformRef>(e).ok_or("Not a platform")?.0);
            if lead_pos.is_none() {
                lead_pos = world.get::<Kinematics>(e).map(|k| k.pos());
            }
        }
    }
    platforms.sort();
    platforms.dedup();
    let plats: Vec<_> = platforms.iter().map(|ix| rules.platform(*ix)).collect();
    let air = plats.iter().all(|p| rules.is_air(p));
    let surface = plats.iter().all(|p| !rules.is_air(p));
    if !air && !surface {
        return Err("Aircraft and ships cannot share one task".to_owned());
    }
    let check_alt = |alt: Option<f64>| -> Result<Option<f64>, String> {
        let Some(a) = alt else { return Ok(None) };
        if !air {
            return Ok(None);
        }
        if !a.is_finite() || a < 30.0 {
            return Err(format!("Altitude {a} m is too low (30 m at least)"));
        }
        for p in &plats {
            let c = rules.ceiling_m(p);
            if a > c {
                return Err(format!(
                    "Altitude {a:.0} m is above the {} ceiling ({c:.0} m)",
                    rules.db.str(p.name)
                ));
            }
        }
        Ok(Some(a))
    };
    let waypoints = |w: &[LonLat], min: usize| -> Result<Vec<LatLon>, String> {
        if w.len() < min {
            return Err(format!("A route needs at least {min} waypoints"));
        }
        if w.len() > MAX_WAYPOINTS {
            return Err(format!("A route takes at most {MAX_WAYPOINTS} waypoints"));
        }
        w.iter().map(|p| point(*p)).collect()
    };
    let bases = bases::list(world);
    let base = |t: &BaseTarget| -> Result<&BaseInfo, String> {
        let found = match t {
            BaseTarget::Id(id) => bases.iter().find(|b| b.id == UnitId(*id)),
            BaseTarget::Content(cid) => bases.iter().find(|b| {
                b.side == side
                    && b.content
                        .is_some_and(|ix| rules.db.str(rules.db.bases[ix.0 as usize].id) == cid)
            }),
        };
        let b = found
            .filter(|b| b.side == side)
            .ok_or_else(|| "Unknown base".to_owned())?;
        for p in &plats {
            bases::suitable(&rules, b, p)?;
        }
        Ok(b)
    };

    let task = match order {
        TaskOrder::Move {
            waypoints: w,
            alt_m,
            speed,
        } => Task::Move {
            waypoints: waypoints(w, 1)?,
            alt_m: check_alt(*alt_m)?,
            speed: speed.unwrap_or_default(),
        },
        TaskOrder::PatrolCircle {
            centre,
            radius_km,
            alt_m,
            speed,
        } => {
            if !(1.0..=1_000.0).contains(radius_km) {
                return Err(format!(
                    "Patrol radius must be 1–1000 km, got {radius_km} km"
                ));
            }
            Task::PatrolCircle {
                centre: point(*centre)?,
                radius_m: km_to_m(*radius_km),
                alt_m: check_alt(*alt_m)?,
                speed: speed.unwrap_or_default(),
            }
        }
        TaskOrder::PatrolRoute {
            waypoints: w,
            looped,
            alt_m,
            speed,
        } => Task::PatrolRoute {
            waypoints: waypoints(w, 2)?,
            looped: *looped,
            alt_m: check_alt(*alt_m)?,
            speed: speed.unwrap_or_default(),
        },
        TaskOrder::TankerTrack { a, b, alt_m } => {
            if let Some(p) = plats.iter().find(|p| p.tanker.is_none()) {
                return Err(format!(
                    "Only tankers fly a tanker track; the {} is not one",
                    rules.db.str(p.name)
                ));
            }
            let (a, b) = (point(*a)?, point(*b)?);
            if distance_m(a, b) < 10_000.0 {
                return Err("Tanker track ends must be at least 10 km apart".to_owned());
            }
            Task::TankerTrack {
                a,
                b,
                alt_m: check_alt(*alt_m)?,
            }
        }
        TaskOrder::Rtb { base: Some(t) } => Task::Rtb { base: base(t)?.id },
        TaskOrder::Rtb { base: None } => {
            let at = lead_pos.ok_or("Unknown unit")?;
            let b = plats
                .first()
                .and_then(|p| bases::nearest_suitable(&rules, &bases, side, p, at, None))
                .filter(|b| plats.iter().all(|p| bases::suitable(&rules, b, p).is_ok()))
                .ok_or_else(|| "No suitable base for these units".to_owned())?;
            Task::Rtb { base: b.id }
        }
        TaskOrder::Land { base: t } => Task::Land { base: base(t)?.id },
        TaskOrder::Transfer { base: t } => {
            let b = base(t)?;
            let n = picks.iter().map(|p| p.members.len()).sum::<usize>() as u32;
            let free = plats
                .iter()
                .map(|p| bases::room(&rules, b, p))
                .min()
                .unwrap_or(0);
            if n > free {
                return Err(format!("{} has room for {free} more, not {n}", b.name));
            }
            Task::Transfer { base: b.id }
        }
        TaskOrder::Intercept { contact } => {
            let (id, domain) = own_contact(world, side, *contact)?;
            carried_for(world, &rules, picks, domain, None)?;
            Task::Intercept { contact: id }
        }
        TaskOrder::Cap {
            centre,
            radius_km,
            commit_km,
            alt_m,
        } => {
            if !air {
                return Err("Only aircraft fly a CAP".to_owned());
            }
            if !(1.0..=1_000.0).contains(radius_km) || !(1.0..=1_000.0).contains(commit_km) {
                return Err("CAP radius and commit range must be 1–1000 km".to_owned());
            }
            Task::Cap {
                centre: point(*centre)?,
                radius_m: km_to_m(*radius_km),
                commit_m: km_to_m(*commit_km),
                alt_m: check_alt(*alt_m)?,
            }
        }
        TaskOrder::Strike {
            contact,
            point: at,
            weapon,
            count,
        } => {
            let weapon = match weapon {
                Some(wid) => Some(
                    rules
                        .db
                        .weapons
                        .iter()
                        .position(|w| rules.db.str(w.id) == wid)
                        .map(|i| WeaponIx(i as u32))
                        .ok_or_else(|| format!("Unknown weapon `{wid}`"))?,
                ),
                None => None,
            };
            if count.is_some_and(|n| !(1..=64).contains(&n)) {
                return Err("A strike fires 1–64 rounds".to_owned());
            }
            let target = match (contact, at) {
                (Some(c), None) => {
                    let (id, domain) = own_contact(world, side, *c)?;
                    carried_for(world, &rules, picks, domain, weapon)?;
                    StrikeTarget::Contact(id)
                }
                (None, Some(p)) => {
                    carried_for(world, &rules, picks, Domain::Ground, weapon)
                        .or_else(|_| carried_for(world, &rules, picks, Domain::Surface, weapon))?;
                    StrikeTarget::Point(point(*p)?)
                }
                _ => return Err("A strike needs a contact or a point, not both".to_owned()),
            };
            Task::Strike {
                target,
                weapon,
                count: *count,
                fired: 0,
            }
        }
        TaskOrder::Jam {
            centre,
            radius_km,
            alt_m,
            speed,
        } => {
            if *radius_km < 1.0 {
                return Err("Orbit radius must be at least 1 km".to_owned());
            }
            Task::Jam {
                centre: point(*centre)?,
                radius_m: km_to_m(*radius_km),
                alt_m: check_alt(*alt_m)?,
                speed: speed.unwrap_or(Speed::Cruise),
            }
        }
        TaskOrder::Suppress {
            contact,
            point: at,
            weapon,
            count,
        } => {
            let weapon = match weapon {
                Some(name) => {
                    let wix = rules
                        .db
                        .weapons
                        .iter()
                        .position(|w| rules.db.str(w.id) == *name)
                        .map(|i| WeaponIx(i as u32))
                        .ok_or_else(|| format!("Unknown weapon `{name}`"))?;
                    Some(wix)
                }
                None => None,
            };
            if count.is_some_and(|n| !(1..=64).contains(&n)) {
                return Err("A suppression fires 1–64 rounds".to_owned());
            }
            let target = match (contact, at) {
                (Some(c), None) => {
                    let (id, domain) = own_contact(world, side, *c)?;
                    carried_for(world, &rules, picks, domain, weapon)?;
                    StrikeTarget::Contact(id)
                }
                (None, Some(p)) => {
                    carried_for(world, &rules, picks, Domain::Ground, weapon)
                        .or_else(|_| carried_for(world, &rules, picks, Domain::Surface, weapon))?;
                    StrikeTarget::Point(point(*p)?)
                }
                _ => {
                    return Err(
                        "Air-defence suppression needs a contact or a point, not both".to_owned(),
                    );
                }
            };
            Task::Suppress {
                target,
                weapon,
                count: *count,
                fired: 0,
            }
        }
    };

    if surface && let Some(geo) = geo {
        check_water(&rules, geo, &bases, lead_pos, &task)?;
    }
    Ok(task)
}

/// A contact of the side's picture with a position; its id and domain.
/// Unknown and other sides' ids get the same answer.
pub(crate) fn own_contact(
    world: &World,
    side: u8,
    raw: u32,
) -> Result<(ContactId, Domain), String> {
    let pics = world.resource::<crate::picture::SidePictures>();
    pics.sides
        .get(usize::from(side))
        .and_then(|p| p.get(ContactId(raw)))
        .filter(|c| c.est.is_some())
        .map(|c| (c.id, c.domain))
        .ok_or_else(|| format!("Unknown contact C{raw}"))
}

/// Some picked member carries a weapon (or the named one) for the domain.
fn carried_for(
    world: &World,
    rules: &Rules,
    picks: &[GroupPick],
    domain: Domain,
    weapon: Option<WeaponIx>,
) -> Result<(), String> {
    let ok = picks.iter().flat_map(|p| &p.members).any(|m| {
        entity(world, *m)
            .and_then(|e| world.get::<Magazine>(e))
            .is_some_and(|mag| {
                mag.stations.iter().any(|s| {
                    s.full > 0
                        && weapon.is_none_or(|w| w == s.weapon)
                        && rules.db.weapons[s.weapon.0 as usize]
                            .targets
                            .contains(&domain)
                })
            })
    });
    if ok {
        return Ok(());
    }
    let what = match domain {
        Domain::Air => "air",
        Domain::Surface => "surface",
        Domain::Subsurface => "subsurface",
        Domain::Ground => "ground",
        Domain::Space => "space",
    };
    Err(match weapon {
        Some(w) => format!(
            "No selected unit carries {} for {what} targets",
            rules.db.str(rules.db.weapons[w.0 as usize].name)
        ),
        None => format!("No selected unit carries a weapon for {what} targets"),
    })
}

/// Ships stay on water: every leg is sampled for land, except near ports and
/// the start point (doc 02 §5: port approach via the berth).
fn check_water(
    rules: &Rules,
    geo: &Geo,
    bases: &[BaseInfo],
    start: Option<LatLon>,
    task: &Task,
) -> Result<(), String> {
    let exempt_m = rules.realism().logistics.berth_radius_m * 2.0;
    let near_port = |p: LatLon| {
        bases
            .iter()
            .any(|b| b.port && distance_m(p, b.at) <= exempt_m)
            || start.is_some_and(|s| distance_m(p, s) <= exempt_m)
    };
    let land = |p: LatLon| -> Result<bool, String> {
        Ok(!near_port(p)
            && geo
                .terrain_class(p)
                .map_err(|e| format!("Cannot read the terrain: {e}"))?
                != TerrainClass::Water)
    };
    let mut legs: Vec<(LatLon, LatLon)> = Vec::new();
    match task {
        Task::Move { waypoints, .. } | Task::PatrolRoute { waypoints, .. } => {
            let mut prev = start;
            for w in waypoints {
                if let Some(p) = prev {
                    legs.push((p, *w));
                }
                prev = Some(*w);
            }
            if let (Task::PatrolRoute { looped: true, .. }, [first, .., last]) =
                (task, waypoints.as_slice())
            {
                legs.push((*last, *first));
            }
        }
        Task::PatrolCircle {
            centre, radius_m, ..
        } => {
            for i in 0..32 {
                let b = f64::from(i) * core::f64::consts::TAU / 32.0;
                let p = wb_core::geo::destination_point(*centre, b, *radius_m);
                if land(p)? {
                    return Err(format!("Patrol circle crosses land near {}", fmt_point(p)));
                }
            }
            if let Some(s) = start {
                legs.push((s, *centre));
            }
        }
        _ => {}
    }
    for (i, (a, b)) in legs.iter().enumerate() {
        let d = distance_m(*a, *b);
        let n = (d / WATER_SAMPLE_M).ceil().max(1.0) as u32;
        for k in 0..=n {
            let p = intermediate_point(*a, *b, f64::from(k) / f64::from(n));
            if land(p)? {
                return Err(format!("Leg {} crosses land near {}", i + 1, fmt_point(p)));
            }
        }
    }
    Ok(())
}

/// First stage of every tick: applies the orders due now, in submit order.
pub fn apply_due(world: &mut World) {
    let tick = world.resource::<SimClock>().tick;
    let due: Vec<QueuedOrder> = {
        let mut q = world.resource_mut::<OrderQueue>();
        let n = q.pending.iter().take_while(|o| o.tick <= tick).count();
        q.pending.drain(..n).collect()
    };
    for o in due {
        apply(world, o.side, o.cmd);
    }
}

fn apply(world: &mut World, side: u8, cmd: Resolved) {
    match cmd {
        Resolved::Assign { groups, task } => {
            for pick in groups {
                if let Some(ge) = split(world, side, &pick) {
                    assign(world, side, ge, task.clone());
                }
            }
        }
        Resolved::Behaviour {
            groups,
            bingo_rtb,
            winchester_rtb,
            resume_after_turnaround,
            threat_reaction,
        } => {
            for pick in groups {
                if let Some(ge) = split(world, side, &pick)
                    && let Some(mut g) = world.get_mut::<Group>(ge)
                {
                    let b: &mut Behaviour = &mut g.behaviour;
                    if let Some(v) = bingo_rtb {
                        b.bingo_rtb = v;
                    }
                    if let Some(v) = winchester_rtb {
                        b.winchester_rtb = v;
                    }
                    if let Some(v) = resume_after_turnaround {
                        b.resume_after_turnaround = v;
                    }
                    if let Some(v) = threat_reaction {
                        b.threat_reaction = v;
                    }
                }
            }
        }
        Resolved::PlaceBase { site, nation, name } => {
            let id = world.resource_mut::<UnitIdAlloc>().take();
            let active = site.active;
            spawn_unit(
                world,
                id,
                (
                    Identity { unit: id, name },
                    Allegiance { side, nation },
                    site,
                ),
            );
            if active {
                emit(world, side, EventKind::BaseActivated { base: id });
            }
        }
        Resolved::AutoSlow { reason } => emit(
            world,
            side,
            EventKind::AutoSlow {
                reason,
                cause: crate::events::SlowCause::Test,
            },
        ),
        Resolved::TestDestroyUnit { unit } => {
            crate::damage::destroy(world, unit);
        }
        Resolved::Emcon { groups, emcon } => {
            for pick in groups {
                for m in &pick.members {
                    if let Some(e) = entity(world, *m) {
                        world.entity_mut(e).insert(EmconState(emcon));
                    }
                }
                emit(
                    world,
                    side,
                    EventKind::EmconChanged {
                        group: pick.group,
                        emcon,
                    },
                );
            }
        }
        Resolved::Jammer { groups, on, mode } => {
            for pick in groups {
                for m in &pick.members {
                    if let Some(e) = entity(world, *m)
                        && let Some(mut st) = world.get_mut::<JammerState>(e)
                    {
                        st.active = on;
                        if let Some(md) = mode {
                            st.mode = md;
                        }
                    }
                }
                let current_mode = pick
                    .members
                    .first()
                    .and_then(|m| entity(world, *m))
                    .and_then(|e| world.get::<JammerState>(e))
                    .map_or(mode.unwrap_or_default(), |st| st.mode);
                emit(
                    world,
                    side,
                    EventKind::JammerChanged {
                        group: pick.group,
                        active: on,
                        mode: current_mode,
                    },
                );
            }
        }
        Resolved::TaskSatellite {
            satellite,
            tasking,
            report_ms,
        } => {
            if let Some(e) = entity(world, satellite)
                && let Some(mut st) = world.get_mut::<SatelliteState>(e)
            {
                let pass_ms = tasking.pass_ms;
                st.taskings.push(tasking);
                st.taskings.sort_by_key(|t| t.pass_ms);
                emit(
                    world,
                    side,
                    EventKind::SatelliteTasked {
                        satellite,
                        pass_ms,
                        report_ms,
                    },
                );
            }
        }
        Resolved::Loadout { units, template } => {
            for (unit, mag) in units {
                set_loadout(world, side, unit, mag, template.clone());
            }
        }
        Resolved::Roe {
            targets,
            roe,
            self_defence,
        } => {
            if targets.is_empty() {
                let mut sides = world.resource_mut::<Sides>();
                let s = &mut sides.sides[usize::from(side)];
                if let Some(r) = roe.and_then(RoeChoice::roe) {
                    s.roe = r;
                }
                if let Some(v) = self_defence {
                    s.self_defence = v;
                }
                return;
            }
            for t in targets {
                let Some(e) = entity(world, t) else { continue };
                let mut r = world.get::<RoeSetting>(e).copied().unwrap_or_default();
                if let Some(c) = roe {
                    r.roe = c.roe();
                }
                if self_defence.is_some() {
                    r.self_defence = self_defence;
                }
                world.entity_mut(e).insert(r);
            }
        }
        Resolved::Decide { request, approve } => crate::roe::decide(world, side, request, approve),
        Resolved::Operation(cmd) => crate::operations::apply(world, side, cmd),
        Resolved::EndWar => crate::reports::end_war(world, side),
        Resolved::Deploy {
            force,
            nation,
            count,
            base,
            loadout,
        } => crate::deployment::apply(world, side, force, nation, count, base, loadout),
        Resolved::FinishDeploy => crate::deployment::finish(world, side),
        Resolved::Doctrine {
            groups,
            launch,
            shots_per_target,
            salvo,
        } => {
            for pick in groups {
                if let Some(ge) = split(world, side, &pick)
                    && let Some(mut g) = world.get_mut::<Group>(ge)
                {
                    let d = &mut g.doctrine;
                    if launch.is_some() {
                        d.launch = launch;
                    }
                    if shots_per_target.is_some() {
                        d.shots_per_target = shots_per_target;
                    }
                    if salvo.is_some() {
                        d.salvo = salvo;
                    }
                }
            }
        }
    }
}

/// Swaps a unit's magazine at its base: the old rounds go back to stock, the
/// new ones are drawn from it, and a turnaround starts. A shortfall (another
/// order drew the stock since this one was checked) is left to the rearm at
/// the end of the turnaround, which reports it. A unit that left its base
/// since the order was checked keeps its weapons.
fn set_loadout(
    world: &mut World,
    side: u8,
    unit: UnitId,
    mut mag: Magazine,
    template: Option<String>,
) {
    let rules = world.resource::<Rules>().clone();
    let now = world.resource::<SimClock>().sim_time_ms;
    let Some(e) = entity(world, unit) else {
        return;
    };
    let Some(Posture::AtBase { base, .. }) = world.get::<Posture>(e).copied() else {
        return;
    };
    let Some(be) = entity(world, base) else {
        return;
    };
    let old = world.get::<Magazine>(e).cloned().unwrap_or_default();
    {
        let mut site = world.get_mut::<BaseSite>(be).expect("base");
        for s in &old.stations {
            bases::put_back(&mut site.stock, s.weapon, s.count);
        }
        for s in &mut mag.stations {
            s.count = bases::draw(&mut site.stock, s.weapon, s.full);
        }
    }
    let p = rules.platform(world.get::<PlatformRef>(e).expect("platform").0);
    let ready_at_ms = now + (bases::turnaround_s(&rules, p) * 1_000.0) as u64;
    world.entity_mut(e).insert((
        mag,
        Posture::AtBase {
            base,
            ready_at_ms,
            ready: false,
        },
    ));
    emit(
        world,
        side,
        EventKind::LoadoutChanged {
            unit,
            base,
            loadout: template,
        },
    );
}

/// The group entity the pick applies to: the group itself when it names
/// every member, otherwise a new group split off from it. `None` if the group
/// or its members are gone (destroyed since the order was given).
pub(crate) fn split(world: &mut World, side: u8, pick: &GroupPick) -> Option<Entity> {
    let ge = entity(world, pick.group)?;
    let g = world.get::<Group>(ge)?.clone();
    let members: Vec<UnitId> = pick
        .members
        .iter()
        .copied()
        .filter(|m| g.members.contains(m))
        .collect();
    if members.is_empty() {
        return None;
    }
    if members.len() == g.members.len() {
        return Some(ge);
    }
    let id = world.resource_mut::<UnitIdAlloc>().take();
    let name = format!(
        "{} ({})",
        world.get::<Identity>(ge).map_or("", |i| i.name.as_str()),
        members.len()
    );
    let nation = world.get::<Allegiance>(ge)?.nation;
    {
        let mut old = world.get_mut::<Group>(ge)?;
        old.members.retain(|m| !members.contains(m));
    }
    let remaining = world.get::<Group>(ge)?.members.clone();
    reslot(world, pick.group, &remaining);
    reslot(world, id, &members);
    let new = Group {
        members,
        task: g.task.clone(),
        mode: g.mode,
        wp: g.wp,
        on_station: false,
        launch: false,
        behaviour: g.behaviour,
        system: g.system,
        evade: g.evade,
        doctrine: g.doctrine,
        autonomous_after_ms: g.autonomous_after_ms,
        fires: Vec::new(),
    };
    Some(spawn_unit(
        world,
        id,
        (
            Identity { unit: id, name },
            Allegiance { side, nation },
            new,
        ),
    ))
}

pub(crate) fn reslot(world: &mut World, group: UnitId, members: &[UnitId]) {
    for (slot, m) in members.iter().enumerate() {
        if let Some(e) = entity(world, *m)
            && let Some(mut gm) = world.get_mut::<GroupMember>(e)
        {
            *gm = GroupMember {
                group,
                slot: slot as u8,
            };
        }
    }
}

/// Gives a group a new task (doc 03 §5: retasking at any time).
pub fn assign(world: &mut World, side: u8, ge: Entity, task: Task) {
    let Some(gid) = world.get::<Identity>(ge).map(|i| i.unit) else {
        return;
    };
    let Some(mut g) = world.get_mut::<Group>(ge) else {
        return;
    };
    let was_on = g.on_station;
    g.wp = 0;
    g.on_station = false;
    match g.mode {
        Mode::AtBase { base } => {
            let stays = matches!(task, Task::Rtb { base: b } | Task::Land { base: b } if b == base)
                || task == Task::Idle;
            g.launch = !stays;
            g.task = if stays { Task::Idle } else { task.clone() };
        }
        _ => {
            g.mode = match task {
                Task::Rtb { base } | Task::Land { base } => Mode::Return {
                    base,
                    why: ReturnWhy::Order,
                },
                Task::Transfer { base } => Mode::Return {
                    base,
                    why: ReturnWhy::Transfer,
                },
                _ => Mode::Task,
            };
            g.task = task.clone();
        }
    }
    let members = g.members.clone();
    if was_on {
        emit(
            world,
            side,
            EventKind::StationChanged {
                group: gid,
                on: false,
            },
        );
    }
    if matches!(task, Task::Jam { .. }) {
        for m in members {
            if let Some(e) = entity(world, m)
                && let Some(mut st) = world.get_mut::<JammerState>(e)
            {
                st.active = true;
                st.mode = JammerMode::StandOff;
            }
        }
        emit(
            world,
            side,
            EventKind::JammerChanged {
                group: gid,
                active: true,
                mode: JammerMode::StandOff,
            },
        );
    }
    emit(
        world,
        side,
        EventKind::TaskChanged {
            group: gid,
            task: task.kind().to_owned(),
        },
    );
}

/// Whether every member of a group is at base and ready (used by take-off).
pub fn all_ready(world: &World, members: &[UnitId]) -> bool {
    members.iter().all(|m| {
        entity(world, *m)
            .and_then(|e| world.get::<Posture>(e))
            .is_some_and(|p| matches!(p, Posture::AtBase { ready: true, .. }))
    })
}
