// The icon atlas (doc 06 §6, doc 10 §4): every symbol in every side colour rasterised once
// into one canvas, with a deck.gl IconLayer mapping. Built at app start from the SVGs in
// icons.ts, in a fixed order, so the layout is the same on every run (ADR-020 (Proposed):
// no build-time rasteriser dependency).
import { SYMBOLS } from '@warboard/protocol';
import { ICON_SIDES, type IconSide, iconDataUrl } from './icons';

/** Pixel size of one icon in the atlas; layers draw it at 30 px (24 px zoomed out). */
export const ATLAS_CELL = 64;
const COLUMNS = 16;

export interface IconMappingEntry {
  x: number;
  y: number;
  width: number;
  height: number;
  anchorY: number;
  mask: false;
}

export interface IconAtlas {
  /** PNG data URL of the atlas. */
  image: string;
  mapping: Record<string, IconMappingEntry>;
}

/** Atlas key of a symbol in a side colour. */
export const iconKey = (symbol: number, side: IconSide, estimate = false) =>
  `${symbol}:${side}${estimate ? ':est' : ''}`;

async function load(url: string): Promise<HTMLImageElement> {
  const img = new Image(ATLAS_CELL, ATLAS_CELL);
  img.src = url;
  await img.decode();
  return img;
}

export async function buildAtlas(): Promise<IconAtlas> {
  const codes = Object.values(SYMBOLS).sort((a, b) => a - b);
  const entries: { key: string; url: string }[] = [];
  for (const code of codes) {
    for (const side of ICON_SIDES) {
      entries.push({ key: iconKey(code, side), url: iconDataUrl(code, side) });
      entries.push({ key: iconKey(code, side, true), url: iconDataUrl(code, side, { estimate: true }) });
    }
  }
  const rows = Math.ceil(entries.length / COLUMNS);
  const canvas = document.createElement('canvas');
  canvas.width = COLUMNS * ATLAS_CELL;
  canvas.height = rows * ATLAS_CELL;
  const ctx = canvas.getContext('2d');
  if (!ctx) throw new Error('No 2D canvas for the icon atlas');
  const images = await Promise.all(entries.map((e) => load(e.url)));
  const mapping: Record<string, IconMappingEntry> = {};
  entries.forEach((e, i) => {
    const x = (i % COLUMNS) * ATLAS_CELL;
    const y = Math.floor(i / COLUMNS) * ATLAS_CELL;
    const img = images[i];
    if (img) ctx.drawImage(img, x, y, ATLAS_CELL, ATLAS_CELL);
    mapping[e.key] = { x, y, width: ATLAS_CELL, height: ATLAS_CELL, anchorY: ATLAS_CELL / 2, mask: false };
  });
  return { image: canvas.toDataURL('image/png'), mapping };
}
