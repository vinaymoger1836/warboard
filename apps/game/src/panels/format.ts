// Text helpers shared by the panels: clock strings, event lines, state colours (doc 10 §7–8).
import type { Event, StringTable } from '@warboard/protocol';
import { cssVar } from '@warboard/symbols';

const pad = (n: number) => String(n).padStart(2, '0');

/** `03:40:12Z` from Unix epoch ms. */
export function utcClock(ms: number): string {
  const d = new Date(ms);
  return `${pad(d.getUTCHours())}:${pad(d.getUTCMinutes())}:${pad(d.getUTCSeconds())}Z`;
}

/** `T+0:00:12` from elapsed ms. */
export function elapsed(ms: number): string {
  const s = Math.floor(ms / 1000);
  return `T+${Math.floor(s / 3600)}:${pad(Math.floor(s / 60) % 60)}:${pad(s % 60)}`;
}

/** `12:30` (minutes:seconds) or `1:02:30` for a duration in seconds. */
export function duration(s: number): string {
  const t = Math.max(0, Math.round(s));
  const h = Math.floor(t / 3600);
  const m = Math.floor(t / 60) % 60;
  return h > 0 ? `${h}:${pad(m)}:${pad(t % 60)}` : `${m}:${pad(t % 60)}`;
}

/** Status colour (doc 10 §7). */
export function statusColour(status: string): string {
  switch (status) {
    case 'ON STATION':
      return cssVar('state.station');
    case 'RTB':
    case 'REFUEL':
      return cssVar('state.warn');
    case 'LANDED':
    case 'TURNAROUND':
    case 'IDLE':
      return cssVar('text.secondary');
    default:
      return cssVar('text.primary');
  }
}

/** Fuel colour: ≥ 50 % ok, 25–50 % warn, < 25 % or bingo bad (doc 10 §7). */
export function fuelColour(pct: number | null | undefined, bingo = false): string {
  if (pct == null) return cssVar('text.secondary');
  if (bingo || pct < 25) return cssVar('state.bad');
  if (pct < 50) return cssVar('state.warn');
  return cssVar('state.ok');
}

export type EventCategory = 'Task' | 'Fuel' | 'Air' | 'Base' | 'Alert';

export const CATEGORY_COLOUR: Record<EventCategory, string> = {
  Task: cssVar('text.secondary'),
  Fuel: cssVar('state.warn'),
  Air: cssVar('side.own'),
  Base: cssVar('ring.sensor'),
  Alert: cssVar('state.bad'),
};

/** Category chip and plain-English text of an event. */
export function describe(e: Event, s: StringTable): { category: EventCategory; text: string } {
  const n = (id: number) => s.name(id) || `unit ${id}`;
  const k = e.kind;
  if ('TaskChanged' in k) return { category: 'Task', text: `${n(k.TaskChanged.group)}: new task, ${k.TaskChanged.task.replace('_', ' ')}` };
  if ('StationChanged' in k)
    return { category: 'Task', text: `${n(k.StationChanged.group)} ${k.StationChanged.on ? 'on station' : 'left station'}` };
  if ('BingoRtb' in k) {
    const b = k.BingoRtb;
    return { category: 'Fuel', text: `${n(b.unit)} bingo at ${Math.round(b.fuel_kg)} kg, returning to ${n(b.base)}` };
  }
  if ('Return' in k) return { category: 'Task', text: `${n(k.Return.group)} returning to ${n(k.Return.base)} (${k.Return.why})` };
  if ('ToTanker' in k) return { category: 'Fuel', text: `${n(k.ToTanker.group)} heading for tanker ${n(k.ToTanker.tanker)}` };
  if ('Refuelled' in k) return { category: 'Fuel', text: `${n(k.Refuelled.unit)} refuelled from ${n(k.Refuelled.tanker)}` };
  if ('TookOff' in k) return { category: 'Air', text: `${n(k.TookOff.unit)} left ${n(k.TookOff.base)}` };
  if ('Landed' in k)
    return { category: 'Air', text: `${n(k.Landed.unit)} arrived at ${n(k.Landed.base)} with ${Math.round(k.Landed.fuel_kg)} kg fuel` };
  if ('TurnaroundDone' in k) return { category: 'Base', text: `${n(k.TurnaroundDone.unit)} ready at ${n(k.TurnaroundDone.base)}` };
  if ('BaseActivated' in k) return { category: 'Base', text: `${n(k.BaseActivated.base)} is active` };
  return { category: 'Alert', text: `Auto-slow: ${k.AutoSlow.reason}` };
}

/** Events that also go to the alert feed. */
export const isAlert = (e: Event) => 'AutoSlow' in e.kind || 'BingoRtb' in e.kind;

/** The unit an event is about, for click-to-locate. */
export function subjectOf(e: Event): number | null {
  const k = e.kind;
  if ('TaskChanged' in k) return k.TaskChanged.group;
  if ('StationChanged' in k) return k.StationChanged.group;
  if ('BingoRtb' in k) return k.BingoRtb.unit;
  if ('Return' in k) return k.Return.group;
  if ('ToTanker' in k) return k.ToTanker.group;
  if ('Refuelled' in k) return k.Refuelled.unit;
  if ('TookOff' in k) return k.TookOff.unit;
  if ('Landed' in k) return k.Landed.unit;
  if ('TurnaroundDone' in k) return k.TurnaroundDone.unit;
  if ('BaseActivated' in k) return k.BaseActivated.base;
  return null;
}
