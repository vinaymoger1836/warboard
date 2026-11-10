//! Ground forces (P11): what the goldens G17 (strike chain) and G18 (terrain
//! loss ratios) do not pin down. Routes over terrain and refused orders,
//! supply from bases and a placed FOB, raids and designation, a save in the
//! middle of a ground battle, and that ground events only tell a side about
//! its own formations (CLAUDE.md rule 1).

use std::sync::Arc;
use wb_core::geo::{LatLon, distance_m};
use wb_sim::components::{Formation, Group, Kinematics};
use wb_sim::events::EventKind;
use wb_sim::orders::{Command, TaskOrder};
use wb_sim::picture::SidePictures;
use wb_sim::world::entity;
use wb_sim::{Order, Sim};

fn fixtures() -> (Arc<wb_content::ContentDb>, [u8; 32]) {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../scenarios/fixtures");
    let set = wb_content::load_dir(std::path::Path::new(dir)).expect("fixtures load");
    let db = wb_content::compile(&set);
    let (header, _) = wb_content::encode(&db, &set.manifest.content_version);
    (Arc::new(db), header.content_hash)
}

fn sim(yaml: &str) -> Sim {
    let (db, hash) = fixtures();
    Sim::from_yaml(yaml, db, hash, None).expect("builds")
}

fn task(tick: u64, side: u8, units: &[u32], task: TaskOrder) -> Order {
    Order {
        tick,
        side,
        command: Command::AssignTask {
            units: units.to_vec(),
            task,
        },
    }
}

/// Position and formation of the formation whose group is `gid`.
fn formation(sim: &Sim, gid: u32) -> Option<(LatLon, Formation)> {
    let w = sim.world();
    let g = w.get::<Group>(entity(w, wb_core::UnitId(gid))?)?;
    let e = entity(w, g.lead()?)?;
    Some((
        w.get::<Kinematics>(e)?.pos(),
        w.get::<Formation>(e)?.clone(),
    ))
}

const HEAD: &str = "seed: 11\ncontent: scenarios/fixtures\nstart_utc: 2026-10-27T09:00:00Z\nsides:\n  - { name: Blue, nations: [tst], roe: charlie }\n  - { name: Red, nations: [opp], roe: charlie }\n";

/// Open land owned by `tst` with a lake across the direct line and a patch
/// of mountain.
const LAND: &str = "test_terrain:\n  west: 16\n  south: 55\n  land:\n    - { class: open, box: [16.1, 55.1, 16.9, 55.9], nation: tst }\n    - { class: water, box: [16.45, 55.1, 16.55, 55.6] }\n    - { class: mountain, box: [16.6, 55.7, 16.8, 55.9] }\n";

#[test]
fn march_goes_round_water_and_slows_in_mountains() {
    let yaml = format!(
        "name: march\nstart_mode: deploy\n{HEAD}{LAND}units:\n  - {{ id: 10, name: Tanks, system: test-tank-battalion, nation: tst, lat_deg: 55.3, lon_deg: 16.2 }}\n  - {{ id: 20, name: Climbers, system: test-infantry-battalion, nation: tst, lat_deg: 55.8, lon_deg: 16.5 }}\n  - {{ id: 30, name: Walkers, system: test-infantry-battalion, nation: tst, lat_deg: 55.65, lon_deg: 16.5 }}\n"
    );
    let mut sim = sim(&yaml);
    let mv = |p: [f64; 2]| TaskOrder::Move {
        waypoints: vec![p],
        alt_m: None,
        speed: None,
    };
    // Refused with a reason: into the lake, two destinations, another side's unit.
    let err = sim.submit(&task(0, 0, &[10], mv([16.5, 55.3]))).unwrap_err();
    assert!(err.contains("not on passable ground"), "{err}");
    let err = sim
        .submit(&task(
            0,
            0,
            &[10],
            TaskOrder::Move {
                waypoints: vec![[16.3, 55.3], [16.4, 55.3]],
                alt_m: None,
                speed: None,
            },
        ))
        .unwrap_err();
    assert!(err.contains("one destination"), "{err}");
    let err = sim.submit(&task(0, 1, &[10], mv([16.3, 55.3]))).unwrap_err();
    assert_eq!(err, "Unknown unit 10");
    let err = sim
        .submit(&task(0, 0, &[10], TaskOrder::Raid { point: [16.3, 55.3] }))
        .unwrap_err();
    assert!(err.contains("not trained for raids"), "{err}");

    // Across the lake: the route goes round its north end.
    let dest = LatLon::from_deg(55.3, 16.8);
    sim.submit(&task(0, 0, &[10], mv([16.8, 55.3]))).unwrap();
    // The same 12 km: one through the mountains, one over open ground.
    sim.submit(&task(0, 0, &[20], mv([16.7, 55.8]))).unwrap();
    sim.submit(&task(0, 0, &[30], mv([16.7, 55.65]))).unwrap();
    let geo = sim.geo_shared().expect("test terrain");
    let mut northmost: f64 = 0.0;
    let (mut t_mountain, mut t_open) = (None, None);
    for _ in 0..20_000 {
        sim.step();
        let (at, _) = formation(&sim, 10).unwrap();
        assert_ne!(
            geo.terrain_class(at).unwrap(),
            wb_geo::TerrainClass::Water,
            "the tanks never enter the lake"
        );
        northmost = northmost.max(at.lat_deg());
        let t = sim.clock().sim_time_ms / 1_000;
        if t_mountain.is_none()
            && distance_m(formation(&sim, 20).unwrap().0, LatLon::from_deg(55.8, 16.7)) < 100.0
        {
            t_mountain = Some(t);
        }
        if t_open.is_none()
            && distance_m(formation(&sim, 30).unwrap().0, LatLon::from_deg(55.65, 16.7)) < 100.0
        {
            t_open = Some(t);
        }
        if distance_m(at, dest) < 100.0 && t_mountain.is_some() {
            break;
        }
    }
    let (at, f) = formation(&sim, 10).unwrap();
    assert!(distance_m(at, dest) < 100.0, "the tanks arrive");
    assert!(northmost > 55.6, "round the lake's north end ({northmost})");
    assert_eq!(f.items(), 31);
    let (tm, to) = (t_mountain.expect("climbers arrive"), t_open.expect("walkers arrive"));
    assert!(
        tm as f64 > 1.5 * to as f64,
        "mountain cells are slower: {tm} s against {to} s"
    );
}

