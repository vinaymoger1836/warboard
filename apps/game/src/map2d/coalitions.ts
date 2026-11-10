// Coalitions for the territory tint, from the war's sides: own = the side on screen (the
// active side, or side A in the umpire view; doc 12 §2), enemy = every other side.
import type { WarInfo } from '../engine/messages';

export interface Coalition {
  /** Own coalition nation codes (content ids, e.g. `ald`). */
  own: string[];
  enemy: string[];
}

export const NO_COALITION: Coalition = { own: [], enemy: [] };

export function coalitionOf(war: WarInfo | null, side: number): Coalition {
  if (!war) return NO_COALITION;
  return {
    own: war.sides[side]?.nations ?? [],
    enemy: war.sides.filter((_, i) => i !== side).flatMap((s) => s.nations),
  };
}
