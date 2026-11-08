//! Engagement (doc 02 §12, doc 03 §5–7, P5): who fires at what, every
//! `tewa_period_s`.
//!
//! For each group in `UnitId` order:
//!
//! 1. **Self-defence (ADR-031):** ships and ground units allowed to defend
//!    themselves engage the weapon contacts their side's picture shows
//!    heading for a member (predicted closest approach inside
//!    `threat_cpa_m` within `threat_horizon_s`), earliest impact first, one
//!    interceptor per threat, again after `kill_assessment_s` if it
//!    survived. Aircraft defend by beaming and countermeasures (endgame).
//! 2. **Offensive:** candidates are hostile, classified, non-weapon
//!    contacts — the ordered target of an Intercept or Strike, the contacts
//!    inside a CAP's commit range, or any hostile otherwise. A shooter needs
//!    a weapon for the contact's domain and class, its own fresh track (or a
//!    networked one for `engage_on_remote` weapons) and the contact inside
//!    the launch-doctrine range from the flyout table. Ordered targets are
//!    pre-approved (ADR-029); the others go through ROE, and a blocked one
//!    becomes an approval request. `shots_per_target` rounds are fired from
//!    the members in slot order; the next shot waits for the last to end
//!    and `kill_assessment_s` (shoot-look-shoot).
//!
//! The pass also records whether any hostile is inside an engagement
//! envelope (`Combat.hostile_in_envelope`, the fine tick) and raises border
//! crossings and request expiries.

use bevy_ecs::prelude::*;
use std::collections::BTreeMap;
use wb_content::compiled::{Weapon, WeaponIx};
use wb_content::source::realism::Subsystem;
use wb_content::source::weapon::ProfileKind;
use wb_content::source::world::LaunchDoctrine;
use wb_content::source::{Domain, StationKind, TargetClass, WeaponCategory};
use wb_core::geo::{LatLon, destination_point, distance_m, initial_bearing};
use wb_core::math::cos;
use wb_core::{ContactId, UnitId};

use crate::clock::SimClock;
use crate::components::{
    Allegiance, Group, Health, Kinematics, Magazine, Mode, PlatformRef, Posture, StrikeTarget,
    Task, WeaponFlight, WeaponTarget,
};
use crate::events::{EventKind, emit};
use crate::picture::{Contact, ContactIdentity, Quality, SidePictures, TypeRef};
use crate::roe::{self, Gate};
use crate::rules::Rules;
use crate::weapons::{Aim, Combat, Launch, LaunchReason, launch};
use crate::world::{UnitIndex, entity};

/// A shooter's own track is fresh for this long (the slowest scan period).
const TRACK_FRESH_MS: u64 = 12_000;

/// A member that can fire this pass.
struct Shooter {
    uid: UnitId,
    pos: LatLon,
    air: bool,
    /// Weapons with rounds left on usable stations, by weapon.
    rounds: Vec<(WeaponIx, u32)>,
    /// Full capacity on usable stations, by weapon.
    capacities: BTreeMap<WeaponIx, u32>,
}

