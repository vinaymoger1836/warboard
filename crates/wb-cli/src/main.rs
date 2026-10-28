//! `wb` — headless WarBoard runner.

use anyhow::Context;
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::time::Instant;
use wb_sim::Sim;

mod geo;

#[derive(Parser)]
#[command(name = "wb", version, about = "WarBoard headless runner")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run a scenario for a number of ticks.
    Run {
        /// Scenario YAML file.
        #[arg(long)]
        scenario: PathBuf,
        /// Ticks to run.
        #[arg(long, default_value_t = 10_000)]
        ticks: u64,
        /// Print the final state hash (64 hex chars) on stdout.
        #[arg(long)]
        print_hash: bool,
    },
    /// Validate and compile the content in `content/`.
    #[command(subcommand)]
    Content(ContentCommand),
    /// Query installed region packs (territory, terrain, elevation, line of sight).
    #[command(subcommand)]
    Geo(geo::GeoCommand),
}

#[derive(Subcommand)]
enum ContentCommand {
    /// Validate every entry; prints all errors.
    Check {
        #[arg(long, default_value = "content")]
        dir: PathBuf,
    },
    /// Validate and write the pack; prints its hash on stdout.
    Build {
        #[arg(long, default_value = "content")]
        dir: PathBuf,
        #[arg(long, default_value = "build/content.wbpack")]
        out: PathBuf,
    },
    /// Write the JSON Schema of the source format.
    Schema {
        #[arg(
            long,
            default_value = "packages/protocol/src/generated/content.schema.json"
        )]
        out: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Run {
            scenario,
            ticks,
            print_hash,
        } => run(&scenario, ticks, print_hash),
        Command::Content(ContentCommand::Check { dir }) => {
            let set = load_content(&dir)?;
            eprintln!(
                "{}: {} entries OK ({})",
                dir.display(),
                set.entries.len(),
                counts(&set)
            );
            Ok(())
        }
        Command::Content(ContentCommand::Build { dir, out }) => {
            let set = load_content(&dir)?;
            let db = wb_content::compile(&set);
            let (header, bytes) = wb_content::encode(&db, &set.manifest.content_version);
            write_file(&out, &bytes)?;
            eprintln!(
                "{}: {} entries, content {}, schema {}, {} bytes",
                out.display(),
                set.entries.len(),
                header.content_version,
                header.schema_version,
                bytes.len()
            );
            println!("{}", header.hash_hex());
            Ok(())
        }
        Command::Geo(cmd) => geo::run(cmd),
        Command::Content(ContentCommand::Schema { out }) => {
            let json = serde_json::to_string_pretty(&wb_content::schema::content_schema())?;
            write_file(&out, format!("{json}\n").as_bytes())?;
            eprintln!("{}: written", out.display());
            Ok(())
        }
    }
}

/// Loads and validates content; prints every error before failing.
fn load_content(dir: &Path) -> anyhow::Result<wb_content::ContentSet> {
    wb_content::load_dir(dir).map_err(|errors| {
        for e in &errors {
            eprintln!("{e}");
        }
        anyhow::anyhow!("{} content error(s) in {}", errors.len(), dir.display())
    })
}

/// `12 platform, 13 weapon, …` in kind order.
fn counts(set: &wb_content::ContentSet) -> String {
    wb_content::source::Kind::ALL
        .iter()
        .filter_map(|k| {
            let n = set.entries.iter().filter(|e| e.kind == *k).count();
            (n > 0).then(|| format!("{n} {}", k.as_str()))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn write_file(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    std::fs::write(path, bytes).with_context(|| format!("cannot write {}", path.display()))
}

fn run(path: &PathBuf, ticks: u64, print_hash: bool) -> anyhow::Result<()> {
    let yaml = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read scenario {}", path.display()))?;
    let mut sim = Sim::from_yaml(&yaml)
        .with_context(|| format!("cannot load scenario {}", path.display()))?;

    let started = Instant::now();
    sim.run_ticks(ticks);
    let elapsed = started.elapsed();

    let clock = sim.clock();
    let per_tick_us = if ticks == 0 {
        0.0
    } else {
        elapsed.as_secs_f64() * 1e6 / ticks as f64
    };
    eprintln!(
        "{}: {} ticks, sim time {:.1} s, {:.2} µs per tick",
        path.display(),
        clock.tick,
        clock.sim_time_ms as f64 / 1_000.0,
        per_tick_us
    );
    if print_hash {
        println!("{}", sim.state_hash_hex());
    }
    Ok(())
}
