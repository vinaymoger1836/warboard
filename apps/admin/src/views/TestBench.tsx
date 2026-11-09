// Test bench (doc 04 §5, doc 02 §19): runs a scenario/bench/*.yaml N seeds per variant
// against the content as edited (compiled in the browser) and charts the outcome
// distribution of every expectation against its band.
import { useEffect, useRef, useState } from 'react';
import { useStore } from '../store';
import { Bench, type BenchCheck, type BenchInfo, type BenchReport, buildPack } from '../wasm';

/** Scenarios under scenarios/bench/ that have a `golden:` block (the others are speed benchmarks). */
function benchFiles(files: Record<string, string>): string[] {
  return Object.keys(files)
    .filter((n) => /^golden:/m.test(files[n] ?? ''))
    .sort();
}

type Phase = 'idle' | 'building' | 'running' | 'done' | 'stopped' | 'failed';

export function TestBench() {
  const bench = useStore((s) => s.bench);
  const files = useStore((s) => s.files);
  const names = benchFiles(bench);
  const [pick, setPick] = useState<string | null>(null);
  const current = pick && names.includes(pick) ? pick : (names[0] ?? null);
  const [seeds, setSeeds] = useState(0);
  const [phase, setPhase] = useState<Phase>('idle');
  const [message, setMessage] = useState('');
  const [info, setInfo] = useState<BenchInfo | null>(null);
  const [left, setLeft] = useState(0);
  const [report, setReport] = useState<BenchReport | null>(null);
  const stop = useRef(false);
  const live = useRef<Bench | null>(null);

  useEffect(
    () => () => {
      stop.current = true;
      live.current?.free();
    },
    [],
  );

  const run = async () => {
    if (!current) return;
    const yaml = bench[current];
    if (yaml === undefined) return;
    stop.current = false;
    live.current?.free();
    live.current = null;
    setReport(null);
    setPhase('building');
    setMessage('Compiling the content as edited…');
    try {
      const { bytes } = await buildPack(files);
      const b = await Bench.create(yaml, bytes, seeds);
      live.current = b;
      const i = b.info();
      setInfo(i);
      const total = b.remaining();
      setLeft(total);
      setPhase('running');
      setMessage('');
      // A few runs per slice, so the page stays responsive and Stop works.
      while (b.remaining() > 0 && !stop.current) {
        b.step(2);
        setLeft(b.remaining());
        await new Promise((r) => setTimeout(r, 0));
      }
      setReport(b.report());
      setPhase(stop.current ? 'stopped' : 'done');
    } catch (e) {
      setPhase('failed');
      setMessage(String(e instanceof Error ? e.message : e));
    }
  };

  if (names.length === 0) {
    return (
      <p className="hint page">
        No test-bench scenarios. Add a scenario with a <span className="mono">golden:</span> block to{' '}
        <span className="mono">scenarios/bench/</span>.
      </p>
    );
  }
  const total = info ? info.variants.length * info.seeds : 0;
  const done = total - left;
  const failed = report?.checks.filter((c) => !c.ok).length ?? 0;
  return (
    <div className="split">
      <aside className="list-pane">
        <div className="count dim">Test-bench scenarios</div>
        <ul className="entries">
          {names.map((n) => (
            <li key={n} className={n === current ? 'sel' : ''} onClick={() => (setPick(n), setReport(null), setInfo(null), setPhase('idle'))}>
              <span className="mono name">{n}</span>
            </li>
          ))}
        </ul>
      </aside>
      <section className="detail-pane">
        <header className="card-head">
          <div>
            <h2 className="mono">{current}</h2>
            <div className="sub">
              Runs against the content as edited (unsaved changes included). Results are measured by the engine and never leave this page.
            </div>
          </div>
          <div className="actions">
            <label className="inline">
              Seeds per variant
              <input
                type="number"
                min={0}
                value={seeds || ''}
                placeholder="scenario"
                onChange={(e) => setSeeds(Math.max(0, Math.floor(Number(e.target.value) || 0)))}
              />
            </label>
            {phase === 'running' || phase === 'building' ? (
              <button className="cmd small" onClick={() => (stop.current = true)}>
                STOP
              </button>
            ) : (
              <button className="cmd small" onClick={() => void run()}>
                RUN
              </button>
            )}
          </div>
        </header>
        {info?.description && <p className="hint">{info.description}</p>}
        {message && <p className={phase === 'failed' ? 'bad' : 'hint'}>{message}</p>}
        {info && phase !== 'idle' && (
          <div className="progress" title={`${done} of ${total} runs`}>
            <div className="progress-fill" style={{ width: `${total ? (100 * done) / total : 0}%` }} />
          </div>
        )}
        {info && (
          <p className="hint">
            {done} of {total} runs · {info.variants.length} variant(s) · up to {info.max_ticks} ticks each
            {phase === 'stopped' && ' · stopped: the results below use the runs finished so far'}
          </p>
        )}
        {report && (
          <>
            <p className={failed === 0 ? 'ok' : 'bad'}>
              {failed === 0 ? `All ${report.checks.length} expectations within their bands.` : `${failed} of ${report.checks.length} expectations outside their band.`}
            </p>
            {report.checks.map((c, i) => (
              <CheckCard key={i} c={c} />
            ))}
          </>
        )}
      </section>
    </div>
  );
}

