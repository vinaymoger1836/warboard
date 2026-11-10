//! Enemy AI v1 (doc 03 §11, P9 acceptance): an incursion into AI territory gets a
//! response within the doctrine reaction time (plus the difficulty's delay) ±20%,
//! and the AI runs only for AI-controlled sides. Generic fixtures (ADR-035).
//!
//! The clock starts when the AI's own picture first shows the intruder hostile and
//! inside its territory, the moment the AI can know of the incursion; how soon
//! that is after the real crossing is the sensors' business (G1–G4, G8).

use std::sync::Arc;
use wb_core::UnitId;
use wb_sim::Sim;
use wb_sim::ai::AiBrains;
use wb_sim::components::Task;
use wb_sim::events::EventKind;
use wb_sim::orders::Resolved;

fn content() -> (Arc<wb_content::ContentDb>, [u8; 32]) {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../scenarios/fixtures");
    let set = wb_content::load_dir(std::path::Path::new(dir)).expect("content loads");
    let db = wb_content::compile(&set);
    let (header, _) = wb_content::encode(&db, &set.manifest.content_version);
    (Arc::new(db), header.content_hash)
}

/// Blue (a person) sends a fighter pair into Red's territory; Red (the AI) has a
/// radar, a flight that will fly CAP and a flight on alert at its air base.
fn scenario(difficulty: &str) -> String {
    format!(
        "
name: ai incursion
seed: 3
start_utc: 2026-10-27T12:00:00Z
test_territory:
  - {{ nation: opp, polygon: [[20.0, 55.0], [22.5, 55.0], [22.5, 56.5], [20.0, 56.5]] }}
  - {{ nation: tst, polygon: [[14.0, 54.5], [19.5, 54.5], [19.5, 56.5], [14.0, 56.5]] }}
sides:
  - {{ name: Blue, nations: [tst], player: true }}
  - {{ name: Red, nations: [opp], controller: ai, difficulty: {difficulty} }}
units:
  - {{ id: 10, name: Intruder, platform: test-fighter, nation: tst, count: 2, lat_deg: 55.6, lon_deg: 19.0, alt_m: 9000, heading_deg: 90, loadout: test-fighter-a2a }}
  - {{ id: 20, name: Red CAP, platform: test-fighter, nation: opp, count: 2, base: opp-airbase, loadout: test-fighter-a2a }}
  - {{ id: 25, name: Red alert, platform: test-fighter, nation: opp, count: 2, base: opp-airbase, loadout: test-fighter-a2a }}
  - {{ id: 30, name: Red radar, platform: test-ew-radar-unit, nation: opp, lat_deg: 55.62, lon_deg: 20.85 }}
orders:
  - {{ tick: 0, side: 0, type: AssignTask, units: [10], task: {{ kind: move, waypoints: [[21.8, 55.6]], alt_m: 9000 }} }}
"
    )
}

struct Run {
    /// Sim ms the AI's picture first showed the intruder inside its territory.
    seen_ms: u64,
    /// Sim ms of the AI's intercept order.
    order_ms: u64,
    expected_ms: u64,
    sim: Sim,
}

fn run(difficulty: &str) -> Run {
    let (db, hash) = content();
    let mut sim = Sim::from_yaml(&scenario(difficulty), db, hash, None).expect("scenario builds");
    let mut seen: Option<(u64, u64)> = None;
    let mut order_ms = None;
    for _ in 0..20_000 {
        sim.step();
        let now = sim.clock().sim_time_ms;
        if seen.is_none() {
            let brains = sim.world().resource::<AiBrains>();
            if let Some((id, t)) = brains.0.get(&1).and_then(|s| {
                s.threats
                    .iter()
                    .find(|(_, t)| t.kind == wb_ai::ThreatKind::Incursion)
                    .map(|(id, t)| (*id, t.since_ms))
            }) {
                let pic = wb_sim::ai::picture(sim.world_mut(), 1);
                seen = Some((t, wb_ai::reaction_delay_ms(&pic, id)));
            }
        }
        let intercept = sim
            .world()
            .resource::<wb_sim::orders::OrderLog>()
            .0
            .iter()
            .find(|o| {
                o.ai && o.side == 1
                    && matches!(
                        &o.cmd,
                        Resolved::Assign {
                            task: Task::Intercept { .. },
                            ..
                        }
                    )
            })
            .map(|o| o.submitted);
        if let (Some(_), Some(_)) = (seen, intercept) {
            order_ms = Some(now);
            break;
        }
    }
    let (seen_ms, expected_ms) = seen.expect("the AI saw the incursion");
    Run {
        seen_ms,
        order_ms: order_ms.expect("the AI ordered an intercept"),
        expected_ms,
        sim,
    }
}

