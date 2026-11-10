//! Ground forces (doc 02 §5, §13–14; P11, ADR-048).
//!
//! A formation is one map object: a group with one platform entity that
//! carries a [`Formation`] (equipment counts, personnel, posture, morale,
//! supply). The stage does, in `UnitId` order:
//!
//! - **every tick:** movement along the route found on the H3 grid (A*,
//!   `wb_geo::path`), at the slowest equipment's speed × the posture's and
//!   the cell's terrain factor; postures (moving, attack, hasty and prepared
//!   defence by time halted);
//! - **every second:** which formations are in contact, designation, raids;
//! - **every logistics period:** supply (own bases flagged `ground_supply`
//!   resupply within `supply_radius_m`), fatigue and morale;
//! - **every `ground_combat_period_s`:** Lanchester-style attrition between
//!   formations in contact, artillery support, retreat and breakthrough.
//!
//! Strikes ([`strike`], [`burst`]) remove equipment by the weapon's effective
//! warhead and the posture's exposure. Every number is content: the platform
//! `ground` stats and the realism `ground` section (CLAUDE.md rule 3).

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use wb_content::compiled::{GroundStats, PlatformIx, SystemIx, Weapon};
use wb_content::source::Domain;
use wb_content::source::realism::{
    CellClass, ContactLevel, GroundPosture, PostureRule, TerrainClass,
};
use wb_core::geo::{LatLon, destination_point, distance_m, initial_bearing, normalize_bearing};
use wb_core::{ContactId, RngPurpose, UnitId, rng, rng_normal};
use wb_geo::Geo;

use crate::bases;
use crate::clock::{RngSeed, SimClock};
use crate::components::{
    Allegiance, EquipmentLine, Formation, GroundKind, GroundOrder, Group, GroupMember, Identity,
    Kinematics, Magazine, Task,
};
use crate::events::{EventKind, emit};
use crate::picture::{Detection, Measured, Quality, SidePictures, SourceKind, TypeRef};
use crate::rules::Rules;
use crate::sensors::GeoRes;
use crate::world::{UnitIndex, entity};

/// A formation within this distance of its goal has arrived.
const ARRIVE_M: f64 = 60.0;
/// A contact that moved this far from where the route ends gets a new route.
const REROUTE_M: f64 = 1_500.0;
/// Routes to a moving contact are refreshed this often.
const REROUTE_PERIOD_MS: u64 = 60_000;
/// Speed factor on a cell the formation should not be on (pushed into water).
const STUCK_FACTOR: f64 = 0.2;
/// A team's own sighting of its target must be this fresh to designate it.
const DESIGNATE_FRESH_MS: u64 = 15_000;
/// 1-sigma position error of an enemy met in ground combat.
const CONTACT_SIGMA_M: f64 = 100.0;
/// Supply below this raises `SupplyLow`; above twice this it clears.
const SUPPLY_LOW: f64 = 0.25;

/// Ground state beside the formations. Saved and hashed.
#[derive(Resource, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GroundState {
    /// Contacts a team is designating: (side, contact) → the team's platform.
    pub designations: BTreeMap<(u8, ContactId), UnitId>,
}

/// The numbers behind one formation's part in a combat step (CLAUDE.md rule 4).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct GroundFactors {
    /// Effective strength it fought with: equipment value × the factors below.
    pub strength: f64,
    /// Posture protection × terrain (terrain only when it is not attacking).
    pub protection: f64,
    pub terrain: TerrainClass,
    pub posture: GroundPosture,
    /// Hostile formations in contact.
    pub enemies: u32,
    pub f_morale: f64,
    pub f_supply: f64,
    pub f_fatigue: f64,
    pub f_night: f64,
    pub f_ew: f64,
    pub f_suppression: f64,
    /// Combat value lost this step, before rounding to equipment.
    pub value_lost: f64,
}

fn stats(rules: &Rules, p: PlatformIx) -> Option<&GroundStats> {
    rules.platform(p).ground.as_ref()
}

fn posture_rule(rules: &Rules, p: GroundPosture) -> PostureRule {
    rules
        .realism()
        .ground
        .posture
        .get(&p)
        .cloned()
        .unwrap_or(PostureRule {
            speed_factor: 1.0,
            protection: 1.0,
            exposure: 1.0,
            size_factor: 1.0,
        })
}

/// Size factor sensors apply to a formation in this posture.
pub fn size_factor(rules: &Rules, f: &Formation) -> f64 {
    posture_rule(rules, f.posture).size_factor
}

/// Direct-fire value of the equipment left.
pub fn value(rules: &Rules, f: &Formation) -> f64 {
    f.equipment
        .iter()
        .map(|e| f64::from(e.count) * stats(rules, e.platform).map_or(0.0, |s| s.combat_value))
        .sum()
}

/// What losses are shared over: every item counts at least 1, so guns and
/// teams without direct-fire value can still be lost.
fn bulk(rules: &Rules, f: &Formation, full: bool) -> f64 {
    f.equipment
        .iter()
        .map(|e| {
            let n = if full { e.full } else { e.count };
            f64::from(n) * stats(rules, e.platform).map_or(1.0, |s| s.combat_value.max(1.0))
        })
        .sum()
}

/// Strength left, 0–1 of the full establishment.
pub fn strength(rules: &Rules, f: &Formation) -> f64 {
    let full = bulk(rules, f, true);
    if full <= 0.0 {
        0.0
    } else {
        bulk(rules, f, false) / full
    }
}

