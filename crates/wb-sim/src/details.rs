//! `details()` for the unit and base cards (doc 06 §3, doc 10 §7): full detail
//! of one own group or base, on demand. Other sides' units answer "Unknown
//! unit", the same as ids that do not exist (CLAUDE.md rule 1).

use bevy_ecs::prelude::*;
use serde::Serialize;
use wb_content::compiled::WeaponIx;
use wb_content::source::weapon::{Midcourse, ProfileKind, Warhead};
use wb_content::source::{Domain, Seeker, SensorType, StationKind, WeaponCategory};
use wb_core::UnitId;
use wb_core::geo::LatLon;
use wb_core::units::rad_to_deg;

use crate::bases::kind_name;
use crate::clock::SimClock;
use crate::components::{
    Allegiance, BaseRef, BaseSite, Countermeasures, Fuel, Group, GroupMember, Identity, Kinematics,
    Magazine, Mode, PlatformRef, Posture, Roe, RoeSetting, Task, WeaponFlight,
};
use crate::roe::RoeSource;
use crate::rules::Rules;
use crate::view::status;
use crate::world::{UnitIndex, entity};

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
    /// Nation code, `USA`.
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
    /// Class archetype name (doc 04 §4), empty if the entry names none.
    pub class_name: String,
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
    /// `Bastra`, `HF-4 Corvan`, `corvette`; empty below class.
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
    /// Horizon of the highest radar against a surface target (doc 02 §7), when
    /// it is shorter than `sensor_m`: ships and ground units beyond it are hidden
    /// by Earth's curvature whatever the radar's range.
    pub surface_horizon_m: Option<f64>,
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

/// Rounds left of one of the lead's weapons, with what the strike panel needs to offer it
/// (doc 03 §6). Own weapons only, so nothing here is hidden from the side.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct WeaponRounds {
    /// Content id, as a strike order names it.
    pub id: String,
    pub name: String,
    pub category: WeaponCategory,
    /// Domains it can engage.
    pub targets: Vec<Domain>,
    pub min_range_m: f64,
    pub max_range_m: f64,
    /// May fire on a track another unit shares; otherwise the shooter's own
    /// sensors must hold the target.
    pub engage_on_remote: bool,
    pub count: u32,
    /// Archetype class name, e.g. `Medium-range radar air-to-air missile`.
    pub class_name: Option<String>,
    /// Flight model (doc 02 §10).
    pub profile: ProfileKind,
    /// Top speed.
    pub max_speed_mach: f64,
    pub midcourse: Vec<Midcourse>,
    pub seeker: Seeker,
    pub seeker_range_m: Option<f64>,
    pub warhead_kg: f64,
    pub warhead: Option<Warhead>,
    /// Circular error probable of satellite-navigation and unguided weapons.
    pub cep_m: Option<f64>,
    /// Altitude band of the targets it can reach, if limited.
    pub min_alt_m: Option<f64>,
    pub max_alt_m: Option<f64>,
    /// Cruise and terminal altitudes of air-breathing and sea-skimming weapons.
    pub cruise_alt_m: Option<f64>,
    pub terminal_alt_m: Option<f64>,
}

/// One weapon on one station of the lead (doc 03 §6).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct StationView {
    /// Content station id, e.g. `bay_l_in`.
    pub station: String,
    /// `internal`, `external`, `vls` or `launcher`.
    pub kind: String,
    pub weapon: String,
    pub count: u32,
    pub full: u32,
}

