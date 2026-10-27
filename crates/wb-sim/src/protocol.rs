//! Types shared with TypeScript through ts-rs (`packages/protocol`).
//!
//! P0 placeholder: the worker's tick report. The binary `SideView` (doc 06 §3)
//! replaces most of this from Phase 3.

use ts_rs::TS;

/// What the engine worker posts to the UI about 10 times a second.
#[derive(TS, Clone, Debug)]
#[ts(export, rename_all = "camelCase")]
pub struct TickReport {
    #[ts(type = "number")]
    pub tick: u64,
    #[ts(type = "number")]
    pub sim_time_ms: u64,
    /// War start, Unix epoch ms UTC.
    #[ts(type = "number")]
    pub start_utc_ms: i64,
    pub tick_len_ms: u32,
    pub requested_speed: f64,
    /// Sim seconds per real second over the last second.
    pub achieved_speed: f64,
    /// BLAKE3 state hash, 64 hex chars.
    pub state_hash: String,
}
