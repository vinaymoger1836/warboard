//! Enemy AI v1 (doc 03 §11, ADR-015): three layers over the side's picture.
//!
//! - **Strategic** (every 5 min): posture (defend, hold, press) from the
//!   doctrine's aggression and the air balance the picture shows, and how many
//!   fighter groups fly CAP.
//! - **Operational** (every 30 s): CAP stations over the air bases nearest the
//!   threat, relieved after the doctrine's rotation time; the IADS emitters on
//!   when hostile aircraft come near, back to the doctrine's discipline when
//!   they leave; satellite imaging of where the enemy was last seen.
//! - **Every second**: threats. A hostile contact inside the side's territory
//!   is an incursion; one that has fired at the side's forces is a shooter. Each
//!   gets a response once the reaction delay has passed (doctrine reaction
//!   time plus the difficulty's delay): scramble or strike for incursions, a
//!   strike package from the doctrine's template for shooters (retaliation).
//!   Approval requests are decided at once.
//!
//! Tactical behaviour (TEWA, evasion, launch decisions) is the engine's own and
//! the same for every side. Difficulty changes reaction delay and how much of
//! the strike package is flown, never what the AI knows.
//!
//! Deterministic: iteration in `UnitId` / `ContactId` order (the picture comes
//! sorted, the state uses ordered maps), maths through `wb_core::math`, random
//! draws only through `wb_core::rng`.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use wb_core::geo::{LatLon, destination_point, distance_m, initial_bearing};
use wb_core::{ContactId, RngPurpose, UnitId, rng};

use crate::ops;
use crate::picture::{
    Action, Activity, AiPicture, Decision, Difficulty, Domain, Emcon, Layer, OwnGroup, Role, Seen,
};

pub const STRATEGIC_MS: u64 = 300_000;
pub const OPERATIONAL_MS: u64 = 30_000;
/// Satellite imaging of the last-known enemy area, at most this often.
const IMAGING_MS: u64 = 2 * 3_600_000;
/// CAP station: patrol circle radius and how far ahead of its base it sits.
const CAP_RADIUS_M: f64 = 30_000.0;
const CAP_FORWARD_M: f64 = 40_000.0;
/// Hostile aircraft this close to a base put the IADS on alert.
const IADS_ALERT_M: f64 = 250_000.0;
/// A SAM this close to a strike target is suppressed first.
const SEAD_REACH_M: f64 = 150_000.0;
const ESCORT_RADIUS_M: f64 = 30_000.0;
const JAM_STANDOFF_M: f64 = 80_000.0;
const JAM_RADIUS_M: f64 = 20_000.0;
const IMAGE_RADIUS_M: f64 = 50_000.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Posture {
    Defend,
    #[default]
    Hold,
    Press,
}

