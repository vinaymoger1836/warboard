// The side picture on the 3D globe (doc 06 §5, doc 10 §11): the same scene as the 2D map
// (`scene/`), the same atlas, tokens and label grammar, at true altitude. Units in the air get
// a thin stalk to the ground; trails and trajectories are drawn in 3D; own satellites show
// their orbit arc above the dashed ground track. Close in (≤ 200 objects on screen) units,
// identified contacts and weapons are drawn as low-poly class models (ADR-047).
//
// Primitives are pooled by object key and updated every frame from the dead-reckoned scene,
// never rebuilt; ground lines (rings, ellipses, routes) are rebuilt only when their content
// changes. Nothing here reads anything but the side view, the string table and the UI state.
import {
  Cartesian2,
  Cartesian3,
  Color,
  EllipsoidalOccluder,
  Ellipsoid,
  FixedFrameTransforms,
  HeadingPitchRoll,
  Math as CesiumMath,
} from '@cesium/core';
import {
  Axis,
  type Billboard,
  BillboardCollection,
  type CesiumWidget,
  ColorBlendMode,
  HorizontalOrigin,
  type Label,
  LabelCollection,
  LabelStyle,
  Material,
  Model,
  type Polyline,
  PolylineCollection,
  SceneTransforms,
  ScreenSpaceEventHandler,
  ScreenSpaceEventType,
  VerticalOrigin,
} from '@cesium/engine';
import { type IconAtlas, iconKey, isAircraft, markAbove, modelFor, modelUrl, rgba, type TokenName } from '@warboard/symbols';
import type { EngineBridge } from '../bridge';
import { effectsOf } from './effects3d';
import { distance, type LonLat } from '../scene/geo';
import { mercatorMetresPerPx, zoomForHeight } from '../scene/lod';
import { buildOverlays, findContact, findUnit, type Overlays } from '../scene/overlays';
import {
  BASE_LABEL_ZOOM,
  buildScene,
  type ContextRequest,
  declutter,
  type DrawnContact,
  type DrawnUnit,
  ENEMY_WEAPON_PX,
  MAX_EXTRAPOLATE_MS,
  type MapPick,
  type Path,
  type Scene,
  type TextItem,
  SMALL_ZOOM,
  selectableIds,
  weaponLabelClusters,
} from '../scene/scene';
import { satelliteTrack } from '../scene/tracks';
import { buildWeapons, type DrawnWeapon, TrailStore, type WeaponScene, weaponTags } from '../scene/weapons';
import { type Tool, ui } from '../store';
import { colour } from './globe';
import { haloImage, iconImage, missileImage, ringImage, satelliteImage } from './icons3d';

/** Models are drawn when at most this many objects are on screen (doc 06 §5). */
export const MODEL_LIMIT = 200;
/** Models are drawn from this equivalent zoom on (closer in). */
const MODEL_ZOOM = 9;
/** Objects higher than this get a stalk to the ground, m. */
const STALK_MIN_ALT = 150;
/** Per-weapon tags from this zoom (doc 10 §5), merged within this many px. */
const TAG_ZOOM = 7;
const TAG_MERGE_PX = 28;
/** A model is never drawn smaller than this, px (so it reads as well as the icon). */
const MODEL_MIN_PX = 36;

const LABEL_FONT = 'italic 11px "Noto Sans"';
const MARK_FONT = '600 9px "JetBrains Mono"';
const TAG_FONT = '11px "JetBrains Mono"';
const SAT_FONT = '700 12px "Noto Sans"';
const RING_FONT = '12px "Noto Sans"';

/** What a 3D primitive stands for (Cesium's pick result `.id`). */
export interface PickId {
  kind: 'unit' | 'contact' | 'satellite' | 'weapon' | 'line';
  id: number;
  base?: boolean;
  symbol?: number;
  tip?: string;
}

const key = (p: PickId) => `${p.kind[0]}:${p.id}`;

interface Pooled<T> {
  item: T;
  seen: number;
}

/** A keyed pool of billboards / labels / models, swept after each frame. */
class Pool<T> {
  private readonly items = new Map<string, Pooled<T>>();
  constructor(
    private readonly make: () => T,
    private readonly drop: (t: T) => void,
  ) {}
  get(k: string, frame: number): T {
    let p = this.items.get(k);
    if (!p) {
      p = { item: this.make(), seen: frame };
      this.items.set(k, p);
    }
    p.seen = frame;
    return p.item;
  }
  sweep(frame: number): void {
    for (const [k, p] of this.items)
      if (p.seen !== frame) {
        this.drop(p.item);
        this.items.delete(k);
      }
  }
  values(): T[] {
    return [...this.items.values()].map((p) => p.item);
  }
  keys(): string[] {
    return [...this.items.keys()];
  }
}

