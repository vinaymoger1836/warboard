//! Territory lookup, H3 terrain classes, DEM sampling and line of sight (doc 05 §3–§5).
//!
//! A [`Geo`] holds the installed region packs. Every query uses the finest pack whose
//! extent contains the point, so results depend only on which packs are loaded (saves
//! record the pack hashes). All maths is deterministic: positions go through `wb-core`
//! and `h3o` (both on `libm`), ring tests are integer-only, and caches never change a
//! result (CLAUDE.md rule 2).

mod cells;
mod codec;
mod dem;
pub mod source;

pub use cells::{CellInfo, TerrainFile, TerritoryFile};
pub use dem::Dem;
pub use source::{BytesSource, PackSource};

#[cfg(not(target_arch = "wasm32"))]
pub use source::FileSource;

use std::collections::BTreeSet;
use std::fmt;
use wb_core::geo::{EARTH_RADIUS_M, LatLon, distance_m, intermediate_point};
use wb_core::units::rad_to_deg;

#[derive(Debug, thiserror::Error)]
pub enum GeoError {
    #[error("{name}: {message}")]
    Io { name: String, message: String },
    #[error("{name}: {message}")]
    Format { name: String, message: String },
    #[error("{0}")]
    Pack(String),
}

impl GeoError {
    pub fn io(name: &str, err: impl fmt::Display) -> Self {
        Self::Io {
            name: name.to_owned(),
            message: err.to_string(),
        }
    }

    pub fn format(name: &str, message: impl fmt::Display) -> Self {
        Self::Format {
            name: name.to_owned(),
            message: message.to_string(),
        }
    }
}

/// Country code as used in content (`pol`, `rus`): lower-case ISO 3166-1 alpha-3, or the
/// Natural Earth code for units without one (`kos`). At most four ASCII bytes.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CountryCode([u8; 4]);

impl CountryCode {
    pub fn new(code: &str) -> Result<Self, String> {
        Self::from_bytes(code.as_bytes())
    }

    fn from_bytes(raw: &[u8]) -> Result<Self, String> {
        let trimmed: Vec<u8> = raw.iter().copied().take_while(|&b| b != 0).collect();
        if trimmed.is_empty()
            || trimmed.len() > 4
            || !trimmed.iter().all(|b| b.is_ascii_lowercase())
        {
            return Err(format!(
                "invalid country code {:?}",
                String::from_utf8_lossy(raw)
            ));
        }
        let mut out = [0u8; 4];
        out[..trimmed.len()].copy_from_slice(&trimmed);
        Ok(Self(out))
    }

    pub fn as_str(&self) -> &str {
        let len = self.0.iter().position(|&b| b == 0).unwrap_or(4);
        std::str::from_utf8(&self.0[..len]).unwrap_or("?")
    }
}

impl fmt::Display for CountryCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Debug for CountryCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CountryCode({})", self.as_str())
    }
}

/// Terrain classes (doc 05 §3). The numbers are the on-disk codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum TerrainClass {
    Water = 0,
    Open = 1,
    Forest = 2,
    Jungle = 3,
    Urban = 4,
    Desert = 5,
    Mountain = 6,
    Marsh = 7,
    Arctic = 8,
}

impl TerrainClass {
    pub const ALL: [TerrainClass; 9] = [
        Self::Water,
        Self::Open,
        Self::Forest,
        Self::Jungle,
        Self::Urban,
        Self::Desert,
        Self::Mountain,
        Self::Marsh,
        Self::Arctic,
    ];

    pub fn from_u8(v: u8) -> Option<Self> {
        Self::ALL.get(usize::from(v)).copied()
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Water => "water",
            Self::Open => "open",
            Self::Forest => "forest",
            Self::Jungle => "jungle",
            Self::Urban => "urban",
            Self::Desert => "desert",
            Self::Mountain => "mountain",
            Self::Marsh => "marsh",
            Self::Arctic => "arctic",
        }
    }
}

/// One installed region pack: DEM, H3 terrain and territory.
pub struct Pack {
    id: String,
    dem: Dem,
    terrain: TerrainFile,
    territory: TerritoryFile,
    resolution: h3o::Resolution,
}

impl Pack {
    pub fn new(
        id: &str,
        dem: Box<dyn PackSource>,
        terrain: Box<dyn PackSource>,
        territory: Box<dyn PackSource>,
    ) -> Result<Self, GeoError> {
        let dem = Dem::open(dem)?;
        let terrain = TerrainFile::open(terrain)?;
        let territory = TerritoryFile::open(territory)?;
        if terrain.res() != territory.res() {
            return Err(GeoError::Pack(format!(
                "pack {id}: terrain res {} but territory res {}",
                terrain.res(),
                territory.res()
            )));
        }
        let resolution = h3o::Resolution::try_from(terrain.res())
            .map_err(|e| GeoError::Pack(format!("pack {id}: {e}")))?;
        Ok(Self {
            id: id.to_owned(),
            dem,
            terrain,
            territory,
            resolution,
        })
    }

