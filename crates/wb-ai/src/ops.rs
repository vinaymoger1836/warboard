//! Enemy AI v2 (doc 03 §8, §11, ADR-013, ADR-049): the AI plans its own
//! multi-phase operations with the same phase-graph model a player uses.
//!
//! **Objectives** come from the side's own picture: identified hostile ships,
//! ground forces and air-defence sites, scored by doctrine weights (how much it
//! values the kind of target, aggression, size, distance from its bases).
//!
//! **Plan** for the best objective, as deep as the difficulty coordinates:
//!
//! | Phase | What | Starts |
//! |---|---|---|
//! | `decoy` | unarmed drones fly at the edge of the SAM envelope, so its radars light up | at H |
//! | `jam` | stand-off jammer orbit short of the envelope | at H |
//! | `sead` | emitter-homing aircraft suppress the SAM that covers the objective | after the decoys / jammer are in place |
//! | `strike` | strikers attack the objective | after `sead` has started, delayed so they arrive a few minutes behind its missiles (or when `sead` has ended) |
//! | `escort` | fighters cover the strikers | with `strike` |
//!
//! Timing comes from distances and cruise speeds of the groups in the picture.
//! A strike is never planned through a known SAM envelope without suppression:
//! with no suppression group free it is postponed, unless the doctrine's risk
//! tolerance is high.
//!
//! The groups of an operation stay reserved until it ends; a watchdog aborts an
//! operation that runs too long. Everything is deterministic: candidate order
//! is by score then id, groups by distance then id.

use std::collections::BTreeSet;
use wb_core::UnitId;
use wb_core::geo::{LatLon, destination_point, distance_m, initial_bearing};

use crate::brain::{AiOp, AiState, Posture, free, km, nearest_free, rounds_for};
use crate::picture::{
    Action, AiPicture, Decision, Difficulty, Domain, Layer, OpPlan, OpStatus, OwnGroup, PlanAction,
    PlanCond, PlanPhase, PlanTask, Role, Seen,
};

/// No operation is planned before the picture has had time to build.
const WARMUP_MS: u64 = 10 * 60_000;
/// Doctrine aggression below this plans no offensive operations.
const MIN_AGGRESSION: f64 = 0.35;
/// Objectives farther than this from every air base are not considered.
const MAX_OBJECTIVE_M: f64 = 900_000.0;
/// The same objective is not planned against again for this long.
const RETRY_MS: u64 = 2 * 3_600_000;
/// An operation still running after this long is aborted by its planner.
const OP_MAX_MS: u64 = 3 * 3_600_000;
/// A plan the engine never shows is forgotten after this long (refused).
const PENDING_MS: u64 = 60_000;
/// Longest the AI waits for an objective and the ground contacts around it to
/// be identified before it plans on what it has.
const INTEL_WAIT_MS: u64 = 45 * 60_000;
/// Untyped ground or sea contacts this close to an objective may be air defences.
const UNKNOWN_RADIUS_M: f64 = 100_000.0;
/// A SAM covers an objective when it is within its threat radius plus this.
const COVER_MARGIN_M: f64 = 30_000.0;
/// Missiles are released about this far out; timing counts the approach to there.
const LAUNCH_M: f64 = 80_000.0;
/// Strikers arrive this long after the suppression missiles.
const SEAD_LEAD_MIN: f64 = 5.0;
const JAM_RADIUS_M: f64 = 20_000.0;
const JAM_MARGIN_M: f64 = 30_000.0;
const DECOY_FRACTION: f64 = 0.9;
const ESCORT_RADIUS_M: f64 = 30_000.0;
/// Cruise speed assumed for a group that reports none.
const DEFAULT_SPEED_MPS: f64 = 200.0;

/// What the difficulty coordinates (doc 03 §11: coordination depth, never information).
struct Depth {
    max_ops: usize,
    cooldown_ms: u64,
    strike_groups: usize,
    sead_groups: usize,
}

