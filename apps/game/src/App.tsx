import type { TickReport } from '@warboard/protocol';
import { useEffect, useRef, useState } from 'react';
import { EngineBridge } from './bridge';

const SPEEDS = [0, 1, 3, 5, 10, 30, 60] as const;

const pad = (n: number) => String(n).padStart(2, '0');

/** `03:40:12Z` from Unix epoch ms. */
function utcClock(ms: number): string {
  const d = new Date(ms);
  return `${pad(d.getUTCHours())}:${pad(d.getUTCMinutes())}:${pad(d.getUTCSeconds())}Z`;
}

/** `T+0:00:12` from elapsed ms. */
function elapsed(ms: number): string {
  const s = Math.floor(ms / 1000);
  return `T+${Math.floor(s / 3600)}:${pad(Math.floor(s / 60) % 60)}:${pad(s % 60)}`;
}

function PauseGlyph() {
  return (
    <svg width="10" height="12" viewBox="0 0 10 12" aria-hidden="true">
      <rect x="1" y="1" width="3" height="10" fill="currentColor" />
      <rect x="6" y="1" width="3" height="10" fill="currentColor" />
    </svg>
  );
}

export function App() {
  const bridgeRef = useRef<EngineBridge | null>(null);
  const speedRef = useRef(1);
  const [report, setReport] = useState<TickReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [speed, setSpeed] = useState(1);

  useEffect(() => {
    const bridge = new EngineBridge();
    bridgeRef.current = bridge;
    bridge.setSpeed(speedRef.current);
    const unsubscribe = bridge.subscribe((s) => {
      setReport(s.report);
      setError(s.error);
    });
    return () => {
      unsubscribe();
      bridge.dispose();
      bridgeRef.current = null;
    };
  }, []);

  const choose = (s: number) => {
    speedRef.current = s;
    setSpeed(s);
    bridgeRef.current?.setSpeed(s);
  };

  return (
    <div className="app">
      <header className="top-bar">
        <span className="brand">WARBOARD</span>
        <span className="clock" aria-label="Sim clock">
          {report ? (
            <>
              {utcClock(report.startUtcMs + report.simTimeMs)}
              <span className="elapsed">{elapsed(report.simTimeMs)}</span>
            </>
          ) : (
            '--:--:--Z'
          )}
        </span>
        <div className="speeds" role="group" aria-label="Speed">
          {SPEEDS.map((s) => (
            <button
              key={s}
              type="button"
              className={s === speed ? 'speed active' : 'speed'}
              aria-pressed={s === speed}
              aria-label={s === 0 ? 'Pause' : `Speed ×${s}`}
              onClick={() => choose(s)}
            >
              {s === 0 ? <PauseGlyph /> : `×${s}`}
            </button>
          ))}
        </div>
        <span className="achieved">
          {report ? `×${report.achievedSpeed.toFixed(1)} actual` : 'Starting engine'}
        </span>
      </header>

      <main className="content">
        <section className="panel" aria-labelledby="engine-status">
          <h1 id="engine-status" className="panel-header">
            Engine status
          </h1>
          {error ? (
            <p className="error">Engine error: {error}</p>
          ) : (
            <dl className="rows">
              <dt>Scenario</dt>
              <dd>scenarios/empty.yaml</dd>
              <dt>Tick</dt>
              <dd>{report?.tick.toLocaleString('en-GB') ?? '–'}</dd>
              <dt>Sim time</dt>
              <dd>{report ? elapsed(report.simTimeMs) : '–'}</dd>
              <dt>Tick length</dt>
              <dd>{report ? `${report.tickLenMs} ms` : '–'}</dd>
              <dt>Requested speed</dt>
              <dd>{report ? (report.requestedSpeed === 0 ? 'Paused' : `×${report.requestedSpeed}`) : '–'}</dd>
              <dt>Achieved speed</dt>
              <dd>{report ? `×${report.achievedSpeed.toFixed(2)}` : '–'}</dd>
              <dt>State hash</dt>
              <dd className="hash">{report?.stateHash ?? '–'}</dd>
            </dl>
          )}
        </section>
      </main>
    </div>
  );
}