/// Cross-country speed of the slowest equipment left, m/s.
pub fn base_speed(rules: &Rules, f: &Formation) -> f64 {
    f.equipment
        .iter()
        .filter(|e| e.count > 0)
        .map(|e| {
            let m = &rules.platform(e.platform).mobility;
            m.offroad_speed_mps.or(m.road_speed_mps).unwrap_or(0.0)
        })
        .min_by(f64::total_cmp)
        .unwrap_or(0.0)
}

fn heavy(rules: &Rules, f: &Formation) -> bool {
    f.equipment
        .iter()
        .any(|e| e.count > 0 && stats(rules, e.platform).is_some_and(|s| s.heavy))
}

fn crew(rules: &Rules, p: PlatformIx) -> u32 {
    rules
        .platform(p)
        .crew
        .unwrap_or(rules.realism().ground.default_crew)
}

/// Speed factor of a terrain class; none when it cannot be entered.
pub fn terrain_factor(rules: &Rules, class: wb_geo::TerrainClass, heavy: bool) -> Option<f64> {
    let c: CellClass = crate::placement::cell_class(class);
    let gr = &rules.realism().ground;
    if heavy && gr.heavy_blocked.contains(&c) {
        return None;
    }
    gr.speed_factor.get(&c).copied().filter(|f| *f > 0.0)
}

/// The route of a formation from `from` to `to`: cell centres, `to` last.
/// Without terrain data it is the straight line.
pub fn route(
    rules: &Rules,
    geo: Option<&Geo>,
    f: &Formation,
    from: LatLon,
    to: LatLon,
) -> Result<Vec<LatLon>, String> {
    let Some(geo) = geo.filter(|g| g.pack_at(from).is_some()) else {
        return Ok(vec![to]);
    };
    let gr = &rules.realism().ground;
    let heavy = heavy(rules, f);
    let max = gr.speed_factor.values().copied().fold(0.0, f64::max);
    geo.ground_path(from, to, max, gr.path_max_cells, &|c| {
        terrain_factor(rules, c.class, heavy)
    })
    .map(|p| p.points)
    .map_err(|e| e.to_string())
}

/// A new formation of a `deploy_as: formation` system: the platform its map
/// entity is built from (the first equipment line), its strength and its
/// magazine (the first line's rounds, for every vehicle).
pub fn build(
    rules: &Rules,
    six: SystemIx,
    posture: Option<GroundPosture>,
) -> Option<(PlatformIx, Formation, Magazine)> {
    let sys = &rules.db.systems[six.0 as usize];
    let first = sys.components.first()?;
    let equipment: Vec<EquipmentLine> = sys
        .components
        .iter()
        .map(|c| EquipmentLine {
            platform: c.platform,
            count: c.count,
            full: c.count,
            worn: 0.0,
        })
        .collect();
    let personnel = equipment
        .iter()
        .map(|e| e.full * crew(rules, e.platform))
        .sum();
    let mut mag = bases::system_magazine(rules, rules.platform(first.platform), &first.magazine);
    for s in &mut mag.stations {
        s.full *= first.count;
        s.count *= first.count;
    }
    Some((
        first.platform,
        Formation {
            equipment,
            personnel,
            personnel_full: personnel,
            posture: posture.unwrap_or(GroundPosture::Hasty),
            halted_ms: None,
            morale: 1.0,
            supply: 1.0,
            fatigue: 0.0,
            suppressed_until_ms: 0,
            last_combat_ms: None,
            in_contact: false,
            supply_low: false,
        },
        mag,
    ))
}

/// One formation: its platform entity and its group.
#[derive(Clone, Copy)]
struct Fm {
    body: UnitId,
    be: Entity,
    gid: UnitId,
    ge: Entity,
    side: u8,
}

/// Every formation, in `UnitId` order of its platform entity.
fn formations(world: &World) -> Vec<Fm> {
    world
        .resource::<UnitIndex>()
        .0
        .iter()
        .filter_map(|(&body, &be)| {
            world.get::<Formation>(be)?;
            let gid = world.get::<GroupMember>(be)?.group;
            let ge = entity(world, gid)?;
            world.get::<Group>(ge)?;
            Some(Fm {
                body,
                be,
                gid,
                ge,
                side: world.get::<Allegiance>(be)?.side,
            })
        })
        .collect()
}

fn pos_of(world: &World, e: Entity) -> Option<LatLon> {
    world.get::<Kinematics>(e).map(|k| k.pos())
}

/// The ground stage.
pub fn run(world: &mut World) {
    let list = formations(world);
    if list.is_empty() {
        return;
    }
    let rules = world.resource::<Rules>().clone();
    let clock = world.resource::<SimClock>().clone();
    let seed = world.resource::<RngSeed>().0;
    let geo = world.get_non_send::<GeoRes>().and_then(|g| g.0.clone());
    let geo = geo.as_deref();
    let second = clock.due(1_000);
    if second {
        contacts(world, &rules, &list);
    }
    for fm in &list {
        advance(world, &rules, &clock, geo, fm);
    }
    if second {
        designate(world, &rules, &clock, &list);
        raids(world, &rules, &clock, seed, &list);
    }
    let time = &rules.realism().time;
    let logistics = (time.logistics_period_s * 1_000.0).max(1_000.0) as u64;
    if clock.due(logistics) {
        upkeep(world, &rules, &clock, &list, logistics);
    }
    let period = (time.ground_combat_period_s * 1_000.0).max(1_000.0) as u64;
    if clock.due(period) {
        combat(world, &rules, &clock, seed, geo);
    }
}