export interface DrawnState {
  scene: Scene;
  weapons: WeaponScene;
  zoom: number;
}

export class Renderer3d {
  private readonly billboards: BillboardCollection;
  private readonly labels: LabelCollection;
  private readonly stalks: PolylineCollection;
  private readonly trails: PolylineCollection;
  private readonly ground: PolylineCollection;
  private readonly orbits: PolylineCollection;
  private readonly bb: Pool<Billboard>;
  private readonly lb: Pool<Label>;
  private readonly st: Pool<Polyline>;
  private readonly tr: Pool<Polyline>;
  private readonly models = new Map<string, { model: Model | null; name: string; seen: number; loading: boolean }>();
  private readonly trailStore = new TrailStore();
  private readonly handler: ScreenSpaceEventHandler;
  private readonly offs: (() => void)[] = [];
  private readonly tip = document.createElement('div');
  private frame = 0;
  private groundSig = '';
  /** Labels of the ground lines, kept between rebuilds and drawn every frame. */
  private groundTexts: { texts: Overlays['texts']; waypoints: Overlays['waypoints']; threats: TextItem[] } = {
    texts: [],
    waypoints: [],
    threats: [],
  };
  private cursor: LonLat | null = null;
  private effectSpeed = 1;
  /** What was drawn last frame (for cameras, parity tests and the inspect view). */
  last: DrawnState | null = null;
  /** Keys drawn as pickable last frame, on screen and not behind the globe. */
  visible = new Set<string>();
  /** Whether models were drawn last frame. */
  modelsOn = false;
  /** The inspect target is always drawn as a model (ADR-047). */
  forceModel: string | null = null;

  constructor(
    private readonly widget: CesiumWidget,
    private readonly bridge: EngineBridge,
    private readonly atlas: IconAtlas,
    private readonly onContext: (req: ContextRequest | null) => void,
    private readonly onToolDone: (tool: Tool) => void,
    private readonly onPickWeapon: (id: number) => void,
  ) {
    const scene = widget.scene;
    this.ground = scene.primitives.add(new PolylineCollection());
    this.orbits = scene.primitives.add(new PolylineCollection());
    this.trails = scene.primitives.add(new PolylineCollection());
    this.stalks = scene.primitives.add(new PolylineCollection());
    this.billboards = scene.primitives.add(new BillboardCollection({ scene }));
    this.labels = scene.primitives.add(new LabelCollection({ scene }));
    this.bb = new Pool(
      () => this.billboards.add({ position: Cartesian3.ZERO, disableDepthTestDistance: Number.POSITIVE_INFINITY }),
      (b) => this.billboards.remove(b),
    );
    this.lb = new Pool(
      () =>
        this.labels.add({
          position: Cartesian3.ZERO,
          disableDepthTestDistance: Number.POSITIVE_INFINITY,
          style: LabelStyle.FILL_AND_OUTLINE,
          outlineColor: colour('map.label.halo'),
          outlineWidth: 3,
          horizontalOrigin: HorizontalOrigin.CENTER,
          verticalOrigin: VerticalOrigin.TOP,
        }),
      (l) => this.labels.remove(l),
    );
    this.st = new Pool(
      () => this.stalks.add({ width: 1 }),
      (p) => this.stalks.remove(p),
    );
    this.tr = new Pool(
      () => this.trails.add({ width: 1.5 }),
      (p) => this.trails.remove(p),
    );

    this.handler = new ScreenSpaceEventHandler(scene.canvas);
    this.handler.setInputAction((e: { position: Cartesian2 }) => this.click(e.position), ScreenSpaceEventType.LEFT_CLICK);
    this.handler.setInputAction(() => this.finishTool(), ScreenSpaceEventType.LEFT_DOUBLE_CLICK);
    this.handler.setInputAction((e: { position: Cartesian2 }) => this.contextMenu(e.position), ScreenSpaceEventType.RIGHT_CLICK);
    this.handler.setInputAction((e: { endPosition: Cartesian2 }) => this.hover(e.endPosition), ScreenSpaceEventType.MOUSE_MOVE);
    const noMenu = (e: Event) => e.preventDefault();
    scene.canvas.addEventListener('contextmenu', noMenu);
    this.offs.push(() => scene.canvas.removeEventListener('contextmenu', noMenu));
    this.tip.className = 'map-tip';
    this.tip.style.display = 'none';
    (widget.container as HTMLElement).appendChild(this.tip);
    this.offs.push(() => this.tip.remove());
    const keyDown = (e: KeyboardEvent) => {
      const tool = ui.get().tool;
      if (!tool) return;
      if (e.key === 'Escape') ui.set({ tool: null });
      if (e.key === 'Enter' && tool.kind === 'route' && tool.points.length > 0) this.onToolDone(tool);
    };
    window.addEventListener('keydown', keyDown);
    this.offs.push(() => window.removeEventListener('keydown', keyDown));
    this.offs.push(
      ui.subscribe(() => {
        scene.canvas.style.cursor = ui.get().tool ? 'crosshair' : '';
      }),
    );
    this.offs.push(scene.preUpdate.addEventListener(() => this.render()));
  }

