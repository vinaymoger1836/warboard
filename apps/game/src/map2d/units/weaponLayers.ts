// Our weapons in flight on the 2D map (doc 10 §5, references 02 and 04): amber missile
// glyphs with a 1.5 px amber trail and `M-n` tags, cyan dashed interceptor trails with a
// ring-dot head and `INT-n` tags, salvo labels at each salvo's centroid, interceptor group
// labels at each shooter's interceptors, and dashed amber shooter→target lines. Other
// sides' weapons are contacts of our picture and drawn with the contacts.
import type { Layer } from '@deck.gl/core';
import { PathStyleExtension, type PathStyleExtensionProps } from '@deck.gl/extensions';
import { IconLayer, PathLayer, ScatterplotLayer, TextLayer } from '@deck.gl/layers';
import { RELATION, type SideView, type StringTable } from '@warboard/protocol';
import {
  interceptorsLabel,
  missileDataUrl,
  rgba,
  salvoLabel,
  type TokenName,
  unitLabel,
  weaponTag,
} from '@warboard/symbols';
import type { LonLat } from '../../store';
import { destination, distance } from './geo';

/** Per-weapon tags from this zoom (doc 10 §5). */
const TAG_ZOOM = 7;
/** Trail points kept per weapon (one per side view, ~10 s at ×1). */
const TRAIL_POINTS = 120;
const MISSILE_PX = 20;
/** Tags closer than this on screen share one label, `M-8 M-6` (reference 04). */
const TAG_MERGE_PX = 28;

interface DrawnWeapon {
  id: number;
  position: LonLat;
  heading: number;
  interceptor: boolean;
  tag: string;
}

interface Trail {
  path: LonLat[];
  interceptor: boolean;
}

interface Label {
  position: LonLat;
  text: string;
}

const dash = new PathStyleExtension({ dash: true });
const SDF = { sdf: true, fontSize: 64, buffer: 10, radius: 14, cutoff: 0.18 };
const c = (t: TokenName, a = 1) => rgba(t, a);
const MISSILE_ICON = { url: missileDataUrl(), width: 32, height: 32, id: 'missile', mask: false };

const centroid = (pts: LonLat[]): LonLat => [
  pts.reduce((s, p) => s + p[0], 0) / pts.length,
  pts.reduce((s, p) => s + p[1], 0) / pts.length,
];

export class WeaponLayers {
  /** Reported positions per weapon id, one per side view. */
  private readonly trails = new Map<number, LonLat[]>();
  private lastFrame = -1;

