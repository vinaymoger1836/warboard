//! Operations (doc 03 §8, ADR-013): a graph of phases. Each phase has tasks,
//! a start trigger, an end condition and optional abort conditions; phases
//! whose triggers hold together start on the same tick.
//!
//! Triggers, ends and aborts share one condition tree ([`CondSpec`] on the
//! wire, [`Cond`] once checked): `all` / `any` of leaves. Leaves read the
//! operation's own state, the clock, the side's picture and the side's own
//! events, never the truth (CLAUDE.md rule 1), so the AI can run the same
//! objects (P12).
//!
//! The `operations` stage runs right after `orders`. For each running
//! operation in id order it repeats, until nothing changes: start every
//! waiting phase whose trigger holds, end or abort every running phase whose
//! condition holds, cancel waiting phases that can no longer start. So "with
//! Phase 4" starts on the tick Phase 4 does (G13).
//!
//! Phase tasks are ordinary tasks given to the units when the phase starts,
//! or fire missions (ADR-043) that leave the group's task alone. A strike
//! planned on a point may name a radius: at phase start it is aimed at the
//! side's nearest non-friendly contact within it (doc 03 §7: the operation's
//! approval covers that designated target only).

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use wb_content::source::Domain;
use wb_content::source::realism::ContactLevel;
use wb_core::geo::{LatLon, distance_m};
use wb_core::units::km_to_m;
use wb_core::{ContactId, UnitId};
use wb_geo::Geo;

use crate::clock::SimClock;
use crate::components::{
    Allegiance, FireMission, Fuel, Group, GroupMember, Identity, Kinematics, Magazine, Mode,
    PlatformRef, Posture, Roe, StrikeTarget, Task,
};
use crate::events::{EventKind, EventLog, Outcome, emit};
use crate::orders::{self, LonLat, TaskOrder};
use crate::picture::{ContactIdentity, SidePictures, TypeRef};
use crate::rules::Rules;
use crate::world::entity;

/// Most phases in one operation.
pub const MAX_PHASES: usize = 32;
/// Most tasks in one phase.
pub const MAX_TASKS: usize = 16;

// ---------------------------------------------------------------------------
// Wire form (planner, scenarios, orders)
// ---------------------------------------------------------------------------

/// An operation as the planner sends it (`CreateOperation`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(deny_unknown_fields)]
pub struct OperationSpec {
    pub name: String,
    pub phases: Vec<PhaseSpec>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(deny_unknown_fields)]
pub struct PhaseSpec {
    /// Short id other phases' conditions refer to (`p1`, `take-station`).
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub tasks: Vec<PhaseTaskSpec>,
    pub trigger: CondSpec,
    /// `tasks_done` when absent.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub end: Option<CondSpec>,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub abort: Option<CondSpec>,
}

/// Units and what they do when the phase starts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(deny_unknown_fields)]
pub struct PhaseTaskSpec {
    /// Groups or members of the side.
    pub units: Vec<u32>,
    pub task: TaskOrder,
    /// Strike or suppression as a fire mission (ADR-043): fire from where
    /// the units are and keep their task.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub fire: Option<bool>,
    /// A strike on a point is aimed, at phase start, at the side's nearest
    /// non-friendly contact within this radius of the point.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub near_km: Option<f64>,
}

/// A circle, `[lon, lat]` degrees.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct AreaSpec {
    pub centre: LonLat,
    pub radius_km: f64,
}

/// A trigger, end or abort condition (doc 03 §8).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CondSpec {
    All {
        of: Vec<CondSpec>,
    },
    Any {
        of: Vec<CondSpec>,
    },
    /// H + delay: the operation started `delay_min` ago.
    Start {
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        delay_min: Option<f64>,
    },
    /// A UTC time, `YYYY-MM-DDTHH:MM:SSZ`.
    AtTime {
        utc: String,
    },
    /// "With phase X" (+ delay).
    PhaseStarted {
        phase: String,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        delay_min: Option<f64>,
    },
    /// "After phase X completes" (+ delay); an aborted phase never completes.
    PhaseEnded {
        phase: String,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        delay_min: Option<f64>,
    },
    /// The player presses Go (trigger) or End (end condition).
    Manual,
    /// The side's picture holds at least `count` contacts (default 1) of the
    /// domain, at `level` or better (default class), in the area. Weapons
    /// and friendly contacts do not count.
    Contacts {
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        domain: Option<Domain>,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        level: Option<ContactLevel>,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        count: Option<u32>,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        area: Option<AreaSpec>,
    },
    /// A satellite report reached the side since the operation started.
    IntelReport,
    /// Phase only: every group of the phase is on station or at its last
    /// waypoint (or home).
    Arrived,
    /// Phase only: every task of the phase is done.
    TasksDone,
    /// Phase only: every strike, suppression and fire mission has fired out
    /// and its weapons have ended.
    WeaponsDone,
    /// Phase only: every designated target has left the side's picture.
    TargetsDestroyed,
    /// Phase only: time since the phase started (a time limit).
    Elapsed {
        min: f64,
    },
    /// Phase only: share of the phase's units lost, percent.
    LossesPct {
        pct: f64,
    },
    /// Phase only: a member of the phase called bingo.
    Bingo,
    /// Phase only: a weapon was seen coming at a member of the phase.
    UnderAttack,
    /// Phase only: a member's fuel is below `pct` percent.
    FuelBelow {
        pct: f64,
    },
}

// ---------------------------------------------------------------------------
// Engine form
// ---------------------------------------------------------------------------

/// A checked condition; phases by index, times in sim ms.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Cond {
    All(Vec<Cond>),
    Any(Vec<Cond>),
    Start {
        delay_ms: u64,
    },
    AtTime {
        at_ms: u64,
    },
    PhaseStarted {
        phase: u32,
        delay_ms: u64,
    },
    PhaseEnded {
        phase: u32,
        delay_ms: u64,
    },
    Manual,
    Contacts {
        domain: Option<Domain>,
        level: ContactLevel,
        count: u32,
        area: Option<(LatLon, f64)>,
    },
    IntelReport,
    Arrived,
    TasksDone,
    WeaponsDone,
    TargetsDestroyed,
    Elapsed {
        ms: u64,
    },
    LossesPct {
        fraction: f64,
    },
    Bingo,
    UnderAttack,
    FuelBelow {
        fraction: f64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum OpState {
    Planned,
    Running,
    Paused,
    Completed,
    Aborted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum PhaseState {
    Waiting,
    Running,
    /// Ended by its end condition (or the player).
    Done,
    /// Ended by an abort condition or the operation's abort.
    Aborted,
    /// Never started: its trigger can no longer hold, or the operation ended.
    Cancelled,
}

impl PhaseState {
    pub fn is_final(self) -> bool {
        matches!(
            self,
            PhaseState::Done | PhaseState::Aborted | PhaseState::Cancelled
        )
    }
}

/// When something happened: tick and sim ms.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Stamp {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub tick: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub ms: u64,
}

/// A phase task in engine form; the run-time fields fill in at phase start.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhaseTask {
    pub units: Vec<u32>,
    pub task: Task,
    pub fire: bool,
    pub near_m: Option<f64>,
    /// Groups the task went to.
    pub groups: Vec<UnitId>,
    /// `(group, mission id)` of fire missions.
    pub missions: Vec<(UnitId, u32)>,
    /// The contact a strike was aimed at.
    pub target: Option<ContactId>,
    /// A group of the task reported `StrikeAborted`.
    pub aborted: bool,
}

