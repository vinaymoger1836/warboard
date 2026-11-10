//! War setup (doc 03 §2, ADR-046): the builder turns coalitions and controllers
//! into a war from the real Theatre North content, garrisoned from the
//! inventories or deployed by hand, and the engine holds the clock while a
//! side deploys.

use std::collections::BTreeMap;
use std::sync::Arc;
use wb_sim::components::{Allegiance, BaseSite, PlatformRef};
use wb_sim::orders::{BaseTarget, Command, Order, RejectKind};
use wb_sim::setup::{SideSetup, WarSetup, build_scenario, nation_options};
use wb_sim::world::{Controller, Difficulty, PlayMode, UnitIndex};
use wb_sim::{Sim, deployment};

fn content() -> (Arc<wb_content::ContentDb>, [u8; 32]) {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../content");
    let set = wb_content::load_dir(std::path::Path::new(dir)).expect("content loads");
    let db = wb_content::compile(&set);
    let (header, _) = wb_content::encode(&db, &set.manifest.content_version);
    (Arc::new(db), header.content_hash)
}

fn side(name: &str, nations: &[&str], controller: Controller) -> SideSetup {
    SideSetup {
        name: name.to_owned(),
        nations: nations.iter().map(|n| (*n).to_owned()).collect(),
        controller,
        difficulty: Difficulty::Normal,
        roe: wb_sim::components::Roe::Bravo,
    }
}

fn setup(start_mode: wb_sim::scenario::StartMode, a: Controller, b: Controller) -> WarSetup {
    WarSetup {
        name: "Setup test".to_owned(),
        seed: 11,
        start_utc: "2026-10-27T06:00:00Z".to_owned(),
        start_mode,
        mode: None,
        sides: vec![
            side("Straits League", &["ald", "hlv", "mrv"], a),
            side("Eastern Pact", &["vsm", "kld"], b),
        ],
    }
}

fn build(
    ws: &WarSetup,
) -> (
    Sim,
    wb_sim::setup::SetupReport,
    Arc<wb_content::ContentDb>,
    [u8; 32],
) {
    let (db, hash) = content();
    let built = build_scenario(&db, ws).expect("setup builds");
    let sim = Sim::new(&built.scenario, db.clone(), hash, None).expect("war builds");
    (sim, built.report, db, hash)
}

/// Members in the war by (nation, platform id).
fn members(sim: &Sim, db: &wb_content::ContentDb) -> BTreeMap<(String, String), u32> {
    let w = sim.world();
    let mut out = BTreeMap::new();
    for e in w.resource::<UnitIndex>().0.values() {
        let (Some(a), Some(p)) = (w.get::<Allegiance>(*e), w.get::<PlatformRef>(*e)) else {
            continue;
        };
        let nation = db.str(db.nations[a.nation.0 as usize].id).to_owned();
        let plat = db.str(db.platforms[p.0.0 as usize].id).to_owned();
        *out.entry((nation, plat)).or_insert(0) += 1;
    }
    out
}

