//! Side pictures (doc 02 §8, ADR-003, ADR-004, ADR-026): what each side
//! believes. Sensors produce [`Detection`]s with the measurement error
//! already applied; they reach the side's picture after the data-link or C2
//! delay, where they are fused into [`Contact`]s by truth association.
//!
//! A contact's `truth` and `first_seen` fields are engine-internal: views,
//! details and events read only the estimated fields and the [`ContactId`].

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use wb_content::compiled::{DatalinkIx, PlatformIx, SensorIx, SystemIx, WeaponIx};
use wb_content::source::Domain;
use wb_content::source::realism::ContactLevel;
use wb_core::geo::{LatLon, destination_point, initial_bearing, normalize_bearing};
use wb_core::math::{atan2, cos, sin};
use wb_core::{ContactId, RngPurpose, UnitId, rng};

use crate::clock::{RngSeed, SimClock};
use crate::events::{EventKind, emit};
use crate::rules::Rules;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum ContactIdentity {
    Unknown,
    Hostile,
    Neutral,
    Friendly,
}

/// How good the latest information is (doc 02 §8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum Quality {
    FireControl,
    Track,
    Report,
}

/// Kind of sensor behind a detection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Radar,
    Esm,
    Irst,
    EoIr,
    Visual,
    Sonar,
    Imagery,
    Sigint,
}

impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SourceKind::Radar => "Radar",
            SourceKind::Esm => "ESM",
            SourceKind::Irst => "IRST",
            SourceKind::EoIr => "EO/IR",
            SourceKind::Visual => "Visual",
            SourceKind::Sonar => "Sonar",
            SourceKind::Imagery => "Satellite imagery",
            SourceKind::Sigint => "SIGINT",
        }
    }
}

/// What a contact is, once known: a platform, a composite system or a
/// weapon (scripted targets).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TypeRef {
    Platform(PlatformIx),
    System(SystemIx),
    Weapon(WeaponIx),
}

/// A position measurement with its error ellipse (1 sigma, metres).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Measured {
    pub at: LatLon,
    pub alt_m: f64,
    pub heading: f64,
    pub speed_mps: f64,
    pub a_m: f64,
    pub b_m: f64,
    /// Bearing of the major axis, radians.
    pub rot: f64,
}

/// A bearing line from a passive sensor.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bearing {
    pub origin: LatLon,
    pub bearing: f64,
    pub sigma: f64,
    pub at_ms: u64,
}

/// One sensor report about one truth object, measurement error applied.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Detection {
    pub side: u8,
    pub observer: UnitId,
    pub sensor: Option<SensorIx>,
    pub kind: SourceKind,
    /// Association key: the target's group, or the target itself.
    pub truth: UnitId,
    pub tick: u64,
    pub at_ms: u64,
    /// True range, recorded only for `first_seen` (golden checks).
    pub range_m: f64,
    pub domain: Domain,
    pub type_ref: TypeRef,
    pub group_size: u16,
    pub level: ContactLevel,
    pub count_resolved: bool,
    pub quality: Quality,
    pub position: Option<Measured>,
    pub bearing: Option<Bearing>,
    pub network: Option<DatalinkIx>,
    pub fixed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SourceRef {
    pub kind: SourceKind,
    pub observer: UnitId,
    pub sensor: Option<SensorIx>,
    pub network: Option<DatalinkIx>,
    pub last_ms: u64,
}

/// Engine-internal record of the first detection (CLI golden checks only).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FirstSeen {
    pub observer: UnitId,
    pub kind: SourceKind,
    pub tick: u64,
    pub range_m: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Contact {
    pub id: ContactId,
    /// Engine-internal truth key. Never leaves the engine.
    pub truth: UnitId,
    pub domain: Domain,
    pub level: ContactLevel,
    pub identity: ContactIdentity,
    /// The truth type, revealed as class from `Class` and by name from `Type`.
    pub type_ref: TypeRef,
    pub est: Option<Measured>,
    pub first_ms: u64,
    pub last_ms: u64,
    pub quality: Quality,
    /// Estimated count range until resolved; equal ends once resolved.
    pub count: (u16, u16),
    pub count_resolved: bool,
    pub looks: u32,
    pub bearings: Vec<Bearing>,
    pub sources: Vec<SourceRef>,
    /// Not refreshed for the domain's age-out time: a last-known marker.
    pub lost: bool,
    pub fixed: bool,
    pub first_seen: FirstSeen,
}

