// Play modes on the client (doc 12 §2–§3): which side gives orders, which picture is on screen,
// the umpire view, and the hot-seat handover. The engine owns the rules (who may switch, who may
// see the umpire view); this module keeps the screen in step with them and swaps each side's UI
// slice (store.ts). Switching never touches the war: the worker only changes whose view it builds.
import type { HotSeat, PlayMode } from '@warboard/protocol';
import { useSyncExternalStore } from 'react';
import type { EngineBridge } from './bridge';
import type { WarInfo } from './engine/messages';
import { type Camera, ui } from './store';

/** The umpire position of the side toggle (the engine's view side 255). */
export const UMPIRE = 255;

export interface PlayState {
  war: WarInfo | null;
  mode: PlayMode | null;
  /** Sides the toggle offers: the human sides, or both in Watch. */
  sides: number[];
  /** The side whose orders the client gives. */
  active: number;
  /** The picture on screen: a side, or `UMPIRE`. */
  view: number;
  /** Command both: the player has the umpire view on (doc 12 §1). */
  umpireOn: boolean;
  /** Hot-seat: the device is being passed to this side; nothing else is on screen. */
  handover: number | null;
  hotSeat: HotSeat;
  hasPin: boolean[];
  /** Sim time of the last hand-over, for the turn timer. */
  turnStartMs: number;
  /** Some side is still deploying (deploy yourself): the clock is held by the engine. */
  deploying: boolean;
}

const DEFAULT_HOT_SEAT: HotSeat = { hand_over: 'OnRequest', urgent: 'interrupt' };

let state: PlayState = {
  war: null,
  mode: null,
  sides: [],
  active: 0,
  view: 0,
  umpireOn: false,
  handover: null,
  hotSeat: DEFAULT_HOT_SEAT,
  hasPin: [],
  turnStartMs: 0,
  deploying: false,
};
const listeners = new Set<() => void>();

function set(patch: Partial<PlayState>): void {
  state = { ...state, ...patch };
  for (const l of listeners) l();
}

/** The map registers here so a switch can keep and restore each side's camera. */
const camera: { get: () => Camera | null; set: (c: Camera) => void } = { get: () => null, set: () => undefined };
/** The app registers its speed control: a handover pauses and the next player resumes. */
const speed: { get: () => number; set: (s: number) => void } = { get: () => 0, set: () => undefined };
/** Camera of the side handing over, taken before the map goes away. */
let pendingCamera: Camera | null = null;
/** Speed to resume at after a handover. */
let speedBefore = 1;

export const play = {
  get: (): PlayState => state,
  subscribe(l: () => void): () => void {
    listeners.add(l);
    return () => listeners.delete(l);
  },
  registerCamera(get: () => Camera | null, setCam: (c: Camera) => void): () => void {
    camera.get = get;
    camera.set = setCam;
    return () => {
      camera.get = () => null;
      camera.set = () => undefined;
    };
  },
  registerSpeed(get: () => number, setSpeed: (s: number) => void): void {
    speed.get = get;
    speed.set = setSpeed;
  },

  /** A war started or loaded. A hot-seat war opens at the active side's handover screen. */
  startWar(war: WarInfo, simMs: number): void {
    const sides = war.sides.map((_, i) => i).filter((i) => war.mode === 'watch' || war.sides[i]?.controller === 'human');
    ui.resetSides(war.side);
    pendingCamera = null;
    const hotSeat = war.mode === 'hot_seat';
    if (hotSeat) {
      speedBefore = speed.get() || 1;
      speed.set(0);
    }
    set({
      war,
      mode: war.mode,
      sides,
      active: war.side,
      view: war.side,
      umpireOn: war.session.umpire_enabled,
      handover: hotSeat ? war.side : null,
      hotSeat: war.session.hot_seat,
      hasPin: war.session.has_pin,
      turnStartMs: simMs,
      deploying: war.deploying,
    });
  },

  setDeploying(deploying: boolean): void {
    if (deploying !== state.deploying) set({ deploying });
  },

  /** Moves orders and the picture to `side` now (Command both, Watch, after a handover). */
  async switchTo(bridge: EngineBridge, side: number): Promise<void> {
    const cam = camera.get() ?? pendingCamera;
    pendingCamera = null;
    await bridge.request({ type: 'setActiveSide', side });
    ui.switchSide(side, cam);
    set({ active: side, view: side });
    const next = ui.get().camera;
    if (next) camera.set(next);
  },

  /** The toggle or Tab asked for `side` (or the umpire position). */
  async choose(bridge: EngineBridge, side: number): Promise<void> {
    if (side === UMPIRE) {
      if (!state.umpireOn) return;
      await bridge.request({ type: 'setViewSide', side: UMPIRE });
      ui.set({ selected: null, details: null, selectedContact: null, contact: null, tool: null });
      set({ view: UMPIRE });
      return;
    }
    if (side === state.active && state.view !== UMPIRE) return;
    if (state.mode === 'hot_seat') {
      if (side !== state.active) play.handOver(side);
      return;
    }
    if (side === state.active) {
      await bridge.request({ type: 'setViewSide', side });
      set({ view: side });
      return;
    }
    await play.switchTo(bridge, side);
  },

  /** Tab: the next position of the toggle. */
  async cycle(bridge: EngineBridge): Promise<void> {
    const positions = [...state.sides, ...(state.umpireOn ? [UMPIRE] : [])];
    if (positions.length < 2) return;
    const at = positions.indexOf(state.view);
    const next = positions[(at + 1) % positions.length];
    if (next !== undefined) await play.choose(bridge, next);
  },

  async setUmpire(bridge: EngineBridge, on: boolean): Promise<void> {
    await bridge.request({ type: 'setUmpire', on });
    set({ umpireOn: on, view: on ? state.view : state.active });
    if (on) await play.choose(bridge, UMPIRE);
  },

  /** Hot-seat: pause and show the handover screen for `side` (doc 12 §3). Nothing says why. */
  handOver(side?: number): void {
    if (state.mode !== 'hot_seat' || state.handover !== null) return;
    const next = side ?? state.sides.find((s) => s !== state.active);
    if (next === undefined) return;
    pendingCamera = camera.get();
    speedBefore = speed.get() || speedBefore || 1;
    speed.set(0);
    set({ handover: next });
  },

  /** Continue on the handover screen; the reason if refused (wrong PIN). */
  async continueAs(bridge: EngineBridge, pin: string, simMs: number): Promise<string | null> {
    const side = state.handover;
    if (side === null) return null;
    if (state.hasPin[side] && !(await bridge.request({ type: 'checkPin', side, pin }))) {
      return 'That PIN is not right';
    }
    if (side !== state.active) await play.switchTo(bridge, side);
    set({ handover: null, turnStartMs: simMs });
    // The clock stays paused while a side is deploying; otherwise resume where it was.
    speed.set(speedBefore || 1);
    return null;
  },
};

export function usePlay<T>(select: (s: PlayState) => T): T {
  return useSyncExternalStore(play.subscribe, () => select(state));
}