/// The engage stage.
pub fn run(world: &mut World) {
    let rules = world.resource::<Rules>().clone();
    let clock = world.resource::<SimClock>().clone();
    let period = (rules.realism().time.tewa_period_s * 1_000.0).max(1_000.0) as u64;
    if !clock.due(period) {
        return;
    }
    let now = clock.sim_time_ms;
    roe::expire(world, now);
    roe::borders(world, &rules);

    let mut inflight: BTreeMap<(u8, ContactId), u32> = BTreeMap::new();
    let mut inflight_by_shooter: BTreeMap<UnitId, u32> = BTreeMap::new();
    for &e in world.resource::<UnitIndex>().0.values() {
        if let (Some(f), Some(a)) = (world.get::<WeaponFlight>(e), world.get::<Allegiance>(e)) {
            *inflight_by_shooter.entry(f.shooter).or_default() += 1;
            if let WeaponTarget::Track { contact, .. } = f.target {
                *inflight.entry((a.side, contact)).or_default() += 1;
            }
        }
    }
    let mut q = world.query::<(&crate::components::Identity, &Group, &Allegiance)>();
    let mut groups: Vec<(UnitId, u8)> = q.iter(world).map(|(i, _, a)| (i.unit, a.side)).collect();
    groups.sort();
    let mut in_env = false;
    let mut incoming = BTreeMap::new();
    for (gid, side) in groups {
        let Some(ge) = entity(world, gid) else {
            continue;
        };
        let Some(g) = world.get::<Group>(ge).cloned() else {
            continue;
        };
        if matches!(g.mode, Mode::AtBase { .. }) {
            continue;
        }
        let shooters = shooters(world, &rules, &g);
        // A strike whose group is all deployed but has no rounds of the ordered weapon
        // left on working launchers can never fire: end it and say why.
        if let Task::Strike { weapon, .. } | Task::Suppress { weapon, .. } = g.task
            && g.members.iter().all(|m| {
                entity(world, *m)
                    .and_then(|e| world.get::<Posture>(e))
                    .is_some_and(|p| matches!(p, Posture::Moving))
            })
            && !shooters.iter().any(|s| {
                s.rounds
                    .iter()
                    .any(|(w, n)| *n > 0 && weapon.is_none_or(|x| x == *w))
            })
        {
            let what = weapon.map_or_else(
                || "weapons".to_owned(),
                |w| rules.db.str(rules.db.weapons[w.0 as usize].name).to_owned(),
            );
            emit(
                world,
                side,
                EventKind::StrikeAborted {
                    group: gid,
                    reason: format!("no {what} left on working launchers"),
                },
            );
            end_strike(world, &rules, side, gid);
            continue;
        }
        if shooters.is_empty() {
            continue;
        }
        self_defence(
            world,
            &rules,
            side,
            gid,
            &shooters,
            &mut inflight,
            &mut inflight_by_shooter,
            &mut incoming,
            now,
        );
        in_env |= offensive(world, &rules, side, gid, &g, &shooters, &mut inflight, now);
    }
    // Incoming weapons stay marked while their contact exists, so a threat
    // whose predicted approach flickers is announced once.
    let alive: Vec<(u8, ContactId)> = {
        let pics = world.resource::<SidePictures>();
        world
            .resource::<Combat>()
            .incoming
            .keys()
            .filter(|(s, ct)| {
                pics.sides
                    .get(usize::from(*s))
                    .is_some_and(|p| p.get(*ct).is_some())
            })
            .copied()
            .collect()
    };
    let mut c = world.resource_mut::<Combat>();
    c.hostile_in_envelope = in_env;
    let fresh: Vec<(u8, ContactId, UnitId, f64)> = incoming
        .iter()
        .filter(|(k, _)| !c.incoming.contains_key(*k))
        .map(|((s, ct), (u, tti))| (*s, *ct, *u, *tti))
        .collect();
    c.incoming.retain(|k, _| alive.contains(k));
    for (k, v) in &incoming {
        c.incoming.insert(*k, v.0);
    }
    for (side, contact, unit, tti) in fresh {
        emit(
            world,
            side,
            EventKind::IncomingWeapon {
                contact,
                unit,
                time_to_impact_s: tti,
            },
        );
    }
}

/// Members able to fire: moving, not out of action, with rounds on a
/// working station.
fn shooters(world: &World, rules: &Rules, g: &Group) -> Vec<Shooter> {
    let oof = rules.realism().damage.ship_out_of_action_fraction;
    let mut out = Vec::new();
    for m in &g.members {
        let Some(e) = entity(world, *m) else { continue };
        if !matches!(world.get::<Posture>(e), Some(Posture::Moving)) {
            continue;
        }
        let (Some(p), Some(k), Some(mag)) = (
            world.get::<PlatformRef>(e).map(|p| rules.platform(p.0)),
            world.get::<Kinematics>(e),
            world.get::<Magazine>(e),
        ) else {
            continue;
        };
        let health = world.get::<Health>(e);
        if health.is_some_and(|h| h.max > 0.0 && h.points <= h.max * oof) {
            continue;
        }
        let out_kind = |kind: StationKind| {
            health.is_some_and(|h| match kind {
                StationKind::Vls => h.has_out(Subsystem::Vls),
                StationKind::Launcher => h.has_out(Subsystem::Launcher),
                _ => false,
            })
        };
        let mut rounds: BTreeMap<WeaponIx, u32> = BTreeMap::new();
        let mut capacities: BTreeMap<WeaponIx, u32> = BTreeMap::new();
        for s in &mag.stations {
            let kind = p.stations.get(s.station as usize).map(|st| st.kind);
            if s.count > 0 && !kind.is_some_and(out_kind) {
                *rounds.entry(s.weapon).or_default() += s.count;
            }
            if !kind.is_some_and(out_kind) {
                *capacities.entry(s.weapon).or_default() += s.full;
            }
        }
        if rounds.is_empty() {
            continue;
        }
        out.push(Shooter {
            uid: *m,
            pos: k.pos(),
            air: rules.is_air(p),
            rounds: rounds.into_iter().collect(),
            capacities,
        });
    }
    out
}

