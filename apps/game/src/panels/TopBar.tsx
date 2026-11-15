// Top bar (doc 10 §8, doc 03 §9): clock, speeds, achieved speed, auto-slow, side ROE.
import type { Roe, TickReport } from '@warboard/protocol';
import { type ReactNode, useEffect, useRef, useState } from 'react';
import { ui, useUi } from '../store';
import { elapsed, utcClock } from './format';
import { MenuGlyph, PauseGlyph } from './glyphs';

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
  /** The side's default ROE (doc 03 §7); none until the war is loaded. */
  sideRoe: Roe | null;
  onRoe: (r: Roe) => void;
  onReports: () => void;
  reportsOpen: boolean;
  onReplay: () => void;
  replaying: boolean;
  /** Leave the war and go back to the main menu (the war is autosaved first). */
  onExit: () => void;
  /** Side toggle (doc 10 §8): first item of the bar in Command both, Hot-seat and Watch. */
  sideToggle?: ReactNode;
}

/** Side ROE buttons (doc 10 §8): letter, value and what it means (doc 03 §7). */
export const ROE_LEVELS: [string, Roe, string][] = [
  ['A', 'alpha', 'Alpha: weapons free'],
  ['B', 'bravo', 'Bravo: weapons tight (own territory and return fire)'],
  ['C', 'charlie', 'Charlie: weapons hold (self-defence only; the rest asks for approval)'],
];

const AUTO_SLOW = [
  ['x1', '×1'],
  ['pause', 'PAUSE'],
  ['off', 'OFF'],
] as const;

export function TopBar({
  report,
  error,
  speed,
  onSpeed,
  onSaves,
  onPacks,
  savesOpen,
  packsOpen,
  sideRoe,
  onRoe,
  onReports,
  reportsOpen,
  onReplay,
  replaying,
  onExit,
  sideToggle,
}: Props) {
  const achieved = report
    ? speed === 0
      ? 'paused'
      : `×${speed} requested, ×${report.achievedSpeed.toFixed(1)} actual`
    : 'Starting engine';
  return (
    <header className="top-bar">
      {sideToggle}
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
        <span className="seg-label">ROE</span>
        <div className="speeds" role="group" aria-label="Side rules of engagement">
          {ROE_LEVELS.map(([letter, roe, meaning]) => (
            <button
              key={roe}
              type="button"
              className={roe === sideRoe ? 'speed active' : 'speed'}
              aria-pressed={roe === sideRoe}
              title={`Side default ${meaning}`}
              disabled={sideRoe === null}
              onClick={() => onRoe(roe)}
            >
              {letter}
            </button>
          ))}
        </div>
      </div>
      <GameMenu
        reportsOpen={reportsOpen}
        savesOpen={savesOpen}
        packsOpen={packsOpen}
        replaying={replaying}
        onReports={onReports}
        onReplay={onReplay}
        onSaves={onSaves}
        onPacks={onPacks}
        onExit={onExit}
      />
    </header>
  );
}

interface MenuProps {
  reportsOpen: boolean;
  savesOpen: boolean;
  packsOpen: boolean;
  replaying: boolean;
  onReports: () => void;
  onReplay: () => void;
  onSaves: () => void;
  onPacks: () => void;
  onExit: () => void;
}

/** The menu at the right end of the bar: reports, replay, saves, packs, auto-slow and exit. */
function GameMenu({ reportsOpen, savesOpen, packsOpen, replaying, onReports, onReplay, onSaves, onPacks, onExit }: MenuProps) {
  const autoSlow = useUi((s) => s.autoSlow);
  const [open, setOpen] = useState(false);
  const [confirmExit, setConfirmExit] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const away = (e: PointerEvent) => {
      if (root.current && !root.current.contains(e.target as Node)) setOpen(false);
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false);
    };
    window.addEventListener('pointerdown', away);
    window.addEventListener('keydown', key);
    return () => {
      window.removeEventListener('pointerdown', away);
      window.removeEventListener('keydown', key);
    };
  }, [open]);
  const item = (label: string, action: () => void, extra?: { expanded?: boolean; pressed?: boolean }) => (
    <button
      type="button"
      role="menuitem"
      className="menu-item"
      aria-expanded={extra?.expanded}
      aria-pressed={extra?.pressed}
      onClick={() => {
        setOpen(false);
        action();
      }}
    >
      {label}
    </button>
  );
  return (
    <div className="game-menu" ref={root}>
      <button
        type="button"
        className="chrome-button menu-button"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label="Menu"
        onClick={() => {
          setConfirmExit(false);
          setOpen((o) => !o);
        }}
      >
        <MenuGlyph />
        MENU
      </button>
      {open && (
        <div className="menu-pop" role="menu" aria-label="Game menu">
          {item('REPORTS', onReports, { expanded: reportsOpen })}
          {item(replaying ? 'EXIT REPLAY' : 'REPLAY', onReplay, { pressed: replaying })}
          {item('SAVES', onSaves, { expanded: savesOpen })}
          {item('PACKS', onPacks, { expanded: packsOpen })}
          <div className="menu-row">
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
          <div className="menu-sep" />
          {confirmExit ? (
            <div className="menu-confirm">
              <span>Leave this war? It is autosaved first.</span>
              <div className="menu-confirm-buttons">
                <button type="button" className="cmd danger" onClick={() => { setOpen(false); onExit(); }}>
                  EXIT
                </button>
                <button type="button" className="cmd" onClick={() => setConfirmExit(false)}>
                  STAY
                </button>
              </div>
            </div>
          ) : (
            <button type="button" role="menuitem" className="menu-item danger" onClick={() => setConfirmExit(true)}>
              EXIT TO MAIN MENU
            </button>
          )}
        </div>
      )}
    </div>
  );
}