/// Marks the formations with a hostile formation inside contact range.
fn contacts(world: &mut World, rules: &Rules, list: &[Fm]) {
    let range = rules.realism().ground.contact_range_m;
    let at: Vec<Option<LatLon>> = list.iter().map(|f| pos_of(world, f.be)).collect();
    let mut hit = vec![false; list.len()];
    for i in 0..list.len() {
        for j in i + 1..list.len() {
            if list[i].side == list[j].side {
                continue;
            }
            if let (Some(a), Some(b)) = (at[i], at[j])
                && distance_m(a, b) <= range
            {
                hit[i] = true;
                hit[j] = true;
            }
        }
    }
    for (fm, now) in list.iter().zip(hit) {
        let Some(mut f) = world.get_mut::<Formation>(fm.be) else {
            continue;
        };
        if f.in_contact != now {
            f.in_contact = now;
            if now {
                emit(
                    world,
                    fm.side,
                    EventKind::GroundContact { formation: fm.gid },
                );
            }
        }
    }
}

/// Movement and posture of one formation for this tick.
fn advance(world: &mut World, rules: &Rules, clock: &SimClock, geo: Option<&Geo>, fm: &Fm) {
    let gr = &rules.realism().ground;
    let now = clock.sim_time_ms;
    let (Some(mut f), Some(mut k)) = (
        world.get::<Formation>(fm.be).cloned(),
        world.get::<Kinematics>(fm.be).copied(),
    ) else {
        return;
    };
    let mut g = std::mem::take(&mut *world.get_mut::<Group>(fm.ge).expect("group"));
    let mut events = Vec::new();
    let mut moving = false;
    let mut attack = false;
    if let Task::Ground(mut o) = g.task.clone() {
        attack = o.kind == GroundKind::Attack;
        let goal = o
            .contact
            .and_then(|c| crate::tasks::estimate(world, fm.side, c).map(|e| e.0))
            .or(o.dest);
        let standoff = match o.kind {
            GroundKind::Recon => gr.recon_standoff_m,
            GroundKind::Designate => f
                .equipment
                .iter()
                .filter_map(|e| stats(rules, e.platform)?.designate_m)
                .fold(gr.recon_standoff_m, |a, b| a.min(b * 0.8)),
            _ => ARRIVE_M,
        };
        match goal {
            Some(goal) => {
                let fresh = match o.routed_to {
                    None => true,
                    Some(r) => {
                        distance_m(r, goal) > REROUTE_M
                            && clock.due_staggered(REROUTE_PERIOD_MS, fm.body.0)
                    }
                };
                if fresh {
                    o.routed_to = Some(goal);
                    g.wp = 0;
                    match route(rules, geo, &f, k.pos(), goal) {
                        Ok(path) => {
                            o.path = path;
                            o.arrived = false;
                        }
                        Err(reason) => {
                            o.path.clear();
                            o.arrived = true;
                            events.push(EventKind::GroundOrderFailed {
                                formation: fm.gid,
                                reason,
                            });
                        }
                    }
                }
                if !o.arrived
                    && (distance_m(k.pos(), goal) <= standoff || g.wp as usize >= o.path.len())
                {
                    o.arrived = true;
                }
            }
            None => o.arrived = true,
        }
        moving = !o.arrived && !(attack && f.in_contact);
        if moving {
            let class = geo.map(|g| {
                g.terrain_class(k.pos())
                    .unwrap_or(wb_geo::TerrainClass::Water)
            });
            let tf = match class {
                Some(c) => terrain_factor(rules, c, heavy(rules, &f)).unwrap_or(STUCK_FACTOR),
                None => 1.0,
            };
            let pr = posture_rule(
                rules,
                if attack {
                    GroundPosture::Attack
                } else {
                    GroundPosture::Moving
                },
            );
            let v = base_speed(rules, &f) * pr.speed_factor * tf;
            let mut left = v * clock.dt_s();
            let mut at = k.pos();
            let mut heading = k.heading;
            while left > 0.0 && (g.wp as usize) < o.path.len() {
                let to = o.path[g.wp as usize];
                let d = distance_m(at, to);
                if d > 0.5 {
                    heading = initial_bearing(at, to);
                }
                if d <= left {
                    at = to;
                    left -= d;
                    g.wp += 1;
                } else {
                    at = destination_point(at, heading, left);
                    left = 0.0;
                }
            }
            k.lat = at.lat;
            k.lon = at.lon;
            k.heading = normalize_bearing(heading);
            k.speed_mps = v;
        }
        if o.arrived && o.kind == GroundKind::Raid && !o.done && o.at_target_ms.is_none() {
            o.at_target_ms = Some(now);
        }
        g.on_station = o.arrived;
        g.task = Task::Ground(o);
    }
    if !moving {
        k.speed_mps = 0.0;
    }
    k.climb_mps = 0.0;

    let posture = if moving {
        f.halted_ms = None;
        if attack {
            GroundPosture::Attack
        } else {
            GroundPosture::Moving
        }
    } else {
        let since = *f.halted_ms.get_or_insert(now);
        let halted_s = now.saturating_sub(since) as f64 / 1_000.0;
        if attack && f.in_contact {
            GroundPosture::Attack
        } else {
            let by_time = if halted_s >= gr.prepared_after_s {
                Some(GroundPosture::Prepared)
            } else if halted_s >= gr.hasty_after_s {
                Some(GroundPosture::Hasty)
            } else {
                None
            };
            match (f.posture, by_time) {
                (GroundPosture::Prepared, _) | (_, Some(GroundPosture::Prepared)) => {
                    GroundPosture::Prepared
                }
                (GroundPosture::Hasty, _) | (_, Some(GroundPosture::Hasty)) => GroundPosture::Hasty,
                (p, _) => p,
            }
        }
    };
    if posture != f.posture {
        f.posture = posture;
        events.push(EventKind::PostureChanged {
            formation: fm.gid,
            posture,
        });
    }
    *world.get_mut::<Group>(fm.ge).expect("group") = g;
    *world.get_mut::<Formation>(fm.be).expect("formation") = f;
    *world.get_mut::<Kinematics>(fm.be).expect("kinematics") = k;
    for e in events {
        emit(world, fm.side, e);
    }
}

