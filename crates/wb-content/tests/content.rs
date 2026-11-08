//! The real `content/` folder: validates, builds deterministically, decodes
//! to the same database, converts units correctly, carries design notes.

use std::path::Path;
use wb_content::compiled::{ContentDb, StrId};
use wb_content::source::Kind;
use wb_content::{ContentSet, compile, decode, encode, load_dir};

fn content() -> ContentSet {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
    load_dir(&dir).unwrap_or_else(|errors| {
        panic!(
            "content check failed:\n{}",
            errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        )
    })
}

fn by_id<'a, T>(db: &ContentDb, list: &'a [T], id: impl Fn(&T) -> StrId, want: &str) -> &'a T {
    list.iter()
        .find(|x| db.str(id(x)) == want)
        .unwrap_or_else(|| panic!("`{want}` not in pack"))
}

#[test]
fn builds_twice_with_the_same_hash_and_round_trips() {
    let a = compile(&content());
    let b = compile(&content());
    let (ha, bytes_a) = encode(&a, "test");
    let (hb, bytes_b) = encode(&b, "test");
    assert_eq!(bytes_a, bytes_b);
    assert_eq!(ha.content_hash, hb.content_hash);

    let (header, decoded) = decode(&bytes_a).unwrap();
    assert_eq!(header, ha);
    assert_eq!(decoded, a);

    // A flipped body byte is caught by the hash.
    let mut corrupt = bytes_a.clone();
    let last = corrupt.len() - 1;
    corrupt[last] ^= 0xff;
    assert!(decode(&corrupt).is_err());
}

#[test]
fn units_are_converted_to_si() {
    let db = compile(&content());
    let w = by_id(&db, &db.weapons, |w| w.id, "aim-120d");
    assert_eq!(w.profile.max_range_m, 160_000.0);

    let p = by_id(&db, &db.platforms, |p| p.id, "f-35a");
    assert_eq!(p.turnaround_s, Some(3_600.0));
    let burn = p.fuel.as_ref().unwrap().burn_kg_s.as_ref().unwrap();
    assert!((burn.cruise - 2_600.0 / 3_600.0).abs() < 1e-12);

    let s = by_id(&db, &db.sensors, |s| s.id, "an-apg-81");
    let radar = s.radar.as_ref().unwrap();
    assert!((radar.field_of_regard_rad - 120f64.to_radians()).abs() < 1e-12);

    let ship = by_id(&db, &db.platforms, |p| p.id, "fremm-bergamini");
    let kt = 1_852.0 / 3_600.0;
    assert!((ship.mobility.max_speed_mps.unwrap() - 30.0 * kt).abs() < 1e-9);

    let base = by_id(&db, &db.bases, |b| b.id, "taranto");
    assert!(base.lat_rad > 0.70 && base.lat_rad < 0.72);

    let r = &db.realism[0];
    assert_eq!(r.fusion.age_out_s[&wb_content::source::Domain::Air], 120.0);
    assert_eq!(r.tewa.ciws_layer_max_m, 4_000.0);
}

#[test]
fn every_entry_has_a_design_note_and_catalogue_entries_an_archetype() {
    let set = content();
    for e in &set.entries {
        if matches!(e.kind, Kind::Archetype | Kind::World | Kind::Realism) {
            continue;
        }
        let d = e
            .entry
            .design()
            .unwrap_or_else(|| panic!("{}: no design", e.file));
        assert!(!d.note.trim().is_empty(), "{}: empty design note", e.file);
    }
    // The compiled pack carries the archetype's class name.
    let db = compile(&set);
    assert!(db.weapons.iter().all(|w| w.design.class_name.is_some()));
    assert!(db.platforms.iter().all(|p| p.design.class_name.is_some()));
}
