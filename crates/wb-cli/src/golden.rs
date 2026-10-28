//! `wb golden`: golden scenarios (doc 08 §3). A golden file is a scenario
//! with a `golden:` block of variants and expected bands; every metric comes
//! from the event log.
//!
//! ```yaml
//! golden:
//!   id: G7
//!   max_ticks: 86400
//!   variants:
//!     no_tanker: { drop_units: [30] }
//!     tanker: {}
//!   expect:
//!     - { variant: no_tanker, metric: landed_reserve_margin_kg, group: 10, min: 0 }
//!     - { variant: tanker, metric: time_on_station_s, group: 10, ratio_to: no_tanker, min: 2.0 }
//! ```
//!
//! A run stops when every group named in `expect` has landed once (after
//! taking off) or at `max_ticks`. Metrics count up to that first landing.

use anyhow::{Context, bail};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;
use wb_core::UnitId;
use wb_sim::components::Group;
use wb_sim::orders::Command;
use wb_sim::{Event, EventKind, Scenario, Sim};

use crate::sim::Content;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    id: String,
    #[serde(default)]
    description: Option<String>,
    max_ticks: u64,
    #[serde(default)]
    variants: BTreeMap<String, Variant>,
    expect: Vec<Expect>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Variant {
    /// Scenario units (and the orders that name only them) left out.
    #[serde(default)]
    drop_units: Vec<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Expect {
    #[serde(default = "base_variant")]
    variant: String,
    metric: Metric,
    group: u32,
    /// Divide by the same metric of this variant before comparing.
    #[serde(default)]
    ratio_to: Option<String>,
    #[serde(default)]
    min: Option<f64>,
    #[serde(default)]
    max: Option<f64>,
}

fn base_variant() -> String {
    "base".into()
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum Metric {
    /// Seconds on station before the first landing.
    TimeOnStationS,
    /// Lowest (landing fuel − reserve recorded at bingo) over the members.
    LandedReserveMarginKg,
    /// Bingo calls by members before the first landing.
    BingoCalls,
    /// Refuellings completed by members before the first landing.
    Refuels,
}

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
    let spec: Spec = serde_yaml::from_value(
        scenario
            .golden
            .clone()
            .with_context(|| format!("{} has no golden block", path.display()))?,
    )
    .with_context(|| format!("{}: golden block is invalid", path.display()))?;
    println!(
        "{} {}{}",
        spec.id,
        path.display(),
        spec.description
            .as_deref()
            .map(|d| format!(" — {d}"))
            .unwrap_or_default()
    );

    let mut variants = spec.variants.keys().cloned().collect::<Vec<_>>();
    if variants.is_empty() {
        variants.push(base_variant());
    }
    let groups: Vec<u32> = {
        let mut g: Vec<u32> = spec.expect.iter().map(|e| e.group).collect();
        g.sort();
        g.dedup();
        g
    };
    let mut results: BTreeMap<(String, Metric, u32), Option<f64>> = BTreeMap::new();
    for v in &variants {
        let mut s = scenario.clone();
        if let Some(var) = spec.variants.get(v) {
            apply_variant(&mut s, var);
        }
        let mut sim = crate::sim::build(&s, content, None)?;
        let members = members_of(&sim, &groups);
        run_until_landed(&mut sim, &members, spec.max_ticks);
        let ev = sim.events();
        for e in &spec.expect {
            for (var, metric) in [
                Some((&e.variant, e.metric)),
                e.ratio_to.as_ref().map(|r| (r, e.metric)),
            ]
            .into_iter()
            .flatten()
            {
                if var == v {
                    let m = &members[&e.group];
                    results.insert(
                        (v.clone(), metric, e.group),
                        metric_value(ev, UnitId(e.group), m, metric),
                    );
                }
            }
        }
        println!(
            "  run {v}: {} ticks, {:.1} h sim time, hash {}",
            sim.clock().tick,
            sim.clock().sim_time_ms as f64 / 3.6e6,
            &sim.state_hash_hex()[..16]
        );
    }

    let mut failed = 0;
    for e in &spec.expect {
        let value = results
            .get(&(e.variant.clone(), e.metric, e.group))
            .copied()
            .flatten();
        let value = match (&e.ratio_to, value) {
            (Some(r), Some(v)) => results
                .get(&(r.clone(), e.metric, e.group))
                .copied()
                .flatten()
                .filter(|d| *d > 0.0)
                .map(|d| v / d),
            (_, v) => v,
        };
        let ok =
            value.is_some_and(|v| e.min.is_none_or(|m| v >= m) && e.max.is_none_or(|m| v <= m));
        let what = match &e.ratio_to {
            Some(r) => format!("{:?}({}) / {:?}({r})", e.metric, e.variant, e.metric),
            None => format!("{:?}({})", e.metric, e.variant),
        };
        println!(
            "  {} group {} {what} = {} (band {}..{})",
            if ok { "pass" } else { "FAIL" },
            e.group,
            value.map_or("missing".into(), |v| format!("{v:.2}")),
            e.min.map_or("".into(), |v| v.to_string()),
            e.max.map_or("".into(), |v| v.to_string()),
        );
        if !ok {
            failed += 1;
        }
    }
    Ok(failed)
}

fn apply_variant(s: &mut Scenario, v: &Variant) {
    s.units.retain(|u| !v.drop_units.contains(&u.id));
    s.orders.retain(|o| match &o.command {
        Command::AssignTask { units, .. } | Command::SetBehaviour { units, .. } => {
            !units.iter().all(|u| v.drop_units.contains(u))
        }
        _ => true,
    });
}

fn members_of(sim: &Sim, groups: &[u32]) -> BTreeMap<u32, Vec<UnitId>> {
    let world = sim.world();
    groups
        .iter()
        .map(|g| {
            let m = wb_sim::world::entity(world, UnitId(*g))
                .and_then(|e| world.get::<Group>(e))
                .map(|g| g.members.clone())
                .unwrap_or_default();
            (*g, m)
        })
        .collect()
}

/// Index of the event that ends the measured sortie: the event by which every
/// member has landed once after taking off.
fn first_landing(events: &[Event], members: &[UnitId]) -> Option<usize> {
    members
        .iter()
        .map(|m| {
            let mut airborne = false;
            events.iter().position(|e| match &e.kind {
                EventKind::TookOff { unit, .. } if unit == m => {
                    airborne = true;
                    false
                }
                EventKind::Landed { unit, .. } => airborne && unit == m,
                _ => false,
            })
        })
        .try_fold(0, |acc, i| i.map(|i| acc.max(i)))
}

fn run_until_landed(sim: &mut Sim, members: &BTreeMap<u32, Vec<UnitId>>, max_ticks: u64) {
    let mut checked = 0;
    while sim.clock().tick < max_ticks {
        sim.run_ticks(60);
        let events = sim.events();
        if events.len() != checked {
            checked = events.len();
            if members.values().all(|m| first_landing(events, m).is_some()) {
                return;
            }
        }
    }
}

fn metric_value(
    events: &[Event],
    group: UnitId,
    members: &[UnitId],
    metric: Metric,
) -> Option<f64> {
    let end = first_landing(events, members);
    let window = &events[..end.map_or(events.len(), |i| i + 1)];
    match metric {
        Metric::TimeOnStationS => {
            let mut total = 0u64;
            let mut since = None;
            for e in window {
                if let EventKind::StationChanged { group: g, on } = e.kind
                    && g == group
                {
                    match (on, since) {
                        (true, None) => since = Some(e.sim_time_ms),
                        (false, Some(s)) => {
                            total += e.sim_time_ms - s;
                            since = None;
                        }
                        _ => {}
                    }
                }
            }
            if let (Some(s), Some(last)) = (since, window.last()) {
                total += last.sim_time_ms - s;
            }
            Some(total as f64 / 1_000.0)
        }
        Metric::LandedReserveMarginKg => {
            end?;
            let mut worst: Option<f64> = None;
            for m in members {
                let reserve = window.iter().rev().find_map(|e| match e.kind {
                    EventKind::BingoRtb {
                        unit, reserve_kg, ..
                    } if unit == *m => Some(reserve_kg),
                    _ => None,
                })?;
                let landed = window.iter().rev().find_map(|e| match e.kind {
                    EventKind::Landed { unit, fuel_kg, .. } if unit == *m => Some(fuel_kg),
                    _ => None,
                })?;
                let margin = landed - reserve;
                worst = Some(worst.map_or(margin, |w: f64| w.min(margin)));
            }
            worst
        }
        Metric::BingoCalls => Some(
            window
                .iter()
                .filter(|e| matches!(e.kind, EventKind::BingoRtb { unit, .. } if members.contains(&unit)))
                .count() as f64,
        ),
        Metric::Refuels => Some(
            window
                .iter()
                .filter(|e| matches!(e.kind, EventKind::Refuelled { unit, .. } if members.contains(&unit)))
                .count() as f64,
        ),
    }
}