fn ground_order(world: &World, fm: &Fm) -> Option<GroundOrder> {
    match &world.get::<Group>(fm.ge)?.task {
        Task::Ground(o) => Some(o.clone()),
        _ => None,
    }
}

/// Which contacts are designated (doc 02 §14): the team is within its
/// designation range of the contact and its own sensors hold it.
fn designate(world: &mut World, rules: &Rules, clock: &SimClock, list: &[Fm]) {
    let now = clock.sim_time_ms;
    let mut next: BTreeMap<(u8, ContactId), UnitId> = BTreeMap::new();
    let mut owner: BTreeMap<UnitId, UnitId> = BTreeMap::new();
    for fm in list {
        owner.insert(fm.body, fm.gid);
        let Some(o) = ground_order(world, fm) else {
            continue;
        };
        let (GroundKind::Designate, Some(c)) = (o.kind, o.contact) else {
            continue;
        };
        let (Some(f), Some(at)) = (world.get::<Formation>(fm.be), pos_of(world, fm.be)) else {
            continue;
        };
        let Some(range) = f
            .equipment
            .iter()
            .filter(|e| e.count > 0)
            .filter_map(|e| stats(rules, e.platform)?.designate_m)
            .max_by(f64::total_cmp)
        else {
            continue;
        };
        let pics = world.resource::<SidePictures>();
        let held = pics
            .sides
            .get(usize::from(fm.side))
            .and_then(|p| p.get(c))
            .is_some_and(|ct| {
                !ct.lost
                    && ct.est.is_some_and(|e| distance_m(at, e.at) <= range)
                    && ct.sources.iter().any(|s| {
                        s.observer == fm.body && now.saturating_sub(s.last_ms) <= DESIGNATE_FRESH_MS
                    })
            });
        if held {
            next.entry((fm.side, c)).or_insert(fm.body);
        }
    }
    let old = world.resource::<GroundState>().designations.clone();
    if old == next {
        return;
    }
    let mut changes = Vec::new();
    for (k, body) in &next {
        if old.get(k) != Some(body) {
            changes.push((*k, *body, true));
        }
    }
    for (k, body) in &old {
        if !next.contains_key(k) {
            changes.push((*k, *body, false));
        }
    }
    world.resource_mut::<GroundState>().designations = next;
    for ((side, contact), body, on) in changes {
        // A team that is gone has no group to report for.
        if let Some(gid) = owner.get(&body) {
            emit(
                world,
                side,
                EventKind::Designation {
                    formation: *gid,
                    contact,
                    on,
                },
            );
        }
    }
}

/// A side's team is designating this contact (endgame CEP, doc 02 §14).
pub fn designated(world: &World, side: u8, contact: ContactId) -> bool {
    world
        .get_resource::<GroundState>()
        .is_some_and(|g| g.designations.contains_key(&(side, contact)))
}

/// Raids that have been at their target long enough sabotage the base there
/// and turn for home.
fn raids(world: &mut World, rules: &Rules, clock: &SimClock, seed: u64, list: &[Fm]) {
    let gr = &rules.realism().ground;
    let now = clock.sim_time_ms;
    for fm in list {
        let Some(o) = ground_order(world, fm) else {
            continue;
        };
        let (GroundKind::Raid, false, Some(t0)) = (o.kind, o.done, o.at_target_ms) else {
            continue;
        };
        if (now.saturating_sub(t0) as f64) < gr.sabotage_time_s * 1_000.0 {
            continue;
        }
        let Some(at) = pos_of(world, fm.be) else {
            continue;
        };
        let skill = world
            .get::<Formation>(fm.be)
            .map(|f| {
                f.equipment
                    .iter()
                    .filter(|e| e.count > 0)
                    .filter_map(|e| stats(rules, e.platform)?.sabotage_skill)
                    .fold(0.0, f64::max)
            })
            .unwrap_or(0.0);
        let target = bases::list(world)
            .into_iter()
            .filter(|b| b.side != fm.side)
            .map(|b| (distance_m(at, b.at), b))
            .filter(|(d, _)| *d <= gr.sabotage_radius_m)
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.id.cmp(&b.1.id)))
            .map(|(_, b)| b);
        let (success, component, reason) = match target {
            None => (
                false,
                None,
                "No enemy base within reach of the target point".to_owned(),
            ),
            Some(b) => {
                let roll = rng(
                    seed,
                    RngPurpose::Sabotage,
                    u64::from(fm.body.0),
                    clock.tick,
                    0,
                );
                if roll < skill {
                    match crate::damage::sabotage(world, rules, seed, b.id, fm.body) {
                        Some(c) => (
                            true,
                            Some(c),
                            "A base component is out of action".to_owned(),
                        ),
                        None => (false, None, "Nothing left to sabotage".to_owned()),
                    }
                } else {
                    (
                        false,
                        None,
                        format!("The raid failed (skill {skill:.2}, roll {roll:.2})"),
                    )
                }
            }
        };
        emit(
            world,
            fm.side,
            EventKind::Sabotage {
                formation: fm.gid,
                success,
                component,
                reason,
            },
        );
        // Home again: a withdrawal to where the raid started.
        if let Some(mut g) = world.get_mut::<Group>(fm.ge) {
            g.wp = 0;
            g.on_station = false;
            g.task = Task::Ground(GroundOrder::new(GroundKind::Withdraw, o.origin, None));
        }
        emit(
            world,
            fm.side,
            EventKind::TaskChanged {
                group: fm.gid,
                task: GroundKind::Withdraw.name().to_owned(),
            },
        );
    }
}

