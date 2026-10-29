//! `wb golden`: golden scenarios (doc 08 §3). A golden file is a scenario
//! with a `golden:` block of variants and expected bands. Flight metrics come
//! from the event log; sensor metrics from the side pictures, which the CLI
//! may read (it is a test harness, not a client).
//!
//! ```yaml
//! golden:
//!   id: G7
//!   max_ticks: 86400
//!   seeds: 1                       # runs per variant, seeds seed, seed+1, …
//!   variants:
//!     no_tanker: { drop_units: [30] }
//!     tanker: {}
//!   expect:
//!     - { variant: no_tanker, metric: landed_reserve_margin_kg, group: 10, min: 0 }
//!     - { variant: tanker, metric: time_on_station_s, group: 10, ratio_to: no_tanker, min: 2.0 }
//!     - { metric: first_detection_range_km, side: 0, target: 900, aggregate: min, min: 28 }
//! ```
//!
//! A run stops when every group named by a flight metric has landed once
//! (after taking off), when every sensor metric has its value, or at
//! `max_ticks`. Flight metrics count up to that first landing.

use anyhow::{Context, bail};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;
use wb_core::UnitId;
use wb_core::math::pow;
use wb_sim::components::{Group, GroupMember, Kinematics, PlatformRef};
use wb_sim::orders::Command;
use wb_sim::picture::SidePictures;
use wb_sim::{Event, EventKind, Scenario, Sim};

use crate::sim::Content;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    id: String,
    #[serde(default)]
    description: Option<String>,
    max_ticks: u64,
    #[serde(default = "one")]
    seeds: u64,
    #[serde(default)]
    variants: BTreeMap<String, Variant>,
    expect: Vec<Expect>,
}