impl Contact {
    /// Uncertainty ellipse now: `σ(t) = σ0 + v_max × age` (doc 02 §8).
    pub fn ellipse(&self, rules: &Rules, now_ms: u64) -> Option<(f64, f64, f64)> {
        let e = self.est?;
        if self.fixed {
            return Some((e.a_m, e.b_m, e.rot));
        }
        let age_s = now_ms.saturating_sub(self.last_ms) as f64 / 1_000.0;
        let v = rules
            .realism()
            .fusion
            .ellipse_growth_mps
            .get(&self.domain)
            .copied()
            .unwrap_or(0.0);
        Some((e.a_m + v * age_s, e.b_m + v * age_s, e.rot))
    }

    /// Estimated: below `Precise`, or not refreshed lately (doc 03 §10).
    pub fn estimated(&self, rules: &Rules, now_ms: u64) -> bool {
        self.level < ContactLevel::Precise
            || self.lost
            || now_ms.saturating_sub(self.last_ms) as f64 / 1_000.0
                > rules.realism().fusion.report_after_s
    }
}

/// One side's picture.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Picture {
    pub next_id: u32,
    pub contacts: BTreeMap<ContactId, Contact>,
    pub by_truth: BTreeMap<UnitId, ContactId>,
}

impl Picture {
    pub fn get(&self, id: ContactId) -> Option<&Contact> {
        self.contacts.get(&id)
    }
}

/// M-of-N radar track state of one (observer, sensor slot, target).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TrackState {
    /// Hit bits of the last scans, newest in bit 0.
    pub window: u8,
    pub misses: u8,
    pub confirmed: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Delivery {
    pub at_ms: u64,
    pub seq: u64,
    pub det: Detection,
}

/// An intel report waiting for its downlink (doc 02 §7.3).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PendingReport {
    pub at_ms: u64,
    pub side: u8,
    pub satellite: UnitId,
    pub pass_ms: u64,
    /// Plain-English outcome, e.g. "3 objects imaged" or "No image: sun −35° at the target".
    pub outcome: String,
    pub imaged: bool,
    pub dets: Vec<Detection>,
}

/// Every side's picture plus the sensor state behind it. Saved and hashed.
#[derive(Resource, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SidePictures {
    pub sides: Vec<Picture>,
    pub tracks: BTreeMap<(UnitId, u8, UnitId), TrackState>,
    pub pending: Vec<Delivery>,
    pub next_seq: u64,
    pub reports: Vec<PendingReport>,
    pub next_report: u32,
    /// Last SIGINT report per (satellite, emitter key), ms.
    pub sigint_seen: BTreeMap<(UnitId, UnitId), u64>,
}

impl SidePictures {
    pub fn new(sides: usize) -> Self {
        Self {
            sides: vec![Picture::default(); sides],
            ..Self::default()
        }
    }

    /// Queues a detection for delivery to its side's picture.
    pub fn deliver(&mut self, at_ms: u64, det: Detection) {
        let seq = self.next_seq;
        self.next_seq += 1;
        let i = self
            .pending
            .partition_point(|d| (d.at_ms, d.seq) <= (at_ms, seq));
        self.pending.insert(i, Delivery { at_ms, seq, det });
    }
}

/// Least-squares-free two-bearing fix: the pair with the widest crossing
/// angle (≥ `min_angle`), intersected on a local flat plane at the first
/// origin. Returns the estimate and its ellipse.
fn bearing_fix(bearings: &[Bearing], min_angle: f64) -> Option<Measured> {
    let mut best: Option<(f64, usize, usize)> = None;
    for i in 0..bearings.len() {
        for j in i + 1..bearings.len() {
            let d = crate::movement::angle_diff(bearings[i].bearing, bearings[j].bearing).abs();
            let cross = d.min(core::f64::consts::PI - d);
            if cross >= min_angle && best.is_none_or(|b| cross > b.0) {
                best = Some((cross, i, j));
            }
        }
    }
    let (cross, i, j) = best?;
    let (b1, b2) = (bearings[i], bearings[j]);
    // Local east/north metres around b1's origin.
    const R: f64 = 6_371_008.8;
    let k = cos(b1.origin.lat);
    let o2 = (
        (b2.origin.lon - b1.origin.lon) * k * R,
        (b2.origin.lat - b1.origin.lat) * R,
    );
    let d1 = (sin(b1.bearing), cos(b1.bearing));
    let d2 = (sin(b2.bearing), cos(b2.bearing));
    let den = d1.0 * d2.1 - d1.1 * d2.0;
    if den.abs() < 1e-9 {
        return None;
    }
    let t1 = (o2.0 * d2.1 - o2.1 * d2.0) / den;
    let t2 = (o2.0 * d1.1 - o2.1 * d1.0) / den;
    if t1 <= 0.0 || t2 <= 0.0 {
        return None;
    }
    let at = destination_point(b1.origin, b1.bearing, t1);
    let s1 = t1 * b1.sigma;
    let s2 = t2 * b2.sigma;
    let a = (s1 * s1 + s2 * s2).sqrt() / sin(cross).max(0.05);
    let b = s1.min(s2).max(1.0);
    let mid = atan2(d1.0 + d2.0, d1.1 + d2.1);
    Some(Measured {
        at,
        alt_m: 0.0,
        heading: 0.0,
        speed_mps: 0.0,
        a_m: a.max(b),
        b_m: b,
        rot: normalize_bearing(mid),
    })
}