impl Posture {
    fn as_str(self) -> &'static str {
        match self {
            Posture::Defend => "defend",
            Posture::Hold => "hold",
            Posture::Press => "press",
        }
    }

    /// Share of the fighter groups that fly CAP.
    fn cap_share(self) -> f64 {
        match self {
            Posture::Defend => 0.6,
            Posture::Hold => 0.4,
            Posture::Press => 0.3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThreatKind {
    /// Inside our territory.
    Incursion,
    /// Fired at our forces.
    Shooter,
}

/// A threat the AI has noticed and when it answers it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Threat {
    pub kind: ThreatKind,
    /// When the AI first saw it as a threat.
    pub since_ms: u64,
    /// Not before: since + reaction delay.
    pub respond_at_ms: u64,
    /// The groups sent, once answered.
    pub responders: Vec<UnitId>,
    pub answered: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Station {
    pub centre: LatLon,
    pub group: Option<UnitId>,
    pub since_ms: u64,
}

/// What the AI of one side remembers. Saved with the war and part of its
/// state hash (it decides what the side does next).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AiState {
    pub next_strategic_ms: u64,
    pub next_operational_ms: u64,
    pub posture: Posture,
    pub cap_stations: u32,
    pub stations: Vec<Station>,
    pub threats: BTreeMap<ContactId, Threat>,
    /// Shooters already answered with a package.
    pub retaliated: BTreeSet<ContactId>,
    /// The IADS emitters are on for an alert (`None` until first set).
    pub iads_alert: Option<bool>,
    pub next_imaging_ms: u64,
    pub imaging_turn: u32,
    /// Decisions taken so far.
    pub decisions: u64,
    /// Operations the AI planned and has not seen end (v2).
    pub ops: Vec<AiOp>,
    /// Operations planned so far; numbers the next one.
    pub op_serial: u32,
    /// Not before: the earliest the next operation may be planned.
    pub next_op_ms: u64,
    /// Objectives already planned against, and when.
    pub planned: BTreeMap<ContactId, u64>,
    /// Objectives the AI is holding back on, and since when (logged once).
    pub postponed: BTreeMap<ContactId, u64>,
}

/// An operation the AI planned: what it targets and the groups it holds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AiOp {
    /// Unique name, how the AI finds it again in its own operations list.
    pub name: String,
    pub objective: ContactId,
    /// Reserved until the operation ends.
    pub groups: Vec<UnitId>,
    pub created_ms: u64,
}

/// The reaction delay for a contact: the doctrine's reaction time plus the
/// difficulty's (doc 03 §11: Easy 60–120 s, Normal 30 s, Hard 10 s).
pub fn reaction_delay_ms(p: &AiPicture, contact: ContactId) -> u64 {
    let extra = match p.difficulty {
        Difficulty::Hard => 10.0,
        Difficulty::Normal => 30.0,
        Difficulty::Easy => {
            let entity = (u64::from(p.side) << 32) | u64::from(contact.0);
            60.0 + 60.0 * rng(p.seed, RngPurpose::Ai, entity, 0, 0)
        }
    };
    ((p.doctrine.reaction_time_s.max(0.0) + extra) * 1_000.0) as u64
}

/// How much of the strike package the difficulty flies.
fn package_depth(d: Difficulty) -> usize {
    match d {
        Difficulty::Easy => 1,
        Difficulty::Normal => 2,
        Difficulty::Hard => 8,
    }
}

/// Most CAP stations the difficulty coordinates.
fn max_stations(d: Difficulty) -> u32 {
    match d {
        Difficulty::Easy => 2,
        Difficulty::Normal => 4,
        Difficulty::Hard => 8,
    }
}

/// One step of the AI. Call every sim second; returns what to do, each with why.
pub fn think(p: &AiPicture, st: &mut AiState) -> Vec<Decision> {
    let mut out = Vec::new();
    let mut busy: BTreeSet<UnitId> = BTreeSet::new();
    ops::sync(p, st, &mut out, &mut busy);
    approvals(p, st, &mut out);
    if p.now_ms >= st.next_strategic_ms {
        strategic(p, st, &mut out);
        st.next_strategic_ms = p.now_ms + STRATEGIC_MS;
    }
    track_threats(p, st);
    respond(p, st, &mut out, &mut busy);
    if p.now_ms >= st.next_operational_ms {
        operational(p, st, &mut out, &mut busy);
        st.next_operational_ms = p.now_ms + OPERATIONAL_MS;
    }
    st.decisions += out.len() as u64;
    out
}

fn contact(p: &AiPicture, id: ContactId) -> Option<&Seen> {
    p.contacts.iter().find(|c| c.id == id)
}

fn group(p: &AiPicture, id: UnitId) -> Option<&OwnGroup> {
    p.groups.iter().find(|g| g.id == id)
}

/// Lowest fuel a group needs to be sent: more caution with less risk tolerance.
fn min_fuel(p: &AiPicture) -> f64 {
    0.4 + 0.3 * (1.0 - p.doctrine.risk_tolerance.clamp(0.0, 1.0))
}

