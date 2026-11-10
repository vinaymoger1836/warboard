// Our weapons in flight as drawable data (doc 10 §5), shared by the 2D map and the 3D view:
// positions, trails (with altitude, so 3D can draw true trajectories), salvo and interceptor
// labels and shooter→target lines. Other sides' weapons are contacts of our picture.
import { RELATION, type SideView, type StringTable } from '@warboard/protocol';
import { interceptorsLabel, salvoLabel, unitLabel, weaponTag } from '@warboard/symbols';
import { destination, distance, type LonLat } from './geo';

/** Trail points kept per weapon (one per side view, ~10 s at ×1). */
const TRAIL_POINTS = 120;

/** A point with altitude, m. */
export type LonLatAlt = [number, number, number];

export interface DrawnWeapon {
  id: number;
  symbol: number;
  position: LonLat;
  alt: number;
  heading: number;
  speed: number;
  interceptor: boolean;
  tag: string;
  shooter: number;
  target: number;
}

export interface Trail {
  id: number;
  path: LonLatAlt[];
  interceptor: boolean;
}

export interface WeaponLabel {
  position: LonLat;
  alt: number;
  text: string;
}

export interface WeaponScene {
  weapons: DrawnWeapon[];
  trails: Trail[];
  /** Dashed shooter→target lines (ground). */
  lines: LonLat[][];
  labels: WeaponLabel[];
}

const centroid = (pts: LonLatAlt[]): LonLatAlt => [
  pts.reduce((s, p) => s + p[0], 0) / pts.length,
  pts.reduce((s, p) => s + p[1], 0) / pts.length,
  pts.reduce((s, p) => s + p[2], 0) / pts.length,
];

/** Reported positions per weapon id, one per side view. Each view keeps its own. */
export class TrailStore {
  private readonly trails = new Map<number, LonLatAlt[]>();
  private lastFrame = -1;

  update(view: SideView, receivedAt: number): void {
    if (receivedAt === this.lastFrame) return;
    this.lastFrame = receivedAt;
    const w = view.weapons;
    const live = new Set<number>();
    for (let i = 0; i < w.count; i++) {
      const id = w.id[i] ?? 0;
      live.add(id);
      const t = this.trails.get(id) ?? [];
      t.push([w.lon[i] ?? 0, w.lat[i] ?? 0, w.altM[i] ?? 0]);
      if (t.length > TRAIL_POINTS) t.shift();
      this.trails.set(id, t);
    }
    for (const id of this.trails.keys()) if (!live.has(id)) this.trails.delete(id);
  }

  get(id: number): LonLatAlt[] {
    return this.trails.get(id) ?? [];
  }
}

/** `targetAt` gives a contact's position as drawn this frame. */
export function buildWeapons(
  view: SideView,
  strings: StringTable,
  store: TrailStore,
  dtS: number,
  targetAt: (contact: number) => LonLat | null,
): WeaponScene {
  const w = view.weapons;
  const u = view.units;
  const weapons: DrawnWeapon[] = [];
  const trails: Trail[] = [];
  const salvos = new Map<number, { type: string; pts: LonLatAlt[] }>();
  const intercepts = new Map<number, LonLatAlt[]>();
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
    const alt = w.altM[i] ?? 0;
    const p: LonLat = [w.lon[i] ?? 0, w.lat[i] ?? 0];
    const position = v > 0 && dtS > 0 ? destination(p, heading, v * dtS) : p;
    const shooter = w.shooter[i] ?? 0;
    const target = w.target[i] ?? 0;
    weapons.push({
      id,
      symbol: w.symbol[i] ?? 0,
      position,
      alt,
      heading,
      speed: v,
      interceptor,
      tag: weaponTag(interceptor, w.tag[i] ?? 0),
      shooter,
      target,
    });
    const here: LonLatAlt = [position[0], position[1], alt];
    trails.push({ id, path: [...store.get(id), here], interceptor });
    if (interceptor) {
      const pts = intercepts.get(shooter) ?? [];
      pts.push(here);
      intercepts.set(shooter, pts);
      continue;
    }
    const salvo = w.salvo[i] ?? 0;
    const s = salvos.get(salvo) ?? { type: strings.type(id), pts: [] };
    s.pts.push(here);
    salvos.set(salvo, s);
    const from = shooterAt(shooter);
    const to = target ? targetAt(target) : null;
    if (from && to) lines.set(`${shooter}:${target}`, [from, to]);
  }

  const labels: WeaponLabel[] = [...salvos.values()].map((s) => {
    const m = centroid(s.pts);
    return { position: [m[0], m[1]], alt: m[2], text: salvoLabel(s.type, s.pts.length) };
  });
  for (const [shooter, pts] of intercepts) {
    const i = view.index.get(shooter);
    const symbol = i === undefined ? 0 : (u.symbol[i] ?? 0);
    const group = i === undefined ? 0 : (u.group[i] ?? 0);
    const name = unitLabel({ symbol, typeName: strings.type(shooter), name: strings.name(group || shooter), count: 1 });
    const m = centroid(pts);
    labels.push({ position: [m[0], m[1]], alt: m[2], text: interceptorsLabel(name, pts.length) });
  }
  return { weapons, trails, lines: [...lines.values()], labels };
}

/** Per-weapon tags (`M-8 M-6`, reference 04): tags closer than `mergeM` share one label. */
export function weaponTags(weapons: DrawnWeapon[], mergeM: number): WeaponLabel[] {
  const tags: WeaponLabel[] = [];
  for (const d of weapons) {
    const near = tags.find((t) => distance(t.position, d.position) < mergeM);
    if (near) near.text += ` ${d.tag}`;
    else tags.push({ position: d.position, alt: d.alt, text: d.tag });
  }
  return tags;
}
