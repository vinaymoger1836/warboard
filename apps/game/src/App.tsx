// The game shell (doc 06 §1 `shell`): main menu and war setup until a war starts, then the war
// board (doc 06 §7: top bar, map with forces panel and unit card, bottom dock). Play modes
// (doc 12): the side toggle and Tab, each human side's UI slice and event stream, the umpire
// banner, and in hot-seat the handover screen, which replaces everything else while it shows.
import type { Event, Roe, TickReport } from '@warboard/protocol';
import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { EngineBridge } from './bridge';
import type { ReplayInfo, WarInfo } from './engine/messages';
import { Handover } from './Handover';
import { coalitionOf } from './map2d/coalitions';
import { MapView } from './map2d/MapView';
import { sendCommand } from './orders';
import { PackManager, type PackState } from './packs/manager';
import { PackPanel } from './packs/PackPanel';
import { fileFor, type InstalledPack } from './packs/types';
import { DeployPanel } from './panels/DeployPanel';
import { Dock } from './panels/Dock';
import { ForcesPanel } from './panels/ForcesPanel';
import { isAlert, isIntel } from './panels/format';
import { Planner } from './panels/Planner';
import { ReplayBar } from './panels/ReplayBar';
import { Reports } from './panels/Reports';
import { SavesPanel } from './panels/SavesPanel';
import { SideToggle } from './panels/SideToggle';
import { TopBar } from './panels/TopBar';
import { UnitCard } from './panels/UnitCard';
import { play, UMPIRE, usePlay } from './play';
import { Setup } from './setup/Setup';
import { ui, useUi } from './store';
import { Toasts, useToasts } from './Toasts';

// The 3D tactical view is its own chunk, loaded on first use (doc 06 §5).
const Map3D = lazy(() => import('./map3d/Map3D'));

/** One engine worker per page; it lives as long as the page. */
let sharedBridge: EngineBridge | null = null;
const engineBridge = () => {
  if (!sharedBridge) {
    sharedBridge = new EngineBridge();
    // For the browser tests (handover: the clock must not move): side views only, no truth.
    (window as unknown as { __wbBridge?: EngineBridge }).__wbBridge = sharedBridge;
  }
  return sharedBridge;
};

/** The unit card refreshes this often while something is selected. */
const DETAILS_MS = 500;

const hasWorld = (packs: InstalledPack[]) =>
  packs.some((p) => p.manifest.extent[0] <= -180 && p.manifest.extent[2] >= 180);

/** Events that make a waiting hot-seat side take the device (doc 12 §3): a launch against its
 *  forces or a new approval request. The handover screen never says which. */
const urgent = (e: Event) =>
  'AutoSlow' in e.kind && (e.kind.AutoSlow.cause === 'launch_against_us' || e.kind.AutoSlow.cause === 'roe_request');

