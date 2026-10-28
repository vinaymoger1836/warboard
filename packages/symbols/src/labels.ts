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