#[test]
fn bases_and_a_placed_fob_resupply() {
    // One battalion beside the air base, one far out on own land.
    let yaml = format!(
        "name: supply\nstart_mode: garrisoned\n{HEAD}{LAND}units:\n  - {{ id: 10, name: Near, system: test-tank-battalion, nation: tst, lat_deg: 55.16, lon_deg: 14.97 }}\n  - {{ id: 20, name: Far, system: test-tank-battalion, nation: tst, lat_deg: 55.3, lon_deg: 16.2 }}\n"
    );
    let mut sim = sim(&yaml);
    let place = |lat: f64, lon: f64| Order {
        tick: 0,
        side: 0,
        command: Command::PlaceBase {
            kind: wb_content::source::realism::BaseKind::ArmyBase,
            lat_deg: lat,
            lon_deg: lon,
            name: Some("FOB One".into()),
        },
    };
    let err = sim.submit(&place(55.3, 16.5)).unwrap_err();
    assert!(err.contains("water"), "a FOB cannot stand in the lake: {err}");
    sim.submit(&place(55.3, 16.22)).unwrap();
    // Five hours: the FOB (6 h to set up) is not open yet.
    sim.run_ticks(5 * 3_600);
    let near = formation(&sim, 10).unwrap().1.supply;
    let far = formation(&sim, 20).unwrap().1.supply;
    assert!((near - 1.0).abs() < 1e-9, "resupplied at the base: {near}");
    assert!(far < 0.96, "out of supply range: {far}");
    // Two hours after it opens the battalion is full again.
    sim.run_ticks(3 * 3_600);
    let far2 = formation(&sim, 20).unwrap().1.supply;
    assert!(far2 > far && far2 > 0.99, "the FOB resupplies: {far} → {far2}");
    assert!(
        sim.events()
            .iter()
            .any(|e| matches!(e.kind, EventKind::BaseActivated { .. }))
    );
}

#[test]
fn raid_sabotages_a_base_and_comes_home() {
    // No terrain: routes are straight lines. The team starts 4 km from the
    // opposing air base.
    let yaml = format!(
        "name: raid\nstart_mode: garrisoned\n{HEAD}units:\n  - {{ id: 10, name: Team, system: test-recon-detachment, nation: tst, lat_deg: 55.6, lon_deg: 20.84 }}\n"
    );
    let mut sim = sim(&yaml);
    let start = formation(&sim, 10).unwrap().0;
    sim.submit(&task(0, 0, &[10], TaskOrder::Raid { point: [20.9, 55.6] }))
        .unwrap();
    sim.run_ticks(3 * 3_600);
    let ev = sim.events();
    let (tick, success, component) = ev
        .iter()
        .find_map(|e| match &e.kind {
            EventKind::Sabotage {
                success, component, ..
            } => {
                assert_eq!(e.sides, 1, "only the raiders' side hears of the raid");
                Some((e.tick, *success, component.clone()))
            }
            _ => None,
        })
        .expect("the raid ended");
    // 4 km at 15 km/h, then the sabotage time.
    assert!((1_400..2_000).contains(&tick), "raid ended at tick {tick}");
    let damaged = ev.iter().find_map(|e| match &e.kind {
        EventKind::BaseComponentDamaged { component, .. } => {
            assert_eq!(e.sides, 2, "the base's side sees the damage");
            Some(component.clone())
        }
        _ => None,
    });
    assert_eq!(success, damaged.is_some());
    assert_eq!(component, damaged);
    assert!(success, "skill 0.9 with this seed");
    let (at, _) = formation(&sim, 10).unwrap();
    assert!(distance_m(at, start) < 100.0, "the team came back");
}

