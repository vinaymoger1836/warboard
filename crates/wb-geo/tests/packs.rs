//! P2 acceptance tests against the real packs in `build/packs/` (doc 08, P2).
//! Packs are build output and not in git (CLAUDE.md rule 9), so these are ignored by
//! default. Build the packs (`bash geodata/run.sh build world`, `… build europe`), then:
//!
//!     cargo test -p wb-geo --test packs -- --ignored
//!
//! `WB_PACKS` overrides the folder holding `world/` and `europe/`.

use std::collections::BTreeSet;
use std::path::PathBuf;
use wb_core::geo::LatLon;
use wb_geo::{CountryCode, Geo, LosParams, Pack};

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

#[test]
#[ignore = "needs build/packs"]
fn kaliningrad_is_russian() {
    let g = geo();
    assert_owner(&g, "Kaliningrad city", 54.7104, 20.4522, Some("rus"));
    assert_owner(&g, "Chkalovsk air base", 54.7667, 20.4, Some("rus"));
    // Vistula Spit, both sides of the Poland–Russia border (19.63°E).
    assert_owner(&g, "Vistula Spit, Polish side", 54.42, 19.58, Some("pol"));
    assert_owner(&g, "Vistula Spit, Russian side", 54.47, 19.72, Some("rus"));
}

#[test]
#[ignore = "needs build/packs"]
fn gotland_is_swedish() {
    let g = geo();
    assert_owner(&g, "Visby", 57.6348, 18.2948, Some("swe"));
    assert_owner(&g, "inland Gotland", 57.45, 18.55, Some("swe"));
}

/// Due north of a straight stretch of the Polish coast west of Łeba. Measured against
/// the Natural Earth coastline in P2: 9.98 nm and 13.94 nm (the Polish baseline here is
/// the low-water line, so the 12 nm limit follows the coast).
#[test]
#[ignore = "needs build/packs"]
fn polish_territorial_sea_ends_at_12_nm() {
    let g = geo();
    assert_owner(
        &g,
        "10 nm off the Polish coast",
        POLISH_10NM.0,
        POLISH_10NM.1,
        Some("pol"),
    );
    assert_owner(
        &g,
        "14 nm off the Polish coast",
        POLISH_14NM.0,
        POLISH_14NM.1,
        None,
    );
}

const POLISH_10NM: (f64, f64) = (54.914, 17.30);
const POLISH_14NM: (f64, f64) = (54.981, 17.30);

#[test]
#[ignore = "needs build/packs"]
fn finland_russia_border() {
    let g = geo();
    // 1 km either side of the land border north of Vyborg, perpendicular to the border
    // at its Natural Earth vertex 60.8594°N 28.3359°E.
    assert_owner(&g, "Finnish side", FIN_SIDE.0, FIN_SIDE.1, Some("fin"));
    assert_owner(&g, "Russian side", RUS_SIDE.0, RUS_SIDE.1, Some("rus"));
    let nato: BTreeSet<CountryCode> = ["fin", "pol", "swe"]
        .into_iter()
        .map(|c| CountryCode::new(c).unwrap())
        .collect();
    assert!(
        g.is_friendly(LatLon::from_deg(FIN_SIDE.0, FIN_SIDE.1), &nato)
            .unwrap()
    );
    assert!(
        !g.is_friendly(LatLon::from_deg(RUS_SIDE.0, RUS_SIDE.1), &nato)
            .unwrap()
    );
}

const FIN_SIDE: (f64, f64) = (60.8643, 28.3205);
const RUS_SIDE: (f64, f64) = (60.8544, 28.3513);

/// The High Tatras (Gerlachovský štít, 2,655 m) between Zakopane and Poprad. At 1,500 m
/// above sea level both ends clear the foothills (Zakopane's slopes reach about 1,100 m),
/// so what blocks the path must be the main ridge; at 4,000 m the path is clear.
#[test]
#[ignore = "needs build/packs"]
fn tatra_ridge_blocks_a_low_path_but_not_a_high_one() {
    let g = geo();
    let params = LosParams {
        k_factor: 4.0 / 3.0,
        step_m: 100.0,
    };
    let zakopane = LatLon::from_deg(49.30, 20.10);
    let poprad = LatLon::from_deg(49.06, 20.30);
    let low = g
        .line_of_sight(zakopane, 1_500.0, poprad, 1_500.0, &params)
        .unwrap();
    assert!(!low.clear, "{low:?}");
    // The path first meets terrain on the ridge's north slope, and the crest stands far
    // above the ray (only the High Tatras rise over 2,000 m here).
    let ridge = low.obstruction.unwrap();
    assert!((49.12..49.25).contains(&ridge.at.lat_deg()), "{ridge:?}");
    assert!(low.min_clearance_m < -500.0, "{low:?}");
    let high = g
        .line_of_sight(zakopane, 4_000.0, poprad, 4_000.0, &params)
        .unwrap();
    assert!(high.clear, "{high:?}");
}
