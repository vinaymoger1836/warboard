// Replay scrubber (doc 03 §12): while open, the map shows a replay of the war re-simulated
// from the nearest checkpoint (every 10 sim minutes, marked on the bar); the war itself is
// paused and carries on from where it was when the replay closes.
import { useEffect, useRef, useState } from 'react';
import type { EngineBridge } from '../bridge';
import type { ReplayInfo } from '../engine/messages';
import { elapsed } from './format';

export function ReplayBar({
  bridge,
  info,
  onInfo,
  toast,
}: {
  bridge: EngineBridge;
  info: Extract<ReplayInfo, { active: true }>;
  onInfo: (i: ReplayInfo) => void;
  toast: (t: string) => void;
}) {
  const [target, setTarget] = useState(info.tick);
  const [busy, setBusy] = useState(false);
  const pending = useRef<number | null>(null);

  // Seek once the slider rests (re-simulation can take a moment).
  useEffect(() => {
    if (target === info.tick) return;
    pending.current = target;
    const t = setTimeout(() => {
      setBusy(true);
      bridge
        .request({ type: 'replaySeek', tick: target })
        .then(onInfo)
        .catch((e: unknown) => toast(String(e)))
        .finally(() => setBusy(false));
    }, 200);
    return () => clearTimeout(t);
  }, [bridge, target, info.tick, onInfo, toast]);

  const max = Math.max(1, info.end_tick);
  const step = (dt: number) => setTarget((t) => Math.min(max, Math.max(0, t + dt)));
  return (
    <div className="replay-bar" role="group" aria-label="Replay">
      <span className="replay-label">REPLAY</span>
      <span className="clock-small">{elapsed(info.sim_time_ms)}</span>
      <button type="button" className="cmd" onClick={() => setTarget(0)} disabled={busy}>
        START
      </button>
      <button type="button" className="cmd" onClick={() => step(-600)} disabled={busy}>
        −10 MIN
      </button>
      <div className="replay-track">
        <input
          type="range"
          min={0}
          max={max}
          value={Math.min(target, max)}
          aria-label="Replay position (tick)"
          onChange={(e) => setTarget(Number(e.target.value))}
        />
        <div className="replay-marks" aria-hidden>
          {info.checkpoints.map((c) => (
            <span key={c.tick} style={{ left: `${(c.tick / max) * 100}%` }} title={`Checkpoint ${elapsed(c.sim_time_ms)}`} />
          ))}
        </div>
      </div>
      <button type="button" className="cmd" onClick={() => step(600)} disabled={busy}>
        +10 MIN
      </button>
      <span className="muted">{busy ? 'Re-simulating…' : `end ${elapsed(info.end_ms)}`}</span>
      <button
        type="button"
        className="cmd"
        onClick={() =>
          void bridge
            .request({ type: 'replayStop' })
            .then(onInfo)
            .catch((e: unknown) => toast(String(e)))
        }
      >
        EXIT REPLAY
      </button>
    </div>
  );
}