/// Weapon results of a phase, from its side's events.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PhaseTally {
    pub launched: u32,
    pub hits: u32,
    pub misses: u32,
    pub intercepted: u32,
    pub decoyed: u32,
    pub jammed: u32,
    pub other: u32,
    pub lost: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Phase {
    pub key: String,
    pub name: String,
    pub tasks: Vec<PhaseTask>,
    pub trigger: Cond,
    pub end: Cond,
    pub abort: Option<Cond>,
    pub state: PhaseState,
    /// The player pressed Go / End.
    pub go: bool,
    pub end_now: bool,
    pub started: Option<Stamp>,
    pub ended: Option<Stamp>,
    pub reason: Option<String>,
    /// Members at phase start (losses are counted against them).
    pub members: Vec<UnitId>,
    pub lost: Vec<UnitId>,
    /// Weapons of the phase still flying.
    pub in_flight: Vec<UnitId>,
    pub tally: PhaseTally,
    pub bingo: bool,
    pub attacked: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Operation {
    /// The `CreateOperation` order's sequence number.
    pub id: u32,
    pub side: u8,
    pub name: String,
    pub state: OpState,
    pub started: Option<Stamp>,
    pub ended: Option<Stamp>,
    pub reason: Option<String>,
    pub phases: Vec<Phase>,
    /// Satellite reports since the start.
    pub intel_reports: u32,
}

/// Every operation of every side. Saved and hashed.
#[derive(Resource, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Operations {
    pub ops: Vec<Operation>,
    pub next_mission: u32,
    /// Events already read into the phases' tallies.
    pub events_seen: u64,
}

impl Operations {
    pub fn get(&self, id: u32) -> Option<&Operation> {
        self.ops.iter().find(|o| o.id == id)
    }
}

/// An operation command, checked (`orders::Resolved::Operation`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum OpCommand {
    /// `replace`: a planned operation this one replaces.
    Create {
        op: Box<Operation>,
        replace: Option<u32>,
        start: bool,
    },
    Start {
        op: u32,
    },
    Pause {
        op: u32,
    },
    Resume {
        op: u32,
    },
    Abort {
        op: u32,
    },
    Go {
        op: u32,
        phase: u32,
    },
    EndPhase {
        op: u32,
        phase: u32,
    },
}

// ---------------------------------------------------------------------------
// Checking (CreateOperation and the planner)
// ---------------------------------------------------------------------------

fn minutes(v: Option<f64>, what: &str) -> Result<u64, String> {
    let m = v.unwrap_or(0.0);
    if !m.is_finite() || !(0.0..=10_080.0).contains(&m) {
        return Err(format!("{what}: delay must be 0–10080 minutes"));
    }
    Ok((m * 60_000.0).round() as u64)
}

/// Phase-relative leaves only make sense once the phase runs.
fn phase_only(c: &CondSpec) -> Option<&'static str> {
    match c {
        CondSpec::Arrived => Some("arrived"),
        CondSpec::TasksDone => Some("tasks done"),
        CondSpec::WeaponsDone => Some("weapons done"),
        CondSpec::TargetsDestroyed => Some("targets destroyed"),
        CondSpec::Elapsed { .. } => Some("time limit"),
        CondSpec::LossesPct { .. } => Some("losses"),
        CondSpec::Bingo => Some("bingo"),
        CondSpec::UnderAttack => Some("under attack"),
        CondSpec::FuelBelow { .. } => Some("fuel state"),
        _ => None,
    }
}

fn resolve_cond(
    c: &CondSpec,
    keys: &[String],
    start_utc_ms: i64,
    trigger: bool,
    depth: u32,
) -> Result<Cond, String> {
    if depth > 8 {
        return Err("Conditions nest at most 8 deep".to_owned());
    }
    if trigger && let Some(what) = phase_only(c) {
        return Err(format!(
            "`{what}` can end or abort a phase, not start one"
        ));
    }
    let phase = |id: &str| -> Result<u32, String> {
        keys.iter()
            .position(|k| k == id)
            .map(|i| i as u32)
            .ok_or_else(|| format!("No phase `{id}` in this operation"))
    };
    let pct = |v: f64| -> Result<f64, String> {
        if !v.is_finite() || !(0.0..=100.0).contains(&v) {
            return Err(format!("{v} is not a percentage (0–100)"));
        }
        Ok(v / 100.0)
    };
    Ok(match c {
        CondSpec::All { of } | CondSpec::Any { of } => {
            if of.is_empty() {
                return Err("`all` and `any` need at least one condition".to_owned());
            }
            let v = of
                .iter()
                .map(|x| resolve_cond(x, keys, start_utc_ms, trigger, depth + 1))
                .collect::<Result<Vec<_>, _>>()?;
            if matches!(c, CondSpec::All { .. }) {
                Cond::All(v)
            } else {
                Cond::Any(v)
            }
        }
        CondSpec::Start { delay_min } => Cond::Start {
            delay_ms: minutes(*delay_min, "H-hour")?,
        },
        CondSpec::AtTime { utc } => {
            let t = crate::scenario::parse_utc_ms(utc)
                .map_err(|_| format!("`{utc}` is not a UTC time like 2026-10-27T04:30:00Z"))?;
            Cond::AtTime {
                at_ms: (t - start_utc_ms).max(0) as u64,
            }
        }
        CondSpec::PhaseStarted { phase: p, delay_min } => Cond::PhaseStarted {
            phase: phase(p)?,
            delay_ms: minutes(*delay_min, "Phase delay")?,
        },
        CondSpec::PhaseEnded { phase: p, delay_min } => Cond::PhaseEnded {
            phase: phase(p)?,
            delay_ms: minutes(*delay_min, "Phase delay")?,
        },
        CondSpec::Manual => Cond::Manual,
        CondSpec::Contacts {
            domain,
            level,
            count,
            area,
        } => {
            let area = match area {
                Some(a) => {
                    if !(a.radius_km.is_finite() && a.radius_km > 0.0 && a.radius_km <= 2_000.0) {
                        return Err("Area radius must be 0–2000 km".to_owned());
                    }
                    Some((orders::point(a.centre)?, km_to_m(a.radius_km)))
                }
                None => None,
            };
            let count = count.unwrap_or(1);
            if !(1..=64).contains(&count) {
                return Err("Contact count must be 1–64".to_owned());
            }
            Cond::Contacts {
                domain: *domain,
                level: level.unwrap_or(ContactLevel::Class),
                count,
                area,
            }
        }
        CondSpec::IntelReport => Cond::IntelReport,
        CondSpec::Arrived => Cond::Arrived,
        CondSpec::TasksDone => Cond::TasksDone,
        CondSpec::WeaponsDone => Cond::WeaponsDone,
        CondSpec::TargetsDestroyed => Cond::TargetsDestroyed,
        CondSpec::Elapsed { min } => Cond::Elapsed {
            ms: minutes(Some(*min), "Time limit")?,
        },
        CondSpec::LossesPct { pct: p } => Cond::LossesPct {
            fraction: pct(*p)?,
        },
        CondSpec::Bingo => Cond::Bingo,
        CondSpec::UnderAttack => Cond::UnderAttack,
        CondSpec::FuelBelow { pct: p } => Cond::FuelBelow {
            fraction: pct(*p)?,
        },
    })
}

/// Phases a trigger waits for.
fn deps(c: &Cond, out: &mut BTreeSet<u32>) {
    match c {
        Cond::All(v) | Cond::Any(v) => v.iter().for_each(|x| deps(x, out)),
        Cond::PhaseStarted { phase, .. } | Cond::PhaseEnded { phase, .. } => {
            out.insert(*phase);
        }
        _ => {}
    }
}

/// The first trigger cycle, as phase indices (`a → b → a`).
fn find_cycle(edges: &[BTreeSet<u32>]) -> Option<Vec<u32>> {
    // 0 unvisited, 1 on the stack, 2 done.
    fn visit(i: u32, edges: &[BTreeSet<u32>], mark: &mut [u8], stack: &mut Vec<u32>) -> bool {
        mark[i as usize] = 1;
        stack.push(i);
        for &j in &edges[i as usize] {
            let m = mark[j as usize];
            if m == 1 {
                stack.push(j);
                return true;
            }
            if m == 0 && visit(j, edges, mark, stack) {
                return true;
            }
        }
        stack.pop();
        mark[i as usize] = 2;
        false
    }
    let mut mark = vec![0u8; edges.len()];
    for i in 0..edges.len() as u32 {
        let mut stack = Vec::new();
        if mark[i as usize] == 0 && visit(i, edges, &mut mark, &mut stack) {
            let last = *stack.last().expect("cycle");
            let from = stack.iter().position(|&x| x == last).unwrap_or(0);
            return Some(stack[from..].to_vec());
        }
    }
    None
}

