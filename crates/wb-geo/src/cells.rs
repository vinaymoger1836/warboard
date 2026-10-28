//! Readers for the two H3-keyed files (doc 05 §2.2): `.wbh3` terrain cells and the
//! territory border-cell rings. Both share one layout: header, country table, block index,
//! zstd blocks of cells sorted by H3 index.

use crate::codec::{Lru, Reader, zstd};
use crate::source::{PackSource, read_range};
use crate::{CountryCode, GeoError, TerrainClass};
use std::cell::RefCell;
use std::rc::Rc;

const HEADER_LEN: usize = 32;
const ENTRY_LEN: usize = 24;
const VERSION: u16 = 1;

pub const FLAG_COAST: u8 = 1;
pub const FLAG_BORDER: u8 = 2;
pub const FLAG_URBAN: u8 = 4;

#[derive(Clone, Copy, Debug)]
struct BlockEntry {
    first_cell: u64,
    offset: u64,
    len: u32,
    count: u32,
}

/// Header, country table and block index shared by both files.
struct Blocked {
    src: Box<dyn PackSource>,
    res: u8,
    cell_count: u64,
    codes: Vec<Option<CountryCode>>,
    blocks: Vec<BlockEntry>,
}

impl Blocked {
    fn open(src: Box<dyn PackSource>, magic: &[u8; 4]) -> Result<Self, GeoError> {
        let head = read_range(src.as_ref(), 0, HEADER_LEN)?;
        let mut r = Reader::new(src.name(), &head);
        if r.bytes(4)? != magic {
            return Err(GeoError::format(src.name(), "wrong file type"));
        }
        let version = r.u16()?;
        if version != VERSION {
            return Err(GeoError::format(
                src.name(),
                format!("version {version}, expected {VERSION}"),
            ));
        }
        let res = r.u8()?;
        r.skip(1)?;
        let cell_count = r.u64()?;
        let block_count = r.u32()? as usize;
        let _block_size = r.u32()?;
        let country_count = usize::from(r.u16()?);
        let table = read_range(src.as_ref(), HEADER_LEN as u64, country_count * 4)?;
        let codes = table
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
            .map(|(i, c)| {
                if i == 0 {
                    Ok(None)
                } else {
                    CountryCode::from_bytes(c).map(Some)
                }
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| GeoError::format(src.name(), e))?;
        let index_at = (HEADER_LEN + country_count * 4) as u64;
        let raw = read_range(src.as_ref(), index_at, block_count * ENTRY_LEN)?;
        let mut r = Reader::new(src.name(), &raw);
        let mut blocks = Vec::with_capacity(block_count);
        for _ in 0..block_count {
            blocks.push(BlockEntry {
                first_cell: r.u64()?,
                offset: r.u64()?,
                len: r.u32()?,
                count: r.u32()?,
            });
        }
        if blocks
            .windows(2)
            .any(|w| w[0].first_cell >= w[1].first_cell)
        {
            return Err(GeoError::format(src.name(), "block index not sorted"));
        }
        Ok(Self {
            src,
            res,
            cell_count,
            codes,
            blocks,
        })
    }

    /// Block that would hold `cell`, if any.
    fn block_for(&self, cell: u64) -> Option<usize> {
        self.blocks
            .partition_point(|b| b.first_cell <= cell)
            .checked_sub(1)
    }

    fn read_block(&self, i: usize, expected_len: Option<usize>) -> Result<Vec<u8>, GeoError> {
        let b = self.blocks[i];
        let blob = read_range(self.src.as_ref(), b.offset, b.len as usize)?;
        zstd(self.src.name(), &blob, expected_len)
    }

    fn code(&self, index: u16) -> Result<Option<CountryCode>, GeoError> {
        self.codes.get(usize::from(index)).copied().ok_or_else(|| {
            GeoError::format(
                self.src.name(),
                format!("country index {index} out of range"),
            )
        })
    }
}

/// What the terrain file says about one cell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellInfo {
    pub cell: u64,
    pub class: TerrainClass,
    pub elevation_m: i16,
    pub slope_deg: f64,
    pub flags: u8,
    /// Owner at the cell centre; for border cells use the exact territory lookup.
    pub country: Option<CountryCode>,
}