/// Free to take a new task: at base and turned round, or out with nothing to do.
pub(crate) fn free(p: &AiPicture, g: &OwnGroup, busy: &BTreeSet<UnitId>) -> bool {
    !busy.contains(&g.id)
        && g.fuel >= min_fuel(p)
        && matches!(
            g.activity,
            Activity::AtBase { ready: true } | Activity::Idle
        )
}

/// The free group nearest `at` that `fits`.
pub(crate) fn nearest_free<'a>(
    p: &'a AiPicture,
    busy: &BTreeSet<UnitId>,
    at: LatLon,
    fits: impl Fn(&OwnGroup) -> bool,
) -> Option<&'a OwnGroup> {
    p.groups
        .iter()
        .filter(|g| free(p, g, busy) && fits(g))
        .map(|g| (distance_m(g.at, at), g))
        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.id.cmp(&b.1.id)))
        .map(|(_, g)| g)
}

pub(crate) fn km(m: f64) -> String {
    format!("{:.0} km", m / 1_000.0)
}

// ---------------------------------------------------------------------------
// Approvals
// ---------------------------------------------------------------------------

fn approvals(p: &AiPicture, st: &AiState, out: &mut Vec<Decision>) {
    for r in &p.requests {
        let c = contact(p, r.contact);
        let threat = c.is_some_and(|c| c.hostile && (c.in_territory || c.fired_at_us));
        let willing = st.posture == Posture::Press || p.doctrine.aggression >= 0.5;
        let approve = c.is_some_and(|c| c.hostile) && (threat || willing);
        let why = match (approve, threat) {
            (true, true) => format!(
                "Approve C{}: it is in our territory or fired at us",
                r.contact.0
            ),
            (true, false) => format!(
                "Approve C{}: doctrine allows engaging hostiles",
                r.contact.0
            ),
            (false, _) => format!("Deny C{}: not a threat under our doctrine", r.contact.0),
        };
        out.push(Decision {
            layer: Layer::Tactical,
            action: Some(Action::Decide {
                request: r.id,
                approve,
            }),
            why,
        });
    }
}

// ---------------------------------------------------------------------------
// Strategic
// ---------------------------------------------------------------------------

fn strategic(p: &AiPicture, st: &mut AiState, out: &mut Vec<Decision>) {
    let fighters = p.groups.iter().filter(|g| g.role == Role::Fighter).count() as u32;
    let hostile_air: u32 = p
        .contacts
        .iter()
        .filter(|c| c.hostile && !c.weapon && !c.stale && c.domain == Domain::Air)
        .map(|c| u32::from(c.count_max.max(1)))
        .sum();
    let ratio = f64::from(hostile_air) / f64::from(fighters.max(1) * 2);
    let a = p.doctrine.aggression;
    let posture = if a < 0.3 || ratio > 1.5 {
        Posture::Defend
    } else if a >= 0.6 && ratio < 0.7 {
        Posture::Press
    } else {
        Posture::Hold
    };
    let stations = if fighters == 0 {
        0
    } else {
        ((f64::from(fighters) * posture.cap_share()).round() as u32)
            .clamp(1, max_stations(p.difficulty))
    };
    if posture != st.posture || stations != st.cap_stations || st.decisions == 0 {
        out.push(Decision {
            layer: Layer::Strategic,
            action: None,
            why: format!(
                "Posture {}: {} hostile aircraft seen against {} fighter groups (aggression {:.1}); {} CAP station{}",
                posture.as_str(),
                hostile_air,
                fighters,
                a,
                stations,
                if stations == 1 { "" } else { "s" }
            ),
        });
    }
    st.posture = posture;
    st.cap_stations = stations;
}

// ---------------------------------------------------------------------------
// Threats
// ---------------------------------------------------------------------------

