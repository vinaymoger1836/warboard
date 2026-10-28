// Orders from the UI (doc 04 §7.2). The worker stamps the tick and side; a rejection's
// reason is shown as a toast (CLAUDE.md rule 5).
import type { Command, TaskOrder } from '@warboard/protocol';
import type { EngineBridge } from './bridge';

export async function sendCommand(bridge: EngineBridge, command: Command, toast: (t: string) => void): Promise<boolean> {
  try {
    const reply = await bridge.request({ type: 'order', order: { tick: 0, side: 0, ...command } });
    if (!reply.ok) toast(reply.reason);
    return reply.ok;
  } catch (err) {
    toast(err instanceof Error ? err.message : String(err));
    return false;
  }
}

export function sendTask(bridge: EngineBridge, units: number[], task: TaskOrder, toast: (t: string) => void) {
  return sendCommand(bridge, { type: 'AssignTask', units, task }, toast);
}

/** Default patrol circle radius for "Patrol here" by domain. */
export const patrolRadiusKm = (domain: string) => (domain === 'air' ? 30 : 10);
