// Own units on the 2D map (doc 06 §4, doc 10 §4–6): icons from the atlas, labels in the
// doc 10 grammar, the selected unit's three range rings, its route or patrol circle with
// WP markers, and the drawing-tool preview. Positions are dead-reckoned from the last
// side view every animation frame (heading × speed × sim time since the view), and the
// layers go straight to deck.gl, never through React state (doc 06 §1).
import type { Layer, PickingInfo } from '@deck.gl/core';
import { PathStyleExtension, type PathStyleExtensionProps } from '@deck.gl/extensions';
import { IconLayer, PathLayer, ScatterplotLayer, TextLayer } from '@deck.gl/layers';
import { FLAG, type GroupDetails, statusOf } from '@warboard/protocol';
import {
  type IconAtlas,
  iconKey,
  isAircraft,
  markAbove,
  rgba,
  type TokenName,
  unitLabel,
} from '@warboard/symbols';
import type { Map as MapLibreMap, MapMouseEvent } from 'maplibre-gl';
import type { EngineBridge } from '../../bridge';
import { type LonLat, type Tool, ui } from '../../store';
import type { LayerHub } from '../layerHub';
import { destination, distance, geodesicCircle } from './geo';

/** Zoom below which icons shrink to 24 px (doc 10 §4). */
const SMALL_ZOOM = 5;
/** Bases are drawn from this zoom. */
const BASE_ZOOM = 5;
/** Group members are drawn one by one from this zoom; below it a flight is one icon. */
const MEMBERS_ZOOM = 11;
/** Base labels from this zoom (selected base always). */
const BASE_LABEL_ZOOM = 7;
/** Dead reckoning runs at most this long past a view (real ms). */
const MAX_EXTRAPOLATE_MS = 400;

export interface DrawnUnit {
  /** Group id for flights and ships, base id for bases. */
  id: number;
  symbol: number;
  position: LonLat;
  label: string;
  mark: string;
  base: boolean;
}

export interface MapPick {
  id: number;
  base: boolean;
  aircraft: boolean;
}

export interface ContextRequest {
  x: number;
  y: number;
  at: LonLat;
  target: MapPick | null;
}

interface Path {
  path: LonLat[];
  color: [number, number, number, number];
  width: number;
  dashed: boolean;
}

interface TextItem {
  position: LonLat;
  text: string;
  color: [number, number, number, number];
}

const dash = new PathStyleExtension({ dash: true });
/** Rough label box per character and line height, px (11 px italic Noto Sans). */
const LABEL_CHAR_PX = 5.6;
const LABEL_LINE_PX = 15;
/** SDF text with room for the dark halo (doc 10 §4.1; deck's outline width is relative). */
const SDF = { sdf: true, fontSize: 64, buffer: 10, radius: 14, cutoff: 0.18 };
const c = (t: TokenName, a = 1) => rgba(t, a);

export class UnitLayers {
  private raf = 0;
  private cursor: LonLat | null = null;
  private drawn: DrawnUnit[] = [];
  private readonly offs: (() => void)[] = [];

  constructor(
    private readonly map: MapLibreMap,
    private readonly hub: LayerHub,
    private readonly bridge: EngineBridge,
    private readonly atlas: IconAtlas,
    private readonly onContext: (req: ContextRequest | null) => void,
    private readonly onToolDone: (tool: Tool) => void,
  ) {
    const on = <E extends keyof HandlerMap>(type: E, fn: HandlerMap[E]) => {
      map.on(type, fn as never);
      this.offs.push(() => map.off(type, fn as never));
    };
    on('click', (e) => this.click(e));
    on('dblclick', (e) => this.finishTool(e));
    on('contextmenu', (e) => this.contextMenu(e));
    on('mousemove', (e) => {
      this.cursor = [e.lngLat.lng, e.lngLat.lat];
    });
    const key = (e: KeyboardEvent) => this.key(e);
    window.addEventListener('keydown', key);
    this.offs.push(() => window.removeEventListener('keydown', key));
    let locateSeq = 0;
    this.offs.push(
      ui.subscribe(() => {
        const { tool, locate } = ui.get();
        if (locate && locate.seq !== locateSeq) {
          locateSeq = locate.seq;
          const at = this.find(locate.id);
          if (at) map.easeTo({ center: at, duration: 600 });
        }
        if (tool) map.doubleClickZoom.disable();
        else map.doubleClickZoom.enable();
        map.getCanvas().style.cursor = tool ? 'crosshair' : '';
      }),
    );
    const loop = () => {
      this.render();
      this.raf = requestAnimationFrame(loop);
    };
    this.raf = requestAnimationFrame(loop);
  }

  dispose(): void {
    cancelAnimationFrame(this.raf);
    for (const off of this.offs) off();
    this.hub.set('units', []);
  }