/// Checks an operation and builds it (state `Planned`); the reason if it is
/// not valid: names, phase references, trigger cycles, units, tasks.
pub fn build(
    world: &mut World,
    geo: Option<&Geo>,
    side: u8,
    id: u32,
    spec: &OperationSpec,
) -> Result<Operation, String> {
    let name = spec.name.trim();
    if name.is_empty() || name.chars().count() > 64 {
        return Err("An operation needs a name of 1–64 characters".to_owned());
    }
    if spec.phases.is_empty() || spec.phases.len() > MAX_PHASES {
        return Err(format!("An operation has 1–{MAX_PHASES} phases"));
    }
    let keys: Vec<String> = spec.phases.iter().map(|p| p.id.trim().to_owned()).collect();
    for (i, k) in keys.iter().enumerate() {
        if k.is_empty() || k.len() > 32 {
            return Err(format!("Phase {}: id must be 1–32 characters", i + 1));
        }
        if keys[..i].contains(k) {
            return Err(format!("Two phases have the id `{k}`"));
        }
    }
    let start_utc_ms = world.resource::<SimClock>().start_utc_ms;
    let mut phases = Vec::new();
    for (i, p) in spec.phases.iter().enumerate() {
        let label = |e: String| format!("Phase {} ({}): {e}", i + 1, p.name);
        if p.name.trim().is_empty() {
            return Err(format!("Phase {} needs a name", i + 1));
        }
        if p.tasks.len() > MAX_TASKS {
            return Err(label(format!("at most {MAX_TASKS} tasks")));
        }
        let trigger = resolve_cond(&p.trigger, &keys, start_utc_ms, true, 0).map_err(label)?;
        let end = match &p.end {
            Some(c) => resolve_cond(c, &keys, start_utc_ms, false, 0).map_err(label)?,
            None => Cond::TasksDone,
        };
        let abort = match &p.abort {
            Some(c) => Some(resolve_cond(c, &keys, start_utc_ms, false, 0).map_err(label)?),
            None => None,
        };
        let mut tasks = Vec::new();
        for (j, t) in p.tasks.iter().enumerate() {
            let label = |e: String| format!("Phase {} ({}), task {}: {e}", i + 1, p.name, j + 1);
            let picks = orders::pick_groups(world, side, &t.units).map_err(label)?;
            let task = orders::resolve_task(world, geo, side, &picks, &t.task).map_err(label)?;
            let fire = t.fire.unwrap_or(false);
            if fire && !matches!(task, Task::Strike { .. } | Task::Suppress { .. }) {
                return Err(label(
                    "only a strike or suppression can be a fire mission".to_owned(),
                ));
            }
            let near_m = match t.near_km {
                Some(r) => {
                    if !matches!(
                        task,
                        Task::Strike {
                            target: StrikeTarget::Point(_),
                            ..
                        } | Task::Suppress {
                            target: StrikeTarget::Point(_),
                            ..
                        }
                    ) {
                        return Err(label(
                            "a search radius needs a strike on a point".to_owned(),
                        ));
                    }
                    if !(r.is_finite() && r > 0.0 && r <= 500.0) {
                        return Err(label("search radius must be 0–500 km".to_owned()));
                    }
                    Some(km_to_m(r))
                }
                None => None,
            };
            let mut units = t.units.clone();
            units.sort();
            units.dedup();
            tasks.push(PhaseTask {
                units,
                task,
                fire,
                near_m,
                groups: Vec::new(),
                missions: Vec::new(),
                target: None,
                aborted: false,
            });
        }
        phases.push(Phase {
            key: keys[i].clone(),
            name: p.name.trim().to_owned(),
            tasks,
            trigger,
            end,
            abort,
            state: PhaseState::Waiting,
            go: false,
            end_now: false,
            started: None,
            ended: None,
            reason: None,
            members: Vec::new(),
            lost: Vec::new(),
            in_flight: Vec::new(),
            tally: PhaseTally::default(),
            bingo: false,
            attacked: false,
        });
    }
    let edges: Vec<BTreeSet<u32>> = phases
        .iter()
        .map(|p| {
            let mut d = BTreeSet::new();
            deps(&p.trigger, &mut d);
            d
        })
        .collect();
    if let Some(cycle) = find_cycle(&edges) {
        let names: Vec<String> = cycle
            .iter()
            .map(|i| format!("Phase {}", i + 1))
            .collect();
        return Err(format!(
            "Trigger cycle: {} wait for each other",
            names.join(" → ")
        ));
    }
    Ok(Operation {
        id,
        side,
        name: name.to_owned(),
        state: OpState::Planned,
        started: None,
        ended: None,
        reason: None,
        phases,
        intel_reports: 0,
    })
}

/// How serious a planner finding is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

/// A planner finding (doc 03 §8 "planner validation before start").
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Issue {
    pub severity: Severity,
    /// 0-based phase, if the finding is about one.
    #[cfg_attr(feature = "ts", ts(optional))]
    pub phase: Option<u32>,
    pub text: String,
}

