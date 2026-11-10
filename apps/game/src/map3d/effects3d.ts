// Launch, intercept and hit effects in the 3D view (doc 10 §5): the shared effect data
// (`scene/effects.ts`) as billboards at the effect's altitude. Bursts use the same spoked glyph
// as 2D, rings and discs are white images tinted per billboard; sizes are the same pixels.
import { Color } from '@cesium/core';
import type { SideView } from '@warboard/protocol';
import { buildEffects } from '../scene/effects';
import type { LonLat } from '../scene/geo';
import { burstImage, discImage, ringImage } from './icons3d';

export interface Effect3d {
  key: string;
  imageKey: string;
  image: HTMLCanvasElement | null;
  position: LonLat;
  alt: number;
  scale: number;
  color: Color;
  rotation: number;
}

const RING_PX = 16;
const toColor = (c: [number, number, number, number]) => new Color(c[0] / 255, c[1] / 255, c[2] / 255, c[3] / 255);

export function effectsOf(view: SideView, dtS: number, simPerReal: number): Effect3d[] {
  if (view.effects.count === 0) return [];
  const { bursts, rings, discs } = buildEffects(view, dtS, simPerReal);
  const out: Effect3d[] = [];
  bursts.forEach((b, i) =>
    out.push({
      key: `fx:b:${i}`,
      imageKey: 'burst',
      image: burstImage(),
      position: b.position,
      alt: b.alt,
      scale: b.size / 64,
      color: toColor(b.color),
      rotation: (b.angle * Math.PI) / 180,
    }),
  );
  rings.forEach((r, i) =>
    out.push({
      key: `fx:r:${i}`,
      imageKey: `ring:${RING_PX}:2`,
      image: ringImage(RING_PX, 2),
      position: r.position,
      alt: r.alt,
      scale: r.radius / RING_PX,
      color: toColor(r.color),
      rotation: 0,
    }),
  );
  discs.forEach((d, i) =>
    out.push({
      key: `fx:d:${i}`,
      imageKey: 'disc',
      image: discImage(),
      position: d.position,
      alt: d.alt,
      scale: d.radius / 15,
      color: toColor(d.color),
      rotation: 0,
    }),
  );
  return out;
}
