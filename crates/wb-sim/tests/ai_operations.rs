//! Enemy AI v2 (doc 03 §8, §11, ADR-049, P12 acceptance): the AI plans its own
//! multi-phase operation, with air-defence suppression before the strike, from
//! its own picture. Generic fixtures (ADR-035).
//!
//! Blue (a person, Charlie ROE and no self-defence, so its SAM site holds fire)
//! keeps an armour column beside a SAM site. Red (the AI, Hard) learns of them
//! only from its own drone's picture, then plans: decoy drones, stand-off
//! jamming, suppression of the SAM, the strike on the column, an escort.

use std::sync::Arc;
use wb_sim::Sim;
use wb_sim::ai::AiBrains;
use wb_sim::events::EventKind;
use wb_sim::operations::{OpState, Operations, PhaseState};

fn content() -> (Arc<wb_content::ContentDb>, [u8; 32]) {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../scenarios/fixtures");
    let set = wb_content::load_dir(std::path::Path::new(dir)).expect("content loads");
    let db = wb_content::compile(&set);
    let (header, _) = wb_content::encode(&db, &set.manifest.content_version);
    (Arc::new(db), header.content_hash)
}

fn scenario(difficulty: &str) -> String {
    format!(
        "
name: ai operation
seed: 12
start_utc: 2026-10-27T12:00:00Z
test_territory:
  - {{ nation: opp, polygon: [[20.0, 55.0], [22.5, 55.0], [22.5, 56.5], [20.0, 56.5]] }}
  - {{ nation: tst, polygon: [[14.0, 54.5], [19.5, 54.5], [19.5, 56.5], [14.0, 56.5]] }}
sides:
  - {{ name: Blue, nations: [tst], player: true, roe: charlie, self_defence: false }}
  - {{ name: Red, nations: [opp], controller: ai, difficulty: {difficulty}, roe: alpha }}
units:
  - {{ id: 400, name: Blue SAM, system: test-sam-site, nation: tst, lat_deg: 55.25, lon_deg: 17.2 }}
  - {{ id: 410, name: Blue column, system: test-armour-column, nation: tst, lat_deg: 55.28, lon_deg: 17.6, heading_deg: 80 }}
  - {{ id: 20, name: Red eyes, platform: test-drone, nation: opp, base: opp-airbase }}
  - {{ id: 25, name: Red decoy, platform: test-drone, nation: opp, base: opp-airbase }}
  - {{ id: 30, name: Red strikers, platform: test-fighter, nation: opp, count: 2, base: opp-airbase, loadout: test-fighter-bombs }}
  - {{ id: 40, name: Red suppression, platform: test-fighter, nation: opp, count: 2, base: opp-airbase, loadout: test-fighter-sead-link }}
  - {{ id: 50, name: Red jammer, platform: test-jammer-aircraft, nation: opp, base: opp-airbase }}
  - {{ id: 60, name: Red fighters A, platform: test-fighter, nation: opp, count: 2, base: opp-airbase, loadout: test-fighter-a2a }}
  - {{ id: 70, name: Red fighters B, platform: test-fighter, nation: opp, count: 2, base: opp-airbase, loadout: test-fighter-a2a }}
base_stock:
  - {{ base: opp-airbase, munitions: {{ test-glide-bomb: 8, test-arm-link: 8, test-aam: 8 }} }}
orders:
  - {{ tick: 0, side: 1, type: AssignTask, units: [20], task: {{ kind: patrol_circle, centre: [17.4, 55.27], radius_km: 8, alt_m: 5000 }} }}
"
    )
}

fn run(difficulty: &str, minutes: u64) -> Sim {
    let (db, hash) = content();
    let mut sim = Sim::from_yaml(&scenario(difficulty), db, hash, None).expect("scenario builds");
    sim.set_recording(true);
    let until = minutes * 60_000;
    while sim.clock().sim_time_ms < until {
        sim.step();
    }
    sim
}

/// Red's operations in id order.
fn red_ops(sim: &Sim) -> Vec<wb_sim::operations::Operation> {
    sim.world()
        .resource::<Operations>()
        .ops
        .iter()
        .filter(|o| o.side == 1)
        .cloned()
        .collect()
}

