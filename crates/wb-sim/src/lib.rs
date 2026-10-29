//! The WarBoard engine. Owns the truth; clients get side views only.
//!
//! A [`Sim`] is an ECS world (bevy_ecs) stepped by a fixed list of exclusive
//! stages on one thread (doc 02 §3–4). Every stage visits units in `UnitId`
//! order wherever order matters. P3 adds movement, fuel, bases, tasks,
//! orders, events, saves and the side view; P4 sensors, satellites and the
//! side pictures (fog of war).

pub mod bases;
pub mod clock;
pub mod components;
pub mod details;
pub mod events;
pub mod fuel;
pub mod movement;
pub mod orders;
pub mod picture;
pub mod placement;
pub mod probe;
#[cfg(feature = "ts")]
pub mod protocol;
pub mod rules;
pub mod satellites;
pub mod save;
pub mod scenario;
pub mod sensors;
pub mod snapshot;
pub mod tasks;
pub mod view;
pub mod world;

use bevy_ecs::world::World;
use std::rc::Rc;
use std::sync::Arc;
use wb_content::ContentDb;
use wb_geo::Geo;

pub use clock::{EngagementState, RngSeed, SimClock, TickLen};
pub use components::{Identity, Kinematics};
pub use events::{Event, EventKind};
pub use orders::{Order, OrderAck};
pub use rules::Rules;
pub use save::{SaveError, SaveHeader};
pub use scenario::{Scenario, ScenarioError};

/// Longest real-time step one `advance` call accepts. Longer gaps (a
/// backgrounded tab, a slow frame) are dropped, so the device shows a lower
/// achieved speed instead of trying to catch up.
pub const MAX_REAL_DT_MS: f64 = 250.0;