    /// Opens `dem-<id>.wbdem`, `terrain-<id>.wbh3` and `territory-<id>.bin` from a pack
    /// folder whose name is the pack id.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_dir(dir: &std::path::Path) -> Result<Self, GeoError> {
        let id = dir
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| GeoError::Pack(format!("{}: not a pack folder", dir.display())))?;
        let open = |name: String| -> Result<Box<dyn PackSource>, GeoError> {
            Ok(Box::new(FileSource::open(&dir.join(name))?))
        };
        Self::new(
            id,
            open(format!("dem-{id}.wbdem"))?,
            open(format!("terrain-{id}.wbh3"))?,
            open(format!("territory-{id}.bin"))?,
        )
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn extent_deg(&self) -> [f64; 4] {
        self.dem.extent_deg()
    }

    pub fn dem(&self) -> &Dem {
        &self.dem
    }

    pub fn terrain(&self) -> &TerrainFile {
        &self.terrain
    }

    pub fn territory(&self) -> &TerritoryFile {
        &self.territory
    }

    pub fn h3_res(&self) -> u8 {
        self.terrain.res()
    }

    fn contains(&self, lat_deg: f64, lon_deg: f64) -> bool {
        let [w, s, e, n] = self.extent_deg();
        (w..=e).contains(&lon_deg) && (s..=n).contains(&lat_deg)
    }

    fn cell(&self, p: LatLon) -> Result<u64, GeoError> {
        let ll = h3o::LatLng::from_radians(p.lat, p.lon)
            .map_err(|e| GeoError::Pack(format!("invalid position: {e}")))?;
        Ok(u64::from(ll.to_cell(self.resolution)))
    }
}

/// Line-of-sight settings; the values come from the realism profile (`earth`, `radar`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LosParams {
    /// Effective Earth radius factor; 4/3 for radio paths.
    pub k_factor: f64,
    /// Sample spacing along the path, metres.
    pub step_m: f64,
}

/// Most samples taken along one path; the step grows for very long paths.
pub const LOS_MAX_SAMPLES: u32 = 20_000;

/// Where terrain first blocks a path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Obstruction {
    pub at: LatLon,
    /// Distance from the first endpoint, metres.
    pub distance_m: f64,
    /// Terrain plus Earth bulge at that point, metres above sea level.
    pub terrain_m: f64,
    /// Height of the line of sight there, metres above sea level.
    pub ray_m: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Los {
    pub clear: bool,
    /// Smallest gap between the ray and terrain plus bulge, metres (negative when blocked).
    pub min_clearance_m: f64,
    /// First blocking point, nearest the first endpoint.
    pub obstruction: Option<Obstruction>,
    pub samples: u32,
}

/// Everything a point query returns, for tools and the UI.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointInfo {
    pub pack: usize,
    pub cell: Option<CellInfo>,
    pub class: TerrainClass,
    pub elevation_m: f64,
    pub owner: Option<CountryCode>,
}

/// The installed packs, finest first.
#[derive(Default)]
pub struct Geo {
    packs: Vec<Pack>,
}

impl Geo {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a pack; packs are kept finest (highest H3 resolution) first, then by id.
    pub fn add_pack(&mut self, pack: Pack) -> Result<(), GeoError> {
        if self.packs.iter().any(|p| p.id == pack.id) {
            return Err(GeoError::Pack(format!(
                "pack {} is already loaded",
                pack.id
            )));
        }
        self.packs.push(pack);
        self.packs
            .sort_by(|a, b| b.h3_res().cmp(&a.h3_res()).then_with(|| a.id.cmp(&b.id)));
        Ok(())
    }

    pub fn packs(&self) -> &[Pack] {
        &self.packs
    }

    /// Index of the finest pack covering the point.
    pub fn pack_at(&self, p: LatLon) -> Option<usize> {
        let (lat, lon) = (rad_to_deg(p.lat), rad_to_deg(p.lon));
        self.packs.iter().position(|pk| pk.contains(lat, lon))
    }

