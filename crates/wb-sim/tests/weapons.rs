//! Weapons in flight (P5 step 3, doc 02 §10).
//!
//! - Calibration rule: every weapon, fired at its reference altitude and
//!   speed against a non-manoeuvring head-on target, reaches its published
//!   range within `range_calibration_tolerance` (fuzes at `R × (1 − tol)`,
//!   does not at `R × (1 + tol)`).
//! - A save in mid-flight reloads to the same state hash.
//!
//! `cargo test -p wb-sim --test weapons -- --ignored --nocapture` prints
//! each weapon's achieved range (bisection), for tuning `coast_tau_s`.

use std::sync::Arc;
use wb_content::compiled::{Weapon, WeaponIx};
use wb_content::source::Domain;
use wb_content::source::weapon::Calibration;
use wb_core::UnitId;
use wb_core::geo::{LatLon, destination_point};
use wb_core::units::deg_to_rad;
use wb_sim::components::{Magazine, StationLoad};
use wb_sim::events::Outcome;
use wb_sim::weapons::{Aim, Launch, LaunchReason, launch};
use wb_sim::world::entity;
use wb_sim::{EventKind, Rules, Sim};

const SHOOTER: u32 = 10;
const TARGET: u32 = 20;

fn content() -> (Arc<wb_content::ContentDb>, [u8; 32]) {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../content");
    let set = wb_content::load_dir(std::path::Path::new(dir)).expect("content loads");
    let db = wb_content::compile(&set);
    let (header, _) = wb_content::encode(&db, &set.manifest.content_version);
    (Arc::new(db), header.content_hash)
}

fn calibration<'a>(rules: &'a Rules, w: &'a Weapon) -> &'a Calibration {
    w.profile
        .calibration
        .as_ref()
        .unwrap_or_else(|| &rules.realism().weapons.calibration[&w.category])
}

/// A shooter at its reference conditions and a head-on target `range_m`
/// east of it; the weapon fired at tick 0.
fn setup(db: &Arc<wb_content::ContentDb>, hash: [u8; 32], wix: WeaponIx, range_m: f64) -> Sim {
    let rules = Rules::new(db.clone(), hash);
    let w = &db.weapons[wix.0 as usize];
    let wid = db.str(w.id);
    let cal = calibration(&rules, w);
    let from = LatLon::from_deg(55.0, 10.0);
    let to = destination_point(from, deg_to_rad(90.0), range_m);
    let air_launch = cal.launch_alt_m > 0.0 || cal.launch_speed_mach > 0.0;
    let shooter = if air_launch {
        format!(
            "{{ id: {SHOOTER}, name: Shooter, platform: f-35a, country: ita, lat_deg: 55.0, lon_deg: 10.0, alt_m: {}, heading_deg: 90, speed_mps: {} }}",
            cal.launch_alt_m,
            cal.launch_speed_mach * rules.speed_of_sound(cal.launch_alt_m)
        )
    } else {
        format!(
            "{{ id: {SHOOTER}, name: Shooter, platform: fremm-bergamini, country: ita, lat_deg: 55.0, lon_deg: 10.0, heading_deg: 90 }}"
        )
    };
    let air_target = w.targets.contains(&Domain::Air);
    let (units, scripted) = if air_target {
        (
            String::new(),
            format!(
                "scripted_targets:\n  - {{ id: {TARGET}, name: Target, weapon: {wid}, country: rus, lat_deg: {}, lon_deg: {}, alt_m: {}, heading_deg: 270, speed_mps: {} }}\n",
                to.lat_deg(),
                to.lon_deg(),
                cal.target_alt_m,
                cal.target_speed_mach * rules.speed_of_sound(cal.target_alt_m)
            ),
        )
    } else {
        (
            format!(
                "  - {{ id: {TARGET}, name: Target, platform: steregushchiy, country: rus, lat_deg: {}, lon_deg: {}, heading_deg: 270, speed_mps: 0, emcon: on }}\n",
                to.lat_deg(),
                to.lon_deg()
            ),
            String::new(),
        )
    };
    let yaml = format!(
        "name: calibration\nseed: 1\nstart_utc: 2026-10-27T12:00:00Z\nstart_mode: deploy\nsides:\n  - {{ name: NATO, countries: [ita] }}\n  - {{ name: Russia, countries: [rus] }}\nunits:\n  - {shooter}\n{units}{scripted}"
    );
    let mut sim = Sim::from_yaml(&yaml, db.clone(), hash, None).expect("calibration scenario");
    // Scenario ids name groups; the platforms are their members.
    let shooter = UnitId(SHOOTER + 1);
    let target = if air_target {
        UnitId(TARGET)
    } else {
        UnitId(TARGET + 1)
    };
    let world = sim.world_mut();
    let se = entity(world, shooter).expect("shooter");
    world.entity_mut(se).insert(Magazine {
        loadout: None,
        stations: vec![StationLoad {
            station: 0,
            weapon: wix,
            full: 1,
            count: 1,
        }],
    });
    let aim = if wb_sim_seeks(w) {
        Aim::Truth(target)
    } else {
        Aim::Point { at: to, alt_m: 0.0 }
    };
    launch(
        world,
        &Launch {
            shooter,
            weapon: wix,
            aim,
            salvo: None,
            reason: LaunchReason::Test,
        },
    )
    .expect("launches");
    sim
}