/// Own active bases that resupply ground forces.
fn supply_points(world: &mut World, rules: &Rules) -> Vec<(u8, LatLon)> {
    bases::list(world)
        .into_iter()
        .filter(|b| {
            b.active
                && rules
                    .realism()
                    .bases
                    .get(&b.kind)
                    .is_some_and(|r| r.ground_supply)
        })
        .map(|b| (b.side, b.at))
        .collect()
}

/// Supply, fatigue and morale (doc 02 §14 modifiers).
fn upkeep(world: &mut World, rules: &Rules, clock: &SimClock, list: &[Fm], period_ms: u64) {
    let gr = &rules.realism().ground;
    let now = clock.sim_time_ms;
    let h = period_ms as f64 / 3_600_000.0;
    let combat_ms = (rules.realism().time.ground_combat_period_s * 2_000.0) as u64;
    let points = supply_points(world, rules);
    for fm in list {
        let (Some(mut f), Some(k)) = (
            world.get::<Formation>(fm.be).cloned(),
            world.get::<Kinematics>(fm.be).copied(),
        ) else {
            continue;
        };
        let fought = f
            .last_combat_ms
            .is_some_and(|t| now.saturating_sub(t) <= combat_ms);
        let moving = k.speed_mps > 0.1;
        let supplied = points
            .iter()
            .any(|(s, at)| *s == fm.side && distance_m(k.pos(), *at) <= gr.supply_radius_m);
        let used = if fought {
            gr.supply_combat_per_h
        } else if moving {
            gr.supply_move_per_h
        } else {
            gr.supply_idle_per_h
        };
        let gained = if supplied { gr.resupply_per_h } else { 0.0 };
        f.supply = (f.supply + (gained - used) * h).clamp(0.0, 1.0);
        f.fatigue = (f.fatigue
            + if fought {
                gr.fatigue_combat_per_h
            } else if moving {
                gr.fatigue_move_per_h
            } else {
                -gr.fatigue_recovery_per_h
            } * h)
            .clamp(0.0, 1.0);
        if !fought {
            f.morale = (f.morale + gr.morale_recovery_per_h * h).clamp(0.0, 1.0);
        }
        let mut low = None;
        if f.supply < SUPPLY_LOW && !f.supply_low {
            f.supply_low = true;
            low = Some(f.supply);
        } else if f.supply >= 2.0 * SUPPLY_LOW {
            f.supply_low = false;
        }
        *world.get_mut::<Formation>(fm.be).expect("formation") = f;
        if let Some(s) = low {
            emit(
                world,
                fm.side,
                EventKind::SupplyLow {
                    formation: fm.gid,
                    supply_pct: s * 100.0,
                },
            );
        }
    }
}

/// What a loss removed.
#[derive(Clone, Copy, Default)]
struct Lost {
    items: u32,
    personnel: u32,
}

/// Removes equipment: `expected(i, line)` items of each line. A strike
/// rounds the fraction up with probability `draw(i)`; ground combat (no
/// `draw`) carries it to the next step. Crews go with their vehicles, and the
/// magazine shrinks with the first line.
fn remove(
    rules: &Rules,
    f: &mut Formation,
    mag: Option<&mut Magazine>,
    expected: impl Fn(usize, &EquipmentLine) -> f64,
    draw: Option<&dyn Fn(u32) -> f64>,
) -> Lost {
    let mut lost = Lost::default();
    let first_before = f.equipment.first().map_or(0, |e| e.count);
    for (i, line) in f.equipment.iter_mut().enumerate() {
        let mut x = expected(i, line).max(0.0);
        if draw.is_none() {
            x += line.worn;
        }
        let x = x.min(f64::from(line.count));
        let whole = x.floor();
        let mut n = whole as u32;
        match draw {
            Some(d) if d(i as u32) < x - whole => n += 1,
            Some(_) => {}
            None => line.worn = x - whole,
        }
        let n = n.min(line.count);
        line.count -= n;
        lost.items += n;
        lost.personnel += n * crew(rules, line.platform);
    }
    lost.personnel = lost.personnel.min(f.personnel);
    f.personnel -= lost.personnel;
    let first_after = f.equipment.first().map_or(0, |e| e.count);
    if let Some(mag) = mag
        && first_before > 0
        && first_after < first_before
    {
        for s in &mut mag.stations {
            s.full = (u64::from(s.full) * u64::from(first_after) / u64::from(first_before)) as u32;
            s.count =
                (u64::from(s.count) * u64::from(first_after) / u64::from(first_before)) as u32;
        }
    }
    lost
}

/// A formation that has lost too much is destroyed; true if it was.
fn destroyed(world: &mut World, rules: &Rules, body: UnitId, f: &Formation) -> bool {
    if f.items() == 0 || strength(rules, f) < rules.realism().ground.destroyed_strength {
        crate::damage::destroy(world, body);
        return true;
    }
    false
}

