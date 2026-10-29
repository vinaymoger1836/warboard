//! Satellites (doc 02 §7.3, ADR-024, ADR-025): SGP4 positions every tick,
//! SIGINT footprints every `satellite_period_s`, imaging of tasked areas at
//! the pass found when the order was accepted, and intel reports after the
//! downlink delay.

use bevy_ecs::prelude::*;
use wb_content::compiled::{Payload, PlatformIx, SensorIx};
use wb_content::source::realism::ContactLevel;
use wb_content::source::{Domain, PayloadKind};
use wb_core::geo::{LatLon, destination_point, distance_m, initial_bearing};
use wb_core::math::{atan2, hypot};
use wb_core::orbit::{Sgp4, Tle};
use wb_core::sun::sun_elevation;
use wb_core::{RngPurpose, UnitId, rng_normal};

use crate::clock::{RngSeed, SimClock};
use crate::components::{Allegiance, Identity, Kinematics, PlatformRef, SatelliteState, Tasking};
use crate::picture::{Detection, Measured, PendingReport, Quality, SidePictures, SourceKind};
use crate::rules::Rules;
use crate::sensors::{self, Body};

/// Engine sphere radius (doc 02 §5).
const R_M: f64 = wb_core::geo::EARTH_RADIUS_M;

pub fn sgp4(rules: &Rules, p: PlatformIx) -> Option<Sgp4> {
    let (l1, l2) = rules.platform(p).orbit?;
    Tle::parse(rules.db.str(l1), rules.db.str(l2))
        .ok()
        .map(Sgp4::new)
}

/// Sub-satellite point and height above the engine's sphere.
pub fn position(s: &Sgp4, utc_ms: f64) -> Option<(LatLon, f64)> {
    let [x, y, z] = s.ecef_m(utc_ms).ok()?;
    let r = hypot(hypot(x, y), z);
    Some((LatLon::new(atan2(z, hypot(x, y)), atan2(y, x)), r - R_M))
}

/// The satellite's payload (first sensor with a payload block).
pub fn payload(rules: &Rules, p: PlatformIx) -> Option<(SensorIx, &Payload)> {
    rules.platform(p).sensors.iter().find_map(|m| {
        rules.db.sensors[m.sensor.0 as usize]
            .payload
            .as_ref()
            .map(|pl| (m.sensor, pl))
    })
}

pub fn downlink_s(rules: &Rules, pl: &Payload) -> f64 {
    pl.downlink_delay_s
        .unwrap_or(rules.realism().satellites.downlink_delay_default_s)
}

/// First pass after `from_utc_ms` (within the tasking horizon) on which the
/// area is inside the payload's strip, at closest approach to the second.
pub fn next_pass(
    rules: &Rules,
    p: PlatformIx,
    centre: LatLon,
    radius_m: f64,
    from_utc_ms: f64,
) -> Option<f64> {
    let s = sgp4(rules, p)?;
    let (_, pl) = payload(rules, p)?;
    let sat = &rules.realism().satellites;
    let reach = pl.swath_m / 2.0 + radius_m;
    let step = sat.pass_search_step_s.max(1.0) * 1_000.0;
    let steps = (sat.tasking_horizon_s * 1_000.0 / step).ceil() as u64;
    let dist = |t: f64| position(&s, t).map(|(sp, _)| distance_m(sp, centre));
    let mut t = from_utc_ms;
    for _ in 0..steps {
        if dist(t).is_some_and(|d| d <= reach) {
            // Walk to the closest point, one second at a time.
            let mut best = (t, dist(t)?);
            let mut u = t;
            for _ in 0..1_200 {
                u += 1_000.0;
                let d = dist(u)?;
                if d > best.1 {
                    break;
                }
                best = (u, d);
            }
            return Some(best.0);
        }
        t += step;
    }
    None
}