export function App() {
  const bridge = useMemo(() => engineBridge(), []);
  const speedRef = useRef(1);
  const [report, setReport] = useState<TickReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [speed, setSpeed] = useState(1);
  const [war, setWar] = useState<WarInfo | null>(null);
  // Side default ROE per side: from the war when it starts or loads, then from accepted SetRoe orders.
  const [sideRoe, setSideRoe] = useState<Record<number, Roe>>({});
  const manager = useMemo(() => new PackManager(), []);
  const [packs, setPacks] = useState<PackState>(manager.snapshot);
  const [packsOpen, setPacksOpen] = useState(false);
  const [savesOpen, setSavesOpen] = useState(false);
  const [geoReady, setGeoReady] = useState(false);
  const [terrainOverlay, setTerrainOverlay] = useState(false);
  const { toasts, show } = useToasts();
  const selected = useUi((s) => s.selected);
  const selectedContact = useUi((s) => s.selectedContact);
  const planner = useUi((s) => s.planner);
  const reportsOpen = useUi((s) => s.reportsOpen);
  const active = usePlay((s) => s.active);
  const view = usePlay((s) => s.view);
  const handover = usePlay((s) => s.handover);
  const deploying = usePlay((s) => s.deploying);
  const mode = usePlay((s) => s.mode);
  const umpire = view === UMPIRE;
  const [replay, setReplay] = useState<ReplayInfo>({ active: false });
  const toggleReplay = useCallback(() => {
    const req = replay.active ? { type: 'replayStop' as const } : { type: 'replayStart' as const };
    bridge
      .request(req)
      .then((info) => {
        setReplay(info);
        ui.select(null);
      })
      .catch((err: unknown) => show(err instanceof Error ? err.message : String(err)));
  }, [bridge, replay.active, show]);

  const choose = useCallback(
    (s: number) => {
      speedRef.current = s;
      setSpeed(s);
      bridge.setSpeed(s);
    },
    [bridge],
  );
  useEffect(() => play.registerSpeed(() => speedRef.current, choose), [choose]);

  useEffect(() => {
    bridge.setSpeed(speedRef.current);
    const offState = bridge.subscribe((s) => {
      setReport(s.report);
      setError(s.error);
    });
    const offWar = bridge.onWar((w) => {
      setWar(w);
      setSideRoe(Object.fromEntries(w.sides.map((s, i) => [i, s.roe])));
      setReplay({ active: false });
      play.startWar(w, bridge.report?.simTimeMs ?? 0);
      setSavesOpen(false);
    });
    const offEvents = bridge.onEvents((events, side) => {
      ui.addEventsFor(side, events, isAlert, isIntel);
      const p = play.get();
      if (side !== p.active || p.handover !== null) {
        // A waiting hot-seat side: urgent events interrupt (pause and hand over), the rest waits.
        if (p.mode === 'hot_seat' && p.hotSeat.urgent === 'interrupt' && p.handover === null && events.some(urgent)) {
          play.handOver(side);
        }
        return;
      }
      // Auto-slow follows the active side's events only (doc 12 §3).
      const slow = events.find((e) => 'AutoSlow' in e.kind);
      if (slow && 'AutoSlow' in slow.kind) {
        const m = ui.get().autoSlow;
        if (m !== 'off' && speedRef.current !== 0) choose(m === 'pause' ? 0 : 1);
        show(`Auto-slow: ${slow.kind.AutoSlow.reason}`);
      }
    });
    // Autosave on close (doc 03 §13); best effort, the 5-minute autosave is the backstop.
    const hidden = () => {
      if (document.visibilityState === 'hidden') bridge.autosaveNow();
    };
    document.addEventListener('visibilitychange', hidden);
    window.addEventListener('pagehide', hidden);
    return () => {
      offState();
      offWar();
      offEvents();
      document.removeEventListener('visibilitychange', hidden);
      window.removeEventListener('pagehide', hidden);
    };
  }, [bridge, choose, show]);

  // Tab moves the side toggle (doc 12 §2), unless typing in a field.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Tab' || e.altKey || e.ctrlKey || e.metaKey) return;
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === 'INPUT' || t.tagName === 'SELECT' || t.tagName === 'TEXTAREA')) return;
      const p = play.get();
      if (!p.war || p.handover !== null || p.mode === 'vs_ai' || p.mode === null) return;
      e.preventDefault();
      play.cycle(bridge).catch((err: unknown) => show(err instanceof Error ? err.message : String(err)));
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [bridge, show]);

  // Hot-seat turns: hand over every N sim minutes (doc 12 §3).
  useEffect(() => {
    const p = play.get();
    const turns = p.hotSeat.hand_over;
    if (p.mode !== 'hot_seat' || p.handover !== null || typeof turns === 'string' || !report) return;
    if (report.simTimeMs - p.turnStartMs >= turns.Turns.minutes * 60_000) play.handOver();
  }, [report]);

  // Unit card details, refreshed while something is selected.
  useEffect(() => {
    if (selected === null) return;
    let live = true;
    const load = () =>
      bridge
        .request({ type: 'details', id: selected })
        .then((d) => {
          if (!live) return;
          // A member was picked: the card and rings show its group.
          if (d.kind === 'group' && d.id !== selected) ui.set({ selected: d.id, details: d });
          else ui.set({ details: d });
        })
        .catch(() => live && ui.select(null));
    load();
    const t = setInterval(load, DETAILS_MS);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, [bridge, selected, active]);

  // Contact card details, refreshed while a contact is selected.
  useEffect(() => {
    if (selectedContact === null) return;
    let live = true;
    const load = () =>
      bridge
        .request({ type: 'contactDetails', id: selectedContact })
        .then((c) => live && ui.set({ contact: c }))
        .catch(() => live && ui.selectContact(null));
    load();
    const t = setInterval(load, DETAILS_MS);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, [bridge, selectedContact, active]);

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
            version: p.version,
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
    // A page kept in the back/forward cache would keep the exclusive pack handles and lock
    // out the next page: release them on the way in, reopen on the way back.
    const onHide = (e: PageTransitionEvent) => {
      if (e.persisted) bridge.request({ type: 'closePacks' }).catch(() => undefined);
    };
    const onShow = (e: PageTransitionEvent) => {
      if (e.persisted) open(manager.snapshot.usable);
    };
    window.addEventListener('pagehide', onHide);
    window.addEventListener('pageshow', onShow);
    manager
      .refresh()
      .then(() => open(manager.snapshot.usable))
      .catch((err: unknown) => show(String(err)));
    return () => {
      offChanged();
      offState();
      window.removeEventListener('pagehide', onHide);
      window.removeEventListener('pageshow', onShow);
    };
  }, [bridge, manager, show]);

  const mapReady = hasWorld(packs.usable);
  const mapView = useUi((s) => s.view);
  // The active side is always own (purple), the other enemy (blue); the umpire view shows side A as own.
  const coalition = useMemo(() => coalitionOf(war, umpire ? 0 : active), [war, active, umpire]);
  const places = useMemo(() => war?.places ?? [], [war]);
  const overlays = (
    <>
      {packsOpen && <PackPanel manager={manager} state={packs} onClose={() => setPacksOpen(false)} />}
      {savesOpen && <SavesPanel bridge={bridge} toast={show} onClose={() => setSavesOpen(false)} />}
    </>
  );

  // Hot-seat handover: nothing else is rendered (no map, panels, alerts or toasts).
  if (war && handover !== null) return <Handover bridge={bridge} />;

  if (!war) {
    return (
      <div className="app">
        <main className="board">
          <Setup
            bridge={bridge}
            packs={packs.usable}
            toast={show}
            onPacks={() => setPacksOpen(true)}
            onSaves={() => setSavesOpen(true)}
          />
          {overlays}
          <Toasts toasts={toasts} sticky={[]} />
        </main>
      </div>
    );
  }

  return (
    <div className="app">
      <TopBar
        report={report}
        error={error}
        speed={speed}
        onSpeed={choose}
        savesOpen={savesOpen}
        packsOpen={packsOpen}
        onSaves={() => setSavesOpen((o) => !o)}
        onPacks={() => setPacksOpen((o) => !o)}
        sideRoe={umpire || mode === 'watch' ? null : (sideRoe[active] ?? null)}
        onRoe={(roe) => {
          const side = active;
          void sendCommand(bridge, { type: 'SetRoe', roe }, show).then((ok) => {
            if (ok) setSideRoe((r) => ({ ...r, [side]: roe }));
          });
        }}
        reportsOpen={reportsOpen}
        onReports={() => ui.set((s) => ({ reportsOpen: !s.reportsOpen }))}
        replaying={replay.active}
        onReplay={toggleReplay}
        sideToggle={<SideToggle bridge={bridge} toast={show} />}
      />
      {replay.active && <ReplayBar bridge={bridge} info={replay} onInfo={setReplay} toast={show} />}

      <main className="board">
        {mapReady && mapView === '3d' ? (
          <Suspense fallback={<div className="map-empty"><p>Opening the 3D view…</p></div>}>
            <Map3D packs={packs.usable} bridge={bridge} coalition={coalition} onToast={show} />
          </Suspense>
        ) : mapReady ? (
          <MapView
            packs={packs.usable}
            bridge={bridge}
            coalition={coalition}
            places={places}
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
        {umpire && (
          <div className="umpire-banner" role="status">
            Umpire view: both sides visible
          </div>
        )}
        {!umpire && (deploying ? <DeployPanel bridge={bridge} toast={show} /> : <ForcesPanel bridge={bridge} />)}
        {!umpire && <UnitCard bridge={bridge} toast={show} />}
        {overlays}
        {planner && !umpire && <Planner bridge={bridge} toast={show} />}
        {reportsOpen && <Reports bridge={bridge} toast={show} />}
        <Toasts
          toasts={toasts}
          sticky={deploying ? ['Deployment: the clock is paused'] : speed === 0 ? ['Simulation paused'] : []}
        />
      </main>
      <Dock bridge={bridge} toast={show} now={report?.simTimeMs ?? 0} />
    </div>
  );
}
