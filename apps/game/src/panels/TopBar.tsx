// Top bar (doc 10 §8, doc 03 §9): clock, speeds, achieved speed, auto-slow, side ROE.
import type { TickReport } from '@warboard/protocol';
import { ui, useUi } from '../store';
import { elapsed, utcClock } from './format';
import { PauseGlyph } from './glyphs';

export const SPEEDS = [0, 1, 3, 5, 10, 30, 60] as const;

interface Props {
  report: TickReport | null;
  error: string | null;
  speed: number;
  onSpeed: (s: number) => void;
  onSaves: () => void;
  onPacks: () => void;
  savesOpen: boolean;
  packsOpen: boolean;
}

const AUTO_SLOW = [
  ['x1', '×1'],
  ['pause', 'PAUSE'],
  ['off', 'OFF'],
] as const;

export function TopBar({ report, error, speed, onSpeed, onSaves, onPacks, savesOpen, packsOpen }: Props) {
  const autoSlow = useUi((s) => s.autoSlow);
  const achieved = report
    ? speed === 0
      ? 'paused'
      : `×${speed} requested, ×${report.achievedSpeed.toFixed(1)} actual`
    : 'Starting engine';
  return (
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
            onClick={() => onSpeed(s)}
          >
            {s === 0 ? <PauseGlyph /> : `×${s}`}
          </button>
        ))}
      </div>
      <span className="achieved">{error ? `Engine error: ${error}` : achieved}</span>
      <div className="segmented-labelled">
        <span className="seg-label">AUTO-SLOW</span>
        <div className="speeds" role="group" aria-label="Auto-slow">
          {AUTO_SLOW.map(([v, label]) => (
            <button
              key={v}
              type="button"
              className={v === autoSlow ? 'speed active' : 'speed'}
              aria-pressed={v === autoSlow}
              onClick={() => ui.set({ autoSlow: v })}
            >
              {label}
            </button>
          ))}
        </div>
      </div>
      <div className="segmented-labelled" title="Rules of engagement arrive in P5">
        <span className="seg-label">ROE</span>
        <div className="speeds" role="group" aria-label="Side rules of engagement">
          {['A', 'B', 'C'].map((r) => (
            <button key={r} type="button" className="speed" disabled>
              {r}
            </button>
          ))}
        </div>
      </div>
      <div className="top-actions">
        <button type="button" className="chrome-button" aria-expanded={savesOpen} onClick={onSaves}>
          SAVES
        </button>
        <button type="button" className="chrome-button" aria-expanded={packsOpen} onClick={onPacks}>
          PACKS
        </button>
      </div>
    </header>
  );
}
