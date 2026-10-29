// Right-click actions on the map (doc 03 §5, doc 06 §4): Move here, Patrol here, routes
// and circles for the selected group; Land or Transfer at a base; Select a unit or
// contact; task an own imaging satellite on the point (next pass, doc 02 §7.3).
import { SYMBOLS } from '@warboard/protocol';
import type { EngineBridge } from '../bridge';
import type { ContextRequest } from '../map2d/units/UnitLayers';
import { patrolRadiusKm, sendTask } from '../orders';

/** Radius of the area a satellite images on "Task … here", km. */
const IMAGING_RADIUS_KM = 20;

/** Own optical and SAR satellites in the latest view. */
function imagingSatellites(bridge: EngineBridge): { id: number; name: string }[] {
  const u = bridge.frame?.view.units;
  if (!u) return [];
  const out: { id: number; name: string }[] = [];
  for (let i = 0; i < u.count; i++) {
    const s = u.symbol[i] ?? 0;
    if (s === SYMBOLS.satellite_eo || s === SYMBOLS.satellite_sar) {
      const id = u.id[i] ?? 0;
      out.push({ id, name: bridge.strings.name(id) });
    }
  }
  return out;
}
import { ui, useUi } from '../store';

interface Props {
  req: ContextRequest;
  bridge: EngineBridge;
  toast: (t: string) => void;
  onClose: () => void;
}

export function ContextMenu({ req, bridge, toast, onClose }: Props) {
  const details = useUi((s) => s.details);
  const group = details?.kind === 'group' ? details : null;
  const items: [string, () => void][] = [];
  const target = req.target;
  if (target?.contact) items.push(['Select contact', () => ui.selectContact(target.id)]);
  else if (target && target.id !== group?.id) items.push([target.base ? 'Select base' : 'Select', () => ui.select(target.id)]);
  if (group) {
    const units = [group.id];
    const send = (task: Parameters<typeof sendTask>[2]) => void sendTask(bridge, units, task, toast);
    if (target?.base) {
      items.push([group.domain === 'air' ? 'Land here' : 'Berth here', () => send({ kind: 'land', base: target.id })]);
      items.push(['Transfer here', () => send({ kind: 'transfer', base: target.id })]);
    } else {
      items.push(['Move here', () => send({ kind: 'move', waypoints: [req.at] })]);
      items.push([
        'Patrol here',
        () => send({ kind: 'patrol_circle', centre: req.at, radius_km: patrolRadiusKm(group.domain) }),
      ]);
      items.push(['Draw route', () => ui.set({ tool: { kind: 'route', patrol: false, points: [] } })]);
      items.push(['Draw patrol route', () => ui.set({ tool: { kind: 'route', patrol: true, points: [] } })]);
      items.push([
        'Draw patrol circle',
        () => ui.set({ tool: { kind: 'circle', centre: null, radiusKm: patrolRadiusKm(group.domain) } }),
      ]);
      items.push(['Return to base', () => send({ kind: 'rtb' })]);
    }
  }
  if (!target) {
    for (const sat of imagingSatellites(bridge)) {
      items.push([
        `Task ${sat.name} imaging here`,
        () =>
          void bridge
            .request({
              type: 'order',
              order: { tick: 0, side: 0, type: 'TaskSatellite', satellite: sat.id, centre: req.at, radius_km: IMAGING_RADIUS_KM },
            })
            .then((r) => toast(r.ok ? `${sat.name}: ${r.ack.note ?? 'tasked'}` : r.reason))
            .catch((err: unknown) => toast(String(err))),
      ]);
    }
  }
  if (items.length === 0) items.push(['Select a unit first', () => undefined]);
  return (
    <ul className="context-menu" role="menu" style={{ left: req.x, top: req.y }}>
      {items.map(([label, run]) => (
        <li key={label} role="none">
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              onClose();
              run();
            }}
          >
            {label}
          </button>
        </li>
      ))}
    </ul>
  );
}
