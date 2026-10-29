//! Sensors (doc 02 §7, §16): radar, ESM, IRST, EO/IR, visual lookout and
//! simplified sonar. Each tick on which a sensor is due, the stage builds a
//! list of bodies (every platform and scripted target, in `UnitId` order), a
//! spatial index of H3 res-3 buckets, and evaluates candidate pairs through
//! domain → range → horizon → field of regard → line of sight → Pd.
//!
//! The index only prunes: candidates come from a conservative ring of
//! buckets and are checked by exact distance, then visited in `UnitId`
//! order, so bucket edges never change a result. Line of sight goes through
//! the mask-altitude memo of ADR-023, whose values are pure functions of
//! their keys.

use bevy_ecs::prelude::*;
use core::f64::consts::{FRAC_PI_2, PI, TAU};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use wb_content::compiled::{DatalinkIx, Platform, PlatformIx, SensorIx, Signature};
use wb_content::source::realism::{ContactLevel, TerrainClass};
use wb_content::source::{Domain, Emcon, SensorRole, SensorType, StationKind};
use wb_core::geo::{LatLon, destination_point, distance_m, initial_bearing, normalize_bearing};
use wb_core::math::{atan2, cos, pow, sqrt};
use wb_core::sun::sun_elevation;
use wb_core::{RngPurpose, UnitId, rng, rng_normal};
use wb_geo::{Geo, LosParams};

use crate::clock::{RngSeed, SimClock};
use crate::components::{
    Allegiance, EmconState, Group, GroupMember, Kinematics, Magazine, PlatformRef, Posture,
    SatelliteState, ScriptedTarget,
};
use crate::movement::angle_diff;
use crate::picture::{Bearing, Detection, Measured, Quality, SidePictures, SourceKind, TypeRef};
use crate::rules::Rules;
use crate::world::{UnitIndex, entity};

/// Sensor slot of the built-in crew lookout (doc 02 §7.2 visual).
pub const LOOKOUT_SLOT: u8 = 255;
/// Highest target altitude considered when pruning by horizon.
const PRUNE_TARGET_ALT_M: f64 = 20_000.0;
/// Pd below this at the prune range: a radar never looks further than
/// `PD_PRUNE_FACTOR × R50` against the largest signature.
const PD_PRUNE_FACTOR: f64 = 2.5;
/// Largest RCS assumed when pruning a radar's search (a large ship), m².
const PRUNE_SIGMA_M2: f64 = 20_000.0;

/// The region packs, inside the world (ADR-022). Not `Send`: the DEM
/// caches are `RefCell`s.
pub struct GeoRes(pub Option<Rc<Geo>>);

/// Line-of-sight memo (ADR-023). Never saved: every value is a pure
/// function of its key, so clearing it never changes a result.
#[derive(Resource, Default)]
pub struct LosMemo(pub BTreeMap<(u64, u16, u64), f32>);

/// What a sensor sees of one platform or scripted target this tick.
pub(crate) struct Body {
    pub(crate) uid: UnitId,
    pub(crate) side: u8,
    pub(crate) pos: LatLon,
    /// Height the body is seen at (top of a ship, terrain + vehicle,
    /// aircraft altitude), metres above sea level.
    pub(crate) h_m: f64,
    /// Height of the ground or sea below, metres.
    pub(crate) ground_m: f64,
    pub(crate) heading: f64,
    pub(crate) speed: f64,
    pub(crate) domain: Domain,
    pub(crate) key: UnitId,
    pub(crate) group_size: u16,
    pub(crate) type_ref: TypeRef,
    pub(crate) platform: Option<PlatformIx>,
    pub(crate) stores_mult: f64,
    pub(crate) emcon: Emcon,
    pub(crate) fixed: bool,
    /// Can carry sensors this tick (moving platform, not a scripted target).
    pub(crate) observer: bool,
    /// Can be seen this tick (not parked at a base).
    pub(crate) visible: bool,
    pub(crate) over_land: bool,
    pub(crate) terrain: TerrainClass,
}

fn signature<'a>(rules: &'a Rules, t: TypeRef) -> Option<&'a Signature> {
    match t {
        TypeRef::Platform(p) => Some(&rules.platform(p).signature),
        TypeRef::Weapon(w) => rules.db.weapons[w.0 as usize].signature.as_ref(),
        TypeRef::System(_) => None,
    }
}

/// RCS by aspect (doc 02 §7.1): azimuth blend of front, side and rear,
/// elevation blend with top.
pub fn rcs(sig: &Signature, aspect_az: f64, aspect_el: f64) -> f64 {
    let r = &sig.rcs_m2;
    let (ca, sa) = (cos(aspect_az), wb_core::math::sin(aspect_az));
    let az = if aspect_az.abs() <= FRAC_PI_2 {
        r.front * ca * ca + r.side * sa * sa
    } else {
        r.rear * ca * ca + r.side * sa * sa
    };
    let (ce, se) = (cos(aspect_el), wb_core::math::sin(aspect_el));
    az * ce * ce + r.top * se * se
}

/// `Pd = 1 / (1 + (d / R50)^k)`.
pub fn pd(d: f64, r50: f64, k: f64) -> f64 {
    if r50 <= 0.0 {
        return 0.0;
    }
    1.0 / (1.0 + pow(d / r50, k))
}

