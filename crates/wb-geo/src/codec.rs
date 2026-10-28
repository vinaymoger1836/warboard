//! Little-endian field reading, zstd frames and a small LRU cache.

use crate::GeoError;
use std::collections::BTreeMap;
use std::io::Read;
use std::rc::Rc;

/// Sequential little-endian reader over a byte slice.
pub(crate) struct Reader<'a> {
    name: &'a str,
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(name: &'a str, buf: &'a [u8]) -> Self {
        Self { name, buf, pos: 0 }
    }

    pub fn at(name: &'a str, buf: &'a [u8], pos: usize) -> Self {
        Self { name, buf, pos }
    }

    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8], GeoError> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.buf.len());
        let end = end.ok_or_else(|| {
            GeoError::format(self.name, format!("truncated at byte {}", self.pos))
        })?;
        let out = &self.buf[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    pub fn skip(&mut self, n: usize) -> Result<(), GeoError> {
        self.bytes(n).map(|_| ())
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], GeoError> {
        let mut a = [0u8; N];
        a.copy_from_slice(self.bytes(N)?);
        Ok(a)
    }

    pub fn u8(&mut self) -> Result<u8, GeoError> {
        Ok(self.array::<1>()?[0])
    }

    pub fn u16(&mut self) -> Result<u16, GeoError> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    pub fn i16(&mut self) -> Result<i16, GeoError> {
        Ok(i16::from_le_bytes(self.array()?))
    }

    pub fn u32(&mut self) -> Result<u32, GeoError> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    pub fn u64(&mut self) -> Result<u64, GeoError> {
        Ok(u64::from_le_bytes(self.array()?))
    }
}

/// Decodes one zstd frame and checks the decoded length.
pub(crate) fn zstd(
    name: &str,
    input: &[u8],
    expected_len: Option<usize>,
) -> Result<Vec<u8>, GeoError> {
    let mut decoder = ruzstd::decoding::StreamingDecoder::new(input)
        .map_err(|e| GeoError::format(name, format!("zstd: {e}")))?;
    let mut out = Vec::with_capacity(expected_len.unwrap_or(input.len() * 4));
    decoder
        .read_to_end(&mut out)
        .map_err(|e| GeoError::format(name, format!("zstd: {e}")))?;
    if let Some(n) = expected_len
        && out.len() != n
    {
        return Err(GeoError::format(
            name,
            format!("block decodes to {} bytes, expected {n}", out.len()),
        ));
    }
    Ok(out)
}

/// Least-recently-used cache keyed by `u64`. Ordered maps only, so it behaves the same
/// everywhere; its contents never affect a result, only how often bytes are re-read.
pub(crate) struct Lru<V> {
    capacity: usize,
    clock: u64,
    entries: BTreeMap<u64, (u64, Rc<V>)>,
    by_age: BTreeMap<u64, u64>,
}

impl<V> Lru<V> {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            clock: 0,
            entries: BTreeMap::new(),
            by_age: BTreeMap::new(),
        }
    }

    pub fn get(&mut self, key: u64) -> Option<Rc<V>> {
        self.clock += 1;
        let clock = self.clock;
        let (stamp, value) = self.entries.get_mut(&key)?;
        self.by_age.remove(stamp);
        *stamp = clock;
        self.by_age.insert(clock, key);
        Some(Rc::clone(value))
    }

    pub fn insert(&mut self, key: u64, value: Rc<V>) {
        self.clock += 1;
        if let Some((stamp, _)) = self.entries.insert(key, (self.clock, value)) {
            self.by_age.remove(&stamp);
        }
        self.by_age.insert(self.clock, key);
        while self.entries.len() > self.capacity {
            let Some((_, oldest)) = self.by_age.pop_first() else {
                break;
            };
            self.entries.remove(&oldest);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lru_evicts_the_oldest() {
        let mut lru = Lru::new(2);
        lru.insert(1, Rc::new("a"));
        lru.insert(2, Rc::new("b"));
        assert!(lru.get(1).is_some());
        lru.insert(3, Rc::new("c"));
        assert!(lru.get(2).is_none());
        assert!(lru.get(1).is_some() && lru.get(3).is_some());
    }
}
