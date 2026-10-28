//! Bases: lookup, suitability, landing and berthing, turnaround and
//! activation (doc 02 §6, doc 03 §3–4).

use bevy_ecs::prelude::*;
use wb_content::compiled::{Base as ContentBase, Platform};
use wb_content::source::realism::BaseKind;
use wb_content::source::world::BaseType;
use wb_core::UnitId;
use wb_core::geo::{LatLon, distance_m};

use crate::clock::SimClock;
use crate::components::{
    Allegiance, BaseRef, BaseSite, Fuel, Group, Identity, Kinematics, Magazine, Mode, PlatformRef,
    Posture, ReturnWhy, TankerState, Task,
};
use crate::events::{EventKind, emit};
use crate::rules::Rules;
use crate::world::{UnitIndex, entity};

/// A base as the task logic sees it.
#[derive(Clone, Debug)]
pub struct BaseInfo {
    pub id: UnitId,
    pub name: String,
    pub content: Option<wb_content::compiled::BaseIx>,
    pub side: u8,
    pub kind: BaseKind,
    pub air: bool,
    pub port: bool,
    pub at: LatLon,
    pub elevation_m: f64,
    pub runway_m: Option<f64>,
    pub aircraft_capacity: u32,
    pub ship_berths: u32,
    pub stationed: usize,
    pub active: bool,
}

/// Every base, in `UnitId` order.
pub fn list(world: &mut World) -> Vec<BaseInfo> {
    let mut q = world.query::<(&Identity, &Allegiance, &BaseSite)>();
    let mut out: Vec<BaseInfo> = q
        .iter(world)
        .map(|(id, a, b)| BaseInfo {
            id: id.unit,
            name: id.name.clone(),
            content: b.content,
            side: a.side,
            kind: b.kind,
            air: b.air,
            port: b.port,
            at: b.at,
            elevation_m: b.elevation_m,
            runway_m: b.runway_m,
            aircraft_capacity: b.aircraft_capacity,
            ship_berths: b.ship_berths,
            stationed: b.stationed.len(),
            active: b.active,
        })
        .collect();
    out.sort_by_key(|b| b.id);
    out
}

/// Plain-English name of a base kind.
pub fn kind_name(kind: BaseKind) -> &'static str {
    match kind {
        BaseKind::AirBase => "Air base",
        BaseKind::RoadStrip => "Road strip",
        BaseKind::Heliport => "Heliport",
        BaseKind::NavalBase => "Naval base",
        BaseKind::Anchorage => "Expeditionary anchorage",
        BaseKind::ArmyBase => "Army base",
        BaseKind::SamSite => "SAM site",
        BaseKind::MissileSite => "Missile launch site",
        BaseKind::Hq => "HQ",
    }
}

/// Kind and services of a real base from content.
pub fn content_kind(b: &ContentBase) -> (BaseKind, bool, bool) {
    match b.base_type {
        BaseType::AirBase => (BaseKind::AirBase, true, false),
        BaseType::Port => (BaseKind::NavalBase, false, true),
        BaseType::AirBaseAndPort => (BaseKind::AirBase, true, true),
    }
}

/// Whether a platform can use a base; the reason if not.
pub fn suitable(rules: &Rules, b: &BaseInfo, p: &Platform) -> Result<(), String> {
    let pname = rules.db.str(p.name);
    if !b.active {
        return Err(format!("{} is not active yet", b.name));
    }
    if rules.is_air(p) {
        if !b.air {
            return Err(format!("{} has no airfield", b.name));
        }
        let rule = &rules.realism().bases[&b.kind];
        if rule.rotary_only && !rules.is_rotary(p) {
            return Err(format!(
                "{} takes helicopters and drones only, not the {pname}",
                b.name
            ));
        }
        if rule.road_capable_only && !p.mobility.road_capable {
            return Err(format!(
                "{} is a road strip; the {pname} cannot use one",
                b.name
            ));
        }
        if let (Some(need), Some(have)) = (p.mobility.min_runway_m, b.runway_m)
            && have < need
        {
            return Err(format!(
                "Runway at {} ({have:.0} m) is too short for the {pname} (needs {need:.0} m)",
                b.name
            ));
        }
        Ok(())
    } else if b.port {
        Ok(())
    } else {
        Err(format!("{} is not a port", b.name))
    }
}

