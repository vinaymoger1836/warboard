//! Endgame and countermeasures (doc 02 §11, P5).
//!
//! - **Awareness:** a target knows of a weapon when its side's picture holds
//!   the weapon or the shooter at track quality, or when the weapon's seeker
//!   is locked on it and is of a kind its warning receiver hears
//!   (`countermeasures.rwr_seekers`).
//! - **Threat reaction:** an aware aircraft group with threat reaction
//!   `beam` turns the weapon onto its beam; inside `release_range_m` of a
//!   locked weapon the target dispenses `rounds_per_threat` of every
//!   countermeasure that defeats the weapon's seeker, once per weapon.
//! - **Roll:** `Pk = clamp(pk_base × f_energy × f_aspect × f_maneuver × f_cm
//!   × f_ew × f_env)`, hit when `rng(seed, Endgame, weapon, tick, 0) < Pk`.
//!   A rocket at `V_nom` (`endgame.nominal_speed_fraction` of the way from
//!   `V_min` to `V_max`) or faster has full `f_energy` and full endgame g.
//!   A miss that would have hit with `f_cm = 1` is `Decoyed`. CEP weapons
//!   roll a Rayleigh radial miss against their lethal radius instead.
//!   `f_ew` is 1 until electronic warfare (P6); `f_env` carries the
//!   anti-radiation memory-mode factor.

use bevy_ecs::prelude::*;
use core::f64::consts::{FRAC_PI_2, PI};
use wb_content::compiled::Weapon;
use wb_content::source::weapon::ProfileKind;
use wb_content::source::{Domain, Seeker, TargetClass};
use wb_core::geo::{LatLon, destination_point, distance_m, initial_bearing};
use wb_core::math::{acos, cos, exp, ln, pow, sin, sqrt};
use wb_core::{RngPurpose, UnitId, rng};

use crate::components::{
    Countermeasures, Group, GroupMember, Kinematics, PlatformRef, Posture, ScriptedTarget,
    SeekerState, ThreatReaction, WeaponFlight, WeaponTarget,
};
use crate::events::{Endgame, Outcome};
use crate::movement::angle_diff;
use crate::picture::{Quality, SidePictures};
use crate::rules::Rules;
use crate::world::entity;
use wb_core::units::rad_to_deg;

/// An aware group keeps beaming this long after its last warning.
const EVADE_HOLD_MS: u64 = 2_000;

/// Target class of a truth object, for `pk_base`.
fn target_class(world: &World, rules: &Rules, e: Entity) -> Option<TargetClass> {
    if let Some(p) = world.get::<PlatformRef>(e) {
        return Some(rules.platform(p.0).target_class);
    }
    if let Some(f) = world.get::<WeaponFlight>(e) {
        return rules.db.weapons[f.weapon.0 as usize].target_class;
    }
    world
        .get::<ScriptedTarget>(e)
        .and_then(|s| rules.db.weapons[s.weapon.0 as usize].target_class)
}

/// The unit a weapon is homing on, or (unlocked) the unit or group key it
/// was fired at.
pub fn threatened(f: &WeaponFlight) -> Option<UnitId> {
    match (f.seeker, f.target) {
        (SeekerState::Locked { unit, .. }, _) => Some(unit),
        (_, WeaponTarget::Track { key, .. }) => Some(key),
        (_, WeaponTarget::Truth(u)) => Some(u),
        (_, WeaponTarget::Point { .. }) => None,
    }
}

/// Whether `side`'s picture holds `truth` at track quality.
fn tracked(world: &World, side: u8, truth: UnitId) -> bool {
    let pics = world.resource::<SidePictures>();
    pics.sides
        .get(usize::from(side))
        .and_then(|p| p.by_truth.get(&truth).and_then(|id| p.get(*id)))
        .is_some_and(|c| !c.lost && c.quality <= Quality::Track)
}

