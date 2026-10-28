//! Base placement (doc 03 §3, P3 acceptance) through the order path, as the
//! UI sends it. Real airfields and ports come from content, so those rules run
//! always; terrain rules need the region packs (`--ignored`, like wb-geo's
//! pack tests).

use std::rc::Rc;
use std::sync::Arc;
use wb_content::source::realism::BaseKind;
use wb_sim::orders::Command;
use wb_sim::{EventKind, Order, Sim};

const SCENARIO: &str = "
name: placement
seed: 1
start_utc: 2026-10-27T06:00:00Z
start_mode: deploy
sides:
  - { name: NATO, countries: [pol, deu, fra, ita, usa], player: true }
  - { name: Russia, countries: [rus] }
";

fn content() -> (Arc<wb_content::ContentDb>, [u8; 32]) {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../content");
    let set = wb_content::load_dir(std::path::Path::new(dir)).expect("content loads");
    let db = wb_content::compile(&set);
    let (header, _) = wb_content::encode(&db, &set.manifest.content_version);
    (Arc::new(db), header.content_hash)
}

fn sim(geo: Option<Rc<wb_geo::Geo>>) -> Sim {
    let (db, hash) = content();
    Sim::from_yaml(SCENARIO, db, hash, geo).expect("scenario builds")
}

fn place(sim: &mut Sim, kind: BaseKind, lat: f64, lon: f64) -> Result<(), String> {
    let tick = sim.clock().tick;
    sim.submit(&Order {
        tick,
        side: 0,
        command: Command::PlaceBase {
            kind,
            lat_deg: lat,
            lon_deg: lon,
            name: None,
        },
    })
    .map(|_| ())
}

#[test]
fn naval_base_inland_is_rejected() {
    let mut s = sim(None);
    // Warsaw: 250 km from the sea.
    let err = place(&mut s, BaseKind::NavalBase, 52.23, 21.01).unwrap_err();
    assert_eq!(err, "Naval base must be at a port (none within 5 km)");
}

#[test]
fn air_base_without_a_real_airfield_is_rejected() {
    let mut s = sim(None);
    // Open country west of Bydgoszcz, no airfield within 3 km.
    let err = place(&mut s, BaseKind::AirBase, 53.10, 17.40).unwrap_err();
    assert_eq!(
        err,
        "Air base must be at a real airfield (none within 3 km)"
    );
}

#[test]
fn real_sites_of_own_side_are_accepted_and_activate() {
    let mut s = sim(None);
    // Łask Air Base and the port of Gdynia, a little off their coordinates.
    place(&mut s, BaseKind::AirBase, 51.56, 19.18).expect("Łask");
    place(&mut s, BaseKind::NavalBase, 54.52, 18.55).expect("Gdynia");
    assert_eq!(
        place(&mut s, BaseKind::AirBase, 51.55, 19.18).unwrap_err(),
        "Łask Air Base is already one of your bases"
    );
    // Activation takes an hour (realism `bases.*.setup_min`).
    s.run_ticks(3_600);
    let active = s
        .events()
        .iter()
        .filter(|e| matches!(e.kind, EventKind::BaseActivated { .. }))
        .count();
    assert_eq!(active, 2);
}

#[test]
fn enemy_airfield_is_rejected() {
    let mut s = sim(None);
    // Chkalovsk, Kaliningrad.
    let err = place(&mut s, BaseKind::AirBase, 54.766, 20.397).unwrap_err();
    assert!(
        err.ends_with("belongs to another country, not your side"),
        "{err}"
    );
}

#[test]
fn terrain_sites_need_the_region_pack() {
    let mut s = sim(None);
    let err = place(&mut s, BaseKind::RoadStrip, 52.0, 19.0).unwrap_err();
    assert_eq!(
        err,
        "Placing a road strip needs the region pack for this area"
    );
}

/// Needs `build/packs/{world,europe}` (`bash geodata/run.sh build …`).
#[test]
#[ignore = "needs the region packs in build/packs"]
fn terrain_rules_on_the_real_packs() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../build/packs");
    let mut geo = wb_geo::Geo::new();
    for p in ["world", "europe"] {
        geo.add_pack(wb_geo::Pack::open_dir(&std::path::Path::new(root).join(p)).unwrap())
            .unwrap();
    }
    let mut s = sim(Some(Rc::new(geo)));
    // Still inland for a naval base, with packs or without.
    assert_eq!(
        place(&mut s, BaseKind::NavalBase, 52.23, 21.01).unwrap_err(),
        "Naval base must be at a port (none within 5 km)"
    );
    // An anchorage on land, a road strip at sea, an army base abroad.
    assert_eq!(
        place(&mut s, BaseKind::Anchorage, 52.23, 21.01).unwrap_err(),
        "Expeditionary anchorage cannot stand on urban"
    );
    assert_eq!(
        place(&mut s, BaseKind::RoadStrip, 55.3, 18.0).unwrap_err(),
        "Road strip cannot stand on water"
    );
    assert_eq!(
        place(&mut s, BaseKind::ArmyBase, 54.71, 20.51).unwrap_err(),
        "Outside own territory"
    );
    // An anchorage in Polish waters off Hel is fine.
    place(&mut s, BaseKind::Anchorage, 54.55, 18.95).expect("anchorage in the Gulf of Gdańsk");
}

/// Ships stay on water (doc 02 §5): a route over land is rejected with the
/// place it crosses; one through the Gulf of Gdańsk is accepted.
#[test]
#[ignore = "needs the region packs in build/packs"]
fn ship_routes_over_land_are_rejected() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../build/packs");
    let mut geo = wb_geo::Geo::new();
    for p in ["world", "europe"] {
        geo.add_pack(wb_geo::Pack::open_dir(&std::path::Path::new(root).join(p)).unwrap())
            .unwrap();
    }
    let yaml = "
name: ships
seed: 1
start_utc: 2026-10-27T06:00:00Z
sides:
  - { name: NATO, countries: [pol, deu, fra, ita, usa], player: true }
  - { name: Russia, countries: [rus] }
units:
  - { id: 50, name: Bergamini, platform: fremm-bergamini, country: ita, base: gdynia }
";
    let (db, hash) = content();
    let mut s = Sim::from_yaml(yaml, db, hash, Some(Rc::new(geo))).expect("scenario builds");
    let order = |to: [f64; 2]| Order {
        tick: 0,
        side: 0,
        command: Command::AssignTask {
            units: vec![50],
            task: wb_sim::orders::TaskOrder::Move {
                waypoints: vec![to],
                alt_m: None,
                speed: None,
            },
        },
    };
    let err = s.submit(&order([21.01, 52.23])).unwrap_err();
    assert!(err.starts_with("Leg 1 crosses land near"), "{err}");
    s.submit(&order([18.9, 54.6])).expect("route over water");
}
