//! The WarBoard engine. Owns the truth; clients get side views only.
//!
//! P0 scope: an ECS world on a single-threaded schedule, the sim clock with the
//! adaptive tick rule (stubbed input), scenario loading and the canonical state
//! hash used by the native-vs-WASM determinism check.

pub mod clock;
pub mod components;
pub mod hash;
pub mod probe;
#[cfg(feature = "ts")]
pub mod protocol;
pub mod scenario;

use bevy_ecs::schedule::{IntoScheduleConfigs, Schedule, SingleThreadedExecutor};
use bevy_ecs::world::World;

pub use clock::{EngagementState, RngSeed, SimClock, TickLen};
pub use components::{Identity, Kinematics};
pub use scenario::{Scenario, ScenarioError};

/// Longest real-time step one `advance` call accepts. Longer gaps (a
/// backgrounded tab, a slow frame) are dropped, so the device shows a lower
/// achieved speed instead of trying to catch up.
pub const MAX_REAL_DT_MS: f64 = 250.0;

/// One running war.
pub struct Sim {
    world: World,
    schedule: Schedule,
    /// Sim milliseconds owed by `advance` but not yet run as a whole tick.
    /// Speed and real time are not part of the simulation and never hashed.
    accum_ms: f64,
}

impl Sim {
    /// Builds a world from a scenario.
    pub fn new(scenario: &Scenario) -> Self {
        let mut world = World::new();
        scenario.spawn_into(&mut world);

        let mut schedule = Schedule::default();
        schedule.set_executor(SingleThreadedExecutor::new());
        schedule.add_systems(
            (
                clock::choose_tick_len,
                probe::determinism_probe,
                clock::advance_clock,
            )
                .chain(),
        );

        Self {
            world,
            schedule,
            accum_ms: 0.0,
        }
    }

    /// Parses YAML and builds a world.
    pub fn from_yaml(yaml: &str) -> Result<Self, ScenarioError> {
        Ok(Self::new(&Scenario::from_yaml(yaml)?))
    }

    /// Runs exactly one tick.
    pub fn step(&mut self) {
        self.schedule.run(&mut self.world);
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

    pub fn clock(&self) -> &SimClock {
        self.world.resource::<SimClock>()
    }

    /// BLAKE3 of the canonical state, see [`hash`].
    pub fn state_hash(&mut self) -> [u8; 32] {
        hash::state_hash(&mut self.world)
    }

    pub fn state_hash_hex(&mut self) -> String {
        hash::to_hex(&self.state_hash())
    }

    pub fn world(&self) -> &World {
        &self.world
    }
}