/// The contact's class when classified (weapons and platforms).
fn class_of(rules: &Rules, c: &Contact) -> Option<TargetClass> {
    if c.level < wb_content::source::realism::ContactLevel::Class {
        return None;
    }
    match c.type_ref {
        TypeRef::Platform(p) => Some(rules.platform(p).target_class),
        TypeRef::Weapon(w) => rules.db.weapons[w.0 as usize].target_class,
        TypeRef::System(s) => rules.db.systems[s.0 as usize]
            .components
            .first()
            .map(|x| rules.platform(x.platform).target_class),
    }
}

/// Available fire channels for a shooter or composite system.
pub(crate) fn available_fire_channels(
    world: &World,
    rules: &Rules,
    shooter: UnitId,
    group: Option<&Group>,
) -> u32 {
    let mut channels = 0;
    if let Some(g) = group
        && g.system.is_some()
    {
        for m in &g.members {
            if let Some(me) = entity(world, *m) {
                let health = world.get::<Health>(me);
                if health.is_some_and(|h| h.has_out(Subsystem::Radar)) {
                    continue;
                }
                if let Some(p) = world.get::<PlatformRef>(me).map(|p| rules.platform(p.0)) {
                    for sm in &p.sensors {
                        if let Some(r) = rules.db.sensors[sm.sensor.0 as usize].radar.as_ref() {
                            channels += r.fire_channels;
                        }
                    }
                }
            }
        }
    } else if let Some(se) = entity(world, shooter) {
        let health = world.get::<Health>(se);
        if !health.is_some_and(|h| h.has_out(Subsystem::Radar))
            && let Some(p) = world.get::<PlatformRef>(se).map(|p| rules.platform(p.0))
        {
            for sm in &p.sensors {
                if let Some(r) = rules.db.sensors[sm.sensor.0 as usize].radar.as_ref() {
                    channels += r.fire_channels;
                }
            }
        }
    }
    if channels == 0 {
        channels = 2;
    }
    channels
}

/// Whether the shooter may fire at a contact: its own confirmed radar track
/// on the contact (fire control needs no report to reach the side's
/// picture), a fresh own detection at track quality, or, for weapons that
/// engage on remote tracks, the side's track.
fn has_track(
    world: &World,
    c: &Contact,
    group: Option<&Group>,
    shooter: UnitId,
    w: &Weapon,
    now: u64,
) -> bool {
    if c.lost || c.est.is_none() {
        return false;
    }
    // If the group is in autonomous transition delay, it cannot track or engage.
    if let Some(g) = group
        && g.autonomous_after_ms.is_some_and(|t| now < t)
    {
        return false;
    }
    let is_autonomous = group
        .and_then(|g| g.autonomous_after_ms)
        .is_some_and(|t| now >= t);

    let pics = world.resource::<SidePictures>();
    let is_own_observer =
        |obs: UnitId| obs == shooter || group.is_some_and(|g| g.members.contains(&obs));

    let own_radar = group.map_or_else(
        || {
            pics.tracks
                .range((shooter, 0, UnitId(0))..=(shooter, u8::MAX, UnitId(u32::MAX)))
                .any(|((_, _, t), st)| {
                    st.confirmed
                        && (*t == c.truth
                            || entity(world, *t)
                                .and_then(|e| world.get::<crate::components::GroupMember>(e))
                                .is_some_and(|m| m.group == c.truth))
                })
        },
        |g| {
            g.members.iter().any(|m| {
                pics.tracks
                    .range((*m, 0, UnitId(0))..=(*m, u8::MAX, UnitId(u32::MAX)))
                    .any(|((_, _, t), st)| {
                        st.confirmed
                            && (*t == c.truth
                                || entity(world, *t)
                                    .and_then(|e| world.get::<crate::components::GroupMember>(e))
                                    .is_some_and(|mem| mem.group == c.truth))
                    })
            })
        },
    );
    if own_radar {
        return true;
    }

    let own_source = c
        .sources
        .iter()
        .any(|s| is_own_observer(s.observer) && now.saturating_sub(s.last_ms) <= TRACK_FRESH_MS);

    if own_source {
        return c.quality <= Quality::Track;
    }

    // In autonomous mode, the site cannot use remote tracks from external units.
    if is_autonomous {
        return false;
    }

    c.quality <= Quality::Track && w.engage_on_remote
}

/// A contact's estimate dead-reckoned to now, and its speed toward `from`.
fn geometry(c: &Contact, from: LatLon, now: u64) -> Option<(LatLon, f64, f64)> {
    let e = c.est?;
    let dt = now.saturating_sub(c.last_ms) as f64 / 1_000.0;
    let at = destination_point(e.at, e.heading, e.speed_mps * dt);
    let to_shooter = initial_bearing(at, from);
    Some((at, e.alt_m, e.speed_mps * cos(e.heading - to_shooter)))
}

