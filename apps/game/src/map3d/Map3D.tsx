// The 3D tactical view (doc 06 §5, ADR-008, ADR-047), loaded lazily as its own chunk: a
// CesiumJS globe with the packs' terrain and surface, the side picture at true altitude, and
// four camera modes. Only one map renders at a time; the selection, the clock and the panels
// live outside it, and the centre and scale carry over through `ui.camera` (play.switchView).
// Chrome follows doc 10 §9/§11: OPEN 2D MAP top right, zoom buttons bottom right, no Cesium
// widgets.
import type { CesiumWidget } from '@cesium/engine';
import { RELATION, statusOf } from '@warboard/protocol';
import { useEffect, useRef, useState } from 'react';
import type { EngineBridge } from '../bridge';
import type { Coalition } from '../map2d/coalitions';
import type { InstalledPack } from '../packs/types';
import { ContextMenu } from '../panels/ContextMenu';
import { play } from '../play';
import { atlasAndFonts } from '../scene/atlas';
import type { ContextRequest } from '../scene/scene';
import { makeToolDone } from '../scene/tools';
import { type Cam3d, type CamTarget, ui, useUi } from '../store';
import { Cameras, targetState } from './cameras';
import { addSurface, createWidget } from './globe';
import { loadAtlas, loadGlyphs } from './icons3d';
import { Renderer3d } from './Renderer3d';

export interface Map3DProps {
  packs: InstalledPack[];
  bridge: EngineBridge;
  coalition: Coalition;
  onToast: (text: string) => void;
}

interface Live {
  widget: CesiumWidget;
  renderer: Renderer3d;
  cameras: Cameras;
  /** Unhooks the camera update. */
  off: () => void;
}

/** Test and benchmark hooks (Playwright, doc 09 P10). */
export interface GlobeHooks {
  selectables: () => string[];
  pickable: (id: string) => boolean;
  windowPosition: (id: string) => { x: number; y: number } | null;
  visible: () => string[];
  fps: () => number;
  modelsOn: () => boolean;
  zoom: () => number;
  camera: () => { center: [number, number]; zoom: number } | null;
}

const setCam = (cam: Cam3d) => ui.set({ cam3d: cam });

/** A weapon of ours in flight that involves the selection (fired by it or at it). */
function engagementOf(bridge: EngineBridge, sel: { selected: number | null; selectedContact: number | null }): number | null {
  const v = bridge.frame?.view;
  if (!v) return null;
  const w = v.weapons;
  for (let i = 0; i < w.count; i++) {
    if ((w.relation[i] ?? 0) === RELATION.THEIRS) continue;
    const shooter = w.shooter[i] ?? 0;
    const row = v.index.get(shooter);
    const group = row === undefined ? 0 : (v.units.group[row] ?? 0);
    if (sel.selected !== null && (shooter === sel.selected || group === sel.selected)) return w.id[i] ?? null;
    if (sel.selectedContact !== null && (w.target[i] ?? 0) === sel.selectedContact) return w.id[i] ?? null;
  }
  return null;
}

/** What INSPECT opens on: the selected own group or satellite, or an identified contact. */
function inspectTarget(r: Renderer3d | null, sel: { selected: number | null; selectedContact: number | null }): CamTarget | null {
  if (sel.selected !== null) {
    const t: CamTarget = { kind: 'unit', id: sel.selected };
    if (!r) return t;
    const st = targetState(r, t);
    return st && r.unit(sel.selected)?.base !== true ? t : null;
  }
  if (sel.selectedContact !== null) {
    if (!r) return null;
    return r.contact(sel.selectedContact)?.identified ? { kind: 'contact', id: sel.selectedContact } : null;
  }
  return null;
}

