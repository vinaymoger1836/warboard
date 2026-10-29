// War board (doc 06 §7): top bar, map with forces panel and unit card, bottom dock.
import type { TickReport } from '@warboard/protocol';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { EngineBridge } from './bridge';
import type { WarInfo } from './engine/messages';
import { coalitionOf } from './map2d/coalitions';
import { MapView } from './map2d/MapView';
import { PackManager, type PackState } from './packs/manager';
import { PackPanel } from './packs/PackPanel';
import { fileFor, type InstalledPack } from './packs/types';
import { Dock } from './panels/Dock';
import { ForcesPanel } from './panels/ForcesPanel';
import { isAlert, isIntel } from './panels/format';
import { SavesPanel } from './panels/SavesPanel';
import { TopBar } from './panels/TopBar';
import { UnitCard } from './panels/UnitCard';
import { ui, useUi } from './store';
import { Toasts, useToasts } from './Toasts';

/** One engine worker per page; it lives as long as the page. */
let sharedBridge: EngineBridge | null = null;
const engineBridge = () => (sharedBridge ??= new EngineBridge());

/** The unit card refreshes this often while something is selected. */
const DETAILS_MS = 500;

const hasWorld = (packs: InstalledPack[]) =>
  packs.some((p) => p.manifest.extent[0] <= -180 && p.manifest.extent[2] >= 180);

export function App() {
  const bridge = useMemo(() => engineBridge(), []);
  const speedRef = useRef(1);
  const [report, setReport] = useState<TickReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [speed, setSpeed] = useState(1);
  const [war, setWar] = useState<WarInfo | null>(null);
  const manager = useMemo(() => new PackManager(), []);
  const [packs, setPacks] = useState<PackState>(manager.snapshot);
  const [packsOpen, setPacksOpen] = useState(false);
  const [savesOpen, setSavesOpen] = useState(false);
  const [geoReady, setGeoReady] = useState(false);
  const [terrainOverlay, setTerrainOverlay] = useState(false);
  const { toasts, show } = useToasts();
  const selected = useUi((s) => s.selected);
  const selectedContact = useUi((s) => s.selectedContact);

  const choose = useCallback(
    (s: number) => {
      speedRef.current = s;
      setSpeed(s);
      bridge.setSpeed(s);
    },
    [bridge],
  );

  useEffect(() => {
    bridge.setSpeed(speedRef.current);
    const offState = bridge.subscribe((s) => {
      setReport(s.report);
      setError(s.error);
    });
    const offWar = bridge.onWar((w) => {
      ui.clearLog();
      setWar(w);
    });
    const offEvents = bridge.onEvents((events) => {
      ui.addEvents(events, isAlert, isIntel);
      const slow = events.find((e) => 'AutoSlow' in e.kind);
      if (slow && 'AutoSlow' in slow.kind) {
        const mode = ui.get().autoSlow;
        if (mode !== 'off' && speedRef.current !== 0) choose(mode === 'pause' ? 0 : 1);
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
  }, [bridge, selected]);

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
  }, [bridge, selectedContact]);

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
      .then(() => {
        const s = manager.snapshot;
        open(s.usable);
        if (!hasWorld(s.usable)) setPacksOpen(true);
      })
      .catch((err: unknown) => show(String(err)));
    return () => {
      offChanged();
      offState();
      window.removeEventListener('pagehide', onHide);
      window.removeEventListener('pageshow', onShow);
    };
  }, [bridge, manager, show]);

  const mapReady = hasWorld(packs.usable);
  const coalition = useMemo(() => coalitionOf(war), [war]);

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
      />

      <main className="board">
        {mapReady ? (
          <MapView
            packs={packs.usable}
            bridge={bridge}
            coalition={coalition}
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
        <ForcesPanel bridge={bridge} />
        <UnitCard bridge={bridge} toast={show} />
        {packsOpen && <PackPanel manager={manager} state={packs} onClose={() => setPacksOpen(false)} />}
        {savesOpen && <SavesPanel bridge={bridge} toast={show} onClose={() => setSavesOpen(false)} />}
        <Toasts toasts={toasts} sticky={speed === 0 ? ['Simulation paused'] : []} />
      </main>
      <Dock strings={bridge.strings} />
    </div>
  );
}