  dispose(): void {
    for (const off of this.offs) off();
    this.handler.destroy();
    if (this.widget.isDestroyed()) return;
    const p = this.widget.scene.primitives;
    for (const m of this.models.values()) if (m.model) p.remove(m.model);
    for (const c of [this.ground, this.orbits, this.trails, this.stalks, this.billboards, this.labels]) p.remove(c);
  }

  // ---- Geometry helpers ---------------------------------------------------------------

  private cart(p: LonLat, alt: number): Cartesian3 {
    return Cartesian3.fromDegrees(p[0], p[1], alt);
  }

  /** Ground point under a screen position (terrain first, then the ellipsoid). */
  groundAt(pos: Cartesian2): LonLat | null {
    const scene = this.widget.scene;
    const ray = scene.camera.getPickRay(pos);
    const hit = (ray && scene.globe.pick(ray, scene)) ?? scene.camera.pickEllipsoid(pos);
    if (!hit) return null;
    const c = Ellipsoid.WGS84.cartesianToCartographic(hit);
    return c ? [CesiumMath.toDegrees(c.longitude), CesiumMath.toDegrees(c.latitude)] : null;
  }

  /** Equivalent 2D zoom of the camera (scene/lod.ts), from its distance to the ground
   *  at the screen centre. */
  zoom(): { zoom: number; centre: LonLat | null; range: number } {
    const scene = this.widget.scene;
    const cam = scene.camera;
    const canvas = scene.canvas;
    const mid = new Cartesian2(canvas.clientWidth / 2, canvas.clientHeight / 2);
    const hit = cam.pickEllipsoid(mid);
    const carto = cam.positionCartographic;
    const range = hit ? Cartesian3.distance(cam.positionWC, hit) : carto.height;
    const centreCarto = hit ? Ellipsoid.WGS84.cartesianToCartographic(hit) : carto;
    const lat = CesiumMath.toDegrees(centreCarto?.latitude ?? carto.latitude);
    const fovy = (cam.frustum as { fovy?: number }).fovy ?? CesiumMath.toRadians(60);
    const zoom = zoomForHeight(range, lat, fovy, canvas.clientHeight || 1);
    const centre: LonLat | null = centreCarto
      ? [CesiumMath.toDegrees(centreCarto.longitude), CesiumMath.toDegrees(centreCarto.latitude)]
      : null;
    return { zoom, centre, range };
  }

  // ---- Interaction --------------------------------------------------------------------

  private pickAt(pos: Cartesian2): PickId | null {
    const picked = this.widget.scene.pick(pos) as { id?: PickId } | undefined;
    const id = picked?.id;
    return id && typeof id === 'object' && 'kind' in id ? id : null;
  }

  private toMapPick(p: PickId | null): MapPick | null {
    if (!p || p.kind === 'line' || p.kind === 'weapon') return null;
    if (p.kind === 'contact') return { id: p.id, base: false, aircraft: false, contact: true, satellite: false };
    if (p.kind === 'satellite') return { id: p.id, base: false, aircraft: false, contact: false, satellite: true };
    return { id: p.id, base: !!p.base, aircraft: isAircraft(p.symbol ?? 0), contact: false, satellite: false };
  }

  private click(pos: Cartesian2): void {
    this.onContext(null);
    const tool = ui.get().tool;
    const at = this.groundAt(pos);
    if (tool?.kind === 'route') {
      if (at) ui.set({ tool: { ...tool, points: [...tool.points, at] } });
      return;
    }
    if (tool?.kind === 'circle') {
      if (!at) return;
      if (!tool.centre) ui.set({ tool: { ...tool, centre: at } });
      else this.onToolDone({ ...tool, radiusKm: Math.max(1, distance(tool.centre, at) / 1000) });
      return;
    }
    const raw = this.pickAt(pos);
    if (raw?.kind === 'weapon') {
      // A salvo label or weapon of ours: the inspect view opens on it (ADR-047).
      this.onPickWeapon(raw.id);
      return;
    }
    const p = this.toMapPick(raw);
    if (tool?.kind === 'strike') {
      ui.set({ tool: { kind: 'strike', aim: p?.contact ? { contact: p.id } : at ? { point: at } : null } });
      return;
    }
    if (p?.contact) ui.selectContact(p.id);
    else ui.select(p?.id ?? null);
  }