/// Nation doctrine of a group, with the group's overrides.
fn doctrine(world: &World, rules: &Rules, gid: UnitId, g: &Group) -> (LaunchDoctrine, u32) {
    let nation = entity(world, gid)
        .and_then(|e| world.get::<Allegiance>(e))
        .map(|a| a.nation);
    let d = nation.map(|c| &rules.db.doctrines[rules.db.nations[c.0 as usize].doctrine.0 as usize]);
    (
        g.doctrine
            .launch
            .or(d.map(|d| d.launch_doctrine))
            .unwrap_or(LaunchDoctrine::MaxRange),
        g.doctrine
            .shots_per_target
            .or(d.map(|d| d.shots_per_target))
            .unwrap_or(1)
            .max(1),
    )
}

/// Launch range allowed by doctrine against a target closing at `v_close`
/// that could run at `v_escape`.
fn doctrine_range(
    rules: &Rules,
    wix: WeaponIx,
    d: LaunchDoctrine,
    v_close: f64,
    v_escape: f64,
) -> f64 {
    let fly = rules.flyout(wix);
    match d {
        LaunchDoctrine::MaxRange => fly.max_range(v_close),
        LaunchDoctrine::NoEscapeZone => fly.max_range(-v_escape),
        LaunchDoctrine::Close => rules.realism().weapons.close_fraction * fly.max_range(v_close),
    }
}

fn fits(rules: &Rules, w: &Weapon, c: &Contact) -> bool {
    if !w.targets.contains(&c.domain) {
        return false;
    }
    match class_of(rules, c) {
        Some(class) => w.pk_base.get(&class).is_some_and(|p| *p > 0.0),
        None => true,
    }
}

