//! Loadouts at bases (P5 step 2, doc 03 §6): `SetLoadout` is refused for a
//! unit in the air with a reason, draws on the base's stock, and a rearm the
//! stock cannot cover reports `RearmShort`.

use std::sync::Arc;
use wb_core::UnitId;
use wb_sim::components::{BaseSite, Magazine, Posture};
use wb_sim::orders::{Command, LoadoutChoice, StationItem};
use wb_sim::world::entity;
use wb_sim::{EventKind, Order, Sim};

/// Husarz (two F-35A) at Łask, whose stock is overridden; Orzeł airborne.
const SCENARIO: &str = "
name: loadout lock
seed: 5
start_utc: 2026-10-27T06:00:00Z
sides:
  - { name: NATO, countries: [pol] }
  - { name: Russia, countries: [rus] }
base_stock:
  - { base: lask-air-base-eplk, munitions: { aim-120d: 4, gbu-31: 2 } }
units:
  - { id: 10, name: Husarz, platform: f-35a, country: pol, count: 2, base: lask-air-base-eplk, loadout: f-35a-air-superiority }
  - { id: 20, name: Orzel, platform: f-35a, country: pol, lat_deg: 53.9, lon_deg: 18.6, alt_m: 8000, heading_deg: 90, loadout: f-35a-air-superiority }
";

fn sim() -> (Sim, Arc<wb_content::ContentDb>) {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../content");
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

/// Łask's stock, by weapon id.
fn lask_stock(sim: &mut Sim, db: &wb_content::ContentDb) -> Vec<(String, u32)> {
    let w = sim.world_mut();
    let mut q = w.query::<&BaseSite>();
    let site = q
        .iter(w)
        .find(|b| {
            b.content
                .is_some_and(|ix| db.str(db.bases[ix.0 as usize].id) == "lask-air-base-eplk")
        })
        .expect("Łask is a base");
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
            .submit(&order(&sim, &units, template("f-35a-strike")))
            .expect_err("an airborne unit cannot change its loadout");
        assert_eq!(err, "Loadout locked: Orzel is airborne");
    }
    let custom = LoadoutChoice::Custom {
        stations: vec![StationItem {
            station: "bay_l_out".into(),
            weapon: "gbu-31".into(),
            count: 1,
        }],
    };
    let err = sim.submit(&order(&sim, &[20], custom)).unwrap_err();
    assert_eq!(err, "Loadout locked: Orzel is airborne");
    // Nothing was queued: the flight keeps its four AMRAAMs.
    sim.run_ticks(2);
    let e = entity(sim.world(), UnitId(21)).expect("Orzel");
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
            station: "bay_l_in".into(),
            weapon: "gbu-31".into(),
            count: 1,
        }],
    };
    let err = sim.submit(&order(&sim, &[11], bad_fit)).unwrap_err();
    assert!(err.contains("does not fit station bay_l_in"), "{err}");
    let err = sim
        .submit(&order(&sim, &[10], template("f-35a-strike")))
        .unwrap_err();
    assert!(
        err.starts_with("Not enough") && err.ends_with("4 needed, 2 in stock"),
        "{err}"
    );

    // Each order alone fits the stock; the second finds it drawn at apply.
    sim.submit(&order(&sim, &[11], template("f-35a-strike")))
        .expect("first strike loadout");
    sim.submit(&order(&sim, &[12], template("f-35a-strike")))
        .expect("second strike loadout");
    sim.run_ticks(1);
    // Two AMRAAMs from each came back; both JDAMs went to Husarz 1.
    assert_eq!(lask_stock(&mut sim, &db), vec![("aim-120d".to_owned(), 8)]);
    let e11 = entity(sim.world(), UnitId(11)).expect("Husarz 1");
    let e12 = entity(sim.world(), UnitId(12)).expect("Husarz 2");
    assert_eq!(sim.world().get::<Magazine>(e11).unwrap().remaining(), 4);
    assert_eq!(sim.world().get::<Magazine>(e12).unwrap().remaining(), 2);
    assert!(matches!(
        sim.world().get::<Posture>(e11),
        Some(Posture::AtBase { ready: false, .. })
    ));
    let changed = sim
        .events()
        .iter()
        .filter(|e| matches!(&e.kind, EventKind::LoadoutChanged { loadout: Some(l), .. } if l == "f-35a-strike"))
        .count();
    assert_eq!(changed, 2);

    // The 60 min turnaround ends; the rearm finds no JDAMs for Husarz 2.
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
    assert_eq!(short, vec![(UnitId(12), "gbu-31".to_owned(), 2)]);
    assert!(matches!(
        sim.world().get::<Posture>(e12),
        Some(Posture::AtBase { ready: true, .. })
    ));
}