export default function Map3D({ packs, bridge, coalition, onToast }: Map3DProps) {
  const container = useRef<HTMLDivElement>(null);
  const [live, setLive] = useState<Live | null>(null);
  const [menu, setMenu] = useState<ContextRequest | null>(null);
  const packKey = packs.map((p) => `${p.id}@${p.version}`).join();
  const coalitionKey = `${coalition.own.join()}|${coalition.enemy.join()}`;

  // The globe, renderer and cameras.
  useEffect(() => {
    const el = container.current;
    if (!el) return;
    let cancelled = false;
    let made: Live | null = null;
    let fps = 0;
    let frames = 0;
    let since = performance.now();
    const hasDem = packs.some((p) => p.manifest.files.some((f) => f.role === 'dem'));
    const widget = createWidget(el, bridge, hasDem);
    const cam = ui.get().camera;
    Cameras.viewFrom(widget, cam?.center ?? [18.5, 54.0], cam?.zoom ?? 6);
    const countFrame = widget.scene.postRender.addEventListener(() => {
      frames++;
      const now = performance.now();
      if (now - since >= 1000) {
        fps = (frames * 1000) / (now - since);
        frames = 0;
        since = now;
      }
    });
    Promise.all([atlasAndFonts(), loadGlyphs()])
      .then(async ([atlas]) => {
        await loadAtlas(atlas);
        if (cancelled) return;
        const renderer = new Renderer3d(widget, bridge, atlas, setMenu, makeToolDone(bridge, onToast), (id) => {
          setCam({ mode: 'inspect', target: { kind: 'weapon', id } });
        });
        const cameras = new Cameras(widget, renderer, bridge, (why) => {
          onToast(why);
          setCam({ mode: 'free', target: null });
        });
        const off = widget.scene.preRender.addEventListener(() => cameras.update());
        made = { widget, renderer, cameras, off };
        setLive(made);
        const hooks: GlobeHooks = {
          selectables: () => renderer.selectables(),
          pickable: (id) => renderer.pickable(id),
          windowPosition: (id) => renderer.windowPosition(id),
          visible: () => [...renderer.visible].sort(),
          fps: () => fps,
          modelsOn: () => renderer.modelsOn,
          zoom: () => renderer.zoom().zoom,
          camera: () => cameras.current(),
        };
        (window as unknown as { __wbGlobe?: GlobeHooks }).__wbGlobe = hooks;
      })
      .catch((err: unknown) => onToast(`The 3D view failed to start: ${String(err)}`));
    return () => {
      cancelled = true;
      countFrame();
      if (made) {
        made.off();
        made.cameras.dispose();
        made.renderer.dispose();
      }
      setLive(null);
      delete (window as unknown as { __wbGlobe?: GlobeHooks }).__wbGlobe;
      widget.destroy();
    };
    // onToast is stable; packKey stands for packs.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [bridge, packKey]);

  // The surface (texture and borders) follows the coalition: a side switch re-tints it.
  useEffect(() => {
    if (!live) return;
    let undo: (() => void) | null = null;
    let cancelled = false;
    addSurface(live.widget, packs, coalition)
      .then((u) => {
        if (cancelled) u();
        else undo = u;
      })
      .catch((err: unknown) => onToast(`The globe surface failed to draw: ${String(err)}`));
    return () => {
      cancelled = true;
      undo?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [live, packKey, coalitionKey]);

  // The side's camera slot (side switches, handover) and click-to-locate.
  useEffect(() => {
    if (!live) return;
    const unregister = play.registerCamera(
      () => live.cameras.current(),
      (c) => Cameras.viewFrom(live.widget, c.center, c.zoom),
    );
    let seq = ui.get().locate?.seq ?? 0;
    const off = ui.subscribe(() => {
      const { locate } = ui.get();
      if (!locate || locate.seq === seq) return;
      seq = locate.seq;
      const st = targetState(live.renderer, { kind: locate.contact ? 'contact' : 'unit', id: locate.id });
      const now = live.cameras.current();
      if (st) Cameras.viewFrom(live.widget, st.position, now?.zoom ?? 7);
    });
    return () => {
      unregister();
      off();
    };
  }, [live]);

  // Camera mode from the store (per side).
  const cam3d = useUi((s) => s.cam3d);
  useEffect(() => {
    live?.cameras.set(cam3d);
  }, [live, cam3d]);

  // Esc leaves follow / watch / inspect (unless a drawing tool is open: Esc cancels that).
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && !ui.get().tool && ui.get().cam3d.mode !== 'free') setCam({ mode: 'free', target: null });
    };
    window.addEventListener('keydown', key);
    return () => window.removeEventListener('keydown', key);
  }, []);

  return (
    <div className="map-root map3d">
      <div ref={container} className="map-canvas" />
      {live && <Chrome3d live={live} bridge={bridge} />}
      {live && cam3d.mode === 'inspect' && cam3d.target && <InspectHud live={live} bridge={bridge} target={cam3d.target} />}
      {menu && <ContextMenu req={menu} bridge={bridge} toast={onToast} onClose={() => setMenu(null)} />}
    </div>
  );
}