/// Events to emit after fusing.
type Pending = Vec<(u8, EventKind)>;

/// Fuses one delivered detection into its side's picture.
fn fuse(pics: &mut SidePictures, rules: &Rules, seed: u64, det: Detection, out: &mut Pending) {
    let fusion = &rules.realism().fusion;
    let side = usize::from(det.side);
    let Some(pic) = pics.sides.get_mut(side) else {
        return;
    };
    let id = match pic.by_truth.get(&det.truth) {
        Some(id) => *id,
        None => {
            pic.next_id += 1;
            let id = ContactId(pic.next_id);
            let spread = fusion.group_count_spread;
            let draw = |n: u32| {
                (rng(
                    seed,
                    RngPurpose::CountEstimate,
                    u64::from(id.0) | (u64::from(det.side) << 40),
                    0,
                    n,
                ) * f64::from(spread + 1)) as u32
            };
            let n = u32::from(det.group_size.max(1));
            let lo = n.saturating_sub(draw(0)).max(1);
            let hi = n + draw(1);
            pic.contacts.insert(
                id,
                Contact {
                    id,
                    truth: det.truth,
                    domain: det.domain,
                    level: ContactLevel::Unknown,
                    identity: ContactIdentity::Unknown,
                    type_ref: det.type_ref,
                    est: None,
                    first_ms: det.at_ms,
                    last_ms: det.at_ms,
                    quality: det.quality,
                    count: (lo as u16, hi as u16),
                    count_resolved: false,
                    looks: 0,
                    bearings: Vec::new(),
                    sources: Vec::new(),
                    lost: false,
                    fixed: det.fixed,
                    first_seen: FirstSeen {
                        observer: det.observer,
                        kind: det.kind,
                        tick: det.tick,
                        range_m: det.range_m,
                    },
                },
            );
            pic.by_truth.insert(det.truth, id);
            out.push((
                det.side,
                EventKind::ContactNew {
                    contact: id,
                    domain: det.domain,
                    source: det.kind,
                    observer: det.observer,
                },
            ));
            id
        }
    };
    let c = pic.contacts.get_mut(&id).expect("contact");
    let before = (c.level, c.identity, c.count_resolved, c.lost);
    if det.at_ms >= c.last_ms || c.est.is_none() {
        c.last_ms = c.last_ms.max(det.at_ms);
    }
    c.lost = false;
    c.fixed |= det.fixed;
    // Position: a measurement replaces the estimate if it is newer or
    // better than the aged one.
    if let Some(m) = det.position {
        let take = match (c.est, c.ellipse(rules, det.at_ms)) {
            (Some(_), Some((a, _, _))) => det.at_ms >= c.last_ms || m.a_m < a,
            _ => true,
        };
        if take {
            c.est = Some(m);
        }
        c.bearings.clear();
    } else if let Some(b) = det.bearing {
        let window = (fusion.esm_bearing_window_s * 1_000.0) as u64;
        c.bearings
            .retain(|x| det.at_ms.saturating_sub(x.at_ms) <= window);
        c.bearings.push(b);
        if c.bearings.len() > 16 {
            c.bearings.remove(0);
        }
        let stale = c.est.is_none() || c.ellipse(rules, det.at_ms).is_some_and(|e| e.0 > 50_000.0);
        if stale && let Some(fix) = bearing_fix(&c.bearings, fusion.esm_fix_min_angle_rad) {
            c.est = Some(fix);
        }
    }
    c.quality = c.quality.min(det.quality);
    if det.at_ms >= c.last_ms {
        c.quality = det.quality;
    }
    c.looks += 1;
    let mut level = c.level.max(det.level);
    if level < ContactLevel::Class && c.looks >= fusion.class_after_looks {
        level = ContactLevel::Class;
    }
    if level >= ContactLevel::Domain && c.level < ContactLevel::Domain {
        level = level.max(ContactLevel::Domain);
    }
    c.level = level;
    if c.level >= fusion.identify_at && c.identity == ContactIdentity::Unknown {
        c.identity = ContactIdentity::Hostile;
    }
    if det.count_resolved && !c.count_resolved {
        c.count_resolved = true;
        let n = det.group_size.max(1);
        c.count = (n, n);
    }
    match c
        .sources
        .iter_mut()
        .find(|s| s.observer == det.observer && s.sensor == det.sensor && s.kind == det.kind)
    {
        Some(s) => s.last_ms = s.last_ms.max(det.at_ms),
        None => {
            c.sources.push(SourceRef {
                kind: det.kind,
                observer: det.observer,
                sensor: det.sensor,
                network: det.network,
                last_ms: det.at_ms,
            });
            c.sources.sort_by_key(|s| (s.kind, s.observer, s.sensor));
        }
    }
    if c.level > before.0 && before.0 > ContactLevel::Unknown {
        out.push((
            det.side,
            EventKind::ContactClassified {
                contact: id,
                level: c.level,
                source: det.kind,
            },
        ));
    }
    if c.count_resolved && !before.2 {
        out.push((
            det.side,
            EventKind::ContactCount {
                contact: id,
                min: c.count.0,
                max: c.count.1,
            },
        ));
    }
    if c.identity == ContactIdentity::Hostile && before.1 != ContactIdentity::Hostile {
        out.push((
            det.side,
            EventKind::AutoSlow {
                reason: "New hostile contact".to_owned(),
            },
        ));
    }
}