/// Radio or visual horizon, metres: `c × (√h1 + √h2)` km.
pub fn horizon_m(rules: &Rules, h1: f64, h2: f64) -> f64 {
    rules.realism().earth.horizon_coefficient * (sqrt(h1.max(0.0)) + sqrt(h2.max(0.0))) * 1_000.0
}

/// External stores raise RCS while any weapon hangs on an external station.
fn stores_mult(rules: &Rules, p: &Platform, mag: Option<&Magazine>) -> f64 {
    let Some(m) = mag else { return 1.0 };
    let Some(l) = m.loadout else { return 1.0 };
    let lo = &rules.db.loadouts[l.0 as usize];
    let external = lo.items.iter().any(|it| {
        p.stations
            .get(it.station as usize)
            .is_some_and(|s| s.kind == StationKind::External)
            && m.rounds.iter().any(|(w, n)| *w == it.weapon && *n > 0)
    });
    if external {
        p.signature.external_stores_rcs_multiplier
    } else {
        1.0
    }
}

/// Network a detection travels on and its delay to the side picture.
fn route(rules: &Rules, p: Option<&Platform>) -> (Option<DatalinkIx>, f64, bool) {
    let fusion = &rules.realism().fusion;
    let best = p.and_then(|p| {
        p.datalinks
            .iter()
            .map(|d| (*d, rules.db.datalinks[d.0 as usize].latency_s))
            .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
    });
    match best {
        Some((d, lat)) => (Some(d), lat, false),
        None => (None, fusion.c2_report_delay_s, true),
    }
}

fn emitting(rules: &Rules, emcon: Emcon, uid: UnitId, now_ms: u64) -> bool {
    match emcon {
        Emcon::On => true,
        Emcon::Off => false,
        Emcon::Blink => {
            let ew = &rules.realism().ew;
            let period = (ew.emcon_blink_period_s * 1_000.0).max(1_000.0) as u64;
            let on = (ew.emcon_blink_on_s * 1_000.0) as u64;
            let off = (u64::from(uid.0).wrapping_mul(0x9E37_79B9) % (period / 1_000).max(1)) * 1_000;
            (now_ms + off) % period < on
        }
    }
}

fn wb_terrain(c: wb_geo::TerrainClass) -> TerrainClass {
    use wb_geo::TerrainClass as G;
    match c {
        G::Forest | G::Jungle | G::Marsh => TerrainClass::Forest,
        G::Urban => TerrainClass::Urban,
        G::Mountain => TerrainClass::Mountain,
        _ => TerrainClass::Open,
    }
}

pub(crate) fn bodies(world: &mut World, rules: &Rules, geo: Option<&Geo>) -> Vec<Body> {
    let earth = &rules.realism().earth;
    let index: Vec<(UnitId, Entity)> = world
        .resource::<UnitIndex>()
        .0
        .iter()
        .map(|(u, e)| (*u, *e))
        .collect();
    let mut out = Vec::new();
    for (uid, e) in index {
        let (Some(a), Some(k)) = (world.get::<Allegiance>(e), world.get::<Kinematics>(e)) else {
            continue;
        };
        if world.get::<SatelliteState>(e).is_some() {
            continue;
        }
        let (type_ref, platform, domain, observer) =
            if let Some(s) = world.get::<ScriptedTarget>(e) {
                (TypeRef::Weapon(s.weapon), None, Domain::Air, false)
            } else if let Some(p) = world.get::<PlatformRef>(e) {
                (TypeRef::Platform(p.0), Some(p.0), rules.platform(p.0).domain, true)
            } else {
                continue;
            };
        let at_base = matches!(world.get::<Posture>(e), Some(Posture::AtBase { .. }));
        let (key, group_size, sys) = match world.get::<GroupMember>(e) {
            Some(m) => {
                let g = entity(world, m.group).and_then(|ge| world.get::<Group>(ge));
                (
                    m.group,
                    g.map_or(1, |g| g.members.len() as u16),
                    g.and_then(|g| g.system),
                )
            }
            None => (uid, 1, None),
        };
        let type_ref = sys.map_or(type_ref, TypeRef::System);
        let (ground_m, over_land, terrain) = match geo {
            Some(g) => {
                let class = g.terrain_class(k.pos()).unwrap_or(wb_geo::TerrainClass::Water);
                let land = class != wb_geo::TerrainClass::Water;
                let z = if domain == Domain::Ground {
                    g.elevation_m(k.pos()).unwrap_or(0.0)
                } else {
                    0.0
                };
                (z, land, wb_terrain(class))
            }
            None => (0.0, domain == Domain::Ground, TerrainClass::Open),
        };
        let h_m = match domain {
            Domain::Air | Domain::Space => k.alt_m.max(0.0),
            Domain::Surface => earth.surface_target_height_m,
            Domain::Subsurface => 0.0,
            Domain::Ground => ground_m + earth.ground_target_height_m,
        };
        let p = platform.map(|p| rules.platform(p));
        out.push(Body {
            uid,
            side: a.side,
            pos: k.pos(),
            h_m,
            ground_m,
            heading: k.heading,
            speed: k.speed_mps,
            domain,
            key,
            group_size,
            type_ref,
            platform,
            stores_mult: p.map_or(1.0, |p| stores_mult(rules, p, world.get::<Magazine>(e))),
            emcon: world.get::<EmconState>(e).map_or(Emcon::On, |x| x.0),
            fixed: sys.is_some_and(|s| {
                rules.db.systems[s.0 as usize].deploy_as == wb_content::source::system::DeployAs::Site
            }) || (domain == Domain::Ground && k.speed_mps < 0.5),
            observer: observer && !at_base,
            visible: !at_base,
            over_land,
            terrain,
        });
    }
    out
}

