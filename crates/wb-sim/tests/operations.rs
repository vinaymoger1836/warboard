//! Operations, reports and replay (P7 acceptance): the Rising Sun golden
//! (G13, `scenarios/golden/rising-sun.yaml`) run end to end; report totals
//! against the raw events; every replay checkpoint re-simulated to its hash;
//! a save mid-operation continued; the planner's DAG checks.

use std::collections::BTreeMap;
use std::sync::Arc;
use wb_sim::events::{EventKind, Outcome};
use wb_sim::operations::{OpState, Operations, PhaseState, Severity};
use wb_sim::orders::Command;
use wb_sim::replay::Replay;
use wb_sim::{Order, Sim};

fn fixtures() -> (Arc<wb_content::ContentDb>, [u8; 32]) {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../scenarios/fixtures");
    let set = wb_content::load_dir(std::path::Path::new(dir)).expect("fixtures load");
    let db = wb_content::compile(&set);
    let (header, _) = wb_content::encode(&db, &set.manifest.content_version);
    (Arc::new(db), header.content_hash)
}

const PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../scenarios/golden/rising-sun.yaml"
);

fn rising_sun() -> Sim {
    let (db, hash) = fixtures();
    let yaml = std::fs::read_to_string(PATH).expect("scenario");
    Sim::from_yaml(&yaml, db, hash, None).expect("builds")
}

fn op(sim: &Sim) -> wb_sim::operations::Operation {
    sim.world().resource::<Operations>().ops[0].clone()
}

#[test]
fn rising_sun_runs_end_to_end() {
    let mut sim = rising_sun();
    sim.step();
    while op(&sim).state == OpState::Running && sim.clock().tick < 40_000 {
        sim.step();
    }
    let o = op(&sim);
    assert_eq!(o.state, OpState::Completed, "{o:#?}");
    assert!(
        o.phases.iter().all(|p| p.state == PhaseState::Done),
        "every phase completes"
    );
    let start = |key: &str| {
        o.phases
            .iter()
            .find(|p| p.key == key)
            .and_then(|p| p.started)
            .expect("started")
    };
    assert_eq!(
        start("strike-1").tick,
        start("strike-2").tick,
        "phases 4 and 5 start together"
    );
    // Phase 4 waits for phase 3's arrival and phase 2's start; phase 6 for
    // phase 2 + 10 min.
    assert!(start("strike-1").tick >= start("blind").tick);
    assert_eq!(start("suppress").ms, start("blind").ms + 600_000);
    // The frigate kept its patrol (fire missions leave the task alone).
    let ev = sim.events();
    assert!(ev.iter().any(|e| matches!(&e.kind,
        EventKind::PhaseStarted { name, .. } if name == "Strike ship 2")));
}

/// Counts straight from the event log, the way the report must add up.
fn raw_counts(sim: &Sim, side: u8) -> BTreeMap<&'static str, u32> {
    let mut c = BTreeMap::new();
    for e in sim.events().iter().filter(|e| e.sides & (1 << side) != 0) {
        let k = match &e.kind {
            EventKind::WeaponLaunched { .. } => "launched",
            EventKind::WeaponTerminated { outcome, .. } => match outcome {
                Outcome::Hit => "hits",
                Outcome::Intercepted => "intercepted",
                Outcome::Decoyed => "decoyed",
                Outcome::Miss => "misses",
                Outcome::Jammed => "jammed",
                _ => "other",
            },
            EventKind::UnitDestroyed { .. } => "losses",
            _ => continue,
        };
        *c.entry(k).or_default() += 1;
    }
    c
}

#[test]
fn report_totals_equal_event_counts() {
    let mut sim = rising_sun();
    sim.run_ticks(14_000);
    for side in [0u8, 1] {
        let r = wb_sim::reports::build(sim.world(), sim.events(), side, false).expect("report");
        let raw = raw_counts(&sim, side);
        let get = |k| raw.get(k).copied().unwrap_or(0);
        let t = r.totals;
        assert_eq!(t.launched, get("launched"), "side {side} launched");
        assert_eq!(t.hits, get("hits"), "side {side} hits");
        assert_eq!(t.intercepted, get("intercepted"), "side {side} intercepted");
        assert_eq!(t.decoyed, get("decoyed"), "side {side} decoyed");
        assert_eq!(t.misses, get("misses"), "side {side} misses");
        assert_eq!(t.losses, get("losses"), "side {side} losses");
        // The tables add up to the totals.
        let s = |f: fn(&wb_sim::reports::SalvoLine) -> u32| r.salvos.iter().map(f).sum::<u32>();
        assert_eq!(s(|x| x.count), t.launched);
        assert_eq!(s(|x| x.hits), t.hits);
        assert_eq!(s(|x| x.intercepted), t.intercepted);
        assert_eq!(s(|x| x.decoyed), t.decoyed);
        assert_eq!(s(|x| x.in_flight), t.in_flight);
        assert_eq!(
            t.launched,
            t.hits + t.misses + t.intercepted + t.decoyed + t.jammed + t.other + t.in_flight
        );
        assert_eq!(
            r.munitions.iter().map(|m| m.expended).sum::<u32>(),
            t.launched
        );
        assert_eq!(r.losses.len() as u32, t.losses);
        // Exports build, and the JSON round-trips.
        for table in wb_sim::reports::CSV_TABLES {
            let _ = wb_sim::reports::to_csv(&r, table);
        }
        assert!(wb_sim::reports::to_html(&r).contains("After-action report"));
        let json = serde_json::to_string(&r).expect("json");
        let back: wb_sim::reports::Report = serde_json::from_str(&json).expect("parse");
        assert_eq!(back, r);
    }
    // Blue's operation report: every phase with its tally.
    let r = wb_sim::reports::build(sim.world(), sim.events(), 0, false).expect("report");
    let o = &r.operations[0];
    assert_eq!(o.phases.len(), 6);
    assert_eq!(o.phases[3].tally.launched + o.phases[4].tally.launched, 6);
    // During the war the report holds no hidden enemy names (rule 1).
    let json = serde_json::to_string(&r).expect("json");
    for hidden in ["Red corvette", "Red SAM battery"] {
        assert!(!json.contains(hidden), "Blue's report names `{hidden}`");
    }
    // The truth review waits for the end of the war, then names both sides.
    assert!(wb_sim::reports::build(sim.world(), sim.events(), 0, true).is_err());
    let tick = sim.clock().tick;
    sim.submit(&Order {
        tick,
        side: 0,
        command: Command::EndWar,
    })
    .expect("end war");
    sim.step();
    let r = wb_sim::reports::build(sim.world(), sim.events(), 0, true).expect("truth");
    let truth = r.truth.expect("truth review");
    assert!(
        truth.sides[1]
            .units
            .iter()
            .any(|u| u.name == "Red corvette B")
    );
    assert!(truth.hits.iter().any(|h| h.target == "Red corvette A"));
    assert!(!truth.contacts.is_empty());
}