#[test]
fn the_ai_plans_suppression_before_the_strike() {
    let sim = run("hard", 150);
    let ops = red_ops(&sim);
    let ai: Vec<String> = sim
        .events()
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::AiDecision {
                layer,
                summary,
                refused,
            } if e.sides == 1 << 1 => Some(format!("[{layer}] {summary} {refused:?}")),
            _ => None,
        })
        .collect();
    assert!(
        !ops.is_empty(),
        "the AI planned an operation; its decisions:\n{}",
        ai.join("\n")
    );
    let op = &ops[0];
    eprintln!("{}: {:?}", op.name, op.state);
    for p in &op.phases {
        eprintln!(
            "  {} [{}] {:?} started {:?}: {:?}",
            p.key,
            p.name,
            p.state,
            p.started.map(|s| s.ms),
            p.reason
        );
    }
    let key = |k: &str| op.phases.iter().position(|p| p.key == k);
    let sead = key("sead").expect("a suppression phase");
    let strike = key("strike").expect("a strike phase");
    assert!(
        op.phases.len() >= 3,
        "multi-phase: {} phases",
        op.phases.len()
    );
    let sead_started = op.phases[sead].started.expect("suppression started").ms;
    let strike_started = op.phases[strike].started.expect("the strike started").ms;
    assert!(
        sead_started <= strike_started,
        "suppression starts ({sead_started}) before the strike ({strike_started})"
    );
    assert!(
        matches!(
            op.phases[strike].state,
            PhaseState::Running | PhaseState::Done
        ),
        "the strike ran"
    );
    // The missiles for the SAM flew before the bombs for the column.
    let first = |w: &str| {
        sim.events().iter().find(|e| {
            e.sides & (1 << 1) != 0
                && matches!(&e.kind, EventKind::WeaponLaunched { weapon_type, .. } if weapon_type == w)
        })
        .map(|e| e.tick)
    };
    let arm = first("test-arm-link").expect("suppression missiles launched");
    let bomb = first("test-glide-bomb").expect("bombs launched");
    assert!(arm <= bomb, "ARM at tick {arm}, bomb at tick {bomb}");
    // Decisions explain themselves and stay on the AI's side.
    assert!(
        ai.iter().any(|d| d.contains("objective")),
        "{}",
        ai.join("\n")
    );
    assert!(matches!(op.state, OpState::Running | OpState::Completed));
    // The AI does not plan against the same objective twice in a row: the
    // second operation, if any, has another objective.
    if let Some(second) = ops.get(1) {
        assert_ne!(second.name.split(": ").nth(1), op.name.split(": ").nth(1));
    }
    let brains = sim.world().resource::<AiBrains>();
    assert!(brains.0.get(&1).is_some_and(|b| b.op_serial >= 1));
    assert!(!brains.0.contains_key(&0), "no AI for the human side");
}

#[test]
fn easy_flies_suppression_and_strike_only() {
    let sim = run("easy", 200);
    let ops = red_ops(&sim);
    let op = ops.first().expect("Easy plans an operation too");
    let keys: Vec<&str> = op.phases.iter().map(|p| p.key.as_str()).collect();
    assert_eq!(keys, ["sead", "strike"], "Easy keeps the plan shallow");
}

#[test]
fn a_replay_and_a_save_with_ai_operations_reach_the_same_hashes() {
    let (db, hash) = content();
    let mut a = Sim::from_yaml(&scenario("hard"), db.clone(), hash, None).expect("builds");
    a.run_ticks(30_000);
    let bytes = a.save("ai ops", vec![], 0);
    let mut b = Sim::load(&bytes, db, hash, None).expect("loads");
    a.run_ticks(10_000);
    b.run_ticks(10_000);
    assert_eq!(
        a.state_hash(),
        b.state_hash(),
        "save and reload mid-operation"
    );
    let end = a.clock().tick;
    let mut replay = wb_sim::replay::Replay::of(&a).expect("replay");
    replay.seek(end).expect("seek to the end");
    assert_eq!(replay.sim().state_hash(), a.state_hash(), "replay");
}