fn depth(d: Difficulty) -> Depth {
    match d {
        Difficulty::Easy => Depth {
            max_ops: 1,
            cooldown_ms: 75 * 60_000,
            strike_groups: 1,
            sead_groups: 1,
        },
        Difficulty::Normal => Depth {
            max_ops: 1,
            cooldown_ms: 40 * 60_000,
            strike_groups: 2,
            sead_groups: 1,
        },
        Difficulty::Hard => Depth {
            max_ops: 2,
            cooldown_ms: 20 * 60_000,
            strike_groups: 3,
            sead_groups: 2,
        },
    }
}

/// Keeps the operation list in step with the side's own operations: forgets
/// finished and refused ones, aborts the overdue, reserves the groups of the rest.
pub(crate) fn sync(
    p: &AiPicture,
    st: &mut AiState,
    out: &mut Vec<Decision>,
    busy: &mut BTreeSet<UnitId>,
) {
    let now = p.now_ms;
    st.ops.retain(|op| {
        match p.operations.iter().find(|o| o.name == op.name) {
            // Not (yet) in the list: still being created, or refused.
            None => now.saturating_sub(op.created_ms) <= PENDING_MS,
            Some(o) => !o.status.is_over(),
        }
    });
    for op in &st.ops {
        busy.extend(op.groups.iter().copied());
        let Some(o) = p.operations.iter().find(|o| o.name == op.name) else {
            continue;
        };
        let overdue = o.status == OpStatus::Running
            && o.started_ms
                .is_some_and(|s| now.saturating_sub(s) > OP_MAX_MS);
        if overdue {
            out.push(Decision {
                layer: Layer::Operational,
                action: Some(Action::AbortOperation { operation: o.id }),
                why: format!(
                    "{} has run over {} h: aborted, its groups go home",
                    o.name,
                    OP_MAX_MS / 3_600_000
                ),
            });
        }
    }
}

/// The groups' cruise speed, with a floor for groups that report none.
fn speed(g: &OwnGroup) -> f64 {
    if g.speed_mps > 50.0 {
        g.speed_mps
    } else {
        DEFAULT_SPEED_MPS
    }
}

/// Minutes for the group to fly from where it is to within launch range of `to`.
fn eta_min(g: &OwnGroup, to: LatLon) -> f64 {
    ((distance_m(g.at, to) - LAUNCH_M).max(0.0) / speed(g)) / 60.0
}

/// Rounds to half a minute, never negative.
fn delay(min: f64) -> f64 {
    ((min.max(0.0)) * 2.0).round() / 2.0
}

fn started(phase: &str, delay_min: f64) -> PlanCond {
    PlanCond::PhaseStarted {
        phase: phase.to_owned(),
        delay_min: delay(delay_min),
    }
}

fn ended(phase: &str) -> PlanCond {
    PlanCond::PhaseEnded {
        phase: phase.to_owned(),
        delay_min: 0.0,
    }
}

/// How much the doctrine values hitting this contact, before distance.
fn kind_weight(c: &Seen) -> f64 {
    if c.threat_m > 0.0 {
        // Rolling back the air defences opens everything else.
        1.0
    } else if c.domain == Domain::Surface {
        0.8
    } else {
        0.6
    }
}

/// A candidate objective with its score.
struct Objective<'a> {
    seen: &'a Seen,
    score: f64,
    from_base_m: f64,
}

