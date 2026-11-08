//! Base placement (doc 03 §3, P3 acceptance) through the order path, as the
//! UI sends it. Preset bases come from content, so those rules run always
//! (on the generic fixtures, ADR-035); terrain rules need the region packs
//! (`--ignored`, like wb-geo's pack tests).

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
  - { name: Blue, nations: [tst], player: true }
  - { name: Red, nations: [opp] }
";

fn content(dir: &str) -> (Arc<wb_content::ContentDb>, [u8; 32]) {
    let dir = format!("{}/../../{dir}", env!("CARGO_MANIFEST_DIR"));
    let set = wb_content::load_dir(std::path::Path::new(&dir)).expect("content loads");
    let db = wb_content::compile(&set);
    let (header, _) = wb_content::encode(&db, &set.manifest.content_version);
    (Arc::new(db), header.content_hash)
}

fn sim() -> Sim {
    let (db, hash) = content("scenarios/fixtures");
    Sim::from_yaml(SCENARIO, db, hash, None).expect("scenario builds")
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
fn naval_base_away_from_a_preset_port_is_rejected() {
    let mut s = sim();
    // 100 km from the test port.
    let err = place(&mut s, BaseKind::NavalBase, 55.9, 17.0).unwrap_err();
    assert_eq!(
        err,
        "Naval base must be at a preset naval base (none within 5 km)"
    );
}

#[test]
fn air_base_away_from_a_preset_air_base_is_rejected() {
    let mut s = sim();
    // 20 km from the test air base.
    let err = place(&mut s, BaseKind::AirBase, 55.33, 14.95).unwrap_err();
    assert_eq!(
        err,
        "Air base must be at a preset air base (none within 3 km)"
    );
}

#[test]
fn preset_sites_of_own_side_are_accepted_and_activate() {
    let mut s = sim();
    // The test air base and the test port, a little off their coordinates.
    place(&mut s, BaseKind::AirBase, 55.16, 14.95).expect("test air base");
    place(&mut s, BaseKind::NavalBase, 55.01, 17.0).expect("test port");
    assert_eq!(
        place(&mut s, BaseKind::AirBase, 55.15, 14.96).unwrap_err(),
        "Test Air Base is already one of your bases"
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
fn enemy_air_base_is_rejected() {
    let mut s = sim();
    let err = place(&mut s, BaseKind::AirBase, 55.6, 20.9).unwrap_err();
    assert!(
        err.ends_with("belongs to another nation, not your side"),
        "{err}"
    );
}

#[test]
fn terrain_sites_need_the_region_pack() {
    let mut s = sim();
    let err = place(&mut s, BaseKind::RoadStrip, 52.0, 19.0).unwrap_err();
    assert_eq!(
        err,
        "Placing a road strip needs the region pack for this area"
    );
}

/// The `world` and `europe` packs from `build/packs`.
fn packs() -> Rc<wb_geo::Geo> {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../build/packs");
    let mut geo = wb_geo::Geo::new();
    for p in ["world", "europe"] {
        geo.add_pack(wb_geo::Pack::open_dir(&std::path::Path::new(root).join(p)).unwrap())
            .unwrap();
    }
    Rc::new(geo)
}

/// Theatre North on the starter catalogue: every nation but Vessmar on our side.
const NORTH: &str = "
name: placement north
seed: 1
start_utc: 2026-10-27T06:00:00Z
start_mode: deploy
sides:
  - { name: West, nations: [ald, tlv, hlv, kld, ost, mrv, cnd], player: true }
  - { name: East, nations: [vsm] }
";

/// Needs `build/packs/{world,europe}` (`bash geodata/run.sh build …`). The
/// territory checks (own waters, abroad) return with the generated borders
/// in PF.
#[test]
#[ignore = "needs the region packs in build/packs"]
fn terrain_rules_on_the_real_packs() {
    let (db, hash) = content("content");
    let mut s = Sim::from_yaml(NORTH, db, hash, Some(packs())).expect("scenario builds");
    // Inland and far from any preset port, with packs or without.
    assert_eq!(
        place(&mut s, BaseKind::NavalBase, 52.23, 21.01).unwrap_err(),
        "Naval base must be at a preset naval base (none within 5 km)"
    );
    // An anchorage on land (a large urban area) and a road strip at sea.
    assert_eq!(
        place(&mut s, BaseKind::Anchorage, 52.23, 21.01).unwrap_err(),
        "Expeditionary anchorage cannot stand on urban"
    );
    assert_eq!(
        place(&mut s, BaseKind::RoadStrip, 55.3, 18.0).unwrap_err(),
        "Road strip cannot stand on water"
    );
}

/// Ships stay on water (doc 02 §5): a route over land is rejected with the
/// place it crosses; one over open water is accepted.
#[test]
#[ignore = "needs the region packs in build/packs"]
fn ship_routes_over_land_are_rejected() {
    let yaml = format!(
        "{NORTH}units:\n  - {{ id: 50, name: Frigate, platform: halvard, nation: ald, lat_deg: 54.9, lon_deg: 18.9, heading_deg: 0 }}\n"
    );
    let (db, hash) = content("content");
    let mut s = Sim::from_yaml(&yaml, db, hash, Some(packs())).expect("scenario builds");
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
    s.submit(&order([18.0, 55.4])).expect("route over water");
}
