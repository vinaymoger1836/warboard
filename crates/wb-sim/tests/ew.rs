//! Tests for electronic warfare (P6b, doc 02 §9).

use std::sync::Arc;
use wb_sim::Sim;
use wb_sim::events::EventKind;

fn fixtures() -> (Arc<wb_content::ContentDb>, [u8; 32]) {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../scenarios/fixtures");
    let set = wb_content::load_dir(std::path::Path::new(dir)).expect("fixtures load");
    let db = wb_content::compile(&set);
    let (header, _) = wb_content::encode(&db, &set.manifest.content_version);
    (Arc::new(db), header.content_hash)
}

#[test]
fn arm_memory_mode_records_f_env() {
    let (db, hash) = fixtures();

    let arm_yaml = r#"
name: EW ARM test
seed: 42
start_utc: 2026-10-27T10:00:00Z
start_mode: deploy
sides:
  - { name: Blue, countries: [tst], roe: alpha }
  - { name: Red, countries: [opp], roe: alpha }
units:
  - { id: 10, name: Fighter, platform: test-fighter, country: tst, lat_deg: 55.0, lon_deg: 10.4, alt_m: 8000, heading_deg: 270, loadout: test-fighter-sead }
  - { id: 20, name: SAM-Radar, platform: test-sam-radar-unit, country: opp, lat_deg: 55.0, lon_deg: 10.0, alt_m: 0, heading_deg: 90 }
orders:
  - { tick: 0, side: 0, type: AssignTask, units: [10], task: { kind: suppress, point: [10.0, 55.0], weapon: test-arm, count: 1 } }
  - { tick: 8, side: 1, type: SetEmcon, units: [20], emcon: off }
"#;

    let mut sim = Sim::from_yaml(arm_yaml, db.clone(), hash, None).expect("sim builds");
    let mut terminated = false;
    for _ in 0..450 {
        sim.step();
        for ev in sim.events() {
            if let EventKind::WeaponTerminated { factors, .. } = &ev.kind {
                let eg = factors.endgame.as_ref().expect("endgame factors present");
                assert_eq!(eg.f_env, 0.45, "arm memory factor must be 0.45");
                assert!((eg.f_ew - 1.0).abs() < 1e-6, "f_ew must be recorded");
                terminated = true;
                break;
            }
        }
        if terminated {
            break;
        }
    }
    assert!(terminated, "ARM weapon must terminate");
}

#[test]
fn every_endgame_under_jamming_records_f_ew() {
    let (db, hash) = fixtures();

    // Red SAM radar defended by a Red jammer aircraft in escort mode
    let jam_yaml = r#"
name: EW seeker jamming test
seed: 42
start_utc: 2026-10-27T10:00:00Z
start_mode: deploy
sides:
  - { name: Blue, countries: [tst], roe: alpha }
  - { name: Red, countries: [opp], roe: alpha }
units:
  - { id: 10, name: Fighter, platform: test-fighter, country: tst, lat_deg: 55.0, lon_deg: 10.4, alt_m: 8000, heading_deg: 270, loadout: test-fighter-sead }
  - { id: 20, name: SAM-Radar, platform: test-sam-radar-unit, country: opp, lat_deg: 55.0, lon_deg: 10.0, alt_m: 0, heading_deg: 90 }
  - { id: 30, name: Jammer, platform: test-jammer-aircraft, country: opp, lat_deg: 55.0, lon_deg: 10.0, alt_m: 5000, heading_deg: 90, jammer: true, jammer_mode: stand_off }
orders:
  - { tick: 0, side: 0, type: AssignTask, units: [10], task: { kind: suppress, point: [10.0, 55.0], weapon: test-arm, count: 1 } }
  - { tick: 0, side: 1, type: AssignTask, units: [30], task: { kind: jam, centre: [10.0, 55.0], radius_km: 5, alt_m: 5000 } }
"#;

    let mut sim = Sim::from_yaml(jam_yaml, db.clone(), hash, None).expect("sim builds");
    let mut terminated = false;
    for _ in 0..450 {
        sim.step();
        for ev in sim.events() {
            if let EventKind::WeaponTerminated { factors, .. } = &ev.kind {
                let eg = factors.endgame.as_ref().expect("endgame factors present");
                // test-arm has seeker_resistance: 0.5.
                // seeker_jam is 0.25 in realism.
                // f_ew = 1.0 - 0.25 * (1.0 - 0.5) = 0.875.
                assert!(eg.f_ew < 1.0, "f_ew must be < 1.0 under jamming");
                assert!(
                    (eg.f_ew - 0.875).abs() < 1e-4,
                    "f_ew must match 0.875, got {}",
                    eg.f_ew
                );
                terminated = true;
                break;
            }
        }
        if terminated {
            break;
        }
    }
    assert!(terminated, "weapon must terminate under jamming");
}

/// Reads the `flags` column of the contacts section of a `SideView` (layout in
/// the `wb_sim::view` module doc: columns padded to 8 bytes).
fn contact_flags(view: &[u8]) -> Vec<u32> {
    let pad8 = |n: usize| (n + 7) & !7;
    let u32_at = |o: usize| u32::from_le_bytes(view[o..o + 4].try_into().unwrap()) as usize;
    let (nu, nc) = (u32_at(24), u32_at(28));
    let units = pad8(8 * nu) * 2 + pad8(4 * nu) * 6 + pad8(2 * nu) * 2 + pad8(nu);
    let flags = wb_sim::view::HEADER_BYTES + units + pad8(8 * nc) * 2 + pad8(4 * nc);
    (0..nc).map(|i| u32_at(flags + 4 * i) as u32).collect()
}

#[test]
fn a_hostile_radar_inside_our_jammers_coverage_is_flagged_jammed() {
    use wb_sim::view::{ViewEncoder, cflag};
    let (db, hash) = fixtures();

    let yaml = r#"
name: EW contact flag
seed: 7
start_utc: 2026-10-27T10:00:00Z
start_mode: deploy
sides:
  - { name: Blue, countries: [tst] }
  - { name: Red, countries: [opp] }
units:
  - { id: 10, name: Watcher, platform: test-fighter, country: tst, lat_deg: 55.0, lon_deg: 11.0, alt_m: 9000, heading_deg: 90 }
  - { id: 30, name: Jammer, platform: test-jammer-aircraft, country: tst, lat_deg: 55.0, lon_deg: 11.4, alt_m: 10000, heading_deg: 90, jammer: true, jammer_mode: stand_off }
  - { id: 20, name: Radar, platform: test-sam-radar-unit, country: opp, lat_deg: 55.0, lon_deg: 13.0, alt_m: 0, heading_deg: 270 }
orders:
  - { tick: 0, side: 0, type: AssignTask, units: [30], task: { kind: jam, centre: [11.4, 55.0], radius_km: 5, alt_m: 10000 } }
  - { tick: 0, side: 0, type: AssignTask, units: [10], task: { kind: move, waypoints: [[11.0, 55.0]], alt_m: 9000 } }
"#;
    let mut sim = Sim::from_yaml(yaml, db, hash, None).expect("sim builds");
    let mut enc = ViewEncoder::new();
    for _ in 0..100 {
        sim.step();
    }
    let flags = contact_flags(enc.encode(sim.world(), 0));
    assert_eq!(flags.len(), 1, "Blue holds one contact: the radar");
    assert_ne!(
        flags[0] & cflag::JAMMED,
        0,
        "the radar is inside the jammer's coverage, so the contact is flagged"
    );
}
