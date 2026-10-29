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
    Satellite(SatelliteDetails),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TaskingView {
    pub centre: [f64; 2],
    pub radius_m: f64,
    /// Seconds until the pass.
    pub pass_in_s: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SatelliteDetails {
    pub id: u32,
    pub name: String,
    pub type_name: String,
    /// Country code, `USA`.
    pub owner: String,
    /// `optical`, `sar` or `sigint`.
    pub payload: String,
    /// `[OPTICAL 600km]`.
    pub payload_label: String,
    pub swath_km: f64,
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub alt_m: f64,
    pub taskings: Vec<TaskingView>,
    pub confidence: String,
}

/// One source of a contact, as the contact card lists it (doc 10 §7).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ContactSourceView {
    /// `Radar`, `ESM`, `Satellite imagery` …
    pub kind: String,
    /// Own unit (or satellite) whose sensor reported.
    pub unit: String,
    pub sensor: Option<String>,
    /// Data link the report came over; none means a C2 report.
    pub network: Option<String>,
    pub age_s: f64,
}

/// What the side believes about one contact: estimates only (ADR-003,
/// ADR-026). Nothing here comes from the truth except through the contact.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ContactDetails {
    pub contact_id: u32,
    /// `S-400`, `Su-35S`, `corvette`; empty below class.
    pub label: String,
    /// Below `Precise` or stale: "(our estimate)".
    pub estimated: bool,
    /// `unknown`, `domain`, `class`, `type`, `precise`.
    pub level: String,
    /// `unknown`, `hostile`, `neutral`, `friendly`.
    pub identity: String,
    /// `air`, `surface`, `subsurface`, `ground`.
    pub domain: String,
    pub class_name: Option<String>,
    pub type_name: Option<String>,
    /// `fire_control`, `track`, `report`.
    pub quality: String,
    pub lat_deg: Option<f64>,
    pub lon_deg: Option<f64>,
    pub alt_m: Option<f64>,
    pub speed_mps: Option<f64>,
    pub heading_deg: Option<f64>,
    /// Semi-axes of the uncertainty ellipse now, metres.
    pub uncertainty_a_m: Option<f64>,
    pub uncertainty_b_m: Option<f64>,
    pub age_s: f64,
    pub first_seen_s_ago: f64,
    pub count_min: u16,
    pub count_max: u16,
    pub count_known: bool,
    pub bearing_only: bool,
    pub lost: bool,
    pub fixed: bool,
    /// Estimated threat range of the identified type (SAM), metres.
    pub threat_m: Option<f64>,
    pub sources: Vec<ContactSourceView>,
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
    /// Emission control of the lead: `on`, `off` or `blink` (none without radars).
    pub emcon: Option<String>,
    /// Sensor names of the platform.
    pub sensors: Vec<String>,
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
    if world.get::<crate::components::SatelliteState>(e).is_some() {
        return satellite(world, UnitId(id), e)
            .map(Details::Satellite)
            .ok_or_else(unknown);
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
        emcon: world
            .get::<crate::components::EmconState>(lead)
            .filter(|_| {
                p.sensors.iter().any(|m| {
                    db.sensors[m.sensor.0 as usize].radar.is_some()
                        || db.sensors[m.sensor.0 as usize].sonar.is_some()
                })
            })
            .map(|e| snake(e.0)),
        sensors: p
            .sensors
            .iter()
            .map(|m| db.str(db.sensors[m.sensor.0 as usize].name).to_owned())
            .collect(),
    })
}

