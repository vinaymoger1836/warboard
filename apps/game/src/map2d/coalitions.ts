// Coalitions for the territory tint. Placeholder until war setup (P9) chooses them;
// set to the Baltic test set of reference screenshot 01.

export interface Coalition {
  /** Own coalition country codes (content ids, e.g. `pol`). */
  own: string[];
  enemy: string[];
}

export const DEMO_COALITION: Coalition = {
  own: ['pol', 'ltu', 'lva', 'est'],
  enemy: ['rus', 'blr'],
};