/// The threat reaction of the weapon's target this tick; updates the
/// weapon's awareness and countermeasure record.
pub fn react(world: &mut World, rules: &Rules, now: u64, id: UnitId, f: &mut WeaponFlight) {
    let Some(t) = threatened(f) else { return };
    let Some(te) = entity(world, t) else { return };
    // The group that turns: the target's group, or the target if it is one.
    let (gid, unit) = match world.get::<GroupMember>(te) {
        Some(m) => (m.group, t),
        None => match world.get::<Group>(te).and_then(|g| g.lead()) {
            Some(lead) => (t, lead),
            None => (t, t),
        },
    };
    let (Some(ge), Some(ue)) = (entity(world, gid), entity(world, unit)) else {
        return;
    };
    let Some(side) = world
        .get::<crate::components::Allegiance>(ue)
        .map(|a| a.side)
    else {
        return;
    };
    let Some(p) = world.get::<PlatformRef>(ue).map(|p| rules.platform(p.0)) else {
        return;
    };
    if (p.domain != Domain::Air && p.domain != Domain::Surface)
        || matches!(world.get::<Posture>(ue), Some(Posture::AtBase { .. }))
    {
        return;
    }
    let w = &rules.db.weapons[f.weapon.0 as usize];
    let locked_on = matches!(f.seeker, SeekerState::Locked { unit: u, .. } if u == unit);
    let shooter_key = entity(world, f.shooter)
        .and_then(|e| world.get::<GroupMember>(e).map(|m| m.group))
        .unwrap_or(f.shooter);
    let rwr = locked_on
        && rules
            .realism()
            .countermeasures
            .rwr_seekers
            .contains(&w.guidance.terminal);
    let aware = rwr || tracked(world, side, id) || tracked(world, side, shooter_key);
    if !aware {
        return;
    }
    f.target_aware = true;
    let (Some(tk), Some(wk)) = (
        world.get::<Kinematics>(ue).copied(),
        world
            .get::<Kinematics>(entity(world, id).expect("weapon"))
            .copied(),
    ) else {
        return;
    };
    let reaction = world
        .get::<Group>(ge)
        .map_or(ThreatReaction::None, |g| g.behaviour.threat_reaction);
    if p.domain == Domain::Air && reaction == ThreatReaction::Beam {
        let b = initial_bearing(tk.pos(), wk.pos());
        let (l, r) = (b - FRAC_PI_2, b + FRAC_PI_2);
        let heading = if angle_diff(tk.heading, l).abs() <= angle_diff(tk.heading, r).abs() {
            l
        } else {
            r
        };
        if let Some(mut g) = world.get_mut::<Group>(ge) {
            g.evade = Some(crate::components::Evade {
                heading: wb_core::geo::normalize_bearing(heading),
                until_ms: now + EVADE_HOLD_MS,
            });
        }
    }
    let cmr = &rules.realism().countermeasures;
    if locked_on
        && f.cm_used.is_empty()
        && distance_m(tk.pos(), wk.pos()) <= cmr.release_range_m
        && let Some(mut cms) = world.get_mut::<Countermeasures>(ue)
    {
        for (cm, _, left) in &mut cms.0 {
            let c = &rules.db.countermeasures[cm.0 as usize];
            if *left == 0 || !c.defeats.contains(&w.guidance.terminal) {
                continue;
            }
            let n = cmr.rounds_per_threat.min(*left);
            *left -= n;
            f.cm_used.push((*cm, n));
        }
    }
}

/// `f_aspect` from the realism table: head-on at 0°, beam at 90°, tail at
/// 180°, linear between.
fn f_aspect(rules: &Rules, seeker: Seeker, aspect_rad: f64) -> f64 {
    let Some(t) = rules.realism().endgame.aspect.get(&seeker) else {
        return 1.0;
    };
    let x = (aspect_rad / FRAC_PI_2).clamp(0.0, 2.0);
    if x <= 1.0 {
        t.head_on + (t.beam - t.head_on) * x
    } else {
        t.beam + (t.tail - t.beam) * (x - 1.0)
    }
}

/// Lateral acceleration a weapon at speed `v` has in the endgame, in g:
/// full at `v_full` and above.
pub fn weapon_g(rules: &Rules, w: &Weapon, v: f64, v_full: f64) -> Option<f64> {
    w.profile.max_g.map(|g| {
        g * pow(
            (v / v_full.max(1.0)).min(1.0),
            rules.realism().weapons.g_speed_exponent,
        )
    })
}