/// Spatial index: per side, H3 res-3 cell → indices into the body list.
struct Index {
    res: u8,
    buckets: BTreeMap<(u8, u64), Vec<usize>>,
    sides: BTreeSet<u8>,
    edge_m: f64,
}

impl Index {
    fn new(bodies: &[Body], res: u8) -> Self {
        let mut buckets: BTreeMap<(u8, u64), Vec<usize>> = BTreeMap::new();
        let mut sides = BTreeSet::new();
        for (i, b) in bodies.iter().enumerate() {
            if b.visible {
                sides.insert(b.side);
                buckets
                    .entry((b.side, wb_geo::h3::cell(b.pos, res)))
                    .or_default()
                    .push(i);
            }
        }
        Self {
            res,
            buckets,
            sides,
            edge_m: wb_geo::h3::edge_m(res),
        }
    }

    /// Bodies of other sides than `own` within `radius_m` of `p`, in body
    /// (`UnitId`) order.
    fn query(&self, p: LatLon, radius_m: f64, own: u8, bodies: &[Body]) -> Vec<usize> {
        if self.sides.iter().all(|s| *s == own) {
            return Vec::new();
        }
        // Neighbouring centres are √3 edges apart; allow for cell size
        // variation (pentagons, distortion) with generous factors.
        let k = ((radius_m + 2.6 * self.edge_m) / (1.2 * self.edge_m)).ceil() as u32 + 1;
        let mut out: Vec<usize> = if k > 60 {
            (0..bodies.len())
                .filter(|i| bodies[*i].visible && bodies[*i].side != own)
                .collect()
        } else {
            let cells = wb_geo::h3::disk(wb_geo::h3::cell(p, self.res), k);
            self.sides
                .iter()
                .filter(|s| **s != own)
                .flat_map(|s| cells.iter().filter_map(move |c| self.buckets.get(&(*s, *c))))
                .flatten()
                .copied()
                .collect()
        };
        out.retain(|i| distance_m(p, bodies[*i].pos) <= radius_m);
        out.sort_unstable();
        out
    }
}

/// Altitude band of a sensor for the LOS memo (ADR-023): lower edge of
/// `step`-wide bands up to 1 km, then bands growing by `ratio`.
fn alt_band(h: f64, step: f64, ratio: f64) -> (u16, f64) {
    let h = h.max(0.0);
    if h < 1_000.0 {
        let i = (h / step).floor();
        return (i as u16, i * step);
    }
    let base = (1_000.0 / step).ceil();
    let i = (wb_core::math::ln(h / 1_000.0) / wb_core::math::ln(ratio)).floor().max(0.0);
    ((base + i) as u16, 1_000.0 * pow(ratio, i))
}

struct Los<'a> {
    geo: Option<&'a Geo>,
    memo: &'a mut LosMemo,
    rules: &'a Rules,
}

impl Los<'_> {
    /// Terrain line of sight from a sensor at `h_s` to a target at `h_t`.
    fn clear(&mut self, s: LatLon, h_s: f64, t: LatLon, h_t: f64) -> bool {
        let Some(geo) = self.geo else { return true };
        let sp = &self.rules.realism().spatial;
        let (band, h_rep) = alt_band(h_s, sp.los_memo_alt_step_m, sp.los_memo_alt_ratio);
        let (cs, ct) = (
            wb_geo::h3::cell(s, sp.los_memo_res),
            wb_geo::h3::cell(t, sp.los_memo_res),
        );
        if cs == ct {
            return true;
        }
        let key = (cs, band, ct);
        let mask = match self.memo.0.get(&key) {
            Some(m) => f64::from(*m),
            None => {
                let (Some(a), Some(b)) = (wb_geo::h3::centre(cs), wb_geo::h3::centre(ct)) else {
                    return true;
                };
                let d = distance_m(a, b);
                let earth = &self.rules.realism().earth;
                let radar = &self.rules.realism().radar;
                let params = LosParams {
                    k_factor: earth.radio_k_factor,
                    step_m: (d / radar.los_sample_divisor).max(30.0),
                };
                let m = geo
                    .los_mask_altitude(a, h_rep, b, &params)
                    .unwrap_or(f64::NEG_INFINITY)
                    .clamp(-1.0e6, 1.0e6) as f32;
                if self.memo.0.len() >= sp.los_memo_capacity as usize {
                    self.memo.0.clear();
                }
                self.memo.0.insert(key, m);
                f64::from(m)
            }
        };
        h_t >= mask
    }
}

/// Everything a scan needs besides the bodies.
struct Scan<'a> {
    rules: &'a Rules,
    seed: u64,
    tick: u64,
    now_ms: u64,
    utc_ms: f64,
}

/// One sensor of an observer, with its mounting.
struct Mounted {
    slot: u8,
    sensor: Option<SensorIx>,
    height_m: f64,
}

