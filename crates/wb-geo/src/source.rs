//! Where pack bytes come from (ADR-019).
//!
//! Every reader works on a [`PackSource`]: a fixed-length byte store that supports
//! positional reads. Natively that is a file read with positional I/O; in the browser
//! `wb-wasm` implements it over an OPFS `FileSystemSyncAccessHandle`. Both go through the
//! same LRU block caches, and results are pure functions of the bytes, so cache state
//! never changes an outcome.

use crate::GeoError;

/// A read-only byte store with positional reads.
pub trait PackSource {
    /// Total length in bytes.
    fn len(&self) -> u64;

    /// Fills `buf` from `offset`; fails if the range runs past the end.
    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), GeoError>;

    /// Name used in error messages.
    fn name(&self) -> &str;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Reads `len` bytes at `offset`, checking the range first.
pub(crate) fn read_range(
    src: &dyn PackSource,
    offset: u64,
    len: usize,
) -> Result<Vec<u8>, GeoError> {
    offset
        .checked_add(len as u64)
        .filter(|&e| e <= src.len())
        .ok_or_else(|| {
            GeoError::format(
                src.name(),
                format!("range {offset}+{len} past end {}", src.len()),
            )
        })?;
    let mut buf = vec![0u8; len];
    src.read_exact_at(offset, &mut buf)?;
    Ok(buf)
}

/// Bytes held in memory (tests, small files).
pub struct BytesSource {
    name: String,
    bytes: Vec<u8>,
}

impl BytesSource {
    pub fn new(name: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self {
            name: name.into(),
            bytes,
        }
    }
}

impl PackSource for BytesSource {
    fn len(&self) -> u64 {
        self.bytes.len() as u64
    }

    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), GeoError> {
        let start = usize::try_from(offset)
            .map_err(|_| GeoError::format(&self.name, "offset too large"))?;
        let src = start
            .checked_add(buf.len())
            .and_then(|end| self.bytes.get(start..end))
            .ok_or_else(|| GeoError::format(&self.name, "read past end"))?;
        buf.copy_from_slice(src);
        Ok(())
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// A file read with positional I/O (no memory map, so no `unsafe`; see ADR-019).
#[cfg(not(target_arch = "wasm32"))]
pub struct FileSource {
    name: String,
    file: std::fs::File,
    len: u64,
}

#[cfg(not(target_arch = "wasm32"))]
impl FileSource {
    pub fn open(path: &std::path::Path) -> Result<Self, GeoError> {
        let name = path.display().to_string();
        let file = std::fs::File::open(path).map_err(|e| GeoError::io(&name, e))?;
        let len = file.metadata().map_err(|e| GeoError::io(&name, e))?.len();
        Ok(Self { name, file, len })
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl PackSource for FileSource {
    fn len(&self) -> u64 {
        self.len
    }

    #[cfg(unix)]
    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), GeoError> {
        use std::os::unix::fs::FileExt;
        self.file
            .read_exact_at(buf, offset)
            .map_err(|e| GeoError::io(&self.name, e))
    }

    #[cfg(windows)]
    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), GeoError> {
        use std::os::windows::fs::FileExt;
        let mut done = 0;
        while done < buf.len() {
            let n = self
                .file
                .seek_read(&mut buf[done..], offset + done as u64)
                .map_err(|e| GeoError::io(&self.name, e))?;
            if n == 0 {
                return Err(GeoError::format(&self.name, "unexpected end of file"));
            }
            done += n;
        }
        Ok(())
    }

    fn name(&self) -> &str {
        &self.name
    }
}