  private finishTool(): void {
    const tool = ui.get().tool;
    if (tool?.kind === 'route' && tool.points.length > 0) this.onToolDone(tool);
  }

  private contextMenu(pos: Cartesian2): void {
    if (ui.get().tool) {
      ui.set({ tool: null });
      return;
    }
    const at = this.groundAt(pos);
    if (!at) return;
    this.onContext({ x: pos.x, y: pos.y, at, target: this.toMapPick(this.pickAt(pos)) });
  }

  private hover(pos: Cartesian2): void {
    this.cursor = this.groundAt(pos);
    let text = '';
    if (!ui.get().tool) {
      const p = this.pickAt(pos);
      if (p?.kind === 'line') text = p.tip ?? '';
    }
    if (!text) {
      this.tip.style.display = 'none';
      return;
    }
    this.tip.textContent = text;
    this.tip.style.display = 'block';
    this.tip.style.left = `${pos.x + 14}px`;
    this.tip.style.top = `${pos.y + 14}px`;
  }

  // ---- Drawing ------------------------------------------------------------------------

  private render(): void {
    const frame = this.bridge.frame;
    if (!frame || this.widget.isDestroyed()) return;
    this.frame++;
    const f = this.frame;
    const { view } = frame;
    const strings = this.bridge.strings;
    const scene = this.widget.scene;
    const cam = scene.camera;
    const { zoom } = this.zoom();
    const report = this.bridge.report;
    const simPerReal = report ? report.achievedSpeed : 0;
    const dtS = (Math.min(performance.now() - frame.receivedAt, MAX_EXTRAPOLATE_MS) / 1000) * simPerReal;
    const state = ui.get();
    const s = buildScene(view, strings, {
      zoom,
      dtS,
      selected: state.selected,
      threatRings: state.threatRings,
      allSensorRings: state.allSensorRings,
    });
    this.trailStore.update(view, frame.receivedAt);
    const contactAt = new Map(s.contacts.map((d) => [d.id, d.position]));
    const w = buildWeapons(view, strings, this.trailStore, dtS, (id) => contactAt.get(id) ?? null);
    this.last = { scene: s, weapons: w, zoom };

    const occluder = new EllipsoidalOccluder(Ellipsoid.WGS84, cam.positionWC);
    const canvas = scene.canvas;
    const onScreen = (c: Cartesian3) => {
      if (!occluder.isPointVisible(c)) return null;
      const px = SceneTransforms.worldToWindowCoordinates(scene, c);
      if (!px || px.x < 0 || px.y < 0 || px.x > canvas.clientWidth || px.y > canvas.clientHeight) return null;
      return px;
    };
    const size = zoom < SMALL_ZOOM ? 24 : 30;
    const scale = size / 64;
    const visible = new Set<string>();

    // Positions once per object.
    const unitPos = new Map(s.units.map((d) => [d.id, this.cart(d.position, d.alt)]));
    const contactPos = new Map(s.contacts.map((d) => [d.id, this.cart(d.position, d.alt)]));
    const onScreenCount =
      [...unitPos.values(), ...contactPos.values()].filter((c) => occluder.isPointVisible(c) && onScreen(c)).length + w.weapons.length;
    const models = zoom >= MODEL_ZOOM && onScreenCount <= MODEL_LIMIT;
    this.modelsOn = models;

    // Units and bases.
    const selected = state.selected;
    for (const d of s.units) {
      const pos = unitPos.get(d.id) as Cartesian3;
      const pid: PickId = { kind: 'unit', id: d.id, base: d.base, symbol: d.symbol };
      const k = key(pid);
      const px = onScreen(pos);
      const asModel = !d.base && this.useModel(k, d.symbol, models);
      const b = this.bb.get(k, f);
      const iconK = iconKey(d.symbol, d.enemy ? 'enemy' : 'own');
      this.setImage(b, iconK, iconImage(this.atlas, iconK));
      b.position = pos;
      b.scale = scale;
      b.id = pid;
      b.show = !asModel || !this.modelReady(k);
      if (asModel) this.placeModel(k, d.symbol, pos, d.heading, d.enemy ? 'side.enemy' : 'side.own', pid, f);
      if (px) visible.add(k);
      if (d.mark) this.label(`m:${k}`, f, pos, d.mark, MARK_FONT, 'side.own.text', -size * 0.62 - 9, pid);
      if (d.alt > STALK_MIN_ALT) this.stalk(k, f, d.position, d.alt, d.enemy ? 'side.enemy' : 'side.own');
      if (d.id === selected) this.ring(`sel:${k}`, f, pos, size * 0.7, 2, 'weapon.offensive');
    }
    const labelled = declutter(
      s.units.filter((d) => zoom >= BASE_LABEL_ZOOM || !d.base || d.id === selected),
      selected,
      size,
      (d) => onScreen(unitPos.get(d.id) as Cartesian3) ?? null,
    );
    for (const d of labelled) this.label(`l:u:${d.id}`, f, unitPos.get(d.id) as Cartesian3, d.label, LABEL_FONT, 'label.unit', size * 0.55 + 7, { kind: 'unit', id: d.id, base: d.base, symbol: d.symbol });

    // Satellites: yellow dot and two-line label; orbit arcs come with the ground lines.
    for (const d of s.satellites) {
      const pid: PickId = { kind: 'satellite', id: d.id, symbol: 60 };
      const k = key(pid);
      const pos = this.cart(d.position, d.alt);
      const b = this.bb.get(k, f);
      const sel = d.id === selected;
      this.setImage(b, `sat:${sel}`, satelliteImage(sel));
      b.position = pos;
      b.scale = 1;
      b.id = pid;
      b.show = true;
      if (onScreen(pos)) visible.add(k);
      this.label(`l:${k}`, f, pos, d.label, SAT_FONT, 'sat.label', 14, pid);
    }

    // Contacts of our picture.
    for (const d of s.contacts) {
      const pos = contactPos.get(d.id) as Cartesian3;
      const pid: PickId = { kind: 'contact', id: d.id, symbol: d.symbol };
      const k = key(pid);
      const px = onScreen(pos);
      if (px) visible.add(k);
      const b = this.bb.get(k, f);
      b.position = pos;
      b.id = pid;
      const tint = d.side === 'enemy' ? 'side.enemy' : d.side === 'neutral' ? 'side.neutral' : 'side.unknown';
      if (d.isWeapon) {
        this.setImage(b, 'missile:weapon.enemy', missileImage('weapon.enemy'));
        b.scale = ENEMY_WEAPON_PX / 32;
        b.rotation = CesiumMath.toRadians(-d.heading) + cam.heading;
        b.show = true;
      } else {
        const iconK = iconKey(d.symbol, d.side, d.estimated);
        this.setImage(b, iconK, iconImage(this.atlas, iconK));
        b.scale = scale;
        b.rotation = 0;
        // Only identified contacts become models, at the reported position (ADR-047).
        const asModel = d.identified && this.useModel(k, d.symbol, models);
        b.show = !asModel || !this.modelReady(k);
        if (asModel) this.placeModel(k, d.symbol, pos, d.heading, tint, pid, f);
        if (markAbove(d.symbol)) this.label(`m:${k}`, f, pos, markAbove(d.symbol), MARK_FONT, 'side.enemy', -size * 0.62 - 9, pid);
        if (d.label) this.label(`l:${k}`, f, pos, d.label, LABEL_FONT, 'label.unit', size * 0.55 + 7, pid);
      }
      if (d.alt > STALK_MIN_ALT && !d.isWeapon) this.stalk(k, f, d.position, d.alt, tint);
      if (d.id === state.selectedContact) this.ring(`sel:${k}`, f, pos, size * 0.7, 2, 'weapon.offensive');
    }
    const hostile = s.contacts.filter((d) => d.isWeapon);
    weaponLabelClusters(hostile, (d) => onScreen(contactPos.get(d.id) as Cartesian3) ?? null).forEach((l, i) =>
      this.label(`wl:${i}`, f, this.cart(l.position, l.alt), l.text, '600 11px "Noto Sans"', 'weapon.enemy.label', ENEMY_WEAPON_PX * 0.5 + 6),
    );

    // Targeted glow.
    const pulse = 0.96 + 0.04 * Math.sin((2 * Math.PI * performance.now()) / 2400);
    s.halos.forEach((h, i) => {
      const b = this.bb.get(`halo:${i}`, f);
      this.setImage(b, 'halo', haloImage());
      b.position = this.cart(h.position, h.alt);
      b.scale = pulse;
      b.id = undefined;
      b.show = true;
    });

    this.drawWeapons(w, f, zoom, models);
    for (const e of effectsOf(view, dtS, simPerReal > 0 ? (this.effectSpeed = simPerReal) : this.effectSpeed)) {
      const b = this.bb.get(e.key, f);
      this.setImage(b, e.imageKey, e.image);
      b.position = this.cart(e.position, e.alt);
      b.scale = e.scale;
      b.color = e.color;
      b.rotation = e.rotation;
      b.id = undefined;
      b.show = true;
    }

    this.groundLines(s, w, state, zoom);
    this.groundLabels(f);

    this.bb.sweep(f);
    this.lb.sweep(f);
    this.st.sweep(f);
    this.tr.sweep(f);
    this.sweepModels(f);
    this.visible = visible;
  }

