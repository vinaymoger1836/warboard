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
use wb_content::source::world::LaunchDoctrine;
use wb_content::source::{Domain, StationKind, TargetClass};
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
    for &e in world.resource::<UnitIndex>().0.values() {
        if let (Some(f), Some(a)) = (world.get::<WeaponFlight>(e), world.get::<Allegiance>(e))
            && let WeaponTarget::Track { contact, .. } = f.target
        {
            *inflight.entry((a.side, contact)).or_default() += 1;
        }
    }
    let mut q = world.query::<(&crate::components::Identity, &Group, &Allegiance)>();
    let mut groups: Vec<(UnitId, u8)> = q
        .iter(world)
        .map(|(i, _, a)| (i.unit, a.side))
        .collect();
    groups.sort();
    let mut in_env = false;
    let mut incoming = BTreeMap::new();
    for (gid, side) in groups {
        let Some(ge) = entity(world, gid) else { continue };
        let Some(g) = world.get::<Group>(ge).cloned() else {
            continue;
        };
        if matches!(g.mode, Mode::AtBase { .. }) {
            continue;
        }
        let shooters = shooters(world, &rules, &g);
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
            &mut incoming,
            now,
        );
        in_env |= offensive(world, &rules, side, gid, &g, &shooters, &mut inflight, now);
    }
    let mut c = world.resource_mut::<Combat>();
    c.hostile_in_envelope = in_env;
    let fresh: Vec<(u8, ContactId, UnitId, f64)> = incoming
        .iter()
        .filter(|(k, _)| !c.incoming.contains_key(*k))
        .map(|((s, ct), (u, tti))| (*s, *ct, *u, *tti))
        .collect();
    c.incoming = incoming.iter().map(|(k, v)| (*k, v.0)).collect();
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
        for s in &mag.stations {
            let kind = p.stations.get(s.station as usize).map(|st| st.kind);
            if s.count > 0 && !kind.is_some_and(out_kind) {
                *rounds.entry(s.weapon).or_default() += s.count;
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

fn has_track(c: &Contact, shooter: UnitId, w: &Weapon, now: u64) -> bool {
    if c.lost || c.quality > Quality::Track || c.est.is_none() {
        return false;
    }
    w.engage_on_remote
        || c.sources
            .iter()
            .any(|s| s.observer == shooter && now.saturating_sub(s.last_ms) <= TRACK_FRESH_MS)
}

/// A contact's estimate dead-reckoned to now, and its speed toward `from`.
fn geometry(c: &Contact, from: LatLon, now: u64) -> Option<(LatLon, f64, f64)> {
    let e = c.est?;
    let dt = now.saturating_sub(c.last_ms) as f64 / 1_000.0;
    let at = destination_point(e.at, e.heading, e.speed_mps * dt);
    let to_shooter = initial_bearing(at, from);
    Some((at, e.alt_m, e.speed_mps * cos(e.heading - to_shooter)))
}

/// Country doctrine of a group, with the group's overrides.
fn doctrine(world: &World, rules: &Rules, gid: UnitId, g: &Group) -> (LaunchDoctrine, u32) {
    let country = entity(world, gid)
        .and_then(|e| world.get::<Allegiance>(e))
        .map(|a| a.country);
    let d = country.map(|c| &rules.db.doctrines[rules.db.countries[c.0 as usize].doctrine.0 as usize]);
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
fn doctrine_range(rules: &Rules, wix: WeaponIx, d: LaunchDoctrine, v_close: f64, v_escape: f64) -> f64 {
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
    incoming: &mut BTreeMap<(u8, ContactId), (UnitId, f64)>,
    now: u64,
) {
    let rr = &rules.realism().roe;
    let assess_ms = (rules.realism().tewa.kill_assessment_s * 1_000.0) as u64;
    // Threats to any member, earliest impact first.
    let mut threats: Vec<(f64, ContactId, UnitId)> = Vec::new();
    {
        let pics = world.resource::<SidePictures>();
        let Some(pic) = pics.sides.get(usize::from(side)) else {
            return;
        };
        let members: Vec<(UnitId, LatLon)> = entity(world, gid)
            .and_then(|e| world.get::<Group>(e))
            .map(|g| {
                g.members
                    .iter()
                    .filter_map(|m| {
                        let k = entity(world, *m).and_then(|e| world.get::<Kinematics>(e))?;
                        Some((*m, k.pos()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        for c in pic.contacts.values() {
            if !matches!(c.type_ref, TypeRef::Weapon(_))
                || c.identity == ContactIdentity::Friendly
                || c.lost
            {
                continue;
            }
            for (m, at) in &members {
                let Some((tp, _, v_close)) = geometry(c, *at, now) else {
                    continue;
                };
                let Some(e) = c.est else { continue };
                // Closest approach of a straight-line threat.
                let d = distance_m(tp, *at);
                let brg = initial_bearing(tp, *at);
                let across = d * wb_core::math::sin(e.heading - brg).abs();
                let tti = if v_close > 1.0 { d / v_close } else { f64::INFINITY };
                if across <= rr.threat_cpa_m && tti <= rr.threat_horizon_s {
                    threats.push((tti, c.id, *m));
                    break;
                }
            }
        }
    }
    threats.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    for (tti, contact, unit) in threats {
        incoming.entry((side, contact)).or_insert((unit, tti));
        if inflight.get(&(side, contact)).copied().unwrap_or(0) > 0 {
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
        for s in shooters {
            if s.air || !roe::effective(world, side, s.uid).2 {
                continue;
            }
            let Some((at, _, v_close)) = geometry(&c, s.pos, now) else {
                continue;
            };
            let range = distance_m(s.pos, at);
            let pick = s.rounds.iter().map(|r| r.0).find(|wix| {
                let w = &rules.db.weapons[wix.0 as usize];
                fits(rules, w, &c)
                    && has_track(&c, s.uid, w, now)
                    && range >= w.profile.min_range_m
                    && range <= rules.flyout(*wix).max_range(v_close)
            });
            let Some(wix) = pick else { continue };
            if launch(
                world,
                &Launch {
                    shooter: s.uid,
                    weapon: wix,
                    aim: Aim::Contact(contact),
                    salvo: None,
                    reason: LaunchReason::SelfDefence,
                },
            )
            .is_ok()
            {
                *inflight.entry((side, contact)).or_default() += 1;
                break;
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
                    && (w.targets.contains(&Domain::Ground) || w.targets.contains(&Domain::Surface))
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
        } => Some(c),
        _ => None,
    };
    let (strike_weapon, strike_left) = match g.task {
        Task::Strike {
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
        let last = world.resource::<Combat>().last_end.get(&(side, c.id)).copied();
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
                        && has_track(&c, s.uid, w, now)
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
    if let Task::Strike { fired, .. } = &mut g.task {
        *fired += done;
    }
    if done >= want {
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
    }
    *world.get_mut::<Group>(ge).expect("group") = g;
}
