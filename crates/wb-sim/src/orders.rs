//! Orders (doc 04 §7.2, CLAUDE.md rule 5): typed commands with the tick they
//! apply at, validated by the engine, logged, and rejected with a reason.
//!
//! The wire form ([`Order`]) uses degrees and `[lon, lat]` pairs. Validation
//! turns it into a [`Resolved`] command (unit ids expanded, bases resolved,
//! radians); the queue and the order log hold that form, and replays apply it
//! without validating again, so a replay never depends on the region packs.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use wb_content::compiled::CountryIx;
use wb_content::source::realism::BaseKind;
use wb_core::UnitId;
use wb_core::geo::{LatLon, distance_m, intermediate_point};
use wb_core::units::{km_to_m, rad_to_deg};
use wb_geo::{Geo, TerrainClass};

use crate::bases::{self, BaseInfo};
use crate::clock::SimClock;
use crate::components::{
    Allegiance, BaseSite, Behaviour, EmconState, Group, GroupMember, Identity, Kinematics, Mode,
    PlatformRef, Posture, ReturnWhy, SatelliteState, Speed, Task, Tasking,
};
use crate::events::{EventKind, emit};
use crate::placement;
use crate::rules::Rules;
use crate::world::{Sides, UnitIdAlloc, entity, spawn_unit};
use wb_content::source::{Emcon, PayloadKind};

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
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "type")]
pub enum Command {
    /// Units or groups; a part of a group splits off into a new group.
    AssignTask { units: Vec<u32>, task: TaskOrder },
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
    TestAutoSlow { reason: String },
    /// Emission control for units or groups (doc 03 §5, doc 02 §9).
    SetEmcon { units: Vec<u32>, emcon: Emcon },
    /// Image an area on the satellite's next pass (doc 03 §5 space tasks).
    TaskSatellite {
        satellite: u32,
        centre: LonLat,
        radius_km: f64,
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
    },
    PlaceBase {
        site: BaseSite,
        country: CountryIx,
        name: String,
    },
    AutoSlow {
        reason: String,
    },
    Emcon {
        groups: Vec<GroupPick>,
        emcon: Emcon,
    },
    TaskSatellite {
        satellite: UnitId,
        tasking: Tasking,
        report_ms: u64,
    },
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

/// Validates and queues an order.
pub fn submit(world: &mut World, geo: Option<&Geo>, order: &Order) -> Result<OrderAck, String> {
    let cmd = validate(world, geo, order)?;
    let note = match &cmd {
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
        _ => None,
    };
    let mut q = world.resource_mut::<OrderQueue>();
    let seq = q.next_seq;
    q.next_seq += 1;
    let queued = QueuedOrder {
        seq,
        tick: order.tick,
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
        } => Ok(Resolved::Behaviour {
            groups: pick_groups(world, side, units)?,
            bingo_rtb: *bingo_rtb,
            winchester_rtb: *winchester_rtb,
            resume_after_turnaround: *resume_after_turnaround,
        }),
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
                country: placed.country,
                name,
            })
        }
        Command::TestAutoSlow { reason } => Ok(Resolved::AutoSlow {
            reason: reason.clone(),
        }),
        Command::SetEmcon { units, emcon } => Ok(Resolved::Emcon {
            groups: pick_groups(world, side, units)?,
            emcon: *emcon,
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
    }
}

/// `[lon, lat]` degrees to a point, checking the ranges.
fn point(p: LonLat) -> Result<LatLon, String> {
    let [lon, lat] = p;
    if !(lat.is_finite() && lon.is_finite()) || !(-89.0..=89.0).contains(&lat) {
        return Err(format!("Latitude {lat} is outside ±89°"));
    }
    if !(-180.0..=180.0).contains(&lon) {
        return Err(format!("Longitude {lon} is outside ±180°"));
    }
    Ok(LatLon::from_deg(lat, lon))
}

fn fmt_point(p: LatLon) -> String {
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
fn pick_groups(world: &World, side: u8, units: &[u32]) -> Result<Vec<GroupPick>, String> {
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

fn resolve_task(
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
    };

    if surface && let Some(geo) = geo {
        check_water(&rules, geo, &bases, lead_pos, &task)?;
    }
    Ok(task)
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
                }
            }
        }
        Resolved::PlaceBase {
            site,
            country,
            name,
        } => {
            let id = world.resource_mut::<UnitIdAlloc>().take();
            let active = site.active;
            spawn_unit(
                world,
                id,
                (
                    Identity { unit: id, name },
                    Allegiance { side, country },
                    site,
                ),
            );
            if active {
                emit(world, side, EventKind::BaseActivated { base: id });
            }
        }
        Resolved::AutoSlow { reason } => emit(world, side, EventKind::AutoSlow { reason }),
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
    }
}

/// The group entity the pick applies to: the group itself when it names
/// every member, otherwise a new group split off from it. `None` if the group
/// or its members are gone (destroyed since the order was given).
fn split(world: &mut World, side: u8, pick: &GroupPick) -> Option<Entity> {
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
    let country = world.get::<Allegiance>(ge)?.country;
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
    };
    Some(spawn_unit(
        world,
        id,
        (
            Identity { unit: id, name },
            Allegiance { side, country },
            new,
        ),
    ))
}

fn reslot(world: &mut World, group: UnitId, members: &[UnitId]) {
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