/// One stage of a tick: an exclusive system over the world.
pub type Stage = (&'static str, fn(&mut World));

/// The tick, in order (doc 02 §3 multi-rate table: stages with their own rate
/// check `SimClock::due` themselves).
pub const STAGES: &[Stage] = &[
    ("orders", orders::apply_due),
    ("tick_len", clock::choose_tick_len),
    ("tasks", tasks::run),
    ("movement", movement::integrate),
    ("formation", movement::formation),
    ("satellites", satellites::update),
    ("scripted", sensors::move_scripted),
    ("sensors", sensors::scan),
    ("fusion", picture::fusion),
    ("fuel", fuel::burn),
    ("aar", fuel::aar),
    ("arrivals", bases::arrivals),
    ("logistics", bases::logistics),
    ("probe", probe::determinism_probe),
    ("clock", clock::advance_clock),
];

/// One running war.
pub struct Sim {
    world: World,
    /// Sim milliseconds owed by `advance` but not yet run as a whole tick.
    /// Speed and real time are not part of the simulation and never hashed.
    accum_ms: f64,
    /// Region packs (or a scenario's test terrain): order validation and,
    /// through `GeoRes`, line of sight every tick (ADR-022).
    geo: Option<Rc<Geo>>,
    /// `(tick, state hash)` every `hash_checkpoint_ticks` (desync checks).
    checkpoints: Vec<(u64, [u8; 32])>,
}

impl Sim {
    /// Builds a world from a scenario and a content pack.
    pub fn new(
        scenario: &Scenario,
        content: Arc<ContentDb>,
        content_hash: [u8; 32],
        geo: Option<Rc<Geo>>,
    ) -> Result<Self, ScenarioError> {
        let rules = Rules::new(content, content_hash);
        let geo = match &scenario.test_terrain {
            Some(t) => Some(Rc::new(t.geo().map_err(ScenarioError::Generated)?)),
            None => geo,
        };
        let world = scenario.build(rules, geo.as_deref())?;
        let mut sim = Self {
            world,
            accum_ms: 0.0,
            geo: None,
            checkpoints: Vec::new(),
        };
        sim.set_geo(geo);
        Ok(sim)
    }

    /// Parses YAML and builds a world.
    pub fn from_yaml(
        yaml: &str,
        content: Arc<ContentDb>,
        content_hash: [u8; 32],
        geo: Option<Rc<Geo>>,
    ) -> Result<Self, ScenarioError> {
        Self::new(&Scenario::from_yaml(yaml)?, content, content_hash, geo)
    }

    /// Restores a war from a `.wbsave`; the content must be the one it was
    /// saved with.
    pub fn load(
        bytes: &[u8],
        content: Arc<ContentDb>,
        content_hash: [u8; 32],
        geo: Option<Rc<Geo>>,
    ) -> Result<Self, SaveError> {
        let (header, body) = save::decode(bytes)?;
        let installed = snapshot::to_hex(&content_hash);
        if body.snapshot.content_hash != content_hash {
            return Err(SaveError::Content {
                save: header.content_hash,
                installed,
            });
        }
        let mut world = snapshot::restore(body.snapshot, Rules::new(content, content_hash));
        world.resource_mut::<orders::OrderLog>().0 = body.order_log;
        world.resource_mut::<events::EventLog>().0 = body.events;
        let mut sim = Self {
            world,
            accum_ms: 0.0,
            geo: None,
            checkpoints: Vec::new(),
        };
        sim.set_geo(geo);
        Ok(sim)
    }

    /// Writes a `.wbsave`. `created_utc_ms` is the client's wall clock (only
    /// recorded, never read by the engine).
    pub fn save(&self, war_name: &str, packs: Vec<String>, created_utc_ms: i64) -> Vec<u8> {
        let snap = snapshot::capture(&self.world);
        let clock = self.clock();
        let header = SaveHeader {
            war_name: war_name.to_owned(),
            content_hash: snapshot::to_hex(&snap.content_hash),
            packs,
            created_utc_ms,
            tick: clock.tick,
            sim_time_ms: clock.sim_time_ms,
            start_utc_ms: clock.start_utc_ms,
        };
        let body = save::SaveBody {
            snapshot: snap,
            order_log: self.world.resource::<orders::OrderLog>().0.clone(),
            events: self.world.resource::<events::EventLog>().0.clone(),
        };
        save::encode(&header, &body)
    }

    /// Installs the region packs. A war with test terrain keeps it instead.
    pub fn set_geo(&mut self, geo: Option<Rc<Geo>>) {
        let geo = match self.world.get_resource::<scenario::TestTerrain>() {
            Some(t) => t.geo().ok().map(Rc::new),
            None => geo,
        };
        self.world
            .insert_non_send(sensors::GeoRes(geo.clone()));
        self.geo = geo;
    }

    /// Runs exactly one tick.
    pub fn step(&mut self) {
        self.step_with(|_, run, world| run(world));
    }

    /// Runs one tick, handing each stage to `wrap` (the CLI times them; the
    /// engine itself never reads a clock).
    pub fn step_with(&mut self, mut wrap: impl FnMut(&'static str, fn(&mut World), &mut World)) {
        for (name, run) in STAGES {
            wrap(name, *run, &mut self.world);
        }
        let every = u64::from(
            self.world
                .resource::<Rules>()
                .realism()
                .time
                .hash_checkpoint_ticks
                .max(1),
        );
        let tick = self.clock().tick;
        if tick.is_multiple_of(every) {
            let h = snapshot::state_hash(&self.world);
            self.checkpoints.push((tick, h));
        }
    }

    /// Runs exactly `n` ticks.
    pub fn run_ticks(&mut self, n: u64) {
        for _ in 0..n {
            self.step();
        }
    }

    /// Adds `real_dt_ms × speed` of sim time and runs every whole tick that
    /// fits. Never skips ticks; returns the number of ticks run.
    pub fn advance(&mut self, real_dt_ms: f64, speed: f64) -> u32 {
        if !(real_dt_ms.is_finite() && speed.is_finite()) || real_dt_ms <= 0.0 || speed <= 0.0 {
            return 0;
        }
        self.accum_ms += real_dt_ms.min(MAX_REAL_DT_MS) * speed;
        let mut ran = 0;
        loop {
            let next = f64::from(clock::next_tick_len(&self.world).ms());
            if self.accum_ms < next {
                break;
            }
            self.accum_ms -= next;
            self.step();
            ran += 1;
        }
        ran
    }

    /// Validates and queues an order; the reason if rejected.
    pub fn submit(&mut self, order: &Order) -> Result<OrderAck, String> {
        orders::submit(&mut self.world, self.geo.as_deref(), order)
    }

    pub fn clock(&self) -> &SimClock {
        self.world.resource::<SimClock>()
    }

    pub fn sides(&self) -> &world::Sides {
        self.world.resource::<world::Sides>()
    }

    /// All events so far (filter with `EventLog::visible` for a side).
    pub fn events(&self) -> &[Event] {
        &self.world.resource::<events::EventLog>().0
    }

    pub fn checkpoints(&self) -> &[(u64, [u8; 32])] {
        &self.checkpoints
    }

    /// BLAKE3 of the canonical state, see [`snapshot`].
    pub fn state_hash(&self) -> [u8; 32] {
        snapshot::state_hash(&self.world)
    }

    pub fn state_hash_hex(&self) -> String {
        snapshot::to_hex(&self.state_hash())
    }

    pub fn world(&self) -> &World {
        &self.world
    }

    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }
}
