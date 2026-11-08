//! Weapons in flight (doc 02 §10, P5): launch, energy, altitude profiles,
//! guidance, seekers and fuzing.
//!
//! A weapon is an entity with a `UnitId` from `UnitIdAlloc`, `Identity`,
//! `Allegiance` (the shooter's), `Kinematics` and [`WeaponFlight`]. The
//! `weapons` stage moves every weapon once per tick, in `UnitId` order, in
//! a local east-north-up frame at the weapon:
//!
//! - **Energy:** rockets boost to burnout speed, hold it while the sustainer
//!   burns, then coast `V·e^(−Δt/τ(alt))`; manoeuvring costs speed in
//!   proportion to lateral acceleration; below `V_min` the weapon is out of
//!   energy. Air-breathing weapons hold cruise speed until their range is
//!   flown. Glide weapons keep their release speed and descend at least at
//!   the glide slope `published range ÷ calibration altitude`.
//! - **Guidance:** pure proportional navigation (N from the realism gains)
//!   against the locked target, else against the dead-reckoned aimpoint,
//!   limited by `max_g × (V / V_max)^k`. Rockets steer in 3D; sea-skimming
//!   and cruise profiles hold their altitude above the aimpoint and steer in
//!   3D only for the last seconds; glide weapons follow the glide slope.
//! - **Midcourse:** every `datalink_update_s` a weapon with a data link (or
//!   command or semi-active guidance) takes the side's contact estimate,
//!   if its shooter still tracks the contact or the weapon may engage on
//!   remote tracks. Semi-active and command weapons whose shooter lost the
//!   track end with `LostTrack`.
//! - **Seeker:** activates within `seeker_range` of the aimpoint and locks
//!   the nearest valid target to the aimpoint inside its range and field of
//!   view, preferring the contact it was fired at. Anti-radiation seekers
//!   need an emitting radar and fly to its last position when it goes
//!   silent. With no target, the weapon self-destructs
//!   `self_destruct_after_lost_s` after reaching the aimpoint or losing lock.
//! - **Fuze:** at the closest point of approach within the tick, inside
//!   `max(fuze_radius, lethal_radius)`. Until the endgame (P5 step 4) the
//!   outcome is geometric: a hit when the miss distance is inside the
//!   lethal radius.
//!
//! Terrain is not modelled in flight yet: altitudes are above sea level.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use wb_content::compiled::{Weapon, WeaponIx};
use wb_content::source::weapon::ProfileKind;
use wb_content::source::{Domain, Seeker, SensorType};
use wb_core::geo::{Enu, LatLon, destination_point, distance_m, from_enu, initial_bearing, to_enu};
use wb_core::math::{atan, atan2, cos, exp, pow, sin, sqrt};
use wb_core::units::deg_to_rad;
use wb_core::{ContactId, UnitId};

use crate::clock::SimClock;
use crate::components::{
    AimPoint, Allegiance, EmconState, FlightPhase, GroupMember, Identity, Kinematics, Magazine,
    PlatformRef, Posture, SeekerState, WeaponFlight, WeaponTarget,
};
use crate::events::{AimRef, Endgame, EventKind, Factors, Outcome, emit};
use crate::picture::{Quality, SidePictures};
use crate::rules::{G0, Rules};
use crate::world::{UnitIdAlloc, UnitIndex, entity, spawn_unit};

/// Hold and cruise profiles steer in 3D for this long before impact.
const TERMINAL_DIVE_S: f64 = 4.0;
/// Time constant of the altitude-hold and glide pitch commands, s.
const PITCH_TAU_S: f64 = 1.0;
/// Flights longer than this end out of energy whatever the profile.
const MAX_FLIGHT_S: f64 = 7_200.0;

/// Combat bookkeeping, saved and hashed. `in_flight` and
/// `hostile_in_envelope` drive the adaptive tick (doc 02 §3), so a loaded
/// war picks the same tick length as the saved one.
#[derive(Resource, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Combat {
    pub in_flight: u32,
    pub hostile_in_envelope: bool,
    pub next_salvo: u32,
    /// Next `M-n` / `INT-n` number per side.
    pub next_tag: Vec<u32>,
    /// When a side's last weapon at a contact ended (shoot-look-shoot).
    pub last_end: BTreeMap<(u8, ContactId), u64>,
    /// When a contact last launched at a side's forces (Bravo return fire).
    pub hostile_acts: BTreeMap<(u8, ContactId), u64>,
    /// Whether a side last saw a hostile contact inside its territory.
    pub inside: BTreeMap<(u8, ContactId), bool>,
    /// Weapon contacts judged aimed at an own unit, and that unit.
    pub incoming: BTreeMap<(u8, ContactId), UnitId>,
    /// Tracked salvos for salvo reporting.
    pub salvos: BTreeMap<u32, SalvoStats>,
    /// Recent visual effects for SideView (doc 10 §5).
    pub effects: Vec<VisualEffect>,
}

/// Statistics of a salvo in flight.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SalvoStats {
    pub side: u8,
    pub shooter: UnitId,
    pub total: u32,
    pub terminated: u32,
    pub hits: u32,
    pub misses: u32,
    pub intercepted: u32,
    pub decoyed: u32,
    pub jammed: u32,
}

/// A visual combat effect (launch flash, intercept burst, hit burst).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct VisualEffect {
    pub time_ms: u64,
    pub at: LatLon,
    pub alt_m: f32,
    pub kind: u8,
    /// Bit `s` set: side `s` may see the effect (CLAUDE.md rule 1). A launch
    /// flash is the shooter's side only; an intercept or a hit also reaches
    /// the side of the weapon or unit on the other end, which learns of it
    /// anyway.
    pub sides: u8,
}

/// Bit of a side in [`VisualEffect::sides`] (and in `Event::sides`).
fn side_bit(side: u8) -> u8 {
    1u8.checked_shl(u32::from(side)).unwrap_or(0)
}