/// A weapon hit a formation (doc 02 §13): equipment is removed by the
/// weapon's effective warhead and the posture's exposure, shared over the
/// equipment lines and divided by each line's protection.
pub fn strike(
    world: &mut World,
    rules: &Rules,
    seed: u64,
    weapon: UnitId,
    w: &Weapon,
    body: UnitId,
) {
    let Some(be) = entity(world, body) else {
        return;
    };
    let Some(mut f) = world.get::<Formation>(be).cloned() else {
        return;
    };
    let gr = &rules.realism().ground;
    let clock = world.resource::<SimClock>().clone();
    let side = world.get::<Allegiance>(be).map_or(0, |a| a.side);
    let gid = world.get::<GroupMember>(be).map_or(body, |m| m.group);
    let total = f.items();
    if total == 0 {
        return;
    }
    let exposure = posture_rule(rules, f.posture).exposure;
    let kills = crate::damage::effective_kg(rules, w) / gr.strike_kg_per_kill.max(1e-9) * exposure;
    let before = bulk(rules, &f, false);
    let mut mag = world.get::<Magazine>(be).cloned();
    let lost = remove(
        rules,
        &mut f,
        mag.as_mut(),
        |_, line| {
            let protection = stats(rules, line.platform).map_or(1.0, |s| s.protection.max(0.05));
            let share = f64::from(line.count) / f64::from(total);
            (kills * share / protection)
                .min((gr.strike_max_fraction * f64::from(line.count)).ceil())
        },
        Some(&|n| {
            rng(
                seed,
                RngPurpose::GroundStrike,
                u64::from(body.0),
                clock.tick,
                weapon.0 ^ (n << 24),
            )
        }),
    );
    let fraction = if before > 0.0 {
        1.0 - bulk(rules, &f, false) / before
    } else {
        0.0
    };
    f.morale = (f.morale - fraction * gr.morale_loss_factor).clamp(0.0, 1.0);
    f.suppressed_until_ms = clock.sim_time_ms + (gr.suppression_s * 1_000.0) as u64;
    let event = EventKind::FormationStruck {
        formation: gid,
        lost_equipment: lost.items,
        lost_personnel: lost.personnel,
        strength_pct: strength(rules, &f) * 100.0,
        posture: f.posture,
        exposure,
    };
    if let Some(m) = mag {
        world.entity_mut(be).insert(m);
    }
    *world.get_mut::<Formation>(be).expect("formation") = f.clone();
    emit(world, side, event);
    destroyed(world, rules, body, &f);
}

/// A burst on a point: the nearest formation within `strike_radius_m` is
/// struck (artillery on a point, a strike on a last-known position).
pub fn burst(world: &mut World, rules: &Rules, seed: u64, weapon: UnitId, w: &Weapon, at: LatLon) {
    let r = rules.realism().ground.strike_radius_m;
    let near = formations(world)
        .into_iter()
        .filter_map(|fm| Some((distance_m(at, pos_of(world, fm.be)?), fm.body)))
        .filter(|(d, _)| *d <= r)
        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    if let Some((_, body)) = near {
        strike(world, rules, seed, weapon, w, body);
    }
}

/// One formation in a combat step.
struct Side {
    fm: Fm,
    pos: LatLon,
    f: Formation,
    kind: Option<GroundKind>,
    withdrawing: bool,
    factors: GroundFactors,
    /// Indirect-fire support it can give, and how far.
    support: f64,
    support_range_m: f64,
    enemies: Vec<usize>,
    loss: f64,
    fired: bool,
    gone: bool,
}

