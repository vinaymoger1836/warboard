// SideView v2 decoder (doc 06 §3; layout in crates/wb-sim/src/view.rs). Columns are
// typed-array views onto the transferred buffer, so decoding copies nothing but strings.
import { CFLAG, FLAG, STATUS, STRING_KIND, VIEW_HEADER_BYTES, VIEW_MAGIC, VIEW_VERSION } from './generated/symbols';

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
  /** Longest sensor range, km (all-units ring layer). */
  sensorKm: Uint16Array;
  /** 0–100, 255 when the unit has no fuel state. */
  fuelPct: Uint8Array;
}

/** Contacts of the side's picture: estimates only (ADR-003, ADR-026). */
export interface ContactColumns {
  count: number;
  lon: Float64Array;
  lat: Float64Array;
  id: Uint32Array;
  /** `CFLAG` bits; the domain in bits 8–10. */
  flags: Uint32Array;
  altM: Float32Array;
  headingDeg: Float32Array;
  speedMps: Float32Array;
  /** Uncertainty ellipse semi-axes (1 sigma), metres; for a bearing-only contact `a` is the strobe length. */
  ellipseA: Float32Array;
  ellipseB: Float32Array;
  /** Bearing of the major axis, degrees. */
  ellipseRotDeg: Float32Array;
  ageS: Float32Array;
  /** Estimated threat ring of an identified SAM type, metres (0 = none). */
  threatM: Float32Array;
  symbol: Uint16Array;
  countMin: Uint16Array;
  countMax: Uint16Array;
  /** 0 unknown, 1 hostile, 2 neutral, 3 friendly. */
  identity: Uint8Array;
  /** 0 unknown, 1 domain, 2 class, 3 type, 4 precise. */
  level: Uint8Array;
}

export interface SideView {
  buffer: ArrayBuffer;
  side: number;
  tick: number;
  simTimeMs: number;
  units: UnitColumns;
  contacts: ContactColumns;
  /** Index of each unit id in the unit columns. */
  index: Map<number, number>;
  /** Index of each contact id in the contact columns. */
  contactIndex: Map<number, number>;
}

export type StatusName = (typeof STATUS)[number];

/** Strings per id, kept across views (the engine sends deltas only). Unit ids and contact
 *  ids are separate namespaces. */
export class StringTable {
  readonly names = new Map<number, string>();
  readonly types = new Map<number, string>();
  readonly contactTypes = new Map<number, string>();
  readonly owners = new Map<number, string>();
  readonly payloads = new Map<number, string>();

  name(id: number): string {
    return this.names.get(id) ?? '';
  }

  type(id: number): string {
    return this.types.get(id) ?? '';
  }

  contactType(id: number): string {
    return this.contactTypes.get(id) ?? '';
  }

  clear(): void {
    this.names.clear();
    this.types.clear();
    this.contactTypes.clear();
    this.owners.clear();
    this.payloads.clear();
  }

  set(kind: number, id: number, text: string): void {
    const table =
      kind === STRING_KIND.NAME
        ? this.names
        : kind === STRING_KIND.TYPE
          ? this.types
          : kind === STRING_KIND.CONTACT_TYPE
            ? this.contactTypes
            : kind === STRING_KIND.OWNER
              ? this.owners
              : kind === STRING_KIND.PAYLOAD
                ? this.payloads
                : null;
    table?.set(id, text);
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
  const nc = dv.getUint32(28, true);
  const nWeapons = dv.getUint32(32, true);
  const nEffects = dv.getUint32(36, true);
  const nStrings = dv.getUint32(40, true);
  if (nWeapons + nEffects !== 0) throw new Error('Weapons and effects arrive in a later phase');

  let at = VIEW_HEADER_BYTES;
  const col = <T>(make: (b: ArrayBuffer, off: number, len: number) => T, size: number, len: number): T => {
    const a = make(buffer, at, len);
    at = pad8(at + size * len);
    return a;
  };
  const f64 = (len: number) => col((b, o, l) => new Float64Array(b, o, l), 8, len);
  const f32 = (len: number) => col((b, o, l) => new Float32Array(b, o, l), 4, len);
  const u32 = (len: number) => col((b, o, l) => new Uint32Array(b, o, l), 4, len);
  const u16 = (len: number) => col((b, o, l) => new Uint16Array(b, o, l), 2, len);
  const u8 = (len: number) => col((b, o, l) => new Uint8Array(b, o, l), 1, len);

  const units: UnitColumns = {
    count: n,
    lon: f64(n),
    lat: f64(n),
    id: u32(n),
    flags: u32(n),
    group: u32(n),
    altM: f32(n),
    headingDeg: f32(n),
    speedMps: f32(n),
    symbol: u16(n),
    sensorKm: u16(n),
    fuelPct: u8(n),
  };
  const contacts: ContactColumns = {
    count: nc,
    lon: f64(nc),
    lat: f64(nc),
    id: u32(nc),
    flags: u32(nc),
    altM: f32(nc),
    headingDeg: f32(nc),
    speedMps: f32(nc),
    ellipseA: f32(nc),
    ellipseB: f32(nc),
    ellipseRotDeg: f32(nc),
    ageS: f32(nc),
    threatM: f32(nc),
    symbol: u16(nc),
    countMin: u16(nc),
    countMax: u16(nc),
    identity: u8(nc),
    level: u8(nc),
  };

  for (let i = 0; i < nStrings; i++) {
    const id = dv.getUint32(at, true);
    const kind = dv.getUint8(at + 4);
    const len = dv.getUint16(at + 5, true);
    const text = decoder.decode(new Uint8Array(buffer, at + 7, len));
    at += 7 + len;
    strings.set(kind, id, text);
  }

  const index = new Map<number, number>();
  for (let i = 0; i < n; i++) index.set(units.id[i] ?? 0, i);
  const contactIndex = new Map<number, number>();
  for (let i = 0; i < nc; i++) contactIndex.set(contacts.id[i] ?? 0, i);
  return {
    buffer,
    side: dv.getUint8(6),
    tick: Number(dv.getBigUint64(8, true)),
    simTimeMs: Number(dv.getBigInt64(16, true)),
    units,
    contacts,
    index,
    contactIndex,
  };
}

/** Status name from a unit's flags. */
export function statusOf(flags: number): StatusName {
  return STATUS[(flags & FLAG.STATUS_MASK) >> FLAG.STATUS_SHIFT] ?? '';
}

export type DomainName = 'air' | 'surface' | 'subsurface' | 'ground' | 'space';
const DOMAINS: DomainName[] = ['air', 'surface', 'subsurface', 'ground', 'space'];

/** Domain from a contact's flags. */
export function contactDomain(flags: number): DomainName {
  return DOMAINS[(flags & CFLAG.DOMAIN_MASK) >> CFLAG.DOMAIN_SHIFT] ?? 'air';
}
