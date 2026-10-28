//! `details()` for the unit and base cards (doc 06 §3, doc 10 §7): full detail
//! of one own group or base, on demand. Other sides' units answer "Unknown
//! unit", the same as ids that do not exist (CLAUDE.md rule 1).

use bevy_ecs::prelude::*;
use serde::Serialize;
use wb_content::source::SensorType;
use wb_core::UnitId;
use wb_core::geo::LatLon;
use wb_core::units::rad_to_deg;

use crate::bases::kind_name;
use crate::clock::SimClock;
use crate::components::{
    Allegiance, BaseRef, BaseSite, Fuel, Group, GroupMember, Identity, Kinematics, Magazine, Mode,
    PlatformRef, Posture, Task,
};
use crate::rules::Rules;
use crate::view::status;
use crate::world::entity;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Details {
    Group(Box<GroupDetails>),
    Base(BaseDetails),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct BaseBrief {
    pub id: u32,
    pub name: String,
    pub lat_deg: f64,
    pub lon_deg: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Ranges {
    /// Radar / sensor horizon of the best sensor.
    pub sensor_m: Option<f64>,
    /// Longest weapon range in the magazine.
    pub engage_m: Option<f64>,
    /// Out and back on a full load with the bingo reserve, from home base.
    pub combat_radius_m: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TaskView {
    pub kind: String,
    /// `[lon, lat]` degrees: route, racetrack ends, or the circle centre.
    pub waypoints: Vec<[f64; 2]>,
    pub radius_m: Option<f64>,
    pub looped: bool,
    /// Next waypoint.
    pub wp: u32,
    pub alt_m: Option<f64>,
    pub base: Option<BaseBrief>,
    /// What the group does now: `task`, `to_tanker`, `refuelling`, `return`, `at_base`.
    pub mode: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct MemberView {
    pub id: u32,
    pub name: String,
    pub status: String,
    pub fuel_pct: Option<f64>,
    pub fuel_kg: Option<f64>,
    pub bingo_kg: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct WeaponCount {
    pub name: String,
    pub count: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct GroupDetails {
    pub id: u32,
    pub name: String,
    pub type_name: String,
    pub category: String,
    pub domain: String,
    pub count: u32,
    pub crew: Option<u32>,
    /// `ON STATION`, `RTB`, … (doc 10 §7).
    pub status: String,
    pub bingo: bool,
    pub task: TaskView,
    pub bingo_rtb: bool,
    pub winchester_rtb: bool,
    pub resume_after_turnaround: bool,
    /// Lowest member fuel.
    pub fuel_pct: Option<f64>,
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub speed_mps: f64,
    pub alt_m: f64,
    pub heading_deg: f64,
    pub mach: Option<f64>,
    pub home_base: Option<BaseBrief>,
    pub ranges: Ranges,
    pub members: Vec<MemberView>,
    pub loadout: Option<String>,
    pub weapons: Vec<WeaponCount>,
    /// Provenance confidence of the platform's data.
    pub confidence: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct StationedView {
    pub id: u32,
    pub group: u32,
    pub name: String,
    pub type_name: String,
    pub ready: bool,
    pub turnaround_left_s: f64,
    pub fuel_pct: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct BaseDetails {
    pub id: u32,
    pub name: String,
    pub kind_name: String,
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub elevation_m: f64,
    pub runway_m: Option<f64>,
    pub aircraft_capacity: u32,
    pub ship_berths: u32,
    pub active: bool,
    pub active_in_s: f64,
    pub stationed: Vec<StationedView>,
}

fn brief(world: &World, id: UnitId) -> Option<BaseBrief> {
    let e = entity(world, id)?;
    let b = world.get::<BaseSite>(e)?;
    Some(BaseBrief {
        id: id.0,
        name: world.get::<Identity>(e)?.name.clone(),
        lat_deg: rad_to_deg(b.at.lat),
        lon_deg: rad_to_deg(b.at.lon),
    })
}

fn lonlat(p: LatLon) -> [f64; 2] {
    [rad_to_deg(p.lon), rad_to_deg(p.lat)]
}

fn pct(f: &Fuel) -> f64 {
    (f.kg / f.max_kg.max(1e-9) * 100.0).clamp(0.0, 100.0)
}

/// Details of an own group (or the group of an own member) or base.
pub fn details(world: &World, side: u8, id: u32) -> Result<Details, String> {
    let unknown = || format!("Unknown unit {id}");
    let e = entity(world, UnitId(id)).ok_or_else(unknown)?;
    if world.get::<Allegiance>(e).map(|a| a.side) != Some(side) {
        return Err(unknown());
    }
    if world.get::<BaseSite>(e).is_some() {
        return Ok(Details::Base(base(world, UnitId(id), e)));
    }
    let gid = match world.get::<GroupMember>(e) {
        Some(m) => m.group,
        None => UnitId(id),
    };
    let ge = entity(world, gid).ok_or_else(unknown)?;
    group(world, gid, ge)
        .map(|g| Details::Group(Box::new(g)))
        .ok_or_else(unknown)
}

fn group(world: &World, gid: UnitId, ge: Entity) -> Option<GroupDetails> {
    let rules = world.resource::<Rules>();
    let g = world.get::<Group>(ge)?;
    let lead = entity(world, g.lead()?)?;
    let p = rules.platform(world.get::<PlatformRef>(lead)?.0);
    let k = *world.get::<Kinematics>(lead)?;
    let db = &rules.db;

    let members: Vec<MemberView> = g
        .members
        .iter()
        .filter_map(|m| {
            let e = entity(world, *m)?;
            let f = world.get::<Fuel>(e);
            Some(MemberView {
                id: m.0,
                name: world.get::<Identity>(e)?.name.clone(),
                status: status(g, world.get::<Posture>(e)).as_str().to_owned(),
                fuel_pct: f.map(pct),
                fuel_kg: f.map(|f| f.kg),
                bingo_kg: f.map(|f| f.bingo_kg),
            })
        })
        .collect();
    let fuel_pct = members
        .iter()
        .filter_map(|m| m.fuel_pct)
        .min_by(|a, b| a.total_cmp(b));
    let bingo = g.members.iter().any(|m| {
        entity(world, *m)
            .and_then(|e| world.get::<Fuel>(e))
            .is_some_and(|f| f.bingo_kg > 0.0 && f.kg <= f.bingo_kg)
    }) || matches!(
        g.mode,
        Mode::Return {
            why: crate::components::ReturnWhy::Bingo,
            ..
        }
    );

    let (waypoints, radius_m, looped, alt_m, base) = match &g.task {
        Task::Move {
            waypoints, alt_m, ..
        } => (
            waypoints.iter().map(|w| lonlat(*w)).collect(),
            None,
            false,
            *alt_m,
            None,
        ),
        Task::PatrolRoute {
            waypoints,
            looped,
            alt_m,
            ..
        } => (
            waypoints.iter().map(|w| lonlat(*w)).collect(),
            None,
            *looped,
            *alt_m,
            None,
        ),
        Task::PatrolCircle {
            centre,
            radius_m,
            alt_m,
            ..
        } => (vec![lonlat(*centre)], Some(*radius_m), true, *alt_m, None),
        Task::TankerTrack { a, b, alt_m } => {
            (vec![lonlat(*a), lonlat(*b)], None, true, *alt_m, None)
        }
        Task::Rtb { base } | Task::Land { base } | Task::Transfer { base } => {
            (Vec::new(), None, false, None, brief(world, *base))
        }
        Task::Idle => (Vec::new(), None, false, None, None),
    };
    let (mode, mode_base) = match g.mode {
        Mode::Task => ("task", None),
        Mode::ToTanker { .. } => ("to_tanker", None),
        Mode::Refuelling { .. } => ("refuelling", None),
        Mode::Return { base, .. } => ("return", Some(base)),
        Mode::AtBase { base } => ("at_base", Some(base)),
    };

    let sensor_m = p
        .sensors
        .iter()
        .filter_map(|m| {
            let s = &db.sensors[m.sensor.0 as usize];
            match s.sensor_type {
                SensorType::Radar => s.radar.as_ref().map(|r| r.max_range_m.unwrap_or(r.r_ref_m)),
                _ => None,
            }
        })
        .max_by(|a, b| a.total_cmp(b));
    let mag = world.get::<Magazine>(lead);
    let engage_m = mag
        .into_iter()
        .flat_map(|m| m.rounds.iter())
        .filter(|(_, n)| *n > 0)
        .map(|(w, _)| db.weapons[w.0 as usize].profile.max_range_m)
        .max_by(|a, b| a.total_cmp(b));
    let weapons = mag
        .into_iter()
        .flat_map(|m| m.rounds.iter())
        .map(|(w, n)| WeaponCount {
            name: db.str(db.weapons[w.0 as usize].name).to_owned(),
            count: *n,
        })
        .collect();
    let home = world.get::<BaseRef>(lead).and_then(|r| r.home);
    let crew = p.crew.map(|c| c * g.members.len() as u32);
    Some(GroupDetails {
        id: gid.0,
        name: world
            .get::<Identity>(world.resource::<crate::world::UnitIndex>().get(gid)?)?
            .name
            .clone(),
        type_name: db.str(p.name).to_owned(),
        category: p.category.as_str().to_owned(),
        domain: if rules.is_air(p) { "air" } else { "surface" }.to_owned(),
        count: g.members.len() as u32,
        crew,
        status: status(g, world.get::<Posture>(lead)).as_str().to_owned(),
        bingo,
        task: TaskView {
            kind: g.task.kind().to_owned(),
            waypoints,
            radius_m,
            looped,
            wp: g.wp,
            alt_m,
            base: base.or_else(|| mode_base.and_then(|b| brief(world, b))),
            mode: mode.to_owned(),
        },
        bingo_rtb: g.behaviour.bingo_rtb,
        winchester_rtb: g.behaviour.winchester_rtb,
        resume_after_turnaround: g.behaviour.resume_after_turnaround,
        fuel_pct,
        lat_deg: rad_to_deg(k.lat),
        lon_deg: rad_to_deg(k.lon),
        speed_mps: k.speed_mps,
        alt_m: k.alt_m,
        heading_deg: rad_to_deg(k.heading),
        mach: rules.is_air(p).then(|| rules.mach(k.speed_mps, k.alt_m)),
        home_base: home.and_then(|h| brief(world, h)),
        ranges: Ranges {
            sensor_m,
            engage_m,
            combat_radius_m: rules.combat_radius_m(p),
        },
        members,
        loadout: mag
            .and_then(|m| m.loadout)
            .map(|l| db.str(db.loadouts[l.0 as usize].name).to_owned()),
        weapons,
        confidence: format!("{:?}", p.provenance.confidence).to_lowercase(),
    })
}

fn base(world: &World, id: UnitId, e: Entity) -> BaseDetails {
    let rules = world.resource::<Rules>();
    let now = world.resource::<SimClock>().sim_time_ms;
    let b = world.get::<BaseSite>(e).expect("base");
    let stationed = b
        .stationed
        .iter()
        .filter_map(|u| {
            let ue = entity(world, *u)?;
            let p = rules.platform(world.get::<PlatformRef>(ue)?.0);
            let (ready, left) = match world.get::<Posture>(ue)? {
                Posture::AtBase {
                    ready, ready_at_ms, ..
                } => (*ready, ready_at_ms.saturating_sub(now) as f64 / 1_000.0),
                Posture::Moving => (false, 0.0),
            };
            Some(StationedView {
                id: u.0,
                group: world.get::<GroupMember>(ue).map_or(0, |m| m.group.0),
                name: world.get::<Identity>(ue)?.name.clone(),
                type_name: rules.db.str(p.name).to_owned(),
                ready,
                turnaround_left_s: if ready { 0.0 } else { left },
                fuel_pct: world.get::<Fuel>(ue).map(pct),
            })
        })
        .collect();
    BaseDetails {
        id: id.0,
        name: world
            .get::<Identity>(e)
            .map_or_else(String::new, |i| i.name.clone()),
        kind_name: kind_name(b.kind).to_owned(),
        lat_deg: rad_to_deg(b.at.lat),
        lon_deg: rad_to_deg(b.at.lon),
        elevation_m: b.elevation_m,
        runway_m: b.runway_m,
        aircraft_capacity: b.aircraft_capacity,
        ship_berths: b.ship_berths,
        active: b.active,
        active_in_s: b.active_at_ms.saturating_sub(now) as f64 / 1_000.0,
        stationed,
    }
}
