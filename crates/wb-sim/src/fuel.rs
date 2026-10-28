//! Fuel: burn every tick, bingo thresholds, air-to-air refuelling (doc 02 §6).

use bevy_ecs::prelude::*;
use wb_content::source::AarSystem;
use wb_core::UnitId;
use wb_core::geo::distance_m;

use crate::bases::{self, BaseInfo};
use crate::clock::SimClock;
use crate::components::{
    Allegiance, BaseRef, Fuel, Group, Identity, Kinematics, Mode, PlatformRef, Posture, TankerState,
};
use crate::events::{EventKind, emit};
use crate::rules::Rules;
use crate::world::entity;

/// A receiver counts as full within this much of capacity.
pub const FULL_MARGIN_KG: f64 = 1.0;
/// How often each unit looks for its nearest suitable base again.
const BINGO_BASE_PERIOD_MS: u64 = 30_000;

/// Burns fuel for every moving platform.
pub fn burn(world: &mut World) {
    let rules = world.resource::<Rules>().clone();
    let dt = world.resource::<SimClock>().dt_s();
    let mut q = world.query::<(&PlatformRef, &Posture, &Kinematics, &mut Fuel)>();
    for (p, posture, k, mut f) in q.iter_mut(world) {
        if *posture != Posture::Moving {
            continue;
        }
        let p = rules.platform(p.0);
        let used = if rules.is_air(p) {
            rules.burn_kg_s(p, k.speed_mps, k.alt_m, k.climb_mps) * dt
        } else {
            rules.ship_burn_per_m(p, k.speed_mps) * k.speed_mps * dt * f.max_kg
        };
        f.kg = (f.kg - used).max(0.0);
    }
}

/// Refreshes the bingo threshold of one unit: fuel to reach the nearest
/// suitable friendly base at cruise, plus the reserve. Returns the base.
pub fn update_bingo(
    world: &mut World,
    rules: &Rules,
    clock: &SimClock,
    bases: &[BaseInfo],
    unit: UnitId,
) -> Option<UnitId> {
    let e = entity(world, unit)?;
    let p = rules.platform(world.get::<PlatformRef>(e)?.0);
    let k = *world.get::<Kinematics>(e)?;
    let side = world.get::<Allegiance>(e)?.side;
    let home = world.get::<BaseRef>(e).and_then(|r| r.home);
    let current = world.get::<Fuel>(e)?.bingo_base;
    let refresh = current.is_none_or(|b| !bases.iter().any(|x| x.id == b))
        || clock.due_staggered(BINGO_BASE_PERIOD_MS, unit.0);
    let base = if refresh {
        bases::nearest_suitable(rules, bases, side, p, k.pos(), home).map(|b| b.id)
    } else {
        current
    };
    let reserve = rules.realism().logistics.fuel_reserve_fraction;
    let bingo_kg = base
        .and_then(|b| bases.iter().find(|x| x.id == b))
        .map_or(0.0, |b| {
            rules.fuel_for_distance(p, distance_m(k.pos(), b.at), k.alt_m) * (1.0 + reserve)
        });
    let mut f = world.get_mut::<Fuel>(e)?;
    f.bingo_base = base;
    f.bingo_kg = bingo_kg;
    base
}

/// Reserve a unit should still have on landing at `base`: the reserve
/// fraction of the fuel to get there from here.
pub fn reserve_for(world: &World, rules: &Rules, unit: UnitId, base: &BaseInfo) -> f64 {
    let Some(e) = entity(world, unit) else {
        return 0.0;
    };
    let (Some(p), Some(k)) = (world.get::<PlatformRef>(e), world.get::<Kinematics>(e)) else {
        return 0.0;
    };
    let p = rules.platform(p.0);
    rules.fuel_for_distance(p, distance_m(k.pos(), base.at), k.alt_m)
        * rules.realism().logistics.fuel_reserve_fraction
}

/// Whether the receiving systems of all `members` fit the tanker.
pub fn aar_compatible(world: &World, rules: &Rules, tanker: UnitId, members: &[UnitId]) -> bool {
    let Some(tp) = entity(world, tanker).and_then(|e| world.get::<PlatformRef>(e)) else {
        return false;
    };
    let Some(t) = &rules.platform(tp.0).tanker else {
        return false;
    };
    members.iter().all(|m| {
        entity(world, *m)
            .and_then(|e| world.get::<PlatformRef>(e))
            .and_then(|p| rules.platform(p.0).fuel.as_ref())
            .is_some_and(|f| f.aar != AarSystem::None && t.systems.contains(&f.aar))
    })
}

/// Air-to-air refuelling: each tanker serves one receiver at a time, groups
/// in `UnitId` order and members in slot order, at its transfer rate. A
/// member is done when full (or the tanker has nothing left to give).
pub fn aar(world: &mut World) {
    let rules = world.resource::<Rules>().clone();
    let dt = world.resource::<SimClock>().dt_s();
    let mut q = world.query::<(Entity, &Identity, &Group)>();
    let mut queue: Vec<(UnitId, UnitId, Entity, UnitId)> = q
        .iter(world)
        .filter_map(|(ge, i, g)| match g.mode {
            Mode::Refuelling { tanker, served } => g
                .members
                .get(usize::from(served))
                .map(|m| (tanker, i.unit, ge, *m)),
            _ => None,
        })
        .collect();
    queue.sort_by_key(|q| (q.0, q.1));
    queue.dedup_by_key(|q| q.0);

    for (tanker, _, ge, member) in queue {
        let (Some(te), Some(re)) = (entity(world, tanker), entity(world, member)) else {
            continue;
        };
        let Some(spec) = world
            .get::<PlatformRef>(te)
            .and_then(|p| rules.platform(p.0).tanker.clone())
        else {
            continue;
        };
        let side = world.get::<Allegiance>(te).map_or(0, |a| a.side);
        let left = world
            .get::<TankerState>(te)
            .map_or(0.0, |t| t.offload_left_kg)
            .min(world.get::<Fuel>(te).map_or(0.0, |f| f.kg));
        let Some(rf) = world.get::<Fuel>(re).copied() else {
            continue;
        };
        let amount = (spec.transfer_rate_kg_s * dt)
            .min(rf.max_kg - rf.kg)
            .min(left)
            .max(0.0);
        if amount > 0.0 {
            let mut f = world.get_mut::<Fuel>(re).expect("receiver fuel");
            f.kg += amount;
            f.reserve_at_bingo_kg = None;
            if let Some(mut t) = world.get_mut::<TankerState>(te) {
                t.offload_left_kg -= amount;
            }
            if let Some(mut f) = world.get_mut::<Fuel>(te) {
                f.kg -= amount;
            }
        }
        let fuel_kg = rf.kg + amount;
        if fuel_kg >= rf.max_kg - FULL_MARGIN_KG || amount >= left {
            if let Some(mut g) = world.get_mut::<Group>(ge)
                && let Mode::Refuelling { served, .. } = &mut g.mode
            {
                *served += 1;
            }
            emit(
                world,
                side,
                EventKind::Refuelled {
                    unit: member,
                    tanker,
                    fuel_kg,
                },
            );
        }
    }
}
