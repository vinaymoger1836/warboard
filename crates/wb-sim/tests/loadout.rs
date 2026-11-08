//! Loadouts at bases (P5 step 2, doc 03 §6): `SetLoadout` is refused for a
//! unit in the air with a reason, draws on the base's stock, and a rearm the
//! stock cannot cover reports `RearmShort`.

use std::sync::Arc;
use wb_core::UnitId;
use wb_sim::components::{BaseSite, Magazine, Posture};
use wb_sim::orders::{Command, LoadoutChoice, StationItem};
use wb_sim::world::entity;
use wb_sim::{EventKind, Order, Sim};

/// Pair (two test fighters) at the test air base, whose stock is overridden;
/// Airborne in flight. Runs on the generic fixtures (ADR-035).
const SCENARIO: &str = "
name: loadout lock
seed: 5
start_utc: 2026-10-27T06:00:00Z
sides:
  - { name: Blue, nations: [tst] }
  - { name: Red, nations: [opp] }
base_stock:
  - { base: test-airbase, munitions: { test-aam: 4, test-bomb: 2 } }
units:
  - { id: 10, name: Pair, platform: test-fighter, nation: tst, count: 2, base: test-airbase, loadout: test-fighter-a2a }
  - { id: 20, name: Airborne, platform: test-fighter, nation: tst, lat_deg: 55.3, lon_deg: 15.5, alt_m: 8000, heading_deg: 90, loadout: test-fighter-a2a }
";

fn sim() -> (Sim, Arc<wb_content::ContentDb>) {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../scenarios/fixtures");
    let set = wb_content::load_dir(std::path::Path::new(dir)).expect("content loads");
    let db = Arc::new(wb_content::compile(&set));
    let (header, _) = wb_content::encode(&db, &set.manifest.content_version);
    let sim = Sim::from_yaml(SCENARIO, db.clone(), header.content_hash, None).expect("builds");
    (sim, db)
}

fn order(sim: &Sim, units: &[u32], loadout: LoadoutChoice) -> Order {
    Order {
        tick: sim.clock().tick,
        side: 0,
        command: Command::SetLoadout {
            units: units.to_vec(),
            loadout,
        },
    }
}

fn template(id: &str) -> LoadoutChoice {
    LoadoutChoice::Template { id: id.to_owned() }
}

/// The test air base's stock, by weapon id.
fn base_stock(sim: &mut Sim, db: &wb_content::ContentDb) -> Vec<(String, u32)> {
    let w = sim.world_mut();
    let mut q = w.query::<&BaseSite>();
    let site = q
        .iter(w)
        .find(|b| {
            b.content
                .is_some_and(|ix| db.str(db.bases[ix.0 as usize].id) == "test-airbase")
        })
        .expect("the test air base is a base");
    site.stock
        .iter()
        .map(|(w, n)| (db.str(db.weapons[w.0 as usize].id).to_owned(), *n))
        .collect()
}

#[test]
fn airborne_loadout_is_rejected_with_a_reason() {
    let (mut sim, _) = sim();
    for units in [[20], [21]] {
        let err = sim
            .submit(&order(&sim, &units, template("test-fighter-strike")))
            .expect_err("an airborne unit cannot change its loadout");
        assert_eq!(err, "Loadout locked: Airborne is airborne");
    }
    let custom = LoadoutChoice::Custom {
        stations: vec![StationItem {
            station: "wing".into(),
            weapon: "test-bomb".into(),
            count: 1,
        }],
    };
    let err = sim.submit(&order(&sim, &[20], custom)).unwrap_err();
    assert_eq!(err, "Loadout locked: Airborne is airborne");
    // Nothing was queued: the flight keeps its four missiles.
    sim.run_ticks(2);
    let e = entity(sim.world(), UnitId(21)).expect("Airborne");
    assert_eq!(
        sim.world()
            .get::<Magazine>(e)
            .expect("magazine")
            .remaining(),
        4
    );
}

#[test]
fn loadout_at_base_draws_stock_and_rearm_reports_shortfall() {
    let (mut sim, db) = sim();
    // Wrong station, and a stock too small for the whole flight.
    let bad_fit = LoadoutChoice::Custom {
        stations: vec![StationItem {
            station: "bay".into(),
            weapon: "test-bomb".into(),
            count: 1,
        }],
    };
    let err = sim.submit(&order(&sim, &[11], bad_fit)).unwrap_err();
    assert!(err.contains("does not fit station bay"), "{err}");
    let err = sim
        .submit(&order(&sim, &[10], template("test-fighter-strike")))
        .unwrap_err();
    assert!(
        err.starts_with("Not enough") && err.ends_with("4 needed, 2 in stock"),
        "{err}"
    );

    // Each order alone fits the stock; the second finds it drawn at apply.
    sim.submit(&order(&sim, &[11], template("test-fighter-strike")))
        .expect("first strike loadout");
    sim.submit(&order(&sim, &[12], template("test-fighter-strike")))
        .expect("second strike loadout");
    sim.run_ticks(1);
    // Two missiles from each came back; both bombs went to Pair 1.
    assert_eq!(base_stock(&mut sim, &db), vec![("test-aam".to_owned(), 8)]);
    let e11 = entity(sim.world(), UnitId(11)).expect("Pair 1");
    let e12 = entity(sim.world(), UnitId(12)).expect("Pair 2");
    assert_eq!(sim.world().get::<Magazine>(e11).unwrap().remaining(), 4);
    assert_eq!(sim.world().get::<Magazine>(e12).unwrap().remaining(), 2);
    assert!(matches!(
        sim.world().get::<Posture>(e11),
        Some(Posture::AtBase { ready: false, .. })
    ));
    let changed = sim
        .events()
        .iter()
        .filter(|e| matches!(&e.kind, EventKind::LoadoutChanged { loadout: Some(l), .. } if l == "test-fighter-strike"))
        .count();
    assert_eq!(changed, 2);

    // The 60 min turnaround ends; the rearm finds no bombs for Pair 2.
    sim.run_ticks(3_700);
    let short: Vec<_> = sim
        .events()
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::RearmShort {
                unit,
                weapon,
                short,
                ..
            } => Some((*unit, weapon.clone(), *short)),
            _ => None,
        })
        .collect();
    assert_eq!(short, vec![(UnitId(12), "test-bomb".to_owned(), 2)]);
    assert!(matches!(
        sim.world().get::<Posture>(e12),
        Some(Posture::AtBase { ready: true, .. })
    ));
}
