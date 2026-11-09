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
    /// A replay being watched (doc 03 §12); the war itself is paused meanwhile.
    replay: Option<wb_sim::replay::Replay>,
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
            replay: None,
        }
    }

    /// The sim the client sees: the replay while one is open, else the war.
    fn current(&self) -> &Sim {
        self.replay.as_ref().map_or(&self.sim, |r| r.sim())
    }

    fn replay_json(&self) -> String {
        match &self.replay {
            None => json(&serde_json::json!({ "active": false })),
            Some(r) => json(&serde_json::json!({
                "active": true,
                "tick": r.sim().clock().tick,
                "sim_time_ms": r.sim().clock().sim_time_ms,
                "end_tick": r.end_tick,
                "end_ms": self.sim.clock().sim_time_ms,
                "checkpoints": r.checkpoints().iter()
                    .map(|c| serde_json::json!({ "tick": c.tick, "sim_time_ms": c.sim_time_ms }))
                    .collect::<Vec<_>>(),
            })),
        }
    }

    /// Region packs: line of sight every tick (ADR-022) and order checks
    /// (ship routes, base placement).
    pub fn set_geo(&mut self, geo: &GeoQuery) {
        self.sim.set_geo(Some(geo.shared()));
    }

    pub fn clear_geo(&mut self) {
        self.sim.set_geo(None);
    }

    /// Sides as JSON: `{ side, sides: [{ name, nations, player }] }`.
    pub fn sides(&self) -> String {
        json(&serde_json::json!({
            "side": self.side,
            "sides": self.sim.sides().sides,
        }))
    }

    /// Adds `real_dt_ms × speed` of sim time and runs every whole tick that
    /// fits. Returns the number of ticks run.
    pub fn advance(&mut self, real_dt_ms: f64, speed: f64) -> u32 {
        if self.replay.is_some() {
            return 0;
        }
        self.sim.advance(real_dt_ms, speed)
    }

    /// Runs exactly `n` ticks (tests and the determinism check).
    pub fn run_ticks(&mut self, n: u32) {
        self.sim.run_ticks(u64::from(n));
    }

    /// Ticks completed. Exact up to 2^53.
    pub fn tick(&self) -> f64 {
        self.current().clock().tick as f64
    }

    /// Sim time since war start, ms. Exact up to 2^53.
    pub fn sim_time_ms(&self) -> f64 {
        self.current().clock().sim_time_ms as f64
    }

    /// War start, Unix epoch ms UTC.
    pub fn start_utc_ms(&self) -> f64 {
        self.sim.clock().start_utc_ms as f64
    }

    pub fn tick_len_ms(&self) -> u32 {
        self.current().clock().tick_len.ms()
    }

    /// BLAKE3 state hash, 64 hex chars.
    pub fn state_hash(&self) -> String {
        self.sim.state_hash_hex()
    }

    /// Encodes the side view; read it at `view_ptr()` for the returned length
    /// before the next call into the engine.
    pub fn encode_view(&mut self) -> usize {
        let world = self.replay.as_ref().map_or(self.sim.world(), |r| r.sim().world());
        self.encoder.encode(world, self.side).len()
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
        wb_sim::details::details(self.current().world(), self.side, id)
            .map(|d| json(&d))
            .map_err(|e| JsError::new(&e))
    }

    /// Contact card JSON for a contact of this side's picture: estimates only.
    pub fn contact_details(&self, id: u32) -> Result<String, JsError> {
        wb_sim::details::contact_details(self.current().world(), self.side, id)
            .map(|d| json(&d))
            .map_err(|e| JsError::new(&e))
    }

    /// Open ROE approval requests of this side, JSON (`ApprovalView[]`).
    pub fn approvals(&self) -> String {
        json(&wb_sim::roe::approvals(self.current().world(), self.side))
    }

    /// Ground track of an own satellite, `[[lon, lat], …]` every `step_s`.
    pub fn ground_track(
        &self,
        id: u32,
        before_min: f64,
        after_min: f64,
        step_s: f64,
    ) -> Result<String, JsError> {
        wb_sim::satellites::ground_track(
            self.sim.world(),
            self.side,
            id,
            before_min,
            after_min,
            step_s,
        )
        .map(|t| json(&t))
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

    /// The side's operations for the planner (`OperationView[]`).
    pub fn operations(&self) -> String {
        json(&wb_sim::operations::views(self.current().world(), self.side))
    }

    /// Planner validation of an `OperationSpec` (JSON): `Issue[]`, errors first.
    pub fn check_operation(&mut self, spec_json: &str) -> String {
        match serde_json::from_str::<wb_sim::operations::OperationSpec>(spec_json) {
            Err(e) => json(&vec![wb_sim::operations::Issue {
                severity: wb_sim::operations::Severity::Error,
                phase: None,
                text: format!("The plan is not valid: {e}"),
            }]),
            Ok(spec) => {
                let geo = self.sim.geo_shared();
                let side = self.side;
                json(&wb_sim::operations::check(
                    self.sim.world_mut(),
                    geo.as_deref(),
                    side,
                    &spec,
                ))
            }
        }
    }

    /// The after-action report (`Report`) of this side; `truth` adds the
    /// truth review once the war has ended. `format`: `json`, `html` or
    /// `csv:<table>`.
    pub fn report(&self, truth: bool, format: &str) -> Result<String, JsError> {
        let sim = self.current();
        let r = wb_sim::reports::build(sim.world(), sim.events(), self.side, truth)
            .map_err(|e| JsError::new(&e))?;
        match format {
            "json" => Ok(json(&r)),
            "html" => Ok(wb_sim::reports::to_html(&r)),
            f => f
                .strip_prefix("csv:")
                .and_then(|t| wb_sim::reports::to_csv(&r, t))
                .ok_or_else(|| JsError::new(&format!("Unknown report format `{f}`"))),
        }
    }

    /// Opens a replay of the war so far at its first checkpoint; the war
    /// pauses. Returns the replay state JSON.
    pub fn replay_start(&mut self) -> Result<String, JsError> {
        let r = wb_sim::replay::Replay::of(&self.sim).map_err(|e| JsError::new(&e))?;
        self.replay = Some(r);
        self.encoder.reset();
        Ok(self.replay_json())
    }

    /// Re-simulates the replay to `tick` (doc 03 §12 scrubber).
    pub fn replay_seek(&mut self, tick: f64) -> Result<String, JsError> {
        let r = self
            .replay
            .as_mut()
            .ok_or_else(|| JsError::new("No replay is open"))?;
        r.seek(tick.max(0.0) as u64).map_err(|e| JsError::new(&e))?;
        self.encoder.reset();
        Ok(self.replay_json())
    }

    /// Closes the replay; the war carries on from where it was.
    pub fn replay_stop(&mut self) -> String {
        self.replay = None;
        self.encoder.reset();
        self.replay_json()
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

/// Map labels of a content pack (doc 11 §2.4) as JSON:
/// `[{ name, kind, nation, lat, lon }]`, degrees; `nation` is a nation id or
/// null. Geography only: no unit, contact or base ever appears here.
#[wasm_bindgen]
pub fn map_labels(content_pack: &[u8]) -> Result<String, JsError> {
    let (db, _) = content(content_pack)?;
    let deg = 180.0 / std::f64::consts::PI;
    let labels: Vec<_> = db
        .places
        .iter()
        .map(|p| {
            serde_json::json!({
                "name": db.str(p.name),
                "kind": p.kind,
                "nation": p.nation.map(|n| db.str(db.nations[n.0 as usize].id)),
                "lat": p.lat_rad * deg,
                "lon": p.lon_rad * deg,
            })
        })
        .collect();
    Ok(json(&labels))
}