  private drawWeapons(w: WeaponScene, f: number, zoom: number, models: boolean): void {
    const cam = this.widget.scene.camera;
    for (const d of w.weapons) {
      const pid: PickId = { kind: 'weapon', id: d.id, symbol: 120 };
      const k = key(pid);
      const pos = this.cart(d.position, d.alt);
      const token: TokenName = d.interceptor ? 'weapon.interceptor' : 'weapon.offensive';
      const b = this.bb.get(k, f);
      this.setImage(b, `missile:${token}`, missileImage(token));
      b.position = pos;
      b.scale = ((d.interceptor ? 0.85 : 1) * 20) / 32;
      b.rotation = CesiumMath.toRadians(-d.heading) + cam.heading;
      b.id = pid;
      const asModel = this.useModel(k, 120, models);
      b.show = !asModel || !this.modelReady(k);
      if (asModel) this.placeModel(k, 120, pos, d.heading, token, pid, f, this.pitchOf(d));
    }
    for (const t of w.trails) {
      if (t.path.length < 2) continue;
      const p = this.tr.get(`t:${t.id}`, f);
      p.positions = Cartesian3.fromDegreesArrayHeights(t.path.flat());
      if (!p.material || (p as { wbInterceptor?: boolean }).wbInterceptor !== t.interceptor) {
        (p as { wbInterceptor?: boolean }).wbInterceptor = t.interceptor;
        p.material = t.interceptor
          ? Material.fromType('PolylineDash', { color: colour('weapon.interceptor'), dashLength: 8 })
          : Material.fromType('Color', { color: colour('weapon.offensive') });
        p.width = t.interceptor ? 2 : 1.5;
      }
    }
    if (zoom >= TAG_ZOOM) {
      const lat = w.weapons[0]?.position[1] ?? 0;
      weaponTags(w.weapons, TAG_MERGE_PX * mercatorMetresPerPx(zoom, lat)).forEach((t, i) =>
        this.label(`tag:${i}`, f, this.cart(t.position, t.alt), t.text, TAG_FONT, 'label.unit', 0, undefined, 9),
      );
    }
    w.labels.forEach((l, i) => {
      const lead = w.weapons.find((d) => distance(d.position, l.position) < 1) ?? this.leadOf(w.weapons, l.position);
      this.label(`salvo:${i}`, f, this.cart(l.position, l.alt), l.text, LABEL_FONT, 'label.unit', 14, lead ? { kind: 'weapon', id: lead.id } : undefined);
    });
  }

