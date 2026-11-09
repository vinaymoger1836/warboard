//! After-action reports (doc 03 §12, ADR-016): intel log, salvo reports,
//! losses and kills, munitions, sorties and operations, built from the event
//! log.
//!
//! A side's report reads only the events that side may see, plus its own
//! units and bases and its own picture (CLAUDE.md rule 1). Once the war has
//! ended (`EndWar`) the truth review unlocks: both sides' forces, the hits
//! recorded by truth-only events (`TruthHit`, side mask 0), and each side's
//! picture against what was really there.
//!
//! The same report exports as JSON (serde), one CSV per table and a printable
//! HTML page, so the CLI and the browser give identical files.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use wb_core::UnitId;

use crate::clock::SimClock;
use crate::components::{
    Allegiance, BaseRef, BaseSite, Group, GroupMember, Identity, Kinematics, Magazine, PlatformRef,
    WeaponFlight,
};
use crate::events::{AimRef, Event, EventKind, Outcome, emit};
use crate::operations::{OpState, Operations, PhaseState, PhaseTally, Stamp};
use crate::picture::{SidePictures, TypeRef};
use crate::rules::Rules;
use crate::world::{Sides, UnitIndex, entity};

/// Whether (and when) the war ended. Saved and hashed.
#[derive(Resource, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct War {
    pub ended: Option<Stamp>,
}

/// `EndWar`: the truth review unlocks; every side is told.
pub fn end_war(world: &mut World) {
    let c = world.resource::<SimClock>();
    let stamp = Stamp {
        tick: c.tick,
        ms: c.sim_time_ms,
    };
    world.resource_mut::<War>().ended = Some(stamp);
    let n = world.resource::<Sides>().len() as u8;
    for s in 0..n {
        emit(world, s, EventKind::WarEnded);
    }
}

/// `h:mm:ss` of sim time.
pub fn fmt_t(ms: u64) -> String {
    let s = ms / 1_000;
    format!("{}:{:02}:{:02}", s / 3_600, (s / 60) % 60, s % 60)
}

// ---------------------------------------------------------------------------
// Report types (TS-exported for the client)
// ---------------------------------------------------------------------------

