//! Browser bindings. The engine runs inside a dedicated Web Worker (ADR-002).
//!
//! The worker reads each side view straight out of WebAssembly memory
//! (`view_ptr`, `encode_view`), copies it into one of its two transfer
//! buffers and posts it; JSON carries the rest (details, orders, events).

use std::sync::Arc;
use wasm_bindgen::prelude::*;
use wb_content::ContentDb;
use wb_sim::view::ViewEncoder;
use wb_sim::{Order, Sim};

mod geo;
pub use geo::{Blake3Hasher, GeoQuery};

fn content(pack: &[u8]) -> Result<(Arc<ContentDb>, [u8; 32]), JsError> {
    let (header, db) = wb_content::decode(pack).map_err(|e| JsError::new(&e.to_string()))?;
    Ok((Arc::new(db), header.content_hash))
}

fn json<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_string(v).expect("serializes to JSON")
}

#[wasm_bindgen]
pub struct Engine {
    sim: Sim,
    encoder: ViewEncoder,
    /// The side this client plays; views, details and events are for it only.
    side: u8,
    /// Events already handed to the client.
    events_sent: usize,
}

#[wasm_bindgen]
impl Engine {
    /// Builds an engine from scenario YAML and a content pack (`content.wbpack`).
    pub fn create(scenario_yaml: &str, content_pack: &[u8]) -> Result<Engine, JsError> {
        let (db, hash) = content(content_pack)?;
        let sim = Sim::from_yaml(scenario_yaml, db, hash, None)
            .map_err(|e| JsError::new(&e.to_string()))?;
        Ok(Self::wrap(sim))
    }

    /// Continues a war from a `.wbsave`.
    pub fn load(save: &[u8], content_pack: &[u8]) -> Result<Engine, JsError> {
        let (db, hash) = content(content_pack)?;
        let sim = Sim::load(save, db, hash, None).map_err(|e| JsError::new(&e.to_string()))?;
        Ok(Self::wrap(sim))
    }

    fn wrap(sim: Sim) -> Engine {
        let side = sim.sides().sides.iter().position(|s| s.player).unwrap_or(0) as u8;
        Engine {
            sim,
            encoder: ViewEncoder::new(),
            side,
            events_sent: 0,
        }
    }

    /// Region packs used to check orders (ship routes, base placement).
    pub fn set_geo(&mut self, geo: &GeoQuery) {
        self.sim.set_geo(Some(geo.shared()));
    }

    pub fn clear_geo(&mut self) {
        self.sim.set_geo(None);
    }

    /// Sides as JSON: `{ side, sides: [{ name, countries, player }] }`.
    pub fn sides(&self) -> String {
        json(&serde_json::json!({
            "side": self.side,
            "sides": self.sim.sides().sides,
        }))
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
    pub fn state_hash(&self) -> String {
        self.sim.state_hash_hex()
    }

    /// Encodes the side view; read it at `view_ptr()` for the returned length
    /// before the next call into the engine.
    pub fn encode_view(&mut self) -> usize {
        self.encoder.encode(self.sim.world(), self.side).len()
    }

    /// Address of the last encoded view in WebAssembly memory.
    pub fn view_ptr(&mut self) -> *const u8 {
        self.encoder.encode_ptr()
    }

    /// Sends every string again with the next view (the client lost its table).
    pub fn reset_view(&mut self) {
        self.encoder.reset();
    }

    /// Unit card / base card JSON for an own unit, group or base.
    pub fn details(&self, id: u32) -> Result<String, JsError> {
        wb_sim::details::details(self.sim.world(), self.side, id)
            .map(|d| json(&d))
            .map_err(|e| JsError::new(&e))
    }

    /// Submits an order (JSON, doc 04 §7.2) for this client's side. Returns
    /// `{"ok":true,"ack":{…}}` or `{"ok":false,"reason":"…"}`.
    pub fn order(&mut self, order_json: &str) -> String {
        let reply = match serde_json::from_str::<Order>(order_json) {
            Err(e) => {
                serde_json::json!({ "ok": false, "reason": format!("Order is not valid: {e}") })
            }
            Ok(mut o) => {
                o.side = self.side;
                match self.sim.submit(&o) {
                    Ok(ack) => serde_json::json!({ "ok": true, "ack": ack }),
                    Err(reason) => serde_json::json!({ "ok": false, "reason": reason }),
                }
            }
        };
        json(&reply)
    }

    /// Events this side may see since the last call, as a JSON array.
    pub fn take_events(&mut self) -> String {
        let all = self.sim.events();
        let side = self.side;
        let new: Vec<_> = all[self.events_sent.min(all.len())..]
            .iter()
            .filter(|e| e.sides & (1 << side) != 0)
            .collect();
        self.events_sent = all.len();
        json(&new)
    }

    /// Writes a `.wbsave`. `packs_json` is a JSON array of `id@version`.
    pub fn save(&self, war_name: &str, packs_json: &str, created_utc_ms: f64) -> Vec<u8> {
        let packs: Vec<String> = serde_json::from_str(packs_json).unwrap_or_default();
        self.sim.save(war_name, packs, created_utc_ms as i64)
    }
}

/// Reads a save's header (slot lists) as JSON.
#[wasm_bindgen]
pub fn save_header(bytes: &[u8]) -> Result<String, JsError> {
    wb_sim::save::read_header(bytes)
        .map(|(h, _)| json(&h))
        .map_err(|e| JsError::new(&e.to_string()))
}
