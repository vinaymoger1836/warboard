//! Damage (doc 02 §13, P5): what a hit does to aircraft, ships, ground
//! units, weapons and bases, and the upkeep that follows (sinking, repair).
//!
//! - **Aircraft:** destroyed with `p_kill_on_hit` (`rng(Damage)`), else
//!   damaged: the group aborts and returns to base.
//! - **Ships and ground units:** damage points; warhead kg × the warhead's
//!   multiplier × `points_per_warhead_kg`. A ship hit lands on a subsystem
//!   (`rng(HitLocation)` over `ship_hits`): radar (sensors off), VLS or
//!   launcher (no fire from that station kind), propulsion (speed ×
//!   `propulsion_speed_factor`). At or below `ship_out_of_action_fraction` a
//!   ship is out of action; at zero it sinks after `ship_sink_delay`.
//!   Ground units at zero are destroyed.
//! - **Weapons:** a hit weapon is intercepted.
//! - **Bases:** a burst within `base_radius` of a base damages one component
//!   (by weight) with probability `effective kg / hardness`; it is repaired
//!   after a time between the component's minimum and maximum. A cut runway
//!   closes the base to aircraft; a hit munitions store loses part of its
//!   stock.
//!
//! Events go to the damaged side only (ADR-027).

use bevy_ecs::prelude::*;
use wb_content::compiled::{Durability, Weapon};
use wb_content::source::realism::{BaseComponentKind, Subsystem};
use wb_core::geo::{LatLon, distance_m};
use wb_core::{RngPurpose, UnitId, rng};

use crate::bases;
use crate::clock::SimClock;
use crate::components::{
    Allegiance, BaseSite, Group, GroupMember, Health, Identity, Mode, PlatformRef, Posture,
    ReturnWhy, ScriptedTarget, SystemMemberRole, WeaponFlight,
};
use crate::events::{EventKind, Outcome, emit};
use crate::picture::SidePictures;
use crate::rules::Rules;
use crate::world::{UnitIndex, entity};

fn name_of(kind: BaseComponentKind) -> &'static str {
    match kind {
        BaseComponentKind::Runway => "runway",
        BaseComponentKind::Shelters => "shelters",
        BaseComponentKind::Fuel => "fuel",
        BaseComponentKind::Munitions => "munitions store",
        BaseComponentKind::Radar => "radar",
        BaseComponentKind::C2 => "C2",
    }
}

fn subsystem_name(s: Subsystem) -> &'static str {
    match s {
        Subsystem::Hull => "hull",
        Subsystem::Radar => "radar",
        Subsystem::Vls => "VLS",
        Subsystem::Launcher => "launcher",
        Subsystem::Propulsion => "propulsion",
    }
}

/// Effective warhead kg: warhead mass × the realism multiplier of its type.
fn effective_kg(rules: &Rules, w: &Weapon) -> f64 {
    let m = w
        .warhead
        .and_then(|h| rules.realism().damage.warhead_multiplier.get(&h).copied())
        .unwrap_or(1.0);
    w.warhead_kg * m
}

/// A weighted choice over `(item, weight)` with `u` in [0, 1).
fn pick<T: Copy>(items: &[(T, f64)], u: f64) -> Option<T> {
    let total: f64 = items.iter().map(|x| x.1.max(0.0)).sum();
    if total <= 0.0 {
        return None;
    }
    let mut x = u * total;
    for (item, wgt) in items {
        x -= wgt.max(0.0);
        if x < 0.0 {
            return Some(*item);
        }
    }
    items.last().map(|x| x.0)
}

