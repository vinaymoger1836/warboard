//! `content.wbpack`: magic, format version, postcard header, postcard body.
//! The header carries the BLAKE3 hash of the body bytes; saves record it.
//! The pack holds no file paths or timestamps, so the same content gives the
//! same bytes on every machine.

use crate::compiled::ContentDb;
use serde::{Deserialize, Serialize};

pub const MAGIC: [u8; 4] = *b"WBPK";
/// Layout of the file around the header and body.
pub const FORMAT_VERSION: u16 = 1;
/// Version of the compiled types. A breaking change bumps it and needs a
/// migration once packs are consumed (doc 04 §6).
pub const SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackHeader {
    pub schema_version: u32,
    /// Semantic version from `content/manifest.yaml`.
    pub content_version: String,
    /// BLAKE3 of the encoded body.
    pub content_hash: [u8; 32],
}

impl PackHeader {
    pub fn hash_hex(&self) -> String {
        self.content_hash
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PackError {
    #[error("not a WarBoard content pack (bad magic)")]
    Magic,
    #[error("pack format {0} is not supported (expected {FORMAT_VERSION})")]
    Format(u16),
    #[error("pack schema {0} is not supported (expected {SCHEMA_VERSION})")]
    Schema(u32),
    #[error("pack is corrupt: body hash does not match the header")]
    Hash,
    #[error("pack is corrupt: {0}")]
    Decode(#[from] postcard::Error),
}

/// Encodes a database; returns the header and the file bytes.
pub fn encode(db: &ContentDb, content_version: &str) -> (PackHeader, Vec<u8>) {
    let body = postcard::to_stdvec(db).expect("content db serializes");
    let header = PackHeader {
        schema_version: SCHEMA_VERSION,
        content_version: content_version.to_owned(),
        content_hash: *blake3::hash(&body).as_bytes(),
    };
    let mut out = Vec::with_capacity(body.len() + 64);
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&postcard::to_stdvec(&header).expect("header serializes"));
    out.extend_from_slice(&body);
    (header, out)
}

/// Reads only the header (cheap; for showing versions and hashes).
pub fn read_header(bytes: &[u8]) -> Result<(PackHeader, &[u8]), PackError> {
    if bytes.len() < 6 || bytes[..4] != MAGIC {
        return Err(PackError::Magic);
    }
    let format = u16::from_le_bytes([bytes[4], bytes[5]]);
    if format != FORMAT_VERSION {
        return Err(PackError::Format(format));
    }
    let (header, body) = postcard::take_from_bytes::<PackHeader>(&bytes[6..])?;
    Ok((header, body))
}

/// Decodes a pack, checking format, schema version and hash.
pub fn decode(bytes: &[u8]) -> Result<(PackHeader, ContentDb), PackError> {
    let (header, body) = read_header(bytes)?;
    if header.schema_version != SCHEMA_VERSION {
        return Err(PackError::Schema(header.schema_version));
    }
    if *blake3::hash(body).as_bytes() != header.content_hash {
        return Err(PackError::Hash);
    }
    let db = postcard::from_bytes(body)?;
    Ok((header, db))
}
