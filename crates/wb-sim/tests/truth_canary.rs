//! Truth canary (doc 08 §2 fog-of-war metric, P4 acceptance, CLAUDE.md rule 1):
//! enemy units with unique names and coordinates must never appear in any
//! view, details response or event payload of the other side unless
//! detected, and then only as contact estimates: never by name, unit id or
//! exact position (full precision, or to about 0.1 m).
//!
//! The test calls exactly what a client can call (wb-wasm wraps the same
//! functions): `ViewEncoder::encode`, `details`, `contact_details`,
//! `ground_track` and the side-filtered event log.

use std::sync::Arc;
use wb_core::UnitId;
use wb_core::geo::LatLon;
use wb_sim::components::Kinematics;
use wb_sim::picture::SidePictures;
use wb_sim::view::ViewEncoder;
use wb_sim::world::{UnitIdAlloc, entity};
use wb_sim::{Sim, details, satellites};

/// The canaries: one that sails into radar range, one that stays hidden
/// far away, one inside a group, all Russian. Our side is 0.
const SCENARIO: &str = "
name: truth canary
seed: 77
start_utc: 2026-10-27T20:00:00Z
start_mode: deploy
sides:
  - { name: NATO, countries: [ita, usa] }
  - { name: Russia, countries: [rus] }
units:
  - { id: 10, name: Bergamini, platform: fremm-bergamini, country: ita, lat_deg: 55.2, lon_deg: 17.0, heading_deg: 90 }
  - { id: 12, name: Growler, platform: ea-18g, country: usa, lat_deg: 55.0, lon_deg: 15.0, alt_m: 8000, heading_deg: 90 }
  - { id: 14, name: CSG-1, platform: csg-1, country: ita }
  - { id: 500, name: CANARY-7f3a9c-SHIP, platform: steregushchiy, country: rus, lat_deg: 55.234567, lon_deg: 18.123457, heading_deg: 270 }
  - { id: 600, name: CANARY-7f3a9c-HIDDEN, platform: steregushchiy, country: rus, lat_deg: 40.765432, lon_deg: 3.987654 }
  - { id: 700, name: CANARY-7f3a9c-FLIGHT, platform: su-35s, country: rus, count: 2, lat_deg: 55.876543, lon_deg: 19.345679, alt_m: 7000, heading_deg: 250 }
orders:
  - { tick: 0, side: 0, type: AssignTask, units: [10], task: { kind: move, waypoints: [[18.5, 55.2]], speed: max } }
  - { tick: 0, side: 0, type: AssignTask, units: [12], task: { kind: patrol_circle, centre: [16.0, 55.0], radius_km: 30 } }
  - { tick: 0, side: 1, type: AssignTask, units: [500], task: { kind: move, waypoints: [[17.0, 55.234567]] } }
  - { tick: 0, side: 1, type: AssignTask, units: [700], task: { kind: patrol_circle, centre: [18.5, 55.6], radius_km: 40 } }
  - { tick: 0, side: 0, type: TaskSatellite, satellite: 14, centre: [9.0, 41.0], radius_km: 30 }
";

const NAME: &str = "CANARY-7f3a9c";
const CANARIES: [u32; 5] = [500, 501, 600, 601, 700];

fn content() -> (Arc<wb_content::ContentDb>, [u8; 32]) {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../content");
    let set = wb_content::load_dir(std::path::Path::new(dir)).expect("content loads");
    let db = wb_content::compile(&set);
    let (header, _) = wb_content::encode(&db, &set.manifest.content_version);
    (Arc::new(db), header.content_hash)
}

