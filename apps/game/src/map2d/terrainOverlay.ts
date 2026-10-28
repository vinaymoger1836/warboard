// Terrain-class overlay (doc 05 §6): H3 cells of the visible area from the engine worker,
// drawn as a deck.gl polygon layer in the terrain tokens. Water is not drawn.
import { SolidPolygonLayer } from '@deck.gl/layers';
import type { MapboxOverlay } from '@deck.gl/mapbox';
import { alpha, rgba, type TokenName } from '@warboard/symbols';
import type { Map as MapLibreMap } from 'maplibre-gl';
import { useEffect } from 'react';
import type { EngineBridge } from '../bridge';

/** Class codes in the order of doc 05 §3 (and wb_geo::TerrainClass). */
export const TERRAIN_CLASSES = ['water', 'open', 'forest', 'jungle', 'urban', 'desert', 'mountain', 'marsh', 'arctic'] as const;
const MIN_ZOOM = 4.5;
const MAX_CELLS = 100_000;

const colours = TERRAIN_CLASSES.map((c) =>
  c === 'water' ? ([0, 0, 0, 0] as const) : rgba(`terrain.${c}` as TokenName, alpha.terrain),
);

interface Cell {
  polygon: [number, number][];
  cls: number;
}

function unpack(packed: Float64Array): Cell[] {
  const out: Cell[] = [];
  let i = 0;
  while (i < packed.length) {
    const cls = packed[i++] ?? 0;
    const n = packed[i++] ?? 0;
    const polygon: [number, number][] = [];
    let minLon = Infinity;
    let maxLon = -Infinity;
    for (let k = 0; k < n; k++) {
      const lon = packed[i++] ?? 0;
      const lat = packed[i++] ?? 0;
      polygon.push([lon, lat]);
      minLon = Math.min(minLon, lon);
      maxLon = Math.max(maxLon, lon);
    }
    if (maxLon - minLon < 180) out.push({ polygon, cls });
  }
  return out;
}

export function useTerrainOverlay(
  map: MapLibreMap | null,
  overlay: MapboxOverlay | null,
  bridge: EngineBridge | null,
  enabled: boolean,
  onToast: (text: string) => void,
): void {
  useEffect(() => {
    if (!map || !overlay || !bridge) return;
    if (!enabled) {
      overlay.setProps({ layers: [] });
      return;
    }
    let seq = 0;
    let warned = false;
    const refresh = () => {
      const mine = ++seq;
      if (map.getZoom() < MIN_ZOOM) {
        overlay.setProps({ layers: [] });
        if (!warned) onToast('Zoom in to see terrain classes');
        warned = true;
        return;
      }
      const b = map.getBounds();
      const bbox: [number, number, number, number] = [
        Math.max(-180, b.getWest()),
        Math.max(-85, b.getSouth()),
        Math.min(180, b.getEast()),
        Math.min(85, b.getNorth()),
      ];
      bridge
        .request({ type: 'terrainCells', bbox, max: MAX_CELLS })
        .then((packed) => {
          if (mine !== seq) return;
          if (!packed) {
            overlay.setProps({ layers: [] });
            if (!warned) onToast('Zoom in to see terrain classes');
            warned = true;
            return;
          }
          warned = false;
          overlay.setProps({
            layers: [
              new SolidPolygonLayer<Cell>({
                id: 'terrain-classes',
                data: unpack(packed),
                getPolygon: (d) => d.polygon,
                getFillColor: (d) => [...(colours[d.cls] ?? [0, 0, 0, 0])] as [number, number, number, number],
                pickable: false,
              }),
            ],
          });
        })
        .catch((err: unknown) => onToast(err instanceof Error ? err.message : String(err)));
    };
    refresh();
    map.on('moveend', refresh);
    return () => {
      seq++;
      map.off('moveend', refresh);
      overlay.setProps({ layers: [] });
    };
  }, [map, overlay, bridge, enabled, onToast]);
}
