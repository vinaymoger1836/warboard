// Unit icons (doc 10 §4): a frame by category in the side colour, a white glyph by type,
// a 1 px stroke one step darker than the fill. All shapes are drawn here on a 64 × 64 grid;
// the atlas rasterises them for deck.gl (and later Cesium), the panels use the SVG directly.
import { SYMBOLS, type SymbolName } from '@warboard/protocol';
import { tokens, type TokenName } from './tokens';

/** Which colour family an icon is drawn in. */
export type IconSide = 'own' | 'enemy' | 'neutral' | 'unknown';

export const ICON_SIDES: readonly IconSide[] = ['own', 'enemy', 'neutral', 'unknown'];

const SIDE_TOKEN: Record<IconSide, TokenName> = {
  own: 'side.own',
  enemy: 'side.enemy',
  neutral: 'side.neutral',
  unknown: 'side.unknown',
};

export type Frame = 'tile' | 'hexagon' | 'site' | 'chevron' | 'dot';

/** Frame by symbol code: aircraft tiles, ship hexagons, site hexagons, ground chevrons. */
export function frameOf(symbol: number): Frame {
  if (symbol >= 1 && symbol < 20) return 'tile';
  if (symbol >= 20 && symbol < 40) return 'hexagon';
  if (symbol >= 40 && symbol < 60) return symbol === 40 || symbol === 41 || symbol === 42 ? 'site' : 'chevron';
  if (symbol >= 60 && symbol < 100) return 'dot';
  return 'site';
}

export const isAircraft = (symbol: number) => symbol >= 1 && symbol < 20;
export const isShip = (symbol: number) => symbol >= 20 && symbol < 40;
export const isBase = (symbol: number) => symbol >= 100;

const FRAMES: Record<Frame, string> = {
  // Rounded tile with a small point at the bottom.
  tile: 'M14 6 H50 Q58 6 58 14 V42 Q58 50 50 50 H38 L32 58 L26 50 H14 Q6 50 6 42 V14 Q6 6 14 6 Z',
  // Flat-top hexagon, wider than tall.
  hexagon: 'M19 11 H45 L59 32 L45 53 H19 L5 32 Z',
  // Rounded hexagon, pointy top.
  site: 'M32 5 L55 18.5 V45.5 L32 59 L9 45.5 V18.5 Z',
  chevron: 'M8 14 H44 L56 32 L44 50 H8 L18 32 Z',
  dot: 'M32 20 A12 12 0 1 1 31.99 20 Z',
};

/** Glyph centre per frame (the tile's body sits above its point). */
const GLYPH_CENTRE: Record<Frame, [number, number]> = {
  tile: [32, 28],
  hexagon: [32, 32],
  site: [32, 32],
  chevron: [32, 32],
  dot: [32, 32],
};

const PLANE_SWEPT = 'M32 10 L35 22 L50 34 V38 L35 34 L34 44 L40 49 V52 L32 50 L24 52 V49 L30 44 L29 34 L14 38 V34 L29 22 Z';
const PLANE_STRAIGHT =
  'M32 10 C34 10 35 13 35 16 V26 L54 34 V37 L35 33 V44 L41 48 V50 L32 48 L23 50 V48 L29 44 V33 L10 37 V34 L29 26 V16 C29 13 30 10 32 10 Z';

/** Side view of an aircraft climbing to the upper right. */
const PLANE_CLIMB = 'M10 40 L16 36 L22 37 L40 26 C44 23 50 21 53 22 C54 24 51 28 47 31 L30 41 L33 46 L29 47 L24 42 L14 44 Z';