/// Sensors of a platform plus the crew lookout.
fn mounted(rules: &Rules, p: &Platform) -> Vec<Mounted> {
    let mut v: Vec<Mounted> = p
        .sensors
        .iter()
        .enumerate()
        .map(|(i, m)| Mounted {
            slot: i as u8,
            sensor: Some(m.sensor),
            height_m: m.height_m,
        })
        .collect();
    if p.crew.is_some_and(|c| c > 0) && p.domain != Domain::Space {
        v.push(Mounted {
            slot: LOOKOUT_SLOT,
            sensor: None,
            height_m: if p.domain == Domain::Surface { 15.0 } else { 2.0 },
        });
    }
    let _ = rules;
    v
}

/// Scan period of a sensor, ms (whole seconds, at least 1 s).
fn period_ms(rules: &Rules, m: &Mounted) -> u64 {
    let t = &rules.realism().time;
    let s = match m.sensor.map(|s| &rules.db.sensors[s.0 as usize]) {
        Some(s) => match s.sensor_type {
            SensorType::Radar => s.radar.as_ref().map_or(1.0, |r| r.scan_period_s),
            SensorType::Irst => s.irst.as_ref().map_or(1.0, |r| r.scan_period_s),
            SensorType::Esm => t.esm_period_s,
            _ => t.fusion_period_s,
        },
        None => t.fusion_period_s,
    };
    ((s.round().max(1.0)) * 1_000.0) as u64
}

/// Measurement with error: range error along the line of sight, cross error
/// across it, both at least the contact floor (ADR-026).
#[allow(clippy::too_many_arguments)]
fn measure(
    sc: &Scan,
    obs: &Body,
    tgt: &Body,
    sigma_r: f64,
    sigma_c: f64,
    slot: u8,
    kind: SourceKind,
) -> Measured {
    let floor = sc.rules.realism().fusion.contact_sigma_floor_m;
    let (sr, scx) = (sigma_r.max(floor), sigma_c.max(floor));
    let ent = (u64::from(obs.uid.0) << 32) | u64::from(tgt.uid.0);
    let n = u32::from(slot) * 64 + kind as u32 * 8;
    let z = |i: u32| rng_normal(sc.seed, RngPurpose::ContactError, ent, sc.tick, n + i);
    let brg = initial_bearing(obs.pos, tgt.pos);
    let p1 = destination_point(tgt.pos, brg, z(0) * sr);
    let at = destination_point(p1, normalize_bearing(brg + FRAC_PI_2), z(1) * scx);
    let (a, b, rot) = if sr >= scx {
        (sr, scx, brg)
    } else {
        (scx, sr, normalize_bearing(brg + FRAC_PI_2))
    };
    let alt_m = match tgt.domain {
        Domain::Air => (tgt.h_m + z(2) * sr.min(2_000.0)).max(0.0),
        _ => tgt.ground_m,
    };
    let speed = (tgt.speed * (1.0 + 0.03 * z(3))).max(0.0);
    let heading = if tgt.speed > 1.0 {
        normalize_bearing(tgt.heading + 0.03 * z(4))
    } else {
        0.0
    };
    Measured {
        at,
        alt_m,
        heading,
        speed_mps: speed,
        a_m: a,
        b_m: b,
        rot: normalize_bearing(rot),
    }
}

/// A gate-passed candidate: the detection fields a model sets.
struct Seen {
    pd: f64,
    level: ContactLevel,
    quality: Quality,
    count_resolved: bool,
    sigma_r: f64,
    sigma_c: f64,
    bearing: Option<f64>,
}

fn in_field(heading: f64, field: f64, brg: f64) -> bool {
    field >= TAU - 1e-9 || angle_diff(heading, brg).abs() <= field / 2.0
}

fn roles(rules: &Rules, s: SensorIx) -> &[SensorRole] {
    &rules.db.sensors[s.0 as usize].role
}

