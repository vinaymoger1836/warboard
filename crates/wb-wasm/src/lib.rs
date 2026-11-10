//! Browser bindings. The engine runs inside a dedicated Web Worker (ADR-002).
//!
//! The worker reads each side view straight out of WebAssembly memory
//! (`view_ptr`, `encode_view`), copies it into one of its two transfer
//! buffers and posts it; JSON carries the rest (details, orders, events).

use std::sync::Arc;
use wasm_bindgen::prelude::*;
use wb_content::ContentDb;
use wb_sim::view::{UMPIRE_SIDE, ViewEncoder};
use wb_sim::world::PlayMode;
use wb_sim::{Order, Sim};

mod admin;
mod geo;
pub use admin::{TestBench, build_content_pack, check_content, content_pack_info};
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
    /// Whose view the client is looking at: a side, or [`UMPIRE_SIDE`] (doc 12).
    /// The session's `active_side` is the side whose orders the client gives.
    view: u8,
    /// Events already handed to the client, per side.
    events_sent: Vec<usize>,
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

    /// Builds an engine from the war setup screens' choices (JSON `WarSetup`,
    /// doc 12 §4) on a content pack: garrisoned from the inventories, or with
    /// the human sides' forces waiting to be deployed (ADR-046).
    pub fn create_from_setup(setup_json: &str, content_pack: &[u8]) -> Result<Engine, JsError> {
        let (db, hash) = content(content_pack)?;
        let setup: wb_sim::setup::WarSetup = serde_json::from_str(setup_json)
            .map_err(|e| JsError::new(&format!("The war setup is not valid: {e}")))?;
        let built =
            wb_sim::setup::build_scenario(&db, &setup).map_err(|e| JsError::new(&e.to_string()))?;
        let sim =
            Sim::new(&built.scenario, db, hash, None).map_err(|e| JsError::new(&e.to_string()))?;
        Ok(Self::wrap(sim))
    }

    /// Continues a war from a `.wbsave`.
    pub fn load(save: &[u8], content_pack: &[u8]) -> Result<Engine, JsError> {
        let (db, hash) = content(content_pack)?;
        let sim = Sim::load(save, db, hash, None).map_err(|e| JsError::new(&e.to_string()))?;
        Ok(Self::wrap(sim))
    }

    fn wrap(sim: Sim) -> Engine {
        let view = sim.session().active_side;
        let sides = sim.sides().len();
        Engine {
            sim,
            encoder: ViewEncoder::new(),
            view,
            events_sent: vec![0; sides],
            replay: None,
        }
    }

    /// A side the client may ask about: a human side, or either side when
    /// two AIs play (Watch). Everything per-side goes through here.
    fn viewable(&self, side: u8) -> Result<u8, JsError> {
        let sides = self.sim.sides();
        if usize::from(side) >= sides.len() {
            return Err(JsError::new(&format!("There is no side {side}")));
        }
        if sides.is_ai(side) && sides.mode != PlayMode::Watch {
            return Err(JsError::new("That side is controlled by the AI"));
        }
        Ok(side)
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

    /// Sides as JSON: `{ side, mode, sides: [{ name, nations, controller, … }],
    /// session }`. `side` is the active side.
    pub fn sides(&self) -> String {
        json(&serde_json::json!({
            "side": self.sim.session().active_side,
            "mode": self.sim.sides().mode,
            "sides": self.sim.sides().sides,
            "session": self.sim.session().view(),
        }))
    }

    /// The session (active side, hot-seat settings, which sides have a PIN,
    /// umpire flags) as JSON.
    pub fn session(&self) -> String {
        json(&self.sim.session().view())
    }

    /// Moves the screen and the orders to a side (doc 12 §2.1). Never touches
    /// the war, so it cannot change the state hash.
    pub fn set_active_side(&mut self, side: u8) -> Result<(), JsError> {
        let side = self.viewable(side)?;
        self.sim
            .set_active_side(side)
            .map_err(|e| JsError::new(&e))?;
        self.view = side;
        Ok(())
    }

    /// Looks at another picture without changing who gives orders: a side
    /// (Watch), or the umpire view (`255`, Command both when turned on).
    pub fn set_view_side(&mut self, side: u8) -> Result<(), JsError> {
        if side == UMPIRE_SIDE {
            self.sim
                .session()
                .umpire_allowed(self.sim.sides())
                .map_err(|e| JsError::new(&e))?;
            self.view = UMPIRE_SIDE;
        } else {
            self.view = self.viewable(side)?;
        }
        Ok(())
    }

    /// Umpire view on or off (Command both only). Turning it off while it is
    /// showing returns the screen to the active side.
    pub fn set_umpire(&mut self, on: bool) -> Result<(), JsError> {
        self.sim.set_umpire(on).map_err(|e| JsError::new(&e))?;
        if !on && self.view == UMPIRE_SIDE {
            self.view = self.sim.session().active_side;
        }
        Ok(())
    }

    /// Hot-seat settings (JSON `HotSeat`).
    pub fn set_hot_seat(&mut self, settings_json: &str) -> Result<(), JsError> {
        let hs = serde_json::from_str(settings_json)
            .map_err(|e| JsError::new(&format!("Hot-seat settings are not valid: {e}")))?;
        self.sim.set_hot_seat(hs).map_err(|e| JsError::new(&e))
    }

    /// Sets a side's 4-digit PIN with the client's random 16-byte `salt`; an
    /// empty `pin` clears it. A courtesy between players, not security.
    pub fn set_pin(&mut self, side: u8, pin: &str, salt: &[u8]) -> Result<(), JsError> {
        let salt: [u8; 16] = salt
            .try_into()
            .map_err(|_| JsError::new("The PIN salt must be 16 bytes"))?;
        let pin = (!pin.is_empty()).then_some(pin);
        self.sim
            .set_pin(side, pin, salt)
            .map_err(|e| JsError::new(&e))
    }

    /// True when the side has no PIN or `pin` is right.
    pub fn check_pin(&self, side: u8, pin: &str) -> bool {
        self.sim.session().check_pin(side, pin)
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
        if let Some(r) = &self.replay {
            let side = if self.view == UMPIRE_SIDE {
                self.sim.session().active_side
            } else {
                self.view
            };
            return self.encoder.encode(r.sim().world(), side).len();
        }
        if self.view == UMPIRE_SIDE {
            match self.sim.umpire_view(&mut self.encoder) {
                Ok(bytes) => return bytes.len(),
                // Turned off meanwhile: back to the active side.
                Err(_) => self.view = self.sim.session().active_side,
            }
        }
        self.encoder.encode(self.sim.world(), self.view).len()
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
    pub fn details(&self, side: u8, id: u32) -> Result<String, JsError> {
        let side = self.viewable(side)?;
        wb_sim::details::details(self.current().world(), side, id)
            .map(|d| json(&d))
            .map_err(|e| JsError::new(&e))
    }

    /// Contact card JSON for a contact of this side's picture: estimates only.
    pub fn contact_details(&self, side: u8, id: u32) -> Result<String, JsError> {
        let side = self.viewable(side)?;
        wb_sim::details::contact_details(self.current().world(), side, id)
            .map(|d| json(&d))
            .map_err(|e| JsError::new(&e))
    }

    /// Content inventories of this side's own nations (Forces panel), JSON
    /// (`NationInventory[]`).
    pub fn inventory(&self, side: u8) -> Result<String, JsError> {
        let side = self.viewable(side)?;
        let rules = self.sim.world().resource::<wb_sim::Rules>();
        Ok(json(&wb_sim::inventory::inventory(
            rules,
            self.sim.sides(),
            side,
        )))
    }

    /// What a side still has to deploy and where (JSON `DeploymentView`): its
    /// pool and its preset bases. Own side only.
    pub fn deployment(&mut self, side: u8) -> Result<String, JsError> {
        let side = self.viewable(side)?;
        Ok(json(&wb_sim::deployment::view(self.sim.world_mut(), side)))
    }

    /// Open ROE approval requests of this side, JSON (`ApprovalView[]`).
    pub fn approvals(&self, side: u8) -> Result<String, JsError> {
        let side = self.viewable(side)?;
        Ok(json(&wb_sim::roe::approvals(self.current().world(), side)))
    }

    /// Ground track of an own satellite, `[[lon, lat, alt_m], …]` every `step_s`.
    pub fn ground_track(
        &self,
        side: u8,
        id: u32,
        before_min: f64,
        after_min: f64,
        step_s: f64,
    ) -> Result<String, JsError> {
        let side = self.viewable(side)?;
        wb_sim::satellites::ground_track(self.sim.world(), side, id, before_min, after_min, step_s)
            .map(|t| json(&t))
            .map_err(|e| JsError::new(&e))
    }

    /// Submits an order (JSON, doc 04 §7.2) for the active side: the client
    /// cannot name another side, whatever the JSON says (doc 12 §1). The
    /// umpire view is read-only. Returns `{"ok":true,"ack":{…}}` or
    /// `{"ok":false,"reason":"…"}`.
    pub fn order(&mut self, order_json: &str) -> String {
        let reply = match serde_json::from_str::<Order>(order_json) {
            Err(e) => {
                serde_json::json!({ "ok": false, "reason": format!("Order is not valid: {e}") })
            }
            Ok(_) if self.view == UMPIRE_SIDE => {
                serde_json::json!({ "ok": false, "reason": "The umpire view is read-only" })
            }
            Ok(mut o) => {
                o.side = self.sim.session().active_side;
                match self.sim.submit(&o) {
                    Ok(ack) => serde_json::json!({ "ok": true, "ack": ack }),
                    Err(reason) => serde_json::json!({ "ok": false, "reason": reason }),
                }
            }
        };
        json(&reply)
    }

    /// Events `side` may see since the last call for that side, as a JSON
    /// array. Each side has its own cursor, so a waiting side's events are
    /// kept until the client asks (doc 12 §2).
    pub fn take_events(&mut self, side: u8) -> Result<String, JsError> {
        let side = self.viewable(side)?;
        let all = self.sim.events();
        let cursor = &mut self.events_sent[usize::from(side)];
        let new: Vec<_> = all[(*cursor).min(all.len())..]
            .iter()
            .filter(|e| e.sides & (1 << side) != 0)
            .collect();
        *cursor = all.len();
        Ok(json(&new))
    }

    /// The side's operations for the planner (`OperationView[]`).
    pub fn operations(&self, side: u8) -> Result<String, JsError> {
        let side = self.viewable(side)?;
        Ok(json(&wb_sim::operations::views(
            self.current().world(),
            side,
        )))
    }

    /// Planner validation of an `OperationSpec` (JSON): `Issue[]`, errors first.
    pub fn check_operation(&mut self, spec_json: &str) -> String {
        let side = self.sim.session().active_side;
        match serde_json::from_str::<wb_sim::operations::OperationSpec>(spec_json) {
            Err(e) => json(&vec![wb_sim::operations::Issue {
                severity: wb_sim::operations::Severity::Error,
                phase: None,
                text: format!("The plan is not valid: {e}"),
            }]),
            Ok(spec) => {
                let geo = self.sim.geo_shared();
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
    pub fn report(&self, side: u8, truth: bool, format: &str) -> Result<String, JsError> {
        let side = self.viewable(side)?;
        let sim = self.current();
        let mut r = wb_sim::reports::build(sim.world(), sim.events(), side, truth)
            .map_err(|e| JsError::new(&e))?;
        r.umpire_used = self.sim.session().umpire_used;
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

/// The nations of a content pack for the coalition builder, JSON
/// (`NationOption[]`): content only, no war.
#[wasm_bindgen]
pub fn nation_options(content_pack: &[u8]) -> Result<String, JsError> {
    let (db, _) = content(content_pack)?;
    Ok(json(&wb_sim::setup::nation_options(&db)))
}

/// Checks a war setup without starting it, JSON:
/// `{ ok, error?, report?, extent? }` where `report` lists forces that found no
/// base and `extent` is the `[west, south, east, north]` the region packs must
/// cover (ADR-022).
#[wasm_bindgen]
pub fn preview_setup(setup_json: &str, content_pack: &[u8]) -> Result<String, JsError> {
    let (db, _) = content(content_pack)?;
    let setup: wb_sim::setup::WarSetup = match serde_json::from_str(setup_json) {
        Ok(s) => s,
        Err(e) => {
            return Ok(json(
                &serde_json::json!({ "ok": false, "error": format!("The war setup is not valid: {e}") }),
            ));
        }
    };
    Ok(match wb_sim::setup::build_scenario(&db, &setup) {
        Err(e) => json(&serde_json::json!({ "ok": false, "error": e.to_string() })),
        Ok(built) => {
            let opts = wb_sim::setup::nation_options(&db);
            let mut ext = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
            for o in opts
                .iter()
                .filter(|o| setup.sides.iter().any(|s| s.nations.contains(&o.id)))
            {
                ext = [
                    ext[0].min(o.extent[0]),
                    ext[1].min(o.extent[1]),
                    ext[2].max(o.extent[2]),
                    ext[3].max(o.extent[3]),
                ];
            }
            json(&serde_json::json!({
                "ok": true,
                "report": built.report,
                "extent": ext,
                "mode": built.scenario.mode,
            }))
        }
    })
}