/** Glyph markup by symbol name, drawn white on the 64 grid around (32, 32). */
const GLYPHS: Partial<Record<SymbolName, string>> = {
  fighter: `<path d="${PLANE_SWEPT}"/>`,
  attack: `<path d="${PLANE_SWEPT}"/>`,
  ew_aircraft: `<path d="${PLANE_SWEPT}"/><path d="M20 14 L44 14" stroke="#FFFFFF" stroke-width="3" fill="none"/>`,
  bomber: '<path d="M32 12 L36 24 L56 40 V44 L36 38 L35 46 L42 52 L32 50 L22 52 L29 46 L28 38 L8 44 V40 L28 24 Z"/>',
  transport: `<path d="${PLANE_STRAIGHT}"/>`,
  maritime_patrol: `<path d="${PLANE_STRAIGHT}"/>`,
  tanker: `<path d="${PLANE_STRAIGHT}"/><path d="M32 48 L32 58" stroke="#FFFFFF" stroke-width="3" fill="none"/>`,
  aew: `<path d="${PLANE_STRAIGHT}"/><ellipse cx="32" cy="30" rx="11" ry="4" fill="none" stroke="#FFFFFF" stroke-width="3"/>`,
  drone: '<path d="M32 16 L34 26 L54 28 V31 L34 31 L33 42 L38 46 L32 45 L26 46 L31 42 L30 31 L10 31 V28 L30 26 Z"/>',
  helicopter_attack:
    '<path d="M12 20 L52 44 M52 20 L12 44" stroke="#FFFFFF" stroke-width="3" fill="none"/><ellipse cx="32" cy="32" rx="6" ry="10"/><path d="M31 40 H33 V54 H31 Z"/>',
  helicopter_utility:
    '<path d="M12 20 L52 44 M52 20 L12 44" stroke="#FFFFFF" stroke-width="3" fill="none"/><ellipse cx="32" cy="32" rx="8" ry="11"/><path d="M31 41 H33 V54 H31 Z"/>',
  destroyer: '<path d="M8 36 H56 L49 45 H15 Z"/><path d="M22 27 H40 V36 H22 Z"/><path d="M30 16 H33 V27 H30 Z"/>',
  frigate: '<path d="M10 36 H54 L47 45 H17 Z"/><path d="M24 27 H39 V36 H24 Z"/><path d="M30 17 H33 V27 H30 Z"/>',
  corvette: '<path d="M13 36 H51 L45 44 H19 Z"/><path d="M25 28 H37 V36 H25 Z"/><path d="M30 20 H33 V28 H30 Z"/>',
  patrol_ship: '<path d="M16 36 H48 L43 43 H21 Z"/><path d="M27 29 H37 V36 H27 Z"/>',
  auxiliary: '<path d="M10 36 H54 L48 45 H16 Z"/><path d="M40 26 H50 V36 H40 Z"/><path d="M14 30 H34 V36 H14 Z"/>',
  submarine: '<path d="M10 38 C10 32 54 32 54 38 C54 44 10 44 10 38 Z"/><path d="M27 27 H36 V34 H27 Z"/>',
  radar: '<path d="M14 44 H50 M16 44 A16 16 0 0 1 48 44 M32 44 L42 30" stroke="#FFFFFF" stroke-width="3.5" fill="none" stroke-linecap="round"/>',
  sam_site:
    '<path d="M14 44 H50 M16 44 A16 16 0 0 1 48 44 M32 44 L42 30" stroke="#FFFFFF" stroke-width="3.5" fill="none" stroke-linecap="round"/>',
  launcher: '<path d="M14 46 H50 M18 46 L40 24 M26 46 L48 24" stroke="#FFFFFF" stroke-width="3.5" fill="none" stroke-linecap="round"/>',
  missile_site:
    '<path d="M14 46 H50 M18 46 L40 24 M26 46 L48 24" stroke="#FFFFFF" stroke-width="3.5" fill="none" stroke-linecap="round"/>',
  c2: '<path d="M22 14 V50" stroke="#FFFFFF" stroke-width="3.5" stroke-linecap="round"/><path d="M23 15 L46 22 L23 30 Z"/>',
  hq: '<path d="M22 14 V50" stroke="#FFFFFF" stroke-width="3.5" stroke-linecap="round"/><path d="M23 15 L46 22 L23 30 Z"/>',
  armour: '<rect x="14" y="22" width="36" height="20" rx="10" fill="none" stroke="#FFFFFF" stroke-width="3.5"/>',
  artillery: '<circle cx="32" cy="32" r="7"/>',
  infantry: '<path d="M16 20 L48 44 M48 20 L16 44" stroke="#FFFFFF" stroke-width="3.5" stroke-linecap="round"/>',
  naval_base:
    '<g fill="none" stroke="#FFFFFF" stroke-width="3.5" stroke-linecap="round"><circle cx="32" cy="17" r="4"/><path d="M32 21 V49 M23 28 H41 M17 38 C17 50 47 50 47 38"/></g>',
  anchorage:
    '<g fill="none" stroke="#FFFFFF" stroke-width="3.5" stroke-linecap="round"><circle cx="32" cy="17" r="4"/><path d="M32 21 V49 M17 38 C17 50 47 50 47 38"/></g>',
  // A side-view aircraft climbing away over a runway line (as SITE Air base in 01).
  air_base: `<path d="${PLANE_CLIMB}"/><path d="M12 50 H52" stroke="#FFFFFF" stroke-width="3.5" stroke-linecap="round"/>`,
  air_naval_base: `<path d="${PLANE_CLIMB}"/><path d="M12 50 H52" stroke="#FFFFFF" stroke-width="3.5" stroke-linecap="round"/>`,
  road_strip: '<path d="M14 50 H50 M20 40 H44" stroke="#FFFFFF" stroke-width="3.5" stroke-linecap="round" stroke-dasharray="6 4"/>',
  heliport: '<path d="M22 18 V46 M42 18 V46 M22 32 H42" stroke="#FFFFFF" stroke-width="4" stroke-linecap="round"/>',
  army_base: '<path d="M14 46 H50 L32 18 Z" fill="none" stroke="#FFFFFF" stroke-width="3.5" stroke-linejoin="round"/>',
};

