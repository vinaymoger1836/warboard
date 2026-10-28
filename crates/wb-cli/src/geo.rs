//! `wb geo`: query region packs from `build/packs/`. The output format is shared with
//! `crates/wb-wasm/node/geo-query.mjs`, so `scripts/determinism-check.sh` can compare
//! native and WebAssembly results line by line.

use anyhow::{Context, bail};
use clap::Subcommand;
use std::path::{Path, PathBuf};
use wb_core::geo::LatLon;
use wb_geo::{Geo, LosParams, Pack};

const DEFAULT_PACKS: &str = "build/packs/world,build/packs/europe";

#[derive(Subcommand)]
pub enum GeoCommand {
    /// Describe each pack: extent, resolutions, tile and cell counts.
    Info {
        #[arg(long, default_value = DEFAULT_PACKS, value_delimiter = ',')]
        packs: Vec<PathBuf>,
    },
    /// Owner, terrain class and elevation at points.
    Query {
        #[arg(long, default_value = DEFAULT_PACKS, value_delimiter = ',')]
        packs: Vec<PathBuf>,
        /// `lat,lon` in degrees; repeatable.
        #[arg(long = "at", allow_hyphen_values = true)]
        at: Vec<String>,
        /// File of probes: `lat lon [label]` per line, `#` comments.
        #[arg(long)]
        probes: Option<PathBuf>,
    },
    /// Terrain line of sight between two points (altitudes above sea level).
    Los {
        #[arg(long, default_value = DEFAULT_PACKS, value_delimiter = ',')]
        packs: Vec<PathBuf>,
        /// `lat,lon,alt_m`
        #[arg(long, allow_hyphen_values = true)]
        from: String,
        /// `lat,lon,alt_m`
        #[arg(long, allow_hyphen_values = true)]
        to: String,
        /// Effective Earth radius factor (realism `earth.radio_k_factor`).
        #[arg(long, default_value_t = 4.0 / 3.0)]
        k: f64,
        /// Sample spacing, metres.
        #[arg(long, default_value_t = 100.0)]
        step: f64,
    },
}

pub fn run(cmd: GeoCommand) -> anyhow::Result<()> {
    match cmd {
        GeoCommand::Info { packs } => {
            for line in wb_geo::info_lines(&open(&packs)?) {
                println!("{line}");
            }
            Ok(())
        }
        GeoCommand::Query { packs, at, probes } => {
            let geo = open(&packs)?;
            let mut points = Vec::new();
            for a in &at {
                let v = numbers(a, 2)?;
                points.push((v[0], v[1], String::new()));
            }
            if let Some(path) = probes {
                points.extend(read_probes(&path)?);
            }
            for (lat, lon, label) in points {
                println!("{}", wb_geo::query_line(&geo, lat, lon, &label)?);
            }
            Ok(())
        }
        GeoCommand::Los {
            packs,
            from,
            to,
            k,
            step,
        } => {
            let geo = open(&packs)?;
            let (a, b) = (numbers(&from, 3)?, numbers(&to, 3)?);
            let los = geo.line_of_sight(
                LatLon::from_deg(a[0], a[1]),
                a[2],
                LatLon::from_deg(b[0], b[1]),
                b[2],
                &LosParams {
                    k_factor: k,
                    step_m: step,
                },
            )?;
            println!("{}", wb_geo::los_line(&los));
            Ok(())
        }
    }
}

pub fn open(dirs: &[PathBuf]) -> anyhow::Result<Geo> {
    let mut geo = Geo::new();
    for dir in dirs {
        let pack =
            Pack::open_dir(dir).with_context(|| format!("cannot open pack {}", dir.display()))?;
        geo.add_pack(pack)?;
    }
    Ok(geo)
}

fn numbers(s: &str, n: usize) -> anyhow::Result<Vec<f64>> {
    let v: Vec<f64> = s
        .split(',')
        .map(|x| x.trim().parse::<f64>())
        .collect::<Result<_, _>>()
        .with_context(|| format!("expected {n} comma-separated numbers, got `{s}`"))?;
    if v.len() != n {
        bail!("expected {n} comma-separated numbers, got `{s}`");
    }
    Ok(v)
}

fn read_probes(path: &Path) -> anyhow::Result<Vec<(f64, f64, String)>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let parse = |x: Option<&str>| -> anyhow::Result<f64> {
            x.and_then(|v| v.parse().ok()).with_context(|| {
                format!("{}:{}: expected `lat lon [label]`", path.display(), i + 1)
            })
        };
        let (lat, lon) = (parse(parts.next())?, parse(parts.next())?);
        out.push((lat, lon, parts.collect::<Vec<_>>().join(" ")));
    }
    Ok(out)
}
