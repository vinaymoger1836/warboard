// Overlays of the selection, the operation planner and the drawing tools (doc 10 §6–7), as
// pure data shared by the 2D map and the 3D view: range rings, route or patrol circle with WP
// markers, plan routes and the tool preview. All of them lie on the ground.
import type { GroupDetails, SideView } from '@warboard/protocol';
import type { PlanRoute, Tool, UiState } from '../store';
import { destination, distance, geodesicCircle, type LonLat } from './geo';
import { c, km, type Path, type TextItem } from './scene';

export interface Waypoint {
  position: LonLat;
  text: string;
}

export interface Overlays {
  paths: Path[];
  texts: TextItem[];
  waypoints: Waypoint[];
}

export interface OverlayInput {
  state: Pick<UiState, 'rings' | 'weaponRing' | 'tool' | 'planRoutes' | 'selected'>;
  /** Selected group's details, when it is drawn. */
  details: GroupDetails | null;
  /** Where the selected group is drawn this frame. */
  selectedAt: LonLat | null;
  cursor: LonLat | null;
  find: (id: number) => LonLat | null;
  findContact: (id: number) => LonLat | null;
}

export function buildOverlays(o: OverlayInput): Overlays {
  const out: Overlays = { paths: [], texts: [], waypoints: [] };
  if (o.details && o.selectedAt) selectionOverlays(o.details, o.selectedAt, o.state, out);
  if (o.state.tool) toolOverlays(o.state.tool, o, out);
  planOverlays(o.state.planRoutes, out);
  return out;
}

