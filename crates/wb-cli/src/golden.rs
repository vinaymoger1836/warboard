//! `wb golden`: golden and test-bench scenarios (doc 08 §3). The runner is
//! `wb_sim::testbench` (shared with the admin app's test bench through
//! `wb-wasm`); this module reads the files and prints the results.

use anyhow::{Context, bail};
use std::path::Path;
use wb_sim::testbench::{Bench, Results};

use crate::sim::Content;

pub fn run(files: &[std::path::PathBuf], content: &Content) -> anyhow::Result<()> {
    let mut failed = 0;
    for f in files {
        failed += run_file(f, content)?;
    }
    if failed > 0 {
        bail!("{failed} golden expectation(s) out of band");
    }
    Ok(())
}

fn run_file(path: &Path, content: &Content) -> anyhow::Result<usize> {
    let scenario = crate::sim::read_scenario(path)?;
    let scenario_content = match &scenario.content {
        Some(c) => crate::sim::content(Path::new(c))?,
        None => Content {
            db: content.db.clone(),
            hash: content.hash,
        },
    };
    let content = &scenario_content;
    let bench = Bench::new(scenario).with_context(|| format!("{}", path.display()))?;
    println!(
        "{} {}{}",
        bench.id(),
        path.display(),
        bench
            .description()
            .map(|d| format!(" — {d}"))
            .unwrap_or_default()
    );
    let seeds = bench.seeds();
    let mut results = Results::default();
    for v in bench.variants() {
        for n in 0..seeds {
            let run = bench
                .run(v, n, content.db.clone(), content.hash, None, &mut results)
                .context("cannot build the scenario")?;
            if n == 0 {
                println!(
                    "  run {v}: {} ticks, {:.1} h sim time, hash {}{}",
                    run.ticks,
                    run.sim_time_ms as f64 / 3.6e6,
                    &run.hash[..16],
                    if seeds > 1 {
                        format!(" (seed 1 of {seeds})")
                    } else {
                        String::new()
                    }
                );
            }
        }
    }
    let mut failed = 0;
    for c in bench.evaluate(&results, &content.db)? {
        println!(
            "  {} {} {} = {} (band {}..{})",
            if c.ok { "pass" } else { "FAIL" },
            c.subject,
            c.label,
            c.value.map_or("missing".into(), |v| format!("{v:.2}")),
            c.min.map_or("".into(), |v| v.to_string()),
            c.max.map_or("".into(), |v| v.to_string()),
        );
        if !c.ok {
            failed += 1;
        }
    }
    Ok(failed)
}