impl CellInfo {
    pub fn is_border(&self) -> bool {
        self.flags & FLAG_BORDER != 0
    }

    pub fn is_coast(&self) -> bool {
        self.flags & FLAG_COAST != 0
    }

    pub fn is_urban(&self) -> bool {
        self.flags & FLAG_URBAN != 0
    }
}

struct TerrainBlock {
    cells: Vec<u64>,
    raw: Vec<u8>,
}

/// `.wbh3`: terrain class, elevation, slope, flags and owner per H3 cell.
pub struct TerrainFile {
    file: Blocked,
    cache: RefCell<Lru<TerrainBlock>>,
}

const TERRAIN_RECORD: usize = 8 + 1 + 2 + 1 + 1 + 2;

impl TerrainFile {
    pub fn open(src: Box<dyn PackSource>) -> Result<Self, GeoError> {
        Ok(Self {
            file: Blocked::open(src, b"WBH3")?,
            cache: RefCell::new(Lru::new(64)),
        })
    }

    pub fn res(&self) -> u8 {
        self.file.res
    }

    pub fn cell_count(&self) -> u64 {
        self.file.cell_count
    }

    pub fn country_count(&self) -> usize {
        self.file.codes.len() - 1
    }

    /// The stored cell, or `None` for open sea outside every territory.
    pub fn lookup(&self, cell: u64) -> Result<Option<CellInfo>, GeoError> {
        let Some(bi) = self.file.block_for(cell) else {
            return Ok(None);
        };
        let block = self.block(bi)?;
        let Ok(i) = block.cells.binary_search(&cell) else {
            return Ok(None);
        };
        let n = block.cells.len();
        let raw = &block.raw;
        let class_at = 8 * n;
        let elev_at = class_at + n;
        let slope_at = elev_at + 2 * n;
        let flags_at = slope_at + n;
        let country_at = flags_at + n;
        let class = TerrainClass::from_u8(raw[class_at + i]).ok_or_else(|| {
            GeoError::format(
                self.file.src.name(),
                format!("unknown class {}", raw[class_at + i]),
            )
        })?;
        let elevation_m = i16::from_le_bytes([raw[elev_at + 2 * i], raw[elev_at + 2 * i + 1]]);
        let country = u16::from_le_bytes([raw[country_at + 2 * i], raw[country_at + 2 * i + 1]]);
        Ok(Some(CellInfo {
            cell,
            class,
            elevation_m,
            slope_deg: f64::from(raw[slope_at + i]) * 0.5,
            flags: raw[flags_at + i],
            country: self.file.code(country)?,
        }))
    }

    fn block(&self, i: usize) -> Result<Rc<TerrainBlock>, GeoError> {
        if let Some(b) = self.cache.borrow_mut().get(i as u64) {
            return Ok(b);
        }
        let n = self.file.blocks[i].count as usize;
        let raw = self.file.read_block(i, Some(n * TERRAIN_RECORD))?;
        let cells = raw[..8 * n]
            .as_chunks::<8>()
            .0
            .iter()
            .map(|c| u64::from_le_bytes(*c))
            .collect();
        let b = Rc::new(TerrainBlock { cells, raw });
        self.cache.borrow_mut().insert(i as u64, Rc::clone(&b));
        Ok(b)
    }
}

struct TerritoryBlock {
    cells: Vec<u64>,
    offsets: Vec<u32>,
    records: Vec<u8>,
}

/// Territory rings for border cells; coordinates in 1e-7 degrees.
pub struct TerritoryFile {
    file: Blocked,
    cache: RefCell<Lru<TerritoryBlock>>,
}

/// Fixed-point scale of the ring coordinates.
pub const COORD_SCALE: f64 = 1e7;
const FULL_TURN: i64 = 3_600_000_000;

impl TerritoryFile {
    pub fn open(src: Box<dyn PackSource>) -> Result<Self, GeoError> {
        Ok(Self {
            file: Blocked::open(src, b"WBTR")?,
            cache: RefCell::new(Lru::new(32)),
        })
    }

    pub fn res(&self) -> u8 {
        self.file.res
    }

    pub fn cell_count(&self) -> u64 {
        self.file.cell_count
    }