  /** The weapon nearest a salvo label's centroid (what a click on the label inspects). */
  private leadOf(weapons: DrawnWeapon[], at: LonLat): DrawnWeapon | null {
    let best: DrawnWeapon | null = null;
    let bestD = Infinity;
    for (const d of weapons) {
      const dd = distance(d.position, at);
      if (dd < bestD) [best, bestD] = [d, dd];
    }
    return best;
  }

  /** Climb angle of a weapon from its trail (cosmetic; the view has no pitch). */
  private pitchOf(d: DrawnWeapon): number {
    const t = this.trailStore.get(d.id);
    const prev = t[t.length - 2];
    const last = t[t.length - 1];
    if (!prev || !last) return 0;
    const run = distance([prev[0], prev[1]], [last[0], last[1]]);
    return run > 1 ? Math.atan2(last[2] - prev[2], run) : 0;
  }

  /** Rings, ellipses, strobes, routes, waypoints, shooter→target lines, satellite tracks and
   *  orbits: rebuilt when their content changes (on each new view or UI change). */
  private groundLines(s: Scene, w: WeaponScene, state: ReturnType<typeof ui.get>, zoom: number): void {
    const view = this.bridge.frame?.view;
    const selectedUnit = s.units.find((d) => d.id === state.selected) ?? null;
    const details = state.details?.kind === 'group' && state.details.id === state.selected ? state.details : null;
    const o = buildOverlays({
      state,
      details,
      selectedAt: selectedUnit?.position ?? null,
      cursor: this.cursor,
      find: (id) => findUnit(view, id),
      findContact: (id) => findContact(view, id),
    });
    const tracks = s.satellites.map((sat) => ({ id: sat.id, paths: satelliteTrack(this.bridge, sat.id) }));
    const sig = JSON.stringify([
      this.bridge.frame?.receivedAt,
      state.tool,
      this.cursor && state.tool ? this.cursor : null,
      state.selected,
      state.rings,
      state.weaponRing,
      state.planRoutes.length,
      tracks.map((t) => t.paths.length),
      Math.round(zoom),
    ]);
    if (sig === this.groundSig) return;
    this.groundSig = sig;
    this.ground.removeAll();
    this.orbits.removeAll();
    const add = (p: Path) => {
      if (p.path.length < 2) return;
      this.ground.add({
        positions: Cartesian3.fromDegreesArray(p.path.flat()),
        width: p.width,
        id: p.tip ? ({ kind: 'line', id: 0, tip: p.tip } satisfies PickId) : undefined,
        material: p.dashed
          ? Material.fromType('PolylineDash', { color: new Color(p.color[0] / 255, p.color[1] / 255, p.color[2] / 255, p.color[3] / 255), dashLength: 8 })
          : Material.fromType('Color', { color: new Color(p.color[0] / 255, p.color[1] / 255, p.color[2] / 255, p.color[3] / 255) }),
      });
    };
    for (const p of s.contactPaths) add(p);
    for (const p of o.paths) add(p);
    for (const line of w.lines) add({ path: line, color: rgba('weapon.line'), width: 1.5, dashed: true });
    for (const t of tracks)
      for (const path of t.paths) {
        add({ path: path.map((q) => [q[0], q[1]] as LonLat), color: rgba('sat.track'), width: 1.5, dashed: true, tip: `Ground track: ${this.bridge.strings.name(t.id)}` });
        if (path.length > 1)
          this.orbits.add({
            positions: Cartesian3.fromDegreesArrayHeights(path.flat()),
            width: 1.5,
            material: Material.fromType('Color', { color: colour('sat', 0.85) }),
          });
      }
    this.groundTexts = { texts: o.texts, waypoints: o.waypoints, threats: s.threatLabels };
  }