/// Delivers due detections and reports, then ages contacts (doc 02 §8).
pub fn fusion(world: &mut World) {
    let rules = world.resource::<Rules>().clone();
    let clock = world.resource::<SimClock>().clone();
    let seed = world.resource::<RngSeed>().0;
    let end = clock.end_ms();
    let mut out: Pending = Vec::new();
    let mut reports_out = Vec::new();
    {
        let mut pics = world.resource_mut::<SidePictures>();
        let n = pics.pending.partition_point(|d| d.at_ms <= end);
        let due: Vec<Delivery> = pics.pending.drain(..n).collect();
        for d in due {
            fuse(&mut pics, &rules, seed, d.det, &mut out);
        }
        let n = pics.reports.partition_point(|r| r.at_ms <= end);
        let reports: Vec<PendingReport> = pics.reports.drain(..n).collect();
        for r in reports {
            let mut contacts = Vec::new();
            for det in r.dets {
                let truth = det.truth;
                let side = usize::from(det.side);
                fuse(&mut pics, &rules, seed, det, &mut out);
                if let Some(id) = pics.sides.get(side).and_then(|p| p.by_truth.get(&truth)) {
                    contacts.push(*id);
                }
            }
            contacts.sort();
            contacts.dedup();
            pics.next_report += 1;
            reports_out.push((
                r.side,
                EventKind::IntelReport {
                    report: pics.next_report,
                    satellite: r.satellite,
                    pass_ms: r.pass_ms,
                    imaged: r.imaged,
                    outcome: r.outcome,
                    contacts,
                },
            ));
        }
        if clock.due((rules.realism().time.fusion_period_s * 1_000.0).max(1_000.0) as u64) {
            age(&mut pics, &rules, end, &mut out);
        }
    }
    for (side, kind) in out.into_iter().chain(reports_out) {
        emit(world, side, kind);
    }
}

