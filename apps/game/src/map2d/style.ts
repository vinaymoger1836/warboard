// Dark 2D map style (doc 05 §6, doc 10 §3) over the Protomaps v4 schema, built only from
// the colour tokens. The world pack draws everywhere; a theatre pack (europe) draws on top
// from its first zoom inside its extent. Only the physical layers of the basemap are drawn
// (land, water, coast); its real labels and boundaries stay hidden. Borders come from the
// packs' generated territory (doc 11 §2.2) and labels from the content's places (§2.4).
import { cssRgba, hex } from '@warboard/symbols';
import type { ExpressionSpecification, LayerSpecification, Map as MapLibreMap, StyleSpecification } from 'maplibre-gl';
import type { PlaceLabel } from '../engine/messages';
import type { Coalition } from './coalitions';

export interface MapPack {
  id: string;
  basemap: string;
  territory: string;
  /** First and last zoom of the pack's basemap tiles. */
  zoom: [number, number];
  /** [west, south, east, north] degrees. */
  extent: [number, number, number, number];
  isWorld: boolean;
}

const FONT = {
  regular: ['Noto Sans Regular'],
  italic: ['Noto Sans Italic'],
  medium: ['Noto Sans Medium'],
};
const HALO = { 'text-halo-color': hex('map.label.halo'), 'text-halo-width': 2 } as const;

/** The labels as GeoJSON points for the `places` source. */
function placesGeoJson(places: PlaceLabel[]) {
  return {
    type: 'FeatureCollection' as const,
    features: places.map((p) => ({
      type: 'Feature' as const,
      geometry: { type: 'Point' as const, coordinates: [p.lon, p.lat] },
      properties: { name: p.name, kind: p.kind, nation: p.nation ?? '' },
    })),
  };
}

const kindIs = (...kinds: string[]): ExpressionSpecification => ['in', ['get', 'kind'], ['literal', kinds]];

/** Territory of the war's coalitions (land and 12 nm waters). */
export function tintFilter(coalition: Coalition): ExpressionSpecification {
  return ['in', ['get', 'code'], ['literal', [...coalition.own, ...coalition.enemy]]];
}

/** Own colour for the side on screen, enemy colour for the rest (doc 12 §2). */
export function tintColour(coalition: Coalition, ownColor: string, enemyColor: string): ExpressionSpecification {
  return ['case', ['in', ['get', 'code'], ['literal', coalition.own]], ownColor, enemyColor];
}

/** Re-tints the territory in place after a side switch, so the map (and its camera) stays. */
export function applyCoalition(map: MapLibreMap, coalition: Coalition): void {
  for (const layer of map.getStyle().layers) {
    if (layer.id.endsWith('-territory-fill')) {
      map.setFilter(layer.id, tintFilter(coalition));
      map.setPaintProperty(layer.id, 'fill-color', tintColour(coalition, hex('terr.own.fill'), hex('terr.enemy.fill')));
    } else if (layer.id.endsWith('-territory-line')) {
      map.setFilter(layer.id, tintFilter(coalition));
      map.setPaintProperty(layer.id, 'line-color', tintColour(coalition, hex('terr.own.line'), hex('terr.enemy.line')));
    }
  }
}