/// Every truth position of the canaries now, with the ways it could leak.
fn exact_positions(sim: &Sim) -> Vec<(LatLon, Vec<Vec<u8>>, Vec<String>)> {
    let w = sim.world();
    let mut ids: Vec<u32> = CANARIES.to_vec();
    ids.extend([701, 702]);
    ids.iter()
        .filter_map(|id| entity(w, UnitId(*id)))
        .filter_map(|e| w.get::<Kinematics>(e))
        .map(|k| {
            let (lat, lon) = (k.pos().lat_deg(), k.pos().lon_deg());
            // "Exact" is the full f64, or six decimals (about 0.1 m): an
            // estimate may by chance fall within metres of the truth, but
            // never on it (ADR-026).
            let bytes = vec![lat.to_le_bytes().to_vec(), lon.to_le_bytes().to_vec()];
            let strings = (6..=8)
                .flat_map(|d| [format!("{lat:.d$}"), format!("{lon:.d$}")])
                .collect();
            (k.pos(), bytes, strings)
        })
        .collect()
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// Contact ids in side 0's picture whose truth is a canary (test only).
fn canary_contacts(sim: &Sim) -> usize {
    let w = sim.world();
    let pics = w.resource::<SidePictures>();
    pics.sides[0]
        .contacts
        .values()
        .filter(|c| [500, 600, 700].contains(&c.truth.0))
        .count()
}

#[test]
fn hidden_enemy_never_leaks() {
    let (db, hash) = content();
    let mut sim = Sim::from_yaml(SCENARIO, db, hash, None).expect("scenario builds");
    let mut enc = ViewEncoder::new();
    let mut seen_events = 0;
    let mut detected_ship = false;
    let mut scans = 0;
    for _ in 0..80 {
        sim.run_ticks(75);
        scans += 1;
        let positions = exact_positions(&sim);
        let mut texts: Vec<String> = Vec::new();

        // The side view (binary): no name, no unit row, no exact coordinate.
        let view = enc.encode(sim.world(), 0).to_vec();
        assert!(!contains(&view, NAME.as_bytes()), "name in view");
        for (_, bytes, _) in &positions {
            for b in bytes {
                assert!(!contains(&view, b), "exact canary coordinate in the view");
            }
        }

        // details() for every id that exists or could exist.
        let next = sim.world().resource::<UnitIdAlloc>().next;
        let mut ids: Vec<u32> = (0..2_000).collect();
        ids.extend(1_000_000..next + 5);
        for id in ids {
            match details::details(sim.world(), 0, id) {
                Ok(d) => {
                    assert!(
                        !CANARIES.contains(&id) && ![701, 702].contains(&id),
                        "details of canary {id}"
                    );
                    texts.push(format!("{d:?}"));
                }
                Err(e) => assert_eq!(e, format!("Unknown unit {id}"), "rejection reveals nothing"),
            }
            if let Ok(t) = satellites::ground_track(sim.world(), 0, id, 10.0, 10.0, 60.0) {
                texts.push(format!("{t:?}"));
            }
        }
        // contact_details() for every contact id and beyond.
        for id in 0..64 {
            match details::contact_details(sim.world(), 0, id) {
                Ok(c) => texts.push(serde_json::to_string(&c).expect("json")),
                Err(e) => assert_eq!(e, format!("Unknown contact {id}")),
            }
        }
        // Events side 0 may see.
        let log = sim.world().resource::<wb_sim::events::EventLog>();
        for e in log.visible(0, seen_events) {
            texts.push(serde_json::to_string(e).expect("json"));
        }
        seen_events = log.0.len();

        for t in &texts {
            assert!(!t.contains(NAME), "name leaked: {t}");
            for (_, _, strings) in &positions {
                for s in strings {
                    assert!(!t.contains(s.as_str()), "exact coordinate {s} leaked: {t}");
                }
            }
        }

        // The view lists no canary as a unit, and contact ids never equal
        // the canary unit ids by construction (a separate namespace).
        let contacts = canary_contacts(&sim);
        if contacts > 0 {
            detected_ship = true;
        }
        let pics = sim.world().resource::<SidePictures>();
        for c in pics.sides[0].contacts.values() {
            if let Some(e) = c.est {
                for (p, _, _) in &positions {
                    assert!(
                        e.at != *p,
                        "a contact estimate equals the truth position (ADR-026)"
                    );
                }
            }
        }
        // The far-away canary is never detected: no contact of it exists.
        assert!(
            !pics.sides[0].by_truth.contains_key(&UnitId(600)),
            "the hidden canary was detected"
        );
    }
    assert!(scans > 0);
    assert!(
        detected_ship,
        "the approaching canary should be detected during the run"
    );
}
