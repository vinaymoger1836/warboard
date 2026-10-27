//! `wb` — headless WarBoard runner.

use anyhow::Context;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::time::Instant;
use wb_sim::Sim;

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
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Run {
            scenario,
            ticks,
            print_hash,
        } => run(&scenario, ticks, print_hash),
    }
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