/// The roll at the fuze. `target` is the unit the weapon fuzed on (none for
/// a point); `burst` where it detonated. Returns the outcome, the factors
/// and, for a CEP weapon, the burst point moved by its radial miss.
#[allow(clippy::too_many_arguments)]
pub fn resolve(
    world: &World,
    rules: &Rules,
    seed: u64,
    tick: u64,
    id: UnitId,
    f: &WeaponFlight,
    wk: &Kinematics,
    target: Option<UnitId>,
) -> (Outcome, Endgame, LatLon) {
    let w = &rules.db.weapons[f.weapon.0 as usize];
    let lethal = crate::weapons::lethal_radius_m(rules, w);
    let te = target.and_then(|t| entity(world, t));
    let tk = te.and_then(|e| world.get::<Kinematics>(e).copied());

    if let Some(cep) = w.cep_m {
        // Rayleigh: σ = CEP / √(2 ln 2).
        let sigma = cep / sqrt(2.0 * ln(2.0));
        let u = rng(seed, RngPurpose::Cep, u64::from(id.0), tick, 0);
        let r = sigma * sqrt(-2.0 * ln((1.0 - u).max(1e-12)));
        let dir = rng(seed, RngPurpose::Cep, u64::from(id.0), tick, 1) * 2.0 * PI;
        let pk = 1.0 - exp(-(lethal * lethal) / (2.0 * sigma * sigma));
        let burst = destination_point(wk.pos(), dir, r);
        let eg = Endgame {
            pk_base: pk,
            f_energy: 1.0,
            f_aspect: 1.0,
            f_maneuver: 1.0,
            f_cm: 1.0,
            f_ew: 1.0,
            f_env: 1.0,
            pk,
            roll: u,
            aspect_deg: 0.0,
            target_g: 0.0,
            cep_m: Some(cep),
            radial_miss_m: Some(r),
        };
        let hit = r <= lethal;
        return (if hit { Outcome::Hit } else { Outcome::Miss }, eg, burst);
    }

    let er = &rules.realism().endgame;
    let pk_base = te
        .and_then(|e| target_class(world, rules, e))
        .and_then(|c| w.pk_base.get(&c).copied())
        .unwrap_or(0.0);
    let v = wk.speed_mps;
    // A rocket at or above V_nom has its full endgame energy and g.
    let (f_energy, v_nom) = match w.profile.kind {
        ProfileKind::Rocket | ProfileKind::Ballistic => {
            let vmin = w.profile.min_speed_mach.unwrap_or(0.0) * rules.speed_of_sound(wk.alt_m);
            let span = (er.nominal_speed_fraction * (f.vmax_mps - vmin)).max(1.0);
            (sqrt(((v - vmin) / span).clamp(0.0, 1.0)), vmin + span)
        }
        _ => (1.0, f.vmax_mps),
    };
    // Aspect: 0° when the weapon meets the target nose-on.
    let (aspect, f_asp) = match tk {
        Some(t) if t.speed_mps >= 1.0 => {
            let (wh, th) = (wk.heading, t.heading);
            let cosang = cos(wh) * cos(th) + sin(wh) * sin(th);
            let a = PI - acos(cosang.clamp(-1.0, 1.0));
            (a, f_aspect(rules, w.guidance.terminal, a))
        }
        _ => (0.0, f_aspect(rules, w.guidance.terminal, 0.0)),
    };
    let air_target = te
        .and_then(|e| world.get::<PlatformRef>(e))
        .map(|p| rules.platform(p.0))
        .filter(|p| p.domain == Domain::Air);
    let (f_maneuver, target_g) = match air_target {
        Some(p) if f.target_aware => {
            let gt = p
                .mobility
                .sustained_g
                .unwrap_or(rules.realism().movement.aircraft_sustained_g);
            match weapon_g(rules, w, v, v_nom) {
                Some(gm) => (
                    (gm / (er.maneuver_g_ratio * gt)).clamp(er.maneuver_floor, 1.0),
                    gt,
                ),
                None => (1.0, gt),
            }
        }
        _ => (1.0, 0.0),
    };
    let mut f_cm = 1.0;
    for (cm, n) in &f.cm_used {
        if *n == 0 {
            continue;
        }
        let c = &rules.db.countermeasures[cm.0 as usize];
        let resist = w
            .countermeasure_resistance
            .get(&c.cm_type)
            .copied()
            .unwrap_or(0.0);
        f_cm *= 1.0 - c.effect * (1.0 - resist);
    }
    let f_ew = 1.0;
    let f_env = if matches!(f.seeker, SeekerState::Locked { memory: true, .. }) {
        rules.realism().ew.arm_memory_pk_factor
    } else {
        1.0
    };
    let raw = pk_base * f_energy * f_asp * f_maneuver * f_ew * f_env;
    let pk = (raw * f_cm).clamp(er.pk_min, er.pk_max);
    let pk_no_cm = raw.clamp(er.pk_min, er.pk_max);
    let roll = rng(seed, RngPurpose::Endgame, u64::from(id.0), tick, 0);
    let outcome = if roll < pk {
        Outcome::Hit
    } else if roll < pk_no_cm {
        Outcome::Decoyed
    } else {
        Outcome::Miss
    };
    let eg = Endgame {
        pk_base,
        f_energy,
        f_aspect: f_asp,
        f_maneuver,
        f_cm,
        f_ew,
        f_env,
        pk,
        roll,
        aspect_deg: rad_to_deg(aspect),
        target_g: target_g * if f.target_aware { 1.0 } else { 0.0 },
        cep_m: None,
        radial_miss_m: None,
    };
    (outcome, eg, wk.pos())
}
