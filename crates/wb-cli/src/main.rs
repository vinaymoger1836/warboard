//! `wb` — headless WarBoard runner.

use anyhow::Context;
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::time::Instant;
use wb_sim::Sim;

mod bench;
mod geo;
mod golden;
mod sim;

/// Content used when `--content` is not given: the YAML sources, compiled in
/// memory exactly as `wb content build` would.
const DEFAULT_CONTENT: &str = "content";

#[derive(Parser)]
#[command(name = "wb", version, about = "WarBoard headless runner")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run a scenario (or a save) for a number of ticks.
    Run {
        /// Scenario YAML file.
        #[arg(long, required_unless_present = "load")]
        scenario: Option<PathBuf>,
        /// Continue from a `.wbsave` instead of a scenario.
        #[arg(long, conflicts_with = "scenario")]
        load: Option<PathBuf>,
        /// Content directory or `.wbpack`.
        #[arg(long, default_value = DEFAULT_CONTENT)]
        content: PathBuf,
        /// Region pack directories (orders are then checked against terrain).
        #[arg(long, value_delimiter = ',')]
        packs: Vec<PathBuf>,
        /// Total ticks to have run when done (counted from war start).
        #[arg(long, default_value_t = 10_000)]
        ticks: u64,
        /// Save at this tick, load the save into a fresh engine and carry on
        /// (the hash must equal an uninterrupted run).
        #[arg(long)]
        save_at: Option<u64>,
        /// Write a `.wbsave` at `--save-at` (or at the end) to this file.
        #[arg(long)]
        save_out: Option<PathBuf>,
        /// Print the final state hash (64 hex chars) on stdout.
        #[arg(long)]
        print_hash: bool,
        /// Print the event log as JSON lines on stdout.
        #[arg(long)]
        events: bool,
    },
    /// Time the engine on a benchmark scenario (doc 02 §16).
    Bench {
        #[arg(long)]
        scenario: PathBuf,
        #[arg(long, default_value = DEFAULT_CONTENT)]
        content: PathBuf,
        #[arg(long, default_value_t = 6_000)]
        ticks: u64,
        /// Fail when the average tick takes longer.
        #[arg(long, default_value_t = 6.0)]
        max_ms: f64,
    },
    /// Run golden scenarios and check their bands (doc 08 §3).
    Golden {
        files: Vec<PathBuf>,
        #[arg(long, default_value = DEFAULT_CONTENT)]
        content: PathBuf,
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
    /// Scan text files for real names on the denylist (doc 11 §3.1); fails on any hit.
    Denylist {
        /// Folders or files to scan.
        #[arg(default_values = ["content", "scenarios", "crates", "apps", "docs"])]
        paths: Vec<PathBuf>,
        #[arg(long, default_value = "content/denylist.txt")]
        list: PathBuf,
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
            load,
            content,
            packs,
            ticks,
            save_at,
            save_out,
            print_hash,
            events,
        } => run(RunArgs {
            scenario,
            load,
            content,
            packs,
            ticks,
            save_at,
            save_out,
            print_hash,
            events,
        }),
        Command::Bench {
            scenario,
            content,
            ticks,
            max_ms,
        } => {
            let c = sim::content(&content)?;
            let mut sim = sim::build(&sim::read_scenario(&scenario)?, &c, None)?;
            bench::run(&mut sim, ticks, max_ms)
        }
        Command::Golden { files, content } => golden::run(&files, &sim::content(&content)?),
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
        Command::Content(ContentCommand::Denylist { paths, list }) => denylist(&paths, &list),
        Command::Geo(cmd) => geo::run(cmd),
        Command::Content(ContentCommand::Schema { out }) => {
            let json = serde_json::to_string_pretty(&wb_content::schema::content_schema())?;
            write_file(&out, format!("{json}\n").as_bytes())?;
            eprintln!("{}: written", out.display());
            Ok(())
        }
    }
}