/// Free places for this platform's domain.
pub fn room(rules: &Rules, b: &BaseInfo, p: &Platform) -> u32 {
    let cap = if rules.is_air(p) {
        b.aircraft_capacity
    } else {
        b.ship_berths
    };
    cap.saturating_sub(b.stationed as u32)
}

/// Nearest base of `side` the platform can use and has room (or is `home`).
pub fn nearest_suitable<'a>(
    rules: &Rules,
    bases: &'a [BaseInfo],
    side: u8,
    p: &Platform,
    at: LatLon,
    home: Option<UnitId>,
) -> Option<&'a BaseInfo> {
    bases
        .iter()
        .filter(|b| b.side == side && suitable(rules, b, p).is_ok())
        .filter(|b| Some(b.id) == home || room(rules, b, p) > 0)
        .map(|b| (distance_m(at, b.at), b))
        .min_by(|x, y| x.0.total_cmp(&y.0).then(x.1.id.cmp(&y.1.id)))
        .map(|(_, b)| b)
}

/// Turnaround time of a platform.
pub fn turnaround_s(rules: &Rules, p: &Platform) -> f64 {
    p.turnaround_s
        .unwrap_or(rules.realism().logistics.turnaround_default_s)
}

/// Lands (or berths) every member of a returning group whose lead reached
/// its base. Runs after movement.
pub fn arrivals(world: &mut World) {
    let rules = world.resource::<Rules>().clone();
    let now = world.resource::<SimClock>().end_ms();
    let log = &rules.realism().logistics;
    let mut q = world.query::<(&Identity, &Group)>();
    let mut returning: Vec<(UnitId, UnitId, ReturnWhy, UnitId)> = q
        .iter(world)
        .filter_map(|(id, g)| match g.mode {
            Mode::Return { base, why } => Some((id.unit, base, why, g.lead()?)),
            _ => None,
        })
        .collect();
    returning.sort_by_key(|r| r.0);

    for (gid, base_id, why, lead) in returning {
        let (Some(ge), Some(le), Some(be)) = (
            entity(world, gid),
            entity(world, lead),
            entity(world, base_id),
        ) else {
            continue;
        };
        let k = *world.get::<Kinematics>(le).expect("lead has kinematics");
        let p = rules.platform(world.get::<PlatformRef>(le).expect("platform").0);
        let site = world.get::<BaseSite>(be).expect("base").clone();
        let d = distance_m(k.pos(), site.at);
        let arrived = if rules.is_air(p) {
            d <= log.landing_radius_m && k.alt_m <= site.elevation_m + log.pattern_alt_m + 100.0
        } else {
            d <= log.berth_radius_m
        };
        if !arrived {
            continue;
        }
        let side = world.get::<Allegiance>(ge).map_or(0, |a| a.side);
        let members = world.get::<Group>(ge).expect("group").members.clone();
        for m in &members {
            let Some(me) = entity(world, *m) else {
                continue;
            };
            let p = rules.platform(world.get::<PlatformRef>(me).expect("platform").0);
            let ready_at_ms = now + (turnaround_s(&rules, p) * 1_000.0) as u64;
            *world.get_mut::<Posture>(me).expect("posture") = Posture::AtBase {
                base: base_id,
                ready_at_ms,
                ready: false,
            };
            if let Some(mut mk) = world.get_mut::<Kinematics>(me) {
                *mk = Kinematics {
                    lat: site.at.lat,
                    lon: site.at.lon,
                    alt_m: site.elevation_m,
                    heading: mk.heading,
                    speed_mps: 0.0,
                    climb_mps: 0.0,
                };
            }
            if why == ReturnWhy::Transfer
                && let Some(mut r) = world.get_mut::<BaseRef>(me)
            {
                r.home = Some(base_id);
            }
            let fuel_kg = world.get::<Fuel>(me).map_or(0.0, |f| f.kg);
            emit(
                world,
                side,
                EventKind::Landed {
                    unit: *m,
                    base: base_id,
                    fuel_kg,
                },
            );
        }
        {
            let mut b = world.get_mut::<BaseSite>(be).expect("base");
            b.stationed.extend(members.iter().copied());
            b.stationed.sort();
            b.stationed.dedup();
        }
        let mut g = world.get_mut::<Group>(ge).expect("group");
        g.mode = Mode::AtBase { base: base_id };
        g.on_station = false;
        g.launch = false;
        let resume = g.behaviour.resume_after_turnaround
            && matches!(why, ReturnWhy::Bingo | ReturnWhy::Winchester);
        if !(g.task.is_sortie() && resume) {
            g.task = Task::Idle;
        }
    }
}