/// Lanchester-style attrition between formations in contact (doc 02 §14).
// The steps index each other's formations, so plain index loops read best.
#[allow(clippy::needless_range_loop)]
fn combat(world: &mut World, rules: &Rules, clock: &SimClock, seed: u64, geo: Option<&Geo>) {
    let gr = &rules.realism().ground;
    let now = clock.sim_time_ms;
    let utc = clock.start_utc_ms as f64 + now as f64;
    let list = formations(world);
    let jammers = crate::sensors::active_jammers(world, rules);
    let mut cs: Vec<Side> = Vec::with_capacity(list.len());
    for fm in list {
        let (Some(f), Some(pos)) = (world.get::<Formation>(fm.be).cloned(), pos_of(world, fm.be))
        else {
            continue;
        };
        let order = ground_order(world, &fm);
        let class = geo.map_or(wb_geo::TerrainClass::Open, |g| {
            g.terrain_class(pos).unwrap_or(wb_geo::TerrainClass::Open)
        });
        let terrain = crate::sensors::wb_terrain(class);
        let pr = posture_rule(rules, f.posture);
        let attacking = f.posture == GroundPosture::Attack || f.posture == GroundPosture::Moving;
        let protection = pr.protection
            * if attacking {
                1.0
            } else {
                gr.defender_terrain_factor
                    .get(&terrain)
                    .copied()
                    .unwrap_or(1.0)
            };
        let v = value(rules, &f);
        let night = wb_core::sun::sun_elevation(pos, utc)
            < rules.realism().passive.eo_min_sun_elevation_rad;
        let f_night = if night && v > 0.0 {
            let capable: f64 = f
                .equipment
                .iter()
                .filter_map(|e| {
                    let s = stats(rules, e.platform)?;
                    s.night_capable
                        .then_some(f64::from(e.count) * s.combat_value)
                })
                .sum();
            (capable + (v - capable) * gr.night_factor) / v
        } else {
            1.0
        };
        let jammed = jammers.iter().any(|j| {
            j.side != fm.side
                && (j.effects.is_empty()
                    || j.effects.contains(&wb_content::source::EwEffect::Datalink))
                && distance_m(j.pos, pos) <= j.coverage_m
        });
        let f_ew = if jammed { gr.ew_c2_factor } else { 1.0 };
        let f_morale = gr.morale_floor_factor + (1.0 - gr.morale_floor_factor) * f.morale;
        let f_supply = gr.supply_floor_factor + (1.0 - gr.supply_floor_factor) * f.supply;
        let f_fatigue = 1.0 - gr.fatigue_penalty * f.fatigue;
        let f_suppression = if now < f.suppressed_until_ms {
            gr.suppression_factor
        } else {
            1.0
        };
        let common = f_morale * f_supply * f_fatigue * f_ew * f_suppression;
        let (support, support_range_m) = f
            .equipment
            .iter()
            .filter_map(|e| {
                let s = stats(rules, e.platform)?;
                (s.support_value > 0.0 && e.count > 0)
                    .then_some((f64::from(e.count) * s.support_value, s.support_range_m))
            })
            .fold((0.0, 0.0), |a, b| (a.0 + b.0, f64::max(a.1, b.1)));
        cs.push(Side {
            fm,
            pos,
            kind: order.as_ref().map(|o| o.kind),
            withdrawing: order
                .as_ref()
                .is_some_and(|o| o.kind == GroundKind::Withdraw && !o.arrived),
            factors: GroundFactors {
                strength: v * common * f_night,
                protection,
                terrain,
                posture: f.posture,
                enemies: 0,
                f_morale,
                f_supply,
                f_fatigue,
                f_night,
                f_ew,
                f_suppression,
                value_lost: 0.0,
            },
            support: support * common,
            support_range_m,
            f,
            enemies: Vec::new(),
            loss: 0.0,
            fired: false,
            gone: false,
        });
    }
    for i in 0..cs.len() {
        for j in i + 1..cs.len() {
            if cs[i].fm.side != cs[j].fm.side
                && distance_m(cs[i].pos, cs[j].pos) <= gr.contact_range_m
            {
                cs[i].enemies.push(j);
                cs[j].enemies.push(i);
            }
        }
    }
    if cs.iter().all(|c| c.enemies.is_empty()) {
        return;
    }
    let k = gr.lanchester_coefficient;
    let vary = |a: UnitId, b: UnitId, n: u32| {
        let u = rng(
            seed,
            RngPurpose::GroundCombat,
            (u64::from(a.0) << 32) | u64::from(b.0),
            clock.tick,
            n,
        );
        1.0 + gr.random_variation * (2.0 * u - 1.0)
    };
    // Direct fire, split evenly over the enemies in contact.
    for i in 0..cs.len() {
        let n = cs[i].enemies.len();
        if n == 0 {
            continue;
        }
        cs[i].factors.enemies = n as u32;
        cs[i].fired = true;
        let share = cs[i].factors.strength / n as f64;
        for e in cs[i].enemies.clone() {
            let l = k * share / cs[e].factors.protection.max(0.05)
                * vary(cs[i].fm.body, cs[e].fm.body, 0);
            cs[e].loss += l;
        }
    }
    // Artillery out of contact supports a friend in contact: the nearest
    // enemy in range that a friend is fighting.
    for i in 0..cs.len() {
        if cs[i].support <= 0.0 || !cs[i].enemies.is_empty() || cs[i].f.supply <= 0.0 {
            continue;
        }
        let side = cs[i].fm.side;
        let target = (0..cs.len())
            .filter(|&j| {
                cs[j].fm.side != side
                    && cs[j].enemies.iter().any(|&e| cs[e].fm.side == side)
                    && distance_m(cs[i].pos, cs[j].pos) <= cs[i].support_range_m
            })
            .map(|j| (distance_m(cs[i].pos, cs[j].pos), j))
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        if let Some((_, j)) = target {
            let l = k * cs[i].support / cs[j].factors.protection.max(0.05)
                * vary(cs[i].fm.body, cs[j].fm.body, 1);
            cs[j].loss += l;
            cs[i].fired = true;
        }
    }
    // Each side sees who it is fighting.
    {
        let floor = rules.realism().fusion.contact_sigma_floor_m;
        let sigma = CONTACT_SIGMA_M.max(floor);
        let mut pics = world.resource_mut::<SidePictures>();
        for i in 0..cs.len() {
            for &e in &cs[i].enemies {
                let (obs, tgt) = (&cs[i], &cs[e]);
                let ent = (u64::from(obs.fm.body.0) << 32) | u64::from(tgt.fm.body.0);
                let z =
                    |n: u32| rng_normal(seed, RngPurpose::ContactError, ent, clock.tick, 900 + n);
                let p1 = destination_point(tgt.pos, 0.0, z(0) * sigma);
                let at = destination_point(p1, core::f64::consts::FRAC_PI_2, z(1) * sigma);
                let det = Detection {
                    side: obs.fm.side,
                    observer: obs.fm.body,
                    sensor: None,
                    kind: SourceKind::Visual,
                    truth: tgt.fm.gid,
                    tick: clock.tick,
                    at_ms: now,
                    range_m: distance_m(obs.pos, tgt.pos),
                    domain: Domain::Ground,
                    type_ref: TypeRef::Platform(tgt.f.equipment[0].platform),
                    group_size: tgt.f.items().min(u32::from(u16::MAX)) as u16,
                    level: ContactLevel::Type,
                    count_resolved: false,
                    quality: Quality::Track,
                    position: Some(Measured {
                        at,
                        alt_m: 0.0,
                        heading: 0.0,
                        speed_mps: 0.0,
                        a_m: sigma,
                        b_m: sigma,
                        rot: 0.0,
                    }),
                    bearing: None,
                    network: None,
                    fixed: false,
                };
                pics.deliver(now, det);
            }
        }
        pics.sort_pending();
    }
    // Losses, morale and what follows.
    let mut retreats: Vec<(usize, String)> = Vec::new();
    for i in 0..cs.len() {
        if !cs[i].fired && cs[i].loss <= 0.0 {
            continue;
        }
        let c = &mut cs[i];
        let before = bulk(rules, &c.f, false);
        let phi = if before > 0.0 {
            (c.loss / before).min(1.0)
        } else {
            1.0
        };
        let mut mag = world.get::<Magazine>(c.fm.be).cloned();
        let lost = remove(
            rules,
            &mut c.f,
            mag.as_mut(),
            |_, line| f64::from(line.count) * phi,
            None,
        );
        c.factors.value_lost = c.loss;
        c.f.morale = (c.f.morale - phi * gr.morale_loss_factor).clamp(0.0, 1.0);
        c.f.last_combat_ms = Some(now);
        let s = strength(rules, &c.f);
        let (fm, f, factors) = (c.fm, c.f.clone(), c.factors);
        if let Some(m) = mag {
            world.entity_mut(fm.be).insert(m);
        }
        *world.get_mut::<Formation>(fm.be).expect("formation") = f.clone();
        if lost.items > 0 {
            emit(
                world,
                fm.side,
                EventKind::GroundCombat {
                    formation: fm.gid,
                    lost_equipment: lost.items,
                    lost_personnel: lost.personnel,
                    strength_pct: s * 100.0,
                    morale_pct: f.morale * 100.0,
                    factors,
                },
            );
        }
        if destroyed(world, rules, fm.body, &f) {
            cs[i].gone = true;
            continue;
        }
        if cs[i].enemies.is_empty() || cs[i].withdrawing {
            continue;
        }
        let hold = cs[i].kind == Some(GroundKind::Hold);
        if s < gr.retreat_strength {
            retreats.push((i, format!("strength down to {:.0}%", s * 100.0)));
        } else if f.morale < gr.retreat_morale && !hold {
            retreats.push((i, format!("morale down to {:.0}%", f.morale * 100.0)));
        }
    }
    let points = supply_points(world, rules);
    for (i, reason) in &retreats {
        let c = &cs[*i];
        let n = c.enemies.len().max(1) as f64;
        let (lat, lon) = c.enemies.iter().fold((0.0, 0.0), |a, &e| {
            (a.0 + cs[e].pos.lat / n, a.1 + cs[e].pos.lon / n)
        });
        let away = initial_bearing(LatLon::new(lat, lon), c.pos);
        let dest = points
            .iter()
            .filter(|(s, _)| *s == c.fm.side)
            .map(|(_, at)| (distance_m(c.pos, *at), *at))
            .filter(|(d, _)| *d <= 5.0 * gr.retreat_distance_m && *d > gr.contact_range_m)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map_or_else(
                || destination_point(c.pos, away, gr.retreat_distance_m),
                |(_, at)| at,
            );
        if let Some(mut g) = world.get_mut::<Group>(c.fm.ge) {
            g.wp = 0;
            g.on_station = false;
            g.task = Task::Ground(GroundOrder::new(GroundKind::Withdraw, Some(dest), None));
        }
        emit(
            world,
            c.fm.side,
            EventKind::FormationRetreated {
                formation: c.fm.gid,
                reason: reason.clone(),
            },
        );
    }
    // Breakthrough: an attacker whose every enemy in contact broke or died.
    let broke: Vec<usize> = (0..cs.len())
        .filter(|i| cs[*i].gone || retreats.iter().any(|r| r.0 == *i))
        .collect();
    for i in 0..cs.len() {
        if cs[i].gone
            || cs[i].kind != Some(GroundKind::Attack)
            || cs[i].enemies.is_empty()
            || broke.contains(&i)
            || !cs[i].enemies.iter().all(|e| broke.contains(e))
        {
            continue;
        }
        emit(
            world,
            cs[i].fm.side,
            EventKind::Breakthrough {
                formation: cs[i].fm.gid,
            },
        );
    }
}

