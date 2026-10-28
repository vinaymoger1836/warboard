//! The content pack and the selected realism profile, with the derived
//! numbers movement and fuel need (doc 02 §5–6). Data, not code: nothing here
//! knows a platform by name (CLAUDE.md rule 3).

use bevy_ecs::prelude::*;
use core::f64::consts::PI;
use std::sync::Arc;
use wb_content::compiled::{ContentDb, Platform, PlatformIx, Realism};
use wb_content::source::{Domain, PlatformCategory};
use wb_core::math::sqrt;
use wb_core::units::MACH_SEA_LEVEL_MPS;

/// Standard gravity, m/s².
pub const G0: f64 = 9.806_65;
/// Below this speed the turn-rate formula is held constant (taxi, take-off roll).
const MIN_TURN_SPEED_MPS: f64 = 60.0;

#[derive(Resource, Clone)]
pub struct Rules {
    pub db: Arc<ContentDb>,
    pub realism: usize,
    /// BLAKE3 of the pack body; saves record it.
    pub content_hash: [u8; 32],
}

impl Rules {
    /// Uses the `default` realism profile.
    pub fn new(db: Arc<ContentDb>, content_hash: [u8; 32]) -> Self {
        let realism = db
            .realism
            .iter()
            .position(|r| db.str(r.id) == "default")
            .unwrap_or(0);
        Self {
            db,
            realism,
            content_hash,
        }
    }

    pub fn realism(&self) -> &Realism {
        &self.db.realism[self.realism]
    }

    pub fn platform(&self, ix: PlatformIx) -> &Platform {
        &self.db.platforms[ix.0 as usize]
    }

    pub fn platform_by_id(&self, id: &str) -> Option<PlatformIx> {
        self.db
            .platforms
            .iter()
            .position(|p| self.db.str(p.id) == id)
            .map(|i| PlatformIx(i as u32))
    }

    /// ISA speed of sound at altitude, m/s.
    pub fn speed_of_sound(&self, alt_m: f64) -> f64 {
        let m = &self.realism().movement;
        let h = alt_m.clamp(0.0, m.isa_tropopause_m);
        let t = m.isa_sea_level_temp_k - m.isa_lapse_rate_k_per_m * h;
        sqrt(m.isa_gamma_r * t)
    }

    pub fn is_air(&self, p: &Platform) -> bool {
        p.domain == Domain::Air
    }

    pub fn is_rotary(&self, p: &Platform) -> bool {
        matches!(
            p.category,
            PlatformCategory::HelicopterAttack
                | PlatformCategory::HelicopterUtility
                | PlatformCategory::AircraftUav
        )
    }

    /// Cruise speed at altitude.
    pub fn cruise_mps(&self, p: &Platform, alt_m: f64) -> f64 {
        let m = &p.mobility;
        if let Some(mach) = m.cruise_speed_mach {
            mach * self.speed_of_sound(alt_m)
        } else if let Some(v) = m.cruise_speed_mps {
            v
        } else {
            m.road_speed_mps.unwrap_or(0.0)
        }
    }

    /// Maximum speed at altitude.
    pub fn max_mps(&self, p: &Platform, alt_m: f64) -> f64 {
        let m = &p.mobility;
        if let Some(mach) = m.max_speed_mach {
            mach * self.speed_of_sound(alt_m)
        } else if let Some(v) = m.max_speed_mps {
            v
        } else {
            self.cruise_mps(p, alt_m)
        }
    }

    /// Cruise altitude for tasks that give none (0 for surface units).
    pub fn cruise_alt_m(&self, p: &Platform) -> f64 {
        if !self.is_air(p) {
            return 0.0;
        }
        let m = &self.realism().movement;
        let alt = m
            .cruise_alt_by_category_m
            .get(&p.category)
            .copied()
            .unwrap_or(m.aircraft_cruise_alt_m);
        p.mobility.ceiling_m.map_or(alt, |c| alt.min(c))
    }

    pub fn ceiling_m(&self, p: &Platform) -> f64 {
        p.mobility.ceiling_m.unwrap_or(self.cruise_alt_m(p))
    }

    pub fn climb_rate_mps(&self, p: &Platform) -> f64 {
        p.mobility
            .climb_rate_mps
            .unwrap_or(self.realism().movement.aircraft_climb_rate_mps)
    }

    pub fn descent_rate_mps(&self) -> f64 {
        self.realism().movement.aircraft_descent_rate_mps
    }

    pub fn accel_mps2(&self, p: &Platform) -> f64 {
        let m = &self.realism().movement;
        if self.is_air(p) {
            m.aircraft_accel_mps2
        } else {
            m.ship_accel_mps2
        }
    }

