// SideView v1 decoder (doc 06 §3; layout in crates/wb-sim/src/view.rs). Columns are
// typed-array views onto the transferred buffer, so decoding copies nothing but strings.
import { FLAG, STATUS, VIEW_HEADER_BYTES, VIEW_MAGIC, VIEW_VERSION } from './generated/symbols';

export interface UnitColumns {
  count: number;
  lon: Float64Array;
  lat: Float64Array;
  id: Uint32Array;
  flags: Uint32Array;
  group: Uint32Array;
  altM: Float32Array;
  headingDeg: Float32Array;
  speedMps: Float32Array;
  symbol: Uint16Array;
  /** 0–100, 255 when the unit has no fuel state. */
  fuelPct: Uint8Array;
}

export interface SideView {
  buffer: ArrayBuffer;
  side: number;
  tick: number;
  simTimeMs: number;
  units: UnitColumns;
  /** Index of each unit id in the columns. */
  index: Map<number, number>;
}

export type StatusName = (typeof STATUS)[number];

/** Names and type names per id, kept across views (the engine sends deltas only). */
export class StringTable {
  readonly names = new Map<number, string>();
  readonly types = new Map<number, string>();

  name(id: number): string {
    return this.names.get(id) ?? '';
  }

  type(id: number): string {
    return this.types.get(id) ?? '';
  }

  clear(): void {
    this.names.clear();
    this.types.clear();
  }
}

const pad8 = (n: number) => (n + 7) & ~7;
const decoder = new TextDecoder();

/** Decodes a view and applies its string delta to `strings`. */
export function decodeSideView(buffer: ArrayBuffer, strings: StringTable): SideView {
  const dv = new DataView(buffer);
  const magic = String.fromCharCode(dv.getUint8(0), dv.getUint8(1), dv.getUint8(2), dv.getUint8(3));
  if (magic !== VIEW_MAGIC) throw new Error('Not a side view');
  const version = dv.getUint16(4, true);
  if (version !== VIEW_VERSION) throw new Error(`Side view version ${version} is not supported`);
  const n = dv.getUint32(24, true);
  const nContacts = dv.getUint32(28, true);
  const nWeapons = dv.getUint32(32, true);
  const nEffects = dv.getUint32(36, true);
  const nStrings = dv.getUint32(40, true);
  if (nContacts + nWeapons + nEffects !== 0) throw new Error('Contacts, weapons and effects arrive in a later phase');

  let at = VIEW_HEADER_BYTES;
  const f64 = () => {
    const a = new Float64Array(buffer, at, n);
    at = pad8(at + 8 * n);
    return a;
  };
  const u32 = () => {
    const a = new Uint32Array(buffer, at, n);
    at = pad8(at + 4 * n);
    return a;
  };
  const f32 = () => {
    const a = new Float32Array(buffer, at, n);
    at = pad8(at + 4 * n);
    return a;
  };
  const units: UnitColumns = {
    count: n,
    lon: f64(),
    lat: f64(),
    id: u32(),
    flags: u32(),
    group: u32(),
    altM: f32(),
    headingDeg: f32(),
    speedMps: f32(),
    symbol: (() => {
      const a = new Uint16Array(buffer, at, n);
      at = pad8(at + 2 * n);
      return a;
    })(),
    fuelPct: (() => {
      const a = new Uint8Array(buffer, at, n);
      at = pad8(at + n);
      return a;
    })(),
  };

  for (let i = 0; i < nStrings; i++) {
    const id = dv.getUint32(at, true);
    const kind = dv.getUint8(at + 4);
    const len = dv.getUint16(at + 5, true);
    const text = decoder.decode(new Uint8Array(buffer, at + 7, len));
    at += 7 + len;
    (kind === 0 ? strings.names : strings.types).set(id, text);
  }

  const index = new Map<number, number>();
  for (let i = 0; i < n; i++) index.set(units.id[i] ?? 0, i);
  return {
    buffer,
    side: dv.getUint8(6),
    tick: Number(dv.getBigUint64(8, true)),
    simTimeMs: Number(dv.getBigInt64(16, true)),
    units,
    index,
  };
}

/** Status name from a unit's flags. */
export function statusOf(flags: number): StatusName {
  return STATUS[(flags & FLAG.STATUS_MASK) >> FLAG.STATUS_SHIFT] ?? '';
}