    /// Owner of the point (`x`, `y` = lon, lat in 1e-7 degrees) inside a border cell.
    /// `Ok(None)` if the cell is not a border cell; `Ok(Some(None))` if no country owns the point.
    pub fn owner(
        &self,
        cell: u64,
        x: i64,
        y: i64,
    ) -> Result<Option<Option<CountryCode>>, GeoError> {
        let Some(bi) = self.file.block_for(cell) else {
            return Ok(None);
        };
        let block = self.block(bi)?;
        let Ok(i) = block.cells.binary_search(&cell) else {
            return Ok(None);
        };
        let name = self.file.src.name();
        let mut r = Reader::at(name, &block.records, block.offsets[i] as usize);
        let pieces = r.u16()?;
        let shifted = r.u16()? & 1 != 0;
        // Cells straddling 180° store rings east of it (longitudes above 180°).
        let x = if shifted && x < 0 { x + FULL_TURN } else { x };
        for _ in 0..pieces {
            let country = r.u16()?;
            let rings = r.u16()?;
            let mut inside = false;
            for _ in 0..rings {
                let n = r.u32()? as usize;
                let pts = r.bytes(n * 8)?;
                if ring_crossings_odd(pts, x, y) {
                    inside = !inside;
                }
            }
            if inside {
                return Ok(Some(self.file.code(country)?));
            }
        }
        Ok(Some(None))
    }

    fn block(&self, i: usize) -> Result<Rc<TerritoryBlock>, GeoError> {
        if let Some(b) = self.cache.borrow_mut().get(i as u64) {
            return Ok(b);
        }
        let n = self.file.blocks[i].count as usize;
        let raw = self.file.read_block(i, None)?;
        let name = self.file.src.name();
        let mut r = Reader::new(name, &raw);
        let mut cells = Vec::with_capacity(n);
        for _ in 0..n {
            cells.push(r.u64()?);
        }
        let mut offsets = Vec::with_capacity(n);
        for _ in 0..n {
            offsets.push(r.u32()?);
        }
        let records = raw[12 * n..].to_vec();
        let b = Rc::new(TerritoryBlock {
            cells,
            offsets,
            records,
        });
        self.cache.borrow_mut().insert(i as u64, Rc::clone(&b));
        Ok(b)
    }
}

/// Even-odd ray test against one closed ring of little-endian `i32` (x, y) pairs.
/// Integer arithmetic only, so every platform gives the same answer.
fn ring_crossings_odd(pts: &[u8], x: i64, y: i64) -> bool {
    let n = pts.len() / 8;
    let at = |k: usize| {
        let b = &pts[k * 8..k * 8 + 8];
        (
            i64::from(i32::from_le_bytes([b[0], b[1], b[2], b[3]])),
            i64::from(i32::from_le_bytes([b[4], b[5], b[6], b[7]])),
        )
    };
    let mut odd = false;
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (xi, yi) = at(i);
        let (xj, yj) = at(j);
        if (yi > y) != (yj > y) {
            // x < xi + (y - yi)·(xj - xi)/(yj - yi), without division.
            let lhs = i128::from(x - xi) * i128::from(yj - yi);
            let rhs = i128::from(y - yi) * i128::from(xj - xi);
            if (yj > yi && lhs < rhs) || (yj < yi && lhs > rhs) {
                odd = !odd;
            }
        }
        j = i;
    }
    odd
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ring(points: &[(i32, i32)]) -> Vec<u8> {
        points
            .iter()
            .flat_map(|(x, y)| [x.to_le_bytes(), y.to_le_bytes()].concat())
            .collect()
    }

    #[test]
    fn even_odd_ring_test() {
        let square = ring(&[(0, 0), (10, 0), (10, 10), (0, 10)]);
        assert!(ring_crossings_odd(&square, 5, 5));
        assert!(!ring_crossings_odd(&square, 15, 5));
        assert!(!ring_crossings_odd(&square, 5, -1));
        // Concave "C" shape: the notch is outside.
        let c = ring(&[
            (0, 0),
            (10, 0),
            (10, 3),
            (3, 3),
            (3, 7),
            (10, 7),
            (10, 10),
            (0, 10),
        ]);
        assert!(ring_crossings_odd(&c, 1, 5));
        assert!(!ring_crossings_odd(&c, 6, 5));
    }
}
