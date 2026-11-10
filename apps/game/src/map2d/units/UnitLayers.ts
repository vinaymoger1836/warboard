// Own units and contacts on the 2D map (doc 06 §4, doc 10 §4–6): icons from the atlas,
// labels in the doc 10 grammar, the selected unit's three range rings, its route or patrol
// circle with WP markers, and the drawing-tool preview. P4 adds contacts (dashed frame,
// uncertainty ellipse and "(our estimate)" when estimated, bearing strobes, threat rings of
// identified SAM types), satellites (yellow dot, two-line label, dashed ground tracks) and
// the sensor rings of every own unit. P5 adds our weapons in flight (weaponLayers.ts); P6a the
// red hostile-weapon glyphs, the targeted glow and the launch, intercept and hit effects
// (effectLayers.ts).
// What is drawn comes from `scene/` (P10), shared with the 3D view so both select the same
// objects; this file only turns it into deck.gl layers. Positions are dead-reckoned from the
// last side view every animation frame, and the layers go straight to deck.gl, never through
// React state (doc 06 §1).
import type { Layer, PickingInfo } from '@deck.gl/core';
import { PathStyleExtension, type PathStyleExtensionProps } from '@deck.gl/extensions';
import { IconLayer, PathLayer, ScatterplotLayer, TextLayer } from '@deck.gl/layers';
import { type IconAtlas, iconKey, isAircraft, markAbove, missileDataUrl } from '@warboard/symbols';
import type { Map as MapLibreMap, MapMouseEvent } from 'maplibre-gl';
import type { EngineBridge } from '../../bridge';
import { distance, type LonLat } from '../../scene/geo';
import { buildOverlays, findContact, findUnit } from '../../scene/overlays';
import {
  BASE_LABEL_ZOOM,
  buildScene,
  type ContextRequest,
  c,
  type DrawnContact,
  type DrawnSatellite,
  type DrawnUnit,
  declutter,
  ENEMY_WEAPON_PX,
  MAX_EXTRAPOLATE_MS,
  type MapPick,
  type Path,
  type Scene,
  SMALL_ZOOM,
  selectableIds,
  type TextItem,
  weaponLabelClusters,
} from '../../scene/scene';
import { satelliteTrack } from '../../scene/tracks';
import { type Tool, ui } from '../../store';
import type { LayerHub } from '../layerHub';
import { effectLayers, haloLayer } from './effectLayers';
import { WeaponLayers } from './weaponLayers';

const dash = new PathStyleExtension({ dash: true });
/** A hostile weapon in our picture: the missile glyph in `weapon.enemy` (doc 10 §5, ref. 06). */
const ENEMY_WEAPON_ICON = { url: missileDataUrl('weapon.enemy'), width: 32, height: 32, id: 'enemy-missile', mask: false };
/** SDF text with room for the dark halo (doc 10 §4.1; deck's outline width is relative). */
const SDF = { sdf: true, fontSize: 64, buffer: 10, radius: 14, cutoff: 0.18 };

