// Dark 2D map style (doc 05 §6, doc 10 §3) over the Protomaps v4 schema, built only from
// the colour tokens. The world pack draws everywhere; a theatre pack (europe) draws on top
// from its first zoom inside its extent.
import { cssRgba, hex } from '@warboard/symbols';
import type { ExpressionSpecification, LayerSpecification, StyleSpecification } from 'maplibre-gl';
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
// Filters see the integer zoom. Protomaps gives major cities min_zoom 2, so the world view
// leads by 1.5 to show them as in reference 06; from zoom 6 the filter lags by 2 so the
// theatre view keeps only towns, as in reference 01.
const CITY_FILTER: ExpressionSpecification = [
  'all',
  ['==', ['get', 'kind'], 'locality'],
  [
    'any',
    ['all', ['<', ['zoom'], 6], ['<=', ['get', 'min_zoom'], ['+', ['zoom'], 1.5]]],
    ['<=', ['get', 'min_zoom'], ['-', ['zoom'], 2]],
  ],
];
const NAME: ExpressionSpecification = ['coalesce', ['get', 'name:en'], ['get', 'name']];
const HALO = { 'text-halo-color': hex('map.label.halo'), 'text-halo-width': 2 } as const;

function extentPolygon([w, s, e, n]: MapPack['extent']) {
  return {
    type: 'Polygon' as const,
    coordinates: [
      [
        [w, s],
        [e, s],
        [e, n],
        [w, n],
        [w, s],
      ],
    ],
  };
}

export function buildStyle(packs: MapPack[], coalition: Coalition, glyphsBase: string): StyleSpecification {
  const world = packs.find((p) => p.isWorld);
  const theatres = packs.filter((p) => !p.isWorld);
  const ordered = [...(world ? [world] : []), ...theatres];
  const sources: StyleSpecification['sources'] = {};
  for (const p of ordered) {
    sources[`bm-${p.id}`] = { type: 'vector', url: `pmtiles://${p.basemap}`, attribution: '© OpenStreetMap' };
    sources[`ter-${p.id}`] = { type: 'vector', url: `pmtiles://${p.territory}` };
  }
  // World layers hand over to a theatre's own layers from the theatre's first zoom.
  const handover = theatres.length ? Math.min(...theatres.map((t) => t.zoom[0])) : null;
  const zoomRange = (p: MapPack): Partial<LayerSpecification> =>
    p.isWorld ? {} : { minzoom: p.zoom[0] };
  const polyRange = (p: MapPack): Partial<LayerSpecification> =>
    p.isWorld ? (handover !== null ? { maxzoom: handover } : {}) : { minzoom: p.zoom[0] };
  // World labels stay on outside the theatres once those take over.
  const outsideTheatres: ExpressionSpecification | null =
    handover === null
      ? null
      : [
          'any',
          ['<', ['zoom'], handover],
          ['!', ['any', ...theatres.map((t): ExpressionSpecification => ['within', extentPolygon(t.extent)])]],
        ];
  const labelFilter = (p: MapPack, f: ExpressionSpecification): ExpressionSpecification =>
    p.isWorld && outsideTheatres ? ['all', f, outsideTheatres] : f;

  const own = coalition.own;
  const enemy = coalition.enemy;
  const tinted: ExpressionSpecification = ['in', ['get', 'code'], ['literal', [...own, ...enemy]]];
  const pick = (ownColor: string, enemyColor: string): ExpressionSpecification => [
    'case',
    ['in', ['get', 'code'], ['literal', own]],
    ownColor,
    enemyColor,
  ];

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
        id: `${p.id}-sea-limit`,
        type: 'line',
        source: `ter-${p.id}`,
        'source-layer': 'territory',
        ...polyRange(p),
        paint: { 'line-color': hex('map.border'), 'line-width': 0.7, 'line-dasharray': [3, 3] },
      } as LayerSpecification,
      {
        id: `${p.id}-borders`,
        type: 'line',
        source: src,
        'source-layer': 'boundaries',
        filter: ['all', ['==', ['get', 'kind'], 'country'], ['!=', ['get', 'disputed'], true]],
        ...polyRange(p),
        paint: { 'line-color': hex('map.border'), 'line-width': ['interpolate', ['linear'], ['zoom'], 2, 0.6, 8, 1.2] },
      } as LayerSpecification,
      {
        id: `${p.id}-borders-disputed`,
        type: 'line',
        source: src,
        'source-layer': 'boundaries',
        filter: ['all', ['==', ['get', 'kind'], 'country'], ['==', ['get', 'disputed'], true]],
        ...polyRange(p),
        paint: { 'line-color': hex('map.border'), 'line-width': 0.8, 'line-dasharray': [2, 2] },
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

  for (const p of ordered) {
    const src = `bm-${p.id}`;
    layers.push(
      {
        id: `${p.id}-sea-labels`,
        type: 'symbol',
        source: src,
        'source-layer': 'water',
        filter: labelFilter(p, [
          'all',
          ['==', ['geometry-type'], 'Point'],
          ['in', ['get', 'kind'], ['literal', ['ocean', 'sea', 'bay', 'strait', 'gulf']]],
        ]),
        ...zoomRange(p),
        layout: {
          'text-field': ['upcase', NAME],
          'text-font': FONT.italic,
          'text-size': ['interpolate', ['linear'], ['zoom'], 2, 10, 8, 13],
          'text-letter-spacing': 0.12,
          'text-max-width': 7,
        },
        paint: { 'text-color': hex('map.label.sea'), ...HALO },
      } as LayerSpecification,
      {
        id: `${p.id}-city-dots`,
        type: 'circle',
        source: src,
        'source-layer': 'places',
        filter: labelFilter(p, CITY_FILTER),
        ...zoomRange(p),
        paint: {
          'circle-color': hex('map.city.dot'),
          'circle-radius': ['interpolate', ['linear'], ['zoom'], 2, 2, 8, 3],
          'circle-stroke-color': cssRgba('map.label.halo', 0.8),
          'circle-stroke-width': 1,
        },
      } as LayerSpecification,
      {
        id: `${p.id}-city-labels`,
        type: 'symbol',
        source: src,
        'source-layer': 'places',
        filter: labelFilter(p, CITY_FILTER),
        ...zoomRange(p),
        layout: {
          'text-field': NAME,
          'text-font': FONT.italic,
          'text-size': ['interpolate', ['linear'], ['zoom'], 2, 11, 8, 14],
          'text-variable-anchor': ['top-left', 'top', 'bottom-left', 'left'],
          'text-radial-offset': 0.5,
          'text-justify': 'auto',
          'symbol-sort-key': ['get', 'min_zoom'],
        },
        paint: { 'text-color': hex('map.label.city'), ...HALO },
      } as LayerSpecification,
      {
        id: `${p.id}-country-labels`,
        type: 'symbol',
        source: src,
        'source-layer': 'places',
        filter: labelFilter(p, ['==', ['get', 'kind'], 'country']),
        ...zoomRange(p),
        layout: {
          'text-field': ['upcase', NAME],
          'text-font': FONT.medium,
          'text-size': ['interpolate', ['linear'], ['zoom'], 2, 10, 6, 14, 9, 16],
          'text-letter-spacing': 0.12,
          'text-max-width': 8,
        },
        paint: { 'text-color': hex('map.label.country'), ...HALO },
      } as LayerSpecification,
    );
  }

  return {
    version: 8,
    glyphs: `${glyphsBase}{fontstack}/{range}.pbf`,
    sources,
    layers,
  };
}