/// Planner validation (doc 03 §8): the hard checks of [`build`] as an error,
/// then warnings for units already busy, rounds and base stock, fuel to the
/// target and back (a tanker on station lifts it to a note), and ROE.
/// Read-only; the planner calls it on every edit.
pub fn check(world: &mut World, geo: Option<&Geo>, side: u8, spec: &OperationSpec) -> Vec<Issue> {
    let op = match build(world, geo, side, 0, spec) {
        Ok(op) => op,
        Err(text) => {
            return vec![Issue {
                severity: Severity::Error,
                phase: None,
                text,
            }];
        }
    };
    let mut out = Vec::new();
    let rules = world.resource::<Rules>().clone();
    let db = &rules.db;
    let name_of = |world: &World, u: UnitId| {
        entity(world, u)
            .and_then(|e| world.get::<Identity>(e))
            .map_or_else(|| format!("unit {}", u.0), |i| i.name.clone())
    };
    let members_of = |world: &World, units: &[u32]| -> Vec<UnitId> {
        let mut m: Vec<UnitId> = orders::pick_groups(world, side, units)
            .map(|p| p.into_iter().flat_map(|p| p.members).collect())
            .unwrap_or_default();
        m.sort();
        m.dedup();
        m
    };
    // Units busy in running operations.
    let busy: BTreeSet<UnitId> = world
        .resource::<Operations>()
        .ops
        .iter()
        .filter(|o| o.side == side && matches!(o.state, OpState::Running | OpState::Paused))
        .flat_map(|o| o.phases.iter())
        .filter(|p| p.state == PhaseState::Running)
        .flat_map(|p| p.members.iter().copied())
        .collect();
    let tanker_up = {
        let mut q = world.query::<(&Group, &Allegiance)>();
        q.iter(world)
            .any(|(g, a)| a.side == side && matches!(g.task, Task::TankerTrack { .. }))
    } || op
        .phases
        .iter()
        .flat_map(|p| &p.tasks)
        .any(|t| matches!(t.task, Task::TankerTrack { .. }));
    let side_roe = world.resource::<crate::world::Sides>().sides[usize::from(side)].roe;
    let mut tasked: std::collections::BTreeMap<UnitId, Vec<u32>> = Default::default();
    let bases = crate::bases::list(world);
    for (pi, p) in op.phases.iter().enumerate() {
        let phase = Some(pi as u32);
        let mut warn = |sev: Severity, text: String| {
            out.push(Issue {
                severity: sev,
                phase,
                text,
            })
        };
        for t in &p.tasks {
            let members = members_of(world, &t.units);
            for m in &members {
                if busy.contains(m) {
                    warn(
                        Severity::Warning,
                        format!(
                            "{} is busy in a running operation",
                            name_of(world, *m)
                        ),
                    );
                }
                if !t.fire {
                    tasked.entry(*m).or_default().push(pi as u32);
                }
            }
            // Rounds and stock for weapon tasks.
            if let Task::Strike { weapon, count, .. } | Task::Suppress { weapon, count, .. } =
                &t.task
            {
                let mut carried = 0u32;
                let mut stock = 0u32;
                let mut stock_bases = BTreeSet::new();
                for m in &members {
                    let Some(e) = entity(world, *m) else { continue };
                    if let Some(mag) = world.get::<Magazine>(e) {
                        carried += mag
                            .stations
                            .iter()
                            .filter(|s| weapon.is_none_or(|w| w == s.weapon))
                            .map(|s| s.count)
                            .sum::<u32>();
                    }
                    if let Some(Posture::AtBase { base, .. }) = world.get::<Posture>(e)
                        && stock_bases.insert(*base)
                        && let Some(be) = entity(world, *base)
                        && let Some(site) = world.get::<crate::components::BaseSite>(be)
                    {
                        stock += site
                            .stock
                            .iter()
                            .filter(|(w, _)| weapon.is_none_or(|x| x == **w))
                            .map(|(_, n)| *n)
                            .sum::<u32>();
                    }
                }
                let what = weapon.map_or_else(
                    || "rounds".to_owned(),
                    |w| db.str(db.weapons[w.0 as usize].name).to_owned(),
                );
                let want = count.unwrap_or(1);
                if carried < want {
                    if carried + stock >= want {
                        warn(
                            Severity::Info,
                            format!(
                                "Carries {carried} of {want} {what}; base stock covers the rest after a rearm"
                            ),
                        );
                    } else {
                        warn(
                            Severity::Warning,
                            format!(
                                "Carries {carried} of {want} {what} and base stock has {stock}"
                            ),
                        );
                    }
                }
            }
            // ROE: designated targets.
            if let Task::Strike { target, .. } | Task::Suppress { target, .. } = &t.task {
                match target {
                    StrikeTarget::Contact(c) => {
                        let pics = world.resource::<SidePictures>();
                        if let Some(ct) = pics.sides[usize::from(side)].get(*c)
                            && ct.identity != ContactIdentity::Hostile
                        {
                            warn(
                                Severity::Warning,
                                format!(
                                    "Contact C{} is not identified hostile; the operation approves this strike on it",
                                    c.0
                                ),
                            );
                        }
                    }
                    StrikeTarget::Point(_) if t.near_m.is_some() => warn(
                        Severity::Info,
                        "The target is the nearest contact to the point when the phase starts; the operation approves that strike only".to_owned(),
                    ),
                    StrikeTarget::Point(_) => {}
                }
            } else if matches!(t.task, Task::Cap { .. } | Task::Intercept { .. })
                || matches!(t.task, Task::PatrolCircle { .. } | Task::PatrolRoute { .. })
            {
                let roe = members
                    .first()
                    .map(|m| crate::roe::effective(world, side, *m).0)
                    .unwrap_or(side_roe);
                if roe == Roe::Charlie && !matches!(t.task, Task::Intercept { .. }) {
                    warn(
                        Severity::Info,
                        "Units on ROE Charlie: every engagement other than self-defence will need approval".to_owned(),
                    );
                }
            }
            // Fuel to the furthest point of the task and back home.
            for m in &members {
                let Some(e) = entity(world, *m) else { continue };
                let Some(pix) = world.get::<PlatformRef>(e).map(|p| p.0) else {
                    continue;
                };
                let pl = rules.platform(pix);
                if !rules.is_air(pl) {
                    continue;
                }
                let (Some(k), Some(f)) = (
                    world.get::<Kinematics>(e).copied(),
                    world.get::<Fuel>(e).copied(),
                ) else {
                    continue;
                };
                let far = task_points(world, side, &t.task)
                    .into_iter()
                    .map(|q| (distance_m(k.pos(), q), q))
                    .max_by(|a, b| a.0.total_cmp(&b.0));
                let Some((out_m, q)) = far else { continue };
                let back_m = bases
                    .iter()
                    .filter(|b| b.side == side && b.air)
                    .map(|b| distance_m(q, b.at))
                    .min_by(|a, b| a.total_cmp(b))
                    .unwrap_or(out_m);
                let alt = rules.cruise_alt_m(pl);
                let need = rules.fuel_for_distance(pl, out_m + back_m, alt)
                    * (1.0 + rules.realism().logistics.fuel_reserve_fraction);
                if need > f.kg {
                    let text = format!(
                        "{} needs about {:.0} kg of fuel to reach the task and return, has {:.0} kg",
                        name_of(world, *m),
                        need,
                        f.kg
                    );
                    if tanker_up {
                        warn(Severity::Info, format!("{text}; a tanker is planned"));
                    } else {
                        warn(Severity::Warning, format!("{text}; no tanker is planned"));
                    }
                }
            }
        }
    }
    for (u, ps) in tasked {
        if ps.len() > 1 {
            let list: Vec<String> = ps.iter().map(|p| format!("Phase {}", p + 1)).collect();
            out.push(Issue {
                severity: Severity::Info,
                phase: ps.last().copied(),
                text: format!(
                    "{} is tasked in {}; each phase that starts takes it over",
                    name_of(world, u),
                    list.join(" and ")
                ),
            });
        }
    }
    out.sort_by(|a, b| b.severity.cmp(&a.severity));
    out
}

/// Where a task goes, for the planner's fuel check and the routes it draws.
pub fn task_points(world: &World, side: u8, task: &Task) -> Vec<LatLon> {
    match task {
        Task::Move { waypoints, .. } | Task::PatrolRoute { waypoints, .. } => waypoints.clone(),
        Task::PatrolCircle { centre, .. } | Task::Cap { centre, .. } | Task::Jam { centre, .. } => {
            vec![*centre]
        }
        Task::TankerTrack { a, b, .. } => vec![*a, *b],
        Task::Strike { target, .. } | Task::Suppress { target, .. } => match target {
            StrikeTarget::Point(p) => vec![*p],
            StrikeTarget::Contact(c) => contact_at(world, side, *c).into_iter().collect(),
        },
        Task::Intercept { contact } => contact_at(world, side, *contact).into_iter().collect(),
        Task::Idle | Task::Rtb { .. } | Task::Land { .. } | Task::Transfer { .. } => Vec::new(),
    }
}