/// Weapons with a homing seeker are fired at the target, the others at its
/// coordinates.
fn wb_sim_seeks(w: &Weapon) -> bool {
    use wb_content::source::Seeker;
    !matches!(w.guidance.terminal, Seeker::GpsIns | Seeker::None)
}

/// Runs until the weapon's flight ends.
fn outcome(sim: &mut Sim) -> (Outcome, f64) {
    for _ in 0..40_000 {
        sim.step();
        if let Some((o, tof)) = sim.events().iter().rev().find_map(|e| match &e.kind {
            EventKind::WeaponTerminated {
                outcome, factors, ..
            } => Some((*outcome, factors.time_of_flight_s)),
            _ => None,
        }) {
            return (o, tof);
        }
    }
    panic!("weapon still flying after 40000 ticks");
}

/// The weapon reached its target (whatever the endgame roll said).
fn fuzed(o: Outcome) -> bool {
    matches!(o, Outcome::Hit | Outcome::Miss | Outcome::Decoyed)
}

fn weapons(db: &wb_content::ContentDb) -> Vec<WeaponIx> {
    (0..db.weapons.len() as u32).map(WeaponIx).collect()
}

#[test]
fn every_weapon_reaches_its_published_range() {
    let (db, hash) = content();
    let rules = Rules::new(db.clone(), hash);
    let tol = rules.realism().weapons.range_calibration_tolerance;
    let mut bad = Vec::new();
    for wix in weapons(&db) {
        let w = &db.weapons[wix.0 as usize];
        let r = w.profile.max_range_m;
        let (near, _) = outcome(&mut setup(&db, hash, wix, r * (1.0 - tol)));
        let (far, _) = outcome(&mut setup(&db, hash, wix, r * (1.0 + tol)));
        if !fuzed(near) || fuzed(far) {
            bad.push(format!(
                "{}: at {:.0} km {near:?}, at {:.0} km {far:?}",
                db.str(w.id),
                r * (1.0 - tol) / 1_000.0,
                r * (1.0 + tol) / 1_000.0
            ));
        }
    }
    assert!(bad.is_empty(), "outside calibration:\n{}", bad.join("\n"));
}

