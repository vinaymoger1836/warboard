//! Canonical state hash (doc 02 §3–4).
//!
//! BLAKE3 over a hand-written byte encoding: fixed field order, little-endian
//! integers, `f64` as raw bits (NaN canonicalised), entities sorted by
//! `UnitId`. Any change to this encoding changes every hash, so bump
//! [`FORMAT_VERSION`] when you change it.

use crate::clock::{RngSeed, SimClock};
use crate::components::{Identity, Kinematics};
use crate::probe::ProbeState;
use bevy_ecs::world::World;

const MAGIC: &[u8; 8] = b"WB-STATE";
pub const FORMAT_VERSION: u8 = 1;

/// Little-endian writer into a BLAKE3 hasher.
struct Canon(blake3::Hasher);

impl Canon {
    fn u8(&mut self, v: u8) {
        self.0.update(&[v]);
    }
    fn u32(&mut self, v: u32) {
        self.0.update(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.update(&v.to_le_bytes());
    }
    fn i64(&mut self, v: i64) {
        self.0.update(&v.to_le_bytes());
    }
    fn f64(&mut self, v: f64) {
        let bits = if v.is_nan() {
            f64::NAN.to_bits()
        } else {
            v.to_bits()
        };
        self.u64(bits);
    }
    fn str(&mut self, s: &str) {
        self.u32(u32::try_from(s.len()).expect("string longer than 4 GiB"));
        self.0.update(s.as_bytes());
    }
    fn len(&mut self, n: usize) {
        self.u32(u32::try_from(n).expect("more than u32::MAX items"));
    }
}

pub fn state_hash(world: &mut World) -> [u8; 32] {
    let mut c = Canon(blake3::Hasher::new());
    c.0.update(MAGIC);
    c.u8(FORMAT_VERSION);

    let clock = world.resource::<SimClock>().clone();
    c.u64(clock.tick);
    c.u64(clock.sim_time_ms);
    c.u32(clock.tick_len.ms());
    c.i64(clock.start_utc_ms);
    c.u64(world.resource::<RngSeed>().0);

    match world.get_resource::<ProbeState>() {
        Some(p) => {
            c.u8(1);
            c.f64(p.point.lat);
            c.f64(p.point.lon);
            c.u64(p.acc);
        }
        None => c.u8(0),
    }

    let mut query = world.query::<(&Identity, &Kinematics)>();
    let mut units: Vec<_> = query.iter(world).collect();
    units.sort_by_key(|(id, _)| id.unit);
    c.len(units.len());
    for (id, k) in units {
        c.u32(id.unit.0);
        c.str(&id.name);
        c.f64(k.lat);
        c.f64(k.lon);
        c.f64(k.alt_m);
        c.f64(k.heading);
        c.f64(k.speed_mps);
    }

    *c.0.finalize().as_bytes()
}

pub fn to_hex(bytes: &[u8; 32]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::with_capacity(64), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}
