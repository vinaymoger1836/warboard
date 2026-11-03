//! Flyout tables (ADR-028): how far each weapon gets and how fast, from its
//! calibration launch conditions.
//!
//! Rockets are flown once through the real flight model (`weapons::fly`,
//! loft included) in a scratch world, toward a point far beyond their range;
//! air-breathing weapons fly at cruise speed until their range is used;
//! glide weapons glide from the calibration altitude. Each table holds, per
//! second of flight, the ground distance flown and `f_energy`. Against a
//! target whose speed toward the shooter is `v` (negative: opening), the
//! weapon reaches `max_i(d_i + v·t_i)`. A pure function of the content,
//! built on first use and never saved.

use bevy_ecs::prelude::*;
use core::f64::consts::FRAC_PI_2;
use wb_content::compiled::{Weapon, WeaponIx};
use wb_content::source::weapon::{Calibration, ProfileKind};
use wb_core::UnitId;
use wb_core::geo::{LatLon, destination_point};
use wb_core::math::sqrt;

use crate::clock::{RngSeed, SimClock, TickLen};
use crate::components::{
    AimPoint, Allegiance, FlightPhase, Identity, Kinematics, SeekerState, WeaponFlight,
    WeaponTarget,
};
use crate::picture::SidePictures;
use crate::rules::Rules;
use crate::snapshot::base_resources;
use crate::weapons::Combat;
use crate::world::{ForceFineTick, UnitIdAlloc, spawn_unit};

/// One weapon's table: per second of flight, ground distance (m) and
/// `f_energy` at that point.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Flyout {
    pub t_s: Vec<f64>,
    pub d_m: Vec<f64>,
    pub f_energy: Vec<f64>,
}

impl Flyout {
    /// Furthest launch range against a target closing at `v_close` m/s.
    pub fn max_range(&self, v_close: f64) -> f64 {
        self.t_s
            .iter()
            .zip(&self.d_m)
            .map(|(t, d)| d + v_close * t)
            .fold(0.0, f64::max)
    }

    /// Time of flight and `f_energy` at intercept of a target `range_m` away
    /// closing at `v_close`; `None` beyond reach.
    pub fn intercept(&self, range_m: f64, v_close: f64) -> Option<(f64, f64)> {
        (0..self.t_s.len())
            .find(|&i| self.d_m[i] + v_close * self.t_s[i] >= range_m)
            .map(|i| (self.t_s[i], self.f_energy[i]))
    }
}

fn calibration<'a>(rules: &'a Rules, w: &'a Weapon) -> Option<&'a Calibration> {
    w.profile
        .calibration
        .as_ref()
        .or_else(|| rules.realism().weapons.calibration.get(&w.category))
}

/// Tables for every weapon, in content order.
pub fn build(rules: &Rules) -> Vec<Flyout> {
    (0..rules.db.weapons.len() as u32)
        .map(|i| one(rules, WeaponIx(i)))
        .collect()
}

fn one(rules: &Rules, wix: WeaponIx) -> Flyout {
    let w = &rules.db.weapons[wix.0 as usize];
    let cal = calibration(rules, w);
    let alt = cal.map_or(0.0, |c| c.launch_alt_m);
    let v0 = cal.map_or(0.0, |c| c.launch_speed_mach) * rules.speed_of_sound(alt);
    let mut out = Flyout::default();
    match w.profile.kind {
        ProfileKind::AirBreathing => {
            let v = (w.profile.max_speed_mach * rules.speed_of_sound(alt.max(0.0))).max(1.0);
            let end = w.profile.max_range_m / v;
            let mut t = 0.0;
            while t <= end + 1.0 {
                out.t_s.push(t.min(end));
                out.d_m.push((v * t).min(w.profile.max_range_m));
                out.f_energy.push(1.0);
                t += 1.0;
            }
        }
        ProfileKind::Glide => {
            let v = v0.max(1.0);
            let end = w.profile.max_range_m / v;
            let mut t = 0.0;
            while t <= end + 1.0 {
                out.t_s.push(t.min(end));
                out.d_m.push((v * t).min(w.profile.max_range_m));
                out.f_energy.push(1.0);
                t += 1.0;
            }
        }
        ProfileKind::Rocket | ProfileKind::Ballistic => fly_rocket(rules, wix, alt, v0, &mut out),
    }
    out
}

/// Flies a rocket in a scratch world toward a point far beyond its range,
/// sampling once a second until it is out of energy.
fn fly_rocket(rules: &Rules, wix: WeaponIx, alt: f64, v0: f64, out: &mut Flyout) {
    let w = &rules.db.weapons[wix.0 as usize];
    let target_alt = calibration(rules, w).map_or(0.0, |c| c.target_alt_m);
    let mut world = World::new();
    base_resources(&mut world, rules.clone());
    world.insert_resource(SimClock {
        tick: 0,
        sim_time_ms: 0,
        tick_len: TickLen::Fine,
        start_utc_ms: 0,
    });
    world.insert_resource(RngSeed(0));
    world.insert_resource(ForceFineTick(true));
    world.insert_resource(UnitIdAlloc { next: 2 });
    world.insert_resource(SidePictures::new(2));
    world.resource_mut::<Combat>().in_flight = 1;
    let from = LatLon::new(0.0, 0.0);
    let far = destination_point(from, FRAC_PI_2, w.profile.max_range_m * 4.0);
    let id = UnitId(1);
    spawn_unit(
        &mut world,
        id,
        (
            Identity {
                unit: id,
                name: "flyout".into(),
            },
            Allegiance {
                side: 0,
                country: wb_content::compiled::CountryIx(0),
            },
            Kinematics {
                lat: from.lat,
                lon: from.lon,
                alt_m: alt,
                heading: FRAC_PI_2,
                speed_mps: v0,
                climb_mps: 0.0,
            },
            WeaponFlight {
                weapon: wix,
                shooter: UnitId(0),
                salvo: 0,
                tag: 0,
                target: WeaponTarget::Point {
                    at: far,
                    alt_m: target_alt,
                },
                aim: AimPoint {
                    at: far,
                    alt_m: target_alt,
                    heading: 0.0,
                    speed_mps: 0.0,
                    climb_mps: 0.0,
                    at_ms: 0,
                },
                phase: FlightPhase::Boost,
                seeker: SeekerState::Off,
                fpa: 0.0,
                launch_ms: 0,
                launch_range_m: 0.0,
                v0_mps: v0,
                vmax_mps: w.profile.max_speed_mach * rules.speed_of_sound(alt),
                flown_m: 0.0,
                manoeuvre_loss_mps: 0.0,
                intercept: false,
                cm_used: Vec::new(),
                target_aware: false,
            },
        ),
    );
    let vmax = w.profile.max_speed_mach * rules.speed_of_sound(alt);
    for tick in 0..(7_200 * 10) {
        let Some(e) = crate::world::entity(&world, id) else {
            break;
        };
        if tick % 10 == 0 {
            let k = world.get::<Kinematics>(e).expect("kinematics");
            let f = world.get::<WeaponFlight>(e).expect("weapon");
            let vmin = w.profile.min_speed_mach.unwrap_or(0.0) * rules.speed_of_sound(k.alt_m);
            out.t_s.push(f64::from(tick) / 10.0);
            out.d_m.push(f.flown_m);
            out.f_energy.push(sqrt(
                ((k.speed_mps - vmin) / (vmax - vmin).max(1.0)).clamp(0.0, 1.0),
            ));
        }
        crate::weapons::fly(&mut world);
        let mut c = world.resource_mut::<SimClock>();
        c.tick += 1;
        c.sim_time_ms += 100;
    }
}