/// A weapon `weapon` (fired by `w`) hit `target`.
pub fn hit(
    world: &mut World,
    rules: &Rules,
    seed: u64,
    weapon: UnitId,
    w: &Weapon,
    target: UnitId,
) {
    let Some(te) = entity(world, target) else {
        return;
    };
    let tick = world.resource::<SimClock>().tick;
    let now = world.resource::<SimClock>().sim_time_ms;
    let side = world.get::<Allegiance>(te).map_or(0, |a| a.side);
    if world.get::<WeaponFlight>(te).is_some() {
        crate::weapons::end_flight(world, rules, target, Outcome::Intercepted);
        return;
    }
    if world.get::<ScriptedTarget>(te).is_some() {
        destroy(world, target);
        return;
    }
    let Some(p) = world.get::<PlatformRef>(te).map(|p| rules.platform(p.0)) else {
        return;
    };
    let dr = &rules.realism().damage;
    let draw = |purpose, n: u32| rng(seed, purpose, u64::from(target.0), tick, weapon.0 ^ n);
    match p.durability {
        Durability::Aircraft { p_kill_on_hit } => {
            if draw(RngPurpose::Damage, 0) < p_kill_on_hit {
                destroy(world, target);
                return;
            }
            let mut h = world.get::<Health>(te).cloned().unwrap_or(Health {
                points: 0.0,
                max: 0.0,
                damaged: false,
                out: Vec::new(),
                sink_at_ms: None,
            });
            h.damaged = true;
            world.entity_mut(te).insert(h);
            let aborted = abort(world, rules, side, te);
            emit(
                world,
                side,
                EventKind::Damage {
                    unit: target,
                    points_pct: None,
                    subsystem: None,
                    aborted,
                },
            );
        }
        Durability::Ship { damage_points } | Durability::Ground { damage_points } => {
            let ship = matches!(p.durability, Durability::Ship { .. });
            let mut h = world.get::<Health>(te).cloned().unwrap_or(Health {
                points: damage_points,
                max: damage_points,
                damaged: false,
                out: Vec::new(),
                sink_at_ms: None,
            });
            h.points -= effective_kg(rules, w) * dr.points_per_warhead_kg;
            h.damaged = true;
            let mut sub = None;
            if ship {
                let table: Vec<(Subsystem, f64)> =
                    dr.ship_hits.iter().map(|(k, v)| (*k, *v)).collect();
                if let Some(s) = pick(&table, draw(RngPurpose::HitLocation, 0)) {
                    sub = Some(s);
                    if s != Subsystem::Hull && !h.out.contains(&s) {
                        h.out.push(s);
                        h.out.sort();
                    }
                }
            }
            let pct = (h.points / h.max.max(1e-9) * 100.0).max(0.0);
            let gone = h.points <= 0.0;
            if gone && ship && h.sink_at_ms.is_none() {
                h.sink_at_ms = Some(now + (dr.ship_sink_delay_s * 1_000.0) as u64);
            }
            world.entity_mut(te).insert(h);
            emit(
                world,
                side,
                EventKind::Damage {
                    unit: target,
                    points_pct: Some(pct),
                    subsystem: sub.map(|s| subsystem_name(s).to_owned()),
                    aborted: false,
                },
            );
            if gone && !ship {
                destroy(world, target);
            }
        }
    }
}

/// Sends a damaged aircraft's group home (mission abort).
fn abort(world: &mut World, rules: &Rules, side: u8, te: Entity) -> bool {
    let Some(gid) = world.get::<GroupMember>(te).map(|m| m.group) else {
        return false;
    };
    let Some(ge) = entity(world, gid) else {
        return false;
    };
    let mode = world.get::<Group>(ge).map(|g| g.mode);
    if !matches!(
        mode,
        Some(Mode::Task | Mode::ToTanker { .. } | Mode::Refuelling { .. })
    ) {
        return false;
    }
    let list = bases::list(world);
    let mut g = std::mem::take(&mut *world.get_mut::<Group>(ge).expect("group"));
    crate::tasks::go_home(world, rules, &list, side, gid, &mut g, ReturnWhy::Damaged);
    let home = matches!(g.mode, Mode::Return { .. });
    *world.get_mut::<Group>(ge).expect("group") = g;
    home
}

/// A burst at `at` (a point strike): damages the nearest base within
/// `base_radius`.
pub fn burst(world: &mut World, rules: &Rules, seed: u64, weapon: UnitId, w: &Weapon, at: LatLon) {
    let dr = &rules.realism().damage;
    let tick = world.resource::<SimClock>().tick;
    let now = world.resource::<SimClock>().sim_time_ms;
    let Some(b) = bases::list(world)
        .into_iter()
        .map(|b| (distance_m(at, b.at), b))
        .filter(|(d, _)| *d <= dr.base_radius_m)
        .min_by(|x, y| x.0.total_cmp(&y.0).then(x.1.id.cmp(&y.1.id)))
        .map(|(_, b)| b)
    else {
        return;
    };
    let draw = |purpose, n: u32| rng(seed, purpose, u64::from(b.id.0), tick, weapon.0 ^ n);
    let table: Vec<(BaseComponentKind, f64)> = dr
        .base_components
        .iter()
        .map(|(k, r)| (*k, r.weight))
        .collect();
    let Some(kind) = pick(&table, draw(RngPurpose::HitLocation, 0)) else {
        return;
    };
    let rule = &dr.base_components[&kind];
    let p = (effective_kg(rules, w) / rule.hardness_kg.max(1e-9)).min(1.0);
    if draw(RngPurpose::Damage, 0) >= p {
        return;
    }
    let repair =
        rule.repair_min_s + (rule.repair_max_s - rule.repair_min_s) * draw(RngPurpose::Damage, 1);
    let until = now + (repair * 1_000.0) as u64;
    let Some(be) = entity(world, b.id) else {
        return;
    };
    {
        let mut site = world.get_mut::<BaseSite>(be).expect("base");
        let e = site.damaged.entry(kind).or_insert(until);
        *e = (*e).max(until);
        if kind == BaseComponentKind::Munitions {
            let keep = 1.0 - dr.munitions_store_loss_fraction;
            for n in site.stock.values_mut() {
                *n = (f64::from(*n) * keep).floor() as u32;
            }
            site.stock.retain(|_, n| *n > 0);
        }
    }
    emit(
        world,
        b.side,
        EventKind::BaseComponentDamaged {
            base: b.id,
            component: name_of(kind).to_owned(),
            repaired_at_ms: until,
        },
    );
}

