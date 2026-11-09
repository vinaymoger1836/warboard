// UI state (doc 06 §1 `store`): selection, ring toggles, drawing tool, alerts and the event
// log. Not the simulation. A tiny external store read with useSyncExternalStore, so map
// layers (outside React) and panels share it.
import type { ContactDetails, Details, Event, OperationSpec } from '@warboard/protocol';
import { useSyncExternalStore } from 'react';

export type LonLat = [number, number];

/** What a strike is aimed at: one of our contacts, or a point (open sea or ground). */
export type StrikeAim = { contact: number } | { point: LonLat };

/**
 * Drawing tools (doc 06 §4): a waypoint route or a patrol circle for the selection, or
 * picking a strike target (the card's strike panel fires it).
 */
export type Tool =
  | { kind: 'route'; patrol: boolean; points: LonLat[] }
  | { kind: 'circle'; centre: LonLat | null; radiusKm: number }
  | { kind: 'strike'; aim: StrikeAim | null };

/** A route of an operation's phase drawn on the map (doc 03 §8, doc 10 §6). */
export interface PlanRoute {
  /** 1-based phase number, for the labels. */
  phase: number;
  kind: string;
  route: LonLat[];
  radiusKm?: number;
  /** The phase is running (drawn at full strength) or the plan is being edited. */
  active: boolean;
}

export interface EventLine {
  key: number;
  event: Event;
}

export interface UiState {
  /** Selected group or base id. */
  selected: number | null;
  details: Details | null;
  /** Selected contact of our picture (a separate id space from units). */
  selectedContact: number | null;
  contact: ContactDetails | null;
  rings: { sensor: boolean; engage: boolean; radius: boolean };
  /** Weapon (content id) picked in the unit card's Weapons tab: its range ring replaces the engagement ring. */
  weaponRing: string | null;
  /** Sensor rings of every own unit, not just the selection (layers button). */
  allSensorRings: boolean;
  /** Estimated threat rings of identified SAM types (doc 03 §10). */
  threatRings: boolean;
  tool: Tool | null;
  /** What an engine AutoSlow event does (doc 02 §3). */
  autoSlow: 'x1' | 'pause' | 'off';
  events: EventLine[];
  /** Unread alerts (auto-slow, bingo) for the dock. */
  alerts: EventLine[];
  /** Contacts, classifications and reports for the intel log (doc 03 §10). */
  intel: EventLine[];
  /** Click-to-locate: the map centres on `locate.id` (a contact id if `contact`) when `seq` changes. */
  locate: { id: number; seq: number; contact: boolean } | null;
  /** Routes of the operation open in the planner. */
  planRoutes: PlanRoute[];
  /** The drawing tool is working for the planner: its result goes to `planTool`, not to an order. */
  capture: boolean;
  planTool: Tool | null;
  /** The operation planner's draft (doc 03 §8); `replace` is the planned operation it edits. */
  planner: { draft: OperationSpec; replace: number | null } | null;
  /** Reports panel open (doc 03 §12). */
  reportsOpen: boolean;
}

const MAX_EVENTS = 500;

let state: UiState = {
  selected: null,
  details: null,
  selectedContact: null,
  contact: null,
  rings: { sensor: true, engage: true, radius: true },
  weaponRing: null,
  allSensorRings: false,
  threatRings: true,
  tool: null,
  autoSlow: 'x1',
  events: [],
  alerts: [],
  intel: [],
  locate: null,
  planRoutes: [],
  capture: false,
  planTool: null,
  planner: null,
  reportsOpen: false,
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
    ui.set({
      selected: id,
      details: id === state.selected ? state.details : null,
      tool: null,
      weaponRing: id === state.selected ? state.weaponRing : null,
      selectedContact: id === null ? state.selectedContact : null,
      contact: id === null ? state.contact : null,
    });
  },
  selectContact(id: number | null): void {
    ui.set({
      selectedContact: id,
      contact: id === state.selectedContact ? state.contact : null,
      selected: id === null ? state.selected : null,
      details: id === null ? state.details : null,
      tool: null,
      weaponRing: id === null ? state.weaponRing : null,
    });
  },
  addEvents(events: Event[], isAlert: (e: Event) => boolean, isIntel: (e: Event) => boolean): void {
    const lines = events.map((event) => ({ key: nextKey++, event }));
    ui.set((s) => ({
      events: [...s.events, ...lines].slice(-MAX_EVENTS),
      alerts: [...s.alerts, ...lines.filter((l) => isAlert(l.event))].slice(-50),
      intel: [...s.intel, ...lines.filter((l) => isIntel(l.event))].slice(-MAX_EVENTS),
    }));
  },
  locate(id: number): void {
    ui.select(id);
    ui.set((s) => ({ locate: { id, seq: (s.locate?.seq ?? 0) + 1, contact: false } }));
  },
  locateContact(id: number): void {
    ui.selectContact(id);
    ui.set((s) => ({ locate: { id, seq: (s.locate?.seq ?? 0) + 1, contact: true } }));
  },
  clearLog(): void {
    ui.set({
      events: [],
      alerts: [],
      intel: [],
      selected: null,
      details: null,
      selectedContact: null,
      contact: null,
      tool: null,
    });
  },
};

export function useUi<T>(select: (s: UiState) => T): T {
  return useSyncExternalStore(ui.subscribe, () => select(state));
}