/// Drops effects older than the view keeps them (`view::effect::RETENTION_MS`).
fn trim_effects(c: &mut Combat, now_ms: u64) {
    c.effects
        .retain(|e| now_ms.saturating_sub(e.time_ms) <= crate::view::effect::RETENTION_MS);
}

/// Why a weapon was fired: the launch event and the golden checks read it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum LaunchReason {
    /// Engine tests and calibration.
    #[default]
    Test,
    /// Self-defence against an incoming weapon (ADR-031).
    SelfDefence,
    /// The target of an Intercept or Strike order (ADR-029).
    Ordered,
    /// Allowed by ROE Alpha.
    WeaponsFree,
    /// ROE Bravo: the target is inside own territory.
    Territory,
    /// ROE Bravo: the target launched at our forces.
    ReturnFire,
    /// An approved request (doc 03 §7).
    Approved,
}

impl Combat {
    pub fn new_salvo(&mut self) -> u32 {
        self.next_salvo += 1;
        self.next_salvo
    }

    fn new_tag(&mut self, side: u8) -> u32 {
        let i = usize::from(side);
        if self.next_tag.len() <= i {
            self.next_tag.resize(i + 1, 0);
        }
        self.next_tag[i] += 1;
        self.next_tag[i]
    }
}

/// What to fire at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Aim {
    /// A contact of the shooter's side.
    Contact(ContactId),
    /// Coordinates; altitude above sea level.
    Point { at: LatLon, alt_m: f64 },
    /// A truth unit, with perfect midcourse updates: tests and calibration
    /// only (never from an order; the launch event names no target).
    Truth(UnitId),
}

/// One weapon to launch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Launch {
    pub shooter: UnitId,
    pub weapon: WeaponIx,
    pub aim: Aim,
    /// Salvo to join; a new one if `None`.
    pub salvo: Option<u32>,
    pub reason: LaunchReason,
}

/// Launches a weapon: takes a round from the shooter's magazine, spawns the
/// weapon at the shooter and emits `WeaponLaunched`. The reason if it
/// cannot.
pub fn launch(world: &mut World, l: &Launch) -> Result<UnitId, String> {
    let rules = world.resource::<Rules>().clone();
    let now = world.resource::<SimClock>().sim_time_ms;
    let db = &rules.db;
    let w = db
        .weapons
        .get(l.weapon.0 as usize)
        .ok_or_else(|| format!("Unknown weapon {}", l.weapon.0))?;
    let wname = db.str(w.name);
    let se = entity(world, l.shooter).ok_or_else(|| format!("Unknown unit {}", l.shooter.0))?;
    let sname = world
        .get::<Identity>(se)
        .map_or_else(String::new, |i| i.name.clone());
    let alg = *world
        .get::<Allegiance>(se)
        .ok_or_else(|| format!("{sname} cannot fire"))?;
    let sk = *world
        .get::<Kinematics>(se)
        .ok_or_else(|| format!("{sname} cannot fire"))?;
    let p = world
        .get::<PlatformRef>(se)
        .map(|p| rules.platform(p.0))
        .ok_or_else(|| format!("{sname} cannot fire"))?;
    if matches!(world.get::<Posture>(se), Some(Posture::AtBase { .. })) {
        return Err(format!("{sname} is at a base"));
    }
    let station = world
        .get::<Magazine>(se)
        .and_then(|m| {
            m.stations
                .iter()
                .position(|s| s.weapon == l.weapon && s.count > 0)
        })
        .ok_or_else(|| format!("{sname} has no {wname} left"))?;

    let (target, aim, aim_ref) = match l.aim {
        Aim::Contact(id) => {
            let pics = world.resource::<SidePictures>();
            let c = pics
                .sides
                .get(usize::from(alg.side))
                .and_then(|pic| pic.get(id))
                .ok_or_else(|| format!("Unknown contact C{}", id.0))?;
            let est = c
                .est
                .ok_or_else(|| format!("C{} has no position estimate", id.0))?;
            if !w.targets.contains(&c.domain) {
                let domain = match c.domain {
                    Domain::Air => "air",
                    Domain::Surface => "surface",
                    Domain::Subsurface => "subsurface",
                    Domain::Ground => "ground",
                    Domain::Space => "space",
                };
                return Err(format!("{wname} cannot engage {domain} targets"));
            }
            (
                WeaponTarget::Track {
                    contact: id,
                    key: c.truth,
                },
                AimPoint {
                    at: est.at,
                    alt_m: est.alt_m,
                    heading: est.heading,
                    speed_mps: est.speed_mps,
                    climb_mps: 0.0,
                    at_ms: c.last_ms,
                },
                Some(AimRef::Contact { contact: id }),
            )
        }
        Aim::Point { at, alt_m } => (
            WeaponTarget::Point { at, alt_m },
            AimPoint {
                at,
                alt_m,
                heading: 0.0,
                speed_mps: 0.0,
                climb_mps: 0.0,
                at_ms: now,
            },
            Some(AimRef::Point {
                lat_deg: at.lat_deg(),
                lon_deg: at.lon_deg(),
            }),
        ),
        Aim::Truth(u) => {
            let k = entity(world, u)
                .and_then(|e| world.get::<Kinematics>(e))
                .ok_or_else(|| format!("Unknown unit {}", u.0))?;
            (WeaponTarget::Truth(u), truth_aim(k, now), None)
        }
    };

    let air = rules.is_air(p);
    let alt = if air {
        sk.alt_m
    } else {
        sk.alt_m.max(0.0) + rules.realism().earth.surface_target_height_m
    };
    let v0 = if air { sk.speed_mps } else { 0.0 };
    let vmax = w.profile.max_speed_mach * rules.speed_of_sound(alt);
    let aim_now = dead_reckon(&aim, now);
    let rel = to_enu(sk.pos(), alt, aim_now.0, aim_now.1);
    let horiz = sqrt(rel.e * rel.e + rel.n * rel.n);
    let max_fp = deg_to_rad(rules.realism().weapons.max_flight_path_deg);
    let fpa = match mode(w) {
        Mode::Intercept => atan2(rel.u, horiz).clamp(-max_fp, max_fp),
        Mode::Hold | Mode::Glide => 0.0,
    };

    // The side fired at, and whether the target is itself a weapon.
    let victim = match target {
        WeaponTarget::Track { key, .. } | WeaponTarget::Truth(key) => entity(world, key),
        WeaponTarget::Point { .. } => None,
    };
    let victim_side = victim.and_then(|e| world.get::<Allegiance>(e).map(|a| a.side));
    let intercept = victim.is_some_and(|e| world.get::<WeaponFlight>(e).is_some())
        || matches!(l.aim, Aim::Contact(id) if world.resource::<SidePictures>().sides
            .get(usize::from(alg.side))
            .and_then(|p| p.get(id))
            .is_some_and(|c| matches!(c.type_ref, crate::picture::TypeRef::Weapon(_))
                && c.level >= wb_content::source::realism::ContactLevel::Class));

    let id = world.resource_mut::<UnitIdAlloc>().take();
    let (salvo, tag) = {
        let mut c = world.resource_mut::<Combat>();
        let salvo = l.salvo.unwrap_or_else(|| c.new_salvo());
        c.in_flight += 1;
        let s = c.salvos.entry(salvo).or_insert_with(|| SalvoStats {
            side: alg.side,
            shooter: l.shooter,
            total: 0,
            terminated: 0,
            hits: 0,
            misses: 0,
            intercepted: 0,
            decoyed: 0,
            jammed: 0,
        });
        s.total += 1;
        c.effects.push(VisualEffect {
            time_ms: now,
            at: sk.pos(),
            alt_m: alt as f32,
            kind: crate::view::effect::LAUNCH,
            sides: side_bit(alg.side),
        });
        (salvo, c.new_tag(alg.side))
    };
    // A launch at a side's forces whose picture holds the shooter is a
    // hostile act against that side (ROE Bravo return fire).
    if let Some(vs) = victim_side.filter(|s| *s != alg.side) {
        let key = world.get::<GroupMember>(se).map_or(l.shooter, |m| m.group);
        let seen = world
            .resource::<SidePictures>()
            .sides
            .get(usize::from(vs))
            .and_then(|p| p.by_truth.get(&key).copied());
        if let Some(c) = seen {
            world
                .resource_mut::<Combat>()
                .hostile_acts
                .insert((vs, c), now);
            emit(
                world,
                vs,
                EventKind::AutoSlow {
                    reason: format!("C{} launched at our forces", c.0),
                    cause: crate::events::SlowCause::LaunchAgainstUs,
                },
            );
        }
    }
    world.get_mut::<Magazine>(se).expect("magazine").stations[station].count -= 1;
    spawn_unit(
        world,
        id,
        (
            Identity {
                unit: id,
                name: format!("{wname} #{tag}"),
            },
            alg,
            Kinematics {
                lat: sk.lat,
                lon: sk.lon,
                alt_m: alt,
                heading: initial_bearing(sk.pos(), aim_now.0),
                speed_mps: v0,
                climb_mps: 0.0,
            },
            WeaponFlight {
                weapon: l.weapon,
                shooter: l.shooter,
                salvo,
                tag,
                target,
                aim,
                phase: match w.profile.kind {
                    ProfileKind::Glide => FlightPhase::Glide,
                    ProfileKind::AirBreathing if w.profile.boost_s <= 0.0 => FlightPhase::Cruise,
                    _ => FlightPhase::Boost,
                },
                seeker: SeekerState::Off,
                fpa,
                launch_ms: now,
                launch_range_m: sqrt(horiz * horiz + rel.u * rel.u),
                v0_mps: v0,
                vmax_mps: vmax,
                flown_m: 0.0,
                manoeuvre_loss_mps: 0.0,
                intercept,
                cm_used: Vec::new(),
                target_aware: false,
            },
        ),
    );
    emit(
        world,
        alg.side,
        EventKind::WeaponLaunched {
            weapon: id,
            shooter: l.shooter,
            weapon_type: db.str(w.id).to_owned(),
            weapon_name: db.str(w.name).to_owned(),
            category: w.category,
            salvo,
            tag,
            target: aim_ref,
            reason: l.reason,
            intercept,
        },
    );
    Ok(id)
}