fn objectives<'a>(p: &'a AiPicture, st: &AiState) -> Vec<Objective<'a>> {
    let mut v: Vec<Objective> = p
        .contacts
        .iter()
        .filter(|c| {
            c.hostile
                && !c.weapon
                && !c.stale
                && matches!(c.domain, Domain::Surface | Domain::Ground)
                && st
                    .planned
                    .get(&c.id)
                    .is_none_or(|t| p.now_ms.saturating_sub(*t) >= RETRY_MS)
        })
        .filter_map(|c| {
            let from_base_m = p
                .bases
                .iter()
                .filter(|b| b.air)
                .map(|b| distance_m(b.at, c.at))
                .min_by(f64::total_cmp)?;
            (from_base_m <= MAX_OBJECTIVE_M).then(|| {
                let mass = 1.0 + 0.1 * f64::from(c.count_max.clamp(1, 10));
                let appetite = 1.0 + p.doctrine.aggression.clamp(0.0, 1.0);
                Objective {
                    seen: c,
                    score: kind_weight(c) * mass * appetite / (1.0 + from_base_m / 300_000.0),
                    from_base_m,
                }
            })
        })
        .collect();
    v.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.seen.id.cmp(&b.seen.id)));
    v
}

/// One planning step of the operational layer: at most one new operation.
pub(crate) fn plan(
    p: &AiPicture,
    st: &mut AiState,
    out: &mut Vec<Decision>,
    busy: &mut BTreeSet<UnitId>,
) {
    let d = depth(p.difficulty);
    if st.posture == Posture::Defend
        || p.doctrine.aggression < MIN_AGGRESSION
        || p.now_ms < WARMUP_MS
        || p.now_ms < st.next_op_ms
        || st.ops.len() >= d.max_ops
    {
        return;
    }
    for obj in objectives(p, st).into_iter().take(3) {
        let held = intel_gap(p, &obj).and_then(|why| {
            let since = *st.postponed.entry(obj.seen.id).or_insert(p.now_ms);
            let first = since == p.now_ms;
            (p.now_ms.saturating_sub(since) < INTEL_WAIT_MS).then_some((why, first))
        });
        if let Some((why, first)) = held {
            if first {
                out.push(Decision {
                    layer: Layer::Operational,
                    action: None,
                    why,
                });
            }
            continue;
        }
        match build(p, st, &d, &obj, busy) {
            Ok((plan, groups, why)) => {
                for g in &groups {
                    busy.insert(*g);
                }
                st.postponed.remove(&obj.seen.id);
                st.planned.insert(obj.seen.id, p.now_ms);
                st.next_op_ms = p.now_ms + d.cooldown_ms;
                st.op_serial += 1;
                st.ops.push(AiOp {
                    name: plan.name.clone(),
                    objective: obj.seen.id,
                    groups,
                    created_ms: p.now_ms,
                });
                out.push(Decision {
                    layer: Layer::Operational,
                    action: Some(Action::Operation(plan)),
                    why,
                });
                return;
            }
            Err(why) => {
                // Said once per objective, not every 30 s.
                if st.postponed.insert(obj.seen.id, p.now_ms).is_none() {
                    out.push(Decision {
                        layer: Layer::Operational,
                        action: None,
                        why,
                    });
                }
            }
        }
    }
}

/// What the AI still wants to know before it plans against an objective: its
/// type, and what the untyped contacts around it are (one may be a SAM).
fn intel_gap(p: &AiPicture, obj: &Objective) -> Option<String> {
    let c = obj.seen;
    if !c.typed {
        return Some(format!(
            "Objective C{} ({}) not yet identified to its type: waiting for more intelligence",
            c.id.0,
            what(c)
        ));
    }
    let unknown = p.contacts.iter().find(|s| {
        s.hostile
            && !s.stale
            && !s.typed
            && !s.weapon
            && s.domain != Domain::Air
            && s.id != c.id
            && distance_m(s.at, c.at) <= UNKNOWN_RADIUS_M
    })?;
    Some(format!(
        "Operation against C{} held: C{} nearby is not yet identified and may be an air-defence site",
        c.id.0, unknown.id.0
    ))
}

/// The SAM-like contacts that cover an objective, nearest first.
fn covering<'a>(p: &'a AiPicture, obj: &Seen) -> Vec<&'a Seen> {
    let mut v: Vec<&Seen> = p
        .contacts
        .iter()
        .filter(|s| {
            s.hostile
                && !s.stale
                && s.threat_m > 0.0
                && s.id != obj.id
                && distance_m(s.at, obj.at) <= s.threat_m + COVER_MARGIN_M
        })
        .collect();
    v.sort_by(|a, b| {
        distance_m(a.at, obj.at)
            .total_cmp(&distance_m(b.at, obj.at))
            .then(a.id.cmp(&b.id))
    });
    v
}