#[test]
fn replay_reproduces_every_checkpoint_hash() {
    let mut sim = rising_sun();
    let mut hashes = BTreeMap::new();
    while sim.clock().tick < 14_000 {
        // An order from outside the scenario, mid-run: a replay must queue it
        // again on the tick it was given.
        if sim.clock().tick == 4_200 {
            let tick = sim.clock().tick;
            sim.submit(&Order {
                tick,
                side: 0,
                command: Command::SetEmcon {
                    units: vec![10],
                    emcon: wb_content::source::Emcon::Off,
                },
            })
            .expect("order");
        }
        sim.step();
        if sim.clock().tick.is_multiple_of(1_700) {
            hashes.insert(sim.clock().tick, sim.state_hash());
        }
    }
    let mut r = Replay::of(&sim).expect("replay");
    assert!(
        r.checkpoints().len() >= 5,
        "a checkpoint every 10 sim minutes: {}",
        r.checkpoints().len()
    );
    for w in r.checkpoints().windows(2) {
        assert!(w[1].sim_time_ms - w[0].sim_time_ms <= 600_000 + 1_000);
    }
    for i in 0..r.checkpoints().len() {
        let (want, got) = r.verify(i).expect("verify");
        assert_eq!(want, got, "checkpoint {i}");
    }
    // Seeking backwards and forwards lands on the recorded hashes.
    let mut ticks: Vec<u64> = hashes.keys().copied().collect();
    ticks.reverse();
    ticks.push(*hashes.keys().next().expect("a tick"));
    for t in ticks {
        r.seek(t).expect("seek");
        assert_eq!(r.sim().state_hash(), hashes[&t], "seek to tick {t}");
    }
}

#[test]
fn save_mid_operation_continues_identically() {
    let (db, hash) = fixtures();
    let mut whole = rising_sun();
    whole.run_ticks(6_000);
    let mut part = rising_sun();
    part.run_ticks(3_000);
    assert_eq!(op(&part).state, OpState::Running);
    let bytes = part.save("rising sun", Vec::new(), 0);
    let mut loaded = Sim::load(&bytes, db, hash, None).expect("loads");
    assert!(
        !loaded.recorder().checkpoints.is_empty(),
        "checkpoints are saved"
    );
    loaded.run_ticks(3_000);
    assert_eq!(loaded.state_hash_hex(), whole.state_hash_hex());
}

#[test]
fn planner_rejects_cycles_and_flags_issues() {
    let mut sim = rising_sun();
    let spec = |trigger_a: &str, trigger_b: &str| -> wb_sim::operations::OperationSpec {
        serde_yaml::from_str(&format!(
            "name: Loop\nphases:\n  - {{ id: a, name: A, trigger: {trigger_a} }}\n  - {{ id: b, name: B, trigger: {trigger_b} }}\n"
        ))
        .expect("spec")
    };
    let geo = None;
    let world = sim.world_mut();
    let cyc = wb_sim::operations::check(
        world,
        geo,
        0,
        &spec(
            "{ kind: phase_ended, phase: b }",
            "{ kind: phase_started, phase: a }",
        ),
    );
    assert_eq!(cyc[0].severity, Severity::Error);
    assert!(cyc[0].text.contains("cycle"), "{}", cyc[0].text);
    let unknown = wb_sim::operations::check(
        world,
        geo,
        0,
        &spec("{ kind: start }", "{ kind: phase_ended, phase: zz }"),
    );
    assert!(unknown[0].text.contains("No phase `zz`"));
    let phase_only =
        wb_sim::operations::check(world, geo, 0, &spec("{ kind: arrived }", "{ kind: start }"));
    assert_eq!(phase_only[0].severity, Severity::Error);
    let ok = wb_sim::operations::check(
        world,
        geo,
        0,
        &spec(
            "{ kind: start }",
            "{ kind: phase_started, phase: a, delay_min: 5 }",
        ),
    );
    assert!(ok.iter().all(|i| i.severity != Severity::Error), "{ok:?}");
}