#[test]
fn a_garrisoned_start_fields_every_inventory_force() {
    let ws = setup(
        wb_sim::scenario::StartMode::Garrisoned,
        Controller::Human,
        Controller::Ai,
    );
    let (mut sim, report, db, _) = build(&ws);
    assert_eq!(sim.sides().mode, PlayMode::VsAi);
    assert!(sim.sides().is_ai(1) && !sim.sides().is_ai(0));

    // Platforms in the war = inventory platforms - the unplaced.
    let in_war = members(&sim, &db);
    let mut expected: BTreeMap<(String, String), u32> = BTreeMap::new();
    for inv in &db.inventories {
        let nation = db.str(db.nations[inv.nation.0 as usize].id).to_owned();
        if !ws.sides.iter().any(|s| s.nations.contains(&nation)) {
            continue;
        }
        for (f, n) in &inv.platforms {
            if let wb_content::compiled::ForceIx::Platform(p) = f {
                let id = db.str(db.platforms[p.0 as usize].id).to_owned();
                *expected.entry((nation.clone(), id)).or_insert(0) += n;
            }
        }
    }
    for u in &report.unplaced {
        if let Some(e) = expected.get_mut(&(u.nation.clone(), u.id.clone())) {
            *e -= u.count;
        }
    }
    // Systems add their components to the members; compare platforms that
    // are inventory lines only.
    for (k, n) in &expected {
        assert!(
            in_war.get(k).copied().unwrap_or(0) >= *n,
            "{k:?}: inventory {n}, in the war {:?}",
            in_war.get(k)
        );
    }
    eprintln!(
        "garrisoned: {} platforms, {} systems, {} unplaced rows",
        report.platforms,
        report.systems,
        report.unplaced.len()
    );
    for u in &report.unplaced {
        eprintln!(
            "  unplaced {} {} x{}: {}",
            u.nation, u.id, u.count, u.reason
        );
    }
    assert!(
        report.unplaced.iter().map(|u| u.count).sum::<u32>() * 20 < report.platforms,
        "more than 5% of the forces had nowhere to go"
    );

    // Bases are open and hold units; it runs.
    let w = sim.world_mut();
    let mut q = w.query::<&BaseSite>();
    let bases: Vec<_> = q.iter(w).collect();
    assert!(bases.len() >= 20, "preset bases are open");
    assert!(bases.iter().all(|b| b.active));
    sim.run_ticks(200);
}

#[test]
fn the_same_setup_always_gives_the_same_war() {
    let ws = setup(
        wb_sim::scenario::StartMode::Garrisoned,
        Controller::Human,
        Controller::Ai,
    );
    let (mut a, _, _, _) = build(&ws);
    let (mut b, _, _, _) = build(&ws);
    assert_eq!(a.state_hash(), b.state_hash());
    a.run_ticks(100);
    b.run_ticks(100);
    assert_eq!(a.state_hash(), b.state_hash());
}

#[test]
fn bad_setups_are_refused_with_a_reason() {
    let (db, _) = content();
    let mut ws = setup(
        wb_sim::scenario::StartMode::Garrisoned,
        Controller::Human,
        Controller::Ai,
    );
    ws.sides[1].nations.push("ald".to_owned());
    let e = build_scenario(&db, &ws).err().expect("refused").to_string();
    assert!(e.contains("both coalitions"), "{e}");
    let mut ws = setup(
        wb_sim::scenario::StartMode::Garrisoned,
        Controller::Human,
        Controller::Ai,
    );
    ws.sides[0].nations = vec!["xyz".to_owned()];
    assert!(
        build_scenario(&db, &ws)
            .err()
            .expect("refused")
            .to_string()
            .contains("Unknown nation")
    );
    let mut ws = setup(
        wb_sim::scenario::StartMode::Garrisoned,
        Controller::Human,
        Controller::Ai,
    );
    ws.mode = Some(PlayMode::HotSeat);
    assert!(
        build_scenario(&db, &ws).is_err(),
        "hot-seat needs two humans"
    );
    ws.sides.pop();
    assert!(build_scenario(&db, &ws).is_err(), "two coalitions");
}

#[test]
fn nation_options_list_every_nation_with_an_extent() {
    let (db, _) = content();
    let opts = nation_options(&db);
    assert_eq!(opts.len(), db.nations.len());
    for o in &opts {
        assert!(o.platforms > 0 && o.bases > 0, "{}", o.id);
        assert!(
            o.extent[0] <= o.extent[2] && o.extent[1] <= o.extent[3],
            "{}",
            o.id
        );
    }
}

fn order(sim: &Sim, side: u8, command: Command) -> Order {
    Order {
        tick: sim.clock().tick,
        side,
        command,
    }
}