  // ---- Interaction --------------------------------------------------------------------

  private pick(x: number, y: number): MapPick | null {
    const info = this.hub.deck.pickObject({ x, y, radius: 6, layerIds: ['unit-icons'] }) as PickingInfo<DrawnUnit> | null;
    const d = info?.object;
    return d ? { id: d.id, base: d.base, aircraft: isAircraft(d.symbol) } : null;
  }

  private click(e: MapMouseEvent): void {
    this.onContext(null);
    const tool = ui.get().tool;
    const at: LonLat = [e.lngLat.lng, e.lngLat.lat];
    if (tool?.kind === 'route') {
      ui.set({ tool: { ...tool, points: [...tool.points, at] } });
      return;
    }
    if (tool?.kind === 'circle') {
      if (!tool.centre) ui.set({ tool: { ...tool, centre: at } });
      else this.onToolDone({ ...tool, radiusKm: Math.max(1, distance(tool.centre, at) / 1000) });
      return;
    }
    ui.select(this.pick(e.point.x, e.point.y)?.id ?? null);
  }

  private finishTool(e: MapMouseEvent): void {
    const tool = ui.get().tool;
    if (tool?.kind !== 'route') return;
    e.preventDefault();
    // The double-click's first click already added this point.
    if (tool.points.length > 0) this.onToolDone(tool);
  }

  private key(e: KeyboardEvent): void {
    const tool = ui.get().tool;
    if (!tool) return;
    if (e.key === 'Escape') ui.set({ tool: null });
    if (e.key === 'Enter' && tool.kind === 'route' && tool.points.length > 0) this.onToolDone(tool);
  }

  private contextMenu(e: MapMouseEvent): void {
    e.preventDefault();
    if (ui.get().tool) {
      ui.set({ tool: null });
      return;
    }
    this.onContext({
      x: e.point.x,
      y: e.point.y,
      at: [e.lngLat.lng, e.lngLat.lat],
      target: this.pick(e.point.x, e.point.y),
    });
  }

  // ---- Drawing ------------------------------------------------------------------------

  /** Last reported position of a unit, group (its first member) or base. */
  private find(id: number): LonLat | null {
    const v = this.bridge.frame?.view;
    if (!v) return null;
    let i = v.index.get(id);
    if (i === undefined) {
      for (let k = 0; k < v.units.count; k++) if (v.units.group[k] === id) i ??= k;
    }
    return i === undefined ? null : [v.units.lon[i] ?? 0, v.units.lat[i] ?? 0];
  }

  /** Labels give way when crowded, icons never do (doc 10 §4.1): greedy by priority
   *  (selection, units, bases), dropping any label whose screen box overlaps one kept. */
  private declutter(items: DrawnUnit[], selected: number | null, size: number): DrawnUnit[] {
    const prio = (d: DrawnUnit) => (d.id === selected ? 0 : d.base ? 2 : 1);
    const sorted = [...items].sort((a, b) => prio(a) - prio(b) || a.id - b.id);
    const kept: DrawnUnit[] = [];
    const boxes: [number, number, number, number][] = [];
    for (const d of sorted) {
      const p = this.map.project(d.position);
      const w = d.label.length * LABEL_CHAR_PX;
      const y = p.y + size * 0.55;
      const box: [number, number, number, number] = [p.x - w / 2, y, p.x + w / 2, y + LABEL_LINE_PX];
      if (boxes.some((b) => box[0] < b[2] && box[2] > b[0] && box[1] < b[3] && box[3] > b[1])) continue;
      boxes.push(box);
      kept.push(d);
    }
    return kept;
  }

  /** Position of a unit or group as drawn this frame. */
  positionOf(id: number): LonLat | null {
    return this.drawn.find((d) => d.id === id)?.position ?? null;
  }

