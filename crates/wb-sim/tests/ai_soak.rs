//! Watch-mode soak (doc 12 §5, P9 acceptance): both sides AI on the Theatre North
//! content, garrisoned, for 24 h of sim time: no panic, bounded memory, logged
//! decisions. Slow, so ignored by default; run in release:
//!
//! ```text
//! cargo test --release -p wb-sim --test ai_soak -- --ignored --nocapture
//! WB_SOAK_HOURS=2 cargo test --release -p wb-sim --test ai_soak -- --ignored --nocapture
//! ```
//!
//! Per sim hour it prints the snapshot size (the whole war state), the event and
//! order log lengths, contacts per side, platforms alive and decisions per side.
//! "Bounded" means the war state stops growing: the largest snapshot of the
//! second half is at most 1.5 × the largest of the first half. The logs are
//! append-only by design (reports and replays read them), so they are checked
//! for a steady rate instead.

use std::sync::Arc;
use std::time::Instant;
use wb_sim::Sim;
use wb_sim::ai::AiBrains;
use wb_sim::components::{Allegiance, PlatformRef};
use wb_sim::events::EventKind;
use wb_sim::picture::SidePictures;
use wb_sim::setup::{SideSetup, WarSetup, build_scenario};
use wb_sim::world::{Controller, Difficulty, PlayMode, UnitIndex};

fn content() -> (Arc<wb_content::ContentDb>, [u8; 32]) {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../content");
    let set = wb_content::load_dir(std::path::Path::new(dir)).expect("content loads");
    let db = wb_content::compile(&set);
    let (header, _) = wb_content::encode(&db, &set.manifest.content_version);
    (Arc::new(db), header.content_hash)
}

fn watch_setup() -> WarSetup {
    let side = |name: &str, nations: &[&str], difficulty| SideSetup {
        name: name.to_owned(),
        nations: nations.iter().map(|n| (*n).to_owned()).collect(),
        controller: Controller::Ai,
        difficulty,
        roe: wb_sim::components::Roe::Bravo,
    };
    WarSetup {
        name: "Watch soak".to_owned(),
        seed: 24,
        start_utc: "2026-10-27T06:00:00Z".to_owned(),
        start_mode: wb_sim::scenario::StartMode::Garrisoned,
        mode: Some(PlayMode::Watch),
        sides: vec![
            side("Straits League", &["ald", "hlv", "mrv"], Difficulty::Hard),
            side("Eastern Pact", &["vsm", "kld"], Difficulty::Normal),
        ],
    }
}

/// One line of the hourly log (printed with `{:?}`; some fields are only read there).
#[derive(Debug)]
#[allow(dead_code)]
struct Hour {
    hour: u64,
    snapshot_kb: usize,
    events: usize,
    orders: usize,
    contacts: [usize; 2],
    alive: [usize; 2],
    decisions: [u64; 2],
    refused: usize,
    wall_s: f64,
}