/// Removes a unit from the world: its group (an empty group goes too), its
/// base, the index and every picture; `UnitDestroyed` to its side.
pub fn destroy(world: &mut World, unit: UnitId) {
    let Some(e) = entity(world, unit) else { return };
    let side = world.get::<Allegiance>(e).map_or(0, |a| a.side);
    let name = world
        .get::<Identity>(e)
        .map_or_else(String::new, |i| i.name.clone());
    let was_c2 = world
        .get::<SystemMemberRole>(e)
        .is_some_and(|r| r.0 == wb_content::source::system::ComponentRole::C2);
    if let Some(m) = world.get::<GroupMember>(e).copied()
        && let Some(ge) = entity(world, m.group)
    {
        let remaining = {
            let mut g = world.get_mut::<Group>(ge).expect("group");
            g.members.retain(|x| *x != unit);
            g.members.clone()
        };
        if remaining.is_empty() {
            world.despawn(ge);
            world.resource_mut::<UnitIndex>().0.remove(&m.group);
            crate::picture::forget(&mut world.resource_mut::<SidePictures>(), m.group);
        } else {
            crate::orders::reslot(world, m.group, &remaining);
            if was_c2 {
                let c2_survives = remaining.iter().any(|rem| {
                    entity(world, *rem).is_some_and(|re| {
                        world
                            .get::<SystemMemberRole>(re)
                            .is_some_and(|r| r.0 == wb_content::source::system::ComponentRole::C2)
                    })
                });
                if !c2_survives {
                    let needs_auto = {
                        let g = world.get::<Group>(ge).expect("group");
                        g.system.is_some() && g.autonomous_after_ms.is_none()
                    };
                    if needs_auto {
                        let (delay_s, delay_ms) = {
                            let rules = world.resource::<Rules>();
                            let country = world
                                .get::<Allegiance>(ge)
                                .map(|a| a.country);
                            let reaction_time_s = country
                                .and_then(|c| rules.db.countries.get(c.0 as usize))
                                .and_then(|c| rules.db.doctrines.get(c.doctrine.0 as usize))
                                .map_or(2.0, |d| d.reaction_time_s);
                            let factor = rules
                                .realism()
                                .tewa
                                .iads_autonomous_reaction_factor;
                            let delay_s = reaction_time_s * factor;
                            (delay_s, (delay_s * 1_000.0) as u64)
                        };
                        let now = world.resource::<SimClock>().sim_time_ms;
                        if let Some(mut g) = world.get_mut::<Group>(ge) {
                            g.autonomous_after_ms = Some(now + delay_ms);
                        }
                        emit(
                            world,
                            side,
                            EventKind::AutonomousMode {
                                group: m.group,
                                delay_s,
                            },
                        );
                    }
                }
            }
        }
    }
    if let Some(Posture::AtBase { base, .. }) = world.get::<Posture>(e).copied()
        && let Some(be) = entity(world, base)
        && let Some(mut site) = world.get_mut::<BaseSite>(be)
    {
        site.stationed.retain(|x| *x != unit);
    }
    world.despawn(e);
    world.resource_mut::<UnitIndex>().0.remove(&unit);
    crate::picture::forget(&mut world.resource_mut::<SidePictures>(), unit);
    emit(world, side, EventKind::UnitDestroyed { unit, name });
}

/// Logistics upkeep: ships whose sink time came sink, base components whose
/// repair ended are back.
pub fn upkeep(world: &mut World, now: u64) {
    let index: Vec<(UnitId, Entity)> = world
        .resource::<UnitIndex>()
        .0
        .iter()
        .map(|(u, e)| (*u, *e))
        .collect();
    for (id, e) in index {
        if world
            .get::<Health>(e)
            .and_then(|h| h.sink_at_ms)
            .is_some_and(|t| now >= t)
        {
            destroy(world, id);
            continue;
        }
        let Some(site) = world.get::<BaseSite>(e) else {
            continue;
        };
        let done: Vec<BaseComponentKind> = site
            .damaged
            .iter()
            .filter(|(_, t)| now >= **t)
            .map(|(k, _)| *k)
            .collect();
        if done.is_empty() {
            continue;
        }
        let side = world.get::<Allegiance>(e).map_or(0, |a| a.side);
        for k in done {
            world
                .get_mut::<BaseSite>(e)
                .expect("base")
                .damaged
                .remove(&k);
            emit(
                world,
                side,
                EventKind::BaseComponentRepaired {
                    base: id,
                    component: name_of(k).to_owned(),
                },
            );
        }
    }
}
