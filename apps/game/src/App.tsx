import type { TickReport } from '@warboard/protocol';
import { useEffect, useMemo, useRef, useState } from 'react';
import { EngineBridge } from './bridge';
import { MapView } from './map2d/MapView';
import { PackManager, type PackState } from './packs/manager';
import { PackPanel } from './packs/PackPanel';
import { fileFor, type InstalledPack } from './packs/types';
import { Toasts, useToasts } from './Toasts';

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

/** One engine worker per page; it lives as long as the page. */
let sharedBridge: EngineBridge | null = null;
const engineBridge = () => (sharedBridge ??= new EngineBridge());

const hasWorld = (packs: InstalledPack[]) =>
  packs.some((p) => p.manifest.extent[0] <= -180 && p.manifest.extent[2] >= 180);

export function App() {
  const bridge = useMemo(() => engineBridge(), []);
  const speedRef = useRef(1);
  const [report, setReport] = useState<TickReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [speed, setSpeed] = useState(1);
  const manager = useMemo(() => new PackManager(), []);
  const [packs, setPacks] = useState<PackState>(manager.snapshot);
  const [packsOpen, setPacksOpen] = useState(false);
  const [geoReady, setGeoReady] = useState(false);
  const [terrainOverlay, setTerrainOverlay] = useState(false);
  const { toasts, show } = useToasts();

  useEffect(() => {
    bridge.setSpeed(speedRef.current);
    return bridge.subscribe((s) => {
      setReport(s.report);
      setError(s.error);
    });
  }, [bridge]);

  // Packs: the engine worker holds the engine files open; release them before any change.
  useEffect(() => {
    const open = (usable: InstalledPack[]) => {
      setGeoReady(false);
      if (usable.length === 0) return;
      bridge
        .request({
          type: 'openPacks',
          packs: usable.map((p) => ({
            id: p.id,
            dem: fileFor(p.manifest, 'dem').name,
            terrain: fileFor(p.manifest, 'terrain').name,
            territory: fileFor(p.manifest, 'territory').name,
          })),
        })
        .then(() => setGeoReady(true))
        .catch((err: unknown) => show(`Cannot open packs: ${err instanceof Error ? err.message : String(err)}`));
    };
    manager.setBeforeChange(async () => {
      setGeoReady(false);
      await bridge.request({ type: 'closePacks' });
    });
    const offChanged = manager.onPacksChanged(open);
    const offState = manager.subscribe(setPacks);
    manager
      .refresh()
      .then(() => {
        const s = manager.snapshot;
        open(s.usable);
        if (!hasWorld(s.usable)) setPacksOpen(true);
      })
      .catch((err: unknown) => show(String(err)));
    return () => {
      offChanged();
      offState();
    };
  }, [bridge, manager, show]);

  const choose = (s: number) => {
    speedRef.current = s;
    setSpeed(s);
    bridge.setSpeed(s);
  };

  const mapReady = hasWorld(packs.usable);

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
          {error
            ? `Engine error: ${error}`
            : report
              ? `×${report.achievedSpeed.toFixed(1)} actual`
              : 'Starting engine'}
        </span>
        <button
          type="button"
          className="chrome-button packs-button"
          aria-expanded={packsOpen}
          onClick={() => setPacksOpen((o) => !o)}
        >
          PACKS
        </button>
      </header>

      <main className="board">
        {mapReady ? (
          <MapView
            packs={packs.usable}
            bridge={bridge}
            geoReady={geoReady}
            terrainOverlay={terrainOverlay}
            onTerrainOverlay={setTerrainOverlay}
            onToast={show}
          />
        ) : (
          <div className="map-empty">
            <p>Install the world pack to show the map.</p>
          </div>
        )}
        {packsOpen && <PackPanel manager={manager} state={packs} onClose={() => setPacksOpen(false)} />}
        <Toasts toasts={toasts} sticky={speed === 0 ? ['Simulation paused'] : []} />
      </main>
    </div>
  );
}
