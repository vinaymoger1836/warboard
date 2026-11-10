// The side picture as drawable items (doc 06 §4–5, ADR-047): one pure function shared by the
// 2D map and the 3D view, so both draw (and let the player select) exactly the same objects.
// Positions are dead-reckoned from the last side view (heading × speed × sim time since the
// view); altitude comes straight from the view. Nothing here reads anything but the view, the
// string table and the UI state, so the truth rule (CLAUDE.md rule 1) holds by construction.
import { CFLAG, contactDomain, FLAG, RELATION, type SideView, type StringTable, statusOf } from '@warboard/protocol';
import { contactLabel, ESTIMATE_LINE, type IconSide, markAbove, rgba, type TokenName, unitLabel } from '@warboard/symbols';
import { destination, geodesicCircle, type LonLat } from './geo';

/** Zoom below which icons shrink to 24 px (doc 10 §4). */
export const SMALL_ZOOM = 5;
/** Bases are drawn from this zoom. */
export const BASE_ZOOM = 5;
/** Group members are drawn one by one from this zoom; below it a flight is one icon. */
export const MEMBERS_ZOOM = 11;
/** Base labels from this zoom (selected base always). */
export const BASE_LABEL_ZOOM = 7;
/** Dead reckoning runs at most this long past a view (real ms). */
export const MAX_EXTRAPOLATE_MS = 400;

export type Rgba = [number, number, number, number];
export const c = (t: TokenName, a = 1): Rgba => rgba(t, a);

export interface DrawnUnit {
  /** Group id for flights and ships, base id for bases. */
  id: number;
  /** The unit drawn: a member's own id (zoomed in), the group lead's (collapsed), or the base. */
  uid: number;
  /** The group's lead (or a unit without a group): cameras follow it. */
  lead: boolean;
  symbol: number;
  position: LonLat;
  /** Altitude above sea level, m (0 for ships and ground units). */
  alt: number;
  heading: number;
  speed: number;
  label: string;
  mark: string;
  base: boolean;
  /** Umpire view (doc 12): side B's units are drawn in the enemy colour. */
  enemy?: boolean;
}

/** What a click or right-click hit, in either view. */
export interface MapPick {
  id: number;
  base: boolean;
  aircraft: boolean;
  /** A contact of our picture (id is a contact id). */
  contact: boolean;
  satellite: boolean;
}

/** A right-click on the map or globe: screen px, ground position, what was under it. */
export interface ContextRequest {
  x: number;
  y: number;
  at: LonLat;
  target: MapPick | null;
}

export interface DrawnContact {
  id: number;
  symbol: number;
  side: IconSide;
  estimated: boolean;
  /** Not stale, not estimated, identity and class known: the inspect view may open on it (ADR-047). */
  identified: boolean;
  position: LonLat;
  alt: number;
  heading: number;
  speed: number;
  label: string;
  isWeapon: boolean;
  /** Weapon contacts: the type or class name, and the estimated speed. */
  typeName: string;
  speedKmh: number;
}

export interface DrawnSatellite {
  id: number;
  symbol: number;
  position: LonLat;
  alt: number;
  label: string;
}

/** A line on the map; `ground` lines are draped on terrain in 3D, the others keep `alts`. */
export interface Path {
  path: LonLat[];
  color: Rgba;
  width: number;
  dashed: boolean;
  /** What the line is, shown when the cursor rests on it (doc 10 §6). */
  tip?: string;
}

export interface TextItem {
  position: LonLat;
  text: string;
  color: Rgba;
}

export interface SceneOptions {
  /** Map zoom (2D) or the equivalent zoom of the 3D camera (`lod.ts`). */
  zoom: number;
  /** Sim seconds since the view arrived (dead reckoning). */
  dtS: number;
  selected: number | null;
  threatRings: boolean;
  allSensorRings: boolean;
}

