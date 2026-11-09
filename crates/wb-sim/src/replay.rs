//! Replay (doc 03 §12, doc 04 §7.1, ADR-016): a checkpoint every 10 sim
//! minutes, and seeking by re-simulation from the nearest earlier one.
//!
//! A checkpoint is the canonical snapshot (postcard, zstd), its state hash
//! and the length of the event log at that moment. Seeking restores the
//! checkpoint, cuts the event log back to that length and runs forward,
//! queueing each logged order again on the tick it was first submitted
//! (`QueuedOrder::submitted`), so the queue holds what it held then and every
//! hash on the way matches the recorded run.
//!
//! Replays run on their own [`Replay`] (a second `Sim`), so scrubbing never
//! touches the war in progress.

use serde::{Deserialize, Serialize};
use std::io::Read;
use std::rc::Rc;

use crate::events::{Event, EventLog};
use crate::orders::{OrderLog, OrderQueue, QueuedOrder};
use crate::rules::Rules;
use crate::snapshot::{self, Snapshot};
use crate::{Sim, SimClock};

/// Sim time between checkpoints (doc 03 §12).
pub const CHECKPOINT_MS: u64 = 600_000;
/// Checkpoints kept; beyond it the older half is thinned (doc 04 §7.1 "pruned").
pub const MAX_CHECKPOINTS: usize = 144;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Checkpoint {
    pub tick: u64,
    pub sim_time_ms: u64,
    pub hash: [u8; 32],
    /// Events in the log when it was taken.
    pub events: u64,
    /// zstd(postcard(Snapshot)).
    pub snapshot: Vec<u8>,
}

/// Checkpoints of a war (kept by [`Sim`], saved with it).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Recorder {
    pub on: bool,
    pub checkpoints: Vec<Checkpoint>,
    /// Sim time of the next checkpoint.
    pub next_ms: u64,
}

/// Takes a checkpoint of the world now.
pub fn capture(world: &bevy_ecs::world::World) -> Checkpoint {
    let snap = snapshot::capture(world);
    let raw = postcard::to_stdvec(&snap).expect("snapshot serializes");
    let clock = world.resource::<SimClock>();
    Checkpoint {
        tick: clock.tick,
        sim_time_ms: clock.sim_time_ms,
        hash: snapshot::hash_of(&raw),
        events: world.resource::<EventLog>().0.len() as u64,
        snapshot: ruzstd::encoding::compress_to_vec(
            raw.as_slice(),
            ruzstd::encoding::CompressionLevel::Fastest,
        ),
    }
}

/// The snapshot inside a checkpoint.
pub fn decode(cp: &Checkpoint) -> Result<Snapshot, String> {
    let mut dec = ruzstd::decoding::StreamingDecoder::new(cp.snapshot.as_slice())
        .map_err(|e| format!("checkpoint is corrupt: {e}"))?;
    let mut raw = Vec::new();
    dec.read_to_end(&mut raw)
        .map_err(|e| format!("checkpoint is corrupt: {e}"))?;
    postcard::from_bytes(&raw).map_err(|e| format!("checkpoint is corrupt: {e}"))
}

impl Recorder {
    /// Called after every tick: takes a checkpoint when one is due.
    pub fn after_tick(&mut self, world: &bevy_ecs::world::World) {
        if !self.on {
            return;
        }
        let now = world.resource::<SimClock>().sim_time_ms;
        if now < self.next_ms {
            return;
        }
        self.checkpoints.push(capture(world));
        self.next_ms = (now / CHECKPOINT_MS + 1) * CHECKPOINT_MS;
        if self.checkpoints.len() > MAX_CHECKPOINTS {
            // Drop every second checkpoint of the older half (never the first).
            let half = self.checkpoints.len() / 2;
            let mut i = 0usize;
            self.checkpoints.retain(|_| {
                let keep = i == 0 || i >= half || i.is_multiple_of(2);
                i += 1;
                keep
            });
        }
    }

    /// Starts recording now (a new war, or a loaded one without checkpoints).
    pub fn start(&mut self, world: &bevy_ecs::world::World) {
        self.on = true;
        if self.checkpoints.is_empty() {
            self.checkpoints.push(capture(world));
        }
        let now = world.resource::<SimClock>().sim_time_ms;
        self.next_ms = (now / CHECKPOINT_MS + 1) * CHECKPOINT_MS;
    }
}