fn contact_at(world: &World, side: u8, c: ContactId) -> Option<LatLon> {
    world.resource::<SidePictures>().sides[usize::from(side)]
        .get(c)
        .and_then(|c| c.est)
        .map(|e| e.at)
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Checks an operation command against the current state; the reason if it
/// cannot apply.
pub fn validate(world: &World, side: u8, cmd: &OpCommand) -> Result<(), String> {
    let ops = world.resource::<Operations>();
    let find = |id: u32| {
        ops.get(id)
            .filter(|o| o.side == side)
            .ok_or_else(|| format!("Unknown operation {id}"))
    };
    fn phase(op: &Operation, p: u32) -> Result<&Phase, String> {
        op.phases
            .get(p as usize)
            .ok_or_else(|| format!("{} has no phase {}", op.name, p + 1))
    }
    match cmd {
        OpCommand::Create { replace, .. } => {
            if let Some(r) = replace {
                let o = find(*r)?;
                if o.state != OpState::Planned {
                    return Err(format!("{} has started and can no longer be edited", o.name));
                }
            }
            Ok(())
        }
        OpCommand::Start { op } => {
            let o = find(*op)?;
            if o.state != OpState::Planned {
                return Err(format!("{} has already started", o.name));
            }
            Ok(())
        }
        OpCommand::Pause { op } => match find(*op)?.state {
            OpState::Running => Ok(()),
            _ => Err("Only a running operation can be paused".to_owned()),
        },
        OpCommand::Resume { op } => match find(*op)?.state {
            OpState::Paused => Ok(()),
            _ => Err("Only a paused operation can be resumed".to_owned()),
        },
        OpCommand::Abort { op } => {
            let o = find(*op)?;
            match o.state {
                OpState::Completed | OpState::Aborted => Err(format!("{} has ended", o.name)),
                _ => Ok(()),
            }
        }
        OpCommand::Go { op, phase: p } => {
            let o = find(*op)?;
            let ph = phase(o, *p)?;
            if !matches!(o.state, OpState::Running | OpState::Paused | OpState::Planned) {
                return Err(format!("{} has ended", o.name));
            }
            if ph.state != PhaseState::Waiting {
                return Err(format!("{} is no longer waiting", ph.name));
            }
            if !has_manual(&ph.trigger) {
                return Err(format!("{} has no manual trigger", ph.name));
            }
            Ok(())
        }
        OpCommand::EndPhase { op, phase: p } => {
            let o = find(*op)?;
            let ph = phase(o, *p)?;
            if ph.state != PhaseState::Running {
                return Err(format!("{} is not running", ph.name));
            }
            Ok(())
        }
    }
}

fn has_manual(c: &Cond) -> bool {
    match c {
        Cond::Manual => true,
        Cond::All(v) | Cond::Any(v) => v.iter().any(has_manual),
        _ => false,
    }
}

/// Applies a checked operation command (orders stage).
pub fn apply(world: &mut World, side: u8, cmd: OpCommand) {
    if validate(world, side, &cmd).is_err() {
        // The state changed since the order was checked (e.g. the operation
        // ended on its own): nothing to do.
        return;
    }
    let stamp = now(world);
    let mut ops = std::mem::take(&mut *world.resource_mut::<Operations>());
    let mut events = Vec::new();
    match cmd {
        OpCommand::Create { op, replace, start } => {
            if let Some(r) = replace {
                ops.ops.retain(|o| o.id != r);
            }
            let mut op = *op;
            if start {
                op.state = OpState::Running;
                op.started = Some(stamp);
            }
            events.push(changed(&op, None));
            ops.ops.push(op);
            ops.ops.sort_by_key(|o| o.id);
        }
        OpCommand::Start { op } => {
            if let Some(o) = ops.ops.iter_mut().find(|o| o.id == op) {
                o.state = OpState::Running;
                o.started = Some(stamp);
                events.push(changed(o, None));
            }
        }
        OpCommand::Pause { op } | OpCommand::Resume { op } => {
            if let Some(o) = ops.ops.iter_mut().find(|o| o.id == op) {
                o.state = if matches!(cmd, OpCommand::Pause { .. }) {
                    OpState::Paused
                } else {
                    OpState::Running
                };
                events.push(changed(o, None));
            }
        }
        OpCommand::Abort { op } => {
            if let Some(i) = ops.ops.iter().position(|o| o.id == op) {
                let mut o = ops.ops[i].clone();
                finish(world, &mut o, stamp, OpState::Aborted, "Aborted by order", &mut events);
                ops.ops[i] = o;
            }
        }
        OpCommand::Go { op, phase } => {
            if let Some(o) = ops.ops.iter_mut().find(|o| o.id == op) {
                o.phases[phase as usize].go = true;
            }
        }
        OpCommand::EndPhase { op, phase } => {
            if let Some(o) = ops.ops.iter_mut().find(|o| o.id == op) {
                o.phases[phase as usize].end_now = true;
            }
        }
    }
    *world.resource_mut::<Operations>() = ops;
    for e in events {
        emit(world, side, e);
    }
}

fn changed(o: &Operation, reason: Option<String>) -> EventKind {
    EventKind::OperationChanged {
        operation: o.id,
        name: o.name.clone(),
        state: o.state,
        reason,
    }
}

fn now(world: &World) -> Stamp {
    let c = world.resource::<SimClock>();
    Stamp {
        tick: c.tick,
        ms: c.sim_time_ms,
    }
}

// ---------------------------------------------------------------------------
// The stage
// ---------------------------------------------------------------------------

/// Second stage of every tick: reads the side's new events into the running
/// phases, then starts, ends and aborts phases.
pub fn run(world: &mut World) {
    if world.resource::<Operations>().ops.is_empty() {
        let n = world.resource::<EventLog>().0.len() as u64;
        world.resource_mut::<Operations>().events_seen = n;
        return;
    }
    let mut ops = std::mem::take(&mut *world.resource_mut::<Operations>());
    tally(world, &mut ops);
    let stamp = now(world);
    let mut events: Vec<(u8, EventKind)> = Vec::new();
    for i in 0..ops.ops.len() {
        if ops.ops[i].state != OpState::Running {
            continue;
        }
        let mut o = ops.ops[i].clone();
        let mut evs = Vec::new();
        step(world, &mut ops.next_mission, &mut o, stamp, &mut evs);
        events.extend(evs.into_iter().map(|e| (o.side, e)));
        ops.ops[i] = o;
    }
    *world.resource_mut::<Operations>() = ops;
    for (side, e) in events {
        emit(world, side, e);
    }
}

/// Reads events since the last pass into the phases (each operation sees
/// only its side's events).
fn tally(world: &World, ops: &mut Operations) {
    let log = &world.resource::<EventLog>().0;
    let from = (ops.events_seen as usize).min(log.len());
    ops.events_seen = log.len() as u64;
    // Fire-mission salvos belong to their phase, not to every phase the
    // shooter is in.
    for e in &log[from..] {
        for o in ops
            .ops
            .iter_mut()
            .filter(|o| matches!(o.state, OpState::Running | OpState::Paused))
        {
            if e.sides & (1 << o.side) == 0 {
                continue;
            }
            if matches!(e.kind, EventKind::IntelReport { .. }) {
                o.intel_reports += 1;
            }
            let mission_salvos: BTreeSet<u32> = o
                .phases
                .iter()
                .flat_map(|p| &p.tasks)
                .flat_map(|t| &t.missions)
                .filter_map(|(g, id)| mission_of(world, *g, *id))
                .flat_map(|m| m.salvos)
                .collect();
            for p in o
                .phases
                .iter_mut()
                .filter(|p| p.state == PhaseState::Running)
            {
                if p.started.is_some_and(|s| e.tick < s.tick) {
                    continue;
                }
                match &e.kind {
                    EventKind::WeaponLaunched {
                        weapon,
                        shooter,
                        salvo,
                        reason,
                        ..
                    } => {
                        let own_mission = p
                            .tasks
                            .iter()
                            .flat_map(|t| &t.missions)
                            .filter_map(|(g, id)| mission_of(world, *g, *id))
                            .any(|m| m.salvos.contains(salvo));
                        let by_task = *reason == crate::weapons::LaunchReason::Ordered
                            && !mission_salvos.contains(salvo)
                            && p.members.contains(shooter)
                            && p.tasks.iter().any(|t| {
                                !t.fire
                                    && matches!(
                                        t.task,
                                        Task::Strike { .. }
                                            | Task::Suppress { .. }
                                            | Task::Intercept { .. }
                                    )
                            });
                        if own_mission || by_task {
                            p.in_flight.push(*weapon);
                            p.tally.launched += 1;
                        }
                    }
                    EventKind::WeaponTerminated {
                        weapon, outcome, ..
                    } => {
                        if let Some(k) = p.in_flight.iter().position(|w| w == weapon) {
                            p.in_flight.remove(k);
                            match outcome {
                                Outcome::Hit => p.tally.hits += 1,
                                Outcome::Miss => p.tally.misses += 1,
                                Outcome::Intercepted => p.tally.intercepted += 1,
                                Outcome::Decoyed => p.tally.decoyed += 1,
                                Outcome::Jammed => p.tally.jammed += 1,
                                _ => p.tally.other += 1,
                            }
                        }
                    }
                    EventKind::UnitDestroyed { unit, .. } => {
                        if p.members.contains(unit) && !p.lost.contains(unit) {
                            p.lost.push(*unit);
                            p.tally.lost += 1;
                        }
                    }
                    EventKind::BingoRtb { unit, .. } => {
                        if p.members.contains(unit) {
                            p.bingo = true;
                        }
                    }
                    EventKind::ToTanker { group, .. } => {
                        if p.tasks.iter().any(|t| t.groups.contains(group)) {
                            p.bingo = true;
                        }
                    }
                    EventKind::IncomingWeapon { unit, .. } => {
                        if p.members.contains(unit) {
                            p.attacked = true;
                        }
                    }
                    EventKind::StrikeAborted { group, .. } => {
                        for t in p.tasks.iter_mut().filter(|t| t.groups.contains(group)) {
                            t.aborted = true;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

fn mission_of(world: &World, group: UnitId, id: u32) -> Option<FireMission> {
    entity(world, group)
        .and_then(|e| world.get::<Group>(e))
        .and_then(|g| g.fires.iter().find(|m| m.id == id).cloned())
}

/// One pass over a running operation, repeated until nothing changes.
fn step(
    world: &mut World,
    next_mission: &mut u32,
    o: &mut Operation,
    stamp: Stamp,
    events: &mut Vec<EventKind>,
) {
    for _ in 0..=(2 * o.phases.len() + 1) {
        let mut changed = false;
        for i in 0..o.phases.len() {
            match o.phases[i].state {
                PhaseState::Waiting => {
                    let p = &o.phases[i];
                    if eval(world, o, p, &p.trigger, p.go) {
                        start_phase(world, next_mission, o, i, stamp, events);
                        changed = true;
                    } else if dead(o, &o.phases[i].trigger) {
                        let p = &mut o.phases[i];
                        p.state = PhaseState::Cancelled;
                        p.ended = Some(stamp);
                        p.reason = Some("Its trigger can no longer happen".to_owned());
                        events.push(phase_ended(o, i));
                        changed = true;
                    }
                }
                PhaseState::Running => {
                    let p = &o.phases[i];
                    if p.end_now || eval(world, o, p, &p.end, p.end_now) {
                        let why = if p.end_now {
                            "Ended by order".to_owned()
                        } else {
                            describe_true(world, o, p, &p.end)
                        };
                        end_phase(world, o, i, stamp, PhaseState::Done, why, events);
                        changed = true;
                    } else if let Some(a) = &p.abort
                        && eval(world, o, p, a, false)
                    {
                        let why = format!("Abort: {}", describe_true(world, o, p, a));
                        end_phase(world, o, i, stamp, PhaseState::Aborted, why, events);
                        changed = true;
                    }
                }
                _ => {}
            }
        }
        if !changed {
            break;
        }
    }
    if o.phases.iter().all(|p| p.state.is_final()) {
        let any_done = o.phases.iter().any(|p| p.state == PhaseState::Done);
        let all_ok = o
            .phases
            .iter()
            .all(|p| matches!(p.state, PhaseState::Done));
        let reason = if all_ok {
            "Every phase completed".to_owned()
        } else if any_done {
            "Every phase has ended; some were aborted or cancelled".to_owned()
        } else {
            "No phase completed".to_owned()
        };
        o.state = OpState::Completed;
        o.ended = Some(stamp);
        o.reason = Some(reason.clone());
        events.push(changed(o, Some(reason)));
    }
}

/// A trigger that can no longer hold: it waits for a phase that will never
/// start, or for one to complete that was aborted or cancelled.
fn dead(o: &Operation, c: &Cond) -> bool {
    match c {
        Cond::All(v) => v.iter().any(|x| dead(o, x)),
        Cond::Any(v) => v.iter().all(|x| dead(o, x)),
        Cond::PhaseStarted { phase, .. } => {
            o.phases[*phase as usize].state == PhaseState::Cancelled
        }
        Cond::PhaseEnded { phase, .. } => matches!(
            o.phases[*phase as usize].state,
            PhaseState::Aborted | PhaseState::Cancelled
        ),
        _ => false,
    }
}

fn phase_ended(o: &Operation, i: usize) -> EventKind {
    let p = &o.phases[i];
    EventKind::PhaseEnded {
        operation: o.id,
        phase: i as u32,
        name: p.name.clone(),
        state: p.state,
        reason: p.reason.clone().unwrap_or_default(),
    }
}

/// Whether a condition holds now. `manual`: the phase's Go (trigger) or End
/// (end) was pressed.
fn eval(world: &World, o: &Operation, p: &Phase, c: &Cond, manual: bool) -> bool {
    let now = world.resource::<SimClock>().sim_time_ms;
    let since = |s: Option<Stamp>, d: u64| s.is_some_and(|s| now >= s.ms + d);
    match c {
        Cond::All(v) => v.iter().all(|x| eval(world, o, p, x, manual)),
        Cond::Any(v) => v.iter().any(|x| eval(world, o, p, x, manual)),
        Cond::Start { delay_ms } => since(o.started, *delay_ms),
        Cond::AtTime { at_ms } => now >= *at_ms,
        Cond::PhaseStarted { phase, delay_ms } => {
            since(o.phases[*phase as usize].started, *delay_ms)
        }
        Cond::PhaseEnded { phase, delay_ms } => {
            let q = &o.phases[*phase as usize];
            q.state == PhaseState::Done && since(q.ended, *delay_ms)
        }
        Cond::Manual => manual,
        Cond::Contacts {
            domain,
            level,
            count,
            area,
        } => {
            let pics = world.resource::<SidePictures>();
            let n = pics.sides[usize::from(o.side)]
                .contacts
                .values()
                .filter(|c| {
                    !c.lost
                        && c.identity != ContactIdentity::Friendly
                        && !matches!(c.type_ref, TypeRef::Weapon(_))
                        && c.level >= *level
                        && domain.is_none_or(|d| d == c.domain)
                        && match (area, c.est) {
                            (None, Some(_)) => true,
                            (Some((at, r)), Some(e)) => distance_m(*at, e.at) <= *r,
                            _ => false,
                        }
                })
                .count();
            n >= *count as usize
        }
        Cond::IntelReport => o.intel_reports > 0,
        Cond::Elapsed { ms } => since(p.started, *ms),
        Cond::LossesPct { fraction } => {
            !p.members.is_empty() && p.lost.len() as f64 / p.members.len() as f64 >= *fraction
        }
        Cond::Bingo => p.bingo,
        Cond::UnderAttack => p.attacked,
        Cond::FuelBelow { fraction } => p.members.iter().any(|m| {
            entity(world, *m)
                .and_then(|e| world.get::<Fuel>(e))
                .is_some_and(|f| f.max_kg > 0.0 && f.kg / f.max_kg < *fraction)
        }),
        Cond::Arrived => p.state == PhaseState::Running && p.tasks.iter().all(|t| arrived(world, t)),
        Cond::TasksDone => {
            p.state == PhaseState::Running
                && p.tasks.iter().all(|t| task_done(world, t))
                && p.in_flight.is_empty()
        }
        Cond::WeaponsDone => {
            p.state == PhaseState::Running
                && p.in_flight.is_empty()
                && p.tasks
                    .iter()
                    .filter(|t| matches!(t.task, Task::Strike { .. } | Task::Suppress { .. }))
                    .all(|t| task_done(world, t))
        }
        Cond::TargetsDestroyed => {
            let pics = world.resource::<SidePictures>();
            let targets: Vec<ContactId> = p.tasks.iter().filter_map(|t| t.target).collect();
            !targets.is_empty()
                && targets
                    .iter()
                    .all(|c| pics.sides[usize::from(o.side)].get(*c).is_none())
        }
    }
}

/// Plain English for a condition that holds: the leaves that made it true.
fn describe_true(world: &World, o: &Operation, p: &Phase, c: &Cond) -> String {
    match c {
        Cond::Any(v) => v
            .iter()
            .find(|x| eval(world, o, p, x, p.end_now))
            .map_or_else(|| describe(o, c), |x| describe_true(world, o, p, x)),
        Cond::All(v) => v
            .iter()
            .map(|x| describe_true(world, o, p, x))
            .collect::<Vec<_>>()
            .join(" and "),
        _ => describe(o, c),
    }
}

fn mins(ms: u64) -> String {
    let m = ms as f64 / 60_000.0;
    if m.fract() == 0.0 {
        format!("{m:.0} min")
    } else {
        format!("{m:.1} min")
    }
}

/// Plain English for a condition, as the planner shows it.
pub fn describe(o: &Operation, c: &Cond) -> String {
    let phase = |i: u32| {
        o.phases
            .get(i as usize)
            .map_or_else(|| format!("phase {}", i + 1), |p| format!("Phase {} ({})", i + 1, p.name))
    };
    let plus = |d: u64| {
        if d == 0 {
            String::new()
        } else {
            format!(" + {}", mins(d))
        }
    };
    match c {
        Cond::All(v) => v
            .iter()
            .map(|x| describe(o, x))
            .collect::<Vec<_>>()
            .join(" and "),
        Cond::Any(v) => v
            .iter()
            .map(|x| describe(o, x))
            .collect::<Vec<_>>()
            .join(" or "),
        Cond::Start { delay_ms } => format!("H+{}", mins(*delay_ms)),
        Cond::AtTime { at_ms } => format!("at T+{}", crate::reports::fmt_t(*at_ms)),
        Cond::PhaseStarted { phase: q, delay_ms } => format!("with {}{}", phase(*q), plus(*delay_ms)),
        Cond::PhaseEnded { phase: q, delay_ms } => format!("after {}{}", phase(*q), plus(*delay_ms)),
        Cond::Manual => "manual".to_owned(),
        Cond::Contacts {
            domain,
            level,
            count,
            area,
        } => {
            let d = domain.map_or("", |d| match d {
                Domain::Air => "air ",
                Domain::Surface => "surface ",
                Domain::Subsurface => "subsurface ",
                Domain::Ground => "ground ",
                Domain::Space => "space ",
            });
            let lv = match level {
                ContactLevel::Unknown => "detected",
                ContactLevel::Domain => "detected",
                ContactLevel::Class => "classified",
                ContactLevel::Type => "identified to type",
                ContactLevel::Precise => "located precisely",
            };
            let a = area.map_or(String::new(), |(_, r)| format!(" within {:.0} km", r / 1_000.0));
            format!(
                "{count} {d}contact{} {lv}{a}",
                if *count == 1 { "" } else { "s" }
            )
        }
        Cond::IntelReport => "satellite report received".to_owned(),
        Cond::Arrived => "arrived".to_owned(),
        Cond::TasksDone => "all tasks done".to_owned(),
        Cond::WeaponsDone => "weapons ended".to_owned(),
        Cond::TargetsDestroyed => "targets destroyed".to_owned(),
        Cond::Elapsed { ms } => format!("{} elapsed", mins(*ms)),
        Cond::LossesPct { fraction } => format!("losses ≥ {:.0}%", fraction * 100.0),
        Cond::Bingo => "bingo".to_owned(),
        Cond::UnderAttack => "under attack".to_owned(),
        Cond::FuelBelow { fraction } => format!("fuel below {:.0}%", fraction * 100.0),
    }
}

fn group_of(world: &World, g: UnitId) -> Option<&Group> {
    entity(world, g).and_then(|e| world.get::<Group>(e))
}

/// A task's groups have reached their station, last waypoint or base.
fn arrived(world: &World, t: &PhaseTask) -> bool {
    t.fire
        || t.groups.iter().all(|gid| {
            let Some(g) = group_of(world, *gid) else {
                return true;
            };
            if g.task.kind() != t.task.kind() {
                return true;
            }
            match &g.task {
                Task::Move { waypoints, .. } => g.wp as usize >= waypoints.len(),
                Task::PatrolRoute {
                    waypoints, looped, ..
                } => g.on_station || (!looped && g.wp as usize >= waypoints.len()),
                Task::Rtb { .. } | Task::Land { .. } | Task::Transfer { .. } => {
                    matches!(g.mode, Mode::AtBase { .. })
                }
                _ => g.on_station || matches!(g.mode, Mode::AtBase { .. }),
            }
        })
}

/// A task is done: its groups are gone or retasked, or what it asked for
/// happened (last waypoint, home, rounds away).
fn task_done(world: &World, t: &PhaseTask) -> bool {
    if t.fire {
        return t.aborted
            || t.missions
                .iter()
                .all(|(g, id)| mission_of(world, *g, *id).is_none_or(|m| m.done));
    }
    t.aborted
        || t.groups.iter().all(|gid| {
            let Some(g) = group_of(world, *gid) else {
                return true;
            };
            if g.task.kind() != t.task.kind() {
                return true;
            }
            let home = matches!(g.mode, Mode::Return { .. } | Mode::AtBase { .. });
            match &g.task {
                Task::Move { waypoints, .. } => g.wp as usize >= waypoints.len(),
                Task::PatrolRoute {
                    waypoints, looped, ..
                } => home || (!looped && g.wp as usize >= waypoints.len()),
                Task::Rtb { .. } | Task::Land { .. } | Task::Transfer { .. } => {
                    matches!(g.mode, Mode::AtBase { .. })
                }
                Task::Strike { count, fired, .. } | Task::Suppress { count, fired, .. } => {
                    home || count.is_some_and(|c| *fired >= c)
                }
                Task::Intercept { contact } => {
                    home || world.resource::<SidePictures>().sides
                        [usize::from(side_of(world, *gid))]
                    .get(*contact)
                    .is_none()
                }
                Task::Idle => true,
                _ => home,
            }
        })
}

fn side_of(world: &World, u: UnitId) -> u8 {
    entity(world, u)
        .and_then(|e| world.get::<Allegiance>(e))
        .map_or(0, |a| a.side)
}

/// Units of a task that are still alive and on the side.
fn live_units(world: &World, side: u8, units: &[u32]) -> Vec<u32> {
    units
        .iter()
        .copied()
        .filter(|u| {
            entity(world, UnitId(*u)).is_some_and(|e| {
                world.get::<Allegiance>(e).map(|a| a.side) == Some(side)
                    && (world.get::<Group>(e).is_some() || world.get::<GroupMember>(e).is_some())
            })
        })
        .collect()
}

/// The side's nearest non-friendly, non-weapon contact within `r` of `at`.
fn nearest_contact(world: &World, side: u8, at: LatLon, r: f64) -> Option<ContactId> {
    let pics = world.resource::<SidePictures>();
    pics.sides[usize::from(side)]
        .contacts
        .values()
        .filter(|c| {
            !c.lost
                && c.identity != ContactIdentity::Friendly
                && !matches!(c.type_ref, TypeRef::Weapon(_))
        })
        .filter_map(|c| c.est.map(|e| (distance_m(at, e.at), c.id)))
        .filter(|(d, _)| *d <= r)
        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
        .map(|(_, id)| id)
}

fn start_phase(
    world: &mut World,
    next_mission: &mut u32,
    o: &mut Operation,
    i: usize,
    stamp: Stamp,
    events: &mut Vec<EventKind>,
) {
    let side = o.side;
    let mut members: BTreeSet<UnitId> = BTreeSet::new();
    let mut tasks = std::mem::take(&mut o.phases[i].tasks);
    for t in &mut tasks {
        let units = live_units(world, side, &t.units);
        let Ok(picks) = orders::pick_groups(world, side, &units) else {
            continue;
        };
        let mut task = t.task.clone();
        if let (Some(r), Task::Strike { target, .. } | Task::Suppress { target, .. }) =
            (t.near_m, &mut task)
            && let StrikeTarget::Point(p) = *target
            && let Some(c) = nearest_contact(world, side, p, r)
        {
            *target = StrikeTarget::Contact(c);
        }
        if let Task::Strike {
            target: StrikeTarget::Contact(c),
            ..
        }
        | Task::Suppress {
            target: StrikeTarget::Contact(c),
            ..
        } = task
        {
            t.target = Some(c);
        }
        for pick in &picks {
            members.extend(pick.members.iter().copied());
            if t.fire {
                let (Task::Strike {
                    target,
                    weapon,
                    count,
                    ..
                }
                | Task::Suppress {
                    target,
                    weapon,
                    count,
                    ..
                }) = &task
                else {
                    continue;
                };
                let Some(ge) = entity(world, pick.group) else {
                    continue;
                };
                *next_mission += 1;
                let id = *next_mission;
                if let Some(mut g) = world.get_mut::<Group>(ge) {
                    g.fires.push(FireMission {
                        id,
                        target: *target,
                        weapon: *weapon,
                        count: count.unwrap_or(1),
                        fired: 0,
                        salvos: Vec::new(),
                        done: false,
                    });
                }
                t.groups.push(pick.group);
                t.missions.push((pick.group, id));
            } else if let Some(ge) = orders::split(world, side, pick) {
                let gid = world.get::<Identity>(ge).map(|x| x.unit);
                orders::assign(world, side, ge, task.clone());
                if let Some(gid) = gid {
                    t.groups.push(gid);
                }
            }
        }
        t.task = task;
    }
    let p = &mut o.phases[i];
    p.tasks = tasks;
    p.state = PhaseState::Running;
    p.started = Some(stamp);
    p.members = members.into_iter().collect();
    events.push(EventKind::PhaseStarted {
        operation: o.id,
        phase: i as u32,
        name: p.name.clone(),
    });
    events.push(EventKind::AutoSlow {
        reason: format!("{}: {} started", o.name, p.name),
        cause: crate::events::SlowCause::PhaseStart,
    });
}

/// Ends a phase. Its fire missions close; an aborted phase sends its
/// aircraft home and stops its ships.
fn end_phase(
    world: &mut World,
    o: &mut Operation,
    i: usize,
    stamp: Stamp,
    state: PhaseState,
    reason: String,
    events: &mut Vec<EventKind>,
) {
    let side = o.side;
    let p = &mut o.phases[i];
    p.state = state;
    p.ended = Some(stamp);
    p.reason = Some(reason);
    for t in &p.tasks {
        for (g, id) in &t.missions {
            if let Some(e) = entity(world, *g)
                && let Some(mut grp) = world.get_mut::<Group>(e)
            {
                grp.fires.retain(|m| m.id != *id);
            }
        }
    }
    if state == PhaseState::Aborted {
        let groups: Vec<UnitId> = p
            .tasks
            .iter()
            .filter(|t| !t.fire)
            .flat_map(|t| t.groups.iter().copied())
            .collect();
        for gid in groups {
            recall(world, side, gid);
        }
    }
    events.push(phase_ended(o, i));
}

/// Aborted phase: aircraft to the nearest suitable base, ships stop.
fn recall(world: &mut World, side: u8, gid: UnitId) {
    let Some(ge) = entity(world, gid) else { return };
    let rules = world.resource::<Rules>().clone();
    let Some(g) = world.get::<Group>(ge).cloned() else {
        return;
    };
    if matches!(g.mode, Mode::AtBase { .. } | Mode::Return { .. }) {
        return;
    }
    let lead = g.lead().and_then(|l| entity(world, l));
    let air = lead
        .and_then(|e| world.get::<PlatformRef>(e))
        .is_some_and(|p| rules.is_air(rules.platform(p.0)));
    if air {
        let list = crate::bases::list(world);
        let mut g = g;
        crate::tasks::go_home(
            world,
            &rules,
            &list,
            side,
            gid,
            &mut g,
            crate::components::ReturnWhy::Order,
        );
        *world.get_mut::<Group>(ge).expect("group") = g;
    } else {
        orders::assign(world, side, ge, Task::Idle);
    }
}

/// Ends an operation: running phases abort, waiting ones are cancelled.
fn finish(
    world: &mut World,
    o: &mut Operation,
    stamp: Stamp,
    state: OpState,
    reason: &str,
    events: &mut Vec<EventKind>,
) {
    for i in 0..o.phases.len() {
        match o.phases[i].state {
            PhaseState::Running => end_phase(
                world,
                o,
                i,
                stamp,
                PhaseState::Aborted,
                reason.to_owned(),
                events,
            ),
            PhaseState::Waiting => {
                let p = &mut o.phases[i];
                p.state = PhaseState::Cancelled;
                p.ended = Some(stamp);
                p.reason = Some(reason.to_owned());
                events.push(phase_ended(o, i));
            }
            _ => {}
        }
    }
    o.state = state;
    o.ended = Some(stamp);
    o.reason = Some(reason.to_owned());
    events.push(changed(o, Some(reason.to_owned())));
}

// ---------------------------------------------------------------------------
// Planner view (client JSON)
// ---------------------------------------------------------------------------

/// What the planner and the map show of a phase task.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TaskView {
    pub kind: String,
    pub units: Vec<u32>,
    pub unit_names: Vec<String>,
    pub fire: bool,
    /// `[lon, lat]` degrees: the route or the target point.
    pub route: Vec<LonLat>,
    /// Circle radius for patrol, CAP and jamming orbits, km.
    #[cfg_attr(feature = "ts", ts(optional))]
    pub radius_km: Option<f64>,
    #[cfg_attr(feature = "ts", ts(optional))]
    pub target: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PhaseView {
    pub id: String,
    pub name: String,
    pub state: PhaseState,
    pub trigger: String,
    pub end: String,
    #[cfg_attr(feature = "ts", ts(optional))]
    pub abort: Option<String>,
    /// Phases the trigger waits for: `(phase, "started" | "ended")`.
    pub deps: Vec<(u32, String)>,
    #[cfg_attr(feature = "ts", ts(optional))]
    pub started: Option<Stamp>,
    #[cfg_attr(feature = "ts", ts(optional))]
    pub ended: Option<Stamp>,
    #[cfg_attr(feature = "ts", ts(optional))]
    pub reason: Option<String>,
    pub manual_go: bool,
    pub tasks: Vec<TaskView>,
    pub tally: PhaseTally,
    pub in_flight: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct OperationView {
    pub id: u32,
    pub name: String,
    pub state: OpState,
    #[cfg_attr(feature = "ts", ts(optional))]
    pub started: Option<Stamp>,
    #[cfg_attr(feature = "ts", ts(optional))]
    pub ended: Option<Stamp>,
    #[cfg_attr(feature = "ts", ts(optional))]
    pub reason: Option<String>,
    pub phases: Vec<PhaseView>,
}

fn deps_of(c: &Cond, out: &mut Vec<(u32, String)>) {
    match c {
        Cond::All(v) | Cond::Any(v) => v.iter().for_each(|x| deps_of(x, out)),
        Cond::PhaseStarted { phase, .. } => out.push((*phase, "started".to_owned())),
        Cond::PhaseEnded { phase, .. } => out.push((*phase, "ended".to_owned())),
        _ => {}
    }
}

fn lonlat(p: LatLon) -> LonLat {
    [p.lon_deg(), p.lat_deg()]
}

/// The side's operations as the planner shows them: own data only.
pub fn views(world: &World, side: u8) -> Vec<OperationView> {
    let ops = world.resource::<Operations>();
    ops.ops
        .iter()
        .filter(|o| o.side == side)
        .map(|o| view(world, o))
        .collect()
}

pub fn view(world: &World, o: &Operation) -> OperationView {
    let name = |u: u32| {
        entity(world, UnitId(u))
            .and_then(|e| world.get::<Identity>(e))
            .map_or_else(|| format!("unit {u} (lost)"), |i| i.name.clone())
    };
    OperationView {
        id: o.id,
        name: o.name.clone(),
        state: o.state,
        started: o.started,
        ended: o.ended,
        reason: o.reason.clone(),
        phases: o
            .phases
            .iter()
            .map(|p| {
                let mut deps = Vec::new();
                deps_of(&p.trigger, &mut deps);
                PhaseView {
                    id: p.key.clone(),
                    name: p.name.clone(),
                    state: p.state,
                    trigger: describe(o, &p.trigger),
                    end: describe(o, &p.end),
                    abort: p.abort.as_ref().map(|a| describe(o, a)),
                    deps,
                    started: p.started,
                    ended: p.ended,
                    reason: p.reason.clone(),
                    manual_go: has_manual(&p.trigger),
                    tasks: p
                        .tasks
                        .iter()
                        .map(|t| {
                            let radius_km = match &t.task {
                                Task::PatrolCircle { radius_m, .. }
                                | Task::Cap { radius_m, .. }
                                | Task::Jam { radius_m, .. } => Some(radius_m / 1_000.0),
                                _ => t.near_m.map(|r| r / 1_000.0),
                            };
                            TaskView {
                                kind: if t.fire {
                                    "fire".to_owned()
                                } else {
                                    t.task.kind().to_owned()
                                },
                                units: t.units.clone(),
                                unit_names: t.units.iter().map(|u| name(*u)).collect(),
                                fire: t.fire,
                                route: task_points(world, o.side, &t.task)
                                    .into_iter()
                                    .map(lonlat)
                                    .collect(),
                                radius_km,
                                target: t.target.map(|c| c.0),
                            }
                        })
                        .collect(),
                    tally: p.tally,
                    in_flight: p.in_flight.len() as u32,
                }
            })
            .collect(),
    }
}
