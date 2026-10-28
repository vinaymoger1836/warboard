//! Always-run tests on tiny packs built here, in the layout of doc 05 §2.2 (the same
//! layout `geodata/formats.py` writes). The tests against real packs are in `packs.rs`.

use std::collections::BTreeSet;
use wb_core::geo::LatLon;
use wb_geo::{BytesSource, CountryCode, Geo, LosParams, Pack, PackSource, TerrainClass};

const RES_ARCSEC: u16 = 15;
const SAMPLES: usize = 3600 / RES_ARCSEC as usize + 1;
const H3_RES: u8 = 7;

fn zstd(raw: &[u8]) -> Vec<u8> {
    ruzstd::encoding::compress_to_vec(raw, ruzstd::encoding::CompressionLevel::Fastest)
}

fn header(fields: &[u8]) -> Vec<u8> {
    let mut h = fields.to_vec();
    h.resize(32, 0);
    h
}

/// One 1° DEM tile at 54–55°N, 20–21°E: flat 10 m with a 600 m north–south ridge at 20.5°E.
fn dem() -> Vec<u8> {
    let mut grid = vec![10i16; SAMPLES * SAMPLES];
    for row in 0..SAMPLES {
        for col in 0..SAMPLES {
            let lon = 20.0 + col as f64 / (SAMPLES - 1) as f64;
            if (lon - 20.5).abs() < 0.01 {
                grid[row * SAMPLES + col] = 600;
            }
        }
    }
    let raw: Vec<u8> = grid.iter().flat_map(|v| v.to_le_bytes()).collect();
    let blob = zstd(&raw);
    let mut h = Vec::new();
    h.extend_from_slice(b"WBDM");
    for v in [1u16, RES_ARCSEC, SAMPLES as u16, 0] {
        h.extend_from_slice(&v.to_le_bytes());
    }
    h.extend_from_slice(&20i16.to_le_bytes());
    h.extend_from_slice(&54i16.to_le_bytes());
    h.extend_from_slice(&1u16.to_le_bytes());
    h.extend_from_slice(&1u16.to_le_bytes());
    let mut out = header(&h);
    out.extend_from_slice(&(32u64 + 16).to_le_bytes());
    out.extend_from_slice(&(blob.len() as u32).to_le_bytes());
    out.extend_from_slice(&10i16.to_le_bytes());
    out.extend_from_slice(&600i16.to_le_bytes());
    out.extend_from_slice(&blob);
    out
}

fn blocked(magic: &[u8; 4], codes: &[&str], cells: &[u64], block: Vec<u8>) -> Vec<u8> {
    let mut h = Vec::new();
    h.extend_from_slice(magic);
    h.extend_from_slice(&1u16.to_le_bytes());
    h.push(H3_RES);
    h.push(0);
    h.extend_from_slice(&(cells.len() as u64).to_le_bytes());
    h.extend_from_slice(&1u32.to_le_bytes());
    h.extend_from_slice(&4096u32.to_le_bytes());
    h.extend_from_slice(&(codes.len() as u16 + 1).to_le_bytes());
    let mut out = header(&h);
    out.extend_from_slice(&[0u8; 4]);
    for c in codes {
        let mut b = c.as_bytes().to_vec();
        b.resize(4, 0);
        out.extend_from_slice(&b);
    }
    let blob = zstd(&block);
    let data_at = out.len() as u64 + 24;
    out.extend_from_slice(&cells[0].to_le_bytes());
    out.extend_from_slice(&data_at.to_le_bytes());
    out.extend_from_slice(&(blob.len() as u32).to_le_bytes());
    out.extend_from_slice(&(cells.len() as u32).to_le_bytes());
    out.extend_from_slice(&blob);
    out
}

fn cell_at(lat: f64, lon: f64) -> u64 {
    let res = h3o::Resolution::try_from(H3_RES).unwrap();
    u64::from(h3o::LatLng::new(lat, lon).unwrap().to_cell(res))
}

/// Interior cell owned by `pol` (country 1) and a border cell split at `SPLIT_LON` between
/// `pol` (west) and `rus` (east).
const INTERIOR: (f64, f64) = (54.2, 20.2);
const BORDER: (f64, f64) = (54.6, 20.7);

fn terrain_and_territory() -> (Vec<u8>, Vec<u8>, f64) {
    let interior = cell_at(INTERIOR.0, INTERIOR.1);
    let border = cell_at(BORDER.0, BORDER.1);
    let mut cells = vec![
        (interior, 2u8, 50i16, 3u8, 0u8, 1u16),
        (border, 1, 20, 1, 2, 1),
    ];
    cells.sort();
    let mut block = Vec::new();
    for c in &cells {
        block.extend_from_slice(&c.0.to_le_bytes());
    }
    block.extend(cells.iter().map(|c| c.1));
    for c in &cells {
        block.extend_from_slice(&c.2.to_le_bytes());
    }
    block.extend(cells.iter().map(|c| c.3));
    block.extend(cells.iter().map(|c| c.4));
    for c in &cells {
        block.extend_from_slice(&c.5.to_le_bytes());
    }
    let ids: Vec<u64> = cells.iter().map(|c| c.0).collect();
    let terrain = blocked(b"WBH3", &["pol", "rus"], &ids, block);

    // The border cell: west of SPLIT_LON is pol, east is rus (rectangles around the cell).
    let split = BORDER.1;
    let e7 = |v: f64| (v * 1e7).round() as i32;
    let rect = |x0: f64, x1: f64| -> Vec<u8> {
        let pts = [(x0, 54.4), (x1, 54.4), (x1, 54.8), (x0, 54.8)];
        let mut r = (pts.len() as u32).to_le_bytes().to_vec();
        for (x, y) in pts {
            r.extend_from_slice(&e7(x).to_le_bytes());
            r.extend_from_slice(&e7(y).to_le_bytes());
        }
        r
    };
    let mut record = Vec::new();
    record.extend_from_slice(&2u16.to_le_bytes());
    record.extend_from_slice(&0u16.to_le_bytes());
    for (country, x0, x1) in [(1u16, split - 0.2, split), (2u16, split, split + 0.2)] {
        record.extend_from_slice(&country.to_le_bytes());
        record.extend_from_slice(&1u16.to_le_bytes());
        record.extend_from_slice(&rect(x0, x1));
    }
    let mut tblock = border.to_le_bytes().to_vec();
    tblock.extend_from_slice(&0u32.to_le_bytes());
    tblock.extend_from_slice(&record);
    let territory = blocked(b"WBTR", &["pol", "rus"], &[border], tblock);
    (terrain, territory, split)
}