/// The world at a checkpoint: its snapshot, the orders submitted before it
/// and the events logged before it.
fn world_at(
    rules: &Rules,
    orders: &[QueuedOrder],
    events: &[Event],
    cp: &Checkpoint,
) -> Result<bevy_ecs::world::World, String> {
    let snap = decode(cp)?;
    let next_seq = snap.orders.next_seq;
    let mut world = snapshot::restore(snap, rules.clone());
    world.resource_mut::<OrderLog>().0 = orders
        .iter()
        .filter(|o| o.seq < next_seq)
        .cloned()
        .collect();
    world.resource_mut::<EventLog>().0 =
        events.get(..cp.events as usize).unwrap_or(events).to_vec();
    Ok(world)
}

/// A replay of a war: its checkpoints and logs, and a sim positioned at
/// some tick of it.
pub struct Replay {
    rules: Rules,
    geo: Option<Rc<wb_geo::Geo>>,
    checkpoints: Vec<Checkpoint>,
    orders: Vec<QueuedOrder>,
    events: Vec<Event>,
    /// Last tick of the recording.
    pub end_tick: u64,
    sim: Sim,
}

impl Replay {
    /// A replay of `live` up to now, positioned at its first checkpoint.
    pub fn of(live: &Sim) -> Result<Self, String> {
        let world = live.world();
        let checkpoints = live.recorder().checkpoints.clone();
        let first = checkpoints
            .first()
            .ok_or("This war has no checkpoints to replay")?;
        let rules = world.resource::<Rules>().clone();
        let orders = world.resource::<OrderLog>().0.clone();
        let events = world.resource::<EventLog>().0.clone();
        let geo = live.geo_rc();
        let start = world_at(&rules, &orders, &events, first)?;
        Ok(Self {
            sim: Sim::from_world(start, geo.clone()),
            rules,
            geo,
            orders,
            events,
            end_tick: live.clock().tick,
            checkpoints,
        })
    }

    pub fn checkpoints(&self) -> &[Checkpoint] {
        &self.checkpoints
    }

    pub fn sim(&self) -> &Sim {
        &self.sim
    }

    /// Positions the sim at checkpoint `i`.
    fn restore(&mut self, i: usize) -> Result<(), String> {
        let world = world_at(
            &self.rules,
            &self.orders,
            &self.events,
            &self.checkpoints[i],
        )?;
        self.sim = Sim::from_world(world, self.geo.clone());
        Ok(())
    }

    /// Re-simulates to `tick` (clamped to the recording) from the nearest
    /// checkpoint at or before it, or from where the sim is when that is
    /// closer.
    pub fn seek(&mut self, tick: u64) -> Result<(), String> {
        let tick = tick.min(self.end_tick);
        let i = self
            .checkpoints
            .iter()
            .rposition(|c| c.tick <= tick)
            .unwrap_or(0);
        let here = self.sim.clock().tick;
        if here > tick || self.checkpoints[i].tick > here {
            self.restore(i)?;
        }
        while self.sim.clock().tick < tick {
            self.step_one();
        }
        Ok(())
    }

    /// One tick forward, re-queueing the orders first submitted now.
    pub fn step_one(&mut self) {
        let now = self.sim.clock().tick;
        let world = self.sim.world_mut();
        let have = world.resource::<OrderQueue>().next_seq;
        let due: Vec<QueuedOrder> = self
            .orders
            .iter()
            .filter(|o| o.seq >= have && o.submitted <= now)
            .cloned()
            .collect();
        for o in due {
            let mut q = world.resource_mut::<OrderQueue>();
            q.next_seq = q.next_seq.max(o.seq + 1);
            q.pending.push(o.clone());
            q.pending.sort_by_key(|x| (x.tick, x.seq));
            world.resource_mut::<OrderLog>().0.push(o);
        }
        self.sim.step();
    }

    /// The hash recorded at checkpoint `i` and the hash a re-simulation from
    /// the previous checkpoint (or the start) reaches at the same tick.
    pub fn verify(&mut self, i: usize) -> Result<([u8; 32], [u8; 32]), String> {
        let target = self.checkpoints[i].tick;
        self.restore(i.saturating_sub(1))?;
        while self.sim.clock().tick < target {
            self.step_one();
        }
        Ok((self.checkpoints[i].hash, self.sim.state_hash()))
    }
}