fn track_threats(p: &AiPicture, st: &mut AiState) {
    let live: BTreeSet<ContactId> = p
        .contacts
        .iter()
        .filter(|c| c.hostile && !c.weapon && !c.stale)
        .map(|c| c.id)
        .collect();
    st.threats.retain(|id, _| live.contains(id));
    for c in &p.contacts {
        if !live.contains(&c.id) {
            continue;
        }
        let kind = if c.in_territory {
            ThreatKind::Incursion
        } else if c.fired_at_us && p.doctrine.aggression >= 0.2 && !st.retaliated.contains(&c.id) {
            ThreatKind::Shooter
        } else {
            continue;
        };
        match st.threats.get_mut(&c.id) {
            None => {
                st.threats.insert(
                    c.id,
                    Threat {
                        kind,
                        since_ms: p.now_ms,
                        respond_at_ms: p.now_ms + reaction_delay_ms(p, c.id),
                        responders: Vec::new(),
                        answered: false,
                    },
                );
            }
            Some(t) => {
                // A responder that is no longer on the job leaves the threat open again.
                if t.answered
                    && kind == ThreatKind::Incursion
                    && !t.responders.iter().any(|r| {
                        group(p, *r).is_some_and(|g| {
                            matches!(
                                g.activity,
                                Activity::Intercept | Activity::Strike | Activity::Suppress
                            )
                        })
                    })
                {
                    t.answered = false;
                    t.responders.clear();
                    t.respond_at_ms = p.now_ms + reaction_delay_ms(p, c.id);
                }
            }
        }
    }
}

/// Weapons a group has for a domain.
pub(crate) fn rounds_for(g: &OwnGroup, d: Domain) -> u32 {
    match d {
        Domain::Air => g.air_rounds,
        Domain::Surface | Domain::Subsurface => g.surface_rounds,
        Domain::Ground => g.ground_rounds,
        Domain::Space => 0,
    }
}

fn respond(p: &AiPicture, st: &mut AiState, out: &mut Vec<Decision>, busy: &mut BTreeSet<UnitId>) {
    let due: Vec<ContactId> = st
        .threats
        .iter()
        .filter(|(_, t)| !t.answered && p.now_ms >= t.respond_at_ms)
        .map(|(id, _)| *id)
        .collect();
    for id in due {
        let Some(c) = contact(p, id) else { continue };
        let t = st.threats.get(&id).cloned().expect("listed");
        let waited_s = (p.now_ms.saturating_sub(t.since_ms)) as f64 / 1_000.0;
        let responders = match t.kind {
            ThreatKind::Incursion => scramble(p, c, waited_s, out, busy),
            ThreatKind::Shooter => {
                st.retaliated.insert(id);
                package(p, c, waited_s, out, busy)
            }
        };
        if responders.is_empty() {
            out.push(Decision {
                layer: Layer::Operational,
                action: None,
                why: format!(
                    "No group free to answer C{} ({}), seen {waited_s:.0} s ago",
                    id.0,
                    match t.kind {
                        ThreatKind::Incursion => "inside our territory",
                        ThreatKind::Shooter => "fired at us",
                    }
                ),
            });
        }
        if let Some(t) = st.threats.get_mut(&id) {
            t.answered = true;
            t.responders = responders;
        }
    }
}