fn geo() -> Geo {
    let (terrain, territory, _) = terrain_and_territory();
    let src =
        |name: &str, b: Vec<u8>| -> Box<dyn PackSource> { Box::new(BytesSource::new(name, b)) };
    let pack = Pack::new(
        "test",
        src("dem", dem()),
        src("terrain", terrain),
        src("territory", territory),
    )
    .unwrap();
    let mut geo = Geo::new();
    geo.add_pack(pack).unwrap();
    geo
}

fn code(s: &str) -> CountryCode {
    CountryCode::new(s).unwrap()
}

#[test]
fn elevation_is_bilinear_on_the_grid() {
    let g = geo();
    assert_eq!(g.elevation_m(LatLon::from_deg(54.3, 20.2)).unwrap(), 10.0);
    assert_eq!(g.elevation_m(LatLon::from_deg(54.3, 20.5)).unwrap(), 600.0);
    let between = g
        .elevation_m(LatLon::from_deg(54.3, 20.5 + 0.5 * 0.01 / 3.0))
        .unwrap();
    assert!((10.0..=600.0).contains(&between), "{between}");
    // Outside every pack: sea level.
    assert_eq!(g.elevation_m(LatLon::from_deg(10.0, 10.0)).unwrap(), 0.0);
}

#[test]
fn ridge_blocks_a_low_path_but_not_a_high_one() {
    let g = geo();
    let params = LosParams {
        k_factor: 4.0 / 3.0,
        step_m: 100.0,
    };
    let (a, b) = (LatLon::from_deg(54.5, 20.2), LatLon::from_deg(54.5, 20.8));
    let low = g.line_of_sight(a, 50.0, b, 50.0, &params).unwrap();
    assert!(!low.clear);
    let hit = low.obstruction.unwrap();
    assert!(
        (hit.at.lon_deg() - 20.5).abs() < 0.02,
        "{}",
        hit.at.lon_deg()
    );
    let high = g.line_of_sight(a, 900.0, b, 900.0, &params).unwrap();
    assert!(high.clear, "{high:?}");
    assert!(high.min_clearance_m > 250.0);
}

#[test]
fn curvature_hides_a_low_target_beyond_the_horizon() {
    let g = geo();
    let params = LosParams {
        k_factor: 4.0 / 3.0,
        step_m: 500.0,
    };
    // 400 km over sea at 20 m: far below the 4/3-Earth horizon (≈ 2·18 km).
    let (a, b) = (LatLon::from_deg(40.0, 0.0), LatLon::from_deg(40.0, 4.7));
    assert!(!g.line_of_sight(a, 20.0, b, 20.0, &params).unwrap().clear);
    assert!(
        g.line_of_sight(a, 20.0, b, 12_000.0, &params)
            .unwrap()
            .clear
    );
}

#[test]
fn territory_uses_cell_owner_or_exact_rings() {
    let g = geo();
    let (_, _, split) = terrain_and_territory();
    assert_eq!(
        g.territory_owner(LatLon::from_deg(INTERIOR.0, INTERIOR.1))
            .unwrap(),
        Some(code("pol"))
    );
    let west = LatLon::from_deg(BORDER.0, split - 0.001);
    let east = LatLon::from_deg(BORDER.0, split + 0.001);
    assert_eq!(g.territory_owner(west).unwrap(), Some(code("pol")));
    assert_eq!(g.territory_owner(east).unwrap(), Some(code("rus")));
    // A cell the file does not hold is open sea.
    assert_eq!(
        g.territory_owner(LatLon::from_deg(54.9, 20.9)).unwrap(),
        None
    );

    let coalition: BTreeSet<_> = [code("pol"), code("ltu")].into();
    assert!(g.is_friendly(west, &coalition).unwrap());
    assert!(!g.is_friendly(east, &coalition).unwrap());
}

#[test]
fn terrain_class_comes_from_the_cell() {
    let g = geo();
    assert_eq!(
        g.terrain_class(LatLon::from_deg(INTERIOR.0, INTERIOR.1))
            .unwrap(),
        TerrainClass::Forest
    );
    assert_eq!(
        g.terrain_class(LatLon::from_deg(54.9, 20.9)).unwrap(),
        TerrainClass::Water
    );
    let info = g
        .cell_info(LatLon::from_deg(INTERIOR.0, INTERIOR.1))
        .unwrap()
        .unwrap();
    assert_eq!((info.elevation_m, info.slope_deg), (50, 1.5));
}
