// Coalitions for the territory tint, from the war's sides (the scenario in P3, war setup
// from P9): own = the player's side, enemy = every other side.
import type { WarInfo } from '../engine/messages';

export interface Coalition {
  /** Own coalition country codes (content ids, e.g. `pol`). */
  own: string[];
  enemy: string[];
}

export const NO_COALITION: Coalition = { own: [], enemy: [] };

export function coalitionOf(war: WarInfo | null): Coalition {
  if (!war) return NO_COALITION;
  return {
    own: war.sides[war.side]?.countries ?? [],
    enemy: war.sides.filter((_, i) => i !== war.side).flatMap((s) => s.countries),
  };
}
