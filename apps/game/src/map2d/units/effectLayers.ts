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
import type { SideView } from '@warboard/protocol';
import { alpha, burstDataUrl, rgba, type TokenName } from '@warboard/symbols';
import { type Burst, buildEffects, type Disc, type Ring } from '../../scene/effects';
import type { LonLat } from '../../store';

type Rgba = [number, number, number, number];

/** Radius of the targeted glow, px (doc 10 §5), and its pulse period. */
const HALO_RADIUS_PX = 34;
const HALO_PERIOD_MS = 2400;
/** Concentric steps that fake the soft edge of the glow without a custom shader. */
const HALO_STEPS = 12;

const BURST_ICON = { url: burstDataUrl(), width: 64, height: 64, id: 'burst', mask: true };

interface HaloDisc {
  position: LonLat;
  radius: number;
  color: Rgba;
}

const c = (t: TokenName, a: number): Rgba => rgba(t, a);

/** The targeted glow: a red disc fading to nothing at the rim, pulsing slowly. Positions are
 *  where the units are drawn this frame. */
export function haloLayer(positions: LonLat[], nowMs: number): Layer {
  const phase = Math.sin((2 * Math.PI * nowMs) / HALO_PERIOD_MS);
  const peak = alpha['halo.targeted'] * (0.85 + 0.15 * phase);
  const radius = HALO_RADIUS_PX * (0.96 + 0.04 * phase);
  // Layers composite one over the other from the rim in, so each step's opacity is chosen to
  // make the total at that step follow `peak × (1 − r/R)^0.9`: strongest next to the icon,
  // nearly gone at the rim, as in reference 03.
  const steps: HaloDisc[] = [];
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
  return new ScatterplotLayer<HaloDisc>({
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
  if (view.effects.count === 0) return [];
  const { bursts, rings, discs } = buildEffects(view, dtS, simPerReal);
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