/// Takes up to `n` free groups nearest `at` that fit, each once.
fn take<'a>(
    p: &'a AiPicture,
    taken: &mut BTreeSet<UnitId>,
    at: LatLon,
    n: usize,
    fits: impl Fn(&OwnGroup) -> bool,
) -> Vec<&'a OwnGroup> {
    let mut v = Vec::new();
    for _ in 0..n {
        let Some(g) = nearest_free(p, taken, at, &fits) else {
            break;
        };
        taken.insert(g.id);
        v.push(g);
    }
    v
}

fn ids(v: &[&OwnGroup]) -> Vec<UnitId> {
    v.iter().map(|g| g.id).collect()
}

/// The slowest group's time to launch range of `to` (the package waits for it).
fn slowest_eta(v: &[&OwnGroup], to: LatLon) -> f64 {
    v.iter().map(|g| eta_min(g, to)).fold(0.0, f64::max)
}

#[allow(clippy::too_many_lines)]
fn build(
    p: &AiPicture,
    st: &AiState,
    d: &Depth,
    obj: &Objective,
    busy: &BTreeSet<UnitId>,
) -> Result<(OpPlan, Vec<UnitId>, String), String> {
    let c = obj.seen;
    let tmpl = |e: &str| p.doctrine.strike_package.iter().any(|x| x == e);
    let hard = p.difficulty == Difficulty::Hard;
    let normal = p.difficulty == Difficulty::Normal;
    let mut taken = busy.clone();

    // Fighters beyond the CAP stations may fly escort or strike.
    let free_fighters = p
        .groups
        .iter()
        .filter(|g| free(p, g, &taken) && g.role == Role::Fighter && g.air_rounds > 0)
        .count();
    let spare_fighters = free_fighters.saturating_sub(st.cap_stations as usize);

    // The strike: Strike-role aircraft first, spare fighters if need be.
    let want = d
        .strike_groups
        .min(1 + usize::from(c.count_max >= 4) + usize::from(c.count_max >= 8));
    let striker_fit = |g: &OwnGroup| {
        g.domain == Domain::Air && g.role == Role::Strike && rounds_for(g, c.domain) > 0
    };
    let mut strikers = take(p, &mut taken, c.at, want, striker_fit);
    if strikers.len() < want && spare_fighters > 0 {
        let more = take(
            p,
            &mut taken,
            c.at,
            (want - strikers.len()).min(spare_fighters),
            |g| g.domain == Domain::Air && g.role == Role::Fighter && rounds_for(g, c.domain) > 0,
        );
        strikers.extend(more);
    }
    if strikers.is_empty() {
        return Err(format!(
            "No aircraft free to strike C{} ({}): objective kept for later",
            c.id.0,
            what(c)
        ));
    }

    // Suppression: what covers the objective, or the objective itself.
    let mut targets: Vec<&Seen> = covering(p, c);
    if c.threat_m > 0.0 {
        targets.insert(0, c);
    }
    targets.truncate(d.sead_groups);
    let mut sead: Vec<(&OwnGroup, &Seen)> = Vec::new();
    for s in &targets {
        if let Some(g) = nearest_free(p, &taken, s.at, |g| {
            g.domain == Domain::Air && g.arm_rounds > 0
        }) {
            taken.insert(g.id);
            sead.push((g, s));
        }
    }
    if !targets.is_empty() && sead.is_empty() && p.doctrine.risk_tolerance < 0.7 {
        return Err(format!(
            "Strike on C{} postponed: C{} covers it and no aircraft with emitter-homing missiles is free",
            c.id.0, targets[0].id.0
        ));
    }

    let primary = targets.first().copied();
    let from = strikers[0].at;
    // Jammer, decoys and escort as the doctrine and the difficulty allow.
    let jammer = if primary.is_some() && (hard || (normal && tmpl("jammer"))) {
        nearest_free(p, &taken, from, |g| g.role == Role::Jammer)
    } else {
        None
    };
    if let Some(g) = jammer {
        taken.insert(g.id);
    }
    let decoys: Vec<&OwnGroup> = if primary.is_some() && hard {
        take(p, &mut taken, from, 1, |g| {
            g.role == Role::Recon && g.domain == Domain::Air
        })
    } else {
        Vec::new()
    };
    let escorts: Vec<&OwnGroup> = if (hard || (normal && tmpl("escort"))) && spare_fighters > 0 {
        take(p, &mut taken, from, 1.min(spare_fighters), |g| {
            g.role == Role::Fighter && g.air_rounds > 0
        })
    } else {
        Vec::new()
    };

    // ---- timing -----------------------------------------------------------
    let eta_strike = slowest_eta(&strikers, c.at);
    let eta_sead = sead
        .iter()
        .map(|(g, s)| eta_min(g, s.at))
        .fold(0.0, f64::max);
    let eta_jam = jammer.map_or(0.0, |g| eta_min(g, primary.map_or(c.at, |s| s.at)));
    let eta_decoy = decoys
        .iter()
        .map(|g| eta_min(g, primary.map_or(c.at, |s| s.at)))
        .fold(0.0, f64::max);

    let mut phases: Vec<PlanPhase> = Vec::new();
    let mut parts: Vec<String> = Vec::new();
    let mut groups: Vec<UnitId> = Vec::new();

    // The bearing from the threat back towards our side, for stand-off points.
    let back = primary.map(|s| initial_bearing(s.at, from));

    if let (Some(s), Some(b)) = (primary, back)
        && !decoys.is_empty()
    {
        let at = destination_point(s.at, b, s.threat_m * DECOY_FRACTION);
        phases.push(PlanPhase {
            id: "decoy".to_owned(),
            name: "Decoys".to_owned(),
            tasks: vec![PlanTask {
                groups: ids(&decoys),
                action: PlanAction::Move {
                    waypoints: vec![at],
                },
            }],
            trigger: PlanCond::Start { delay_min: 0.0 },
            end: None,
            abort: None,
        });
        groups.extend(ids(&decoys));
        parts.push(format!(
            "decoy drones to the edge of C{}'s envelope",
            s.id.0
        ));
    }
    if let (Some(g), Some(s), Some(b)) = (jammer, primary, back) {
        let centre = destination_point(s.at, b, s.threat_m + JAM_MARGIN_M);
        phases.push(PlanPhase {
            id: "jam".to_owned(),
            name: "Blind them".to_owned(),
            tasks: vec![PlanTask {
                groups: vec![g.id],
                action: PlanAction::Jam {
                    centre,
                    radius_m: JAM_RADIUS_M,
                },
            }],
            trigger: PlanCond::Start { delay_min: 0.0 },
            end: Some(PlanCond::Any(vec![
                ended("strike"),
                PlanCond::Elapsed { min: 120.0 },
            ])),
            abort: None,
        });
        groups.push(g.id);
        parts.push(format!("stand-off jamming short of C{}", s.id.0));
    }

    // Suppression starts once the decoys or the jammer will be in place.
    let sead_trigger = if !decoys.is_empty() {
        started("decoy", eta_decoy - eta_sead)
    } else if jammer.is_some() {
        started("jam", eta_jam - eta_sead)
    } else {
        PlanCond::Start { delay_min: 0.0 }
    };
    let sead_lead_in = match &sead_trigger {
        PlanCond::PhaseStarted { delay_min, .. } => *delay_min,
        _ => 0.0,
    };
    if !sead.is_empty() {
        phases.push(PlanPhase {
            id: "sead".to_owned(),
            name: "Suppress the air defences".to_owned(),
            tasks: sead
                .iter()
                .map(|(g, s)| PlanTask {
                    groups: vec![g.id],
                    action: PlanAction::Suppress { contact: s.id },
                })
                .collect(),
            trigger: sead_trigger,
            end: Some(PlanCond::Any(vec![
                PlanCond::WeaponsDone,
                PlanCond::Elapsed { min: 60.0 },
            ])),
            abort: None,
        });
        groups.extend(sead.iter().map(|(g, _)| g.id));
        parts.push(format!(
            "suppress {}",
            sead.iter()
                .map(|(_, s)| format!("C{}", s.id.0))
                .collect::<Vec<_>>()
                .join(" and ")
        ));
    }

    // The strike follows the suppression by the lead time, or the jammer.
    let (strike_trigger, behind_min) = if !sead.is_empty() {
        let wait = eta_sead + SEAD_LEAD_MIN - eta_strike;
        (
            PlanCond::Any(vec![started("sead", wait), ended("sead")]),
            sead_lead_in.max(0.0) + delay(wait),
        )
    } else if jammer.is_some() {
        (started("jam", eta_jam - eta_strike), 0.0)
    } else {
        (PlanCond::Start { delay_min: 0.0 }, 0.0)
    };
    let abort_pct = 40.0 + 40.0 * p.doctrine.risk_tolerance.clamp(0.0, 1.0);
    phases.push(PlanPhase {
        id: "strike".to_owned(),
        name: format!("Strike {}", what(c)),
        tasks: vec![PlanTask {
            groups: ids(&strikers),
            action: PlanAction::Strike { contact: c.id },
        }],
        trigger: strike_trigger,
        end: Some(PlanCond::Any(vec![
            PlanCond::WeaponsDone,
            PlanCond::Elapsed { min: 90.0 },
        ])),
        abort: Some(PlanCond::LossesPct { pct: abort_pct }),
    });
    groups.extend(ids(&strikers));
    parts.push(format!("strike C{}", c.id.0));

    if !escorts.is_empty() {
        let mid = destination_point(
            c.at,
            initial_bearing(c.at, from),
            distance_m(c.at, from) / 2.0,
        );
        phases.push(PlanPhase {
            id: "escort".to_owned(),
            name: "Escort".to_owned(),
            tasks: vec![PlanTask {
                groups: ids(&escorts),
                action: PlanAction::Cap {
                    centre: mid,
                    radius_m: ESCORT_RADIUS_M,
                    commit_m: ESCORT_RADIUS_M * 3.0,
                },
            }],
            trigger: started("strike", 0.0),
            end: Some(PlanCond::Any(vec![
                PlanCond::PhaseEnded {
                    phase: "strike".to_owned(),
                    delay_min: 4.0,
                },
                PlanCond::Elapsed { min: 90.0 },
            ])),
            abort: None,
        });
        groups.extend(ids(&escorts));
        parts.push("escort over the ingress".to_owned());
    }

    let serial = st.op_serial + 1;
    let name = format!(
        "Operation {serial}: {} C{}",
        if c.threat_m > 0.0 {
            "suppress"
        } else {
            "strike"
        },
        c.id.0
    );
    let why = format!(
        "{name}: objective C{} ({}, {} from our bases) scores {:.2}; {} phases: {}{}",
        c.id.0,
        what(c),
        km(obj.from_base_m),
        obj.score,
        phases.len(),
        parts.join(", then "),
        if !sead.is_empty() {
            format!("; strikers follow the suppression by {behind_min:.1} min")
        } else {
            String::new()
        }
    );
    groups.sort();
    groups.dedup();
    Ok((OpPlan { name, phases }, groups, why))
}

/// A short plain description of an objective.
fn what(c: &Seen) -> &'static str {
    if c.threat_m > 0.0 {
        "air-defence site"
    } else if c.domain == Domain::Surface {
        "ship"
    } else {
        "ground force"
    }
}
