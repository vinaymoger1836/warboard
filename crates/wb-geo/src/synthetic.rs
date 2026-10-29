//! Small packs built in memory, in the on-disk layout of doc 05 §2.2: test
//! terrain for golden scenarios and tests that must run without the real
//! packs (CI has none). Scenarios use it through their `test_terrain` block.
//!
//! The DEM is flat sea-level-or-`base_m` ground with straight ridges of a
//! triangular profile. The terrain and territory files hold one open cell in
//! the south-west corner, so every other point is open water without an owner.

use crate::source::{BytesSource, PackSource};
use crate::{GeoError, Pack};
use wb_core::geo::LatLon;
use wb_core::math::cos;
use wb_core::units::deg_to_rad;

/// Sample spacing of the synthetic DEM, arc seconds (doc 05 §4 theatre grid).
pub const RES_ARCSEC: u16 = 15;
const SAMPLES: usize = 3600 / RES_ARCSEC as usize + 1;
const H3_RES: u8 = 7;
/// Largest zstd block (128 KiB).
const MAX_BLOCK: usize = 128 * 1024;

/// A straight ridge from `from` to `to` (degrees), `height_m` above the base
/// along its crest and falling linearly to the base `half_width_m` either side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ridge {
    pub from: LatLon,
    pub to: LatLon,
    pub height_m: f64,
    pub half_width_m: f64,
}

/// A zstd frame made of raw (stored) blocks: valid zstd without an encoder.
fn zstd_raw(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 16);
    out.extend_from_slice(&0xFD2F_B528u32.to_le_bytes());
    // Single segment, 4-byte frame content size, no checksum, no dictionary.
    out.push(0b1010_0000);
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    let chunks: Vec<&[u8]> = if data.is_empty() {
        vec![&[]]
    } else {
        data.chunks(MAX_BLOCK).collect()
    };
    for (i, c) in chunks.iter().enumerate() {
        let last = u32::from(i + 1 == chunks.len());
        let header = last | ((c.len() as u32) << 3);
        out.extend_from_slice(&header.to_le_bytes()[..3]);
        out.extend_from_slice(c);
    }
    out
}