/// How a weapon flies vertically.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// 3D proportional navigation (air-to-air, SAMs, ARMs, rocket AGMs).
    Intercept,
    /// Holds a cruise or sea-skimming altitude above the aimpoint.
    Hold,
    /// Unpowered: at least the glide slope.
    Glide,
}

fn mode(w: &Weapon) -> Mode {
    match w.profile.kind {
        ProfileKind::Glide => Mode::Glide,
        ProfileKind::AirBreathing => Mode::Hold,
        _ if w.profile.cruise_alt_m.is_some() => Mode::Hold,
        _ => Mode::Intercept,
    }
}

/// Seekers that lock a target (the others fly to coordinates).
fn locks(s: Seeker) -> bool {
    !matches!(s, Seeker::SatnavIns | Seeker::None)
}

/// Seekers that need the shooter's track until impact.
fn needs_track(s: Seeker) -> bool {
    matches!(s, Seeker::SemiActiveRadar | Seeker::Command | Seeker::Laser)
}

fn updates(w: &Weapon) -> bool {
    use wb_content::source::weapon::Midcourse;
    needs_track(w.guidance.terminal)
        || w.guidance
            .midcourse
            .iter()
            .any(|m| matches!(m, Midcourse::Datalink | Midcourse::Command))
}

/// Lethal radius: the weapon's, else the realism default for its warhead.
pub fn lethal_radius_m(rules: &Rules, w: &Weapon) -> f64 {
    let wr = &rules.realism().weapons;
    w.lethal_radius_m
        .or_else(|| w.warhead.and_then(|h| wr.lethal_radius_m.get(&h).copied()))
        .unwrap_or(wr.fuze_radius_m)
}

/// Coast time constant at an altitude: linear from sea level to
/// `tau_high_alt_m`.
fn tau_s(rules: &Rules, w: &Weapon, alt_m: f64) -> f64 {
    let Some(t) = &w.profile.coast_tau_s else {
        return f64::INFINITY;
    };
    let f = (alt_m / rules.realism().weapons.tau_high_alt_m).clamp(0.0, 1.0);
    t.sea_level + (t.high_alt - t.sea_level) * f
}