export function buildStyle(
  packs: MapPack[],
  coalition: Coalition,
  glyphsBase: string,
  places: PlaceLabel[],
): StyleSpecification {
  const world = packs.find((p) => p.isWorld);
  const theatres = packs.filter((p) => !p.isWorld);
  const ordered = [...(world ? [world] : []), ...theatres];
  const sources: StyleSpecification['sources'] = {};
  for (const p of ordered) {
    sources[`bm-${p.id}`] = { type: 'vector', url: `pmtiles://${p.basemap}`, attribution: '© OpenStreetMap' };
    sources[`ter-${p.id}`] = { type: 'vector', url: `pmtiles://${p.territory}` };
  }
  sources.places = { type: 'geojson', data: placesGeoJson(places) };
  // World layers hand over to a theatre's own layers from the theatre's first zoom.
  const handover = theatres.length ? Math.min(...theatres.map((t) => t.zoom[0])) : null;
  const zoomRange = (p: MapPack): Partial<LayerSpecification> =>
    p.isWorld ? {} : { minzoom: p.zoom[0] };
  const polyRange = (p: MapPack): Partial<LayerSpecification> =>
    p.isWorld ? (handover !== null ? { maxzoom: handover } : {}) : { minzoom: p.zoom[0] };
  const tinted = tintFilter(coalition);
  const pick = (ownColor: string, enemyColor: string) => tintColour(coalition, ownColor, enemyColor);

  const layers: LayerSpecification[] = [{ id: 'background', type: 'background', paint: { 'background-color': hex('map.sea') } }];

  for (const p of ordered) {
    const src = `bm-${p.id}`;
    layers.push(
      {
        id: `${p.id}-earth`,
        type: 'fill',
        source: src,
        'source-layer': 'earth',
        ...zoomRange(p),
        paint: { 'fill-color': hex('map.land'), 'fill-antialias': false },
      } as LayerSpecification,
      {
        id: `${p.id}-water`,
        type: 'fill',
        source: src,
        'source-layer': 'water',
        filter: ['==', ['geometry-type'], 'Polygon'],
        ...zoomRange(p),
        paint: { 'fill-color': hex('map.sea') },
      } as LayerSpecification,
    );
  }

  for (const p of ordered) {
    const src = `ter-${p.id}`;
    layers.push({
      id: `${p.id}-territory-fill`,
      type: 'fill',
      source: src,
      'source-layer': 'territory',
      filter: tinted,
      ...polyRange(p),
      paint: { 'fill-color': pick(hex('terr.own.fill'), hex('terr.enemy.fill')), 'fill-antialias': false },
    } as LayerSpecification);
  }

  for (const p of ordered) {
    const src = `bm-${p.id}`;
    layers.push(
      {
        id: `${p.id}-coast`,
        type: 'line',
        source: src,
        'source-layer': 'earth',
        ...polyRange(p),
        paint: { 'line-color': hex('map.coast'), 'line-width': 0.6 },
      } as LayerSpecification,
      {
        id: `${p.id}-borders-sea`,
        type: 'line',
        source: `ter-${p.id}`,
        'source-layer': 'borders',
        filter: ['==', ['get', 'kind'], 'sea'],
        ...polyRange(p),
        paint: { 'line-color': hex('map.border'), 'line-width': 0.7, 'line-dasharray': [3, 3] },
      } as LayerSpecification,
      {
        id: `${p.id}-borders-land`,
        type: 'line',
        source: `ter-${p.id}`,
        'source-layer': 'borders',
        filter: ['==', ['get', 'kind'], 'land'],
        ...polyRange(p),
        paint: { 'line-color': hex('map.border'), 'line-width': ['interpolate', ['linear'], ['zoom'], 2, 0.6, 8, 1.2] },
      } as LayerSpecification,
      {
        id: `${p.id}-territory-line`,
        type: 'line',
        source: `ter-${p.id}`,
        'source-layer': 'territory',
        filter: tinted,
        ...polyRange(p),
        paint: {
          'line-color': pick(hex('terr.own.line'), hex('terr.enemy.line')),
          'line-width': ['interpolate', ['linear'], ['zoom'], 2, 1, 8, 1.8],
        },
      } as LayerSpecification,
    );
  }

  // Fictional labels (doc 10 §3, doc 11 §2.4): water, islands and regions, nations, cities.
  layers.push(
    {
      id: 'labels-water',
      type: 'symbol',
      source: 'places',
      filter: kindIs('sea', 'gulf', 'strait'),
      layout: {
        'text-field': ['upcase', ['get', 'name']],
        'text-font': FONT.italic,
        'text-size': ['interpolate', ['linear'], ['zoom'], 3, ['match', ['get', 'kind'], 'sea', 11, 9], 8, ['match', ['get', 'kind'], 'sea', 15, 13]],
        'text-letter-spacing': 0.12,
        'text-max-width': 7,
      },
      paint: {
        'text-color': hex('map.label.sea'),
        'text-opacity': ['step', ['zoom'], ['match', ['get', 'kind'], 'sea', 1, 0], 4.5, ['match', ['get', 'kind'], 'strait', 0, 1], 5.5, 1],
        ...HALO,
      },
    } as LayerSpecification,
    {
      id: 'labels-land',
      type: 'symbol',
      source: 'places',
      filter: kindIs('islands', 'region'),
      minzoom: 5,
      layout: {
        'text-field': ['get', 'name'],
        'text-font': FONT.italic,
        'text-size': ['interpolate', ['linear'], ['zoom'], 5, 10, 9, 13],
        'text-max-width': 7,
      },
      paint: { 'text-color': hex('map.label.sea'), ...HALO },
    } as LayerSpecification,
    {
      id: 'labels-city-dots',
      type: 'circle',
      source: 'places',
      filter: ['any', ['all', kindIs('capital'), ['>=', ['zoom'], 3]], ['all', kindIs('city'), ['>=', ['zoom'], 5]]],
      paint: {
        'circle-color': hex('map.city.dot'),
        'circle-radius': ['interpolate', ['linear'], ['zoom'], 3, ['match', ['get', 'kind'], 'capital', 2.5, 2], 8, ['match', ['get', 'kind'], 'capital', 4, 3]],
        'circle-stroke-color': cssRgba('map.label.halo', 0.8),
        'circle-stroke-width': 1,
      },
    } as LayerSpecification,
    {
      id: 'labels-cities',
      type: 'symbol',
      source: 'places',
      filter: ['any', ['all', kindIs('capital'), ['>=', ['zoom'], 3]], ['all', kindIs('city'), ['>=', ['zoom'], 5]]],
      layout: {
        'text-field': ['get', 'name'],
        'text-font': FONT.italic,
        'text-size': ['interpolate', ['linear'], ['zoom'], 3, ['match', ['get', 'kind'], 'capital', 12, 11], 8, ['match', ['get', 'kind'], 'capital', 15, 14]],
        'text-variable-anchor': ['top-left', 'top', 'bottom-left', 'left'],
        'text-radial-offset': 0.5,
        'text-justify': 'auto',
        'symbol-sort-key': ['match', ['get', 'kind'], 'capital', 0, 1],
      },
      paint: { 'text-color': hex('map.label.city'), ...HALO },
    } as LayerSpecification,
    {
      id: 'labels-nations',
      type: 'symbol',
      source: 'places',
      filter: kindIs('nation'),
      maxzoom: 9,
      layout: {
        'text-field': ['upcase', ['get', 'name']],
        'text-font': FONT.medium,
        'text-size': ['interpolate', ['linear'], ['zoom'], 2, 10, 6, 14, 9, 16],
        'text-letter-spacing': 0.12,
        'text-max-width': 8,
        'symbol-sort-key': 0,
      },
      paint: { 'text-color': hex('map.label.country'), ...HALO },
    } as LayerSpecification,
  );

  return {
    version: 8,
    glyphs: `${glyphsBase}{fontstack}/{range}.pbf`,
    sources,
    layers,
  };
}