  private render(): void {
    const frame = this.bridge.frame;
    if (!frame) return;
    const { view } = frame;
    const u = view.units;
    const strings = this.bridge.strings;
    const zoom = this.map.getZoom();
    const report = this.bridge.report;
    const simPerReal = report ? report.achievedSpeed : 0;
    const dtS = (Math.min(performance.now() - frame.receivedAt, MAX_EXTRAPOLATE_MS) / 1000) * simPerReal;
    const state = ui.get();

    const drawn: DrawnUnit[] = [];
    const groups = new Map<number, { index: number; count: number }>();
    const at = (i: number): LonLat => {
      const p: LonLat = [u.lon[i] ?? 0, u.lat[i] ?? 0];
      const v = u.speedMps[i] ?? 0;
      return v > 0 && dtS > 0 ? destination(p, u.headingDeg[i] ?? 0, v * dtS) : p;
    };
    for (let i = 0; i < u.count; i++) {
      const flags = u.flags[i] ?? 0;
      const id = u.id[i] ?? 0;
      const symbol = u.symbol[i] ?? 0;
      if (flags & FLAG.BASE) {
        if (zoom < BASE_ZOOM && id !== state.selected) continue;
        drawn.push({ id, symbol, position: at(i), label: unitLabel({ symbol, typeName: '', name: strings.name(id), count: 1 }), mark: '', base: true });
        continue;
      }
      if (flags & FLAG.AT_BASE) continue;
      const group = u.group[i] ?? 0;
      if (zoom >= MEMBERS_ZOOM || group === 0) {
        drawn.push({ id: group || id, symbol, position: at(i), label: strings.name(id), mark: markAbove(symbol), base: false });
        continue;
      }
      const g = groups.get(group);
      if (!g) groups.set(group, { index: i, count: 1 });
      else {
        g.count++;
        if (flags & FLAG.GROUP_LEAD) g.index = i;
      }
    }
    for (const [group, g] of groups) {
      const i = g.index;
      const symbol = u.symbol[i] ?? 0;
      const id = u.id[i] ?? 0;
      let label = unitLabel({ symbol, typeName: strings.type(id), name: strings.name(group), count: g.count });
      if (group === state.selected) {
        const fuel = u.fuelPct[i] ?? 255;
        label += ` [${statusOf(u.flags[i] ?? 0)}]${fuel <= 100 ? ` ${fuel}%` : ''}`;
      }
      drawn.push({ id: group, symbol, position: at(i), label, mark: markAbove(symbol), base: false });
    }
    this.drawn = drawn;

    const selected = drawn.find((d) => d.id === state.selected) ?? null;
    const details = state.details?.kind === 'group' && state.details.id === state.selected ? state.details : null;
    const paths: Path[] = [];
    const texts: TextItem[] = [];
    const waypoints: { position: LonLat; text: string }[] = [];
    if (details && selected) this.selectionOverlays(details, selected.position, paths, texts, waypoints);
    if (state.tool) this.toolOverlays(state.tool, paths, waypoints);

    const size = zoom < SMALL_ZOOM ? 24 : 30;
    const layers: Layer[] = [
      new PathLayer<Path, PathStyleExtensionProps<Path>>({
        id: 'unit-paths',
        data: paths,
        getPath: (d) => d.path,
        getColor: (d) => d.color,
        getWidth: (d) => d.width,
        widthUnits: 'pixels',
        getDashArray: (d) => (d.dashed ? [4, 3] : [0, 0]),
        dashJustified: true,
        extensions: [dash],
        wrapLongitude: true,
      }),
      new ScatterplotLayer<{ position: LonLat }>({
        id: 'waypoints',
        data: waypoints,
        getPosition: (d) => d.position,
        getRadius: 5,
        radiusUnits: 'pixels',
        stroked: true,
        filled: false,
        getLineColor: c('waypoint'),
        getLineWidth: 2,
        lineWidthUnits: 'pixels',
      }),
      new TextLayer<{ position: LonLat; text: string }>({
        id: 'waypoint-labels',
        data: waypoints,
        getPosition: (d) => d.position,
        getText: (d) => d.text,
        getSize: 12,
        getColor: c('waypoint.label'),
        getPixelOffset: [0, -16],
        fontFamily: 'Noto Sans',
        fontWeight: 600,
        fontSettings: SDF,
        outlineWidth: 0.3,
        outlineColor: c('map.label.halo'),
        characterSet: 'auto',
      }),
      new ScatterplotLayer<DrawnUnit>({
        id: 'selection-ring',
        data: selected ? [selected] : [],
        getPosition: (d) => d.position,
        getRadius: size * 0.7,
        radiusUnits: 'pixels',
        stroked: true,
        filled: false,
        getLineColor: c('weapon.offensive'),
        getLineWidth: 2,
        lineWidthUnits: 'pixels',
      }),
      new IconLayer<DrawnUnit>({
        id: 'unit-icons',
        data: drawn,
        iconAtlas: this.atlas.image,
        iconMapping: this.atlas.mapping,
        getIcon: (d) => iconKey(d.symbol, 'own'),
        getPosition: (d) => d.position,
        getSize: size,
        sizeUnits: 'pixels',
        pickable: true,
      }),
      new TextLayer<DrawnUnit>({
        id: 'unit-marks',
        data: drawn.filter((d) => d.mark),
        getPosition: (d) => d.position,
        getText: (d) => d.mark,
        getSize: 9,
        getColor: c('side.own.text'),
        getPixelOffset: [0, -size * 0.62],
        fontFamily: 'JetBrains Mono',
        fontWeight: 600,
      }),
      new TextLayer<DrawnUnit>({
        id: 'unit-labels',
        data: this.declutter(
          drawn.filter((d) => zoom >= BASE_LABEL_ZOOM || !d.base || d.id === state.selected),
          state.selected,
          size,
        ),
        getPosition: (d) => d.position,
        getText: (d) => d.label,
        getSize: 11,
        getColor: c('label.unit'),
        getPixelOffset: [0, size * 0.55 + 7],
        fontFamily: 'Noto Sans',
        fontWeight: 'italic',
        fontSettings: SDF,
        outlineWidth: 0.3,
        outlineColor: c('map.label.halo'),
        characterSet: 'auto',
      }),
      new TextLayer<TextItem>({
        id: 'ring-labels',
        data: texts,
        getPosition: (d) => d.position,
        getText: (d) => d.text,
        getColor: (d) => d.color,
        getSize: 12,
        getPixelOffset: [12, 0],
        getTextAnchor: 'start',
        fontFamily: 'Noto Sans',
        fontSettings: SDF,
        outlineWidth: 0.3,
        outlineColor: c('map.label.halo'),
        characterSet: 'auto',
      }),
      new ScatterplotLayer<TextItem>({
        id: 'ring-label-dots',
        data: texts,
        getPosition: (d) => d.position,
        getRadius: 5,
        radiusUnits: 'pixels',
        getFillColor: c('ring.radius'),
      }),
    ];
    this.hub.set('units', layers);
  }