/// Glide ratio: published range over the calibration release altitude.
fn glide_ratio(rules: &Rules, w: &Weapon) -> f64 {
    let alt = w
        .profile
        .calibration
        .as_ref()
        .or_else(|| rules.realism().weapons.calibration.get(&w.category))
        .map_or(0.0, |c| c.launch_alt_m);
    if alt > 0.0 {
        w.profile.max_range_m / alt
    } else {
        f64::INFINITY
    }
}

fn truth_aim(k: &Kinematics, now: u64) -> AimPoint {
    AimPoint {
        at: k.pos(),
        alt_m: k.alt_m,
        heading: k.heading,
        speed_mps: k.speed_mps,
        climb_mps: k.climb_mps,
        at_ms: now,
    }
}

/// The aimpoint carried forward to `now`: position, altitude.
fn dead_reckon(a: &AimPoint, now: u64) -> (LatLon, f64) {
    let dt = now.saturating_sub(a.at_ms) as f64 / 1_000.0;
    let at = if a.speed_mps > 0.0 {
        destination_point(a.at, a.heading, a.speed_mps * dt)
    } else {
        a.at
    };
    (at, a.alt_m + a.climb_mps * dt)
}

fn vel(heading: f64, speed: f64, climb: f64) -> Enu {
    Enu {
        e: speed * sin(heading),
        n: speed * cos(heading),
        u: climb,
    }
}

fn dot(a: Enu, b: Enu) -> f64 {
    a.e * b.e + a.n * b.n + a.u * b.u
}

fn cross(a: Enu, b: Enu) -> Enu {
    Enu {
        e: a.n * b.u - a.u * b.n,
        n: a.u * b.e - a.e * b.u,
        u: a.e * b.n - a.n * b.e,
    }
}

fn scale(a: Enu, k: f64) -> Enu {
    Enu {
        e: a.e * k,
        n: a.n * k,
        u: a.u * k,
    }
}

fn sub(a: Enu, b: Enu) -> Enu {
    Enu {
        e: a.e - b.e,
        n: a.n - b.n,
        u: a.u - b.u,
    }
}

fn norm(a: Enu) -> f64 {
    sqrt(dot(a, a))
}

/// A body a seeker may lock, this tick.
struct Candidate {
    uid: UnitId,
    key: UnitId,
    side: u8,
    domain: Domain,
    pos: LatLon,
    alt_m: f64,
    radar_on: bool,
}

fn candidates(world: &World, rules: &Rules, now: u64) -> Vec<Candidate> {
    let mut out = Vec::new();
    for (&uid, &e) in &world.resource::<UnitIndex>().0 {
        let (Some(a), Some(k)) = (world.get::<Allegiance>(e), world.get::<Kinematics>(e)) else {
            continue;
        };
        if matches!(world.get::<Posture>(e), Some(Posture::AtBase { .. })) {
            continue;
        }
        let (domain, radar_on) = if let Some(p) = world.get::<PlatformRef>(e) {
            let p = rules.platform(p.0);
            if p.orbit.is_some() {
                continue;
            }
            let radar = p
                .sensors
                .iter()
                .any(|m| rules.db.sensors[m.sensor.0 as usize].sensor_type == SensorType::Radar);
            let emcon = world
                .get::<EmconState>(e)
                .map_or(wb_content::source::Emcon::On, |x| x.0);
            (
                p.domain,
                radar && crate::sensors::emitting(rules, emcon, uid, now),
            )
        } else if world.get::<WeaponFlight>(e).is_some()
            || world.get::<crate::components::ScriptedTarget>(e).is_some()
        {
            (Domain::Air, false)
        } else {
            continue;
        };
        out.push(Candidate {
            uid,
            key: world.get::<GroupMember>(e).map_or(uid, |m| m.group),
            side: a.side,
            domain,
            pos: k.pos(),
            alt_m: k.alt_m,
            radar_on,
        });
    }
    out
}

/// The weapons stage: every tick, every weapon in `UnitId` order.
pub fn fly(world: &mut World) {
    // Effects expire by age even when nothing is in the air.
    let end_ms = world.resource::<SimClock>().end_ms();
    if world
        .resource::<Combat>()
        .effects
        .first()
        .is_some_and(|e| end_ms.saturating_sub(e.time_ms) > crate::view::effect::RETENTION_MS)
    {
        trim_effects(&mut world.resource_mut::<Combat>(), end_ms);
    }
    if world.resource::<Combat>().in_flight == 0 {
        return;
    }
    let rules = world.resource::<Rules>().clone();
    let clock = world.resource::<SimClock>().clone();
    let wr = &rules.realism().weapons;
    let update_ms = (wr.datalink_update_s * 1_000.0).max(1_000.0) as u64;
    let update_due = clock.due(update_ms);
    let weapons: Vec<(UnitId, Entity)> = world
        .resource::<UnitIndex>()
        .0
        .iter()
        .filter(|(_, e)| world.get::<WeaponFlight>(**e).is_some())
        .map(|(u, e)| (*u, *e))
        .collect();
    let seed = world.resource::<crate::clock::RngSeed>().0;
    let mut cands: Option<Vec<Candidate>> = None;
    for (id, e) in weapons {
        // Intercepted earlier this tick.
        let Some(mut f) = world.get::<WeaponFlight>(e).cloned() else {
            continue;
        };
        crate::endgame::react(world, &rules, clock.sim_time_ms, id, &mut f);
        let k = *world.get::<Kinematics>(e).expect("kinematics");
        let side = world.get::<Allegiance>(e).map_or(0, |a| a.side);
        let w = &rules.db.weapons[f.weapon.0 as usize];
        let ended = step(
            world, &rules, &clock, update_due, update_ms, &mut cands, side, &mut f, k,
        );
        match ended {
            Ok(k2) => {
                *world.get_mut::<Kinematics>(e).expect("kinematics") = k2;
                *world.get_mut::<WeaponFlight>(e).expect("weapon") = f;
            }
            Err((End::Plain(outcome), k2)) => {
                terminate(
                    world, &rules, &clock, id, side, &f, &k2, outcome, None, None, None,
                );
            }
            Err((End::Fuzed { target, miss }, k2)) => {
                let (outcome, eg, burst) =
                    crate::endgame::resolve(world, &rules, seed, clock.tick, id, &f, &k2, target);
                terminate(
                    world,
                    &rules,
                    &clock,
                    id,
                    side,
                    &f,
                    &k2,
                    outcome,
                    Some(miss),
                    Some(eg),
                    target,
                );
                match (outcome, target, f.target) {
                    (Outcome::Hit, Some(t), _) => crate::damage::hit(world, &rules, seed, id, w, t),
                    (_, _, WeaponTarget::Point { .. }) => {
                        crate::damage::burst(world, &rules, seed, id, w, burst);
                    }
                    _ => {}
                }
            }
        }
    }
}