function Chrome3d({ live, bridge }: { live: Live; bridge: EngineBridge }) {
  const cam3d = useUi((s) => s.cam3d);
  const selected = useUi((s) => s.selected);
  const selectedContact = useUi((s) => s.selectedContact);
  // Mode availability depends on the moving picture: re-checked twice a second.
  const [, tick] = useState(0);
  useEffect(() => {
    const t = setInterval(() => tick((n) => n + 1), 500);
    return () => clearInterval(t);
  }, []);
  const sel = { selected, selectedContact };
  const followTarget: CamTarget | null = selected !== null ? { kind: 'unit', id: selected } : selectedContact !== null ? { kind: 'contact', id: selectedContact } : null;
  const watchWeapon = engagementOf(bridge, sel);
  const inspect = inspectTarget(live.renderer, sel);
  const zoomBy = (factor: number) => {
    const camera = live.widget.scene.camera;
    const h = camera.positionCartographic.height;
    if (factor > 1) camera.zoomIn(h * (1 - 1 / factor));
    else camera.zoomOut(h * (1 / factor - 1));
  };
  const modes: { mode: Cam3d['mode']; label: string; target: CamTarget | null; title: string }[] = [
    { mode: 'free', label: 'FREE', target: null, title: 'Free camera' },
    { mode: 'follow', label: 'FOLLOW', target: followTarget, title: 'Follow the selection' },
    { mode: 'watch', label: 'WATCH', target: watchWeapon !== null ? { kind: 'weapon', id: watchWeapon } : null, title: 'Watch an engagement of the selection' },
    { mode: 'inspect', label: 'INSPECT', target: inspect, title: 'Inspect the selection close up' },
  ];
  return (
    <>
      <button type="button" className="chrome-button open-3d" onClick={() => play.switchView('2d')}>
        OPEN 2D MAP
      </button>
      <div className="cam-modes" role="group" aria-label="Camera">
        {modes.map((m) => (
          <button
            key={m.mode}
            type="button"
            className="chrome-button cam-mode"
            aria-pressed={cam3d.mode === m.mode}
            disabled={m.mode !== 'free' && !m.target && cam3d.mode !== m.mode}
            title={m.title}
            onClick={() => setCam(m.mode === 'free' ? { mode: 'free', target: null } : { mode: m.mode, target: m.target })}
          >
            {m.label}
          </button>
        ))}
      </div>
      <div className="map-corner-br">
        <div className="zoom-buttons">
          <button type="button" className="zoom-button" aria-label="Zoom in" onClick={() => zoomBy(2)}>
            +
          </button>
          <button type="button" className="zoom-button" aria-label="Zoom out" onClick={() => zoomBy(0.5)}>
            −
          </button>
        </div>
      </div>
    </>
  );
}

const fmt = (n: number, unit: string) => `${Math.round(n).toLocaleString('en-GB')} ${unit}`;
const compass = (deg: number) => `${String(Math.round(((deg % 360) + 360) % 360)).padStart(3, '0')}°`;

/** Close-up data strip (ADR-047): own objects show what the unit card shows; contacts only
 *  what our picture holds (class, identity, reported motion), never a name. */
function InspectHud({ live, bridge, target }: { live: Live; bridge: EngineBridge; target: CamTarget }) {
  const [, tick] = useState(0);
  useEffect(() => {
    const t = setInterval(() => tick((n) => n + 1), 250);
    return () => clearInterval(t);
  }, []);
  const st = targetState(live.renderer, target);
  if (!st) return null;
  const strings = bridge.strings;
  const rows: [string, string][] = [];
  let title: string;
  if (target.kind === 'contact') {
    const c = live.renderer.contact(target.id);
    title = c?.typeName ?? 'Contact';
    rows.push(['Identity', c?.side === 'enemy' ? 'Hostile' : c?.side === 'neutral' ? 'Neutral' : c?.side === 'own' ? 'Friendly' : 'Unknown']);
    rows.push(['Position', 'Reported (our picture)']);
  } else if (target.kind === 'weapon') {
    const w = live.renderer.weapon(target.id);
    title = `${w?.tag ?? ''} ${strings.type(target.id)}`.trim();
    rows.push(['Role', w?.interceptor ? 'Interceptor' : 'Offensive weapon']);
  } else {
    title = strings.name(target.id);
    const v = bridge.frame?.view;
    const row = v?.index.get(target.id) ?? (v ? [...Array(v.units.count).keys()].find((i) => v.units.group[i] === target.id) : undefined);
    if (v && row !== undefined) {
      const id = v.units.id[row] ?? 0;
      rows.push(['Type', strings.type(id) || '—']);
      rows.push(['Status', statusOf(v.units.flags[row] ?? 0) || '—']);
      const fuel = v.units.fuelPct[row] ?? 255;
      if (fuel <= 100) rows.push(['Fuel', `${fuel}%`]);
    }
  }
  rows.push(['Altitude', fmt(st.alt, 'm')], ['Speed', fmt(st.speed * 3.6, 'km/h')], ['Heading', compass(st.heading)]);
  return (
    <div className="inspect-hud panel" role="status" aria-label="Inspect">
      <div className="inspect-title">
        <span>INSPECT</span>
        <strong>{title}</strong>
        <button type="button" className="chrome-button" onClick={() => setCam({ mode: 'free', target: null })}>
          EXIT INSPECT
        </button>
      </div>
      <dl>
        {rows.map(([k, v]) => (
          <div key={k}>
            <dt>{k}</dt>
            <dd>{v}</dd>
          </div>
        ))}
      </dl>
    </div>
  );
}
