// UI state (doc 06 §1 `store`): selection, ring toggles, drawing tool, alerts and the event
// log. Not the simulation. A tiny external store read with useSyncExternalStore, so map
// layers (outside React) and panels share it.
//
// Per side (doc 12 §2, P9): each human side keeps its own camera, selection, open panels,
// alerts, event and intel logs and operation planner. The fields in `SIDE_KEYS` are swapped
// out on a side switch and kept for the side not on screen; a waiting side's events go
// straight into its kept slice, so nothing of it reaches the screen until it is active.
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

/** Map camera of a side (restored when the side comes back). */
export interface Camera {
  center: LonLat;
  zoom: number;
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
  /** The planner task (`[phase, task]`) a map tool is drawing for. */
  planDrawing: [number, number] | null;
  /** Reports panel open (doc 03 §12). */
  reportsOpen: boolean;
  /** The side whose slice is on screen. */
  side: number;
  /** Where that side last left the map; null until it has looked. */
  camera: Camera | null;
}

/** Fields kept per side (doc 12 §2). */
const SIDE_KEYS = [
  'selected',
  'details',
  'selectedContact',
  'contact',
  'weaponRing',
  'tool',
  'events',
  'alerts',
  'intel',
  'locate',
  'planRoutes',
  'capture',
  'planTool',
  'planner',
  'planDrawing',
  'reportsOpen',
  'camera',
] as const;
type SideKey = (typeof SIDE_KEYS)[number];
export type SideSlice = Pick<UiState, SideKey>;

const freshSlice = (): SideSlice => ({
  selected: null,
  details: null,
  selectedContact: null,
  contact: null,
  weaponRing: null,
  tool: null,
  events: [],
  alerts: [],
  intel: [],
  locate: null,
  planRoutes: [],
  capture: false,
  planTool: null,
  planner: null,
  planDrawing: null,
  reportsOpen: false,
  camera: null,
});

const MAX_EVENTS = 500;

let state: UiState = {
  ...freshSlice(),
  rings: { sensor: true, engage: true, radius: true },
  allSensorRings: false,
  threatRings: true,
  autoSlow: 'x1',
  side: 0,
};
const listeners = new Set<() => void>();
let nextKey = 1;
/** Slices of the sides not on screen. */
const kept = new Map<number, SideSlice>();

function sliceOf(s: UiState): SideSlice {
  const out = {} as Record<SideKey, unknown>;
  for (const k of SIDE_KEYS) out[k] = s[k];
  return out as SideSlice;
}

function withLines(slice: SideSlice, events: Event[], isAlert: (e: Event) => boolean, isIntel: (e: Event) => boolean): SideSlice {
  const lines = events.map((event) => ({ key: nextKey++, event }));
  return {
    ...slice,
    events: [...slice.events, ...lines].slice(-MAX_EVENTS),
    alerts: [...slice.alerts, ...lines.filter((l) => isAlert(l.event))].slice(-50),
    intel: [...slice.intel, ...lines.filter((l) => isIntel(l.event))].slice(-MAX_EVENTS),
  };
}

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
    ui.set((s) => {
      const next = withLines(sliceOf(s), events, isAlert, isIntel);
      return { events: next.events, alerts: next.alerts, intel: next.intel };
    });
  },
  /** Events of one side: on screen if it is the side shown, else into its kept slice. */
  addEventsFor(side: number, events: Event[], isAlert: (e: Event) => boolean, isIntel: (e: Event) => boolean): void {
    if (side === state.side) {
      ui.addEvents(events, isAlert, isIntel);
      return;
    }
    kept.set(side, withLines(kept.get(side) ?? freshSlice(), events, isAlert, isIntel));
  },
  /** Puts the shown side's slice away and brings `side`'s back (doc 12 §2). */
  switchSide(side: number, camera: Camera | null): void {
    if (side === state.side) return;
    kept.set(state.side, { ...sliceOf(state), camera });
    const next = kept.get(side) ?? freshSlice();
    kept.delete(side);
    ui.set({ ...next, side });
  },
  /** A new war: every side starts clean, `side` on screen. */
  resetSides(side: number): void {
    kept.clear();
    ui.set({ ...freshSlice(), side });
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
  /** Unread alerts of a side not on screen (the side toggle shows a count). */
  keptAlerts(side: number): number {
    return kept.get(side)?.alerts.length ?? 0;
  },
};

export function useUi<T>(select: (s: UiState) => T): T {
  return useSyncExternalStore(ui.subscribe, () => select(state));
}
