//! Types shared with TypeScript through ts-rs (`packages/protocol`).
//!
//! The worker's tick report, and the icon codes of the side view as a TS
//! module (`generated/symbols.ts`). Orders, events and details types export
//! from their own modules.

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

/// Writes `symbols.ts` next to the ts-rs output (run by `pnpm gen`, whose
/// filter `export_bindings` matches this test's name).
#[cfg(test)]
mod tests {
    #[test]
    fn export_bindings_symbols() {
        let dir = std::env::var("TS_RS_EXPORT_DIR").unwrap_or_else(|_| "bindings".into());
        let mut lines = vec![
            "// Generated from wb-sim view.rs (SYMBOLS); do not edit. Run `pnpm gen`.".to_owned(),
            "export const SYMBOLS = {".to_owned(),
        ];
        for (code, name) in crate::view::SYMBOLS {
            lines.push(format!("  {name}: {code},"));
        }
        lines.push("} as const;".to_owned());
        lines.push(String::new());
        lines.push("export type SymbolName = keyof typeof SYMBOLS;".to_owned());
        lines.push(String::new());
        lines.push("/** SideView `flags` bits. */".to_owned());
        lines.push("export const FLAG = {".to_owned());
        for (name, v) in crate::view::FLAG_NAMES {
            lines.push(format!("  {name}: {v},"));
        }
        lines.push("} as const;".to_owned());
        lines.push(String::new());
        lines.push("/** Status names by value (bits 8–11 of `flags`). */".to_owned());
        let names: Vec<String> = crate::view::STATUS_NAMES
            .iter()
            .map(|n| format!("'{n}'"))
            .collect();
        lines.push(format!(
            "export const STATUS = [{}] as const;",
            names.join(", ")
        ));
        lines.push(String::new());
        lines.push(format!(
            "export const VIEW_MAGIC = '{}';",
            String::from_utf8_lossy(&crate::view::MAGIC)
        ));
        lines.push(format!(
            "export const VIEW_VERSION = {};",
            crate::view::VERSION
        ));
        lines.push(format!(
            "export const VIEW_HEADER_BYTES = {};",
            crate::view::HEADER_BYTES
        ));
        lines.push(String::new());
        let path = std::path::Path::new(&dir).join("symbols.ts");
        std::fs::create_dir_all(&dir).expect("export dir");
        std::fs::write(path, lines.join("\n")).expect("write symbols.ts");
    }
}