#[allow(clippy::too_many_arguments)]
fn self_defence(
    world: &mut World,
    rules: &Rules,
    side: u8,
    gid: UnitId,
    shooters: &[Shooter],
    inflight: &mut BTreeMap<(u8, ContactId), u32>,
    inflight_by_shooter: &mut BTreeMap<UnitId, u32>,
    incoming: &mut BTreeMap<(u8, ContactId), (UnitId, f64)>,
    now: u64,
) {
    let rr = &rules.realism().roe;
    let assess_ms = (rules.realism().tewa.kill_assessment_s * 1_000.0) as u64;

    let ge = entity(world, gid);
    let g = ge.and_then(|e| world.get::<Group>(e).cloned());
    let nation = ge
        .and_then(|e| world.get::<Allegiance>(e))
        .map(|a| a.nation);
    let doc =
        nation.map(|c| &rules.db.doctrines[rules.db.nations[c.0 as usize].doctrine.0 as usize]);
    let reserve_fraction = doc.map_or(0.0, |d| d.missile_reserve_fraction);

    // Friendly ships and ground units to defend
    let friendlies: Vec<(UnitId, LatLon)> = {
        let index = world.resource::<UnitIndex>();
        index
            .0
            .iter()
            .filter_map(|(uid, e)| {
                let alg = world.get::<Allegiance>(*e)?;
                if alg.side != side {
                    return None;
                }
                let p = world.get::<PlatformRef>(*e).map(|p| rules.platform(p.0))?;
                if rules.is_air(p) {
                    return None;
                }
                let k = world.get::<Kinematics>(*e)?;
                Some((*uid, k.pos()))
            })
            .collect()
    };

    let pics = world.resource::<SidePictures>();
    let Some(pic) = pics.sides.get(usize::from(side)) else {
        return;
    };

    let mut all_threats: Vec<(ContactId, UnitId, f64, f64)> = Vec::new();
    if pic.contacts.is_empty() {
        // eprintln!("DEBUG: pic.contacts is empty on tick {}", world.resource::<SimClock>().tick);
    }
    for c in pic.contacts.values() {
        if !matches!(c.type_ref, TypeRef::Weapon(_))
            || c.identity == ContactIdentity::Friendly
            || c.lost
        {
            continue;
        }
        let warhead_kg = match c.type_ref {
            TypeRef::Weapon(wix) => rules.db.weapons[wix.0 as usize].warhead_kg,
            _ => 100.0,
        };
        for (target_uid, target_pos) in &friendlies {
            let Some((tp, _, v_close)) = geometry(c, *target_pos, now) else {
                continue;
            };
            let Some(e) = c.est else { continue };
            let d = distance_m(tp, *target_pos);
            let brg = initial_bearing(tp, *target_pos);
            let across = d * wb_core::math::sin(e.heading - brg).abs();
            let tti = if v_close > 1.0 {
                d / v_close
            } else {
                f64::INFINITY
            };
            if across <= rr.threat_cpa_m && tti <= rr.threat_horizon_s {
                all_threats.push((c.id, *target_uid, tti, warhead_kg));
                incoming.entry((side, c.id)).or_insert((*target_uid, tti));
                break;
            }
        }
    }

    if all_threats.is_empty() {
        return;
    }

    for s in shooters {
        if s.air || !roe::effective(world, side, s.uid).2 {
            continue;
        }

        let mut prioritized: Vec<(u8, f64, f64, ContactId, UnitId)> = Vec::new();
        for (contact, target_uid, tti, warhead_kg) in &all_threats {
            let bucket = if *target_uid == s.uid { 0u8 } else { 1u8 };
            prioritized.push((bucket, *tti, *warhead_kg, *contact, *target_uid));
        }
        // Threat order: bucket asc (self first), tti asc, lethality desc, contact id asc
        prioritized.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then(a.1.total_cmp(&b.1))
                .then(b.2.total_cmp(&a.2))
                .then(a.3.cmp(&b.3))
        });

        let total_channels = available_fire_channels(world, rules, s.uid, g.as_ref());
        let used_channels = if let Some(g) = g.as_ref()
            && g.system.is_some()
        {
            g.members
                .iter()
                .map(|m| inflight_by_shooter.get(m).copied().unwrap_or(0))
                .sum::<u32>()
        } else {
            inflight_by_shooter.get(&s.uid).copied().unwrap_or(0)
        };
        let mut free_channels = total_channels.saturating_sub(used_channels);

        for (bucket, tti, _, contact, _) in prioritized {
            let current_inflight = inflight.get(&(side, contact)).copied().unwrap_or(0);
            if current_inflight >= 2 {
                continue;
            }
            let last = world
                .resource::<Combat>()
                .last_end
                .get(&(side, contact))
                .copied();
            if last.is_some_and(|t| now.saturating_sub(t) < assess_ms) {
                continue;
            }
            let Some(c) = world
                .resource::<SidePictures>()
                .sides
                .get(usize::from(side))
                .and_then(|p| p.get(contact))
                .cloned()
            else {
                continue;
            };
            let Some((at, _, v_close)) = geometry(&c, s.pos, now) else {
                continue;
            };
            let range = distance_m(s.pos, at);
            let Some(e) = c.est else { continue };

            let ciws_max = rules.realism().tewa.ciws_layer_max_m;
            let in_ciws = range <= ciws_max;

            let mut candidates: Vec<(WeaponIx, f64, bool, u32)> = Vec::new();
            for (wix, count) in &s.rounds {
                let w = &rules.db.weapons[wix.0 as usize];
                if !fits(rules, w, &c) {
                    continue;
                }
                if !has_track(world, &c, g.as_ref(), s.uid, w, now) {
                    continue;
                }
                let is_ciws = matches!(w.category, WeaponCategory::Shell);
                let full_cap = s.capacities.get(wix).copied().unwrap_or(*count);
                let reserve = (full_cap as f64 * reserve_fraction).round() as u32;
                let available_rounds = if bucket == 0 {
                    *count
                } else {
                    count.saturating_sub(reserve)
                };
                if available_rounds == 0 {
                    continue;
                }
                if !is_ciws && free_channels == 0 {
                    continue;
                }
                let v_max = w.profile.max_speed_mach * rules.speed_of_sound(e.alt_m);
                let v_avg = f64::max(0.75 * v_max, 200.0);
                let t_int = f64::max(range / (v_avg + v_close), 0.1);
                let pip = destination_point(at, e.heading, e.speed_mps * t_int);
                let d_pip = distance_m(s.pos, pip);
                let reach = rules.flyout(*wix).max_range(v_close);
                if d_pip < w.profile.min_range_m || d_pip > reach {
                    continue;
                }
                if let Some(env) = &w.envelope
                    && (e.alt_m < env.min_alt_m || e.alt_m > env.max_alt_m)
                {
                    continue;
                }
                candidates.push((*wix, reach, is_ciws, available_rounds));
            }

            if candidates.is_empty() {
                continue;
            }

            candidates.sort_by(|a, b| {
                if in_ciws {
                    b.2.cmp(&a.2).then(b.1.total_cmp(&a.1))
                } else {
                    a.2.cmp(&b.2).then(b.1.total_cmp(&a.1))
                }
            });

            let (chosen_wix, _, is_ciws, available_rounds) = candidates[0];
            let w = &rules.db.weapons[chosen_wix.0 as usize];

            let v_max = w.profile.max_speed_mach * rules.speed_of_sound(e.alt_m);
            let v_avg = f64::max(0.75 * v_max, 200.0);
            let t_flight = range / v_avg;
            let assess_s = rules.realism().tewa.kill_assessment_s;
            let can_look = tti >= (2.0 * t_flight + assess_s);

            if current_inflight >= 1 && can_look {
                continue;
            }

            let desired: u32 = if can_look {
                1
            } else if e.speed_mps >= 340.0 || w.profile.kind == ProfileKind::Ballistic {
                2
            } else {
                1
            };
            let needed = desired.saturating_sub(current_inflight);
            let max_possible = if is_ciws {
                needed.min(available_rounds)
            } else {
                needed.min(available_rounds).min(free_channels)
            };

            let mut fired = 0;
            for _ in 0..max_possible {
                if launch(
                    world,
                    &Launch {
                        shooter: s.uid,
                        weapon: chosen_wix,
                        aim: Aim::Contact(contact),
                        salvo: None,
                        reason: LaunchReason::SelfDefence,
                    },
                )
                .is_ok()
                {
                    fired += 1;
                    if !is_ciws {
                        free_channels = free_channels.saturating_sub(1);
                        *inflight_by_shooter.entry(s.uid).or_default() += 1;
                    }
                }
            }
            if fired > 0 {
                *inflight.entry((side, contact)).or_default() += fired;
            }
        }
    }
}