  /** Ring labels, threat labels and waypoints of the ground lines, every frame. */
  private groundLabels(f: number): void {
    const g = this.groundTexts;
    g.texts.forEach((t, i) => this.label(`rt:${i}`, f, this.cart(t.position, 0), t.text, RING_FONT, null, 0, undefined, 12, t.color));
    g.threats.forEach((t, i) => this.label(`th:${i}`, f, this.cart(t.position, 0), t.text, LABEL_FONT, 'ring.threat', -8));
    g.waypoints.forEach((wp, i) => {
      const pos = this.cart(wp.position, 0);
      this.ring(`wp:${i}`, f, pos, 5, 2, 'waypoint');
      this.label(`wpl:${i}`, f, pos, wp.text, '600 12px "Noto Sans"', 'waypoint.label', -24);
    });
  }

  // ---- Primitive helpers --------------------------------------------------------------

  private setImage(b: Billboard, k: string, img: HTMLCanvasElement | null): void {
    const tagged = b as Billboard & { wbImage?: string };
    if (!img || tagged.wbImage === k) return;
    tagged.wbImage = k;
    b.setImage(k, img);
  }

  private label(
    k: string,
    f: number,
    pos: Cartesian3,
    text: string,
    font: string,
    token: TokenName | null,
    dy: number,
    id?: PickId,
    dx = 0,
    rgbaColour?: [number, number, number, number],
  ): void {
    const l = this.lb.get(k, f);
    l.position = pos;
    if (l.text !== text) l.text = text;
    if (l.font !== font) l.font = font;
    l.fillColor = rgbaColour
      ? new Color(rgbaColour[0] / 255, rgbaColour[1] / 255, rgbaColour[2] / 255, rgbaColour[3] / 255)
      : colour(token ?? 'label.unit');
    l.pixelOffset = new Cartesian2(dx, dy);
    l.horizontalOrigin = dx ? HorizontalOrigin.LEFT : HorizontalOrigin.CENTER;
    l.verticalOrigin = dx ? VerticalOrigin.CENTER : VerticalOrigin.TOP;
    l.id = id;
    l.show = true;
  }

  private ring(k: string, f: number, pos: Cartesian3, px: number, width: number, token: TokenName): void {
    const b = this.bb.get(k, f);
    const img = ringImage(Math.round(px), width);
    this.setImage(b, `ring:${Math.round(px)}:${width}`, img);
    b.position = pos;
    b.scale = 1;
    b.color = colour(token);
    b.id = undefined;
    b.show = true;
  }

  private stalk(k: string, f: number, at: LonLat, alt: number, token: TokenName): void {
    const p = this.st.get(k, f);
    p.positions = [this.cart(at, alt), this.cart(at, 0)];
    p.material = Material.fromType('Color', { color: colour(token, 0.55) });
  }

