// Launch, intercept and hit effects as drawable data (doc 10 §5, reference 03), shared by the
// 2D map and the 3D view. Stateless from the side view: the engine sends each effect with its
// age in sim ms, and the animation is a pure function of that age, so a redraw, a pause or a
// reload cannot replay or lose an effect. Age is turned into real time by the achieved sim
// speed (at ×10 an effect lasts a tenth of its time in sim ms), so flashes stay visible at any
// speed.
import { EFFECT, type SideView } from '@warboard/protocol';
import type { LonLat } from './geo';
import { c, type Rgba } from './scene';

/** How long each effect is drawn, real ms. */
const LIFE_MS: Record<number, number> = {
  [EFFECT.LAUNCH]: 650,
  [EFFECT.INTERCEPT]: 900,
  [EFFECT.HIT]: 1300,
};

export interface Disc {
  position: LonLat;
  alt: number;
  /** Radius, px. */
  radius: number;
  color: Rgba;
}

export interface Ring extends Disc {
  width: number;
}

export interface Burst {
  position: LonLat;
  alt: number;
  /** Size, px. */
  size: number;
  angle: number;
  color: Rgba;
}

export interface Effects {
  bursts: Burst[];
  rings: Ring[];
  discs: Disc[];
}

const easeOut = (t: number) => 1 - (1 - t) ** 3;

/** Spoke rotation from the position, so bursts differ but a redraw never changes one. */
const spin = (p: LonLat) => Math.abs(p[0] * 7919 + p[1] * 104729) % 45;

/** `dtS` is the sim time since the view arrived and `simPerReal` the achieved speed. */
export function buildEffects(view: SideView, dtS: number, simPerReal: number): Effects {
  const ev = view.effects;
  const out: Effects = { bursts: [], rings: [], discs: [] };
  if (ev.count === 0) return out;
  const speed = Math.max(1, simPerReal);
  const { bursts, rings, discs } = out;
  for (let k = 0; k < ev.count; k++) {
    const kind = ev.kind[k] ?? 0;
    const life = LIFE_MS[kind];
    if (life === undefined) continue;
    const realMs = ((ev.ageMs[k] ?? 0) + dtS * 1000) / speed;
    if (realMs >= life) continue;
    const t = realMs / life;
    const e = easeOut(t);
    const fade = 1 - t;
    const position: LonLat = [ev.lon[k] ?? 0, ev.lat[k] ?? 0];
    const alt = ev.altM[k] ?? 0;
    if (kind === EFFECT.LAUNCH) {
      discs.push({ position, alt, radius: 3 + 8 * e, color: c('glyph', 0.95 * fade * fade) });
      rings.push({ position, alt, radius: 5 + 14 * e, width: 1.5, color: c('glyph', 0.7 * fade) });
    } else if (kind === EFFECT.INTERCEPT) {
      bursts.push({ position, alt, size: 14 + 36 * e, angle: spin(position), color: c('weapon.interceptor', fade ** 1.3) });
      rings.push({ position, alt, radius: 6 + 20 * e, width: 1.5, color: c('weapon.interceptor', 0.9 * fade) });
      if (t < 0.4) discs.push({ position, alt, radius: 4 * (1 - t / 0.4) + 1, color: c('glyph', 1 - t / 0.4) });
    } else {
      bursts.push({ position, alt, size: 18 + 42 * e, angle: spin(position), color: c('weapon.offensive', fade ** 1.2) });
      rings.push({ position, alt, radius: 7 + 24 * e, width: 1.5, color: c('weapon.offensive', 0.85 * fade) });
      if (t < 0.6) discs.push({ position, alt, radius: 6 + 6 * t, color: c('glyph', 1 - t / 0.6) });
    }
  }
  return out;
}