#[test]
fn a_team_designates_what_it_watches() {
    let yaml = format!(
        "name: designate\nstart_mode: deploy\n{HEAD}{LAND}units:\n  - {{ id: 10, name: Team, system: test-recon-detachment, nation: tst, lat_deg: 55.3, lon_deg: 16.2 }}\n  - {{ id: 100, name: Column, system: test-armour-column, nation: opp, lat_deg: 55.3, lon_deg: 16.3 }}\n"
    );
    let mut sim = sim(&yaml);
    sim.run_ticks(30);
    let contact = {
        let pics = sim.world().resource::<SidePictures>();
        let c = pics.sides[0].contacts.values().next().expect("the team sees the column");
        assert_eq!(c.count, (12, 12), "its scope counts the tanks");
        c.id.0
    };
    // The tanks, 6 km away, do not see a hidden team of six.
    assert!(
        sim.world().resource::<SidePictures>().sides[1]
            .contacts
            .is_empty(),
        "the team stays unseen"
    );
    let err = sim
        .submit(&task(31, 1, &[100], TaskOrder::Designate { contact: 1 }))
        .unwrap_err();
    assert!(err.contains("Unknown contact") || err.contains("designator"), "{err}");
    sim.submit(&task(31, 0, &[10], TaskOrder::Designate { contact }))
        .unwrap();
    sim.run_ticks(600);
    let on = sim.events().iter().any(|e| {
        matches!(&e.kind, EventKind::Designation { formation, contact: c, on: true }
            if formation.0 == 10 && c.0 == contact)
            && e.sides == 1
    });
    assert!(on, "the team designates the column");
    let state = sim.world().resource::<wb_sim::ground::GroundState>();
    assert_eq!(state.designations.len(), 1);
    // It closed to inside its designator's reach, and no nearer than needed.
    let d = distance_m(formation(&sim, 10).unwrap().0, formation(&sim, 100).unwrap().0);
    assert!((1_000.0..8_000.0).contains(&d), "{d}");
}

const G18: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../scenarios/golden/g18-ground-terrain.yaml"
);

#[test]
fn battle_survives_a_save_and_tells_each_side_only_its_own() {
    let (db, hash) = fixtures();
    let yaml = std::fs::read_to_string(G18).expect("scenario");
    let mut a = Sim::from_yaml(&yaml, db.clone(), hash, None).expect("builds");
    a.run_ticks(2_200);
    let bytes = a.save("ground", Vec::new(), 0);
    let mut b = Sim::load(&bytes, db, hash, None).expect("loads");
    assert_eq!(a.state_hash(), b.state_hash());
    a.run_ticks(600);
    b.run_ticks(600);
    assert_eq!(a.state_hash(), b.state_hash(), "the battle continues the same");

    // Ground events name only formations of the side that sees them, and a
    // fight was explained to both.
    let own = |side: u8, id: u32| (side == 0) == (id < 200);
    let mut fought = [0, 0];
    for e in a.events() {
        let formation = match &e.kind {
            EventKind::GroundCombat {
                formation, factors, ..
            } => {
                assert!(factors.strength > 0.0 && factors.protection > 0.0);
                fought[e.sides.trailing_zeros() as usize] += 1;
                formation
            }
            EventKind::GroundContact { formation }
            | EventKind::PostureChanged { formation, .. }
            | EventKind::FormationRetreated { formation, .. }
            | EventKind::Breakthrough { formation }
            | EventKind::FormationStruck { formation, .. } => formation,
            _ => continue,
        };
        assert_eq!(e.sides.count_ones(), 1);
        assert!(
            own(e.sides.trailing_zeros() as u8, formation.0),
            "{e:?} went to the wrong side"
        );
    }
    assert!(fought[0] > 0 && fought[1] > 0, "{fought:?}");
    // Each side knows its enemy as a contact, never by unit id.
    let pics = a.world().resource::<SidePictures>();
    assert_eq!(pics.sides[0].contacts.len(), 3);
    assert_eq!(pics.sides[1].contacts.len(), 3);
}
