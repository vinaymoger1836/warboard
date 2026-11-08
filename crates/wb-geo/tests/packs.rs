//! Acceptance tests against the region packs in `build/packs/` (doc 08 P2, doc 11 §2).
//! Packs are build output and not in git (CLAUDE.md rule 9), so these are ignored by
//! default. Build the packs (`bash geodata/run.sh build europe`, then `… build world`):
//!
//!     cargo test -p wb-geo --test packs -- --ignored
//!
//! `WB_PACKS` overrides the folder holding `world/` and `europe/`.
//!
//! The points belong to Theatre North as grown from `content/world/theatre-north.yaml`
//! (world seed 7301). Moving a seed or changing a weight moves the borders: rebuild the
//! packs and pick new points with `wb geo query --at lat,lon`.

use std::collections::BTreeSet;
use std::path::PathBuf;
use wb_core::geo::LatLon;
use wb_geo::{Geo, LosParams, NationCode, Pack};

fn geo() -> Geo {
    let root = std::env::var_os("WB_PACKS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../build/packs"));
    let mut geo = Geo::new();
    for id in ["world", "europe"] {
        let dir = root.join(id);
        let pack = Pack::open_dir(&dir)
            .unwrap_or_else(|e| panic!("{}: {e} (build the packs first)", dir.display()));
        geo.add_pack(pack).unwrap();
    }
    geo
}

fn owner(geo: &Geo, lat: f64, lon: f64) -> Option<String> {
    geo.territory_owner(LatLon::from_deg(lat, lon))
        .unwrap()
        .map(|c| c.to_string())
}

fn assert_owner(geo: &Geo, what: &str, lat: f64, lon: f64, expected: Option<&str>) {
    let got = owner(geo, lat, lon);
    assert_eq!(
        got.as_deref(),
        expected,
        "{what} ({lat}, {lon}): {}",
        wb_geo::query_line(geo, lat, lon, "").unwrap()
    );
}

/// Every nation holds the land around its seed point (`seed_at` in the world file).
#[test]
#[ignore = "needs build/packs"]
fn each_seed_lies_in_its_nation() {
    let g = geo();
    for (code, lon, lat) in [
        ("ald", 13.6, 56.0),
        ("vsm", 27.5, 57.2),
        ("tlv", 15.2, 63.4),
        ("hlv", 18.5, 57.45),
        ("kld", 16.0, 53.4),
        ("ost", 22.2, 53.7),
        ("mrv", 9.4, 56.0),
        ("cnd", 22.3, 60.4),
    ] {
        assert_owner(&g, &format!("seed of {code}"), lat, lon, Some(code));
    }
}

/// The island republic keeps its whole island, coast included.
#[test]
#[ignore = "needs build/packs"]
fn island_republic_holds_its_island() {
    let g = geo();
    assert_owner(&g, "south of the island", 57.2, 18.3, Some("hlv"));
    assert_owner(&g, "west coast of the island", 57.64, 18.29, Some("hlv"));
}

/// Due north of a straight stretch of Kaldoria's coast at 17.30°E, where the coast lies at
/// about 54.743°N: 17.5 km (9.4 nm) out is Kaldorian, 26.4 km (14.2 nm) out is open sea.
/// The belt is computed from the generated coastline cells (doc 11 §2.3).
#[test]
#[ignore = "needs build/packs"]
fn territorial_sea_ends_at_12_nm() {
    let g = geo();
    assert_owner(&g, "9.4 nm off the coast", 54.90, 17.30, Some("kld"));
    assert_owner(&g, "14.2 nm off the coast", 54.98, 17.30, None);
    // The world pack has the same belt at its coarser cells.
    let mut world = Geo::new();
    let root = std::env::var_os("WB_PACKS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../build/packs"));
    world
        .add_pack(Pack::open_dir(&root.join("world")).unwrap())
        .unwrap();
    assert_owner(
        &world,
        "9.4 nm off the coast (world pack)",
        54.90,
        17.30,
        Some("kld"),
    );
}

/// About 1 km either side of the Kaldoria–Ostrevia land border at 52.5°N.
#[test]
#[ignore = "needs build/packs"]
fn land_border_between_two_nations() {
    let g = geo();
    assert_owner(&g, "Kaldorian side", KLD_SIDE.0, KLD_SIDE.1, Some("kld"));
    assert_owner(&g, "Ostrevian side", OST_SIDE.0, OST_SIDE.1, Some("ost"));
    let coalition: BTreeSet<NationCode> = ["kld", "mrv"]
        .into_iter()
        .map(|c| NationCode::new(c).unwrap())
        .collect();
    assert!(
        g.is_friendly(LatLon::from_deg(KLD_SIDE.0, KLD_SIDE.1), &coalition)
            .unwrap()
    );
    assert!(
        !g.is_friendly(LatLon::from_deg(OST_SIDE.0, OST_SIDE.1), &coalition)
            .unwrap()
    );
}

const KLD_SIDE: (f64, f64) = (52.5, 19.475);
const OST_SIDE: (f64, f64) = (52.5, 19.505);

/// Land the eight nations do not reach is unclaimed (doc 11 §2.2), inside and outside
/// the theatre pack.
#[test]
#[ignore = "needs build/packs"]
fn land_beyond_the_nations_is_unclaimed() {
    let g = geo();
    assert_owner(&g, "far north-east", 66.5, 27.0, None);
    assert_owner(&g, "far south-west", 51.0, 4.0, None);
    assert_owner(&g, "outside the theatre window", 45.0, 34.0, None);
    assert_owner(&g, "another continent (world pack)", 38.9, -77.0, None);
}

/// A high ridge in the southern mountains, crest about 2,650 m. At 1,500 m above sea
/// level both ends clear the foothills (the northern slopes reach about 1,100 m), so what
/// blocks the path must be the main ridge; at 4,000 m the path is clear.
#[test]
#[ignore = "needs build/packs"]
fn ridge_blocks_a_low_path_but_not_a_high_one() {
    let g = geo();
    let params = LosParams {
        k_factor: 4.0 / 3.0,
        step_m: 100.0,
    };
    let north = LatLon::from_deg(49.30, 20.10);
    let south = LatLon::from_deg(49.06, 20.30);
    let low = g
        .line_of_sight(north, 1_500.0, south, 1_500.0, &params)
        .unwrap();
    assert!(!low.clear, "{low:?}");
    // The path first meets terrain on the ridge's north slope, and the crest stands far
    // above the ray (only this ridge rises over 2,000 m here).
    let ridge = low.obstruction.unwrap();
    assert!((49.12..49.25).contains(&ridge.at.lat_deg()), "{ridge:?}");
    assert!(low.min_clearance_m < -500.0, "{low:?}");
    let high = g
        .line_of_sight(north, 4_000.0, south, 4_000.0, &params)
        .unwrap();
    assert!(high.clear, "{high:?}");
}