export class UnitLayers {
  private raf = 0;
  private cursor: LonLat | null = null;
  private drawn: DrawnUnit[] = [];
  private contacts: DrawnContact[] = [];
  private readonly weapons = new WeaponLayers();
  /** Ring tooltip (doc 10 §6): a DOM element over the map, not a deck.gl widget. */
  private readonly tip = document.createElement('div');
  private hoverAt: [number, number] | null = null;
  private hoverQueued = false;
  /** Last non-zero achieved speed (sim s per real s), for the effects' real-time scale. */
  private effectSpeed = 1;
  private readonly offs: (() => void)[] = [];
  /** The scene drawn last frame (parity hooks, doc 09 P10). */
  private lastScene: Scene | null = null;

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
      this.hoverAt = [e.point.x, e.point.y];
      if (!this.hoverQueued) {
        this.hoverQueued = true;
        requestAnimationFrame(() => {
          this.hoverQueued = false;
          this.hover();
        });
      }
    });
    on('mouseout', () => {
      this.hoverAt = null;
      this.tip.style.display = 'none';
    });
    this.tip.className = 'map-tip';
    this.tip.style.display = 'none';
    map.getContainer().appendChild(this.tip);
    this.offs.push(() => this.tip.remove());
    const key = (e: KeyboardEvent) => this.key(e);
    window.addEventListener('keydown', key);
    this.offs.push(() => window.removeEventListener('keydown', key));
    let locateSeq = 0;
    this.offs.push(
      ui.subscribe(() => {
        const { tool, locate } = ui.get();
        if (locate && locate.seq !== locateSeq) {
          locateSeq = locate.seq;
          const at = locate.contact ? this.findContact(locate.id) : this.find(locate.id);
          if (at) map.easeTo({ center: at, duration: 600 });
        }
        if (tool) map.doubleClickZoom.disable();
        else map.doubleClickZoom.enable();
        map.getCanvas().style.cursor = tool ? 'crosshair' : '';
      }),
    );
    // Test hooks (Playwright parity test): what this view can select, and whether a click at
    // an object's screen position reaches it.
    const hooks = {
      selectables: () => (this.lastScene ? selectableIds(this.lastScene) : []),
      windowPosition: (id: string) => {
        const at = this.positionOfKey(id);
        if (!at) return null;
        const p = map.project(at);
        return { x: p.x, y: p.y };
      },
      pickable: (id: string) => {
        const at = this.positionOfKey(id);
        if (!at) return false;
        const p = map.project(at);
        const hits = this.hub.deck.pickMultipleObjects({
          x: p.x,
          y: p.y,
          radius: 3,
          depth: 20,
          layerIds: ['unit-icons', 'contact-icons', 'enemy-weapon-icons', 'satellite-dots'],
        }) as PickingInfo<DrawnUnit | DrawnContact | DrawnSatellite>[];
        const [kind] = id.split(':');
        const layers = kind === 'u' ? ['unit-icons'] : kind === 'c' ? ['contact-icons', 'enemy-weapon-icons'] : ['satellite-dots'];
        return hits.some((h) => h.layer && layers.includes(h.layer.id) && `${kind}:${h.object?.id}` === id);
      },
    };
    (window as unknown as { __wbMap2d?: typeof hooks }).__wbMap2d = hooks;
    this.offs.push(() => delete (window as unknown as { __wbMap2d?: typeof hooks }).__wbMap2d);
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

  /** Shows what the ring or line under the cursor is; units and contacts take precedence. */
  private hover(): void {
    const at = this.hoverAt;
    let text = '';
    if (at && !ui.get().tool && !this.pick(at[0], at[1])) {
      const info = this.hub.deck.pickObject({ x: at[0], y: at[1], radius: 5, layerIds: ['unit-paths', 'contact-paths'] }) as PickingInfo<Path> | null;
      text = info?.object?.tip ?? '';
    }
    if (!text || !at) {
      this.tip.style.display = 'none';
      return;
    }
    this.tip.textContent = text;
    this.tip.style.display = 'block';
    this.tip.style.left = `${at[0] + 14}px`;
    this.tip.style.top = `${at[1] + 14}px`;
  }

  private pick(x: number, y: number): MapPick | null {
    const info = this.hub.deck.pickObject({
      x,
      y,
      radius: 6,
      layerIds: ['unit-icons', 'contact-icons', 'enemy-weapon-icons', 'satellite-dots'],
    }) as PickingInfo<DrawnUnit | DrawnContact | DrawnSatellite> | null;
    const d = info?.object;
    if (!d) return null;
    if (info?.layer?.id === 'contact-icons' || info?.layer?.id === 'enemy-weapon-icons') {
      return { id: d.id, base: false, aircraft: false, contact: true, satellite: false };
    }
    if (info?.layer?.id === 'satellite-dots') {
      return { id: d.id, base: false, aircraft: false, contact: false, satellite: true };
    }
    const u = d as DrawnUnit;
    return { id: u.id, base: u.base, aircraft: isAircraft(u.symbol), contact: false, satellite: false };
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
    const p = this.pick(e.point.x, e.point.y);
    if (tool?.kind === 'strike') {
      // A contact under the cursor is the target; anywhere else is a point. Nothing fires
      // until the card's LAUNCH STRIKE.
      ui.set({ tool: { kind: 'strike', aim: p?.contact ? { contact: p.id } : { point: at } } });
      return;
    }
    if (p?.contact) ui.selectContact(p.id);
    else ui.select(p?.id ?? null);
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

  private find(id: number): LonLat | null {
    return findUnit(this.bridge.frame?.view, id);
  }

  private findContact(id: number): LonLat | null {
    return findContact(this.bridge.frame?.view, id);
  }

  /** Where a selectable id (`u:1`, `c:2`, `s:3`) is drawn this frame. */
  private positionOfKey(id: string): LonLat | null {
    const s = this.lastScene;
    if (!s) return null;
    const [kind, raw] = id.split(':');
    const n = Number(raw);
    if (kind === 'u') return (s.units.find((d) => d.id === n && d.lead) ?? s.units.find((d) => d.id === n))?.position ?? null;
    if (kind === 'c') return s.contacts.find((d) => d.id === n)?.position ?? null;
    return s.satellites.find((d) => d.id === n)?.position ?? null;
  }

  /** Position of a unit or group as drawn this frame. */
  positionOf(id: number): LonLat | null {
    return this.drawn.find((d) => d.id === id)?.position ?? null;
  }

  private render(): void {
    const frame = this.bridge.frame;
    if (!frame) return;
    const { view } = frame;
    const strings = this.bridge.strings;
    const zoom = this.map.getZoom();
    const report = this.bridge.report;
    const simPerReal = report ? report.achievedSpeed : 0;
    const dtS = (Math.min(performance.now() - frame.receivedAt, MAX_EXTRAPOLATE_MS) / 1000) * simPerReal;
    const state = ui.get();

    const scene = buildScene(view, strings, {
      zoom,
      dtS,
      selected: state.selected,
      threatRings: state.threatRings,
      allSensorRings: state.allSensorRings,
    });
    this.lastScene = scene;
    const { units: drawn, satellites, contacts, contactPaths: ellipses, threatLabels } = scene;
    this.drawn = drawn;
    this.contacts = contacts;
    const selectedContact = contacts.find((d) => d.id === state.selectedContact) ?? null;
    const tracks: Path[] = satellites.flatMap((s) =>
      satelliteTrack(this.bridge, s.id).map((path) => ({
        path: path.map((p) => [p[0], p[1]] as LonLat),
        color: c('sat.track'),
        width: 1.5,
        dashed: true,
        tip: `Ground track: ${strings.name(s.id)}`,
      })),
    );

    const selected = drawn.find((d) => d.id === state.selected) ?? null;
    const details = state.details?.kind === 'group' && state.details.id === state.selected ? state.details : null;
    const { paths, texts, waypoints } = buildOverlays({
      state,
      details,
      selectedAt: selected?.position ?? null,
      cursor: this.cursor,
      find: (id) => this.find(id),
      findContact: (id) => this.findContact(id),
    });
    const halos = scene.halos.map((h) => h.position);
    const project = (d: { position: LonLat }) => this.map.project(d.position);

    const size = zoom < SMALL_ZOOM ? 24 : 30;
    const layers: Layer[] = [
      haloLayer(halos, performance.now()),
      new PathLayer<Path, PathStyleExtensionProps<Path>>({
        id: 'contact-paths',
        data: [...tracks, ...ellipses],
        getPath: (d) => d.path,
        getColor: (d) => d.color,
        getWidth: (d) => d.width,
        widthUnits: 'pixels',
        getDashArray: (d) => (d.dashed ? [4, 3] : [0, 0]),
        dashJustified: true,
        extensions: [dash],
        wrapLongitude: true,
        pickable: true,
      }),
      new TextLayer<TextItem>({
        id: 'threat-labels',
        data: threatLabels,
        getPosition: (d) => d.position,
        getText: (d) => d.text,
        getColor: (d) => d.color,
        getSize: 11,
        getPixelOffset: [0, -8],
        fontFamily: 'Noto Sans',
        fontWeight: 'italic',
        fontSettings: SDF,
        outlineWidth: 0.3,
        outlineColor: c('map.label.halo'),
        characterSet: 'auto',
      }),
      new ScatterplotLayer<DrawnContact>({
        id: 'contact-selection-ring',
        data: selectedContact ? [selectedContact] : [],
        getPosition: (d) => d.position,
        getRadius: size * 0.7,
        radiusUnits: 'pixels',
        stroked: true,
        filled: false,
        getLineColor: c('weapon.offensive'),
        getLineWidth: 2,
        lineWidthUnits: 'pixels',
      }),
      new IconLayer<DrawnContact>({
        id: 'contact-icons',
        data: contacts.filter((d) => !d.isWeapon),
        iconAtlas: this.atlas.image,
        iconMapping: this.atlas.mapping,
        getIcon: (d) => iconKey(d.symbol, d.side, d.estimated),
        getPosition: (d) => d.position,
        getSize: size,
        sizeUnits: 'pixels',
        pickable: true,
      }),
      new IconLayer<DrawnContact>({
        id: 'enemy-weapon-icons',
        data: contacts.filter((d) => d.isWeapon),
        getIcon: () => ENEMY_WEAPON_ICON,
        getPosition: (d) => d.position,
        getAngle: (d) => -d.heading,
        getSize: ENEMY_WEAPON_PX,
        sizeUnits: 'pixels',
        pickable: true,
      }),
      new TextLayer<DrawnContact>({
        id: 'contact-marks',
        data: contacts.filter((d) => markAbove(d.symbol)),
        getPosition: (d) => d.position,
        getText: (d) => markAbove(d.symbol),
        getSize: 9,
        getColor: c('side.enemy', 0.8),
        getPixelOffset: [0, -size * 0.62],
        fontFamily: 'JetBrains Mono',
        fontWeight: 600,
      }),
      new TextLayer<{ position: LonLat; text: string }>({
        id: 'enemy-weapon-labels',
        data: weaponLabelClusters(contacts.filter((d) => d.isWeapon), project),
        getPosition: (d) => d.position,
        getText: (d) => d.text,
        getSize: 11,
        getColor: c('weapon.enemy.label'),
        getPixelOffset: [0, ENEMY_WEAPON_PX * 0.5 + 6],
        getTextAnchor: 'middle',
        getAlignmentBaseline: 'top',
        fontFamily: 'Noto Sans',
        fontWeight: 600,
        fontSettings: SDF,
        outlineWidth: 0.3,
        outlineColor: c('map.label.halo'),
        characterSet: 'auto',
      }),
      new TextLayer<DrawnContact>({
        id: 'contact-labels',
        data: contacts.filter((d) => !d.isWeapon),
        getPosition: (d) => d.position,
        getText: (d) => d.label,
        getSize: 11,
        getColor: c('label.unit'),
        getPixelOffset: [0, size * 0.55 + 7],
        getTextAnchor: 'middle',
        getAlignmentBaseline: 'top',
        fontFamily: 'Noto Sans',
        fontWeight: 'italic',
        fontSettings: SDF,
        outlineWidth: 0.3,
        outlineColor: c('map.label.halo'),
        characterSet: 'auto',
      }),
      new ScatterplotLayer<DrawnSatellite>({
        id: 'satellite-dots',
        data: satellites,
        getPosition: (d) => d.position,
        getRadius: 7,
        radiusUnits: 'pixels',
        getFillColor: c('sat'),
        stroked: true,
        getLineColor: (d) => (d.id === state.selected ? c('weapon.offensive') : c('map.label.halo')),
        getLineWidth: (d) => (d.id === state.selected ? 2 : 1),
        lineWidthUnits: 'pixels',
        pickable: true,
      }),
      new TextLayer<DrawnSatellite>({
        id: 'satellite-labels',
        data: satellites,
        getPosition: (d) => d.position,
        getText: (d) => d.label,
        getSize: 12,
        getColor: c('sat.label'),
        getPixelOffset: [0, 14],
        getAlignmentBaseline: 'top',
        fontFamily: 'Noto Sans',
        fontWeight: 700,
        fontSettings: SDF,
        outlineWidth: 0.3,
        outlineColor: c('map.label.halo'),
        characterSet: 'auto',
      }),
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
        pickable: true,
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
        getIcon: (d) => iconKey(d.symbol, d.enemy ? 'enemy' : 'own'),
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
        data: declutter(
          drawn.filter((d) => zoom >= BASE_LABEL_ZOOM || !d.base || d.id === state.selected),
          state.selected,
          size,
          project,
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
        getFillColor: (d) => d.color,
      }),
    ];
    const contactAt = new Map(contacts.map((d) => [d.id, d.position]));
    layers.push(...this.weapons.layers(view, frame.receivedAt, strings, dtS, zoom, (id) => contactAt.get(id) ?? null));
    // Flashes and bursts on top of everything else. While paused the last running speed keeps
    // an effect at the same size instead of letting it jump.
    if (simPerReal > 0) this.effectSpeed = simPerReal;
    layers.push(...effectLayers(view, dtS, this.effectSpeed));
    this.hub.set('units', layers);
  }
}

interface HandlerMap {
  click: (e: MapMouseEvent) => void;
  dblclick: (e: MapMouseEvent) => void;
  contextmenu: (e: MapMouseEvent) => void;
  mousemove: (e: MapMouseEvent) => void;
  mouseout: (e: MapMouseEvent) => void;
}
