// Colour tokens from docs/10-ui-style-guide.md §2. The only source of colour in the
// game: CSS variables for panels (`--map-sea` for `map.sea`), RGBA arrays for deck.gl
// and Cesium, hex strings for MapLibre styles. Add a token to the guide first.

export const tokens = {
  // §2.1 Map
  'map.sea': '#2C343C',
  'map.land': '#0E0E0E',
  'map.border': '#3A4048',
  'map.coast': '#3A4048',
  'map.label.country': '#E6E8EB',
  'map.label.sea': '#5E6E86',
  'map.label.city': '#E6E8EB',
  'map.label.halo': '#0E0E0E',
  'map.city.dot': '#C9A227',
  'terr.own.fill': '#423662',
  'terr.own.line': '#544C7C',
  'terr.enemy.fill': '#284A62',
  'terr.enemy.line': '#4C98CC',
  // §2.1 Terrain-class overlay (drawn at `alpha['terrain']`; water is not drawn)
  'terrain.open': '#5A6A3A',
  'terrain.forest': '#2E5A36',
  'terrain.jungle': '#1E6A44',
  'terrain.urban': '#9A8C7A',
  'terrain.desert': '#A08A50',
  'terrain.mountain': '#7A6658',
  'terrain.marsh': '#3A6A6A',
  'terrain.arctic': '#B8C4CC',

  // §2.2 Units, weapons, overlays
  'side.own': '#886CD4',
  'side.own.text': '#755EB3',
  'side.enemy': '#4C9CD4',
  'side.neutral': '#8C949C',
  'side.unknown': '#ECB030',
  glyph: '#FFFFFF',
  'label.unit': '#D9D9D9',
  'weapon.offensive': '#FCB020',
  'weapon.line': '#ECB030',
  'weapon.interceptor': '#4CD0E0',
  'weapon.enemy': '#D8504C',
  'weapon.enemy.label': '#E0504C',
  'halo.targeted': '#A05460',
  'ring.sensor': '#4CC0F4',
  'ring.engage': '#E08804',
  'ring.radius': '#A058AC',
  'ring.radius.text': '#B868C8',
  'ring.threat': '#D8504C',
  sat: '#FFD54F',
  'sat.label': '#E9C34A',
  'sat.track': '#B49C48',
  waypoint: '#886CD4',
  'waypoint.label': '#FFFFFF',

  // §2.3 Panels and chrome
  'panel.bg': '#0A101A',
  'panel.header': '#0E1E2C',
  'panel.inset': '#060C14',
  'panel.border': '#245470',
  'text.primary': '#FFFFFF',
  'text.secondary': '#8C9AAA',
  'text.muted': '#586675',
  'state.ok': '#4CA85C',
  'state.warn': '#FCB020',
  'state.bad': '#E03C68',
  'state.station': '#B868C8',
  'btn.border': '#FCB020',
  'btn.text': '#FCB020',
  'chrome.bg': '#0C141C',
  'toast.bg': '#080E16',
  'toast.text': '#C4CED6',
  'scale.bg': '#FFFFFF',
  'scale.text': '#0E0E0E',
} as const;

export type TokenName = keyof typeof tokens;

/** Opacities the guide gives with a token. */
export const alpha = {
  'halo.targeted': 0.45,
  terrain: 0.45,
} as const;

/** `map.sea` → `--map-sea`. */
export function cssVarName(name: TokenName): string {
  return `--${name.replace(/\./g, '-')}`;
}

/** `map.sea` → `var(--map-sea)`. */
export function cssVar(name: TokenName): string {
  return `var(${cssVarName(name)})`;
}

/** Hex string, for MapLibre style JSON. */
export function hex(name: TokenName): string {
  return tokens[name];
}

/** `[r, g, b, a]` with every channel 0–255, for deck.gl and Cesium. */
export function rgba(name: TokenName, opacity = 1): [number, number, number, number] {
  const h = tokens[name];
  return [
    parseInt(h.slice(1, 3), 16),
    parseInt(h.slice(3, 5), 16),
    parseInt(h.slice(5, 7), 16),
    Math.round(Math.min(1, Math.max(0, opacity)) * 255),
  ];
}

/** `rgba(r, g, b, a)` CSS colour with an opacity, for MapLibre paint properties. */
export function cssRgba(name: TokenName, opacity: number): string {
  const [r, g, b] = rgba(name);
  return `rgba(${r}, ${g}, ${b}, ${opacity})`;
}

/** A `:root { … }` block declaring every token as a CSS variable. */
export function tokenCss(): string {
  const lines = (Object.keys(tokens) as TokenName[]).map((k) => `  ${cssVarName(k)}: ${tokens[k]};`);
  return `:root {\n${lines.join('\n')}\n}\n`;
}