const NAMES = new Map<number, SymbolName>(
  (Object.entries(SYMBOLS) as [SymbolName, number][]).map(([name, code]) => [code, name]),
);

export function symbolName(code: number): SymbolName {
  return NAMES.get(code) ?? 'unknown';
}

/** One step darker than a fill (the 1 px icon stroke, doc 10 §4). */
function darker(hex: string): string {
  const c = (i: number) =>
    Math.round(parseInt(hex.slice(i, i + 2), 16) * 0.72)
      .toString(16)
      .padStart(2, '0');
  return `#${c(1)}${c(3)}${c(5)}`;
}

export interface IconOptions {
  /** Dashed frame at 60 % for estimated contacts (doc 10 §4.1); P4 uses it. */
  estimate?: boolean;
}

/** The icon as a standalone 64 × 64 SVG document. */
export function iconSvg(symbol: number, side: IconSide, opts: IconOptions = {}): string {
  const frame = frameOf(symbol);
  const fill = tokens[SIDE_TOKEN[side]];
  const [cx, cy] = GLYPH_CENTRE[frame];
  const glyph = GLYPHS[symbolName(symbol)] ?? '<circle cx="32" cy="32" r="6"/>';
  const scale = frame === 'tile' ? 0.62 : 0.6;
  const dash = opts.estimate ? ' stroke-dasharray="6 4"' : '';
  const opacity = opts.estimate ? ' opacity="0.6"' : '';
  return (
    `<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64" viewBox="0 0 64 64"${opacity}>` +
    `<path d="${FRAMES[frame]}" fill="${fill}" stroke="${darker(fill)}" stroke-width="2" stroke-linejoin="round"${dash}/>` +
    `<g fill="${tokens.glyph}" transform="translate(${cx} ${cy}) scale(${scale}) translate(-32 -32)">${glyph}</g>` +
    '</svg>'
  );
}

export function iconDataUrl(symbol: number, side: IconSide, opts: IconOptions = {}): string {
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(iconSvg(symbol, side, opts))}`;
}