/// Offensive engagements of one group; whether a hostile was inside a
/// member's envelope.
#[allow(clippy::too_many_arguments)]
fn offensive(
    world: &mut World,
    rules: &Rules,
    side: u8,
    gid: UnitId,
    g: &Group,
    shooters: &[Shooter],
    inflight: &mut BTreeMap<(u8, ContactId), u32>,
    now: u64,
) -> bool {
    let (doc, shots_doc) = doctrine(world, rules, gid, g);
    let assess_ms = (rules.realism().tewa.kill_assessment_s * 1_000.0) as u64;
    let shots = g.doctrine.salvo.unwrap_or(shots_doc);
    let mut in_env = false;

    // Strike on a point: fire the salvo once in range.
    if let Task::Strike {
        target: StrikeTarget::Point(p),
        weapon,
        count,
        fired,
    }
    | Task::Suppress {
        target: StrikeTarget::Point(p),
        weapon,
        count,
        fired,
    } = g.task
    {
        let want = count.unwrap_or(shots).saturating_sub(fired);
        let mut done = 0;
        let mut salvo = None;
        for s in shooters {
            let range = distance_m(s.pos, p);
            for (wix, n) in &s.rounds {
                let w = &rules.db.weapons[wix.0 as usize];
                let usable = weapon.is_none_or(|x| x == *wix)
                    && (w.targets.contains(&Domain::Ground)
                        || w.targets.contains(&Domain::Surface))
                    && range >= w.profile.min_range_m
                    && range <= rules.flyout(*wix).max_range(0.0);
                if !usable {
                    continue;
                }
                let salvo =
                    *salvo.get_or_insert_with(|| world.resource_mut::<Combat>().new_salvo());
                for _ in 0..(*n).min(want.saturating_sub(done)) {
                    if launch(
                        world,
                        &Launch {
                            shooter: s.uid,
                            weapon: *wix,
                            aim: Aim::Point { at: p, alt_m: 0.0 },
                            salvo: Some(salvo),
                            reason: LaunchReason::Ordered,
                        },
                    )
                    .is_ok()
                    {
                        done += 1;
                    }
                }
            }
        }
        if done > 0 {
            strike_fired(world, rules, side, gid, done, want);
        }
        return done > 0;
    }

    let ordered = match g.task {
        Task::Intercept { contact } => Some(contact),
        Task::Strike {
            target: StrikeTarget::Contact(c),
            ..
        }
        | Task::Suppress {
            target: StrikeTarget::Contact(c),
            ..
        } => Some(c),
        _ => None,
    };
    let (strike_weapon, strike_left) = match g.task {
        Task::Strike {
            weapon,
            count,
            fired,
            ..
        }
        | Task::Suppress {
            weapon,
            count,
            fired,
            ..
        } => (weapon, Some(count.unwrap_or(shots).saturating_sub(fired))),
        _ => (None, None),
    };
    let candidates: Vec<Contact> = {
        let pics = world.resource::<SidePictures>();
        let Some(pic) = pics.sides.get(usize::from(side)) else {
            return false;
        };
        pic.contacts
            .values()
            .filter(|c| match ordered {
                Some(o) => c.id == o && c.identity != ContactIdentity::Friendly,
                None => {
                    c.identity == ContactIdentity::Hostile
                        && !matches!(c.type_ref, TypeRef::Weapon(_))
                        && c.level >= wb_content::source::realism::ContactLevel::Class
                }
            })
            .filter(|c| match g.task {
                Task::Cap {
                    centre, commit_m, ..
                } => c.est.is_some_and(|e| distance_m(e.at, centre) <= commit_m),
                _ => true,
            })
            .cloned()
            .collect()
    };
    for c in candidates {
        let want = strike_left.unwrap_or(shots);
        if want == 0 {
            continue;
        }
        // Shoot-look-shoot: wait for our weapons at it to end, then assess.
        let flying = inflight.get(&(side, c.id)).copied().unwrap_or(0);
        let last = world
            .resource::<Combat>()
            .last_end
            .get(&(side, c.id))
            .copied();
        if flying > 0 || last.is_some_and(|t| now.saturating_sub(t) < assess_ms) {
            in_env = true;
            continue;
        }
        let v_escape = match c.type_ref {
            TypeRef::Platform(p) if c.level >= wb_content::source::realism::ContactLevel::Type => {
                rules.max_mps(rules.platform(p), c.est.map_or(0.0, |e| e.alt_m))
            }
            _ => c.est.map_or(0.0, |e| e.speed_mps),
        };
        // First shooter in slot order with a weapon in range; its best Pk.
        let mut pick: Option<(usize, WeaponIx, f64, f64, f64)> = None;
        for (i, s) in shooters.iter().enumerate() {
            let Some((at, _, v_close)) = geometry(&c, s.pos, now) else {
                continue;
            };
            let range = distance_m(s.pos, at);
            let class = class_of(rules, &c);
            let best = s
                .rounds
                .iter()
                .map(|r| r.0)
                .filter(|wix| strike_weapon.is_none_or(|x| x == *wix))
                .filter(|wix| {
                    let w = &rules.db.weapons[wix.0 as usize];
                    fits(rules, w, &c)
                        && has_track(world, &c, Some(g), s.uid, w, now)
                        && range >= w.profile.min_range_m
                        && range <= doctrine_range(rules, *wix, doc, v_close, v_escape)
                })
                .map(|wix| {
                    let w = &rules.db.weapons[wix.0 as usize];
                    let pk = class
                        .and_then(|k| w.pk_base.get(&k).copied())
                        .unwrap_or(0.5);
                    (wix, pk)
                })
                .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)));
            if let Some((wix, pk)) = best {
                pick = Some((i, wix, pk, range, v_close));
                break;
            }
        }
        let Some((first, wix, pk_base, range, v_close)) = pick else {
            continue;
        };
        in_env = true;
        let reason = if ordered.is_some() {
            LaunchReason::Ordered
        } else if roe::take_approval(world, side, gid, c.id) {
            LaunchReason::Approved
        } else {
            match roe::gate(world, rules, side, shooters[first].uid, &c, now) {
                Gate::Fire(r) => r,
                Gate::Ask => {
                    let fly = rules.flyout(wix);
                    let window_s = if v_close < 0.0 {
                        (fly.max_range(v_close) - range).max(0.0) / -v_close
                    } else {
                        rules.realism().roe.threat_horizon_s
                    };
                    if window_s >= rules.realism().roe.approval_window_min_s {
                        let fe = fly.intercept(range, v_close).map_or(0.0, |x| x.1);
                        roe::ask(
                            world,
                            rules,
                            side,
                            gid,
                            shooters[first].uid,
                            c.id,
                            wix,
                            pk_base * fe,
                            now + (window_s * 1_000.0) as u64,
                        );
                    }
                    continue;
                }
            }
        };
        // Fire `want` rounds of the weapon, members in slot order.
        let salvo = world.resource_mut::<Combat>().new_salvo();
        let mut done = 0;
        for s in &shooters[first..] {
            let n = s.rounds.iter().find(|r| r.0 == wix).map_or(0, |r| r.1);
            for _ in 0..n.min(want.saturating_sub(done)) {
                if launch(
                    world,
                    &Launch {
                        shooter: s.uid,
                        weapon: wix,
                        aim: Aim::Contact(c.id),
                        salvo: Some(salvo),
                        reason,
                    },
                )
                .is_ok()
                {
                    done += 1;
                }
            }
            if done >= want {
                break;
            }
        }
        *inflight.entry((side, c.id)).or_default() += done;
        if let Some(left) = strike_left
            && done > 0
        {
            strike_fired(world, rules, side, gid, done, left);
        }
    }
    in_env
}