fn age(pics: &mut SidePictures, rules: &Rules, now: u64, out: &mut Pending) {
    let fusion = &rules.realism().fusion;
    for (side, pic) in pics.sides.iter_mut().enumerate() {
        let mut gone = Vec::new();
        for c in pic.contacts.values_mut() {
            let age_s = now.saturating_sub(c.last_ms) as f64 / 1_000.0;
            if age_s > fusion.report_after_s {
                c.quality = Quality::Report;
            }
            let keep = c.fixed && fusion.fixed_sites_kept;
            let limit = fusion
                .age_out_s
                .get(&c.domain)
                .copied()
                .unwrap_or(f64::INFINITY);
            if !keep && !c.lost && age_s > limit {
                c.lost = true;
                out.push((side as u8, EventKind::ContactLost { contact: c.id }));
            }
            if !keep && c.lost && age_s > 4.0 * limit {
                gone.push((c.id, c.truth));
            }
        }
        for (id, truth) in gone {
            pic.contacts.remove(&id);
            pic.by_truth.remove(&truth);
        }
    }
}

/// Bearing from a contact estimate to a point (UI helpers and tests).
pub fn bearing_to(c: &Contact, p: LatLon) -> Option<f64> {
    c.est.map(|e| initial_bearing(e.at, p))
}

impl TypeRef {
    /// Label form of the type (`S-400`, `Su-35S`).
    pub fn short_name<'a>(&self, rules: &'a Rules) -> &'a str {
        let db = &rules.db;
        match *self {
            TypeRef::Platform(p) => db.str(rules.platform(p).short_name),
            TypeRef::System(s) => db.str(db.systems[s.0 as usize].short_name),
            TypeRef::Weapon(w) => db.str(db.weapons[w.0 as usize].name),
        }
    }

    /// The platform that stands for the type (a system's first component).
    pub fn lead_platform(&self, rules: &Rules) -> Option<PlatformIx> {
        match *self {
            TypeRef::Platform(p) => Some(p),
            TypeRef::System(s) => rules.db.systems[s.0 as usize]
                .components
                .first()
                .map(|c| c.platform),
            TypeRef::Weapon(_) => None,
        }
    }

    /// What `Class` reveals: `fighter`, `corvette`, `SAM site`, `anti-ship missile`.
    pub fn class_name(&self, rules: &Rules) -> String {
        match *self {
            TypeRef::System(s)
                if rules.db.systems[s.0 as usize].deploy_as
                    == wb_content::source::system::DeployAs::Site =>
            {
                "SAM site".to_owned()
            }
            TypeRef::Weapon(w) => {
                use wb_content::source::WeaponCategory as W;
                match rules.db.weapons[w.0 as usize].category {
                    W::Ashm => "anti-ship missile",
                    W::Sam => "SAM",
                    W::Aam => "air-to-air missile",
                    W::Lacm => "cruise missile",
                    W::Arm => "anti-radiation missile",
                    W::Ballistic => "ballistic missile",
                    _ => "missile",
                }
                .to_owned()
            }
            _ => self
                .lead_platform(rules)
                .map(|p| {
                    let cat = rules.platform(p).category.as_str();
                    cat.split('/').nth(1).unwrap_or(cat).replace('_', " ")
                })
                .unwrap_or_default(),
        }
    }

    /// Icon code (view `SYMBOLS`).
    pub fn symbol(&self, rules: &Rules) -> u16 {
        match *self {
            TypeRef::System(s)
                if rules.db.systems[s.0 as usize].deploy_as
                    == wb_content::source::system::DeployAs::Site =>
            {
                107
            }
            _ => self
                .lead_platform(rules)
                .map(|p| crate::view::platform_symbol(rules.platform(p).category))
                .unwrap_or(0),
        }
    }

    /// Longest surface-to-air range known for the type: system magazines,
    /// or the platform's loadouts (doc 03 §10 threat rings).
    pub fn threat_m(&self, rules: &Rules) -> Option<f64> {
        use wb_content::source::WeaponCategory;
        let db = &rules.db;
        let sam = |w: &wb_content::compiled::WeaponIx| {
            let w = &db.weapons[w.0 as usize];
            (w.category == WeaponCategory::Sam).then_some(w.profile.max_range_m)
        };
        match *self {
            TypeRef::System(s) => db.systems[s.0 as usize]
                .components
                .iter()
                .flat_map(|c| c.magazine.iter())
                .filter_map(|(w, _)| sam(w))
                .fold(None, |a: Option<f64>, r| Some(a.map_or(r, |a| a.max(r)))),
            TypeRef::Platform(p) => db
                .loadouts
                .iter()
                .filter(|l| l.platform == p)
                .flat_map(|l| l.items.iter())
                .filter_map(|it| sam(&it.weapon))
                .fold(None, |a: Option<f64>, r| Some(a.map_or(r, |a| a.max(r)))),
            TypeRef::Weapon(_) => None,
        }
    }
}