/// Distance in metres from `p` to the segment `a`–`b` on a local flat
/// approximation (good to well under 1% across a 1° tile).
fn segment_distance_m(p: LatLon, a: LatLon, b: LatLon) -> f64 {
    const M_PER_RAD: f64 = 6_371_008.8;
    let k = cos(p.lat);
    let xy = |q: LatLon| ((q.lon - p.lon) * k * M_PER_RAD, (q.lat - p.lat) * M_PER_RAD);
    let (ax, ay) = xy(a);
    let (bx, by) = xy(b);
    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 {
        (-(ax * dx + ay * dy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (cx, cy) = (ax + t * dx, ay + t * dy);
    wb_core::math::sqrt(cx * cx + cy * cy)
}

fn dem(west: i16, south: i16, cols: u16, rows: u16, base_m: i16, ridges: &[Ridge]) -> Vec<u8> {
    let n = usize::from(cols) * usize::from(rows);
    let mut blobs = Vec::with_capacity(n);
    for row in 0..rows {
        for col in 0..cols {
            let (lon0, lat0) = (f64::from(west + col as i16), f64::from(south + row as i16));
            let mut grid = vec![base_m; SAMPLES * SAMPLES];
            for i in 0..SAMPLES {
                for j in 0..SAMPLES {
                    let p = LatLon::from_deg(
                        lat0 + i as f64 / (SAMPLES - 1) as f64,
                        lon0 + j as f64 / (SAMPLES - 1) as f64,
                    );
                    let mut z = f64::from(base_m);
                    for r in ridges {
                        let d = segment_distance_m(p, r.from, r.to);
                        let h = r.height_m * (1.0 - d / r.half_width_m.max(1.0)).max(0.0);
                        z = z.max(f64::from(base_m) + h);
                    }
                    grid[i * SAMPLES + j] = z.round().clamp(-32_000.0, 32_000.0) as i16;
                }
            }
            let (lo, hi) = grid
                .iter()
                .fold((i16::MAX, i16::MIN), |(a, b), &v| (a.min(v), b.max(v)));
            let raw: Vec<u8> = grid.iter().flat_map(|v| v.to_le_bytes()).collect();
            blobs.push((zstd_raw(&raw), lo, hi));
        }
    }
    let mut out = Vec::new();
    out.extend_from_slice(b"WBDM");
    for v in [1u16, RES_ARCSEC, SAMPLES as u16, 0] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&west.to_le_bytes());
    out.extend_from_slice(&south.to_le_bytes());
    out.extend_from_slice(&cols.to_le_bytes());
    out.extend_from_slice(&rows.to_le_bytes());
    out.resize(32, 0);
    let mut offset = 32 + 16 * n as u64;
    for (blob, lo, hi) in &blobs {
        out.extend_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(&(blob.len() as u32).to_le_bytes());
        out.extend_from_slice(&lo.to_le_bytes());
        out.extend_from_slice(&hi.to_le_bytes());
        offset += blob.len() as u64;
    }
    for (blob, _, _) in &blobs {
        out.extend_from_slice(blob);
    }
    out
}

/// A one-block `.wbh3` / `.bin` file with no countries.
fn blocked(magic: &[u8; 4], cell: u64, block: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(magic);
    out.extend_from_slice(&1u16.to_le_bytes());
    out.push(H3_RES);
    out.push(0);
    out.extend_from_slice(&1u64.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&4096u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.resize(32, 0);
    out.extend_from_slice(&[0u8; 4]);
    let blob = zstd_raw(block);
    let data_at = out.len() as u64 + 24;
    out.extend_from_slice(&cell.to_le_bytes());
    out.extend_from_slice(&data_at.to_le_bytes());
    out.extend_from_slice(&(blob.len() as u32).to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&blob);
    out
}

/// A pack covering `cols × rows` 1° tiles from (`west`, `south`), degrees.
pub fn ridge_pack(
    id: &str,
    west: i16,
    south: i16,
    cols: u16,
    rows: u16,
    base_m: i16,
    ridges: &[Ridge],
) -> Result<Pack, GeoError> {
    if cols == 0 || rows == 0 || cols > 4 || rows > 4 {
        return Err(GeoError::Pack(
            "synthetic terrain covers 1–4 tiles each way".into(),
        ));
    }
    let corner = h3o::LatLng::from_radians(
        deg_to_rad(f64::from(south) + 0.01),
        deg_to_rad(f64::from(west) + 0.01),
    )
    .map_err(|e| GeoError::Pack(e.to_string()))?;
    let res = h3o::Resolution::try_from(H3_RES).map_err(|e| GeoError::Pack(e.to_string()))?;
    let cell = u64::from(corner.to_cell(res));
    // cell, class (open), elevation, slope, flags, country.
    let mut terrain = cell.to_le_bytes().to_vec();
    terrain.push(1);
    terrain.extend_from_slice(&base_m.to_le_bytes());
    terrain.push(0);
    terrain.push(0);
    terrain.extend_from_slice(&0u16.to_le_bytes());
    let mut territory = cell.to_le_bytes().to_vec();
    territory.extend_from_slice(&0u32.to_le_bytes());
    territory.extend_from_slice(&[0u8; 4]);
    let src = |name: &str, b: Vec<u8>| -> Box<dyn PackSource> {
        Box::new(BytesSource::new(format!("{id}/{name}"), b))
    };
    Pack::new(
        id,
        src("dem", dem(west, south, cols, rows, base_m, ridges)),
        src("terrain", blocked(b"WBH3", cell, &terrain)),
        src("territory", blocked(b"WBTR", cell, &territory)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Geo, LosParams};

    #[test]
    fn ridge_rises_and_blocks() {
        let ridge = Ridge {
            from: LatLon::from_deg(54.0, 20.5),
            to: LatLon::from_deg(55.0, 20.5),
            height_m: 600.0,
            half_width_m: 1_000.0,
        };
        let mut geo = Geo::new();
        geo.add_pack(ridge_pack("t", 20, 54, 1, 1, 10, &[ridge]).unwrap())
            .unwrap();
        let top = geo.elevation_m(LatLon::from_deg(54.5, 20.5)).unwrap();
        assert!((590.0..=610.0).contains(&top), "{top}");
        assert_eq!(geo.elevation_m(LatLon::from_deg(54.5, 20.2)).unwrap(), 10.0);
        let p = LosParams {
            k_factor: 4.0 / 3.0,
            step_m: 100.0,
        };
        let (a, b) = (LatLon::from_deg(54.5, 20.2), LatLon::from_deg(54.5, 20.8));
        assert!(!geo.line_of_sight(a, 30.0, b, 100.0, &p).unwrap().clear);
        assert!(geo.line_of_sight(a, 30.0, b, 3_000.0, &p).unwrap().clear);
        assert_eq!(
            geo.terrain_class(LatLon::from_deg(54.5, 20.5)).unwrap(),
            crate::TerrainClass::Water
        );
    }
}