/// Counts a strike's rounds; once all are away the group goes home (air) or
/// stops (ships).
fn strike_fired(world: &mut World, rules: &Rules, side: u8, gid: UnitId, done: u32, want: u32) {
    let Some(ge) = entity(world, gid) else { return };
    let mut g = std::mem::take(&mut *world.get_mut::<Group>(ge).expect("group"));
    if let Task::Strike { fired, .. } | Task::Suppress { fired, .. } = &mut g.task {
        *fired += done;
    }
    *world.get_mut::<Group>(ge).expect("group") = g;
    if done >= want {
        end_strike(world, rules, side, gid);
    }
}

/// Ends a strike: aircraft go home, ships stop.
fn end_strike(world: &mut World, rules: &Rules, side: u8, gid: UnitId) {
    let Some(ge) = entity(world, gid) else { return };
    let mut g = std::mem::take(&mut *world.get_mut::<Group>(ge).expect("group"));
    let air = g
        .lead()
        .and_then(|l| entity(world, l))
        .and_then(|e| world.get::<PlatformRef>(e))
        .is_some_and(|p| rules.is_air(rules.platform(p.0)));
    if air {
        let list = crate::bases::list(world);
        crate::tasks::go_home(
            world,
            rules,
            &list,
            side,
            gid,
            &mut g,
            crate::components::ReturnWhy::Order,
        );
    } else {
        g.task = Task::Idle;
        emit(
            world,
            side,
            EventKind::TaskChanged {
                group: gid,
                task: "idle".to_owned(),
            },
        );
    }
    *world.get_mut::<Group>(ge).expect("group") = g;
}