/// The sensor models: what one sensor makes of one target, before the draw.
#[allow(clippy::too_many_arguments)]
fn evaluate(
    sc: &Scan,
    los: &mut Los,
    obs: &Body,
    m: &Mounted,
    tgt: &Body,
    d: f64,
) -> Option<Seen> {
    let rules = sc.rules;
    let real = rules.realism();
    let h_s = obs.h_m.max(obs.ground_m) + m.height_m;
    let brg = initial_bearing(obs.pos, tgt.pos);
    let night = || sun_elevation(tgt.pos, sc.utc_ms) < real.passive.eo_min_sun_elevation_rad;
    let size = || {
        match tgt.type_ref {
            TypeRef::Platform(p) => rules.platform(p).signature.visual_size_m,
            TypeRef::Weapon(w) => rules.db.weapons[w.0 as usize]
                .signature
                .as_ref()
                .and_then(|s| s.visual_size_m),
            TypeRef::System(_) => tgt.platform.and_then(|p| rules.platform(p).signature.visual_size_m),
        }
        .unwrap_or(10.0)
    };
    let Some(six) = m.sensor else {
        // Crew lookout.
        let p = &real.passive;
        let base = p.visual_range_m.get(&tgt.terrain).copied().unwrap_or(4_000.0);
        let mut r = base
            * sqrt(size() / p.visual_ref_size_m.max(0.1)).min(p.visual_max_size_factor);
        if night() {
            r *= p.visual_night_factor;
        }
        if d > r || d > horizon_m(rules, h_s, tgt.h_m) || !los.clear(obs.pos, h_s, tgt.pos, tgt.h_m) {
            return None;
        }
        return Some(Seen {
            pd: 1.0,
            level: if d < r * 0.5 { ContactLevel::Type } else { ContactLevel::Class },
            quality: Quality::Track,
            count_resolved: true,
            sigma_r: d * 0.05,
            sigma_c: d * 0.02,
            bearing: None,
        });
    };
    let s = &rules.db.sensors[six.0 as usize];
    match s.sensor_type {
        SensorType::Radar => {
            let r = s.radar.as_ref()?;
            let role = roles(rules, six);
            let ok_domain = match tgt.domain {
                Domain::Air => role.iter().any(|x| {
                    matches!(
                        x,
                        SensorRole::AirSearch
                            | SensorRole::Aew
                            | SensorRole::FireControl
                            | SensorRole::HeightFinding
                            | SensorRole::MissileGuidance
                    )
                }),
                Domain::Surface => role
                    .iter()
                    .any(|x| matches!(x, SensorRole::SurfaceSearch | SensorRole::GroundMapping | SensorRole::Aew)),
                Domain::Ground => role.iter().any(|x| matches!(x, SensorRole::GroundMapping)),
                _ => false,
            };
            if !ok_domain || !emitting(rules, obs.emcon, obs.uid, sc.now_ms) {
                return None;
            }
            if r.max_range_m.is_some_and(|mx| d > mx) || d > horizon_m(rules, h_s, tgt.h_m) {
                return None;
            }
            if !in_field(obs.heading, r.field_of_regard_rad, brg) {
                return None;
            }
            let sig = signature(rules, tgt.type_ref)
                .or_else(|| tgt.platform.map(|p| &rules.platform(p).signature))?;
            let back = initial_bearing(tgt.pos, obs.pos);
            let az = angle_diff(tgt.heading, back);
            let el = atan2(h_s - tgt.h_m, d.max(1.0));
            let sigma = rcs(sig, az, el) * tgt.stores_mult;
            let mut r50 = r.r_ref_m * pow((sigma / r.sigma_ref_m2).max(1e-12), 0.25);
            let rr = &real.radar;
            let low = tgt.h_m - tgt.ground_m
                < if tgt.over_land { rr.clutter_alt_land_m } else { rr.clutter_alt_sea_m };
            if low {
                r50 *= r.clutter_rejection;
            }
            if !los.clear(obs.pos, h_s, tgt.pos, tgt.h_m) {
                return None;
            }
            let mut p = pd(d, r50, rr.pd_steepness_k);
            if r.pulse_doppler && tgt.h_m < h_s {
                let radial = tgt.speed * cos(angle_diff(tgt.heading, brg))
                    - obs.speed * cos(angle_diff(obs.heading, brg));
                if radial.abs() < rr.doppler_notch_radial_mps {
                    p *= rr.doppler_notch_pd_factor;
                }
            }
            let fc = r.fire_channels > 0
                && role
                    .iter()
                    .any(|x| matches!(x, SensorRole::FireControl | SensorRole::MissileGuidance));
            let bs = r
                .bearing_sigma_rad
                .unwrap_or(wb_core::units::deg_to_rad(rr.bearing_sigma_default_deg));
            Some(Seen {
                pd: p,
                level: if fc { ContactLevel::Class } else { ContactLevel::Domain },
                quality: if fc { Quality::FireControl } else { Quality::Track },
                count_resolved: d < rr.group_resolve_fraction * r50,
                sigma_r: rr.range_sigma_m,
                sigma_c: d * bs,
                bearing: None,
            })
        }
        SensorType::Irst => {
            let r = s.irst.as_ref()?;
            if tgt.domain != Domain::Air || !in_field(obs.heading, r.field_of_regard_rad, brg) {
                return None;
            }
            let p = &real.passive;
            let (cruise, ab) = match signature(rules, tgt.type_ref).and_then(|s| s.ir.as_ref()) {
                Some(ir) => (ir.cruise, ir.afterburner),
                None => return None,
            };
            let back = initial_bearing(tgt.pos, obs.pos);
            let az = angle_diff(tgt.heading, back).abs();
            let f = &p.irst_aspect_factor;
            let aspect = if az < PI / 3.0 {
                f.head_on
            } else if az < 2.0 * PI / 3.0 {
                f.beam
            } else {
                f.tail
            };
            let fast = tgt
                .platform
                .map(|pl| tgt.speed > rules.cruise_mps(rules.platform(pl), tgt.h_m) * 1.3)
                .unwrap_or(false);
            let i = if fast {
                ab.unwrap_or(cruise * p.irst_afterburner_factor)
            } else {
                cruise
            } * aspect;
            let r50 = r.r_ref_m * sqrt((i / r.i_ref).max(0.0));
            if d > horizon_m(rules, h_s, tgt.h_m) || !los.clear(obs.pos, h_s, tgt.pos, tgt.h_m) {
                return None;
            }
            Some(Seen {
                pd: pd(d, r50, real.radar.pd_steepness_k),
                level: ContactLevel::Domain,
                quality: Quality::Track,
                count_resolved: false,
                sigma_r: d * p.irst_range_sigma_fraction,
                sigma_c: d * 0.002,
                bearing: None,
            })
        }
        SensorType::EoIr => {
            let e = s.eo_ir.as_ref()?;
            if d > e.max_range_m || !in_field(obs.heading, e.field_of_regard_rad, brg) {
                return None;
            }
            if !e.night_capable && night() {
                return None;
            }
            let p = &real.passive;
            let gsd = e.gsd_at_10km_m * (d / 10_000.0).max(0.01);
            if size() / gsd < p.eo_detect_pixels {
                return None;
            }
            if d > horizon_m(rules, h_s, tgt.h_m) || !los.clear(obs.pos, h_s, tgt.pos, tgt.h_m) {
                return None;
            }
            let level = if gsd < p.eo_gsd_type_m {
                ContactLevel::Precise
            } else if gsd < p.eo_gsd_class_m {
                ContactLevel::Class
            } else {
                ContactLevel::Domain
            };
            Some(Seen {
                pd: 1.0,
                level,
                quality: Quality::Track,
                count_resolved: gsd < p.eo_gsd_type_m,
                sigma_r: d * 0.02,
                sigma_c: d * 0.002,
                bearing: None,
            })
        }
        SensorType::Visual => {
            let v = s.visual.as_ref()?;
            let p = &real.passive;
            let base = p.visual_range_m.get(&tgt.terrain).copied().unwrap_or(4_000.0);
            let mut r = base * sqrt(size() / p.visual_ref_size_m.max(0.1)).min(p.visual_max_size_factor);
            if !v.night_vision && night() {
                r *= p.visual_night_factor;
            }
            if d > r || !los.clear(obs.pos, h_s, tgt.pos, tgt.h_m) {
                return None;
            }
            Some(Seen {
                pd: 1.0,
                level: ContactLevel::Type,
                quality: Quality::Track,
                count_resolved: true,
                sigma_r: d * 0.05,
                sigma_c: d * 0.02,
                bearing: None,
            })
        }
        SensorType::Sonar => {
            let so = s.sonar.as_ref()?;
            if !matches!(tgt.domain, Domain::Surface | Domain::Subsurface)
                || !matches!(obs.domain, Domain::Surface | Domain::Subsurface)
            {
                return None;
            }
            let pl = tgt.platform.map(|p| rules.platform(p))?;
            let noise0 = pl.signature.acoustic_db?;
            let vc = rules.cruise_mps(pl, 0.0).max(0.5);
            let doublings = wb_core::math::ln((tgt.speed / vc).max(0.25)) / core::f64::consts::LN_2;
            let noise = noise0 + real.passive.sonar_noise_db_per_doubling * doublings;
            let passive = so.passive_r_ref_m * pow(10.0, (noise - so.noise_ref_db) / 20.0);
            let active = if emitting(rules, obs.emcon, obs.uid, sc.now_ms) {
                so.active_range_m.unwrap_or(0.0)
            } else {
                0.0
            };
            let r50 = passive.max(active);
            Some(Seen {
                pd: pd(d, r50, real.radar.pd_steepness_k),
                level: ContactLevel::Domain,
                quality: Quality::Track,
                count_resolved: false,
                sigma_r: d * 0.1,
                sigma_c: d * 0.02,
                bearing: None,
            })
        }
        SensorType::Esm => {
            let e = s.esm.as_ref()?;
            if !in_field(obs.heading, e.field_of_regard_rad, brg) {
                return None;
            }
            if !emitting(rules, tgt.emcon, tgt.uid, sc.now_ms) {
                return None;
            }
            let pl = tgt.platform.map(|p| rules.platform(p))?;
            // The strongest emitting radar in band.
            let best = pl
                .sensors
                .iter()
                .filter_map(|mt| {
                    let r = rules.db.sensors[mt.sensor.0 as usize].radar.as_ref()?;
                    e.bands.contains(&r.band).then_some((mt, r))
                })
                .map(|(mt, r)| {
                    let range = e.r_ref_m * sqrt(pow(10.0, (r.erp_class_dbw - e.erp_ref_dbw) / 10.0));
                    (range, mt.height_m, mt.sensor)
                })
                .max_by(|a, b| a.0.total_cmp(&b.0).then(b.2.cmp(&a.2)))?;
            let (range, mast, _) = best;
            let h_t = tgt.h_m.max(tgt.ground_m) + mast;
            if d > horizon_m(rules, h_s, h_t) || !los.clear(obs.pos, h_s, tgt.pos, h_t) {
                return None;
            }
            let sigma = if e.bearing_sigma_rad > 0.0 {
                e.bearing_sigma_rad
            } else {
                real.passive.esm_bearing_sigma_default_rad
            };
            Some(Seen {
                pd: pd(d, range, real.radar.pd_steepness_k),
                level: ContactLevel::Type,
                quality: Quality::Report,
                count_resolved: false,
                sigma_r: 0.0,
                sigma_c: sigma,
                bearing: Some(brg),
            })
        }
        SensorType::SatellitePayload => None,
    }
}

