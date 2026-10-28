// UI state (doc 06 §1 `store`): selection, ring toggles, drawing tool, alerts and the event
// log. Not the simulation. A tiny external store read with useSyncExternalStore, so map
// layers (outside React) and panels share it.
import type { Details, Event } from '@warboard/protocol';
import { useSyncExternalStore } from 'react';

export type LonLat = [number, number];

/** Drawing tools (doc 06 §4): a waypoint route or a patrol circle for the selection. */
export type Tool =
  | { kind: 'route'; patrol: boolean; points: LonLat[] }
  | { kind: 'circle'; centre: LonLat | null; radiusKm: number };

export interface EventLine {
  key: number;
  event: Event;
}

export interface UiState {
  /** Selected group or base id. */
  selected: number | null;
  details: Details | null;
  rings: { sensor: boolean; engage: boolean; radius: boolean };
  tool: Tool | null;
  /** What an engine AutoSlow event does (doc 02 §3). */
  autoSlow: 'x1' | 'pause' | 'off';
  events: EventLine[];
  /** Unread alerts (auto-slow, bingo) for the dock. */
  alerts: EventLine[];
  /** Click-to-locate: the map centres on `locate.id` when `seq` changes. */
  locate: { id: number; seq: number } | null;
}

const MAX_EVENTS = 500;

let state: UiState = {
  selected: null,
  details: null,
  rings: { sensor: true, engage: true, radius: true },
  tool: null,
  autoSlow: 'x1',
  events: [],
  alerts: [],
  locate: null,
};
const listeners = new Set<() => void>();
let nextKey = 1;

export const ui = {
  get: (): UiState => state,
  set(patch: Partial<UiState> | ((s: UiState) => Partial<UiState>)): void {
    state = { ...state, ...(typeof patch === 'function' ? patch(state) : patch) };
    for (const l of listeners) l();
  },
  subscribe(l: () => void): () => void {
    listeners.add(l);
    return () => listeners.delete(l);
  },
  select(id: number | null): void {
    ui.set({ selected: id, details: id === state.selected ? state.details : null, tool: null });
  },
  addEvents(events: Event[], isAlert: (e: Event) => boolean): void {
    const lines = events.map((event) => ({ key: nextKey++, event }));
    ui.set((s) => ({
      events: [...s.events, ...lines].slice(-MAX_EVENTS),
      alerts: [...s.alerts, ...lines.filter((l) => isAlert(l.event))].slice(-50),
    }));
  },
  locate(id: number): void {
    ui.set((s) => ({ selected: id, locate: { id, seq: (s.locate?.seq ?? 0) + 1 } }));
  },
  clearLog(): void {
    ui.set({ events: [], alerts: [], selected: null, details: null, tool: null });
  },
};

export function useUi<T>(select: (s: UiState) => T): T {
  return useSyncExternalStore(ui.subscribe, () => select(state));
}