/// Counts the acceptance test compares with the raw events (P7).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Totals {
    /// Our weapons launched.
    pub launched: u32,
    /// Our weapons that hit.
    pub hits: u32,
    pub misses: u32,
    /// Our weapons intercepted by the enemy.
    pub intercepted: u32,
    pub decoyed: u32,
    pub jammed: u32,
    /// Out of energy, lost track, self-destruct.
    pub other: u32,
    /// Our weapons still flying.
    pub in_flight: u32,
    /// Our interceptors that hit an enemy weapon.
    pub intercepts_made: u32,
    /// Our units lost.
    pub losses: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct IntelLine {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub t_ms: u64,
    /// `new`, `classified`, `count`, `lost`, `report`.
    pub kind: String,
    #[cfg_attr(feature = "ts", ts(optional))]
    pub contact: Option<u32>,
    pub source: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct WeaponLine {
    pub tag: String,
    /// `in_flight` until it ends.
    pub outcome: String,
    #[cfg_attr(feature = "ts", ts(optional))]
    pub pk: Option<f64>,
    /// The endgame factors in one line (CLAUDE.md rule 4).
    pub factors: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SalvoLine {
    pub salvo: u32,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub t_ms: u64,
    pub shooter: u32,
    pub shooter_name: String,
    pub weapon: String,
    pub weapon_name: String,
    pub count: u32,
    pub target: String,
    pub launch_range_km: f64,
    pub reason: String,
    pub hits: u32,
    pub misses: u32,
    pub intercepted: u32,
    pub decoyed: u32,
    pub jammed: u32,
    pub other: u32,
    pub in_flight: u32,
    pub weapons: Vec<WeaponLine>,
    /// The one-line summary (doc 03 §12 example).
    pub summary: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct LossLine {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub t_ms: u64,
    pub unit: u32,
    pub name: String,
    pub platform: String,
    pub cause: String,
}

/// A hit our weapon scored on a contact (what we know, not its condition).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct HitLine {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub t_ms: u64,
    pub target: String,
    pub weapon_name: String,
    pub shooter_name: String,
    /// The contact has since left our picture.
    pub gone: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct MunitionLine {
    pub weapon: String,
    pub name: String,
    pub expended: u32,
    pub on_units: u32,
    pub in_stock: u32,
    /// `(base, launched by units based there, in its stock)`.
    pub by_base: Vec<(String, u32, u32)>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SortieLine {
    pub unit: u32,
    pub name: String,
    pub base: String,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub took_off_ms: u64,
    #[cfg_attr(feature = "ts", ts(optional, type = "number"))]
    pub landed_ms: Option<u64>,
    #[cfg_attr(feature = "ts", ts(optional))]
    pub landed_at: Option<String>,
    #[cfg_attr(feature = "ts", ts(optional))]
    pub fuel_at_landing_kg: Option<f64>,
    pub refuels: u32,
    pub bingo: bool,
    pub launched: u32,
    pub loadout: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PhaseReport {
    pub name: String,
    pub state: PhaseState,
    #[cfg_attr(feature = "ts", ts(optional, type = "number"))]
    pub started_ms: Option<u64>,
    #[cfg_attr(feature = "ts", ts(optional, type = "number"))]
    pub ended_ms: Option<u64>,
    pub reason: String,
    pub tasks: Vec<String>,
    pub tally: PhaseTally,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct OperationReport {
    pub id: u32,
    pub name: String,
    pub state: OpState,
    #[cfg_attr(feature = "ts", ts(optional, type = "number"))]
    pub started_ms: Option<u64>,
    #[cfg_attr(feature = "ts", ts(optional, type = "number"))]
    pub ended_ms: Option<u64>,
    pub phases: Vec<PhaseReport>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TruthUnit {
    pub unit: u32,
    pub name: String,
    pub platform: String,
    pub lon: f64,
    pub lat: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TruthSide {
    pub side: u8,
    pub name: String,
    pub units: Vec<TruthUnit>,
    pub lost: Vec<LossLine>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TruthHitLine {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub t_ms: u64,
    pub shooter_side: u8,
    pub shooter: String,
    pub weapon: String,
    pub target_side: u8,
    pub target: String,
    /// The target was lost later in the war.
    pub destroyed: bool,
}

/// "What we thought vs what was there": a contact against its truth.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ContactVsTruth {
    pub side: u8,
    pub contact: u32,
    pub thought: String,
    pub was: String,
    /// `[lon, lat]` of our estimate and of the truth.
    #[cfg_attr(feature = "ts", ts(optional))]
    pub estimate: Option<[f64; 2]>,
    #[cfg_attr(feature = "ts", ts(optional))]
    pub truth: Option<[f64; 2]>,
    #[cfg_attr(feature = "ts", ts(optional))]
    pub error_km: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TruthReview {
    pub sides: Vec<TruthSide>,
    pub hits: Vec<TruthHitLine>,
    pub contacts: Vec<ContactVsTruth>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Report {
    pub side: u8,
    pub side_name: String,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub sim_time_ms: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub start_utc_ms: i64,
    pub war_ended: bool,
    pub totals: Totals,
    pub intel: Vec<IntelLine>,
    pub salvos: Vec<SalvoLine>,
    pub losses: Vec<LossLine>,
    pub hits: Vec<HitLine>,
    pub munitions: Vec<MunitionLine>,
    pub sorties: Vec<SortieLine>,
    pub operations: Vec<OperationReport>,
    #[cfg_attr(feature = "ts", ts(optional))]
    pub truth: Option<TruthReview>,
}

// ---------------------------------------------------------------------------
// Building
// ---------------------------------------------------------------------------

fn outcome_str(o: Outcome) -> &'static str {
    match o {
        Outcome::Hit => "hit",
        Outcome::Miss => "miss",
        Outcome::Decoyed => "decoyed",
        Outcome::Jammed => "jammed",
        Outcome::OutOfEnergy => "out of energy",
        Outcome::Intercepted => "intercepted",
        Outcome::LostTrack => "lost track",
        Outcome::SelfDestruct => "self-destruct",
    }
}

fn reason_str(r: crate::weapons::LaunchReason) -> &'static str {
    use crate::weapons::LaunchReason as L;
    match r {
        L::Test => "test",
        L::SelfDefence => "self-defence",
        L::Ordered => "ordered",
        L::WeaponsFree => "weapons free",
        L::Territory => "inside our territory",
        L::ReturnFire => "return fire",
        L::Approved => "approved",
    }
}

/// A contact as the side knows it now: `C12 (corvette)`, or just `C12`.
fn contact_label(world: &World, rules: &Rules, side: u8, c: u32) -> String {
    let pics = world.resource::<SidePictures>();
    match pics.sides[usize::from(side)].get(wb_core::ContactId(c)) {
        Some(ct) if ct.level >= wb_content::source::realism::ContactLevel::Class => {
            let n = ct.type_ref.class_name(rules);
            if n.is_empty() {
                format!("C{c}")
            } else {
                format!("C{c} ({n})")
            }
        }
        _ => format!("C{c}"),
    }
}

fn aim_label(world: &World, rules: &Rules, side: u8, a: &Option<AimRef>) -> String {
    match a {
        Some(AimRef::Contact { contact }) => contact_label(world, rules, side, contact.0),
        Some(AimRef::Point { lat_deg, lon_deg }) => format!(
            "point {:.2}°{} {:.2}°{}",
            lat_deg.abs(),
            if *lat_deg >= 0.0 { 'N' } else { 'S' },
            lon_deg.abs(),
            if *lon_deg >= 0.0 { 'E' } else { 'W' }
        ),
        None => "test target".to_owned(),
    }
}

fn unit_name(world: &World, names: &BTreeMap<UnitId, String>, u: UnitId) -> String {
    entity(world, u)
        .and_then(|e| world.get::<Identity>(e))
        .map(|i| i.name.clone())
        .or_else(|| names.get(&u).cloned())
        .unwrap_or_else(|| format!("unit {}", u.0))
}

fn platform_name(rules: &Rules, id: &str) -> String {
    rules
        .platform_by_id(id)
        .map(|p| rules.db.str(rules.platform(p).name).to_owned())
        .unwrap_or_else(|| id.to_owned())
}

/// The report of `side`. `truth`: add the truth review (only once the war
/// has ended; refused before).
pub fn build(world: &World, events: &[Event], side: u8, truth: bool) -> Result<Report, String> {
    let war = world.resource::<War>();
    if truth && war.ended.is_none() {
        return Err("The truth review unlocks when the war ends".to_owned());
    }
    let sides = world.resource::<Sides>();
    if usize::from(side) >= sides.len() {
        return Err(format!("There is no side {side}"));
    }
    let rules = world.resource::<Rules>();
    let db = &rules.db;
    let clock = world.resource::<SimClock>();
    let mine = || events.iter().filter(move |e| e.sides & (1 << side) != 0);

    // Names of own units that are gone (from their loss events).
    let mut gone_names: BTreeMap<UnitId, String> = BTreeMap::new();
    for e in mine() {
        if let EventKind::UnitDestroyed { unit, name, .. } = &e.kind {
            gone_names.insert(*unit, name.clone());
        }
    }

    let mut totals = Totals::default();
    let mut intel = Vec::new();
    let mut salvos: BTreeMap<u32, SalvoLine> = BTreeMap::new();
    let mut weapon_salvo: BTreeMap<UnitId, (u32, usize)> = BTreeMap::new();
    let mut losses = Vec::new();
    let mut hits = Vec::new();
    let mut expended: BTreeMap<String, u32> = BTreeMap::new();
    let mut expended_by_base: BTreeMap<(String, UnitId), u32> = BTreeMap::new();
    let mut sorties: Vec<SortieLine> = Vec::new();
    let mut open_sortie: BTreeMap<UnitId, usize> = BTreeMap::new();

    let base_of = |u: UnitId| {
        entity(world, u)
            .and_then(|e| world.get::<BaseRef>(e))
            .and_then(|b| b.home)
    };
    let pics = world.resource::<SidePictures>();

    for e in mine() {
        let t = e.sim_time_ms;
        match &e.kind {
            EventKind::ContactNew {
                contact,
                domain,
                source,
                ..
            } => intel.push(IntelLine {
                t_ms: t,
                kind: "new".into(),
                contact: Some(contact.0),
                source: source.as_str().to_owned(),
                text: format!("New {} contact C{}", format!("{domain:?}").to_lowercase(), contact.0),
            }),
            EventKind::ContactClassified {
                contact,
                level,
                source,
            } => intel.push(IntelLine {
                t_ms: t,
                kind: "classified".into(),
                contact: Some(contact.0),
                source: source.as_str().to_owned(),
                text: format!(
                    "{} now {}",
                    contact_label(world, rules, side, contact.0),
                    format!("{level:?}").to_lowercase()
                ),
            }),
            EventKind::ContactCount { contact, min, max } => intel.push(IntelLine {
                t_ms: t,
                kind: "count".into(),
                contact: Some(contact.0),
                source: String::new(),
                text: if min == max {
                    format!("C{}: {min} units", contact.0)
                } else {
                    format!("C{}: {min}–{max} units", contact.0)
                },
            }),
            EventKind::ContactLost { contact } => intel.push(IntelLine {
                t_ms: t,
                kind: "lost".into(),
                contact: Some(contact.0),
                source: String::new(),
                text: format!("C{} lost (last known position kept)", contact.0),
            }),
            EventKind::IntelReport {
                satellite,
                outcome,
                contacts,
                ..
            } => intel.push(IntelLine {
                t_ms: t,
                kind: "report".into(),
                contact: None,
                source: unit_name(world, &gone_names, *satellite),
                text: format!("{outcome} ({} contacts)", contacts.len()),
            }),
            EventKind::WeaponLaunched {
                weapon,
                shooter,
                weapon_type,
                weapon_name,
                salvo,
                tag,
                target,
                reason,
                intercept,
                ..
            } => {
                totals.launched += 1;
                *expended.entry(weapon_type.clone()).or_default() += 1;
                if let Some(b) = base_of(*shooter) {
                    *expended_by_base
                        .entry((weapon_type.clone(), b))
                        .or_default() += 1;
                }
                if let Some(&i) = open_sortie.get(shooter) {
                    sorties[i].launched += 1;
                }
                let line = salvos.entry(*salvo).or_insert_with(|| SalvoLine {
                    salvo: *salvo,
                    t_ms: t,
                    shooter: shooter.0,
                    shooter_name: unit_name(world, &gone_names, *shooter),
                    weapon: weapon_type.clone(),
                    weapon_name: weapon_name.clone(),
                    count: 0,
                    target: aim_label(world, rules, side, target),
                    launch_range_km: 0.0,
                    reason: reason_str(*reason).to_owned(),
                    hits: 0,
                    misses: 0,
                    intercepted: 0,
                    decoyed: 0,
                    jammed: 0,
                    other: 0,
                    in_flight: 0,
                    weapons: Vec::new(),
                    summary: String::new(),
                });
                line.count += 1;
                line.in_flight += 1;
                weapon_salvo.insert(*weapon, (*salvo, line.weapons.len()));
                line.weapons.push(WeaponLine {
                    tag: format!("{}-{tag}", if *intercept { "INT" } else { "M" }),
                    outcome: "in_flight".into(),
                    pk: None,
                    factors: String::new(),
                });
            }
            EventKind::WeaponTerminated {
                weapon,
                outcome,
                factors,
                intercept,
                ..
            } => {
                match outcome {
                    Outcome::Hit => {
                        totals.hits += 1;
                        if *intercept {
                            totals.intercepts_made += 1;
                        }
                    }
                    Outcome::Miss => totals.misses += 1,
                    Outcome::Intercepted => totals.intercepted += 1,
                    Outcome::Decoyed => totals.decoyed += 1,
                    Outcome::Jammed => totals.jammed += 1,
                    _ => totals.other += 1,
                }
                if let Some(&(s, i)) = weapon_salvo.get(weapon)
                    && let Some(line) = salvos.get_mut(&s)
                {
                    line.in_flight = line.in_flight.saturating_sub(1);
                    match outcome {
                        Outcome::Hit => line.hits += 1,
                        Outcome::Miss => line.misses += 1,
                        Outcome::Intercepted => line.intercepted += 1,
                        Outcome::Decoyed => line.decoyed += 1,
                        Outcome::Jammed => line.jammed += 1,
                        _ => line.other += 1,
                    }
                    if line.launch_range_km == 0.0 {
                        line.launch_range_km = factors.launch_range_m / 1_000.0;
                    }
                    let w = &mut line.weapons[i];
                    w.outcome = outcome_str(*outcome).to_owned();
                    w.pk = factors.endgame.map(|g| g.pk);
                    w.factors = match factors.endgame {
                        Some(g) => format!(
                            "Pk {:.2} = base {:.2} × energy {:.2} × aspect {:.2} × manoeuvre {:.2} × CM {:.2} × EW {:.2} × env {:.2}; roll {:.2}; flown {:.0} km in {:.0} s",
                            g.pk,
                            g.pk_base,
                            g.f_energy,
                            g.f_aspect,
                            g.f_maneuver,
                            g.f_cm,
                            g.f_ew,
                            g.f_env,
                            g.roll,
                            factors.flown_m / 1_000.0,
                            factors.time_of_flight_s
                        ),
                        None => format!(
                            "flown {:.0} km in {:.0} s, {:.0} m/s at the end",
                            factors.flown_m / 1_000.0,
                            factors.time_of_flight_s,
                            factors.speed_mps
                        ),
                    };
                    if *outcome == Outcome::Hit
                        && !*intercept
                        && let Some(s) = salvos.get(&s)
                    {
                        hits.push(HitLine {
                            t_ms: t,
                            target: s.target.clone(),
                            weapon_name: s.weapon_name.clone(),
                            shooter_name: s.shooter_name.clone(),
                            gone: false,
                        });
                    }
                }
            }
            EventKind::UnitDestroyed {
                unit,
                name,
                platform,
                cause,
            } => {
                totals.losses += 1;
                losses.push(LossLine {
                    t_ms: t,
                    unit: unit.0,
                    name: name.clone(),
                    platform: platform
                        .as_deref()
                        .map_or_else(String::new, |p| platform_name(rules, p)),
                    cause: cause.clone(),
                });
                open_sortie.remove(unit);
            }
            EventKind::TookOff { unit, base } => {
                let loadout = entity(world, *unit)
                    .and_then(|e| world.get::<Magazine>(e))
                    .and_then(|m| m.loadout)
                    .map_or_else(
                        || "custom or none".to_owned(),
                        |l| db.str(db.loadouts[l.0 as usize].name).to_owned(),
                    );
                open_sortie.insert(*unit, sorties.len());
                sorties.push(SortieLine {
                    unit: unit.0,
                    name: unit_name(world, &gone_names, *unit),
                    base: unit_name(world, &gone_names, *base),
                    took_off_ms: t,
                    landed_ms: None,
                    landed_at: None,
                    fuel_at_landing_kg: None,
                    refuels: 0,
                    bingo: false,
                    launched: 0,
                    loadout,
                });
            }
            EventKind::Landed {
                unit,
                base,
                fuel_kg,
            } => {
                if let Some(i) = open_sortie.remove(unit) {
                    let s = &mut sorties[i];
                    s.landed_ms = Some(t);
                    s.landed_at = Some(unit_name(world, &gone_names, *base));
                    s.fuel_at_landing_kg = Some(*fuel_kg);
                }
            }
            EventKind::Refuelled { unit, .. } => {
                if let Some(&i) = open_sortie.get(unit) {
                    sorties[i].refuels += 1;
                }
            }
            EventKind::BingoRtb { unit, .. } => {
                if let Some(&i) = open_sortie.get(unit) {
                    sorties[i].bingo = true;
                }
            }
            _ => {}
        }
    }
    for h in &mut hits {
        // `C12 (…)`: gone once the contact has left our picture.
        if let Some(n) = h
            .target
            .strip_prefix('C')
            .and_then(|r| r.split(' ').next())
            .and_then(|n| n.parse::<u32>().ok())
        {
            h.gone = pics.sides[usize::from(side)]
                .get(wb_core::ContactId(n))
                .is_none();
        }
    }
    totals.in_flight = salvos.values().map(|s| s.in_flight).sum();
    let mut salvos: Vec<SalvoLine> = salvos.into_values().collect();
    for s in &mut salvos {
        s.summary = salvo_summary(s);
    }

    // Munitions: expended (events) and what is left on own units and bases.
    let mut on_units: BTreeMap<String, u32> = BTreeMap::new();
    let mut in_stock: BTreeMap<String, u32> = BTreeMap::new();
    let mut stock_by_base: BTreeMap<(String, UnitId), u32> = BTreeMap::new();
    let mut base_names: BTreeMap<UnitId, String> = BTreeMap::new();
    for (&uid, &e) in &world.resource::<UnitIndex>().0 {
        if world.get::<Allegiance>(e).map(|a| a.side) != Some(side)
            || world.get::<WeaponFlight>(e).is_some()
        {
            continue;
        }
        if let Some(m) = world.get::<Magazine>(e) {
            for (w, n) in m.rounds() {
                *on_units
                    .entry(db.str(db.weapons[w.0 as usize].id).to_owned())
                    .or_default() += n;
            }
        }
        if let Some(b) = world.get::<BaseSite>(e) {
            base_names.insert(uid, unit_name(world, &gone_names, uid));
            for (w, n) in &b.stock {
                let id = db.str(db.weapons[w.0 as usize].id).to_owned();
                *in_stock.entry(id.clone()).or_default() += n;
                *stock_by_base.entry((id, uid)).or_default() += n;
            }
        }
    }
    let mut ids: Vec<String> = expended
        .keys()
        .chain(on_units.keys())
        .chain(in_stock.keys())
        .cloned()
        .collect();
    ids.sort();
    ids.dedup();
    let munitions = ids
        .into_iter()
        .map(|id| {
            let name = db
                .weapons
                .iter()
                .find(|w| db.str(w.id) == id)
                .map_or_else(|| id.clone(), |w| db.str(w.name).to_owned());
            let mut by_base: BTreeMap<UnitId, (u32, u32)> = BTreeMap::new();
            for ((w, b), n) in &expended_by_base {
                if *w == id {
                    by_base.entry(*b).or_default().0 += n;
                }
            }
            for ((w, b), n) in &stock_by_base {
                if *w == id {
                    by_base.entry(*b).or_default().1 += n;
                }
            }
            MunitionLine {
                expended: expended.get(&id).copied().unwrap_or(0),
                on_units: on_units.get(&id).copied().unwrap_or(0),
                in_stock: in_stock.get(&id).copied().unwrap_or(0),
                by_base: by_base
                    .into_iter()
                    .map(|(b, (x, s))| {
                        (
                            base_names
                                .get(&b)
                                .cloned()
                                .unwrap_or_else(|| unit_name(world, &gone_names, b)),
                            x,
                            s,
                        )
                    })
                    .collect(),
                weapon: id,
                name,
            }
        })
        .collect();

    let operations = world
        .resource::<Operations>()
        .ops
        .iter()
        .filter(|o| o.side == side)
        .map(|o| {
            let v = crate::operations::view(world, o);
            OperationReport {
                id: o.id,
                name: o.name.clone(),
                state: o.state,
                started_ms: o.started.map(|s| s.ms),
                ended_ms: o.ended.map(|s| s.ms),
                phases: o
                    .phases
                    .iter()
                    .zip(v.phases)
                    .map(|(p, pv)| PhaseReport {
                        name: p.name.clone(),
                        state: p.state,
                        started_ms: p.started.map(|s| s.ms),
                        ended_ms: p.ended.map(|s| s.ms),
                        reason: p.reason.clone().unwrap_or_default(),
                        tasks: pv
                            .tasks
                            .iter()
                            .map(|t| format!("{} · {}", t.kind.replace('_', " "), t.unit_names.join(", ")))
                            .collect(),
                        tally: p.tally,
                    })
                    .collect(),
            }
        })
        .collect();

    let truth = truth.then(|| truth_review(world, events));
    Ok(Report {
        side,
        side_name: sides.sides[usize::from(side)].name.clone(),
        sim_time_ms: clock.sim_time_ms,
        start_utc_ms: clock.start_utc_ms,
        war_ended: war.ended.is_some(),
        totals,
        intel,
        salvos,
        losses,
        hits,
        munitions,
        sorties,
        operations,
        truth,
    })
}

/// `Salvo S-17 · Frigate · 3 × Test ASM → C4 (corvette) at 142 km · 1 intercepted, 1 decoyed, 1 hit`.
fn salvo_summary(s: &SalvoLine) -> String {
    let mut parts = Vec::new();
    for (n, what) in [
        (s.intercepted, "intercepted"),
        (s.decoyed, "decoyed"),
        (s.jammed, "jammed"),
        (s.misses, "missed"),
        (s.other, "lost (energy, track or self-destruct)"),
        (s.hits, if s.hits == 1 { "hit" } else { "hits" }),
        (s.in_flight, "in flight"),
    ] {
        if n > 0 {
            parts.push(format!("{n} {what}"));
        }
    }
    format!(
        "Salvo S-{} · {} · {} × {} → {}{} · {}",
        s.salvo,
        s.shooter_name,
        s.count,
        s.weapon_name,
        s.target,
        if s.launch_range_km > 0.0 {
            format!(" at {:.0} km", s.launch_range_km)
        } else {
            String::new()
        },
        parts.join(", ")
    )
}

fn truth_review(world: &World, events: &[Event]) -> TruthReview {
    let rules = world.resource::<Rules>();
    let sides = world.resource::<Sides>();
    let lead_pos = |u: UnitId| -> Option<wb_core::geo::LatLon> {
        let e = entity(world, u)?;
        let e = match world.get::<Group>(e) {
            Some(g) => entity(world, *g.members.first()?)?,
            None => e,
        };
        world.get::<Kinematics>(e).map(|k| k.pos())
    };
    let mut out = Vec::new();
    for (s, info) in sides.sides.iter().enumerate() {
        let s = s as u8;
        let mut units = Vec::new();
        for (&uid, &e) in &world.resource::<UnitIndex>().0 {
            if world.get::<Allegiance>(e).map(|a| a.side) != Some(s)
                || world.get::<GroupMember>(e).is_none()
            {
                continue;
            }
            let (Some(p), Some(k)) = (world.get::<PlatformRef>(e), world.get::<Kinematics>(e))
            else {
                continue;
            };
            units.push(TruthUnit {
                unit: uid.0,
                name: world
                    .get::<Identity>(e)
                    .map_or_else(String::new, |i| i.name.clone()),
                platform: rules.db.str(rules.platform(p.0).name).to_owned(),
                lon: k.pos().lon_deg(),
                lat: k.pos().lat_deg(),
            });
        }
        let lost = events
            .iter()
            .filter(|e| e.sides & (1 << s) != 0)
            .filter_map(|e| match &e.kind {
                EventKind::UnitDestroyed {
                    unit,
                    name,
                    platform,
                    cause,
                } => Some(LossLine {
                    t_ms: e.sim_time_ms,
                    unit: unit.0,
                    name: name.clone(),
                    platform: platform
                        .as_deref()
                        .map_or_else(String::new, |p| platform_name(rules, p)),
                    cause: cause.clone(),
                }),
                _ => None,
            })
            .collect();
        out.push(TruthSide {
            side: s,
            name: info.name.clone(),
            units,
            lost,
        });
    }
    let destroyed: std::collections::BTreeSet<UnitId> = events
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::UnitDestroyed { unit, .. } => Some(*unit),
            _ => None,
        })
        .collect();
    let names: BTreeMap<UnitId, String> = events
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::UnitDestroyed { unit, name, .. } => Some((*unit, name.clone())),
            _ => None,
        })
        .collect();
    let hits = events
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::TruthHit {
                weapon_type,
                shooter,
                shooter_side,
                target,
                target_side,
                target_name,
                ..
            } => Some(TruthHitLine {
                t_ms: e.sim_time_ms,
                shooter_side: *shooter_side,
                shooter: unit_name(world, &names, *shooter),
                weapon: rules
                    .db
                    .weapons
                    .iter()
                    .find(|w| rules.db.str(w.id) == weapon_type)
                    .map_or_else(|| weapon_type.clone(), |w| rules.db.str(w.name).to_owned()),
                target_side: *target_side,
                target: target_name.clone(),
                destroyed: destroyed.contains(target),
            }),
            _ => None,
        })
        .collect();
    let pics = world.resource::<SidePictures>();
    let mut contacts = Vec::new();
    for (s, pic) in pics.sides.iter().enumerate() {
        for c in pic.contacts.values() {
            if matches!(c.type_ref, TypeRef::Weapon(_)) {
                continue;
            }
            let thought = if c.level >= wb_content::source::realism::ContactLevel::Class {
                c.type_ref.class_name(rules)
            } else {
                format!("{:?} contact", c.domain).to_lowercase()
            };
            let was = match c.type_ref {
                TypeRef::Platform(p) => rules.db.str(rules.platform(p).name).to_owned(),
                TypeRef::System(x) => rules.db.str(rules.db.systems[x.0 as usize].name).to_owned(),
                TypeRef::Weapon(w) => rules.db.str(rules.db.weapons[w.0 as usize].name).to_owned(),
            };
            let truth = lead_pos(c.truth);
            let est = c.est.map(|e| e.at);
            contacts.push(ContactVsTruth {
                side: s as u8,
                contact: c.id.0,
                thought,
                was,
                estimate: est.map(|p| [p.lon_deg(), p.lat_deg()]),
                truth: truth.map(|p| [p.lon_deg(), p.lat_deg()]),
                error_km: est
                    .zip(truth)
                    .map(|(a, b)| wb_core::geo::distance_m(a, b) / 1_000.0),
            });
        }
    }
    TruthReview {
        sides: out,
        hits,
        contacts,
    }
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

/// The CSV tables a report exports to.
pub const CSV_TABLES: &[&str] = &[
    "intel",
    "salvos",
    "weapons",
    "losses",
    "hits",
    "munitions",
    "sorties",
    "operations",
    "truth_hits",
    "truth_contacts",
];

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_owned()
    }
}

fn csv_rows(header: &[&str], rows: Vec<Vec<String>>) -> String {
    let mut out = header.join(",");
    out.push('\n');
    for r in rows {
        out.push_str(&r.iter().map(|f| csv_field(f)).collect::<Vec<_>>().join(","));
        out.push('\n');
    }
    out
}

fn opt<T: ToString>(v: Option<T>) -> String {
    v.map_or_else(String::new, |x| x.to_string())
}

/// One table as CSV; `None` for an unknown table (or truth tables before the
/// war ends).
pub fn to_csv(r: &Report, table: &str) -> Option<String> {
    Some(match table {
        "intel" => csv_rows(
            &["t", "kind", "contact", "source", "text"],
            r.intel
                .iter()
                .map(|l| {
                    vec![
                        fmt_t(l.t_ms),
                        l.kind.clone(),
                        opt(l.contact),
                        l.source.clone(),
                        l.text.clone(),
                    ]
                })
                .collect(),
        ),
        "salvos" => csv_rows(
            &[
                "salvo", "t", "shooter", "weapon", "count", "target", "launch_range_km", "reason",
                "hits", "misses", "intercepted", "decoyed", "jammed", "other", "in_flight",
            ],
            r.salvos
                .iter()
                .map(|s| {
                    vec![
                        s.salvo.to_string(),
                        fmt_t(s.t_ms),
                        s.shooter_name.clone(),
                        s.weapon_name.clone(),
                        s.count.to_string(),
                        s.target.clone(),
                        format!("{:.1}", s.launch_range_km),
                        s.reason.clone(),
                        s.hits.to_string(),
                        s.misses.to_string(),
                        s.intercepted.to_string(),
                        s.decoyed.to_string(),
                        s.jammed.to_string(),
                        s.other.to_string(),
                        s.in_flight.to_string(),
                    ]
                })
                .collect(),
        ),
        "weapons" => csv_rows(
            &["salvo", "tag", "outcome", "pk", "factors"],
            r.salvos
                .iter()
                .flat_map(|s| {
                    s.weapons.iter().map(move |w| {
                        vec![
                            s.salvo.to_string(),
                            w.tag.clone(),
                            w.outcome.clone(),
                            w.pk.map_or_else(String::new, |p| format!("{p:.3}")),
                            w.factors.clone(),
                        ]
                    })
                })
                .collect(),
        ),
        "losses" => csv_rows(
            &["t", "unit", "name", "platform", "cause"],
            r.losses
                .iter()
                .map(|l| {
                    vec![
                        fmt_t(l.t_ms),
                        l.unit.to_string(),
                        l.name.clone(),
                        l.platform.clone(),
                        l.cause.clone(),
                    ]
                })
                .collect(),
        ),
        "hits" => csv_rows(
            &["t", "target", "weapon", "shooter", "contact_gone"],
            r.hits
                .iter()
                .map(|h| {
                    vec![
                        fmt_t(h.t_ms),
                        h.target.clone(),
                        h.weapon_name.clone(),
                        h.shooter_name.clone(),
                        h.gone.to_string(),
                    ]
                })
                .collect(),
        ),
        "munitions" => csv_rows(
            &["weapon", "name", "expended", "on_units", "in_stock", "by_base"],
            r.munitions
                .iter()
                .map(|m| {
                    vec![
                        m.weapon.clone(),
                        m.name.clone(),
                        m.expended.to_string(),
                        m.on_units.to_string(),
                        m.in_stock.to_string(),
                        m.by_base
                            .iter()
                            .map(|(b, x, s)| format!("{b}: {x} expended, {s} in stock"))
                            .collect::<Vec<_>>()
                            .join("; "),
                    ]
                })
                .collect(),
        ),
        "sorties" => csv_rows(
            &[
                "unit", "name", "base", "took_off", "landed", "landed_at", "fuel_at_landing_kg",
                "refuels", "bingo", "launched", "loadout",
            ],
            r.sorties
                .iter()
                .map(|s| {
                    vec![
                        s.unit.to_string(),
                        s.name.clone(),
                        s.base.clone(),
                        fmt_t(s.took_off_ms),
                        s.landed_ms.map_or_else(String::new, fmt_t),
                        opt(s.landed_at.clone()),
                        s.fuel_at_landing_kg
                            .map_or_else(String::new, |f| format!("{f:.0}")),
                        s.refuels.to_string(),
                        s.bingo.to_string(),
                        s.launched.to_string(),
                        s.loadout.clone(),
                    ]
                })
                .collect(),
        ),
        "operations" => csv_rows(
            &[
                "operation", "phase", "state", "started", "ended", "reason", "tasks", "launched",
                "hits", "intercepted", "decoyed", "lost",
            ],
            r.operations
                .iter()
                .flat_map(|o| {
                    o.phases.iter().map(move |p| {
                        vec![
                            o.name.clone(),
                            p.name.clone(),
                            format!("{:?}", p.state).to_lowercase(),
                            p.started_ms.map_or_else(String::new, fmt_t),
                            p.ended_ms.map_or_else(String::new, fmt_t),
                            p.reason.clone(),
                            p.tasks.join("; "),
                            p.tally.launched.to_string(),
                            p.tally.hits.to_string(),
                            p.tally.intercepted.to_string(),
                            p.tally.decoyed.to_string(),
                            p.tally.lost.to_string(),
                        ]
                    })
                })
                .collect(),
        ),
        "truth_hits" => {
            let t = r.truth.as_ref()?;
            csv_rows(
                &["t", "shooter_side", "shooter", "weapon", "target_side", "target", "destroyed"],
                t.hits
                    .iter()
                    .map(|h| {
                        vec![
                            fmt_t(h.t_ms),
                            h.shooter_side.to_string(),
                            h.shooter.clone(),
                            h.weapon.clone(),
                            h.target_side.to_string(),
                            h.target.clone(),
                            h.destroyed.to_string(),
                        ]
                    })
                    .collect(),
            )
        }
        "truth_contacts" => {
            let t = r.truth.as_ref()?;
            csv_rows(
                &["side", "contact", "thought", "was", "error_km"],
                t.contacts
                    .iter()
                    .map(|c| {
                        vec![
                            c.side.to_string(),
                            c.contact.to_string(),
                            c.thought.clone(),
                            c.was.clone(),
                            c.error_km.map_or_else(String::new, |e| format!("{e:.2}")),
                        ]
                    })
                    .collect(),
            )
        }
        _ => return None,
    })
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn table(out: &mut String, title: &str, header: &[&str], rows: &[Vec<String>]) {
    let _ = write!(out, "<h2>{}</h2>", esc(title));
    if rows.is_empty() {
        out.push_str("<p class=\"none\">None.</p>");
        return;
    }
    out.push_str("<table><thead><tr>");
    for h in header {
        let _ = write!(out, "<th>{}</th>", esc(h));
    }
    out.push_str("</tr></thead><tbody>");
    for r in rows {
        out.push_str("<tr>");
        for f in r {
            let _ = write!(out, "<td>{}</td>", esc(f));
        }
        out.push_str("</tr>");
    }
    out.push_str("</tbody></table>");
}

/// A self-contained printable HTML page of the report.
pub fn to_html(r: &Report) -> String {
    let mut o = String::new();
    let _ = write!(
        o,
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>After-action report · {}</title><style>{}</style></head><body>",
        esc(&r.side_name),
        HTML_STYLE
    );
    let _ = write!(
        o,
        "<h1>After-action report · {}</h1><p class=\"meta\">Sim time T+{} · {}</p>",
        esc(&r.side_name),
        fmt_t(r.sim_time_ms),
        if r.war_ended {
            "war ended"
        } else {
            "war in progress: our side's knowledge only"
        }
    );
    let t = &r.totals;
    let _ = write!(
        o,
        "<div class=\"totals\"><div><b>{}</b>launched</div><div><b>{}</b>hits</div><div><b>{}</b>intercepted</div><div><b>{}</b>decoyed</div><div><b>{}</b>interceptions made</div><div><b>{}</b>losses</div></div>",
        t.launched, t.hits, t.intercepted, t.decoyed, t.intercepts_made, t.losses
    );
    for op in &r.operations {
        let rows: Vec<Vec<String>> = op
            .phases
            .iter()
            .map(|p| {
                vec![
                    p.name.clone(),
                    format!("{:?}", p.state).to_lowercase(),
                    p.started_ms.map_or_else(|| "—".into(), |m| format!("T+{}", fmt_t(m))),
                    p.ended_ms.map_or_else(|| "—".into(), |m| format!("T+{}", fmt_t(m))),
                    p.reason.clone(),
                    p.tasks.join("; "),
                    format!(
                        "{} launched, {} hit, {} lost",
                        p.tally.launched, p.tally.hits, p.tally.lost
                    ),
                ]
            })
            .collect();
        table(
            &mut o,
            &format!("Operation {} ({})", op.name, format!("{:?}", op.state).to_lowercase()),
            &["Phase", "State", "Start", "End", "Why it ended", "Tasks", "Results"],
            &rows,
        );
    }
    let rows: Vec<Vec<String>> = r
        .salvos
        .iter()
        .map(|s| vec![format!("T+{}", fmt_t(s.t_ms)), s.summary.clone(), s.reason.clone()])
        .collect();
    table(&mut o, "Salvo reports", &["Time", "Salvo", "Why fired"], &rows);
    let rows: Vec<Vec<String>> = r
        .losses
        .iter()
        .map(|l| {
            vec![
                format!("T+{}", fmt_t(l.t_ms)),
                l.name.clone(),
                l.platform.clone(),
                l.cause.clone(),
            ]
        })
        .collect();
    table(&mut o, "Losses", &["Time", "Unit", "Type", "Cause"], &rows);
    let rows: Vec<Vec<String>> = r
        .hits
        .iter()
        .map(|h| {
            vec![
                format!("T+{}", fmt_t(h.t_ms)),
                h.target.clone(),
                h.weapon_name.clone(),
                h.shooter_name.clone(),
                if h.gone { "left our picture" } else { "still tracked" }.to_owned(),
            ]
        })
        .collect();
    table(
        &mut o,
        "Our hits",
        &["Time", "Target", "Weapon", "Shooter", "Contact"],
        &rows,
    );
    let rows: Vec<Vec<String>> = r
        .munitions
        .iter()
        .map(|m| {
            vec![
                m.name.clone(),
                m.expended.to_string(),
                m.on_units.to_string(),
                m.in_stock.to_string(),
            ]
        })
        .collect();
    table(
        &mut o,
        "Munitions",
        &["Weapon", "Expended", "On units", "In stock"],
        &rows,
    );
    let rows: Vec<Vec<String>> = r
        .sorties
        .iter()
        .map(|s| {
            vec![
                s.name.clone(),
                s.base.clone(),
                format!("T+{}", fmt_t(s.took_off_ms)),
                s.landed_ms
                    .map_or_else(|| "airborne".into(), |m| format!("T+{}", fmt_t(m))),
                s.fuel_at_landing_kg
                    .map_or_else(String::new, |f| format!("{f:.0} kg")),
                s.refuels.to_string(),
                s.launched.to_string(),
                s.loadout.clone(),
            ]
        })
        .collect();
    table(
        &mut o,
        "Sorties",
        &["Unit", "Base", "Took off", "Landed", "Fuel left", "Refuels", "Fired", "Loadout"],
        &rows,
    );
    let rows: Vec<Vec<String>> = r
        .intel
        .iter()
        .map(|l| {
            vec![
                format!("T+{}", fmt_t(l.t_ms)),
                l.source.clone(),
                l.text.clone(),
            ]
        })
        .collect();
    table(&mut o, "Intel log", &["Time", "Source", "Entry"], &rows);
    if let Some(tr) = &r.truth {
        o.push_str("<h1 class=\"truth\">Truth review</h1>");
        let rows: Vec<Vec<String>> = tr
            .contacts
            .iter()
            .map(|c| {
                vec![
                    format!("Side {}", c.side + 1),
                    format!("C{}", c.contact),
                    c.thought.clone(),
                    c.was.clone(),
                    c.error_km.map_or_else(String::new, |e| format!("{e:.1} km")),
                ]
            })
            .collect();
        table(
            &mut o,
            "What we thought vs what was there",
            &["Side", "Contact", "We thought", "It was", "Position error"],
            &rows,
        );
        let rows: Vec<Vec<String>> = tr
            .hits
            .iter()
            .map(|h| {
                vec![
                    format!("T+{}", fmt_t(h.t_ms)),
                    h.shooter.clone(),
                    h.weapon.clone(),
                    h.target.clone(),
                    if h.destroyed { "lost" } else { "survived" }.to_owned(),
                ]
            })
            .collect();
        table(
            &mut o,
            "Every hit",
            &["Time", "Shooter", "Weapon", "Target", "Target"],
            &rows,
        );
        for s in &tr.sides {
            let rows: Vec<Vec<String>> = s
                .units
                .iter()
                .map(|u| vec![u.name.clone(), u.platform.clone(), format!("{:.3}, {:.3}", u.lat, u.lon)])
                .collect();
            table(
                &mut o,
                &format!("{} at the end ({} lost)", s.name, s.lost.len()),
                &["Unit", "Type", "Position"],
                &rows,
            );
        }
    }
    o.push_str("</body></html>");
    o
}

const HTML_STYLE: &str = "body{font:12px/1.45 system-ui,sans-serif;color:#111;margin:24px;max-width:1100px}\
h1{font-size:20px;margin:0 0 4px}h1.truth{margin-top:32px;border-top:2px solid #111;padding-top:16px}\
h2{font-size:14px;margin:20px 0 6px}.meta{color:#555;margin:0 0 12px}\
.totals{display:flex;flex-wrap:wrap;gap:8px}.totals div{border:1px solid #bbb;border-radius:6px;padding:6px 10px;min-width:90px}\
.totals b{display:block;font-size:18px}table{border-collapse:collapse;width:100%}\
th,td{border-bottom:1px solid #ddd;text-align:left;padding:3px 6px;vertical-align:top}\
th{background:#f2f2f2}.none{color:#777}@media print{body{margin:0}h2{break-after:avoid}tr{break-inside:avoid}}";
