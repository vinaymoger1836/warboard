//! Event log (doc 02 §15, doc 04 §7.3). Reports, alerts and the golden checks
//! are built from it. Each event names the sides that may see it.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use wb_core::UnitId;

use crate::clock::SimClock;
use crate::components::ReturnWhy;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
// Externally tagged (`{"Landed": {...}}`): saves store events with postcard,
// which cannot read internally tagged enums.
pub enum EventKind {
    /// A group's task changed.
    TaskChanged {
        group: UnitId,
        task: String,
    },
    /// A group arrived on (or left) its station.
    StationChanged {
        group: UnitId,
        on: bool,
    },
    /// Bingo: the unit heads home; it should land with `reserve_kg`.
    BingoRtb {
        unit: UnitId,
        fuel_kg: f64,
        reserve_kg: f64,
        base: UnitId,
    },
    /// A group heads home for another reason.
    Return {
        group: UnitId,
        base: UnitId,
        why: ReturnWhy,
    },
    /// A group diverts to a tanker.
    ToTanker {
        group: UnitId,
        tanker: UnitId,
    },
    /// A receiver is full (or the tanker dry); fuel after refuelling.
    Refuelled {
        unit: UnitId,
        tanker: UnitId,
        fuel_kg: f64,
    },
    TookOff {
        unit: UnitId,
        base: UnitId,
    },
    Landed {
        unit: UnitId,
        base: UnitId,
        fuel_kg: f64,
    },
    TurnaroundDone {
        unit: UnitId,
        base: UnitId,
    },
    BaseActivated {
        base: UnitId,
    },
    /// The client should drop to ×1 or pause (doc 02 §3).
    AutoSlow {
        reason: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Event {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub tick: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub sim_time_ms: u64,
    /// Bit `s` set: side `s` sees it.
    pub sides: u8,
    pub kind: EventKind,
}

#[derive(Resource, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EventLog(pub Vec<Event>);

impl EventLog {
    /// Events side `side` may see, from index `from`.
    pub fn visible(&self, side: u8, from: usize) -> impl Iterator<Item = &Event> {
        self.0
            .get(from..)
            .unwrap_or_default()
            .iter()
            .filter(move |e| e.sides & (1 << side) != 0)
    }
}

/// Appends an event at the current tick.
pub fn emit(world: &mut World, side: u8, kind: EventKind) {
    let clock = world.resource::<SimClock>();
    let ev = Event {
        tick: clock.tick,
        sim_time_ms: clock.sim_time_ms,
        sides: 1 << side,
        kind,
    };
    world.resource_mut::<EventLog>().0.push(ev);
}