/// Why a strike has not fired yet, in plain English, or `None` when nothing
/// holds it back (CLAUDE.md rule 4). Read-only: the card shows it live and a
/// strike order's acknowledgement carries it. `g` gives the members; `task` is
/// the strike (it may not be the group's task yet).
pub fn strike_status(
    world: &World,
    rules: &Rules,
    side: u8,
    g: &Group,
    task: &Task,
) -> Option<String> {
    let (target, weapon) = match task {
        Task::Strike { target, weapon, .. } | Task::Suppress { target, weapon, .. } => {
            (*target, *weapon)
        }
        _ => return None,
    };
    let now = world.resource::<SimClock>().sim_time_ms;
    let contact = match target {
        StrikeTarget::Contact(id) => {
            let pics = world.resource::<SidePictures>();
            Some(pics.sides.get(usize::from(side))?.get(id)?.clone())
        }
        StrikeTarget::Point(_) => None,
    };
    let (at, domain) = match (&contact, target) {
        (Some(c), _) => (geometry(c, LatLon::from_deg(0.0, 0.0), now)?.0, c.domain),
        (None, StrikeTarget::Point(p)) => (p, Domain::Surface),
        _ => return None,
    };
    // The lead's position, platform and rounds stand for the group.
    let lead = *g.members.first()?;
    let e = entity(world, lead)?;
    let k = world.get::<Kinematics>(e)?;
    let p = rules.platform(world.get::<PlatformRef>(e)?.0);
    let mag = world.get::<Magazine>(e)?;
    let range = distance_m(k.pos(), at);
    let (wix, w) = mag
        .rounds()
        .into_iter()
        .filter(|(_, n)| *n > 0)
        .map(|(wix, _)| (wix, &rules.db.weapons[wix.0 as usize]))
        .filter(|(wix, w)| weapon.is_none_or(|x| x == *wix) && w.targets.contains(&domain))
        .max_by(|a, b| {
            a.1.profile
                .max_range_m
                .total_cmp(&b.1.profile.max_range_m)
                .then(b.0.cmp(&a.0))
        })?;
    let name = rules.db.str(w.name);
    let reach = rules.flyout(wix).max_range(0.0);
    let km = |m: f64| (m / 1_000.0).round();
    if range > reach {
        return Some(format!(
            "Closing to weapon range: {name} reaches {} km, target {} km away",
            km(reach),
            km(range)
        ));
    }
    let c = contact?;
    if has_track(world, &c, Some(g), lead, w, now) {
        return None;
    }
    if w.engage_on_remote {
        return Some(format!(
            "In range; waiting for a firm track on the target before firing {name}"
        ));
    }
    // Our own radar horizon against this kind of target (doc 02 §7).
    let earth = &rules.realism().earth;
    let h_target = match domain {
        Domain::Air | Domain::Space => c.est.map_or(0.0, |e| e.alt_m),
        Domain::Ground => earth.ground_target_height_m,
        _ => earth.surface_target_height_m,
    };
    let base = if rules.is_air(p) {
        k.alt_m.max(0.0)
    } else {
        0.0
    };
    let mast = p
        .sensors
        .iter()
        .filter(|m| rules.db.sensors[m.sensor.0 as usize].radar.is_some())
        .map(|m| m.height_m)
        .max_by(f64::total_cmp);
    let horizon = mast.map(|h| crate::sensors::horizon_m(rules, base + h, h_target));
    Some(match horizon {
        Some(hz) if hz < range => format!(
            "Closing to own radar range: {name} fires only on our own track, and the target ({} km) is beyond our radar horizon (about {} km)",
            km(range),
            km(hz)
        ),
        _ => format!(
            "Closing: {name} fires only on our own track, and our own sensors do not hold the target yet"
        ),
    })
}