fn satellite(world: &World, id: UnitId, e: Entity) -> Option<SatelliteDetails> {
    let rules = world.resource::<Rules>();
    let now = world.resource::<SimClock>().sim_time_ms;
    let pix = world.get::<PlatformRef>(e)?.0;
    let p = rules.platform(pix);
    let k = world.get::<Kinematics>(e)?;
    let (_, pl) = crate::satellites::payload(rules, pix)?;
    let st = world.get::<crate::components::SatelliteState>(e)?;
    let country = &rules.db.countries[world.get::<Allegiance>(e)?.country.0 as usize];
    Some(SatelliteDetails {
        id: id.0,
        name: world.get::<Identity>(e)?.name.clone(),
        type_name: rules.db.str(p.name).to_owned(),
        owner: rules.db.str(country.iso3).to_uppercase(),
        payload: format!("{:?}", pl.kind).to_lowercase(),
        payload_label: crate::view::payload_label(rules, pix).unwrap_or_default(),
        swath_km: pl.swath_m / 1_000.0,
        lat_deg: rad_to_deg(k.lat),
        lon_deg: rad_to_deg(k.lon),
        alt_m: k.alt_m,
        taskings: st
            .taskings
            .iter()
            .map(|t| TaskingView {
                centre: lonlat(t.centre),
                radius_m: t.radius_m,
                pass_in_s: t.pass_ms.saturating_sub(now) as f64 / 1_000.0,
            })
            .collect(),
        confidence: format!("{:?}", p.provenance.confidence).to_lowercase(),
    })
}

fn snake<T: std::fmt::Debug>(v: T) -> String {
    let s = format!("{v:?}");
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

/// Details of one contact in `side`'s picture. Unknown ids answer the same
/// for every side.
pub fn contact_details(world: &World, side: u8, id: u32) -> Result<ContactDetails, String> {
    use crate::picture::SidePictures;
    use wb_content::source::realism::ContactLevel;
    let unknown = || format!("Unknown contact {id}");
    let rules = world.resource::<Rules>();
    let now = world.resource::<SimClock>().sim_time_ms;
    let c = world
        .get_resource::<SidePictures>()
        .and_then(|p| p.sides.get(usize::from(side)))
        .and_then(|p| p.get(wb_core::ContactId(id)))
        .ok_or_else(unknown)?;
    let db = &rules.db;
    let class_name = (c.level >= ContactLevel::Class).then(|| c.type_ref.class_name(rules));
    let type_name =
        (c.level >= ContactLevel::Type).then(|| c.type_ref.short_name(rules).to_owned());
    let ell = c.ellipse(rules, now);
    let sources = c
        .sources
        .iter()
        .map(|s| ContactSourceView {
            kind: s.kind.as_str().to_owned(),
            unit: entity(world, s.observer)
                .and_then(|e| world.get::<Identity>(e))
                .map_or_else(String::new, |i| i.name.clone()),
            sensor: s
                .sensor
                .map(|x| db.str(db.sensors[x.0 as usize].name).to_owned()),
            network: s
                .network
                .map(|n| db.str(db.datalinks[n.0 as usize].name).to_owned()),
            age_s: now.saturating_sub(s.last_ms) as f64 / 1_000.0,
        })
        .collect();
    Ok(ContactDetails {
        contact_id: id,
        label: type_name
            .clone()
            .or_else(|| class_name.clone())
            .unwrap_or_default(),
        estimated: c.estimated(rules, now),
        level: snake(c.level),
        identity: snake(c.identity),
        domain: snake(c.domain),
        class_name,
        type_name,
        quality: snake(c.quality),
        lat_deg: c.est.map(|e| rad_to_deg(e.at.lat)),
        lon_deg: c.est.map(|e| rad_to_deg(e.at.lon)),
        alt_m: c.est.map(|e| e.alt_m),
        speed_mps: c.est.map(|e| e.speed_mps),
        heading_deg: c.est.map(|e| rad_to_deg(e.heading)),
        uncertainty_a_m: ell.map(|e| e.0),
        uncertainty_b_m: ell.map(|e| e.1),
        age_s: now.saturating_sub(c.last_ms) as f64 / 1_000.0,
        first_seen_s_ago: now.saturating_sub(c.first_ms) as f64 / 1_000.0,
        count_min: c.count.0,
        count_max: c.count.1,
        count_known: c.count_resolved,
        bearing_only: c.est.is_none(),
        lost: c.lost,
        fixed: c.fixed,
        threat_m: (c.level >= ContactLevel::Type)
            .then(|| c.type_ref.threat_m(rules))
            .flatten(),
        sources,
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