/// How a flight ended.
enum End {
    Plain(Outcome),
    /// The fuze fired at the closest approach to `target` (none: a point).
    Fuzed {
        target: Option<UnitId>,
        miss: f64,
    },
}

/// End of a flight and the kinematics at the end.
type Ended = (End, Kinematics);

/// Ends a weapon's flight from outside the weapons stage (an interceptor hit
/// it).
pub fn end_flight(world: &mut World, rules: &Rules, id: UnitId, outcome: Outcome) {
    let Some(e) = entity(world, id) else { return };
    let (Some(f), Some(k)) = (
        world.get::<WeaponFlight>(e).cloned(),
        world.get::<Kinematics>(e).copied(),
    ) else {
        return;
    };
    let side = world.get::<Allegiance>(e).map_or(0, |a| a.side);
    let clock = world.resource::<SimClock>().clone();
    terminate(
        world, rules, &clock, id, side, &f, &k, outcome, None, None, None,
    );
}

#[allow(clippy::too_many_arguments)]
fn step(
    world: &World,
    rules: &Rules,
    clock: &SimClock,
    update_due: bool,
    update_ms: u64,
    cands: &mut Option<Vec<Candidate>>,
    side: u8,
    f: &mut WeaponFlight,
    k: Kinematics,
) -> Result<Kinematics, Ended> {
    let w = &rules.db.weapons[f.weapon.0 as usize];
    let wr = &rules.realism().weapons;
    let now = clock.sim_time_ms;
    let dt = clock.dt_s();
    let tof = now.saturating_sub(f.launch_ms) as f64 / 1_000.0;
    let seeker = w.guidance.terminal;

    // Midcourse updates and the shooter's track.
    if update_due && updates(w) {
        match f.target {
            WeaponTarget::Track { contact, .. } => {
                let tracked = shooter_track(world, side, f.shooter, contact, now, 2 * update_ms);
                if needs_track(seeker) && tracked.is_none() {
                    return Err((End::Plain(Outcome::LostTrack), k));
                }
                if let Some(aim) = tracked.or_else(|| {
                    w.engage_on_remote
                        .then(|| side_estimate(world, side, contact))
                        .flatten()
                }) && !matches!(f.seeker, SeekerState::Locked { .. })
                {
                    f.aim = aim;
                }
            }
            WeaponTarget::Truth(u) => {
                match entity(world, u).and_then(|e| world.get::<Kinematics>(e)) {
                    Some(tk) => f.aim = truth_aim(tk, now),
                    None if needs_track(seeker) => {
                        return Err((End::Plain(Outcome::LostTrack), k));
                    }
                    None => {}
                }
            }
            WeaponTarget::Point { .. } => {}
        }
    }

    // Seeker.
    let (aim_pos, aim_alt) = dead_reckon(&f.aim, now);
    let to_aim = to_enu(k.pos(), k.alt_m, aim_pos, aim_alt);
    let aim_dist = norm(to_aim);
    let u = vel(k.heading, 1.0, 0.0);
    let u = Enu {
        e: u.e * cos(f.fpa),
        n: u.n * cos(f.fpa),
        u: sin(f.fpa),
    };
    let seeker_range = w.guidance.seeker_range_m.unwrap_or(f64::INFINITY);
    let half_fov = deg_to_rad(wr.seeker_half_angle_deg);
    let in_fov = |rel: Enu| {
        let d = norm(rel);
        d > 0.0 && dot(rel, u) / d >= cos(half_fov)
    };
    let reached_aim = aim_dist <= 2.0 * k.speed_mps.max(1.0) * dt || dot(to_aim, u) < 0.0;
    if locks(seeker) {
        if f.seeker == SeekerState::Off && aim_dist <= seeker_range {
            f.seeker = SeekerState::Searching { lost_ms: None };
        }
        if let SeekerState::Locked { unit, memory } = f.seeker {
            let alive =
                entity(world, unit).and_then(|e| world.get::<Kinematics>(e).map(|tk| (e, *tk)));
            // Within two ticks of closing the target may already be abeam
            // (it moved this tick, the weapon not yet): the lock holds so the
            // fuze can judge the closest approach.
            let holds = |tk: &Kinematics| {
                let rel = to_enu(k.pos(), k.alt_m, tk.pos(), tk.alt_m);
                in_fov(rel) || norm(rel) <= 2.0 * (k.speed_mps + tk.speed_mps) * dt
            };
            match alive {
                Some((_, tk)) if holds(&tk) => {
                    if seeker == Seeker::AntiRadiation {
                        let on = cands
                            .get_or_insert_with(|| candidates(world, rules, now))
                            .iter()
                            .any(|c| c.uid == unit && c.radar_on);
                        if !on && !memory {
                            f.aim = AimPoint {
                                climb_mps: 0.0,
                                speed_mps: 0.0,
                                ..truth_aim(&tk, now)
                            };
                        }
                        f.seeker = SeekerState::Locked { unit, memory: !on };
                    }
                }
                _ => f.seeker = SeekerState::Searching { lost_ms: Some(now) },
            }
        }
        if let SeekerState::Searching { lost_ms } = f.seeker {
            let list = cands.get_or_insert_with(|| candidates(world, rules, now));
            match acquire(list, w, f, side, k, aim_pos, seeker_range, &in_fov) {
                Some(unit) => {
                    f.seeker = SeekerState::Locked {
                        unit,
                        memory: false,
                    }
                }
                None => {
                    let lost = lost_ms.or(reached_aim.then_some(now));
                    if lost.is_some_and(|t| {
                        now.saturating_sub(t) as f64 / 1_000.0 >= wr.self_destruct_after_lost_s
                    }) {
                        return Err((End::Plain(Outcome::SelfDestruct), k));
                    }
                    f.seeker = SeekerState::Searching { lost_ms: lost };
                }
            }
        }
    }

    // Guidance target: the locked truth, else the aimpoint.
    let locked = match f.seeker {
        SeekerState::Locked {
            unit,
            memory: false,
        } => entity(world, unit).and_then(|e| world.get::<Kinematics>(e).copied()),
        _ => None,
    };
    let (t_pos, t_alt, t_vel) = match locked {
        Some(tk) => (
            tk.pos(),
            tk.alt_m,
            vel(tk.heading, tk.speed_mps, tk.climb_mps),
        ),
        None if matches!(f.seeker, SeekerState::Locked { memory: true, .. }) => (
            f.aim.at,
            f.aim.alt_m,
            Enu {
                e: 0.0,
                n: 0.0,
                u: 0.0,
            },
        ),
        None => (
            aim_pos,
            aim_alt,
            vel(f.aim.heading, f.aim.speed_mps, f.aim.climb_mps),
        ),
    };
    // Movement already ran this tick: the target's position at the start
    // of the tick is one step back.
    let t_now = to_enu(k.pos(), k.alt_m, t_pos, t_alt);
    let t0 = if locked.is_some() {
        sub(t_now, scale(t_vel, dt))
    } else {
        t_now
    };

    // Guidance: pure PN on the target at the start of the tick.
    let mut v = k.speed_mps.max(1.0);
    let n_gain = (wr.pn_gain_min + wr.pn_gain_max) / 2.0;
    let vw = scale(u, v);
    let rel_v = sub(t_vel, vw);
    let r2 = dot(t0, t0).max(1.0);
    let omega = scale(cross(t0, rel_v), 1.0 / r2);
    let mut a_cmd = scale(cross(omega, u), n_gain * v);
    let e_psi = Enu {
        e: cos(k.heading),
        n: -sin(k.heading),
        u: 0.0,
    };
    let e_gam = Enu {
        e: -sin(k.heading) * sin(f.fpa),
        n: -cos(k.heading) * sin(f.fpa),
        u: cos(f.fpa),
    };
    // A target outside the seeker's field of view (a new aimpoint, a
    // retarget): PN alone barely turns, so add pursuit toward the line of
    // sight.
    let los = scale(t0, 1.0 / sqrt(r2));
    if dot(los, u) < cos(half_fov) {
        a_cmd = Enu {
            e: a_cmd.e + (los.e - u.e) * v / PITCH_TAU_S,
            n: a_cmd.n + (los.n - u.n) * v / PITCH_TAU_S,
            u: a_cmd.u + (los.u - u.u) * v / PITCH_TAU_S,
        };
    }
    let cg = cos(f.fpa).max(1e-3);
    let mut psi_dot = dot(a_cmd, e_psi) / (v * cg);
    let mut gam_dot = dot(a_cmd, e_gam) / v;
    let horiz = sqrt(t0.e * t0.e + t0.n * t0.n);
    let max_fp = deg_to_rad(wr.max_flight_path_deg);
    let closing = (-dot(t0, rel_v) / sqrt(r2)).max(1.0);
    let terminal = horiz / closing <= TERMINAL_DIVE_S;
    // Rockets loft while the seeker is off: toward `tau_high_alt_m`, where
    // they coast furthest, but never above what they can descend from at
    // the flight-path limit over the distance left.
    let loft = mode(w) == Mode::Intercept && f.seeker == SeekerState::Off && !terminal;
    let held = (mode(w) != Mode::Intercept || loft) && !terminal;
    match mode(w) {
        _ if !held => {}
        Mode::Intercept => {
            let ceiling = t_alt + horiz * wb_core::math::tan(max_fp);
            let want = wr.tau_high_alt_m.max(t_alt).min(ceiling);
            let cmd = atan((want - k.alt_m) / (v * 4.0 * PITCH_TAU_S)).clamp(-max_fp, max_fp);
            gam_dot = (cmd - f.fpa) / PITCH_TAU_S;
        }
        Mode::Hold => {
            let in_seeker = aim_dist <= seeker_range;
            let cruise = w.profile.cruise_alt_m.unwrap_or(wr.cruise_alt_default_m);
            let above = if in_seeker {
                w.profile.terminal_alt_m.unwrap_or(cruise)
            } else {
                cruise
            };
            let want = aim_alt + above;
            let cmd = atan((want - k.alt_m) / (v * 4.0 * PITCH_TAU_S)).clamp(-max_fp, max_fp);
            gam_dot = (cmd - f.fpa) / PITCH_TAU_S;
        }
        Mode::Glide => {
            let slope = -atan(1.0 / glide_ratio(rules, w));
            let cmd = atan2(t0.u, horiz).min(slope).max(-max_fp);
            gam_dot = (cmd - f.fpa) / PITCH_TAU_S;
        }
    }
    let mut a_lat = v * sqrt(psi_dot * psi_dot * cg * cg + gam_dot * gam_dot);
    let a_max = w.profile.max_g.map_or(f64::INFINITY, |g| {
        g * G0 * pow((v / f.vmax_mps.max(1.0)).min(1.0), wr.g_speed_exponent)
    });
    if a_lat > a_max {
        let s = a_max / a_lat;
        psi_dot *= s;
        gam_dot *= s;
        a_lat = a_max;
    }

    // Energy.
    let p = &w.profile;
    let sound = rules.speed_of_sound(k.alt_m);
    match p.kind {
        ProfileKind::Rocket | ProfileKind::Ballistic => {
            if tof < p.boost_s {
                f.phase = FlightPhase::Boost;
                v += (f.vmax_mps - f.v0_mps) / p.boost_s * dt;
                v = v.min(f.vmax_mps);
            } else if tof < p.boost_s + p.sustain_s {
                f.phase = FlightPhase::Sustain;
                v = f.vmax_mps;
            } else {
                f.phase = FlightPhase::Coast;
                v *= exp(-dt / tau_s(rules, w, k.alt_m));
            }
            let loss = wr.manoeuvre_energy_factor * a_lat * dt;
            v -= loss;
            f.manoeuvre_loss_mps += loss;
        }
        ProfileKind::AirBreathing => {
            let cruise = p.max_speed_mach * sound;
            if tof < p.boost_s {
                f.phase = FlightPhase::Boost;
                v = (v + (cruise - f.v0_mps) / p.boost_s * dt).min(cruise);
            } else {
                f.phase = FlightPhase::Cruise;
                v = cruise;
            }
        }
        ProfileKind::Glide => {
            f.phase = FlightPhase::Glide;
            let loss = wr.manoeuvre_energy_factor * a_lat * dt;
            v = f.v0_mps.max(v) - loss;
            f.manoeuvre_loss_mps += loss;
        }
    }
    v = v.max(0.0);

    // Move.
    let heading = wb_core::geo::normalize_bearing(k.heading + psi_dot * dt);
    let mut fpa = f.fpa + gam_dot * dt;
    if held {
        fpa = fpa.clamp(-max_fp, max_fp);
    }
    let u2 = Enu {
        e: sin(heading) * cos(fpa),
        n: cos(heading) * cos(fpa),
        u: sin(fpa),
    };
    let step = scale(u2, v * dt);
    let (pos, alt) = from_enu(k.pos(), k.alt_m, step);
    f.fpa = fpa;
    f.flown_m += sqrt(step.e * step.e + step.n * step.n);
    let k2 = Kinematics {
        lat: pos.lat,
        lon: pos.lon,
        alt_m: alt,
        heading,
        speed_mps: v,
        climb_mps: v * sin(fpa),
    };

    // Fuze: closest approach within the tick.
    let fuze_target =
        locked.is_some() || matches!(f.seeker, SeekerState::Locked { .. }) || !locks(seeker);
    if fuze_target {
        let rel_v = sub(t_vel, scale(u2, v));
        let vv = dot(rel_v, rel_v);
        let t_star = if vv > 0.0 {
            (-dot(t0, rel_v) / vv).clamp(0.0, dt)
        } else {
            0.0
        };
        let miss = norm(Enu {
            e: t0.e + rel_v.e * t_star,
            n: t0.n + rel_v.n * t_star,
            u: t0.u + rel_v.u * t_star,
        });
        let gate = wr.fuze_radius_m.max(lethal_radius_m(rules, w));
        if t_star < dt && miss <= gate {
            let (target, miss) = match (f.seeker, f.target) {
                (SeekerState::Locked { unit, .. }, _) => (Some(unit), miss),
                (_, WeaponTarget::Track { key, .. } | WeaponTarget::Truth(key)) => {
                    match key_miss(world, key, pos, alt) {
                        Some((u, d)) => (Some(u), d),
                        None => (None, miss),
                    }
                }
                (_, WeaponTarget::Point { .. }) => (None, miss),
            };
            return Err((End::Fuzed { target, miss }, k2));
        }
    }

    // Out of energy: too slow, range flown, or down at the aimpoint's
    // height without fuzing (a glide weapon short of its target, a rocket
    // into the sea).
    let spent = match p.kind {
        ProfileKind::Rocket | ProfileKind::Ballistic => {
            tof >= p.boost_s && v < p.min_speed_mach.unwrap_or(0.0) * sound
        }
        ProfileKind::AirBreathing => f.flown_m >= p.max_range_m,
        ProfileKind::Glide => alt <= aim_alt,
    };
    if spent || alt < 0.0 || tof > MAX_FLIGHT_S {
        return Err((End::Plain(Outcome::OutOfEnergy), k2));
    }
    Ok(k2)
}