/// Fire channels of a ship or battery: how many engagements its fire control
/// can run at once, and how many are taken by weapons in flight (doc 02 §9).
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct FireChannels {
    pub used: u32,
    pub total: u32,
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
    /// Why an ordered strike has not fired yet (`engage::strike_status`).
    pub strike_note: Option<String>,
    /// Template name; none for a custom or system magazine.
    pub loadout: Option<String>,
    /// Rounds left per weapon, lead, with what a strike order needs.
    pub weapons: Vec<WeaponRounds>,
    /// Lead's stations.
    pub stations: Vec<StationView>,
    /// Chaff and flares left, lead.
    pub countermeasures: Vec<WeaponCount>,
    /// Fire channels in use, lead (or the battery); none for aircraft and
    /// units without weapons.
    pub fire_channels: Option<FireChannels>,
    /// Class archetype name of the lead's platform (doc 04 §4), empty if none.
    pub class_name: String,
    /// Emission control of the lead: `on`, `off` or `blink` (none without radars).
    pub emcon: Option<String>,
    /// Electronic warfare / jammer state of the lead (none without EW suites).
    pub jammer: Option<JammerView>,
    /// Sensor names of the platform.
    pub sensors: Vec<String>,
    /// Effective ROE of the lead and where it comes from (doc 03 §7).
    pub roe: Roe,
    pub roe_source: RoeSource,
    /// The group's own ROE, none when it inherits the side's.
    pub roe_override: Option<Roe>,
    /// Effective self-defence; the group's own setting, none when inherited.
    pub self_defence: bool,
    pub self_defence_override: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct JammerView {
    pub active: bool,
    pub mode: String,
    pub jam_strength: f64,
    pub coverage_km: f64,
    pub sector_deg: f64,
    pub available_modes: Vec<String>,
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
    /// Munitions in store, by weapon.
    pub stock: Vec<WeaponCount>,
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
    group(world, side, gid, ge)
        .map(|g| Details::Group(Box::new(g)))
        .ok_or_else(unknown)
}

fn group(world: &World, side: u8, gid: UnitId, ge: Entity) -> Option<GroupDetails> {
    let rules = world.resource::<Rules>();
    let g = world.get::<Group>(ge)?;
    let (roe, roe_source, self_defence) = crate::roe::effective(world, side, g.lead()?);
    let own_roe = world.get::<RoeSetting>(ge).copied().unwrap_or_default();
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
        Task::Cap {
            centre,
            radius_m,
            alt_m,
            ..
        }
        | Task::Jam {
            centre,
            radius_m,
            alt_m,
            ..
        } => (vec![lonlat(*centre)], Some(*radius_m), true, *alt_m, None),
        Task::Strike {
            target: crate::components::StrikeTarget::Point(at),
            ..
        }
        | Task::Suppress {
            target: crate::components::StrikeTarget::Point(at),
            ..
        } => (vec![lonlat(*at)], None, false, None, None),
        Task::Idle | Task::Intercept { .. } | Task::Strike { .. } | Task::Suppress { .. } => {
            (Vec::new(), None, false, None, None)
        }
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
    // Earth's curvature limits what the highest radar sees of ships and ground units.
    let surface_horizon_m = p
        .sensors
        .iter()
        .filter(|m| db.sensors[m.sensor.0 as usize].radar.is_some())
        .map(|m| m.height_m)
        .max_by(f64::total_cmp)
        .map(|h| {
            let base = if rules.is_air(p) {
                k.alt_m.max(0.0)
            } else {
                0.0
            };
            crate::sensors::horizon_m(
                rules,
                base + h,
                rules.realism().earth.surface_target_height_m,
            )
        })
        .filter(|hz| sensor_m.is_some_and(|s| *hz < s));
    let mag = world.get::<Magazine>(lead);
    let rounds = mag.map(Magazine::rounds).unwrap_or_default();
    let engage_m = rounds
        .iter()
        .filter(|(_, n)| *n > 0)
        .map(|(w, _)| db.weapons[w.0 as usize].profile.max_range_m)
        .max_by(|a, b| a.total_cmp(b));
    let weapon_name = |w: WeaponIx| db.str(db.weapons[w.0 as usize].name).to_owned();
    let weapons = rounds
        .iter()
        .map(|(w, n)| {
            let c = &db.weapons[w.0 as usize];
            WeaponRounds {
                id: db.str(c.id).to_owned(),
                name: weapon_name(*w),
                category: c.category,
                targets: c.targets.clone(),
                min_range_m: c.profile.min_range_m,
                max_range_m: c.profile.max_range_m,
                engage_on_remote: c.engage_on_remote,
                count: *n,
                class_name: c.design.class_name.map(|s| db.str(s).to_owned()),
                profile: c.profile.kind,
                max_speed_mach: c.profile.max_speed_mach,
                midcourse: c.guidance.midcourse.clone(),
                seeker: c.guidance.terminal,
                seeker_range_m: c.guidance.seeker_range_m,
                warhead_kg: c.warhead_kg,
                warhead: c.warhead,
                cep_m: c.cep_m,
                min_alt_m: c.envelope.as_ref().map(|e| e.min_alt_m),
                max_alt_m: c.envelope.as_ref().map(|e| e.max_alt_m),
                cruise_alt_m: c.profile.cruise_alt_m,
                terminal_alt_m: c.profile.terminal_alt_m,
            }
        })
        .collect();
    let stations = mag
        .into_iter()
        .flat_map(|m| m.stations.iter())
        .filter_map(|s| {
            let st = p.stations.get(s.station as usize)?;
            Some(StationView {
                station: db.str(st.id).to_owned(),
                kind: match st.kind {
                    StationKind::Internal => "internal",
                    StationKind::External => "external",
                    StationKind::Vls => "vls",
                    StationKind::Launcher => "launcher",
                }
                .to_owned(),
                weapon: weapon_name(s.weapon),
                count: s.count,
                full: s.full,
            })
        })
        .collect();
    let countermeasures = world
        .get::<Countermeasures>(lead)
        .into_iter()
        .flat_map(|c| c.0.iter())
        .map(|(c, _, n)| WeaponCount {
            name: db.str(db.countermeasures[c.0 as usize].name).to_owned(),
            count: *n,
        })
        .collect();
    // Channels are taken by every weapon in flight its shooter(s) fired.
    let fire_channels = g
        .lead()
        .filter(|_| !rules.is_air(p) && mag.is_some_and(|m| !m.stations.is_empty()))
        .map(|lead_id| {
            let shooters: Vec<UnitId> = if g.system.is_some() {
                g.members.clone()
            } else {
                vec![lead_id]
            };
            let used = world
                .resource::<UnitIndex>()
                .0
                .values()
                .filter_map(|e| world.get::<WeaponFlight>(*e))
                .filter(|f| shooters.contains(&f.shooter))
                .count() as u32;
            FireChannels {
                used,
                total: crate::engage::available_fire_channels(world, rules, lead_id, Some(g)),
            }
        });
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
            surface_horizon_m,
            combat_radius_m: rules.combat_radius_m(p),
        },
        strike_note: crate::engage::strike_status(world, rules, side, g, &g.task),
        members,
        loadout: mag
            .and_then(|m| m.loadout)
            .map(|l| db.str(db.loadouts[l.0 as usize].name).to_owned()),
        stations,
        countermeasures,
        fire_channels,
        weapons,
        class_name: p
            .design
            .class_name
            .map(|n| rules.db.str(n).to_owned())
            .unwrap_or_default(),
        emcon: world
            .get::<crate::components::EmconState>(lead)
            .filter(|_| {
                p.sensors.iter().any(|m| {
                    db.sensors[m.sensor.0 as usize].radar.is_some()
                        || db.sensors[m.sensor.0 as usize].sonar.is_some()
                })
            })
            .map(|e| snake(e.0)),
        jammer: world
            .get::<crate::components::JammerState>(lead)
            .and_then(|st| {
                let suite_ref = p.ew.first()?;
                let suite = &db.ew_suites[suite_ref.0 as usize];
                let mut available_modes = Vec::new();
                if suite.modes.self_protect.is_some() {
                    available_modes.push("self_protect".to_owned());
                }
                if suite.modes.escort.is_some() {
                    available_modes.push("escort".to_owned());
                }
                if suite.modes.stand_off.is_some() {
                    available_modes.push("stand_off".to_owned());
                }
                let mode_str = match st.mode {
                    crate::components::JammerMode::SelfProtect => "self_protect",
                    crate::components::JammerMode::Escort => "escort",
                    crate::components::JammerMode::StandOff => "stand_off",
                };
                let (strength, coverage_km, sector_deg) = match st.mode {
                    crate::components::JammerMode::SelfProtect => {
                        let m = suite.modes.self_protect.as_ref();
                        (m.map_or(0.0, |x| x.jam_strength), 0.0, 360.0)
                    }
                    crate::components::JammerMode::Escort => {
                        let m = suite.modes.escort.as_ref();
                        (
                            m.map_or(0.0, |x| x.jam_strength),
                            m.map(|x| x.coverage_m / 1000.0).unwrap_or(0.0),
                            m.map(|x| rad_to_deg(x.sector_rad)).unwrap_or(360.0),
                        )
                    }
                    crate::components::JammerMode::StandOff => {
                        let m = suite.modes.stand_off.as_ref();
                        (
                            m.map_or(0.0, |x| x.jam_strength),
                            m.map(|x| x.coverage_m / 1000.0).unwrap_or(0.0),
                            m.map(|x| rad_to_deg(x.sector_rad)).unwrap_or(360.0),
                        )
                    }
                };
                Some(JammerView {
                    active: st.active,
                    mode: mode_str.to_owned(),
                    jam_strength: strength,
                    coverage_km,
                    sector_deg,
                    available_modes,
                })
            }),
        sensors: p
            .sensors
            .iter()
            .map(|m| db.str(db.sensors[m.sensor.0 as usize].name).to_owned())
            .collect(),
        roe,
        roe_source,
        roe_override: own_roe.roe,
        self_defence,
        self_defence_override: own_roe.self_defence,
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
    let nation = &rules.db.nations[world.get::<Allegiance>(e)?.nation.0 as usize];
    Some(SatelliteDetails {
        id: id.0,
        name: world.get::<Identity>(e)?.name.clone(),
        type_name: rules.db.str(p.name).to_owned(),
        owner: rules.db.str(nation.id).to_uppercase(),
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
        class_name: p
            .design
            .class_name
            .map(|n| rules.db.str(n).to_owned())
            .unwrap_or_default(),
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
        stock: b
            .stock
            .iter()
            .map(|(w, n)| WeaponCount {
                name: rules.db.str(rules.db.weapons[w.0 as usize].name).to_owned(),
                count: *n,
            })
            .collect(),
    }
}
