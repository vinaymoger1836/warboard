// Right-click actions on the map (doc 03 §5, doc 06 §4): Move here, Patrol here, routes
// and circles for the selected group; Land or Transfer at a base; Select a unit.
import type { EngineBridge } from '../bridge';
import type { ContextRequest } from '../map2d/units/UnitLayers';
import { patrolRadiusKm, sendTask } from '../orders';
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
  if (target && target.id !== group?.id) items.push([target.base ? 'Select base' : 'Select', () => ui.select(target.id)]);
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