/// Nearest valid target to the aimpoint inside the seeker's range and field
/// of view; the contact the weapon was fired at first.
#[allow(clippy::too_many_arguments)]
fn acquire(
    list: &[Candidate],
    w: &Weapon,
    f: &WeaponFlight,
    side: u8,
    k: Kinematics,
    aim: LatLon,
    range_m: f64,
    in_fov: &dyn Fn(Enu) -> bool,
) -> Option<UnitId> {
    let wanted = match f.target {
        WeaponTarget::Track { key, .. } => Some(key),
        WeaponTarget::Truth(u) => Some(u),
        WeaponTarget::Point { .. } => None,
    };
    let only_wanted = needs_track(w.guidance.terminal);
    let mut best: Option<(bool, f64, UnitId)> = None;
    for c in list {
        if c.side == side || !w.targets.contains(&c.domain) {
            continue;
        }
        if w.guidance.terminal == Seeker::AntiRadiation && !c.radar_on {
            continue;
        }
        let is_wanted = wanted.is_some_and(|x| x == c.key || x == c.uid);
        if only_wanted && !is_wanted {
            continue;
        }
        // Cheap prune before the ENU transform.
        if distance_m(k.pos(), c.pos) > range_m {
            continue;
        }
        let rel = to_enu(k.pos(), k.alt_m, c.pos, c.alt_m);
        if norm(rel) > range_m || !in_fov(rel) {
            continue;
        }
        let score = (!is_wanted, distance_m(aim, c.pos), c.uid);
        if best.is_none_or(|b| (score.0, score.1, score.2) < (b.0, b.1, b.2)) {
            best = Some(score);
        }
    }
    best.map(|b| b.2)
}