/// Logistics (every `logistics_period_s`): turnarounds finish, placed bases
/// activate.
pub fn logistics(world: &mut World) {
    let rules = world.resource::<Rules>().clone();
    let clock = world.resource::<SimClock>().clone();
    let period = (rules.realism().time.logistics_period_s * 1_000.0) as u64;
    if !clock.due(period.max(1_000)) {
        return;
    }
    let now = clock.end_ms();
    let index: Vec<(UnitId, Entity)> = world
        .resource::<UnitIndex>()
        .0
        .iter()
        .map(|(k, v)| (*k, *v))
        .collect();
    for (id, e) in index {
        if let Some(Posture::AtBase {
            base,
            ready_at_ms,
            ready: false,
        }) = world.get::<Posture>(e).copied()
            && now >= ready_at_ms
        {
            *world.get_mut::<Posture>(e).expect("posture") = Posture::AtBase {
                base,
                ready_at_ms,
                ready: true,
            };
            let pix = world.get::<PlatformRef>(e).map(|p| p.0);
            if let Some(mut f) = world.get_mut::<Fuel>(e) {
                f.kg = f.max_kg;
                f.reserve_at_bingo_kg = None;
            }
            if let (Some(mut t), Some(pix)) = (world.get_mut::<TankerState>(e), pix)
                && let Some(tk) = &rules.platform(pix).tanker
            {
                t.offload_left_kg = tk.offload_kg;
            }
            if let Some(mut m) = world.get_mut::<Magazine>(e)
                && let Some(lix) = m.loadout
            {
                m.rounds = loadout_rounds(&rules, lix);
            }
            let side = world.get::<Allegiance>(e).map_or(0, |a| a.side);
            emit(world, side, EventKind::TurnaroundDone { unit: id, base });
        }
        if let Some(b) = world.get::<BaseSite>(e)
            && !b.active
            && now >= b.active_at_ms
        {
            world.get_mut::<BaseSite>(e).expect("base").active = true;
            let side = world.get::<Allegiance>(e).map_or(0, |a| a.side);
            emit(world, side, EventKind::BaseActivated { base: id });
        }
    }
}

/// Rounds per weapon of a loadout, merged and sorted.
pub fn loadout_rounds(
    rules: &Rules,
    lix: wb_content::compiled::LoadoutIx,
) -> Vec<(wb_content::compiled::WeaponIx, u32)> {
    let mut out: Vec<(wb_content::compiled::WeaponIx, u32)> = Vec::new();
    for item in &rules.db.loadouts[lix.0 as usize].items {
        match out.iter_mut().find(|(w, _)| *w == item.weapon) {
            Some((_, n)) => *n += item.count,
            None => out.push((item.weapon, item.count)),
        }
    }
    out.sort();
    out
}
