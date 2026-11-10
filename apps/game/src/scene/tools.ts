// What happens when a drawing tool finishes (doc 06 §4), in either view: a route or patrol
// circle becomes an order for the selected group, or goes to the operation planner when the
// tool was drawing for it. Shared by the 2D map and the 3D view.
import type { EngineBridge } from '../bridge';
import { patrolRadiusKm, sendTask } from '../orders';
import { type Tool, ui } from '../store';

export function makeToolDone(bridge: EngineBridge, onToast: (text: string) => void): (tool: Tool) => void {
  return (tool: Tool) => {
    const s = ui.get();
    ui.set({ tool: null });
    // Drawing for the operation planner: hand the route over, order nothing.
    if (s.capture) {
      ui.set({ capture: false, planTool: tool });
      return;
    }
    const d = s.details;
    if (d?.kind !== 'group') return;
    if (tool.kind === 'route') {
      if (tool.patrol && tool.points.length < 2) {
        onToast('A patrol route needs at least two waypoints');
        return;
      }
      void sendTask(
        bridge,
        [d.id],
        tool.patrol ? { kind: 'patrol_route', waypoints: tool.points, loop: true } : { kind: 'move', waypoints: tool.points },
        onToast,
      );
    } else if (tool.kind === 'circle' && tool.centre) {
      void sendTask(
        bridge,
        [d.id],
        { kind: 'patrol_circle', centre: tool.centre, radius_km: Math.round(tool.radiusKm) || patrolRadiusKm(d.domain) },
        onToast,
      );
    }
  };
}