/// Largest distance a sensor could see anything at, for candidate pruning.
fn prune_range(rules: &Rules, m: &Mounted, h_s: f64) -> f64 {
    let hz = horizon_m(rules, h_s, PRUNE_TARGET_ALT_M);
    let Some(six) = m.sensor else {
        let p = &rules.realism().passive;
        let r = p.visual_range_m.values().copied().fold(0.0, f64::max);
        return (r * p.visual_max_size_factor).min(hz);
    };
    let s = &rules.db.sensors[six.0 as usize];
    let r = match s.sensor_type {
        SensorType::Radar => s.radar.as_ref().map_or(0.0, |r| {
            let cap = r.r_ref_m * pow(PRUNE_SIGMA_M2 / r.sigma_ref_m2, 0.25) * PD_PRUNE_FACTOR;
            r.max_range_m.map_or(cap, |m| m.min(cap))
        }),
        SensorType::Irst => s.irst.as_ref().map_or(0.0, |r| r.r_ref_m * 3.0 * PD_PRUNE_FACTOR),
        SensorType::EoIr => s.eo_ir.as_ref().map_or(0.0, |e| e.max_range_m),
        SensorType::Visual => {
            let p = &rules.realism().passive;
            p.visual_range_m.values().copied().fold(0.0, f64::max) * p.visual_max_size_factor
        }
        SensorType::Sonar => s.sonar.as_ref().map_or(0.0, |so| {
            (so.passive_r_ref_m * 10.0).max(so.active_range_m.unwrap_or(0.0)) * PD_PRUNE_FACTOR
        }),
        SensorType::Esm => s.esm.as_ref().map_or(0.0, |e| e.r_ref_m * 10.0 * PD_PRUNE_FACTOR),
        SensorType::SatellitePayload => 0.0,
    };
    r.min(hz)
}

