//! `wb bench`: engine step time on a benchmark scenario (doc 02 §16, G15).
//!
//! Times every stage of every tick and, at ×10 with 100 ms ticks, builds the
//! side view every 10 ticks (10 Hz real time), as the browser worker does.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};
use wb_sim::Sim;
use wb_sim::view::ViewEncoder;

/// Ticks between side views: 10 Hz real time at ×10 and 100 ms ticks.
const VIEW_EVERY: u64 = 10;

pub fn run(sim: &mut Sim, ticks: u64, max_ms: f64) -> anyhow::Result<()> {
    let mut stages: BTreeMap<&'static str, (usize, Duration)> = BTreeMap::new();
    let mut order: Vec<&'static str> = Vec::new();
    let mut encoder = ViewEncoder::new();
    let mut view = Duration::ZERO;
    let mut view_bytes = 0;
    let units = sim.world().resource::<wb_sim::world::UnitIndex>().0.len();
    let side = sim.sides().sides.iter().position(|s| s.player).unwrap_or(0) as u8;

    let started = Instant::now();
    for t in 0..ticks {
        sim.step_with(|name, run, world| {
            let t0 = Instant::now();
            run(world);
            let e = stages.entry(name).or_insert_with(|| {
                order.push(name);
                (0, Duration::ZERO)
            });
            e.1 += t0.elapsed();
            e.0 += 1;
        });
        if t % VIEW_EVERY == 0 {
            let t0 = Instant::now();
            view_bytes = encoder.encode(sim.world(), side).len();
            view += t0.elapsed();
        }
    }
    let total = started.elapsed();
    let per_tick_ms = total.as_secs_f64() * 1e3 / ticks.max(1) as f64;
    let clock = sim.clock();
    eprintln!(
        "{units} entities, {ticks} ticks of {} ms, sim time {:.0} s",
        clock.tick_len.ms(),
        clock.sim_time_ms as f64 / 1e3
    );
    for name in &order {
        let (_, d) = stages[name];
        eprintln!(
            "  {name:<10} {:>8.3} ms/tick",
            d.as_secs_f64() * 1e3 / ticks.max(1) as f64
        );
    }
    eprintln!(
        "  {:<10} {:>8.3} ms/tick (amortised; {view_bytes} bytes per view)",
        "view",
        view.as_secs_f64() * 1e3 / ticks.max(1) as f64
    );
    println!("average {per_tick_ms:.3} ms per tick (budget {max_ms} ms)");
    if per_tick_ms > max_ms {
        anyhow::bail!("benchmark over budget: {per_tick_ms:.3} ms > {max_ms} ms");
    }
    Ok(())
}