/// What the unit card shows of a formation. Own side only (rule 1).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct FormationView {
    pub equipment: Vec<EquipmentView>,
    pub personnel: u32,
    pub personnel_full: u32,
    pub posture: GroundPosture,
    pub strength_pct: f64,
    pub morale_pct: f64,
    pub supply_pct: f64,
    pub fatigue_pct: f64,
    pub in_contact: bool,
    pub suppressed: bool,
    /// The team is designating a contact.
    pub designating: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct EquipmentView {
    /// Content platform id.
    pub id: String,
    pub name: String,
    pub count: u32,
    pub full: u32,
}

/// The formation of a group's lead, if it is one.
pub fn view(world: &World, side: u8, lead: UnitId) -> Option<FormationView> {
    let e = entity(world, lead)?;
    let f = world.get::<Formation>(e)?;
    let rules = world.resource::<Rules>();
    let now = world.resource::<SimClock>().sim_time_ms;
    let designating = world.get_resource::<GroundState>().and_then(|g| {
        g.designations
            .iter()
            .find(|((s, _), body)| *s == side && **body == lead)
            .map(|((_, c), _)| c.0)
    });
    Some(FormationView {
        equipment: f
            .equipment
            .iter()
            .map(|l| {
                let p = rules.platform(l.platform);
                EquipmentView {
                    id: rules.db.str(p.id).to_owned(),
                    name: rules.db.str(p.short_name).to_owned(),
                    count: l.count,
                    full: l.full,
                }
            })
            .collect(),
        personnel: f.personnel,
        personnel_full: f.personnel_full,
        posture: f.posture,
        strength_pct: strength(rules, f) * 100.0,
        morale_pct: f.morale * 100.0,
        supply_pct: f.supply * 100.0,
        fatigue_pct: f.fatigue * 100.0,
        in_contact: f.in_contact,
        suppressed: now < f.suppressed_until_ms,
        designating,
    })
}

/// Name of a formation's group, for messages.
pub fn name(world: &World, gid: UnitId) -> String {
    entity(world, gid)
        .and_then(|e| world.get::<Identity>(e))
        .map_or_else(String::new, |i| i.name.clone())
}