/// Answers an incursion: interceptors against aircraft, strikers against
/// ships and ground forces. A large raid gets a second group on Hard.
fn scramble(
    p: &AiPicture,
    c: &Seen,
    waited_s: f64,
    out: &mut Vec<Decision>,
    busy: &mut BTreeSet<UnitId>,
) -> Vec<UnitId> {
    let wanted = if p.difficulty == Difficulty::Hard && c.count_max >= 3 {
        2
    } else {
        1
    };
    let mut sent = Vec::new();
    for _ in 0..wanted {
        let pick = if c.domain == Domain::Air {
            nearest_free(p, busy, c.at, |g| {
                g.role == Role::Fighter && g.air_rounds > 0
            })
        } else {
            nearest_free(p, busy, c.at, |g| {
                rounds_for(g, c.domain) > 0
                    && matches!(
                        g.role,
                        Role::Strike | Role::Fighter | Role::Ship | Role::Sead
                    )
            })
        };
        let Some(g) = pick else { break };
        busy.insert(g.id);
        sent.push(g.id);
        let action = if c.domain == Domain::Air {
            Action::Intercept {
                group: g.id,
                contact: c.id,
            }
        } else {
            Action::Strike {
                group: g.id,
                contact: c.id,
            }
        };
        out.push(Decision {
            layer: Layer::Operational,
            action: Some(action),
            why: format!(
                "Incursion: C{} inside our territory, answered after {waited_s:.0} s; group {} sent from {} away",
                c.id.0,
                g.id.0,
                km(distance_m(g.at, c.at))
            ),
        });
    }
    sent
}

/// Retaliation against a shooter with the doctrine's strike package, as deep
/// as the difficulty coordinates. The strike itself is always flown first.
fn package(
    p: &AiPicture,
    c: &Seen,
    waited_s: f64,
    out: &mut Vec<Decision>,
    busy: &mut BTreeSet<UnitId>,
) -> Vec<UnitId> {
    let striker = if c.domain == Domain::Air {
        nearest_free(p, busy, c.at, |g| {
            g.role == Role::Fighter && g.air_rounds > 0
        })
    } else {
        nearest_free(p, busy, c.at, |g| {
            rounds_for(g, c.domain) > 0
                && matches!(g.role, Role::Strike | Role::Fighter | Role::Ship)
        })
    };
    let Some(striker) = striker else {
        return Vec::new();
    };
    let from = striker.at;
    busy.insert(striker.id);
    let mut sent = vec![striker.id];
    out.push(Decision {
        layer: Layer::Operational,
        action: Some(if c.domain == Domain::Air {
            Action::Intercept {
                group: striker.id,
                contact: c.id,
            }
        } else {
            Action::Strike {
                group: striker.id,
                contact: c.id,
            }
        }),
        why: format!(
            "Retaliation: C{} fired at our forces; group {} strikes it ({} away, after {waited_s:.0} s)",
            c.id.0,
            striker.id.0,
            km(distance_m(from, c.at))
        ),
    });
    let template: Vec<&str> = p
        .doctrine
        .strike_package
        .iter()
        .map(String::as_str)
        .filter(|e| *e != "strike")
        .take(package_depth(p.difficulty).saturating_sub(1))
        .collect();
    let towards = initial_bearing(c.at, from);
    for element in template {
        match element {
            "escort" => {
                if let Some(g) = nearest_free(p, busy, from, |g| {
                    g.role == Role::Fighter && g.air_rounds > 0
                }) {
                    busy.insert(g.id);
                    sent.push(g.id);
                    let mid = destination_point(c.at, towards, distance_m(c.at, from) / 2.0);
                    out.push(Decision {
                        layer: Layer::Operational,
                        action: Some(Action::Cap {
                            group: g.id,
                            centre: mid,
                            radius_m: ESCORT_RADIUS_M,
                            commit_m: ESCORT_RADIUS_M * 3.0,
                        }),
                        why: format!(
                            "Package escort for the strike on C{}: group {}",
                            c.id.0, g.id.0
                        ),
                    });
                }
            }
            "sead" => {
                let sam = p
                    .contacts
                    .iter()
                    .filter(|s| s.hostile && s.threat_m > 0.0 && !s.stale)
                    .map(|s| (distance_m(s.at, c.at), s))
                    .filter(|(d, _)| *d <= SEAD_REACH_M)
                    .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.id.cmp(&b.1.id)))
                    .map(|(_, s)| s.id);
                if let Some(sam) = sam
                    && let Some(g) = nearest_free(p, busy, from, |g| g.arm_rounds > 0)
                {
                    busy.insert(g.id);
                    sent.push(g.id);
                    out.push(Decision {
                        layer: Layer::Operational,
                        action: Some(Action::Suppress {
                            group: g.id,
                            contact: sam,
                        }),
                        why: format!(
                            "Package air-defence suppression: group {} against C{} near the target",
                            g.id.0, sam.0
                        ),
                    });
                }
            }
            "jammer" => {
                if let Some(g) = nearest_free(p, busy, from, |g| g.role == Role::Jammer) {
                    busy.insert(g.id);
                    sent.push(g.id);
                    let stand = destination_point(c.at, towards, JAM_STANDOFF_M);
                    out.push(Decision {
                        layer: Layer::Operational,
                        action: Some(Action::Jam {
                            group: g.id,
                            centre: stand,
                            radius_m: JAM_RADIUS_M,
                        }),
                        why: format!(
                            "Package stand-off jamming: group {} {} short of C{}",
                            g.id.0,
                            km(JAM_STANDOFF_M),
                            c.id.0
                        ),
                    });
                }
            }
            _ => {}
        }
    }
    sent
}

