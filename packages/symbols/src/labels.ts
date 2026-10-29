// Map label grammar (doc 10 §4.1).
import { isAircraft, isBase, isShip } from './icons';

export interface LabelInput {
  symbol: number;
  /** Platform type name, e.g. `F-35A Lightning II`. */
  typeName: string;
  /** Unit, group or base name. */
  name: string;
  /** Aircraft in the group as drawn (1 for a single airframe). */
  count: number;
}

/** `3 × Rafale`, `SQN MQ-9A Reaper`, `SHIP FREMM class`, `SITE Łask Air Base`. */
export function unitLabel(u: LabelInput): string {
  if (isBase(u.symbol)) return `SITE ${u.name}`;
  if (isShip(u.symbol)) return `SHIP ${u.typeName}`;
  if (isAircraft(u.symbol)) return u.count > 1 ? `${u.count} × ${u.typeName}` : `SQN ${u.typeName}`;
  return u.name;
}

/** The small mark above an icon: `SQN` on aircraft tiles. */
export function markAbove(symbol: number): string {
  return isAircraft(symbol) ? 'SQN' : '';
}

export interface ContactLabelInput {
  symbol: number;
  /** Type short name at type level, class name at class level, empty below. */
  typeName: string;
  domain: string;
  countMin: number;
  countMax: number;
  countKnown: boolean;
}

/** First line of a contact label (doc 10 §4.1): `SITE S-400`, `SHIP Steregushchiy`,
 *  `2–4 × Su-35S`, `UNKNOWN AIR` while only the domain is known. The second line
 *  `(our estimate)` is added by the map for estimated contacts. */
export function contactLabel(c: ContactLabelInput): string {
  if (!c.typeName) return `UNKNOWN ${c.domain.toUpperCase()}`;
  if (isBase(c.symbol)) return `SITE ${c.typeName}`;
  if (isShip(c.symbol)) return `SHIP ${c.typeName}`;
  if (isAircraft(c.symbol) || c.domain === 'air') {
    const n = c.countKnown || c.countMin === c.countMax ? `${c.countMax}` : `${c.countMin}–${c.countMax}`;
    return n === '1' ? `SQN ${c.typeName}` : `${n} × ${c.typeName}`;
  }
  return c.typeName;
}

/** Second line under an estimated contact. */
export const ESTIMATE_LINE = '(our estimate)';