export interface Scene {
  units: DrawnUnit[];
  satellites: DrawnSatellite[];
  contacts: DrawnContact[];
  /** Estimate ellipses, bearing strobes, threat and sensor rings (all on the ground). */
  contactPaths: Path[];
  threatLabels: TextItem[];
  /** Targeted glow positions (own units flagged, contacts our weapons fly at). */
  halos: { position: LonLat; alt: number }[];
}

/** Satellite symbols (60–62) are drawn as yellow dots, not atlas icons (doc 10 §4). */
export const isSatellite = (symbol: number) => symbol >= 60 && symbol < 70;
const IDENTITY_SIDE: IconSide[] = ['unknown', 'enemy', 'neutral', 'own'];

export const km = (m: number) => `${Math.round(m / 1000).toLocaleString('en-GB')} km`;

/** An ellipse of semi-axes `a`, `b` (m), major axis along `rotDeg`, as a closed path. */
export function ellipse(centre: LonLat, a: number, b: number, rotDeg: number): LonLat[] {
  const pts: LonLat[] = [];
  for (let t = 0; t <= 360; t += 10) {
    const r = (t * Math.PI) / 180;
    const along = destination(centre, rotDeg, a * Math.cos(r));
    pts.push(destination(along, rotDeg + 90, b * Math.sin(r)));
  }
  return pts;
}

/** Splits a lon/lat path where it crosses the antimeridian. */
export function splitAntimeridian<P extends readonly number[]>(pts: P[]): P[][] {
  const out: P[][] = [];
  let cur: P[] = [];
  for (const p of pts) {
    const last = cur[cur.length - 1];
    if (last && Math.abs((p[0] ?? 0) - (last[0] ?? 0)) > 180) {
      out.push(cur);
      cur = [];
    }
    cur.push(p);
  }
  if (cur.length > 1) out.push(cur);
  return out;
}

/** Whether a contact of our picture is identified enough to inspect close up (ADR-047): not
 *  estimated, not stale, not bearing-only, identity and class known. */
export function identifiedContact(view: SideView, strings: StringTable, id: number): boolean {
  const i = view.contactIndex.get(id);
  if (i === undefined) return false;
  const flags = view.contacts.flags[i] ?? 0;
  if (flags & (CFLAG.ESTIMATED | CFLAG.STALE | CFLAG.BEARING_ONLY)) return false;
  return (view.contacts.identity[i] ?? 0) !== 0 && (view.contacts.symbol[i] ?? 0) !== 0 && strings.contactType(id) !== '';
}

/** Whether an own group, unit, base or satellite is drawn on the map (not parked at a base),
 *  so the inspect view has something to look at. */
export function drawnInView(view: SideView, id: number): boolean {
  const u = view.units;
  for (let i = 0; i < u.count; i++) {
    if ((u.id[i] === id || u.group[i] === id) && ((u.flags[i] ?? 0) & FLAG.AT_BASE) === 0) return true;
  }
  return false;
}