// ---------------------------------------------------------------------------
// Operational
// ---------------------------------------------------------------------------

/// Centroid of the hostile ships and aircraft the side has seen.
fn threat_centroid(p: &AiPicture) -> Option<LatLon> {
    let pts: Vec<LatLon> = p
        .contacts
        .iter()
        .filter(|c| c.hostile && !c.weapon && matches!(c.domain, Domain::Air | Domain::Surface))
        .map(|c| c.at)
        .collect();
    if pts.is_empty() {
        return None;
    }
    let n = pts.len() as f64;
    Some(LatLon::new(
        pts.iter().map(|p| p.lat).sum::<f64>() / n,
        pts.iter().map(|p| p.lon).sum::<f64>() / n,
    ))
}

fn operational(
    p: &AiPicture,
    st: &mut AiState,
    out: &mut Vec<Decision>,
    busy: &mut BTreeSet<UnitId>,
) {
    iads(p, st, out);
    cap(p, st, out, busy);
    ops::plan(p, st, out, busy);
    imaging(p, st, out);
}

/// IADS emitters: on while hostile aircraft are near, else the doctrine's discipline.
fn iads(p: &AiPicture, st: &mut AiState, out: &mut Vec<Decision>) {
    let sites: Vec<UnitId> = p
        .groups
        .iter()
        .filter(|g| matches!(g.role, Role::AirDefence | Role::Radar))
        .map(|g| g.id)
        .collect();
    if sites.is_empty() {
        return;
    }
    let near = p.contacts.iter().find(|c| {
        c.hostile
            && !c.stale
            && c.domain == Domain::Air
            && (c.in_territory
                || p.bases
                    .iter()
                    .any(|b| distance_m(b.at, c.at) <= IADS_ALERT_M))
    });
    let alert = near.is_some();
    if st.iads_alert == Some(alert) {
        return;
    }
    st.iads_alert = Some(alert);
    let calm = if p.doctrine.emcon_discipline >= 0.5 {
        Emcon::Blink
    } else {
        Emcon::On
    };
    let (emcon, why) = match near {
        Some(c) => (
            Emcon::On,
            format!(
                "IADS alert: hostile aircraft C{} near our bases; radars on",
                c.id.0
            ),
        ),
        None => (
            calm,
            format!(
                "IADS stands down to doctrine emission control ({}, discipline {:.1})",
                if calm == Emcon::Blink { "blink" } else { "on" },
                p.doctrine.emcon_discipline
            ),
        ),
    };
    out.push(Decision {
        layer: Layer::Operational,
        action: Some(Action::Emcon {
            groups: sites,
            emcon,
        }),
        why,
    });
}

