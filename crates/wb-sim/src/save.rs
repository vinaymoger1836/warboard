//! `.wbsave` (doc 04 §7.1): header, then a zstd-compressed postcard body with
//! the snapshot, the command log and the event log.
//!
//! ```text
//! "WBSG" | save version u16 LE | postcard(SaveHeader) | zstd(postcard(SaveBody))
//! ```
//!
//! A save loads only with the content pack it was made with.

use serde::{Deserialize, Serialize};
use std::io::Read;

use crate::events::Event;
use crate::orders::QueuedOrder;
use crate::snapshot::Snapshot;

pub const MAGIC: [u8; 4] = *b"WBSG";
pub const SAVE_VERSION: u16 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SaveHeader {
    pub war_name: String,
    /// BLAKE3 of the content pack body, hex.
    pub content_hash: String,
    /// `id@version` of the region packs in use.
    pub packs: Vec<String>,
    /// Wall-clock time the client made the save (Unix ms); never read by the engine.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub created_utc_ms: i64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub tick: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub sim_time_ms: u64,
    /// War start, Unix ms UTC.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub start_utc_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SaveBody {
    pub snapshot: Snapshot,
    pub order_log: Vec<QueuedOrder>,
    pub events: Vec<Event>,
}

#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error("not a WarBoard save (bad magic)")]
    Magic,
    #[error("save version {0} is not supported (expected {SAVE_VERSION})")]
    Version(u16),
    #[error("save is corrupt: {0}")]
    Decode(String),
    #[error(
        "this save was made with different content (save {save}, installed {installed}); \
         install the matching content pack to load it"
    )]
    Content { save: String, installed: String },
}

pub fn encode(header: &SaveHeader, body: &SaveBody) -> Vec<u8> {
    let raw = postcard::to_stdvec(body).expect("save body serializes");
    let packed = ruzstd::encoding::compress_to_vec(
        raw.as_slice(),
        ruzstd::encoding::CompressionLevel::Fastest,
    );
    let mut out = Vec::with_capacity(packed.len() + 256);
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&SAVE_VERSION.to_le_bytes());
    out.extend_from_slice(&postcard::to_stdvec(header).expect("save header serializes"));
    out.extend_from_slice(&packed);
    out
}

/// Reads only the header (save slot lists).
pub fn read_header(bytes: &[u8]) -> Result<(SaveHeader, &[u8]), SaveError> {
    if bytes.len() < 6 || bytes[..4] != MAGIC {
        return Err(SaveError::Magic);
    }
    let version = u16::from_le_bytes([bytes[4], bytes[5]]);
    if version != SAVE_VERSION {
        return Err(SaveError::Version(version));
    }
    postcard::take_from_bytes::<SaveHeader>(&bytes[6..])
        .map_err(|e| SaveError::Decode(e.to_string()))
}

pub fn decode(bytes: &[u8]) -> Result<(SaveHeader, SaveBody), SaveError> {
    let (header, rest) = read_header(bytes)?;
    let mut raw = Vec::new();
    ruzstd::decoding::StreamingDecoder::new(rest)
        .map_err(|e| SaveError::Decode(e.to_string()))?
        .read_to_end(&mut raw)
        .map_err(|e| SaveError::Decode(e.to_string()))?;
    let body = postcard::from_bytes(&raw).map_err(|e| SaveError::Decode(e.to_string()))?;
    Ok((header, body))
}