fn kind_of(rules: &Rules, m: &Mounted) -> SourceKind {
    match m.sensor.map(|s| rules.db.sensors[s.0 as usize].sensor_type) {
        Some(SensorType::Radar) => SourceKind::Radar,
        Some(SensorType::Esm) => SourceKind::Esm,
        Some(SensorType::Irst) => SourceKind::Irst,
        Some(SensorType::EoIr) => SourceKind::EoIr,
        Some(SensorType::Sonar) => SourceKind::Sonar,
        Some(SensorType::Visual) | None => SourceKind::Visual,
        Some(SensorType::SatellitePayload) => SourceKind::Imagery,
    }
}

/// The type an emitter reveals (emitter library, derived from content): the
/// one system or platform that mounts the radar, if it is unique; otherwise
/// only the truth class.
pub(crate) fn emitter_type(rules: &Rules, tgt: &Body) -> (TypeRef, ContactLevel) {
    let Some(pl) = tgt.platform else {
        return (tgt.type_ref, ContactLevel::Class);
    };
    let radars: BTreeSet<SensorIx> = rules
        .platform(pl)
        .sensors
        .iter()
        .filter(|m| rules.db.sensors[m.sensor.0 as usize].radar.is_some())
        .map(|m| m.sensor)
        .collect();
    let mounts: Vec<usize> = rules
        .db
        .platforms
        .iter()
        .enumerate()
        .filter(|(_, p)| p.sensors.iter().any(|m| radars.contains(&m.sensor)))
        .map(|(i, _)| i)
        .collect();
    if mounts.len() != 1 {
        return (tgt.type_ref, ContactLevel::Class);
    }
    let systems: Vec<usize> = rules
        .db
        .systems
        .iter()
        .enumerate()
        .filter(|(_, s)| s.components.iter().any(|c| c.platform.0 as usize == mounts[0]))
        .map(|(i, _)| i)
        .collect();
    match (systems.len(), tgt.type_ref) {
        (1, TypeRef::System(s)) if s.0 as usize == systems[0] => (tgt.type_ref, ContactLevel::Type),
        (0, TypeRef::Platform(_)) => (tgt.type_ref, ContactLevel::Type),
        _ => (tgt.type_ref, ContactLevel::Class),
    }
}

/// Whether a body has a radar in one of `bands` that emits now (SIGINT).
pub(crate) fn emits_in(rules: &Rules, b: &Body, bands: &[wb_content::source::Band], now_ms: u64) -> bool {
    let Some(p) = b.platform else { return false };
    emitting(rules, b.emcon, b.uid, now_ms)
        && rules.platform(p).sensors.iter().any(|m| {
            rules.db.sensors[m.sensor.0 as usize]
                .radar
                .as_ref()
                .is_some_and(|r| bands.contains(&r.band))
        })
}

/// Moves scripted targets straight and level (test-only, before sensing).
pub fn move_scripted(world: &mut World) {
    let dt = world.resource::<SimClock>().dt_s();
    let mut q = world.query::<(&ScriptedTarget, &mut Kinematics)>();
    for (_, mut k) in q.iter_mut(world) {
        let next = destination_point(k.pos(), k.heading, k.speed_mps * dt);
        k.lat = next.lat;
        k.lon = next.lon;
    }
}