  /** `targetAt` gives a contact's position as drawn this frame. */
  layers(
    view: SideView,
    receivedAt: number,
    strings: StringTable,
    dtS: number,
    zoom: number,
    targetAt: (contact: number) => LonLat | null,
  ): Layer[] {
    const w = view.weapons;
    const u = view.units;
    if (receivedAt !== this.lastFrame) {
      this.lastFrame = receivedAt;
      const live = new Set<number>();
      for (let i = 0; i < w.count; i++) {
        const id = w.id[i] ?? 0;
        live.add(id);
        const t = this.trails.get(id) ?? [];
        t.push([w.lon[i] ?? 0, w.lat[i] ?? 0]);
        if (t.length > TRAIL_POINTS) t.shift();
        this.trails.set(id, t);
      }
      for (const id of this.trails.keys()) if (!live.has(id)) this.trails.delete(id);
    }

    const weapons: DrawnWeapon[] = [];
    const trails: Trail[] = [];
    const salvos = new Map<number, { type: string; pts: LonLat[] }>();
    const intercepts = new Map<number, LonLat[]>();
    const lines = new Map<string, LonLat[]>();
    const shooterAt = (id: number): LonLat | null => {
      const i = view.index.get(id);
      return i === undefined ? null : [u.lon[i] ?? 0, u.lat[i] ?? 0];
    };
    for (let i = 0; i < w.count; i++) {
      const id = w.id[i] ?? 0;
      const interceptor = (w.relation[i] ?? 0) === RELATION.INTERCEPTOR;
      const heading = w.headingDeg[i] ?? 0;
      const v = w.speedMps[i] ?? 0;
      const p: LonLat = [w.lon[i] ?? 0, w.lat[i] ?? 0];
      const position = v > 0 && dtS > 0 ? destination(p, heading, v * dtS) : p;
      weapons.push({ id, position, heading, interceptor, tag: weaponTag(interceptor, w.tag[i] ?? 0) });
      trails.push({ path: [...(this.trails.get(id) ?? []), position], interceptor });
      const shooter = w.shooter[i] ?? 0;
      if (interceptor) {
        const pts = intercepts.get(shooter) ?? [];
        pts.push(position);
        intercepts.set(shooter, pts);
        continue;
      }
      const salvo = w.salvo[i] ?? 0;
      const s = salvos.get(salvo) ?? { type: strings.type(id), pts: [] };
      s.pts.push(position);
      salvos.set(salvo, s);
      const target = w.target[i] ?? 0;
      const from = shooterAt(shooter);
      const to = target ? targetAt(target) : null;
      if (from && to) lines.set(`${shooter}:${target}`, [from, to]);
    }

    const labels: Label[] = [...salvos.values()].map((s) => ({
      position: centroid(s.pts),
      text: salvoLabel(s.type, s.pts.length),
    }));
    for (const [shooter, pts] of intercepts) {
      const i = view.index.get(shooter);
      const symbol = i === undefined ? 0 : (u.symbol[i] ?? 0);
      const group = i === undefined ? 0 : (u.group[i] ?? 0);
      const name = unitLabel({ symbol, typeName: strings.type(shooter), name: strings.name(group || shooter), count: 1 });
      labels.push({ position: centroid(pts), text: interceptorsLabel(name, pts.length) });
    }
    const missiles = weapons.filter((d) => !d.interceptor);
    const heads = weapons.filter((d) => d.interceptor);
    // Web Mercator metres per pixel at this zoom and latitude.
    const lat = weapons[0]?.position[1] ?? 0;
    const merge = TAG_MERGE_PX * ((156_543 * Math.cos((lat * Math.PI) / 180)) / 2 ** zoom);
    const tags: Label[] = [];
    if (zoom >= TAG_ZOOM) {
      for (const d of weapons) {
        const near = tags.find((t) => distance(t.position, d.position) < merge);
        if (near) near.text += ` ${d.tag}`;
        else tags.push({ position: d.position, text: d.tag });
      }
    }

    return [
      new PathLayer<LonLat[], PathStyleExtensionProps<LonLat[]>>({
        id: 'weapon-lines',
        data: [...lines.values()],
        getPath: (d) => d,
        getColor: c('weapon.line'),
        getWidth: 1.5,
        widthUnits: 'pixels',
        getDashArray: [5, 4],
        dashJustified: true,
        extensions: [dash],
      }),
      new PathLayer<Trail, PathStyleExtensionProps<Trail>>({
        id: 'weapon-trails',
        data: trails.filter((t) => t.path.length > 1),
        getPath: (d) => d.path,
        getColor: (d) => (d.interceptor ? c('weapon.interceptor') : c('weapon.offensive')),
        getWidth: (d) => (d.interceptor ? 2 : 1.5),
        widthUnits: 'pixels',
        getDashArray: (d) => (d.interceptor ? [4, 3] : [0, 0]),
        dashJustified: true,
        extensions: [dash],
      }),
      new IconLayer<DrawnWeapon>({
        id: 'missile-icons',
        data: missiles,
        getIcon: () => MISSILE_ICON,
        getPosition: (d) => d.position,
        getAngle: (d) => -d.heading,
        getSize: MISSILE_PX,
        sizeUnits: 'pixels',
      }),
      new ScatterplotLayer<DrawnWeapon>({
        id: 'interceptor-rings',
        data: heads,
        getPosition: (d) => d.position,
        getRadius: 5,
        radiusUnits: 'pixels',
        stroked: true,
        filled: true,
        getFillColor: c('map.label.halo', 0.6),
        getLineColor: c('weapon.interceptor'),
        getLineWidth: 1.5,
        lineWidthUnits: 'pixels',
      }),
      new ScatterplotLayer<DrawnWeapon>({
        id: 'interceptor-dots',
        data: heads,
        getPosition: (d) => d.position,
        getRadius: 1.8,
        radiusUnits: 'pixels',
        getFillColor: c('glyph'),
      }),
      new TextLayer<Label>({
        id: 'weapon-tags',
        data: tags,
        getPosition: (d) => d.position,
        getText: (d) => d.text,
        getSize: 11,
        getColor: c('label.unit'),
        getPixelOffset: [9, 0],
        getTextAnchor: 'start',
        getAlignmentBaseline: 'center',
        fontFamily: 'JetBrains Mono',
        fontSettings: SDF,
        outlineWidth: 0.3,
        outlineColor: c('map.label.halo'),
        characterSet: 'auto',
      }),
      new TextLayer<Label>({
        id: 'salvo-labels',
        data: labels,
        getPosition: (d) => d.position,
        getText: (d) => d.text,
        getSize: 11,
        getColor: c('label.unit'),
        getPixelOffset: [0, 14],
        getAlignmentBaseline: 'top',
        fontFamily: 'Noto Sans',
        fontWeight: 'italic',
        fontSettings: SDF,
        outlineWidth: 0.3,
        outlineColor: c('map.label.halo'),
        characterSet: 'auto',
      }),
    ];
  }
}