/// The member of a truth key nearest a burst point, and its distance.
fn key_miss(world: &World, key: UnitId, at: LatLon, alt: f64) -> Option<(UnitId, f64)> {
    let e = entity(world, key)?;
    let members = match world.get::<crate::components::Group>(e) {
        Some(g) => g.members.clone(),
        None => vec![key],
    };
    members
        .iter()
        .filter_map(|m| {
            let tk = entity(world, *m).and_then(|e| world.get::<Kinematics>(e))?;
            Some((*m, norm(to_enu(at, alt, tk.pos(), tk.alt_m))))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
}

/// The side's estimate of a contact when the shooter itself holds a fresh
/// track on it (fire-control or track quality).
fn shooter_track(
    world: &World,
    side: u8,
    shooter: UnitId,
    contact: ContactId,
    now: u64,
    fresh_ms: u64,
) -> Option<AimPoint> {
    entity(world, shooter)?;
    let pics = world.resource::<SidePictures>();
    let c = pics.sides.get(usize::from(side))?.get(contact)?;
    let own = c.quality <= Quality::Track
        && c.sources
            .iter()
            .any(|s| s.observer == shooter && now.saturating_sub(s.last_ms) <= fresh_ms);
    if !own {
        return None;
    }
    side_estimate(world, side, contact)
}

/// The side's current estimate of a contact at track quality.
fn side_estimate(world: &World, side: u8, contact: ContactId) -> Option<AimPoint> {
    let pics = world.resource::<SidePictures>();
    let c = pics.sides.get(usize::from(side))?.get(contact)?;
    if c.quality > Quality::Track || c.lost {
        return None;
    }
    let est = c.est?;
    Some(AimPoint {
        at: est.at,
        alt_m: est.alt_m,
        heading: est.heading,
        speed_mps: est.speed_mps,
        climb_mps: 0.0,
        at_ms: c.last_ms,
    })
}

#[allow(clippy::too_many_arguments)]
fn terminate(
    world: &mut World,
    rules: &Rules,
    clock: &SimClock,
    id: UnitId,
    side: u8,
    f: &WeaponFlight,
    k: &Kinematics,
    outcome: Outcome,
    miss: Option<f64>,
    endgame: Option<Endgame>,
    // The unit the fuze resolved against (a hit's victim).
    fuzed_on: Option<UnitId>,
) {
    let w = &rules.db.weapons[f.weapon.0 as usize];
    let factors = Factors {
        speed_mps: k.speed_mps,
        time_of_flight_s: clock.end_ms().saturating_sub(f.launch_ms) as f64 / 1_000.0,
        launch_range_m: f.launch_range_m,
        flown_m: f.flown_m,
        miss_distance_m: miss,
        target_aware: f.target_aware,
        cm_used: f.cm_used.iter().map(|c| c.1).sum(),
        endgame,
    };
    let target = match f.target {
        WeaponTarget::Track { contact, .. } => {
            world
                .resource_mut::<Combat>()
                .last_end
                .insert((side, contact), clock.end_ms());
            Some(AimRef::Contact { contact })
        }
        WeaponTarget::Point { at, .. } => Some(AimRef::Point {
            lat_deg: at.lat_deg(),
            lon_deg: at.lon_deg(),
        }),
        WeaponTarget::Truth(_) => None,
    };
    // The side on the other end of the weapon sees the burst too: the unit it
    // fuzed on (a strike on a point can hit a ship), else the unit aimed at.
    let victim = fuzed_on.or(match f.target {
        WeaponTarget::Track { key, .. } | WeaponTarget::Truth(key) => Some(key),
        WeaponTarget::Point { .. } => None,
    });
    let other_side = victim
        .and_then(|u| entity(world, u))
        .and_then(|e| world.get::<Allegiance>(e))
        .map(|a| a.side);
    if let Some(e) = entity(world, id) {
        world.despawn(e);
    }
    world.resource_mut::<UnitIndex>().0.remove(&id);
    let salvo_report = {
        let mut c = world.resource_mut::<Combat>();
        c.in_flight = c.in_flight.saturating_sub(1);
        let mut report = None;
        if let Some(s) = c.salvos.get_mut(&f.salvo) {
            s.terminated += 1;
            match outcome {
                Outcome::Hit => s.hits += 1,
                Outcome::Miss => s.misses += 1,
                Outcome::Intercepted => s.intercepted += 1,
                Outcome::Decoyed => s.decoyed += 1,
                Outcome::Jammed => s.jammed += 1,
                _ => {}
            }
            if s.terminated >= s.total {
                report = Some((
                    s.side,
                    EventKind::SalvoReport {
                        salvo: f.salvo,
                        shooter: s.shooter,
                        total: s.total,
                        hits: s.hits,
                        misses: s.misses,
                        intercepted: s.intercepted,
                        decoyed: s.decoyed,
                        jammed: s.jammed,
                    },
                ));
            }
        }
        // An interceptor that fuzes on its target bursts where it fuzed; the
        // weapon it killed ends as `Intercepted` without a second burst.
        let eff_kind = match outcome {
            Outcome::Hit if f.intercept => Some(crate::view::effect::INTERCEPT),
            Outcome::Hit => Some(crate::view::effect::HIT),
            _ => None,
        };
        if let Some(kind) = eff_kind {
            c.effects.push(VisualEffect {
                time_ms: clock.end_ms(),
                at: k.pos(),
                alt_m: k.alt_m as f32,
                kind,
                sides: side_bit(side) | other_side.map_or(0, side_bit),
            });
        }
        trim_effects(&mut c, clock.end_ms());
        report
    };
    crate::picture::forget(&mut world.resource_mut::<SidePictures>(), id);
    emit(
        world,
        side,
        EventKind::WeaponTerminated {
            weapon: id,
            shooter: f.shooter,
            weapon_type: rules.db.str(w.id).to_owned(),
            weapon_name: rules.db.str(w.name).to_owned(),
            category: w.category,
            tag: f.tag,
            target,
            outcome,
            factors,
            intercept: f.intercept,
        },
    );
    if let Some((rep_side, rep_event)) = salvo_report {
        emit(world, rep_side, rep_event);
    }
}