/** Rings (doc 10 §6–7), route or patrol circle of the selected group. */
function selectionOverlays(d: GroupDetails, pos: LonLat, state: OverlayInput['state'], { paths, texts, waypoints }: Overlays): void {
  const { rings, weaponRing } = state;
  const r = d.ranges;
  const picked = d.weapons.find((w) => w.id === weaponRing) ?? null;
  if (rings.sensor && r.sensor_m) {
    paths.push({
      path: geodesicCircle(pos, r.sensor_m),
      color: c('ring.sensor'),
      width: 2,
      dashed: true,
      tip:
        r.surface_horizon_m != null
          ? `Radar range: ${km(r.sensor_m)} · against aircraft; ships and ground units are hidden beyond ${km(r.surface_horizon_m)} (inner ring)`
          : `Radar / sensor horizon: ${km(r.sensor_m)} · how far ${d.name} can see a typical target`,
    });
    // Earth's curvature: low targets drop below the radar horizon long before the radar's range.
    if (r.surface_horizon_m != null)
      paths.push({
        path: geodesicCircle(pos, r.surface_horizon_m),
        color: c('ring.sensor', 0.6),
        width: 1,
        dashed: true,
        tip: `Radar horizon vs ships: ${km(r.surface_horizon_m)} · beyond it Earth's curvature hides ships and ground units from ${d.name}'s own radar`,
      });
  }
  if (rings.engage && picked) {
    // The weapon picked in the card's Weapons tab replaces the engagement ring.
    const own = !picked.engage_on_remote ? ' · fires only on our own track' : '';
    paths.push({
      path: geodesicCircle(pos, picked.max_range_m),
      color: c('ring.engage'),
      width: 2.5,
      dashed: false,
      tip: `${picked.name}: reaches ${km(picked.max_range_m)}${picked.min_range_m > 0 ? `, not closer than ${(picked.min_range_m / 1000).toFixed(1)} km` : ''}${own}`,
    });
    if (picked.min_range_m >= 1000)
      paths.push({
        path: geodesicCircle(pos, picked.min_range_m),
        color: c('ring.engage', 0.6),
        width: 1,
        dashed: true,
        tip: `${picked.name}: minimum range ${(picked.min_range_m / 1000).toFixed(1)} km`,
      });
    texts.push({
      position: destination(pos, 0, picked.max_range_m),
      text: `${picked.name} (${km(picked.max_range_m)})`,
      color: c('ring.engage'),
    });
  } else if (rings.engage && r.engage_m) {
    const longest = d.weapons.find((w) => w.max_range_m === r.engage_m);
    const ownTrack = longest && !longest.engage_on_remote;
    paths.push({
      path: geodesicCircle(pos, r.engage_m),
      color: c('ring.engage'),
      width: 2,
      dashed: true,
      tip: `Max engagement range: ${km(r.engage_m)}${longest ? ` · ${longest.name}` : ''}${
        ownTrack ? ' · fires only on our own track, so the target must also be inside our own sensor coverage' : ''
      }`,
    });
  }
  if (rings.radius && r.combat_radius_m && d.home_base) {
    const home: LonLat = [d.home_base.lon_deg, d.home_base.lat_deg];
    paths.push({
      path: geodesicCircle(home, r.combat_radius_m),
      color: c('ring.radius'),
      width: 1.5,
      dashed: true,
      tip: `Combat radius: ${km(r.combat_radius_m)} from ${d.home_base.name} · out and back on current fuel rules`,
    });
    texts.push({
      position: destination(home, 315, r.combat_radius_m * 0.55),
      text: `${d.home_base.name} Combat Radius (${Math.round(r.combat_radius_m / 1000)} km)`,
      color: c('ring.radius.text'),
    });
  }
  if (d.jammer && d.jammer.coverage_km > 0) {
    // Coverage is a circle around the jammer (doc 02 §9): every radar inside it is affected.
    // The sector is an angle measured at the victim radar between the jammer and the
    // protected unit, not a direction the aircraft points, so it is not drawn as a wedge.
    paths.push({
      path: geodesicCircle(pos, d.jammer.coverage_km * 1000),
      color: c('ring.sensor', 0.6),
      width: 1.5,
      dashed: true,
      tip: `Jammer coverage: ${km(d.jammer.coverage_km * 1000)} · enemy radars inside are affected`,
    });
  }

  const t = d.task;
  const wps = t.waypoints as LonLat[];
  if ((t.kind === 'patrol_circle' || t.kind === 'jam') && wps[0] && t.radius_m) {
    paths.push({
      path: geodesicCircle(wps[0], t.radius_m),
      color: t.kind === 'jam' ? c('ring.sensor', 0.8) : c('waypoint'),
      width: 1.5,
      dashed: true,
      tip: `${t.kind === 'jam' ? 'Jamming orbit' : 'Patrol circle'}: ${km(t.radius_m)} radius`,
    });
    return;
  }
  if (t.kind === 'suppress' && wps[0]) {
    paths.push({ path: [pos, wps[0]], color: c('weapon.offensive'), width: 2, dashed: false });
    waypoints.push({ position: wps[0], text: 'SEAD' });
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

/** Routes of the operation open in the planner (doc 03 §8): amber routes, dashed orbits, `P3 WP 1` labels. */
function planOverlays(routes: PlanRoute[], { paths, waypoints }: Overlays): void {
  for (const r of routes) {
    const a = r.active ? 1 : 0.55;
    const first = r.route[0];
    if (!first) continue;
    const target = r.kind === 'strike' || r.kind === 'suppress' || r.kind === 'fire';
    if (target) {
      waypoints.push({ position: first, text: `P${r.phase} TARGET` });
      if (r.radiusKm) paths.push({ path: geodesicCircle(first, r.radiusKm * 1000), color: c('weapon.offensive', a * 0.6), width: 1, dashed: true });
    } else if (r.radiusKm && r.route.length === 1) {
      paths.push({ path: geodesicCircle(first, r.radiusKm * 1000), color: c('waypoint', a), width: 1.5, dashed: true });
      waypoints.push({ position: first, text: `P${r.phase} ${r.kind.replace('_', ' ').toUpperCase()}` });
    } else {
      if (r.route.length > 1) {
        const loop = r.kind === 'patrol_route' || r.kind === 'tanker_track';
        paths.push({ path: loop ? [...r.route, first] : r.route, color: c('weapon.offensive', a), width: 2, dashed: false });
      }
      r.route.forEach((p, i) => waypoints.push({ position: p, text: `P${r.phase} WP ${i + 1}` }));
    }
  }
}

function toolOverlays(tool: Tool, o: OverlayInput, { paths, waypoints }: Overlays): void {
  const cursor = o.cursor;
  if (tool.kind === 'route') {
    const pts = cursor ? [...tool.points, cursor] : tool.points;
    if (pts.length > 1) paths.push({ path: pts, color: c('weapon.offensive'), width: 2, dashed: false });
    tool.points.forEach((p, i) => waypoints.push({ position: p, text: `WP ${i + 1}` }));
  } else if (tool.kind === 'circle' && tool.centre) {
    const radius = cursor ? distance(tool.centre, cursor) : tool.radiusKm * 1000;
    paths.push({ path: geodesicCircle(tool.centre, Math.max(radius, 1000)), color: c('waypoint'), width: 1.5, dashed: true });
  } else if (tool.kind === 'strike') {
    // Dashed amber aim line from the shooter to the target (or to the cursor while picking).
    const selected = o.state.selected;
    const from = selected == null ? null : o.find(selected);
    const aim = tool.aim;
    const to = aim ? ('contact' in aim ? o.findContact(aim.contact) : aim.point) : cursor;
    if (from && to) paths.push({ path: [from, to], color: c('weapon.offensive'), width: 2, dashed: true });
    if (aim && to) waypoints.push({ position: to, text: 'TARGET' });
  }
}

/** Last reported position of a unit, group (its first member) or base. */
export function findUnit(view: SideView | undefined, id: number): LonLat | null {
  if (!view) return null;
  let i = view.index.get(id);
  if (i === undefined) {
    for (let k = 0; k < view.units.count; k++) if (view.units.group[k] === id) i ??= k;
  }
  return i === undefined ? null : [view.units.lon[i] ?? 0, view.units.lat[i] ?? 0];
}

/** Last reported position of a contact. */
export function findContact(view: SideView | undefined, id: number): LonLat | null {
  const i = view?.contactIndex.get(id);
  return view && i !== undefined ? [view.contacts.lon[i] ?? 0, view.contacts.lat[i] ?? 0] : null;
}