function fmt(v: number | null): string {
  if (v === null) return '—';
  const a = Math.abs(v);
  return a !== 0 && (a >= 10000 || a < 0.01) ? v.toExponential(2) : String(Math.round(v * 1000) / 1000);
}

/** One expectation: its label, the aggregate against the band, and a histogram of the per-seed values. */
function CheckCard({ c }: { c: BenchCheck }) {
  const vals = c.values.filter((v): v is number => v !== null);
  const missing = c.values.length - vals.length;
  return (
    <div className={`check-card ${c.ok ? 'pass' : 'fail'}`}>
      <div className="check-head">
        <span className={`pill ${c.ok ? 'ok' : 'bad'}`}>{c.ok ? 'in band' : 'outside'}</span>
        <span className="mono">{c.variant}</span>
        <span className="mono dim">
          {c.metric}
          {c.subject && ` · ${c.subject}`}
        </span>
      </div>
      <div className="check-label">{c.label}</div>
      <Histogram vals={vals} min={c.min} max={c.max} value={c.value} />
      <div className="mono dim check-foot">
        {c.aggregate} = {fmt(c.value)} · {vals.length} run(s)
        {missing > 0 && ` · ${missing} without a value`}
      </div>
    </div>
  );
}

const W = 420;
const H = 70;

function Histogram({ vals, min, max, value }: { vals: number[]; min: number | null; max: number | null; value: number | null }) {
  if (vals.length === 0) return <p className="hint">No run produced a value.</p>;
  const marks = [...vals, ...(min !== null ? [min] : []), ...(max !== null ? [max] : []), ...(value !== null ? [value] : [])];
  let lo = Math.min(...marks);
  let hi = Math.max(...marks);
  if (hi - lo < 1e-9) {
    lo -= 0.5;
    hi += 0.5;
  }
  const pad = (hi - lo) * 0.05;
  lo -= pad;
  hi += pad;
  const bins = Math.min(30, Math.max(5, Math.ceil(Math.sqrt(vals.length))));
  const counts = new Array<number>(bins).fill(0);
  for (const v of vals) counts[Math.min(bins - 1, Math.floor(((v - lo) / (hi - lo)) * bins))]! += 1;
  const peak = Math.max(...counts);
  const x = (v: number) => ((v - lo) / (hi - lo)) * W;
  const bw = W / bins;
  return (
    <svg className="hist" viewBox={`0 0 ${W} ${H + 16}`} role="img" aria-label="Distribution of the per-seed values">
      <rect
        className="hist-band"
        x={x(min ?? lo)}
        width={Math.max(1, x(max ?? hi) - x(min ?? lo))}
        y={0}
        height={H}
      />
      {counts.map((n, i) => (
        <rect key={i} className="hist-bar" x={i * bw + 1} width={Math.max(1, bw - 2)} y={H - (n / peak) * H} height={(n / peak) * H} />
      ))}
      {value !== null && <line className="hist-value" x1={x(value)} x2={x(value)} y1={0} y2={H} />}
      <line className="hist-axis" x1={0} x2={W} y1={H} y2={H} />
      <text className="hist-text" x={0} y={H + 12}>
        {fmt(lo + pad)}
      </text>
      <text className="hist-text" x={W} y={H + 12} textAnchor="end">
        {fmt(hi - pad)}
      </text>
    </svg>
  );
}
