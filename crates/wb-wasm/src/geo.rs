//! Region packs in the browser (ADR-011, ADR-019). The worker hands over JS objects that
//! read pack files synchronously (an OPFS `FileSystemSyncAccessHandle` in the browser,
//! `fs.readSync` under Node); `wb-geo` reads them through the same code path as natively.

use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wb_core::geo::LatLon;
use wb_geo::{Geo, GeoError, LosParams, Pack, PackSource};

#[wasm_bindgen]
extern "C" {
    /// `{ name: string, size(): number, readAt(buf: Uint8Array, at: number): number }`.
    /// `readAt` fills `buf` from byte `at` and returns the number of bytes read.
    pub type PackFile;

    #[wasm_bindgen(method, getter)]
    fn name(this: &PackFile) -> String;

    #[wasm_bindgen(method, catch)]
    fn size(this: &PackFile) -> Result<f64, JsValue>;

    #[wasm_bindgen(method, catch, js_name = readAt)]
    fn read_at(this: &PackFile, buf: &mut [u8], at: f64) -> Result<f64, JsValue>;
}

struct JsSource {
    file: PackFile,
    name: String,
    len: u64,
}

impl JsSource {
    fn new(file: PackFile) -> Result<Self, JsError> {
        let name = file.name();
        let len = file.size().map_err(|e| js_error(&name, &e))?;
        if !(0.0..=9_007_199_254_740_991.0).contains(&len) {
            return Err(JsError::new(&format!("{name}: invalid size {len}")));
        }
        Ok(Self {
            file,
            name,
            len: len as u64,
        })
    }
}

fn js_error(name: &str, e: &JsValue) -> JsError {
    JsError::new(&format!(
        "{name}: {}",
        e.as_string().unwrap_or_else(|| format!("{e:?}"))
    ))
}

impl PackSource for JsSource {
    fn len(&self) -> u64 {
        self.len
    }

    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), GeoError> {
        let mut done = 0usize;
        while done < buf.len() {
            let n = self
                .file
                .read_at(&mut buf[done..], (offset + done as u64) as f64)
                .map_err(|e| {
                    GeoError::io(
                        &self.name,
                        e.as_string().unwrap_or_else(|| format!("{e:?}")),
                    )
                })?;
            if n.is_nan() || n < 1.0 {
                return Err(GeoError::format(&self.name, "unexpected end of file"));
            }
            done += n as usize;
        }
        Ok(())
    }

    fn name(&self) -> &str {
        &self.name
    }
}

fn err(e: GeoError) -> JsError {
    JsError::new(&e.to_string())
}

/// Territory, terrain, elevation and line-of-sight queries over installed packs.
#[wasm_bindgen]
#[derive(Default)]
pub struct GeoQuery {
    /// Shared with the engine (`Engine::set_geo`), which reads it to validate
    /// orders.
    geo: Rc<Geo>,
}

impl GeoQuery {
    pub(crate) fn shared(&self) -> Rc<Geo> {
        self.geo.clone()
    }
}

#[wasm_bindgen]
impl GeoQuery {
    #[wasm_bindgen(constructor)]
    pub fn new() -> GeoQuery {
        GeoQuery::default()
    }

    /// Adds a pack from its three engine files.
    pub fn add_pack(
        &mut self,
        id: &str,
        dem: PackFile,
        terrain: PackFile,
        territory: PackFile,
    ) -> Result<(), JsError> {
        let src = |f: PackFile| -> Result<Box<dyn PackSource>, JsError> {
            Ok(Box::new(JsSource::new(f)?))
        };
        let pack = Pack::new(id, src(dem)?, src(terrain)?, src(territory)?).map_err(err)?;
        Rc::get_mut(&mut self.geo)
            .ok_or_else(|| JsError::new("packs are in use by the engine; open a new GeoQuery"))?
            .add_pack(pack)
            .map_err(err)
    }

    pub fn pack_count(&self) -> usize {
        self.geo.packs().len()
    }

    /// Same text as `wb geo info`.
    pub fn info(&self) -> String {
        wb_geo::info_lines(&self.geo).join("\n")
    }

    /// Same text as one line of `wb geo query`.
    pub fn query(&self, lat_deg: f64, lon_deg: f64, label: &str) -> Result<String, JsError> {
        wb_geo::query_line(&self.geo, lat_deg, lon_deg, label).map_err(err)
    }

    /// Same text as `wb geo los`.
    #[allow(clippy::too_many_arguments)]
    pub fn los(
        &self,
        lat_a: f64,
        lon_a: f64,
        alt_a_m: f64,
        lat_b: f64,
        lon_b: f64,
        alt_b_m: f64,
        k_factor: f64,
        step_m: f64,
    ) -> Result<String, JsError> {
        let los = self
            .geo
            .line_of_sight(
                LatLon::from_deg(lat_a, lon_a),
                alt_a_m,
                LatLon::from_deg(lat_b, lon_b),
                alt_b_m,
                &LosParams { k_factor, step_m },
            )
            .map_err(err)?;
        Ok(wb_geo::los_line(&los))
    }

    /// `[elevation_m, class, owner…]` for the cursor readout: the elevation, the class
    /// code (doc 05 §3 order) and the owner's code as char codes (empty if none).
    pub fn point(&self, lat_deg: f64, lon_deg: f64) -> Result<Vec<f64>, JsError> {
        let p = LatLon::from_deg(lat_deg, lon_deg);
        let Some(info) = self.geo.point_info(p).map_err(err)? else {
            return Ok(Vec::new());
        };
        let mut out = vec![info.elevation_m, f64::from(info.class as u8)];
        if let Some(owner) = info.owner {
            out.extend(owner.as_str().bytes().map(f64::from));
        }
        Ok(out)
    }

    /// Terrain cells in a box for the map overlay, packed as
    /// `[class, n, lon₀, lat₀, …, lonₙ₋₁, latₙ₋₁, class, n, …]`.
    /// Returns `undefined` if the box holds more than `max_cells` cells (zoom in).
    pub fn terrain_cells(
        &self,
        west: f64,
        south: f64,
        east: f64,
        north: f64,
        max_cells: usize,
    ) -> Result<Option<Vec<f64>>, JsError> {
        let Some(cells) = self
            .geo
            .cells_in_box(west, south, east, north, max_cells)
            .map_err(err)?
        else {
            return Ok(None);
        };
        let mut out = Vec::new();
        for c in cells {
            if c.class == wb_geo::TerrainClass::Water {
                continue;
            }
            out.push(f64::from(c.class as u8));
            out.push(c.boundary.len() as f64);
            for [lon, lat] in c.boundary {
                out.push(lon);
                out.push(lat);
            }
        }
        Ok(Some(out))
    }
}

/// Streaming BLAKE3 for pack verification in the pack worker.
#[wasm_bindgen]
pub struct Blake3Hasher {
    inner: blake3::Hasher,
}

#[wasm_bindgen]
impl Blake3Hasher {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Blake3Hasher {
        Blake3Hasher {
            inner: blake3::Hasher::new(),
        }
    }

    pub fn update(&mut self, bytes: &[u8]) {
        self.inner.update(bytes);
    }

    /// 64 lower-case hex characters.
    pub fn finalize_hex(&self) -> String {
        self.inner.finalize().to_hex().to_string()
    }
}

impl Default for Blake3Hasher {
    fn default() -> Self {
        Self::new()
    }
}
