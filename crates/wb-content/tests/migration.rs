//! Tests for P6b schema migration: eccm_db -> jam_resistance, seeker_eccm -> seeker_resistance.

use wb_content::source::sensor::Radar;
use wb_content::source::weapon::Guidance;
use wb_content::source::ew::EwSuite;

#[test]
fn radar_eccm_db_migrates_to_jam_resistance() {
    let yaml = r#"
band: X
r_ref_km: 100
sigma_ref_m2: 1.0
scan_period_s: 2
field_of_regard_deg: 120
clutter_rejection: 0.8
pulse_doppler: true
eccm_db: 20
erp_class_dbw: 70
"#;
    let r: Radar = serde_yaml::from_str(yaml).expect("parse radar");
    assert_eq!(r.jam_resistance(), 6.0);
}

#[test]
fn radar_explicit_jam_resistance_takes_precedence() {
    let yaml = r#"
band: X
r_ref_km: 100
sigma_ref_m2: 1.0
scan_period_s: 2
field_of_regard_deg: 120
clutter_rejection: 0.8
pulse_doppler: true
jam_resistance: 8
erp_class_dbw: 70
"#;
    let r: Radar = serde_yaml::from_str(yaml).expect("parse radar");
    assert_eq!(r.jam_resistance(), 8.0);
}

#[test]
fn seeker_eccm_migrates_to_seeker_resistance() {
    let yaml = r#"
terminal: active_radar
seeker_eccm: 0.7
"#;
    let g: Guidance = serde_yaml::from_str(yaml).expect("parse guidance");
    assert_eq!(g.seeker_resistance(), Some(0.7));
}

#[test]
fn seeker_explicit_resistance_takes_precedence() {
    let yaml = r#"
terminal: active_radar
seeker_resistance: 0.85
"#;
    let g: Guidance = serde_yaml::from_str(yaml).expect("parse guidance");
    assert_eq!(g.seeker_resistance(), Some(0.85));
}

#[test]
fn legacy_ew_suite_migrates_to_modes() {
    let yaml = r#"
id: test-legacy-jammer
name: Legacy Jammer
ew_type: stand_off
bands: [X]
j_db: 30
max_range_km: 200
simultaneous_targets: 4
field_of_regard_deg: 120
provenance:
  confidence: low
  sources:
    - { title: "Test", url: "https://example.org" }
"#;
    let ew: EwSuite = serde_yaml::from_str(yaml).expect("parse legacy ew suite");
    let modes = ew.resolved_modes();
    let so = modes.stand_off.expect("stand_off mode migrated");
    assert_eq!(so.jam_strength, 9.0);
    assert_eq!(so.coverage_km, Some(200.0));
    assert_eq!(so.sector_deg, Some(120.0));
}
