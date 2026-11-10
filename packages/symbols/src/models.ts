// Low-poly 3D models per symbol class (doc 06 §5, ADR-047): which model file a symbol uses in
// the 3D view, and the length it is drawn at. Art data only: the engine never reads it, and it
// carries no stats. Models are generic class shapes (no real design), built by
// `models/build-models.ts` during `pnpm gen` into `apps/game/public/models/<name>.glb`; a
// hand-made Blender export with the same file name replaces one without code changes.
// Every model has its nose along +X, up along +Y and is 1 m long; the 3D view scales it.

export type ModelName =
  | 'fighter'
  | 'attack'
  | 'bomber'
  | 'transport'
  | 'aew'
  | 'drone'
  | 'helicopter-attack'
  | 'helicopter-utility'
  | 'ship-large'
  | 'ship-medium'
  | 'ship-small'
  | 'auxiliary'
  | 'submarine'
  | 'radar-vehicle'
  | 'launcher-vehicle'
  | 'c2-vehicle'
  | 'tank'
  | 'artillery'
  | 'infantry-vehicle'
  | 'satellite'
  | 'missile'
  | 'bomb'
  | 'torpedo';

export const MODEL_NAMES: readonly ModelName[] = [
  'fighter',
  'attack',
  'bomber',
  'transport',
  'aew',
  'drone',
  'helicopter-attack',
  'helicopter-utility',
  'ship-large',
  'ship-medium',
  'ship-small',
  'auxiliary',
  'submarine',
  'radar-vehicle',
  'launcher-vehicle',
  'c2-vehicle',
  'tank',
  'artillery',
  'infantry-vehicle',
  'satellite',
  'missile',
  'bomb',
  'torpedo',
];

export interface ModelRef {
  name: ModelName;
  /** Drawn length, m (a class size, not any one system's). */
  lengthM: number;
}

/** Model per symbol id (`SYMBOLS` in @warboard/protocol). Bases have none: they stay icons. */
const BY_SYMBOL: Record<number, ModelRef> = {
  1: { name: 'fighter', lengthM: 18 },
  2: { name: 'bomber', lengthM: 45 },
  3: { name: 'attack', lengthM: 16 },
  4: { name: 'transport', lengthM: 45 },
  5: { name: 'aew', lengthM: 36 },
  6: { name: 'transport', lengthM: 32 },
  7: { name: 'transport', lengthM: 40 },
  8: { name: 'transport', lengthM: 36 },
  9: { name: 'drone', lengthM: 12 },
  10: { name: 'helicopter-attack', lengthM: 16 },
  11: { name: 'helicopter-utility', lengthM: 18 },
  20: { name: 'ship-large', lengthM: 155 },
  21: { name: 'ship-medium', lengthM: 130 },
  22: { name: 'ship-small', lengthM: 90 },
  23: { name: 'ship-small', lengthM: 55 },
  24: { name: 'auxiliary', lengthM: 170 },
  25: { name: 'submarine', lengthM: 90 },
  40: { name: 'radar-vehicle', lengthM: 11 },
  41: { name: 'launcher-vehicle', lengthM: 12 },
  42: { name: 'c2-vehicle', lengthM: 10 },
  43: { name: 'tank', lengthM: 9.5 },
  44: { name: 'artillery', lengthM: 11 },
  45: { name: 'infantry-vehicle', lengthM: 7.5 },
  60: { name: 'satellite', lengthM: 9 },
  61: { name: 'satellite', lengthM: 12 },
  62: { name: 'satellite', lengthM: 10 },
  120: { name: 'missile', lengthM: 6 },
  121: { name: 'bomb', lengthM: 3.5 },
  122: { name: 'torpedo', lengthM: 6 },
};

/** The model a symbol is drawn with, or null (bases, unknown). Our own weapons in flight use
 *  the missile model (their view symbol may be any weapon class). */
export function modelFor(symbol: number): ModelRef | null {
  return BY_SYMBOL[symbol] ?? null;
}

/** File of a model, relative to the app root. */
export const modelUrl = (name: ModelName) => `models/${name}.glb`;