#[test]
fn an_incursion_is_answered_within_the_reaction_time() {
    for (difficulty, extra_s) in [("hard", 10.0), ("normal", 30.0), ("easy", f64::NAN)] {
        let r = run(difficulty);
        // Doctrine 5 s + difficulty (Easy draws 60–120 s from the seed).
        if extra_s.is_finite() {
            assert_eq!(
                r.expected_ms,
                ((5.0 + extra_s) * 1_000.0) as u64,
                "{difficulty}"
            );
        } else {
            assert!(
                (65_000..=125_000).contains(&r.expected_ms),
                "easy: {}",
                r.expected_ms
            );
        }
        let took = r.order_ms - r.seen_ms;
        let (lo, hi) = (r.expected_ms * 8 / 10, r.expected_ms * 12 / 10);
        eprintln!(
            "{difficulty}: answered {took} ms after the AI saw it inside (reaction time {} ms)",
            r.expected_ms
        );
        assert!(
            (lo..=hi).contains(&took),
            "{difficulty}: response after {took} ms, outside {lo}–{hi} ms"
        );
        // The decision is logged to the AI's side, and says why.
        let decided = r.sim.events().iter().any(|e| {
            e.sides == 1 << 1
                && matches!(&e.kind, EventKind::AiDecision { summary, .. } if summary.starts_with("Incursion"))
        });
        assert!(
            decided,
            "{difficulty}: the incursion decision is in the log"
        );
    }
}

#[test]
fn the_ai_runs_only_for_ai_sides_and_its_decisions_stay_on_its_side() {
    let mut r = run("normal");
    r.sim.run_ticks(600);
    let events = r.sim.events();
    let ai: Vec<_> = events
        .iter()
        .filter(|e| matches!(e.kind, EventKind::AiDecision { .. }))
        .collect();
    assert!(!ai.is_empty());
    assert!(
        ai.iter().all(|e| e.sides == 1 << 1),
        "AI decisions go to the AI's side only, never to the human side"
    );
    // Every AI order is the AI side's own; none for the human side.
    let log = &r.sim.world().resource::<wb_sim::orders::OrderLog>().0;
    assert!(log.iter().filter(|o| o.ai).all(|o| o.side == 1));
    assert!(log.iter().any(|o| o.ai));
    // The responder really took off after the intruder.
    let alert = wb_sim::world::entity(r.sim.world(), UnitId(25)).expect("alert flight");
    let task = &r
        .sim
        .world()
        .get::<wb_sim::components::Group>(alert)
        .expect("group")
        .task;
    let cap = wb_sim::world::entity(r.sim.world(), UnitId(20)).expect("cap flight");
    let cap_task = &r
        .sim
        .world()
        .get::<wb_sim::components::Group>(cap)
        .expect("group")
        .task;
    assert!(
        matches!(task, Task::Intercept { .. }) || matches!(cap_task, Task::Intercept { .. }),
        "one of Red's flights intercepts: {task:?} / {cap_task:?}"
    );
}

#[test]
fn a_replay_with_the_ai_reaches_the_same_hashes() {
    let (db, hash) = content();
    let mut sim = Sim::from_yaml(&scenario("hard"), db, hash, None).expect("scenario builds");
    sim.run_ticks(3_000);
    let end = sim.clock().tick;
    let final_hash = sim.state_hash();
    let mut replay = wb_sim::replay::Replay::of(&sim).expect("replay");
    replay.seek(end).expect("seek to the end");
    assert_eq!(
        replay.sim().state_hash(),
        final_hash,
        "AI orders are not queued twice"
    );
    // A save mid-war, reloaded, continues to the same hash.
    let (db, hash) = content();
    let mut a = Sim::from_yaml(&scenario("hard"), db.clone(), hash, None).expect("builds");
    a.run_ticks(1_500);
    let bytes = a.save("ai", vec![], 0);
    let mut b = Sim::load(&bytes, db, hash, None).expect("loads");
    a.run_ticks(1_500);
    b.run_ticks(1_500);
    assert_eq!(a.state_hash(), b.state_hash());
}