  /** Rings (doc 10 §6–7), route or patrol circle of the selected group. */
  private selectionOverlays(
    d: GroupDetails,
    pos: LonLat,
    paths: Path[],
    texts: TextItem[],
    waypoints: { position: LonLat; text: string }[],
  ): void {
    const { rings } = ui.get();
    const r = d.ranges;
    if (rings.sensor && r.sensor_m) paths.push({ path: geodesicCircle(pos, r.sensor_m), color: c('ring.sensor'), width: 2, dashed: true });
    if (rings.engage && r.engage_m) paths.push({ path: geodesicCircle(pos, r.engage_m), color: c('ring.engage'), width: 2, dashed: true });
    if (rings.radius && r.combat_radius_m && d.home_base) {
      const home: LonLat = [d.home_base.lon_deg, d.home_base.lat_deg];
      paths.push({ path: geodesicCircle(home, r.combat_radius_m), color: c('ring.radius'), width: 1.5, dashed: true });
      texts.push({
        position: destination(home, 315, r.combat_radius_m * 0.55),
        text: `${d.home_base.name} Combat Radius (${Math.round(r.combat_radius_m / 1000)} km)`,
        color: c('ring.radius.text'),
      });
    }

    const t = d.task;
    const wps = t.waypoints as LonLat[];
    if (t.kind === 'patrol_circle' && wps[0] && t.radius_m) {
      paths.push({ path: geodesicCircle(wps[0], t.radius_m), color: c('waypoint'), width: 1.5, dashed: true });
      return;
    }
    if (t.kind === 'move' || t.kind === 'patrol_route' || t.kind === 'tanker_track') {
      const next = Math.min(t.wp, wps.length);
      const ahead = wps.slice(next);
      const route: LonLat[] = t.mode === 'task' ? [pos, ...ahead] : [...ahead];
      if (t.looped) route.push(...wps.slice(0, next + 1));
      if (route.length > 1) paths.push({ path: route, color: c('weapon.offensive'), width: 2, dashed: false });
      wps.forEach((w, i) => waypoints.push({ position: w, text: `WP ${i + 1}` }));
    }
    if ((t.mode === 'return' || t.mode === 'at_base') && t.base && t.mode === 'return') {
      paths.push({ path: [pos, [t.base.lon_deg, t.base.lat_deg]], color: c('weapon.offensive'), width: 2, dashed: false });
    }
  }

  private toolOverlays(tool: Tool, paths: Path[], waypoints: { position: LonLat; text: string }[]): void {
    const cursor = this.cursor;
    if (tool.kind === 'route') {
      const pts = cursor ? [...tool.points, cursor] : tool.points;
      if (pts.length > 1) paths.push({ path: pts, color: c('weapon.offensive'), width: 2, dashed: false });
      tool.points.forEach((p, i) => waypoints.push({ position: p, text: `WP ${i + 1}` }));
    } else if (tool.centre) {
      const radius = cursor ? distance(tool.centre, cursor) : tool.radiusKm * 1000;
      paths.push({ path: geodesicCircle(tool.centre, Math.max(radius, 1000)), color: c('waypoint'), width: 1.5, dashed: true });
    }
  }
}

interface HandlerMap {
  click: (e: MapMouseEvent) => void;
  dblclick: (e: MapMouseEvent) => void;
  contextmenu: (e: MapMouseEvent) => void;
  mousemove: (e: MapMouseEvent) => void;
}
