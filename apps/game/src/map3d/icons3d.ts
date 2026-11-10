// Billboard images for the 3D view, from the same sources as the 2D map (ADR-020, doc 10 §11):
// unit and contact icons are cut from the shared atlas, missile glyphs and the satellite dot
// come from the same SVG builders and tokens. Each image is made once and cached by key, and
// Cesium's billboard texture atlas caches them by the same key (`setImage(key, canvas)`).
import { ATLAS_CELL, alpha, burstDataUrl, type IconAtlas, missileDataUrl, rgba, type TokenName } from '@warboard/symbols';

const cache = new Map<string, HTMLCanvasElement>();
let atlasImage: HTMLImageElement | null = null;

function canvas(size: number): [HTMLCanvasElement, CanvasRenderingContext2D] {
  const c = document.createElement('canvas');
  c.width = size;
  c.height = size;
  const ctx = c.getContext('2d');
  if (!ctx) throw new Error('No 2D canvas');
  return [c, ctx];
}

/** Loads the atlas image once; call before drawing. */
export async function loadAtlas(atlas: IconAtlas): Promise<void> {
  if (atlasImage) return;
  const img = new Image();
  img.src = atlas.image;
  await img.decode();
  atlasImage = img;
}

/** The atlas cell of `key` (`iconKey(symbol, side, estimate)`) as its own canvas. */
export function iconImage(atlas: IconAtlas, key: string): HTMLCanvasElement | null {
  const hit = cache.get(key);
  if (hit) return hit;
  const m = atlas.mapping[key];
  if (!m || !atlasImage) return null;
  const [c, ctx] = canvas(ATLAS_CELL);
  ctx.drawImage(atlasImage, m.x, m.y, m.width, m.height, 0, 0, ATLAS_CELL, ATLAS_CELL);
  cache.set(key, c);
  return c;
}

const loading = new Map<string, Promise<void>>();

/** An SVG data URL drawn into a canvas once (async; null until ready). */
function svgImage(key: string, url: string, size: number): HTMLCanvasElement | null {
  const hit = cache.get(key);
  if (hit) return hit;
  if (!loading.has(key)) {
    const img = new Image();
    img.src = url;
    loading.set(
      key,
      img.decode().then(() => {
        const [c, ctx] = canvas(size);
        ctx.drawImage(img, 0, 0, size, size);
        cache.set(key, c);
      }),
    );
  }
  return null;
}

/** The missile glyph in a token colour (amber ours, cyan interceptors, red hostile). */
export const missileImage = (token: TokenName) => svgImage(`missile:${token}`, missileDataUrl(token), 32);

/** The spoked burst of intercepts and hits, white (tinted per billboard). */
export const burstImage = () => svgImage('burst', burstDataUrl(), 64);

/** Waits for the SVG glyphs the view uses. */
export async function loadGlyphs(): Promise<void> {
  missileImage('weapon.offensive');
  missileImage('weapon.interceptor');
  missileImage('weapon.enemy');
  burstImage();
  await Promise.all(loading.values());
}

const css = (token: TokenName, a = 1) => {
  const [r, g, b] = rgba(token, a);
  return `rgba(${r},${g},${b},${(a * 1).toFixed(3)})`;
};

/** Satellite: a yellow dot with a dark rim (doc 10 §4); `selected` adds the amber rim. */
export function satelliteImage(selected: boolean): HTMLCanvasElement {
  const key = `sat:${selected}`;
  const hit = cache.get(key);
  if (hit) return hit;
  const [c, ctx] = canvas(20);
  ctx.beginPath();
  ctx.arc(10, 10, 7, 0, 2 * Math.PI);
  ctx.fillStyle = css('sat');
  ctx.fill();
  ctx.lineWidth = selected ? 2 : 1;
  ctx.strokeStyle = selected ? css('weapon.offensive') : css('map.label.halo');
  ctx.stroke();
  cache.set(key, c);
  return c;
}

/** The targeted glow (doc 10 §5): a red disc fading to nothing at the rim. */
export function haloImage(): HTMLCanvasElement {
  const hit = cache.get('halo');
  if (hit) return hit;
  const [c, ctx] = canvas(72);
  const g = ctx.createRadialGradient(36, 36, 0, 36, 36, 34);
  const peak = alpha['halo.targeted'];
  for (let i = 0; i <= 10; i++) {
    const t = i / 10;
    g.addColorStop(t, css('halo.targeted', peak * (1 - t) ** 0.9));
  }
  ctx.fillStyle = g;
  ctx.fillRect(0, 0, 72, 72);
  cache.set('halo', c);
  return c;
}

/** A ring (selection, waypoint, effect ring) in a token colour; tinted white rings are
 *  coloured per billboard. */
export function ringImage(px: number, width: number): HTMLCanvasElement {
  const key = `ring:${px}:${width}`;
  const hit = cache.get(key);
  if (hit) return hit;
  const size = Math.ceil(px * 2 + width * 2 + 2);
  const [c, ctx] = canvas(size);
  ctx.beginPath();
  ctx.arc(size / 2, size / 2, px, 0, 2 * Math.PI);
  ctx.lineWidth = width;
  ctx.strokeStyle = '#ffffff';
  ctx.stroke();
  cache.set(key, c);
  return c;
}

/** A white filled disc for effect flashes (coloured per billboard). */
export function discImage(): HTMLCanvasElement {
  const hit = cache.get('disc');
  if (hit) return hit;
  const [c, ctx] = canvas(32);
  ctx.beginPath();
  ctx.arc(16, 16, 15, 0, 2 * Math.PI);
  ctx.fillStyle = '#ffffff';
  ctx.fill();
  cache.set('disc', c);
  return c;
}