/// Loads and validates content; prints every error before failing, and every
/// archetype band warning (doc 04 §4).
fn load_content(dir: &Path) -> anyhow::Result<wb_content::ContentSet> {
    let set = wb_content::load_dir(dir).map_err(|errors| {
        for e in &errors {
            eprintln!("{e}");
        }
        anyhow::anyhow!("{} content error(s) in {}", errors.len(), dir.display())
    })?;
    for w in &set.warnings {
        eprintln!("warning: {w}");
    }
    Ok(set)
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

struct RunArgs {
    scenario: Option<PathBuf>,
    load: Option<PathBuf>,
    content: PathBuf,
    packs: Vec<PathBuf>,
    ticks: u64,
    save_at: Option<u64>,
    save_out: Option<PathBuf>,
    print_hash: bool,
    events: bool,
}

fn run(a: RunArgs) -> anyhow::Result<()> {
    let content = sim::content(&a.content)?;
    let geo = sim::geo(&a.packs)?;
    let packs: Vec<String> = a.packs.iter().map(|p| p.display().to_string()).collect();
    let (label, scn_content, mut sim) = match (&a.scenario, &a.load) {
        (_, Some(path)) => {
            let bytes =
                std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
            let sim = Sim::load(&bytes, content.db.clone(), content.hash, geo.clone())
                .with_context(|| format!("cannot load {}", path.display()))?;
            (path.display().to_string(), content, sim)
        }
        (Some(path), None) => {
            let scn = sim::read_scenario(path)?;
            let scn_content = match &scn.content {
                Some(c) if a.content == Path::new(DEFAULT_CONTENT) => sim::content(Path::new(c))?,
                _ => content,
            };
            let sim = sim::build(&scn, &scn_content, geo.clone())?;
            (path.display().to_string(), scn_content, sim)
        }
        (None, None) => anyhow::bail!("give --scenario or --load"),
    };

    let started = Instant::now();
    let start_tick = sim.clock().tick;
    if let Some(at) = a.save_at {
        sim.run_ticks(at.saturating_sub(sim.clock().tick));
        let bytes = sim.save(&label, packs.clone(), 0);
        if let Some(out) = &a.save_out {
            write_file(out, &bytes)?;
        }
        sim = Sim::load(
            &bytes,
            scn_content.db.clone(),
            scn_content.hash,
            geo.clone(),
        )
        .context("cannot reload the save")?;
        eprintln!(
            "saved and reloaded at tick {} ({} bytes)",
            sim.clock().tick,
            bytes.len()
        );
    }
    sim.run_ticks(a.ticks.saturating_sub(sim.clock().tick));
    if let (Some(out), None) = (&a.save_out, a.save_at) {
        write_file(out, &sim.save(&label, packs, 0))?;
    }
    let elapsed = started.elapsed();

    let clock = sim.clock();
    let ran = clock.tick - start_tick;
    let per_tick_us = if ran == 0 {
        0.0
    } else {
        elapsed.as_secs_f64() * 1e6 / ran as f64
    };
    eprintln!(
        "{label}: {} ticks, sim time {:.1} s, {:.2} µs per tick",
        clock.tick,
        clock.sim_time_ms as f64 / 1_000.0,
        per_tick_us
    );
    if a.events {
        for e in sim.events() {
            println!("{}", serde_json::to_string(e)?);
        }
    }
    if a.print_hash {
        println!("{}", sim.state_hash_hex());
    }
    Ok(())
}

/// Folders never scanned: build output, dependencies and generated code.
const SKIP_DIRS: [&str; 8] = [
    "target",
    "node_modules",
    "pkg",
    "dist",
    "build",
    "generated",
    ".git",
    ".vite",
];
const TEXT_EXT: [&str; 16] = [
    "rs", "ts", "tsx", "js", "mjs", "cjs", "json", "yaml", "yml", "md", "txt", "toml", "css",
    "html", "sh", "py",
];

fn denylist(paths: &[PathBuf], list: &Path) -> anyhow::Result<()> {
    let text =
        std::fs::read_to_string(list).map_err(|e| anyhow::anyhow!("{}: {e}", list.display()))?;
    let deny = wb_content::denylist::Denylist::parse(&text);
    let mut files = Vec::new();
    for p in paths {
        collect_text(p, true, &mut files);
    }
    files.sort();
    let mut hits = 0usize;
    let skip = std::fs::canonicalize(list).ok();
    for f in &files {
        if std::fs::canonicalize(f).ok() == skip {
            continue;
        }
        let Ok(body) = std::fs::read_to_string(f) else {
            continue;
        };
        for h in deny.find(&body) {
            println!(
                "{}:{}: `{}` is a real name (doc 11 §3.1)",
                f.display().to_string().replace('\\', "/"),
                h.line,
                h.term
            );
            hits += 1;
        }
    }
    eprintln!(
        "denylist: {} terms, {} files, {hits} hits",
        deny.len(),
        files.len()
    );
    if hits > 0 {
        anyhow::bail!("{hits} real names found");
    }
    Ok(())
}

/// Text files under `p`. Build and dependency folders are skipped below the
/// top level; a folder named on the command line (e.g. `apps/game/dist`) is read.
fn collect_text(p: &Path, top: bool, out: &mut Vec<PathBuf>) {
    if p.is_dir() {
        if !top
            && p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| SKIP_DIRS.contains(&n))
        {
            return;
        }
        let Ok(read) = std::fs::read_dir(p) else {
            return;
        };
        let mut entries: Vec<_> = read.filter_map(|e| e.ok().map(|e| e.path())).collect();
        entries.sort();
        for e in entries {
            collect_text(&e, false, out);
        }
    } else if p
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| TEXT_EXT.contains(&e))
    {
        out.push(p.to_path_buf());
    }
}