/// Positions every tick; SIGINT every satellite period; imaging at passes.
pub fn update(world: &mut World) {
    let rules = world.resource::<Rules>().clone();
    let clock = world.resource::<SimClock>().clone();
    let seed = world.resource::<RngSeed>().0;
    let utc = clock.start_utc_ms as f64 + clock.sim_time_ms as f64;
    let mut q = world.query::<(&Identity, &PlatformRef, &Allegiance, &SatelliteState)>();
    let mut sats: Vec<(UnitId, PlatformIx, u8, Vec<Tasking>)> = q
        .iter(world)
        .map(|(i, p, a, s)| (i.unit, p.0, a.side, s.taskings.clone()))
        .collect();
    if sats.is_empty() {
        return;
    }
    sats.sort_by_key(|s| s.0);
    let mut moved = Vec::new();
    for (uid, p, _, _) in &sats {
        let Some(s) = sgp4(&rules, *p) else { continue };
        let (Some((a, h)), Some((b, _))) = (position(&s, utc), position(&s, utc + 1_000.0)) else {
            continue;
        };
        moved.push((
            *uid,
            Kinematics {
                lat: a.lat,
                lon: a.lon,
                alt_m: h,
                heading: initial_bearing(a, b),
                speed_mps: distance_m(a, b),
                climb_mps: 0.0,
            },
        ));
    }
    for (uid, k) in moved {
        if let Some(e) = crate::world::entity(world, uid) {
            world.entity_mut(e).insert(k);
        }
    }

    let sat_period = (rules.realism().time.satellite_period_s * 1_000.0).max(1_000.0) as u64;
    let sigint_due = clock.due(sat_period);
    let imaging_due = sats
        .iter()
        .any(|s| s.3.iter().any(|t| t.pass_ms < clock.end_ms()));
    if !sigint_due && !imaging_due {
        return;
    }
    let geo = world
        .get_non_send::<sensors::GeoRes>()
        .and_then(|g| g.0.clone());
    let bodies = sensors::bodies(world, &rules, geo.as_deref());
    let mut pics = world.remove_resource::<SidePictures>().unwrap_or_default();
    let mut done: Vec<(UnitId, usize)> = Vec::new();
    for (uid, p, side, taskings) in &sats {
        let Some(s) = sgp4(&rules, *p) else { continue };
        let Some((sensor, pl)) = payload(&rules, *p) else {
            continue;
        };
        let Some((sub, _)) = position(&s, utc) else {
            continue;
        };
        let delay = (downlink_s(&rules, pl) * 1_000.0) as u64;
        if sigint_due && pl.kind == PayloadKind::Sigint {
            let mut dets = Vec::new();
            for b in bodies.iter().filter(|b| b.visible && b.side != *side) {
                if distance_m(sub, b.pos) > pl.swath_m / 2.0
                    || !sensors::emits_in(&rules, b, &pl.bands, clock.sim_time_ms)
                {
                    continue;
                }
                let seen = pics.sigint_seen.get(&(*uid, b.key)).copied();
                if seen.is_some_and(|t| clock.sim_time_ms.saturating_sub(t) < 15 * 60_000) {
                    continue;
                }
                pics.sigint_seen.insert((*uid, b.key), clock.sim_time_ms);
                let sigma = pl.location_sigma_m.unwrap_or(5_000.0);
                let (type_ref, level) = sensors::emitter_type(&rules, b);
                dets.push(detection(
                    seed,
                    &clock,
                    *side,
                    *uid,
                    sensor,
                    SourceKind::Sigint,
                    b,
                    type_ref,
                    level,
                    sigma,
                    false,
                ));
            }
            if !dets.is_empty() {
                let n = dets.len();
                pics.reports.push(PendingReport {
                    at_ms: clock.sim_time_ms + delay,
                    side: *side,
                    satellite: *uid,
                    pass_ms: clock.sim_time_ms,
                    outcome: format!("{n} emitter{} located", if n == 1 { "" } else { "s" }),
                    imaged: true,
                    dets,
                });
            }
        }
        for (ti, t) in taskings.iter().enumerate() {
            if t.pass_ms >= clock.end_ms() {
                continue;
            }
            done.push((*uid, ti));
            let sun = sun_elevation(t.centre, clock.start_utc_ms as f64 + t.pass_ms as f64);
            let min_sun = rules.realism().satellites.eo_min_sun_elevation_rad;
            let report_at = t.pass_ms + delay;
            if pl.kind == PayloadKind::Optical && sun < min_sun {
                pics.reports.push(PendingReport {
                    at_ms: report_at,
                    side: *side,
                    satellite: *uid,
                    pass_ms: t.pass_ms,
                    outcome: format!(
                        "No image: sun {:.0}° at the target (optical needs {:.0}°)",
                        wb_core::units::rad_to_deg(sun),
                        wb_core::units::rad_to_deg(min_sun)
                    ),
                    imaged: false,
                    dets: Vec::new(),
                });
                continue;
            }
            let gsd = pl.gsd_m.unwrap_or(10.0);
            let pass = &rules.realism().passive;
            let level = if gsd < pass.eo_gsd_type_m {
                ContactLevel::Precise
            } else if gsd < pass.eo_gsd_class_m {
                ContactLevel::Class
            } else {
                ContactLevel::Domain
            };
            let mut dets = Vec::new();
            for b in bodies.iter().filter(|b| b.visible && b.side != *side) {
                if b.domain == Domain::Air
                    || distance_m(t.centre, b.pos) > t.radius_m
                    || distance_m(sub, b.pos) > pl.swath_m / 2.0 + t.radius_m
                {
                    continue;
                }
                let sigma = (5.0 * gsd).max(rules.realism().fusion.contact_sigma_floor_m);
                dets.push(detection(
                    seed,
                    &clock,
                    *side,
                    *uid,
                    sensor,
                    SourceKind::Imagery,
                    b,
                    b.type_ref,
                    level,
                    sigma,
                    gsd < pass.eo_gsd_type_m,
                ));
            }
            let n = dets.len();
            let kind = if pl.kind == PayloadKind::Sar {
                "radar"
            } else {
                "optical"
            };
            pics.reports.push(PendingReport {
                at_ms: report_at,
                side: *side,
                satellite: *uid,
                pass_ms: t.pass_ms,
                outcome: match n {
                    0 => format!("Area imaged ({kind}): nothing found"),
                    1 => format!("Area imaged ({kind}): 1 object"),
                    _ => format!("Area imaged ({kind}): {n} objects"),
                },
                imaged: true,
                dets,
            });
        }
    }
    pics.reports.sort_by_key(|r| (r.at_ms, r.side, r.satellite));
    world.insert_resource(pics);
    for (uid, ti) in done.into_iter().rev() {
        if let Some(e) = crate::world::entity(world, uid)
            && let Some(mut st) = world.get_mut::<SatelliteState>(e)
            && ti < st.taskings.len()
        {
            st.taskings.remove(ti);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn detection(
    seed: u64,
    clock: &SimClock,
    side: u8,
    sat: UnitId,
    sensor: SensorIx,
    kind: SourceKind,
    b: &Body,
    type_ref: crate::picture::TypeRef,
    level: ContactLevel,
    sigma: f64,
    resolved: bool,
) -> Detection {
    let ent = (u64::from(sat.0) << 32) | u64::from(b.uid.0);
    let z = |n: u32| rng_normal(seed, RngPurpose::Satellite, ent, clock.tick, n);
    let p1 = destination_point(b.pos, 0.0, z(0) * sigma);
    let at = destination_point(p1, core::f64::consts::FRAC_PI_2, z(1) * sigma);
    Detection {
        side,
        observer: sat,
        sensor: Some(sensor),
        kind,
        truth: b.key,
        tick: clock.tick,
        at_ms: clock.sim_time_ms,
        range_m: 0.0,
        domain: b.domain,
        type_ref,
        group_size: b.group_size,
        level,
        count_resolved: resolved,
        quality: Quality::Report,
        position: Some(Measured {
            at,
            alt_m: b.ground_m,
            heading: 0.0,
            speed_mps: 0.0,
            a_m: sigma,
            b_m: sigma,
            rot: 0.0,
        }),
        bearing: None,
        network: None,
        fixed: b.fixed,
    }
}

/// Ground track of an own satellite, `[lon, lat]` degrees every `step_s`.
pub fn ground_track(
    world: &World,
    side: u8,
    id: u32,
    before_min: f64,
    after_min: f64,
    step_s: f64,
) -> Result<Vec<[f64; 2]>, String> {
    let unknown = || format!("Unknown satellite {id}");
    let e = crate::world::entity(world, UnitId(id)).ok_or_else(unknown)?;
    if world.get::<Allegiance>(e).map(|a| a.side) != Some(side)
        || world.get::<SatelliteState>(e).is_none()
    {
        return Err(unknown());
    }
    let rules = world.resource::<Rules>();
    let clock = world.resource::<SimClock>();
    let p = world.get::<PlatformRef>(e).ok_or_else(unknown)?.0;
    let s = sgp4(rules, p).ok_or_else(unknown)?;
    let now = clock.start_utc_ms as f64 + clock.sim_time_ms as f64;
    let step = step_s.clamp(10.0, 600.0) * 1_000.0;
    let n = ((before_min.clamp(0.0, 600.0) + after_min.clamp(0.0, 600.0)) * 60_000.0 / step) as u32;
    let start = now - before_min.clamp(0.0, 600.0) * 60_000.0;
    Ok((0..=n)
        .filter_map(|i| position(&s, start + f64::from(i) * step))
        .map(|(p, _)| [p.lon_deg(), p.lat_deg()])
        .collect())
}