#[test]
#[ignore = "tuning report: prints each weapon's achieved range"]
fn calibration_report() {
    let (db, hash) = content();
    for wix in weapons(&db) {
        let w = &db.weapons[wix.0 as usize];
        let r = w.profile.max_range_m;
        let (mut lo, mut hi) = (0.05 * r, 3.0 * r);
        for _ in 0..14 {
            let mid = (lo + hi) / 2.0;
            if fuzed(outcome(&mut setup(&db, hash, wix, mid)).0) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        println!(
            "{:<12} published {:>6.0} km, reaches {:>6.0} km ({:.2})",
            db.str(w.id),
            r / 1_000.0,
            lo / 1_000.0,
            lo / r
        );
        // Rockets: the `coast_tau_s` (both ends scaled alike) that just
        // reaches the published range.
        let Some(tau) = w.profile.coast_tau_s.clone() else {
            continue;
        };
        let (mut lo, mut hi) = (0.25, 20.0);
        for _ in 0..16 {
            let mid: f64 = (lo + hi) / 2.0;
            let mut tuned = (*db).clone();
            let t = tuned.weapons[wix.0 as usize]
                .profile
                .coast_tau_s
                .as_mut()
                .expect("tau");
            t.sea_level = tau.sea_level * mid;
            t.high_alt = tau.high_alt * mid;
            let tuned = Arc::new(tuned);
            if fuzed(outcome(&mut setup(&tuned, hash, wix, r)).0) {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        println!(
            "{:<12} coast_tau_s × {hi:.3}: {{ sea_level: {:.1}, high_alt: {:.1} }}",
            "",
            tau.sea_level * hi,
            tau.high_alt * hi
        );
    }
}

/// Effects in a side's view (header bytes 36..40).
fn effect_rows(enc: &mut wb_sim::view::ViewEncoder, sim: &Sim, side: u8) -> u32 {
    let v = enc.encode(sim.world(), side);
    u32::from_le_bytes(v[36..40].try_into().expect("header"))
}

#[test]
fn effects_reach_only_the_sides_that_may_see_them() {
    use wb_sim::view::effect;
    use wb_sim::weapons::Combat;
    let (db, hash) = content();
    let wix = WeaponIx(
        db.weapons
            .iter()
            .position(|w| db.str(w.id) == "aim-120d")
            .expect("AIM-120D") as u32,
    );
    let mut sim = setup(&db, hash, wix, 40_000.0);
    let mut enc = wb_sim::view::ViewEncoder::new();
    // The launch flash is the shooter's (side 0): the other side's view has none.
    assert_eq!(effect_rows(&mut enc, &sim, 0), 1, "own launch flash");
    assert_eq!(
        effect_rows(&mut enc, &sim, 1),
        0,
        "enemy launch flash leaked"
    );
    let fx = &sim.world().resource::<Combat>().effects;
    assert_eq!((fx[0].kind, fx[0].sides), (effect::LAUNCH, 0b01));
    // Until the flight ends, and after it, each view holds what its mask allows.
    for _ in 0..6_000 {
        sim.step();
        let fx = sim.world().resource::<Combat>().effects.clone();
        for side in 0..2u8 {
            let may = fx.iter().filter(|e| e.sides & (1 << side) != 0).count() as u32;
            assert_eq!(effect_rows(&mut enc, &sim, side), may, "side {side}");
        }
        assert!(
            fx.iter()
                .all(|e| e.kind != effect::LAUNCH || e.sides == 0b01),
            "a launch flash must stay with the shooter"
        );
    }
    // Long after the flight the effects have expired by age.
    assert!(sim.world().resource::<Combat>().effects.is_empty());
    assert_eq!(effect_rows(&mut enc, &sim, 0), 0);
}

#[test]
fn save_in_flight_reloads_to_the_same_hash() {
    let (db, hash) = content();
    let wix = WeaponIx(
        db.weapons
            .iter()
            .position(|w| db.str(w.id) == "aim-120d")
            .expect("AIM-120D") as u32,
    );
    let mut a = setup(&db, hash, wix, 60_000.0);
    a.run_ticks(40);
    let bytes = a.save("in flight", Vec::new(), 0);
    let mut b = Sim::load(&bytes, db.clone(), hash, None).expect("loads");
    assert_eq!(a.state_hash(), b.state_hash());
    a.run_ticks(600);
    b.run_ticks(600);
    assert_eq!(a.state_hash(), b.state_hash());
    let ended = |s: &Sim| {
        s.events()
            .iter()
            .any(|e| matches!(e.kind, EventKind::WeaponTerminated { .. }))
    };
    assert!(ended(&a) && ended(&b), "the shot should be over");
}
