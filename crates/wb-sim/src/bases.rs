//! Bases: lookup, suitability, landing and berthing, turnaround and
//! activation, munitions stock and rearming (doc 02 §6, doc 03 §3–4, §6).

use bevy_ecs::prelude::*;
use std::collections::BTreeMap;
use wb_content::compiled::{
    Base as ContentBase, BaseIx, LoadoutIx, Platform, WeaponIx, station_cells,
};
use wb_content::source::StationKind;
use wb_content::source::realism::BaseKind;
use wb_content::source::world::BaseType;
use wb_core::UnitId;
use wb_core::geo::{LatLon, distance_m};

use crate::clock::SimClock;
use crate::components::{
    Allegiance, BaseRef, BaseSite, Countermeasures, Fuel, Group, Identity, Kinematics, Magazine,
    Mode, PlatformRef, Posture, ReturnWhy, TankerState, Task,
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

/// Logistics (every `logistics_period_s`): turnarounds finish (refuel,
/// countermeasures, rearm from the base's stock), placed bases activate.
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
            if let Some(mut c) = world.get_mut::<Countermeasures>(e) {
                c.refill();
            }
            rearm(world, &rules, id, e, base);
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

/// A full magazine of a loadout template.
pub fn template_magazine(rules: &Rules, lix: LoadoutIx) -> Magazine {
    let items = &rules.db.loadouts[lix.0 as usize].items;
    Magazine::full(
        Some(lix),
        items.iter().map(|i| (i.station, i.weapon, i.count)),
    )
}

/// A system component's content magazine, spread over the stations that fit
/// each weapon in station order (`wb content check` has checked the cells).
pub fn system_magazine(rules: &Rules, p: &Platform, rounds: &[(WeaponIx, u32)]) -> Magazine {
    let db = &rules.db;
    let mut free: Vec<u32> = p.stations.iter().map(|s| s.capacity).collect();
    let mut items = Vec::new();
    for (wix, n) in rounds {
        let w = &db.weapons[wix.0 as usize];
        let mut left = *n;
        for (i, st) in p.stations.iter().enumerate() {
            if left == 0 {
                break;
            }
            if !w.station_category.iter().any(|c| st.categories.contains(c)) {
                continue;
            }
            let per_cell = match st.kind {
                StationKind::Vls | StationKind::Launcher => w.rounds_per_cell.max(1),
                StationKind::Internal | StationKind::External => 1,
            };
            let take = left.min(free[i] * per_cell);
            if take > 0 {
                free[i] -= station_cells(st.kind, w.rounds_per_cell, take);
                items.push((i as u32, *wix, take));
                left -= take;
            }
        }
    }
    Magazine::full(None, items)
}

/// Munitions at a real base at war start: its `munition_basing` in the
/// latest inventory of the base's country.
pub fn initial_stock(rules: &Rules, base: BaseIx) -> BTreeMap<WeaponIx, u32> {
    let db = &rules.db;
    let country = db.bases[base.0 as usize].country;
    db.inventories
        .iter()
        .filter(|i| i.country == country)
        .max_by(|a, b| db.str(a.as_of).cmp(db.str(b.as_of)))
        .and_then(|inv| inv.munition_basing.iter().find(|(b, _)| *b == base))
        .map(|(_, m)| m.iter().copied().filter(|(_, n)| *n > 0).collect())
        .unwrap_or_default()
}

/// Whether a base reloads VLS cells (doc 03 §4: naval bases only).
pub fn vls_reload(rules: &Rules, kind: BaseKind) -> bool {
    rules
        .realism()
        .bases
        .get(&kind)
        .is_some_and(|r| r.vls_reload)
}

/// Draws up to `n` rounds of a weapon from a base's stock; returns what it got.
pub fn draw(stock: &mut BTreeMap<WeaponIx, u32>, w: WeaponIx, n: u32) -> u32 {
    let Some(have) = stock.get_mut(&w) else {
        return 0;
    };
    let got = n.min(*have);
    *have -= got;
    if *have == 0 {
        stock.remove(&w);
    }
    got
}

/// Puts rounds back into a base's stock.
pub fn put_back(stock: &mut BTreeMap<WeaponIx, u32>, w: WeaponIx, n: u32) {
    if n > 0 {
        *stock.entry(w).or_default() += n;
    }
}

/// Tops a unit's stations up from its base's stock (VLS cells only where the
/// base reloads them) and reports each weapon it could not fill.
pub fn rearm(world: &mut World, rules: &Rules, unit: UnitId, e: Entity, base: UnitId) {
    let Some(be) = entity(world, base) else {
        return;
    };
    let Some(mut mag) = world.get::<Magazine>(e).cloned() else {
        return;
    };
    let Some(pix) = world.get::<PlatformRef>(e).map(|p| p.0) else {
        return;
    };
    let p = rules.platform(pix);
    let mut site = world.get::<BaseSite>(be).expect("base").clone();
    let vls = vls_reload(rules, site.kind);
    let mut short: BTreeMap<WeaponIx, u32> = BTreeMap::new();
    for s in &mut mag.stations {
        let need = s.full.saturating_sub(s.count);
        if need == 0 {
            continue;
        }
        let kind = p.stations.get(s.station as usize).map(|st| st.kind);
        if kind == Some(StationKind::Vls) && !vls {
            continue;
        }
        let got = draw(&mut site.stock, s.weapon, need);
        s.count += got;
        if got < need {
            *short.entry(s.weapon).or_default() += need - got;
        }
    }
    *world.get_mut::<Magazine>(e).expect("magazine") = mag;
    world.get_mut::<BaseSite>(be).expect("base").stock = site.stock;
    let side = world.get::<Allegiance>(e).map_or(0, |a| a.side);
    for (w, n) in short {
        let weapon = rules.db.str(rules.db.weapons[w.0 as usize].id).to_owned();
        emit(
            world,
            side,
            EventKind::RearmShort {
                unit,
                base,
                weapon,
                short: n,
            },
        );
    }
}
