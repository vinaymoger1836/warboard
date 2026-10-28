//! `.wbdem` reader (doc 05 §2.2): 1° tiles of int16 metres on a regular point grid.

use crate::GeoError;
use crate::codec::{Lru, Reader, zstd};
use crate::source::{PackSource, read_range};
use std::cell::RefCell;
use std::rc::Rc;

const MAGIC: &[u8; 4] = b"WBDM";
const VERSION: u16 = 1;
const HEADER_LEN: usize = 32;
const ENTRY_LEN: usize = 16;
const CACHE_TILES: usize = 64;

#[derive(Clone, Copy, Debug)]
struct TileEntry {
    offset: u64,
    len: u32,
    min_m: i16,
    max_m: i16,
}

/// Elevation grid of one pack.
pub struct Dem {
    src: Box<dyn PackSource>,
    res_arcsec: u16,
    samples: usize,
    lon0: i16,
    lat0: i16,
    cols: u16,
    rows: u16,
    index: Vec<TileEntry>,
    cache: RefCell<Lru<Vec<i16>>>,
}

impl Dem {
    pub fn open(src: Box<dyn PackSource>) -> Result<Self, GeoError> {
        let head = read_range(src.as_ref(), 0, HEADER_LEN)?;
        let mut r = Reader::new(src.name(), &head);
        if r.bytes(4)? != MAGIC {
            return Err(GeoError::format(src.name(), "not a .wbdem file"));
        }
        let version = r.u16()?;
        if version != VERSION {
            return Err(GeoError::format(
                src.name(),
                format!("version {version}, expected {VERSION}"),
            ));
        }
        let res_arcsec = r.u16()?;
        let samples = usize::from(r.u16()?);
        r.skip(2)?;
        let (lon0, lat0, cols, rows) = (r.i16()?, r.i16()?, r.u16()?, r.u16()?);
        if res_arcsec == 0
            || 3600 % res_arcsec != 0
            || samples != 3600 / usize::from(res_arcsec) + 1
        {
            return Err(GeoError::format(src.name(), "inconsistent resolution"));
        }
        let n = usize::from(cols) * usize::from(rows);
        let raw = read_range(src.as_ref(), HEADER_LEN as u64, n * ENTRY_LEN)?;
        let mut r = Reader::new(src.name(), &raw);
        let mut index = Vec::with_capacity(n);
        for _ in 0..n {
            index.push(TileEntry {
                offset: r.u64()?,
                len: r.u32()?,
                min_m: r.i16()?,
                max_m: r.i16()?,
            });
        }
        Ok(Self {
            src,
            res_arcsec,
            samples,
            lon0,
            lat0,
            cols,
            rows,
            index,
            cache: RefCell::new(Lru::new(CACHE_TILES)),
        })
    }

    /// Pack extent `[west, south, east, north]` in degrees.
    pub fn extent_deg(&self) -> [f64; 4] {
        let (w, s) = (f64::from(self.lon0), f64::from(self.lat0));
        [w, s, w + f64::from(self.cols), s + f64::from(self.rows)]
    }

    pub fn res_arcsec(&self) -> u16 {
        self.res_arcsec
    }

    /// Number of 1° tiles that hold data.
    pub fn tile_count(&self) -> usize {
        self.index.iter().filter(|e| e.len > 0).count()
    }

    /// Highest elevation in the whole grid, metres.
    pub fn max_m(&self) -> i16 {
        self.index.iter().map(|e| e.max_m).max().unwrap_or(0)
    }

    /// Bilinear elevation at a point, metres; `None` outside the pack. Tiles without
    /// data (open sea) are sea level.
    pub fn elevation_deg(&self, lat_deg: f64, lon_deg: f64) -> Result<Option<f64>, GeoError> {
        let x = lon_deg - f64::from(self.lon0);
        let y = lat_deg - f64::from(self.lat0);
        let (cols, rows) = (f64::from(self.cols), f64::from(self.rows));
        if !(0.0..=cols).contains(&x) || !(0.0..=rows).contains(&y) {
            return Ok(None);
        }
        let col = (x.floor() as usize).min(usize::from(self.cols) - 1);
        let row = (y.floor() as usize).min(usize::from(self.rows) - 1);
        let entry = self.index[row * usize::from(self.cols) + col];
        if entry.len == 0 {
            return Ok(Some(0.0));
        }
        if entry.min_m == entry.max_m {
            return Ok(Some(f64::from(entry.min_m)));
        }
        let tile = self.tile(row * usize::from(self.cols) + col, entry)?;
        let n = self.samples - 1;
        let fx = (x - col as f64) * n as f64;
        let fy = (y - row as f64) * n as f64;
        let j = (fx.floor() as usize).min(n - 1);
        let i = (fy.floor() as usize).min(n - 1);
        let (tx, ty) = (fx - j as f64, fy - i as f64);
        let s = self.samples;
        let z = |r: usize, c: usize| f64::from(tile[r * s + c]);
        let south = z(i, j) * (1.0 - tx) + z(i, j + 1) * tx;
        let north = z(i + 1, j) * (1.0 - tx) + z(i + 1, j + 1) * tx;
        Ok(Some(south * (1.0 - ty) + north * ty))
    }

    fn tile(&self, key: usize, entry: TileEntry) -> Result<Rc<Vec<i16>>, GeoError> {
        if let Some(t) = self.cache.borrow_mut().get(key as u64) {
            return Ok(t);
        }
        let blob = read_range(self.src.as_ref(), entry.offset, entry.len as usize)?;
        let count = self.samples * self.samples;
        let raw = zstd(self.src.name(), &blob, Some(count * 2))?;
        let grid: Vec<i16> = raw
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| i16::from_le_bytes(*c))
            .collect();
        let grid = Rc::new(grid);
        self.cache.borrow_mut().insert(key as u64, Rc::clone(&grid));
        Ok(grid)
    }
}