/** Builds everything of the side picture both views draw. */
export function buildScene(view: SideView, strings: StringTable, o: SceneOptions): Scene {
  const u = view.units;
  const { zoom, dtS } = o;
  const units: DrawnUnit[] = [];
  const satellites: DrawnSatellite[] = [];
  const groups = new Map<number, { index: number; count: number }>();
  /** Rows of own units the engine flags as targeted by a weapon of our picture. */
  const targetedRows: number[] = [];
  const at = (i: number): LonLat => {
    const p: LonLat = [u.lon[i] ?? 0, u.lat[i] ?? 0];
    const v = u.speedMps[i] ?? 0;
    return v > 0 && dtS > 0 ? destination(p, u.headingDeg[i] ?? 0, v * dtS) : p;
  };
  const row = (i: number) => ({ alt: u.altM[i] ?? 0, heading: u.headingDeg[i] ?? 0, speed: u.speedMps[i] ?? 0 });
  // The umpire view carries both sides exactly: side A as own, side B as enemy (doc 10 §8).
  const enemyRow = (i: number) => view.umpire && (u.side[i] ?? 0) !== 0;
  for (let i = 0; i < u.count; i++) {
    const flags = u.flags[i] ?? 0;
    const id = u.id[i] ?? 0;
    const symbol = u.symbol[i] ?? 0;
    if (flags & FLAG.BASE) {
      if (zoom < BASE_ZOOM && id !== o.selected) continue;
      units.push({
        id,
        uid: id,
        lead: true,
        symbol,
        position: at(i),
        ...row(i),
        label: unitLabel({ symbol, typeName: '', name: strings.name(id), count: 1 }),
        mark: '',
        base: true,
        enemy: enemyRow(i),
      });
      continue;
    }
    if (flags & FLAG.AT_BASE) continue;
    if (isSatellite(symbol)) {
      const owner = strings.owners.get(id) ?? '';
      satellites.push({
        id,
        symbol,
        position: at(i),
        alt: u.altM[i] ?? 0,
        label: `${owner} ${strings.name(id)}\n${strings.payloads.get(id) ?? ''}`.trim(),
      });
      continue;
    }
    if (flags & FLAG.TARGETED) targetedRows.push(i);
    const group = u.group[i] ?? 0;
    if (zoom >= MEMBERS_ZOOM || group === 0) {
      units.push({
        id: group || id,
        uid: id,
        lead: group === 0 || (flags & FLAG.GROUP_LEAD) !== 0,
        symbol,
        position: at(i),
        ...row(i),
        label: strings.name(id),
        mark: markAbove(symbol),
        base: false,
        enemy: enemyRow(i),
      });
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
    if (group === o.selected) {
      const fuel = u.fuelPct[i] ?? 255;
      label += ` [${statusOf(u.flags[i] ?? 0)}]${fuel <= 100 ? ` ${fuel}%` : ''}`;
    }
    if ((u.flags[i] ?? 0) & FLAG.JAMMED) label += ' [JAMMED]';
    units.push({ id: group, uid: id, lead: true, symbol, position: at(i), ...row(i), label, mark: markAbove(symbol), base: false, enemy: enemyRow(i) });
  }

  // Contacts of our picture: estimates only.
  const cv = view.contacts;
  const contacts: DrawnContact[] = [];
  const contactPaths: Path[] = [];
  const threatLabels: TextItem[] = [];
  for (let i = 0; i < cv.count; i++) {
    const id = cv.id[i] ?? 0;
    const flags = cv.flags[i] ?? 0;
    const p: LonLat = [cv.lon[i] ?? 0, cv.lat[i] ?? 0];
    const identity = cv.identity[i] ?? 0;
    const side = IDENTITY_SIDE[identity] ?? 'unknown';
    const colour = c(side === 'enemy' ? 'side.enemy' : side === 'neutral' ? 'side.neutral' : 'side.unknown', 0.8);
    if (flags & CFLAG.BEARING_ONLY) {
      contactPaths.push({
        path: [p, destination(p, cv.headingDeg[i] ?? 0, cv.ellipseA[i] ?? 0)],
        color: colour,
        width: 1.5,
        dashed: true,
        tip: 'Bearing to an emitter (passive detection, no range yet)',
      });
      continue;
    }
    const stale = (flags & CFLAG.STALE) !== 0;
    const v = cv.speedMps[i] ?? 0;
    const pos = !stale && v > 0 && dtS > 0 ? destination(p, cv.headingDeg[i] ?? 0, v * dtS) : p;
    const estimated = (flags & CFLAG.ESTIMATED) !== 0;
    const symbol = cv.symbol[i] ?? 0;
    const isWeapon = symbol >= 120 && symbol <= 122;
    const typeName = strings.contactType(id);
    // A hostile weapon is labelled with its type, then its speed (doc 10 §4.1, `SL-20` /
    // `(4288 km/h)`); those labels are built per cluster in `weaponLabelClusters`.
    let label = isWeapon
      ? ''
      : contactLabel({
          symbol,
          typeName,
          domain: contactDomain(flags),
          countMin: cv.countMin[i] ?? 1,
          countMax: cv.countMax[i] ?? 1,
          countKnown: (flags & CFLAG.COUNT_KNOWN) !== 0,
        });
    // A fixed-size ring would cut through the label at mid zoom and vanish at far zoom, so
    // the badge alone marks an affected radar.
    if ((flags & CFLAG.JAMMED) !== 0) label += '\n[JAMMED]';
    if (estimated && !isWeapon) label += `\n${ESTIMATE_LINE}`;
    contacts.push({
      id,
      symbol,
      side,
      estimated,
      identified: identifiedContact(view, strings, id),
      position: pos,
      alt: cv.altM[i] ?? 0,
      heading: cv.headingDeg[i] ?? 0,
      speed: v,
      label,
      isWeapon,
      typeName: typeName || 'WEAPON',
      speedKmh: Math.round(v * 3.6),
    });
    const a = cv.ellipseA[i] ?? 0;
    const b = cv.ellipseB[i] ?? 0;
    if (estimated && a > 50) {
      const tint = isWeapon ? c('weapon.enemy', 0.8) : colour;
      contactPaths.push({
        path: ellipse(pos, a, Math.max(b, 50), cv.ellipseRotDeg[i] ?? 0),
        color: tint,
        width: 1,
        dashed: true,
        tip: `Position uncertainty (our estimate): ±${km(a)}`,
      });
    }
    const threat = cv.threatM[i] ?? 0;
    if (o.threatRings && threat > 0) {
      contactPaths.push({
        path: geodesicCircle(pos, threat),
        color: c('ring.threat'),
        width: 1.5,
        dashed: true,
        tip: `Threat range (our estimate): ${km(threat)}${typeName ? ` · ${typeName}` : ''}`,
      });
      threatLabels.push({ position: destination(pos, 0, threat), text: '(est.)', color: c('ring.threat') });
    }
  }

  // Sensor rings of every own unit (layers button).
  if (o.allSensorRings) {
    const rowOf = new Map<number, number>();
    for (let k = 0; k < u.count; k++) {
      const g = u.group[k] ?? 0;
      if (g && !rowOf.has(g)) rowOf.set(g, k);
    }
    for (const d of units) {
      if (d.base) continue;
      const i = view.index.get(d.id) ?? rowOf.get(d.id);
      const r = i === undefined ? 0 : (u.sensorKm[i] ?? 0) * 1000;
      if (r > 0)
        contactPaths.push({
          path: geodesicCircle(d.position, r),
          color: c('ring.sensor', 0.45),
          width: 1,
          dashed: true,
          tip: `Sensor horizon: ${km(r)} · ${strings.name(d.id)}`,
        });
    }
  }

  // Targeted glow (doc 10 §5): own units the engine flags (a weapon of our picture is judged
  // aimed at them), and contacts one of our own offensive weapons is flying at. Both come from
  // the view; a collapsed group glows at its icon.
  const halos: { position: LonLat; alt: number }[] = [];
  if (targetedRows.length > 0) {
    const drawnAt = new Map(units.map((d) => [d.id, d]));
    for (const i of targetedRows) {
      const group = u.group[i] ?? 0;
      const collapsed = group !== 0 && zoom < MEMBERS_ZOOM;
      const d = collapsed ? drawnAt.get(group) : undefined;
      const p = collapsed ? d?.position : at(i);
      const alt = collapsed ? (d?.alt ?? 0) : (u.altM[i] ?? 0);
      if (p && !halos.some((h) => h.position[0] === p[0] && h.position[1] === p[1])) halos.push({ position: p, alt });
    }
  }
  if (view.weapons.count > 0) {
    const aimedAt = new Set<number>();
    const w = view.weapons;
    for (let k = 0; k < w.count; k++) {
      if ((w.relation[k] ?? 0) === RELATION.OURS && (w.target[k] ?? 0) !== 0) aimedAt.add(w.target[k] ?? 0);
    }
    for (const d of contacts) if (aimedAt.has(d.id) && !d.isWeapon) halos.push({ position: d.position, alt: d.alt });
  }

  return { units, satellites, contacts, contactPaths, threatLabels, halos };
}

/** What a view can select (the parity set, doc 09 P10): `u:<id>`, `c:<id>`, `s:<id>`.
 *  Bearing-only contacts are lines, not selectable; own weapons are not selectable. */
export function selectableIds(scene: Scene): string[] {
  const ids = new Set([
    ...scene.units.map((d) => `u:${d.id}`),
    ...scene.contacts.map((d) => `c:${d.id}`),
    ...scene.satellites.map((d) => `s:${d.id}`),
  ]);
  return [...ids].sort();
}

/** Labels give way when crowded, icons never do (doc 10 §4.1): greedy by priority
 *  (selection, units, bases), dropping any label whose screen box overlaps one kept.
 *  `project` gives screen px, or null when the point is not on screen. */
export function declutter(
  items: DrawnUnit[],
  selected: number | null,
  size: number,
  project: (d: DrawnUnit) => { x: number; y: number } | null,
): DrawnUnit[] {
  const prio = (d: DrawnUnit) => (d.id === selected ? 0 : d.base ? 2 : 1);
  const sorted = [...items].sort((a, b) => prio(a) - prio(b) || a.id - b.id);
  const kept: DrawnUnit[] = [];
  const boxes: [number, number, number, number][] = [];
  for (const d of sorted) {
    const p = project(d);
    if (!p) continue;
    const w = d.label.length * LABEL_CHAR_PX;
    const y = p.y + size * 0.55;
    const box: [number, number, number, number] = [p.x - w / 2, y, p.x + w / 2, y + LABEL_LINE_PX];
    if (boxes.some((b) => box[0] < b[2] && box[2] > b[0] && box[1] < b[3] && box[3] > b[1])) continue;
    boxes.push(box);
    kept.push(d);
  }
  return kept;
}

/** Rough label box per character and line height, px (11 px italic Noto Sans). */
export const LABEL_CHAR_PX = 5.6;
export const LABEL_LINE_PX = 15;
export const ENEMY_WEAPON_PX = 20;

/** Labels of hostile weapons (doc 10 §4.1): `type` over `(speed)`, red. Weapons whose labels
 *  would overlap on screen share one label with a count (`anti-ship missile ×6`), so a raid
 *  of a dozen missiles stays legible; the glyphs are never merged. */
export function weaponLabelClusters(
  weapons: DrawnContact[],
  project: (d: DrawnContact) => { x: number; y: number } | null,
): { position: LonLat; alt: number; text: string }[] {
  const kept: { position: LonLat; alt: number; type: string; speed: number; count: number; box: [number, number, number, number] }[] = [];
  for (const d of weapons) {
    const p = project(d);
    if (!p) continue;
    const w = Math.max(d.typeName.length + 3, 12) * LABEL_CHAR_PX;
    const y = p.y + ENEMY_WEAPON_PX * 0.5 + 6;
    const box: [number, number, number, number] = [p.x - w / 2, y, p.x + w / 2, y + 2 * LABEL_LINE_PX];
    const near = kept.find((k) => box[0] < k.box[2] && box[2] > k.box[0] && box[1] < k.box[3] && box[3] > k.box[1]);
    if (near) near.count++;
    else kept.push({ position: d.position, alt: d.alt, type: d.typeName, speed: d.speedKmh, count: 1, box });
  }
  return kept.map((k) => ({
    position: k.position,
    alt: k.alt,
    text: `${k.type}${k.count > 1 ? ` ×${k.count}` : ''}\n(${k.speed} km/h)`,
  }));
}