#[test]
fn deploying_yourself_holds_the_clock_until_the_side_finishes() {
    let ws = setup(
        wb_sim::scenario::StartMode::Deploy,
        Controller::Human,
        Controller::Ai,
    );
    let (mut sim, report, db, _) = build(&ws);
    assert_eq!(report.deploy_sides, vec![0]);
    // Side 0 has only its satellites; the AI side is garrisoned.
    let m = members(&sim, &db);
    assert!(!m.keys().any(|(n, p)| n == "ald" && p == "mf-4"));
    assert!(m.keys().any(|(n, _)| n == "vsm"));
    // The clock is held.
    assert_eq!(sim.advance(100.0, 1.0), 0);
    assert_eq!(sim.clock().tick, 0);

    let view = deployment::view(sim.world_mut(), 0);
    assert!(view.deploying && view.clock_held);
    let line = view
        .pool
        .iter()
        .find(|l| l.id == "mf-4" && l.nation == "ald")
        .expect("Aldmark's mf-4 are in the pool");
    assert_eq!(line.place, "airfield");
    assert!(line.default_loadout.is_some());
    let base = view
        .bases
        .iter()
        .find(|b| b.id > 0 && b.air && b.nation == "ald")
        .expect("an own air base")
        .clone();
    let before = line.left;

    // Rejections: too many, another side's base (never named), a naval base.
    let go = |sim: &mut Sim, side: u8, force: &str, count: u32, b: u32| {
        sim.submit_typed(&order(
            sim,
            side,
            Command::DeployForce {
                force: force.to_owned(),
                count,
                base: BaseTarget::Id(b),
                loadout: None,
            },
        ))
    };
    assert!(go(&mut sim, 0, "mf-4", before + 1, base.id).is_err());
    let ai_view = deployment::view(sim.world_mut(), 1);
    assert!(
        !ai_view.deploying && ai_view.pool.is_empty(),
        "the AI side deploys nothing"
    );
    let err = go(&mut sim, 1, "mf-4", 1, base.id).expect_err("the AI side takes no client orders");
    assert_eq!(err.kind, RejectKind::AiControlled);

    // Deploy 2 and check the group is at the base with a loadout.
    go(&mut sim, 0, "mf-4", 2, base.id).expect("deploy 2 mf-4");
    let m = members(&sim, &db);
    assert_eq!(m.get(&("ald".into(), "mf-4".into())), Some(&2));
    let view = deployment::view(sim.world_mut(), 0);
    let line = view
        .pool
        .iter()
        .find(|l| l.id == "mf-4" && l.nation == "ald")
        .expect("pool");
    assert_eq!(line.left, before - 2);
    // A mobile system goes beside a base.
    let sys = view
        .pool
        .iter()
        .find(|l| l.place == "ground")
        .expect("a site or system to deploy")
        .clone();
    go(&mut sim, 0, &sys.id, 1, base.id).expect("deploy a site");

    // Still held; finishing releases the clock.
    assert_eq!(sim.advance(100.0, 1.0), 0);
    sim.submit(&order(&sim, 0, Command::FinishDeployment))
        .expect("finish");
    assert!(
        sim.submit(&order(&sim, 0, Command::FinishDeployment))
            .is_err(),
        "only once"
    );
    assert!(sim.advance(250.0, 10.0) > 0, "the clock runs");
    let after = go(&mut sim, 0, "mf-4", 1, base.id);
    assert!(after.is_err(), "no deploying once finished");

    // A save made mid-war reloads to the same hash.
    let (db2, hash2) = content();
    let bytes = sim.save("deploy", vec![], 0);
    let loaded = Sim::load(&bytes, db2, hash2, None).expect("loads");
    assert_eq!(loaded.state_hash(), sim.state_hash());
}

#[test]
fn a_save_made_while_deploying_keeps_the_pool() {
    let ws = setup(
        wb_sim::scenario::StartMode::Deploy,
        Controller::Human,
        Controller::Ai,
    );
    let (mut sim, _, db, hash) = build(&ws);
    let base = deployment::view(sim.world_mut(), 0)
        .bases
        .iter()
        .find(|b| b.air)
        .map(|b| b.id)
        .expect("air base");
    sim.submit(&order(
        &sim,
        0,
        Command::DeployForce {
            force: "mf-4".into(),
            count: 1,
            base: BaseTarget::Id(base),
            loadout: None,
        },
    ))
    .ok();
    let h = sim.state_hash();
    let bytes = sim.save("deploying", vec![], 0);
    let mut loaded = Sim::load(&bytes, db, hash, None).expect("loads");
    assert_eq!(loaded.state_hash(), h);
    assert_eq!(loaded.advance(100.0, 1.0), 0, "still held after loading");
    assert!(deployment::view(loaded.world_mut(), 0).deploying);
}