/// CAP stations over the air bases nearest the threat, manned and relieved.
fn cap(p: &AiPicture, st: &mut AiState, out: &mut Vec<Decision>, busy: &mut BTreeSet<UnitId>) {
    let centroid = threat_centroid(p);
    let mut airfields: Vec<_> = p.bases.iter().filter(|b| b.air).collect();
    if let Some(c) = centroid {
        airfields.sort_by(|a, b| {
            distance_m(a.at, c)
                .total_cmp(&distance_m(b.at, c))
                .then(a.id.cmp(&b.id))
        });
    }
    let n = if airfields.is_empty() {
        0
    } else {
        st.cap_stations as usize
    };
    st.stations.truncate(n);
    for i in 0..n {
        let base = airfields[i % airfields.len()];
        let lap = (i / airfields.len()) as f64;
        let bearing = match centroid {
            Some(c) => initial_bearing(base.at, c) + lap * 0.6,
            None => lap * 2.1,
        };
        let forward = if centroid.is_some() || lap > 0.0 {
            CAP_FORWARD_M
        } else {
            0.0
        };
        let centre = destination_point(base.at, bearing, forward);
        if st.stations.len() <= i {
            st.stations.push(Station {
                centre,
                group: None,
                since_ms: p.now_ms,
            });
        }
        st.stations[i].centre = centre;
    }
    let commit = CAP_RADIUS_M * (2.0 + 2.0 * p.doctrine.aggression.clamp(0.0, 1.0));
    let rotation_ms = (p.doctrine.cap_rotation_min.max(5.0) * 60_000.0) as u64;
    for (i, s) in st.stations.iter_mut().enumerate() {
        let manned = s
            .group
            .and_then(|g| group(p, g))
            .filter(|g| g.activity == Activity::Cap);
        if let Some(g) = manned {
            busy.insert(g.id);
            if p.now_ms.saturating_sub(s.since_ms) < rotation_ms {
                continue;
            }
        }
        let fresh = nearest_free(p, busy, s.centre, |g| {
            g.role == Role::Fighter && g.air_rounds > 0
        });
        let Some(fresh) = fresh else { continue };
        busy.insert(fresh.id);
        out.push(Decision {
            layer: Layer::Operational,
            action: Some(Action::Cap {
                group: fresh.id,
                centre: s.centre,
                radius_m: CAP_RADIUS_M,
                commit_m: commit,
            }),
            why: match manned {
                Some(old) => format!(
                    "CAP station {}: group {} relieves group {} after {:.0} min",
                    i + 1,
                    fresh.id.0,
                    old.id.0,
                    p.doctrine.cap_rotation_min
                ),
                None => format!("CAP station {}: group {} takes it", i + 1, fresh.id.0),
            },
        });
        if let Some(old) = manned {
            out.push(Decision {
                layer: Layer::Operational,
                action: Some(Action::Rtb { group: old.id }),
                why: format!("CAP station {}: group {} relieved, home", i + 1, old.id.0),
            });
        }
        s.group = Some(fresh.id);
        s.since_ms = p.now_ms;
    }
}

/// Images where the enemy was last seen, one satellite at a time (ADR-025).
fn imaging(p: &AiPicture, st: &mut AiState, out: &mut Vec<Decision>) {
    if p.now_ms < st.next_imaging_ms {
        return;
    }
    let sats: Vec<UnitId> = p
        .groups
        .iter()
        .filter(|g| g.role == Role::Satellite)
        .map(|g| g.id)
        .collect();
    let Some(centre) = threat_centroid(p) else {
        return;
    };
    if sats.is_empty() {
        return;
    }
    let sat = sats[st.imaging_turn as usize % sats.len()];
    st.imaging_turn = st.imaging_turn.wrapping_add(1);
    st.next_imaging_ms = p.now_ms + IMAGING_MS;
    out.push(Decision {
        layer: Layer::Operational,
        action: Some(Action::Image {
            satellite: sat,
            centre,
            radius_m: IMAGE_RADIUS_M,
        }),
        why: format!(
            "Satellite {} tasked on the area where the enemy was last seen",
            sat.0
        ),
    });
}