fn sample(sim: &mut Sim, hour: u64, wall_s: f64) -> Hour {
    let w = sim.world_mut();
    let snap = wb_sim::snapshot::capture(w);
    let kb = |b: Result<Vec<u8>, postcard::Error>| b.map_or(0, |b| b.len() / 1024);
    let snapshot_kb = kb(postcard::to_stdvec(&snap));
    if std::env::var("WB_SOAK_FIELDS").is_ok() {
        eprintln!(
            "  state KB: units {} pictures {} combat {} orders {} roe {} operations {} ai {} deployment {}",
            kb(postcard::to_stdvec(&snap.units)),
            kb(postcard::to_stdvec(&snap.pictures)),
            kb(postcard::to_stdvec(&snap.combat)),
            kb(postcard::to_stdvec(&snap.orders)),
            kb(postcard::to_stdvec(&snap.roe_requests)),
            kb(postcard::to_stdvec(&snap.operations)),
            kb(postcard::to_stdvec(&snap.ai)),
            kb(postcard::to_stdvec(&snap.deployment)),
        );
    }
    let events = w.resource::<wb_sim::events::EventLog>().0.len();
    let orders = w.resource::<wb_sim::orders::OrderLog>().0.len();
    let pics = w.resource::<SidePictures>();
    let contacts = [pics.sides[0].contacts.len(), pics.sides[1].contacts.len()];
    let mut alive = [0, 0];
    for e in w.resource::<UnitIndex>().0.values() {
        if w.get::<PlatformRef>(*e).is_some()
            && let Some(a) = w.get::<Allegiance>(*e)
        {
            alive[usize::from(a.side.min(1))] += 1;
        }
    }
    let brains = w.resource::<AiBrains>();
    let decisions = [
        brains.0.get(&0).map_or(0, |s| s.decisions),
        brains.0.get(&1).map_or(0, |s| s.decisions),
    ];
    let refused = w
        .resource::<wb_sim::events::EventLog>()
        .0
        .iter()
        .filter(|e| {
            matches!(
                &e.kind,
                EventKind::AiDecision {
                    refused: Some(_),
                    ..
                }
            )
        })
        .count();
    // The AI keeps nothing that grows without bound.
    for s in brains.0.values() {
        assert!(s.stations.len() <= 8, "CAP stations bounded");
        assert!(
            s.threats.len() <= contacts[0].max(contacts[1]) + 1,
            "threats follow the picture"
        );
    }
    Hour {
        hour,
        snapshot_kb,
        events,
        orders,
        contacts,
        alive,
        decisions,
        refused,
        wall_s,
    }
}

#[test]
#[ignore = "24 h of sim time; run in release with --ignored"]
fn watch_mode_soak() {
    let hours: u64 = std::env::var("WB_SOAK_HOURS")
        .ok()
        .and_then(|h| h.parse().ok())
        .unwrap_or(24);
    let (db, hash) = content();
    let built = build_scenario(&db, &watch_setup()).expect("setup builds");
    let mut sim = Sim::new(&built.scenario, db, hash, None).expect("war builds");
    sim.set_recording(false);
    assert_eq!(sim.sides().mode, PlayMode::Watch);
    let started = Instant::now();
    let mut log = vec![sample(&mut sim, 0, 0.0)];
    for hour in 1..=hours {
        let until = hour * 3_600_000;
        while sim.clock().sim_time_ms < until {
            sim.step();
        }
        let h = sample(&mut sim, hour, started.elapsed().as_secs_f64());
        eprintln!("{h:?}");
        log.push(h);
    }
    let last = log.last().expect("hours");
    assert!(
        last.decisions[0] > 0 && last.decisions[1] > 0,
        "both AIs decided"
    );
    // AI decisions are logged, each to its own side.
    let ai_events = sim
        .events()
        .iter()
        .filter(|e| matches!(e.kind, EventKind::AiDecision { .. }))
        .count();
    assert!(ai_events > 0);
    assert!(
        sim.events()
            .iter()
            .filter(|e| matches!(e.kind, EventKind::AiDecision { .. }))
            .all(|e| e.sides == 1 || e.sides == 2)
    );
    // Bounded: the war state of the second half stays within 1.5 × the first half's.
    let mid = log.len() / 2;
    let first = log[..mid].iter().map(|h| h.snapshot_kb).max().unwrap_or(0);
    let second = log[mid..].iter().map(|h| h.snapshot_kb).max().unwrap_or(0);
    assert!(
        second * 2 <= first.max(1) * 3,
        "war state keeps growing: {first} KB → {second} KB"
    );
    // The event log grows at a steady rate (no runaway loop of decisions).
    if log.len() > 4 {
        let rate = |a: &Hour, b: &Hour| (b.events - a.events) as f64 / (b.hour - a.hour) as f64;
        let early = rate(&log[1], &log[mid]);
        let late = rate(&log[mid], last);
        assert!(
            late <= early.max(100.0) * 2.0,
            "event rate ran away: {early:.0}/h → {late:.0}/h"
        );
    }
    eprintln!(
        "soak: {hours} h sim in {:.0} s, {} events, {} orders, {} AI decisions ({} refused), state {} KB",
        last.wall_s, last.events, last.orders, ai_events, last.refused, last.snapshot_kb
    );
}