    /// Turn rate at speed `v`, rad/s: `g·√(n²−1)/V` for aircraft (sustained
    /// G), `V/r` for ships.
    pub fn turn_rate(&self, p: &Platform, v: f64) -> f64 {
        if self.is_air(p) {
            let n = p
                .mobility
                .sustained_g
                .unwrap_or(self.realism().movement.aircraft_sustained_g);
            G0 * sqrt((n * n - 1.0).max(0.0)) / v.max(MIN_TURN_SPEED_MPS)
        } else {
            let r = p.mobility.turn_radius_m.unwrap_or(500.0).max(1.0);
            v.max(1.0) / r
        }
    }

    /// Time to turn round (180°) at speed `v`.
    pub fn half_turn_s(&self, p: &Platform, v: f64) -> f64 {
        PI / self.turn_rate(p, v).max(1e-6)
    }

    /// Burn multiplier for the altitude band (doc 02 §6).
    pub fn altitude_factor(&self, alt_m: f64) -> f64 {
        let bands = &self.realism().fuel.burn_altitude_factor;
        bands
            .iter()
            .find(|b| alt_m < b.below_m)
            .or(bands.last())
            .map_or(1.0, |b| b.factor)
    }

    /// Aircraft fuel flow, kg/s, at speed, altitude and vertical speed.
    pub fn burn_kg_s(&self, p: &Platform, v: f64, alt_m: f64, climb_mps: f64) -> f64 {
        let Some(burn) = p.fuel.as_ref().and_then(|f| f.burn_kg_s.as_ref()) else {
            return 0.0;
        };
        let military = burn.military.unwrap_or(burn.cruise);
        let cruise_v = self.cruise_mps(p, alt_m);
        let base = if climb_mps > 1.0 {
            military
        } else if climb_mps < -1.0 {
            burn.cruise * self.realism().fuel.descent_burn_fraction
        } else if v <= cruise_v * 1.05 {
            burn.cruise
        } else {
            let max_v = self.max_mps(p, alt_m);
            match burn.afterburner {
                Some(ab) if v > (cruise_v + max_v) / 2.0 => ab,
                _ => military,
            }
        };
        base * self.altitude_factor(alt_m)
    }

    /// Cruise fuel flow at altitude, kg/s (bingo and combat radius).
    pub fn cruise_burn_kg_s(&self, p: &Platform, alt_m: f64) -> f64 {
        p.fuel
            .as_ref()
            .and_then(|f| f.burn_kg_s.as_ref())
            .map_or(0.0, |b| b.cruise * self.altitude_factor(alt_m))
    }

    /// Ship fuel used per metre at speed `v`, as a fraction of a full load.
    pub fn ship_burn_per_m(&self, p: &Platform, v: f64) -> f64 {
        let Some(range) = p.fuel.as_ref().and_then(|f| f.range_m) else {
            return 0.0;
        };
        let vc = self.cruise_mps(p, 0.0).max(0.1);
        let exp = self.realism().fuel.ship_burn_speed_exponent;
        wb_core::math::pow((v / vc).max(0.0), exp) / range.max(1.0)
    }

    /// Fuel capacity: kg for aircraft, 1 (a full load) for ships with a range.
    pub fn fuel_capacity(&self, p: &Platform) -> Option<f64> {
        let f = p.fuel.as_ref()?;
        if let Some(kg) = f.internal_kg {
            Some(kg)
        } else if f.range_m.is_some() {
            Some(1.0)
        } else {
            None
        }
    }

    /// Fuel needed to cover `distance_m` at cruise (aircraft at `alt_m`),
    /// plus a half-turn for aircraft.
    pub fn fuel_for_distance(&self, p: &Platform, distance_m: f64, alt_m: f64) -> f64 {
        if self.is_air(p) {
            let v = self.cruise_mps(p, alt_m).max(1.0);
            let t = distance_m / v + self.half_turn_s(p, v);
            t * self.cruise_burn_kg_s(p, alt_m)
        } else {
            distance_m * self.ship_burn_per_m(p, self.cruise_mps(p, 0.0))
        }
    }

    /// Combat radius from a full load at cruise altitude, out and back with
    /// the bingo reserve.
    pub fn combat_radius_m(&self, p: &Platform) -> Option<f64> {
        let cap = self.fuel_capacity(p)?;
        let usable = cap / (1.0 + self.realism().logistics.fuel_reserve_fraction);
        if self.is_air(p) {
            let alt = self.cruise_alt_m(p);
            let burn = self.cruise_burn_kg_s(p, alt);
            if burn <= 0.0 {
                return None;
            }
            Some(usable / burn * self.cruise_mps(p, alt) / 2.0)
        } else {
            let per_m = self.ship_burn_per_m(p, self.cruise_mps(p, 0.0));
            (per_m > 0.0).then(|| usable / per_m / 2.0)
        }
    }

    /// Mach at altitude (for display).
    pub fn mach(&self, v: f64, alt_m: f64) -> f64 {
        v / self.speed_of_sound(alt_m).max(MACH_SEA_LEVEL_MPS * 0.5)
    }
}
