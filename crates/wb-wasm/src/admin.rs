//! Bindings for the admin app (doc 04 §5, ADR-045): the content check, the
//! pack compiler and the test bench. The game never calls these; the test
//! bench runs its own sims from scenario YAML and returns per-seed metric
//! values only, never a war's state.

use serde::Deserialize;
use std::sync::Arc;
use wasm_bindgen::prelude::*;
use wb_content::{ContentDb, SourceFile};
use wb_sim::Scenario;
use wb_sim::testbench::{Bench, Results, RunSummary};

#[derive(Deserialize)]
struct FileIn {
    path: String,
    text: String,
}

fn files(files_json: &str) -> Result<Vec<SourceFile>, JsError> {
    let files: Vec<FileIn> =
        serde_json::from_str(files_json).map_err(|e| JsError::new(&e.to_string()))?;
    Ok(files
        .into_iter()
        .map(|f| SourceFile {
            path: f.path,
            text: f.text,
        })
        .collect())
}

/// Checks a content folder: `files_json` is `[{ path, text }]` with paths
/// relative to the content root (and `denylist.txt`). Returns the report as
/// JSON (`wb_content::report::Report`): the same rules as `wb content check`.
#[wasm_bindgen]
pub fn check_content(files_json: &str) -> Result<String, JsError> {
    let report = wb_content::report::check("content", files(files_json)?);
    serde_json::to_string(&report).map_err(|e| JsError::new(&e.to_string()))
}

/// Compiles a content folder into `content.wbpack`, the same bytes as
/// `wb content build`. Fails with the first errors when the check fails.
#[wasm_bindgen]
pub fn build_content_pack(files_json: &str) -> Result<Vec<u8>, JsError> {
    let set = wb_content::load_files("content", files(files_json)?).map_err(|errors| {
        let lines: Vec<String> = errors.iter().take(20).map(|e| e.to_string()).collect();
        JsError::new(&lines.join("\n"))
    })?;
    let db = wb_content::compile(&set);
    Ok(wb_content::encode(&db, &set.manifest.content_version).1)
}

/// `{ content_hash, content_version, schema_version }` of a pack.
#[wasm_bindgen]
pub fn content_pack_info(pack: &[u8]) -> Result<String, JsError> {
    let (h, _) = wb_content::decode(pack).map_err(|e| JsError::new(&e.to_string()))?;
    let hex: String = h.content_hash.iter().map(|b| format!("{b:02x}")).collect();
    Ok(serde_json::json!({
        "content_hash": hex,
        "content_version": h.content_version,
        "schema_version": h.schema_version,
    })
    .to_string())
}

/// Runs a test-bench scenario (a scenario with a `golden:` block) seed by
/// seed, so the page can show progress between calls.
#[wasm_bindgen]
pub struct TestBench {
    bench: Bench,
    db: Arc<ContentDb>,
    hash: [u8; 32],
    seeds: u64,
    /// Next run: index into `variants × seeds`.
    next: u64,
    results: Results,
    runs: Vec<RunSummary>,
}

#[wasm_bindgen]
impl TestBench {
    /// `seeds` overrides the block's seed count when non-zero.
    pub fn create(
        scenario_yaml: &str,
        content_pack: &[u8],
        seeds: u32,
    ) -> Result<TestBench, JsError> {
        let scenario =
            Scenario::from_yaml(scenario_yaml).map_err(|e| JsError::new(&e.to_string()))?;
        let bench = Bench::new(scenario).map_err(|e| JsError::new(&e.to_string()))?;
        let (h, db) = wb_content::decode(content_pack).map_err(|e| JsError::new(&e.to_string()))?;
        let seeds = if seeds > 0 {
            u64::from(seeds)
        } else {
            bench.seeds()
        };
        Ok(TestBench {
            bench,
            db: Arc::new(db),
            hash: h.content_hash,
            seeds,
            next: 0,
            results: Results::default(),
            runs: Vec::new(),
        })
    }

    /// `{ id, description, variants, seeds, max_ticks }`.
    pub fn info(&self) -> String {
        serde_json::json!({
            "id": self.bench.id(),
            "description": self.bench.description(),
            "variants": self.bench.variants(),
            "seeds": self.seeds,
            "max_ticks": self.bench.max_ticks(),
        })
        .to_string()
    }

    /// Runs left.
    pub fn remaining(&self) -> f64 {
        (self.bench.variants().len() as u64 * self.seeds - self.next) as f64
    }

    /// Runs up to `n` more runs; returns the number run.
    pub fn step(&mut self, n: u32) -> Result<u32, JsError> {
        let total = self.bench.variants().len() as u64 * self.seeds;
        let mut done = 0;
        while done < n && self.next < total {
            let v = self.bench.variants()[(self.next / self.seeds) as usize].clone();
            let k = self.next % self.seeds;
            let run = self
                .bench
                .run(&v, k, self.db.clone(), self.hash, None, &mut self.results)
                .map_err(|e| JsError::new(&e.to_string()))?;
            self.runs.push(run);
            self.next += 1;
            done += 1;
        }
        Ok(done)
    }

    /// `{ runs: RunSummary[], checks: Check[] }` for the runs so far.
    pub fn report(&self) -> Result<String, JsError> {
        let checks = self
            .bench
            .evaluate(&self.results, &self.db)
            .map_err(|e| JsError::new(&e.to_string()))?;
        Ok(serde_json::json!({ "runs": self.runs, "checks": checks }).to_string())
    }
}