  // ---- Models -------------------------------------------------------------------------

  /** Close in, every object with a class model is drawn as one; the inspect target always. */
  private useModel(k: string, symbol: number, models: boolean): boolean {
    return modelFor(symbol) !== null && (models || this.forceModel === k);
  }

  private modelReady(k: string): boolean {
    return !!this.models.get(k)?.model?.ready;
  }

  private placeModel(k: string, symbol: number, pos: Cartesian3, headingDeg: number, token: TokenName, pid: PickId, f: number, pitch = 0): void {
    const ref = modelFor(symbol);
    if (!ref) return;
    let m = this.models.get(k);
    if (m && m.name !== ref.name) {
      if (m.model) this.widget.scene.primitives.remove(m.model);
      this.models.delete(k);
      m = undefined;
    }
    if (!m) {
      const entry = { model: null as Model | null, name: ref.name, seen: f, loading: true };
      this.models.set(k, entry);
      void Model.fromGltfAsync({
        url: modelUrl(ref.name),
        scale: ref.lengthM,
        minimumPixelSize: MODEL_MIN_PX,
        upAxis: Axis.Y,
        forwardAxis: Axis.X,
        color: colour(token),
        colorBlendMode: ColorBlendMode.MIX,
        colorBlendAmount: 0.55,
        id: pid,
      }).then((model) => {
        if (this.widget.isDestroyed() || this.models.get(k) !== entry) {
          model.destroy();
          return;
        }
        entry.model = this.widget.scene.primitives.add(model);
        entry.loading = false;
      });
      m = entry;
    }
    m.seen = f;
    if (!m.model) return;
    // Compass heading (clockwise from north) → Cesium heading (from east, about −Z).
    const hpr = new HeadingPitchRoll(CesiumMath.toRadians(headingDeg - 90), pitch, 0);
    FixedFrameTransforms.headingPitchRollToFixedFrame(pos, hpr, Ellipsoid.WGS84, undefined, m.model.modelMatrix);
    m.model.show = true;
  }

  private sweepModels(f: number): void {
    for (const [k, m] of this.models)
      if (m.seen !== f) {
        if (m.model) this.widget.scene.primitives.remove(m.model);
        this.models.delete(k);
      }
  }

  // ---- Test hooks (doc 09 P10 parity) -------------------------------------------------

  /** Selectable ids of the scene (`u:1`, `c:7`, `s:3`), as the 2D map reports them. */
  selectables(): string[] {
    return this.last ? selectableIds(this.last.scene) : [];
  }

  /** Window position of a selectable id, or null when off screen or behind the globe. */
  windowPosition(id: string): { x: number; y: number } | null {
    const s = this.last?.scene;
    if (!s) return null;
    const [kind, raw] = id.split(':');
    const n = Number(raw);
    const d =
      kind === 'u' ? s.units.find((x) => x.id === n) : kind === 'c' ? s.contacts.find((x) => x.id === n) : s.satellites.find((x) => x.id === n);
    if (!d) return null;
    const pos = this.cart(d.position, d.alt);
    const occluder = new EllipsoidalOccluder(Ellipsoid.WGS84, this.widget.scene.camera.positionWC);
    if (!occluder.isPointVisible(pos)) return null;
    const px = SceneTransforms.worldToWindowCoordinates(this.widget.scene, pos);
    return px ? { x: px.x, y: px.y } : null;
  }

  /** Whether a click at the id's window position reaches it (drill pick through overlaps). */
  pickable(id: string): boolean {
    const px = this.windowPosition(id);
    if (!px) return false;
    const hits = this.widget.scene.drillPick(new Cartesian2(px.x, px.y), 20, 6, 6) as { id?: PickId }[];
    const [kind] = id.split(':');
    const want = kind === 'u' ? 'unit' : kind === 'c' ? 'contact' : 'satellite';
    return hits.some((h) => h.id?.kind === want && `${kind}:${h.id.id}` === id);
  }

  /** Last-frame state of a drawn unit, for the cameras and the inspect HUD. */
  unit(id: number): DrawnUnit | null {
    return this.last?.scene.units.find((d) => d.id === id) ?? null;
  }

  contact(id: number): DrawnContact | null {
    return this.last?.scene.contacts.find((d) => d.id === id) ?? null;
  }

  weapon(id: number): DrawnWeapon | null {
    return this.last?.weapons.weapons.find((d) => d.id === id) ?? null;
  }

  poolSizes(): { billboards: number; labels: number; models: number } {
    return { billboards: this.bb.keys().length, labels: this.lb.keys().length, models: this.models.size };
  }
}
