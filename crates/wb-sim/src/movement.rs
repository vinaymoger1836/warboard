//! Aircraft and ship kinematics on the sphere (doc 02 §5).
//!
//! Leads (slot 0) steer toward the target the task logic set: heading limited
//! by turn rate, speed by acceleration, altitude by climb or descent rate,
//! then a great-circle step. Followers keep rigid formation slots behind the
//! lead. Each unit's update reads only its own state and its lead's, so the
//! iteration order cannot change the result.

use bevy_ecs::prelude::*;
use core::f64::consts::{PI, TAU};
use wb_core::UnitId;
use wb_core::geo::{LatLon, destination_point, normalize_bearing};
use wb_core::math::{atan2, hypot};

use crate::clock::SimClock;
use crate::components::{Group, GroupMember, Kinematics, Mode, PlatformRef, Posture, Steer};
use crate::rules::Rules;
use crate::world::entity;

/// Distance a refuelling receiver keeps behind its tanker.
const TANKER_TRAIL_M: f64 = 300.0;

/// Signed smallest angle from `from` to `to`, in (-π, π].
pub fn angle_diff(from: f64, to: f64) -> f64 {
    let d = (to - from).rem_euclid(TAU);
    if d > PI { d - TAU } else { d }
}

/// Integrates every moving lead toward its steer target.
pub fn integrate(world: &mut World) {
    let rules = world.resource::<Rules>().clone();
    let dt = world.resource::<SimClock>().dt_s();
    let mut q = world.query::<(
        &GroupMember,
        &PlatformRef,
        &Posture,
        &Steer,
        &mut Kinematics,
    )>();
    for (m, p, posture, steer, mut k) in q.iter_mut(world) {
        if m.slot != 0 || *posture != Posture::Moving {
            continue;
        }
        let p = rules.platform(p.0);
        let air = rules.is_air(p);

        let w = rules.turn_rate(p, k.speed_mps);
        let dh = angle_diff(k.heading, steer.heading).clamp(-w * dt, w * dt);
        let heading = normalize_bearing(k.heading + dh);

        let a = rules.accel_mps2(p) * dt;
        let v0 = k.speed_mps;
        let v1 = (v0 + (steer.speed_mps - v0).clamp(-a, a)).max(0.0);

        let (alt, climb) = if air {
            let up = rules.climb_rate_mps(p) * dt;
            let down = rules.descent_rate_mps() * dt;
            let da = (steer.alt_m - k.alt_m).clamp(-down, up);
            (k.alt_m + da, da / dt)
        } else {
            (0.0, 0.0)
        };

        let next = destination_point(k.pos(), heading, (v0 + v1) / 2.0 * dt);
        *k = Kinematics {
            lat: next.lat,
            lon: next.lon,
            alt_m: alt,
            heading,
            speed_mps: v1,
            climb_mps: climb,
        };
    }
}

/// Formation offset of a slot as (bearing relative to heading, distance):
/// slots alternate right and left, one spacing back and aside per rank.
fn slot_offset(slot: u8, spacing: f64) -> (f64, f64) {
    let rank = f64::from(slot.div_ceil(2));
    let side = if slot % 2 == 1 { 1.0 } else { -1.0 };
    let (fwd, right) = (-rank * spacing, side * rank * spacing);
    (atan2(right, fwd), hypot(fwd, right))
}

fn place_followers(world: &mut World, members: &[UnitId], spacing: f64) {
    let Some(lead) = members.first().and_then(|l| entity(world, *l)) else {
        return;
    };
    let Some(lk) = world.get::<Kinematics>(lead).copied() else {
        return;
    };
    for m in &members[1..] {
        let Some(e) = entity(world, *m) else { continue };
        if world.get::<Posture>(e) != Some(&Posture::Moving) {
            continue;
        }
        let slot = world.get::<GroupMember>(e).map_or(1, |g| g.slot);
        let (rel, d) = slot_offset(slot, spacing);
        let p = destination_point(lk.pos(), normalize_bearing(lk.heading + rel), d);
        if let Some(mut k) = world.get_mut::<Kinematics>(e) {
            *k = Kinematics {
                lat: p.lat,
                lon: p.lon,
                ..lk
            };
        }
    }
}

/// Places followers in their slots; receivers in contact ride behind their
/// tanker. Tankers' own groups go first, so a receiver sees the tanker's
/// final position for this tick.
pub fn formation(world: &mut World) {
    let spacing = world
        .resource::<Rules>()
        .realism()
        .movement
        .formation_spacing_m;
    let mut q = world.query::<(&crate::components::Identity, &Group)>();
    let mut groups: Vec<(UnitId, Vec<UnitId>, Mode)> = q
        .iter(world)
        .map(|(i, g)| (i.unit, g.members.clone(), g.mode))
        .collect();
    groups.sort_by_key(|g| g.0);

    for (_, members, mode) in &groups {
        if !matches!(mode, Mode::Refuelling { .. }) {
            place_followers(world, members, spacing);
        }
    }
    for (_, members, mode) in &groups {
        let Mode::Refuelling { tanker, .. } = mode else {
            continue;
        };
        let (Some(te), Some(le)) = (
            entity(world, *tanker),
            members.first().and_then(|l| entity(world, *l)),
        ) else {
            continue;
        };
        let Some(tk) = world.get::<Kinematics>(te).copied() else {
            continue;
        };
        let p: LatLon =
            destination_point(tk.pos(), normalize_bearing(tk.heading + PI), TANKER_TRAIL_M);
        if let Some(mut k) = world.get_mut::<Kinematics>(le) {
            *k = Kinematics {
                lat: p.lat,
                lon: p.lon,
                ..tk
            };
        }
        place_followers(world, members, spacing);
    }
}