    /// Ground elevation, metres above sea level; 0 where no pack has data.
    pub fn elevation_m(&self, p: LatLon) -> Result<f64, GeoError> {
        let (lat, lon) = (rad_to_deg(p.lat), rad_to_deg(p.lon));
        for pk in &self.packs {
            if let Some(z) = pk.dem.elevation_deg(lat, lon)? {
                return Ok(z);
            }
        }
        Ok(0.0)
    }

    /// The terrain cell under the point; `None` for open sea or outside every pack.
    pub fn cell_info(&self, p: LatLon) -> Result<Option<CellInfo>, GeoError> {
        let Some(i) = self.pack_at(p) else {
            return Ok(None);
        };
        let pk = &self.packs[i];
        pk.terrain.lookup(pk.cell(p)?)
    }

    pub fn terrain_class(&self, p: LatLon) -> Result<TerrainClass, GeoError> {
        Ok(self.cell_info(p)?.map_or(TerrainClass::Water, |c| c.class))
    }

    /// Country whose land, 12 nm sea or airspace contains the point (doc 05 §5).
    pub fn territory_owner(&self, p: LatLon) -> Result<Option<CountryCode>, GeoError> {
        let Some(i) = self.pack_at(p) else {
            return Ok(None);
        };
        let pk = &self.packs[i];
        let cell = pk.cell(p)?;
        let Some(info) = pk.terrain.lookup(cell)? else {
            return Ok(None);
        };
        if !info.is_border() {
            return Ok(info.country);
        }
        let x = (rad_to_deg(p.lon) * cells::COORD_SCALE).round() as i64;
        let y = (rad_to_deg(p.lat) * cells::COORD_SCALE).round() as i64;
        Ok(pk.territory.owner(cell, x, y)?.unwrap_or(info.country))
    }

    /// Whether the point is territory of a coalition member. Neutral and unowned
    /// territory is not friendly.
    pub fn is_friendly(
        &self,
        p: LatLon,
        coalition: &BTreeSet<CountryCode>,
    ) -> Result<bool, GeoError> {
        Ok(self
            .territory_owner(p)?
            .is_some_and(|c| coalition.contains(&c)))
    }

    pub fn point_info(&self, p: LatLon) -> Result<Option<PointInfo>, GeoError> {
        let Some(pack) = self.pack_at(p) else {
            return Ok(None);
        };
        let cell = self.cell_info(p)?;
        Ok(Some(PointInfo {
            pack,
            cell,
            class: cell.map_or(TerrainClass::Water, |c| c.class),
            elevation_m: self.elevation_m(p)?,
            owner: self.territory_owner(p)?,
        }))
    }

    /// Terrain line of sight between two points at altitudes above sea level (doc 05 §4).
    ///
    /// Samples the great circle; at each sample the straight ray (linear in height) must
    /// clear terrain plus the Earth bulge `d1·d2 / (2kR)` of a sphere with radius kR.
    pub fn line_of_sight(
        &self,
        a: LatLon,
        alt_a_m: f64,
        b: LatLon,
        alt_b_m: f64,
        params: &LosParams,
    ) -> Result<Los, GeoError> {
        let d = distance_m(a, b);
        let step = params.step_m.max(1.0);
        let n = ((d / step).ceil() as u32).clamp(1, LOS_MAX_SAMPLES);
        let reff = params.k_factor * EARTH_RADIUS_M;
        let mut min_clearance = f64::INFINITY;
        let mut obstruction = None;
        for i in 1..n {
            let t = f64::from(i) / f64::from(n);
            let at = intermediate_point(a, b, t);
            let (d1, d2) = (t * d, (1.0 - t) * d);
            let terrain = self.elevation_m(at)? + d1 * d2 / (2.0 * reff);
            let ray = alt_a_m + (alt_b_m - alt_a_m) * t;
            let clearance = ray - terrain;
            if clearance < min_clearance {
                min_clearance = clearance;
            }
            if clearance < 0.0 && obstruction.is_none() {
                obstruction = Some(Obstruction {
                    at,
                    distance_m: d1,
                    terrain_m: terrain,
                    ray_m: ray,
                });
            }
        }
        Ok(Los {
            clear: obstruction.is_none(),
            min_clearance_m: if min_clearance.is_finite() {
                min_clearance
            } else {
                0.0
            },
            obstruction,
            samples: n.saturating_sub(1),
        })
    }

