// Combat effects and the targeted-unit halo on the 2D map (doc 10 §5, reference 03): a small
// white launch flash at the shooter, a cyan burst where an interceptor fuzed, an amber-white
// burst where a weapon hit, and a slowly pulsing red glow around units known to be targeted.
//
// Everything is stateless from the side view (doc 06 §1): the engine sends each effect with its
// age in sim ms, and the animation here is a pure function of that age, so a redraw, a pause or
// a reload cannot replay or lose an effect. Age is turned into real time by the achieved sim
// speed (at ×10 an effect lasts a tenth of its time in sim ms), so flashes stay visible at any
// speed; the engine keeps them for `RETENTION_MS` of sim time for that reason. Effects are drawn
// with three layers whatever their number, so a naval salvo against interceptors (hundreds of
// weapons at once) costs a few buffer updates per frame, not per effect.
import type { Layer } from '@deck.gl/core';
import { IconLayer, ScatterplotLayer } from '@deck.gl/layers';
import { EFFECT, type SideView } from '@warboard/protocol';
import { alpha, burstDataUrl, rgba, type TokenName } from '@warboard/symbols';
import type { LonLat } from '../../store';

type Rgba = [number, number, number, number];

/** How long each effect is drawn, real ms. */
const LIFE_MS: Record<number, number> = {
  [EFFECT.LAUNCH]: 650,
  [EFFECT.INTERCEPT]: 900,
  [EFFECT.HIT]: 1300,
};

/** Radius of the targeted glow, px (doc 10 §5), and its pulse period. */
const HALO_RADIUS_PX = 34;
const HALO_PERIOD_MS = 2400;
/** Concentric steps that fake the soft edge of the glow without a custom shader. */
const HALO_STEPS = 12;

const BURST_ICON = { url: burstDataUrl(), width: 64, height: 64, id: 'burst', mask: true };

interface Disc {
  position: LonLat;
  radius: number;
  color: Rgba;
}

interface Ring extends Disc {
  width: number;
}

interface Burst {
  position: LonLat;
  size: number;
  angle: number;
  color: Rgba;
}

const easeOut = (t: number) => 1 - (1 - t) ** 3;
const c = (t: TokenName, a: number): Rgba => rgba(t, a);

/** Spoke rotation from the position, so bursts differ but a redraw never changes one. */
const spin = (p: LonLat) => Math.abs(p[0] * 7919 + p[1] * 104729) % 45;

/** The targeted glow: a red disc fading to nothing at the rim, pulsing slowly. Positions are
 *  where the units are drawn this frame. */
export function haloLayer(positions: LonLat[], nowMs: number): Layer {
  const phase = Math.sin((2 * Math.PI * nowMs) / HALO_PERIOD_MS);
  const peak = alpha['halo.targeted'] * (0.85 + 0.15 * phase);
  const radius = HALO_RADIUS_PX * (0.96 + 0.04 * phase);
  // Layers composite one over the other from the rim in, so each step's opacity is chosen to
  // make the total at that step follow `peak × (1 − r/R)^0.9`: strongest next to the icon,
  // nearly gone at the rim, as in reference 03.
  const steps: Disc[] = [];
  const total = (j: number) => peak * ((j + 1) / HALO_STEPS) ** 0.9;
  for (const position of positions) {
    let before = 0;
    for (let j = 0; j < HALO_STEPS; j++) {
      const target = total(j);
      const a = 1 - (1 - target) / (1 - before);
      before = target;
      steps.push({ position, radius: radius * (1 - j / HALO_STEPS), color: c('halo.targeted', Math.max(0, a)) });
    }
  }
  return new ScatterplotLayer<Disc>({
    id: 'targeted-halos',
    data: steps,
    getPosition: (d) => d.position,
    getRadius: (d) => d.radius,
    radiusUnits: 'pixels',
    filled: true,
    stroked: false,
    getFillColor: (d) => d.color,
  });
}

/** Launch, intercept and hit effects of the view, as three layers (bursts, rings, discs).
 *  `dtS` is the sim time since the view arrived and `simPerReal` the achieved speed. */
export function effectLayers(view: SideView, dtS: number, simPerReal: number): Layer[] {
  const ev = view.effects;
  if (ev.count === 0) return [];
  const speed = Math.max(1, simPerReal);
  const bursts: Burst[] = [];
  const rings: Ring[] = [];
  const discs: Disc[] = [];
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
    if (kind === EFFECT.LAUNCH) {
      discs.push({ position, radius: 3 + 8 * e, color: c('glyph', 0.95 * fade * fade) });
      rings.push({ position, radius: 5 + 14 * e, width: 1.5, color: c('glyph', 0.7 * fade) });
    } else if (kind === EFFECT.INTERCEPT) {
      bursts.push({ position, size: 14 + 36 * e, angle: spin(position), color: c('weapon.interceptor', fade ** 1.3) });
      rings.push({ position, radius: 6 + 20 * e, width: 1.5, color: c('weapon.interceptor', 0.9 * fade) });
      if (t < 0.4) discs.push({ position, radius: 4 * (1 - t / 0.4) + 1, color: c('glyph', 1 - t / 0.4) });
    } else {
      bursts.push({ position, size: 18 + 42 * e, angle: spin(position), color: c('weapon.offensive', fade ** 1.2) });
      rings.push({ position, radius: 7 + 24 * e, width: 1.5, color: c('weapon.offensive', 0.85 * fade) });
      if (t < 0.6) discs.push({ position, radius: 6 + 6 * t, color: c('glyph', 1 - t / 0.6) });
    }
  }
  return [
    new IconLayer<Burst>({
      id: 'fx-bursts',
      data: bursts,
      getIcon: () => BURST_ICON,
      getPosition: (d) => d.position,
      getSize: (d) => d.size,
      getAngle: (d) => d.angle,
      getColor: (d) => d.color,
      sizeUnits: 'pixels',
    }),
    new ScatterplotLayer<Ring>({
      id: 'fx-rings',
      data: rings,
      getPosition: (d) => d.position,
      getRadius: (d) => d.radius,
      radiusUnits: 'pixels',
      filled: false,
      stroked: true,
      getLineColor: (d) => d.color,
      getLineWidth: (d) => d.width,
      lineWidthUnits: 'pixels',
    }),
    new ScatterplotLayer<Disc>({
      id: 'fx-discs',
      data: discs,
      getPosition: (d) => d.position,
      getRadius: (d) => d.radius,
      radiusUnits: 'pixels',
      filled: true,
      stroked: false,
      getFillColor: (d) => d.color,
    }),
  ];
}