/// The sensor stage: every due sensor scans; detections go to the delivery
/// queue of the observer's side.
pub fn scan(world: &mut World) {
    let rules = world.resource::<Rules>().clone();
    let clock = world.resource::<SimClock>().clone();
    let seed = world.resource::<RngSeed>().0;
    let geo = world
        .get_non_send::<GeoRes>()
        .and_then(|g| g.0.clone());

    // Which observers have a due sensor this tick?
    let mut any_due = false;
    {
        let mut q = world.query::<(&PlatformRef, &crate::components::Identity)>();
        for (p, ident) in q.iter(world) {
            let pl = rules.platform(p.0);
            if mounted(&rules, pl)
                .iter()
                .any(|m| clock.due_staggered(period_ms(&rules, m), ident.unit.0 ^ u32::from(m.slot)))
            {
                any_due = true;
                break;
            }
        }
    }
    if !any_due {
        return;
    }
    let bodies = bodies(world, &rules, geo.as_deref());
    let index = Index::new(&bodies, rules.realism().spatial.bucket_res);
    let sc = Scan {
        rules: &rules,
        seed,
        tick: clock.tick,
        now_ms: clock.sim_time_ms,
        utc_ms: clock.start_utc_ms as f64 + clock.sim_time_ms as f64,
    };
    let radar = &rules.realism().radar;
    let window_mask: u8 = if radar.track_confirm_window_scans >= 8 {
        0xFF
    } else {
        (1u8 << radar.track_confirm_window_scans) - 1
    };
    let mut memo = world.remove_resource::<LosMemo>().unwrap_or_default();
    let mut pics = world.remove_resource::<SidePictures>().unwrap_or_default();
    {
        let mut los = Los {
            geo: geo.as_deref(),
            memo: &mut memo,
            rules: &rules,
        };
        for obs in bodies.iter().filter(|b| b.observer) {
            let Some(pl) = obs.platform.map(|p| rules.platform(p)) else {
                continue;
            };
            let (network, delay_s, c2) = route(&rules, Some(pl));
            for m in mounted(&rules, pl) {
                if !clock.due_staggered(period_ms(&rules, &m), obs.uid.0 ^ u32::from(m.slot)) {
                    continue;
                }
                let kind = kind_of(&rules, &m);
                let h_s = obs.h_m.max(obs.ground_m) + m.height_m;
                let range = prune_range(&rules, &m, h_s);
                let mut hits: BTreeSet<UnitId> = BTreeSet::new();
                for i in index.query(obs.pos, range, obs.side, &bodies) {
                    let tgt = &bodies[i];
                    if tgt.side == obs.side || tgt.uid == obs.uid {
                        continue;
                    }
                    let d = distance_m(obs.pos, tgt.pos);
                    let Some(seen) = evaluate(&sc, &mut los, obs, &m, tgt, d) else {
                        continue;
                    };
                    let ent = (u64::from(obs.uid.0) << 32) | u64::from(tgt.uid.0);
                    if seen.pd < 1.0
                        && rng(seed, RngPurpose::Detection, ent, clock.tick, u32::from(m.slot)) >= seen.pd
                    {
                        continue;
                    }
                    if kind == SourceKind::Radar {
                        hits.insert(tgt.uid);
                        let t = pics.tracks.entry((obs.uid, m.slot, tgt.uid)).or_default();
                        t.window = ((t.window << 1) | 1) & window_mask;
                        t.misses = 0;
                        if t.window.count_ones() >= radar.track_confirm_hits {
                            t.confirmed = true;
                        }
                        if !t.confirmed {
                            continue;
                        }
                    }
                    let (type_ref, level) = if kind == SourceKind::Esm {
                        emitter_type(&rules, tgt)
                    } else {
                        (tgt.type_ref, seen.level)
                    };
                    let (position, bearing) = match seen.bearing {
                        Some(b) => {
                            let ent = (u64::from(obs.uid.0) << 32) | u64::from(tgt.uid.0);
                            let z = rng_normal(seed, RngPurpose::EsmBearing, ent, clock.tick, u32::from(m.slot));
                            (
                                None,
                                Some(Bearing {
                                    origin: obs.pos,
                                    bearing: normalize_bearing(b + z * seen.sigma_c),
                                    sigma: seen.sigma_c,
                                    at_ms: clock.sim_time_ms,
                                }),
                            )
                        }
                        None => (
                            Some(measure(&sc, obs, tgt, seen.sigma_r, seen.sigma_c, m.slot, kind)),
                            None,
                        ),
                    };
                    let det = Detection {
                        side: obs.side,
                        observer: obs.uid,
                        sensor: m.sensor,
                        kind,
                        truth: tgt.key,
                        tick: clock.tick,
                        at_ms: clock.sim_time_ms,
                        range_m: d,
                        domain: tgt.domain,
                        type_ref,
                        group_size: tgt.group_size,
                        level,
                        count_resolved: seen.count_resolved,
                        quality: if c2 { Quality::Report } else { seen.quality },
                        position,
                        bearing,
                        network,
                        fixed: tgt.fixed,
                    };
                    pics.deliver(clock.sim_time_ms + (delay_s * 1_000.0) as u64, det);
                }
                if kind == SourceKind::Radar {
                    // Misses for every track of this sensor not hit this scan.
                    let lo = (obs.uid, m.slot, UnitId(0));
                    let hi = (obs.uid, m.slot, UnitId(u32::MAX));
                    let mut drop = Vec::new();
                    for (k, t) in pics.tracks.range_mut(lo..=hi) {
                        if hits.contains(&k.2) {
                            continue;
                        }
                        t.window = (t.window << 1) & window_mask;
                        t.misses = t.misses.saturating_add(1);
                        if u32::from(t.misses) >= radar.track_drop_misses {
                            drop.push(*k);
                        }
                    }
                    for k in drop {
                        pics.tracks.remove(&k);
                    }
                }
            }
        }
    }
    world.insert_resource(memo);
    world.insert_resource(pics);
}