    /// Cells of the finest pack covering the box centre, for the terrain overlay
    /// (UI only). Returns `None` if the box would hold more than `max_cells` cells.
    pub fn cells_in_box(
        &self,
        west: f64,
        south: f64,
        east: f64,
        north: f64,
        max_cells: usize,
    ) -> Result<Option<Vec<OverlayCell>>, GeoError> {
        let (clat, clon) = ((south + north) * 0.5, (west + east) * 0.5);
        let lat_span = (north - south).max(0.0);
        let mid_cos = wb_core::math::cos(wb_core::units::deg_to_rad(clat)).max(0.05);
        // Finest pack covering the centre whose cells fit; coarser packs when zoomed out.
        let mut covering = self
            .packs
            .iter()
            .filter(|p| p.contains(clat, clon))
            .peekable();
        if covering.peek().is_none() {
            return Ok(Some(Vec::new()));
        }
        let Some((pk, edge_deg)) = covering
            .map(|p| (p, p.resolution.edge_length_km() / 111.32))
            .find(|(_, edge)| {
                // A hexagon covers about 2.6 edge² (3√3/2).
                let est = (lat_span / edge) * ((east - west).max(0.0) * mid_cos / edge) / 2.6;
                est <= max_cells as f64
            })
        else {
            return Ok(None);
        };
        let step = edge_deg * 0.5;
        let mut seen = BTreeSet::new();
        let mut lat = south;
        while lat <= north {
            let lon_step = step / wb_core::math::cos(wb_core::units::deg_to_rad(lat)).max(0.05);
            let mut lon = west;
            while lon <= east {
                seen.insert(pk.cell(LatLon::from_deg(lat, lon))?);
                lon += lon_step;
            }
            lat += step;
        }
        let mut out = Vec::new();
        for cell in seen {
            if let Some(info) = pk.terrain.lookup(cell)? {
                let idx =
                    h3o::CellIndex::try_from(cell).map_err(|e| GeoError::Pack(e.to_string()))?;
                let boundary = idx
                    .boundary()
                    .iter()
                    .map(|ll| [ll.lng(), ll.lat()])
                    .collect();
                out.push(OverlayCell {
                    class: info.class,
                    boundary,
                });
                if out.len() > max_cells {
                    return Ok(None);
                }
            }
        }
        Ok(Some(out))
    }
}

/// A cell outline for the map overlay, `[lon, lat]` degrees.
#[derive(Clone, Debug, PartialEq)]
pub struct OverlayCell {
    pub class: TerrainClass,
    pub boundary: Vec<[f64; 2]>,
}

/// One line describing a point:
/// `lat lon owner=… class=… elev=… cell=… pack=… flags=… [label]`.
/// Shared by `wb geo query` and the WebAssembly build, so their outputs compare byte for byte.
pub fn query_line(geo: &Geo, lat_deg: f64, lon_deg: f64, label: &str) -> Result<String, GeoError> {
    let p = LatLon::from_deg(lat_deg, lon_deg);
    let line = match geo.point_info(p)? {
        None => format!("{lat_deg:.6} {lon_deg:.6} outside every pack {label}"),
        Some(info) => {
            let (cell, flags) = info.cell.map_or((String::from("-"), 0), |c| {
                (format!("{:x}", c.cell), c.flags)
            });
            format!(
                "{lat_deg:.6} {lon_deg:.6} owner={} class={} elev={:.3} cell={cell} pack={} flags={flags} {label}",
                info.owner.map_or_else(|| "-".to_owned(), |c| c.to_string()),
                info.class.as_str(),
                info.elevation_m,
                geo.packs()[info.pack].id(),
            )
        }
    };
    Ok(line.trim_end().to_owned())
}

/// One line describing a line-of-sight result (shared like [`query_line`]).
pub fn los_line(los: &Los) -> String {
    match los.obstruction {
        Some(o) => format!(
            "blocked min_clearance={:.1} first_obstruction={:.5},{:.5} at {:.0} m (terrain+bulge {:.1} m, ray {:.1} m) samples={}",
            los.min_clearance_m,
            o.at.lat_deg(),
            o.at.lon_deg(),
            o.distance_m,
            o.terrain_m,
            o.ray_m,
            los.samples
        ),
        None => format!(
            "clear min_clearance={:.1} samples={}",
            los.min_clearance_m, los.samples
        ),
    }
}

/// One line per pack: extent, resolutions and counts.
pub fn info_lines(geo: &Geo) -> Vec<String> {
    geo.packs()
        .iter()
        .map(|p| {
            let [w, s, e, n] = p.extent_deg();
            format!(
                "{}: extent {w},{s},{e},{n}; DEM {}″, {} tiles, max {} m; H3 res {}, {} cells, {} countries; {} border cells",
                p.id(),
                p.dem().res_arcsec(),
                p.dem().tile_count(),
                p.dem().max_m(),
                p.h3_res(),
                p.terrain().cell_count(),
                p.terrain().country_count(),
                p.territory().cell_count()
            )
        })
        .collect()
}
