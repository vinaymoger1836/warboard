//! Browser bindings. The engine runs inside a dedicated Web Worker (ADR-002).

use wasm_bindgen::prelude::*;
use wb_sim::Sim;

mod geo;
pub use geo::{Blake3Hasher, GeoQuery};

#[wasm_bindgen]
pub struct Engine {
    sim: Sim,
}

#[wasm_bindgen]
impl Engine {
    /// Builds an engine from scenario YAML text.
    pub fn create(scenario_yaml: &str) -> Result<Engine, JsError> {
        let sim = Sim::from_yaml(scenario_yaml).map_err(|e| JsError::new(&e.to_string()))?;
        Ok(Engine { sim })
    }

    /// Adds `real_dt_ms × speed` of sim time and runs every whole tick that
    /// fits. Returns the number of ticks run.
    pub fn advance(&mut self, real_dt_ms: f64, speed: f64) -> u32 {
        self.sim.advance(real_dt_ms, speed)
    }

    /// Runs exactly `n` ticks (tests and the determinism check).
    pub fn run_ticks(&mut self, n: u32) {
        self.sim.run_ticks(u64::from(n));
    }

    /// Ticks completed. Exact up to 2^53.
    pub fn tick(&self) -> f64 {
        self.sim.clock().tick as f64
    }

    /// Sim time since war start, ms. Exact up to 2^53.
    pub fn sim_time_ms(&self) -> f64 {
        self.sim.clock().sim_time_ms as f64
    }

    /// War start, Unix epoch ms UTC.
    pub fn start_utc_ms(&self) -> f64 {
        self.sim.clock().start_utc_ms as f64
    }

    pub fn tick_len_ms(&self) -> u32 {
        self.sim.clock().tick_len.ms()
    }

    /// BLAKE3 state hash, 64 hex chars.
    pub fn state_hash(&mut self) -> String {
        self.sim.state_hash_hex()
    }
}