fn one() -> u64 {
    1
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Variant {
    /// Scenario units (and the orders that name only them) left out.
    #[serde(default)]
    drop_units: Vec<u32>,
    /// Scripted targets left out.
    #[serde(default)]
    drop_targets: Vec<u32>,
    /// Units whose platform (and country) is swapped, so one target id
    /// stands for different types across variants (G2).
    #[serde(default)]
    replace: Vec<Replace>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Replace {
    unit: u32,
    platform: String,
    #[serde(default)]
    country: Option<String>,
}

/// One measured quantity of a run.
// No `deny_unknown_fields` here or on `Expect`: serde cannot combine it with `flatten`.
#[derive(Debug, Clone, Deserialize, PartialEq)]
struct Measure {
    metric: Metric,
    #[serde(default)]
    group: Option<u32>,
    /// Side whose picture is read.
    #[serde(default)]
    side: Option<u8>,
    /// Unit or scripted target (its group is the contact).
    #[serde(default)]
    target: Option<u32>,
    /// Own sensor unit, for line-of-sight metrics.
    #[serde(default)]
    observer: Option<u32>,
    #[serde(default)]
    satellite: Option<u32>,
    #[serde(default)]
    imaged: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct Expect {
    #[serde(default = "base_variant")]
    variant: String,
    #[serde(flatten)]
    what: Measure,
    /// Divide by the same metric of this variant before comparing.
    #[serde(default)]
    ratio_to: Option<String>,
    /// Divide by another metric of the same variant.
    #[serde(default)]
    over: Option<Measure>,
    /// Divide by `(σ_a / σ_b)^¼` from the content front RCS of two platforms (G2).
    #[serde(default)]
    rcs_quarter_root: Option<[String; 2]>,
    /// How runs over seeds combine (default: median).
    #[serde(default)]
    aggregate: Aggregate,
    #[serde(default)]
    min: Option<f64>,
    #[serde(default)]
    max: Option<f64>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Aggregate {
    #[default]
    Median,
    Min,
    Max,
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
    /// True range at the side's first detection of the target, km.
    FirstDetectionRangeKm,
    /// Terrain clearance of the exact line of sight observer → target at the
    /// side's first detection, metres (negative: blocked).
    ClearanceAtFirstDetectionM,
    /// Seconds from the exact line of sight clearing to first detection.
    FirstDetectionAfterLosClearS,
    /// Intel reports from a satellite (optionally only `imaged` ones).
    ReportCount,
}

impl Metric {
    fn is_flight(self) -> bool {
        matches!(
            self,
            Metric::TimeOnStationS
                | Metric::LandedReserveMarginKg
                | Metric::BingoCalls
                | Metric::Refuels
        )
    }
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

fn key(m: &Measure) -> String {
    format!("{m:?}")
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
    // Every measure each variant needs.
    let mut needs: BTreeMap<String, Vec<Measure>> = BTreeMap::new();
    for e in &spec.expect {
        let mut add = |v: &str, m: &Measure| {
            let list = needs.entry(v.to_owned()).or_default();
            if !list.contains(m) {
                list.push(m.clone());
            }
        };
        add(&e.variant, &e.what);
        if let Some(r) = &e.ratio_to {
            add(r, &e.what);
        }
        if let Some(o) = &e.over {
            add(&e.variant, o);
        }
    }
    // (variant, measure) → value per seed.
    let mut results: BTreeMap<(String, String), Vec<Option<f64>>> = BTreeMap::new();
    for v in &variants {
        let measures = needs.get(v).cloned().unwrap_or_default();
        for n in 0..spec.seeds.max(1) {
            let mut s = scenario.clone();
            s.seed = scenario.seed.wrapping_add(n);
            if let Some(var) = spec.variants.get(v) {
                apply_variant(&mut s, var);
            }
            let mut sim = crate::sim::build(&s, content, None)?;
            let values = measure_run(&mut sim, &measures, spec.max_ticks);
            for (m, val) in measures.iter().zip(values) {
                results.entry((v.clone(), key(m))).or_default().push(val);
            }
            if n == 0 {
                println!(
                    "  run {v}: {} ticks, {:.1} h sim time, hash {}{}",
                    sim.clock().tick,
                    sim.clock().sim_time_ms as f64 / 3.6e6,
                    &sim.state_hash_hex()[..16],
                    if spec.seeds > 1 {
                        format!(" (seed 1 of {})", spec.seeds)
                    } else {
                        String::new()
                    }
                );
            }
        }
    }

    let mut failed = 0;
    for e in &spec.expect {
        let get = |v: &str, m: &Measure| {
            results
                .get(&(v.to_owned(), key(m)))
                .cloned()
                .unwrap_or_default()
        };
        let mut vals = get(&e.variant, &e.what);
        let combine = |a: Vec<Option<f64>>, b: Vec<Option<f64>>| -> Vec<Option<f64>> {
            a.iter()
                .zip(b.iter().chain(std::iter::repeat(&None)))
                .map(|(x, y)| match (x, y) {
                    (Some(x), Some(y)) if *y != 0.0 => Some(x / y),
                    _ => None,
                })
                .collect()
        };
        if let Some(o) = &e.over {
            vals = combine(vals, get(&e.variant, o));
        }
        let mut value = aggregate(&vals, e.aggregate);
        if let Some(r) = &e.ratio_to {
            let other = aggregate(&get(r, &e.what), e.aggregate);
            value = match (value, other) {
                (Some(v), Some(d)) if d > 0.0 => Some(v / d),
                _ => None,
            };
        }
        if let Some([a, b]) = &e.rcs_quarter_root {
            let front = |id: &str| {
                content
                    .db
                    .platforms
                    .iter()
                    .find(|p| content.db.str(p.id) == id)
                    .map(|p| p.signature.rcs_m2.front)
            };
            let q = match (front(a), front(b)) {
                (Some(x), Some(y)) if y > 0.0 => pow(x / y, 0.25),
                _ => bail!("{}: unknown platform in rcs_quarter_root", spec.id),
            };
            value = value.map(|v| v / q);
        }
        let ok =
            value.is_some_and(|v| e.min.is_none_or(|m| v >= m) && e.max.is_none_or(|m| v <= m));
        let mut what = format!("{:?}({})", e.what.metric, e.variant);
        if let Some(o) = &e.over {
            what += &format!(" / {:?}", o.metric);
        }
        if let Some(r) = &e.ratio_to {
            what += &format!(" / {:?}({r})", e.what.metric);
        }
        if e.rcs_quarter_root.is_some() {
            what += " / (σ₁/σ₂)^¼";
        }
        if spec.seeds > 1 {
            what += &format!(" [{:?} of {}]", e.aggregate, spec.seeds);
        }
        let subject = match (e.what.group, e.what.target, e.what.satellite) {
            (Some(g), _, _) => format!("group {g}"),
            (_, Some(t), _) => format!("target {t}"),
            (_, _, Some(s)) => format!("satellite {s}"),
            _ => String::new(),
        };
        println!(
            "  {} {subject} {what} = {} (band {}..{})",
            if ok { "pass" } else { "FAIL" },
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

/// Combines per-seed values; a missing value makes the result missing.
fn aggregate(vals: &[Option<f64>], how: Aggregate) -> Option<f64> {
    let mut v: Vec<f64> = vals.iter().copied().collect::<Option<Vec<_>>>()?;
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    Some(match how {
        Aggregate::Min => v[0],
        Aggregate::Max => v[v.len() - 1],
        Aggregate::Median => {
            let n = v.len();
            if n % 2 == 1 {
                v[n / 2]
            } else {
                (v[n / 2 - 1] + v[n / 2]) / 2.0
            }
        }
    })
}

fn apply_variant(s: &mut Scenario, v: &Variant) {
    s.units.retain(|u| !v.drop_units.contains(&u.id));
    s.scripted_targets
        .retain(|t| !v.drop_targets.contains(&t.id));
    for r in &v.replace {
        for u in s.units.iter_mut().filter(|u| u.id == r.unit) {
            u.platform = Some(r.platform.clone());
            if let Some(c) = &r.country {
                u.country = Some(c.clone());
            }
        }
    }
    s.orders.retain(|o| match &o.command {
        Command::AssignTask { units, .. }
        | Command::SetBehaviour { units, .. }
        | Command::SetEmcon { units, .. } => !units.iter().all(|u| v.drop_units.contains(u)),
        Command::TaskSatellite { satellite, .. } => !v.drop_units.contains(satellite),
        _ => true,
    });
}

fn members_of(sim: &Sim, group: u32) -> Vec<UnitId> {
    let world = sim.world();
    wb_sim::world::entity(world, UnitId(group))
        .and_then(|e| world.get::<Group>(e))
        .map(|g| g.members.clone())
        .unwrap_or_default()
}

/// Association key of a target: its group, or itself.
fn truth_key(sim: &Sim, target: u32) -> UnitId {
    let world = sim.world();
    wb_sim::world::entity(world, UnitId(target))
        .and_then(|e| world.get::<GroupMember>(e))
        .map_or(UnitId(target), |m| m.group)
}

/// The side's contact of a target: (first detection tick, true range).
fn first_seen(sim: &Sim, side: u8, target: u32) -> Option<(u64, f64)> {
    let key = truth_key(sim, target);
    let pics = sim.world().get_resource::<SidePictures>()?;
    let pic = pics.sides.get(usize::from(side))?;
    let c = pic.get(*pic.by_truth.get(&key)?)?;
    Some((c.first_seen.tick, c.first_seen.range_m))
}

/// Position and line-of-sight height of a unit: a lead or a member of
/// `id` (groups use their lead).
fn place(sim: &Sim, id: u32) -> Option<(wb_core::geo::LatLon, f64, Option<f64>)> {
    let world = sim.world();
    let mut e = wb_sim::world::entity(world, UnitId(id))?;
    if let Some(g) = world.get::<Group>(e) {
        e = wb_sim::world::entity(world, *g.members.first()?)?;
    }
    let k = world.get::<Kinematics>(e)?;
    let rules = world.resource::<wb_sim::Rules>();
    let mount = world
        .get::<PlatformRef>(e)
        .and_then(|p| rules.platform(p.0).sensors.first().map(|m| m.height_m));
    Some((k.pos(), k.alt_m, mount))
}

/// Exact clearance of the line of sight from `observer` (ground + its first
/// sensor's mount) to `target`, metres.
fn clearance(sim: &Sim, observer: u32, target: u32) -> Option<f64> {
    let geo = sim.geo()?;
    let (a, alt_a, mount) = place(sim, observer)?;
    let (b, alt_b, _) = place(sim, target)?;
    let ground = geo.elevation_m(a).ok()?;
    let h_a = alt_a.max(ground) + mount.unwrap_or(0.0);
    let real = sim.world().resource::<wb_sim::Rules>().realism().clone();
    let d = wb_core::geo::distance_m(a, b);
    let p = wb_geo::LosParams {
        k_factor: real.earth.radio_k_factor,
        step_m: (d / real.radar.los_sample_divisor).max(30.0),
    };
    geo.line_of_sight(a, h_a, b, alt_b, &p)
        .ok()
        .map(|l| l.min_clearance_m)
}

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

/// Runs one scenario and returns each measure.
fn measure_run(sim: &mut Sim, measures: &[Measure], max_ticks: u64) -> Vec<Option<f64>> {
    let groups: BTreeMap<u32, Vec<UnitId>> = measures
        .iter()
        .filter(|m| m.metric.is_flight())
        .filter_map(|m| m.group)
        .map(|g| (g, members_of(sim, g)))
        .collect();
    let los: Vec<&Measure> = measures
        .iter()
        .filter(|m| {
            matches!(
                m.metric,
                Metric::ClearanceAtFirstDetectionM | Metric::FirstDetectionAfterLosClearS
            )
        })
        .collect();
    let sensing: Vec<&Measure> = measures
        .iter()
        .filter(|m| !m.metric.is_flight() && m.metric != Metric::ReportCount)
        .collect();
    let reports_only = measures.iter().all(|m| m.metric == Metric::ReportCount);
    // Line of sight per (observer, target): clearance each tick and the tick it cleared.
    let mut cleared: BTreeMap<(u32, u32), Option<u64>> = BTreeMap::new();
    let mut history: BTreeMap<(u32, u32), BTreeMap<u64, f64>> = BTreeMap::new();
    let mut checked = 0;
    let mut flight_done = groups.is_empty();
    while sim.clock().tick < max_ticks {
        let step = if los.is_empty() { 10 } else { 1 };
        sim.run_ticks(step);
        let tick = sim.clock().tick;
        for m in &los {
            let (Some(o), Some(t), Some(side)) = (m.observer, m.target, m.side) else {
                continue;
            };
            let c = clearance(sim, o, t);
            let entry = cleared.entry((o, t)).or_insert(None);
            if entry.is_none() && c.is_some_and(|c| c >= 0.0) {
                *entry = Some(tick);
            }
            if let Some(c) = c {
                // Positions after tick `tick - 1` ran: what its sensors saw.
                history.entry((o, t)).or_default().insert(tick - 1, c);
            }
            let _ = side;
        }
        if reports_only {
            continue;
        }
        let events = sim.events();
        if events.len() != checked {
            checked = events.len();
            flight_done = groups.values().all(|m| first_landing(events, m).is_some());
        }
        let sensing_done = sensing.iter().all(|m| match (m.side, m.target) {
            (Some(s), Some(t)) => first_seen(sim, s, t).is_some(),
            _ => true,
        });
        if flight_done && sensing_done {
            break;
        }
    }
    let at_detect: BTreeMap<(u32, u32), f64> = los
        .iter()
        .filter_map(|m| {
            let (o, t) = (m.observer?, m.target?);
            let (ft, _) = first_seen(sim, m.side?, t)?;
            let c = history.get(&(o, t))?.range(..=ft).next_back()?.1;
            Some(((o, t), *c))
        })
        .collect();
    measures
        .iter()
        .map(|m| value(sim, m, &groups, &cleared, &at_detect))
        .collect()
}

fn value(
    sim: &Sim,
    m: &Measure,
    groups: &BTreeMap<u32, Vec<UnitId>>,
    cleared: &BTreeMap<(u32, u32), Option<u64>>,
    at_detect: &BTreeMap<(u32, u32), f64>,
) -> Option<f64> {
    let events = sim.events();
    match m.metric {
        Metric::FirstDetectionRangeKm => {
            first_seen(sim, m.side?, m.target?).map(|(_, r)| r / 1_000.0)
        }
        Metric::ClearanceAtFirstDetectionM => at_detect.get(&(m.observer?, m.target?)).copied(),
        Metric::FirstDetectionAfterLosClearS => {
            let (t, _) = first_seen(sim, m.side?, m.target?)?;
            let c = (*cleared.get(&(m.observer?, m.target?))?)?;
            // Coarse ticks are 1 s.
            Some(t as f64 - c as f64)
        }
        Metric::ReportCount => {
            let sat = UnitId(m.satellite?);
            Some(
                events
                    .iter()
                    .filter(|e| {
                        matches!(&e.kind, EventKind::IntelReport { satellite, imaged, .. }
                        if *satellite == sat && m.imaged.is_none_or(|i| i == *imaged))
                    })
                    .count() as f64,
            )
        }
        _ => flight_value(events, UnitId(m.group?), groups.get(&m.group?)?, m.metric),
    }
}

fn flight_value(
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
        _ => None,
    }
}
