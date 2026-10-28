// 2D strategic map (doc 06 §4, ADR-008): MapLibre GL JS v6 reading PMTiles from OPFS,
// with a deck.gl overlay for game layers. No default MapLibre controls (doc 10 §12);
// the chrome is ours.
import { MapboxOverlay } from '@deck.gl/mapbox';
import { addProtocol, Map as MapLibreMap, setWorkerUrl } from 'maplibre-gl';
import workerUrl from 'maplibre-gl/dist/maplibre-gl-worker.mjs?url';
import 'maplibre-gl/dist/maplibre-gl.css';
import { FileSource, PMTiles, Protocol } from 'pmtiles';
import { useEffect, useRef, useState } from 'react';
import type { EngineBridge } from '../bridge';
import { packFile } from '../packs/opfs';
import { fileFor, type InstalledPack } from '../packs/types';
import { DEMO_COALITION } from './coalitions';
import { MapChrome } from './MapChrome';
import { buildStyle, type MapPack } from './style';
import { useTerrainOverlay } from './terrainOverlay';

setWorkerUrl(workerUrl);

const protocol = new Protocol();
addProtocol('pmtiles', protocol.tile);

async function toMapPacks(packs: InstalledPack[]): Promise<MapPack[]> {
  const out: MapPack[] = [];
  for (const p of packs) {
    const m = p.manifest;
    const basemap = fileFor(m, 'basemap').name;
    const territory = fileFor(m, 'territory_tiles').name;
    // Re-register on every build: a reinstalled pack is a new File under the same name.
    for (const name of [basemap, territory]) {
      const file = await packFile(p.id, name);
      if (!file) throw new Error(`${p.id}/${name} is missing from browser storage`);
      protocol.add(new PMTiles(new FileSource(file)));
    }
    const [w, s, e, n] = m.extent;
    out.push({
      id: p.id,
      basemap,
      territory,
      zoom: m.basemap_zoom,
      extent: m.extent,
      isWorld: w <= -180 && s <= -90 && e >= 180 && n >= 90,
    });
  }
  return out;
}

export interface MapViewProps {
  packs: InstalledPack[];
  bridge: EngineBridge | null;
  geoReady: boolean;
  terrainOverlay: boolean;
  onTerrainOverlay: (on: boolean) => void;
  onToast: (text: string) => void;
}

export function MapView({ packs, bridge, geoReady, terrainOverlay, onTerrainOverlay, onToast }: MapViewProps) {
  const container = useRef<HTMLDivElement>(null);
  const [map, setMap] = useState<MapLibreMap | null>(null);
  const [overlay, setOverlay] = useState<MapboxOverlay | null>(null);
  const packKey = packs.map((p) => `${p.id}@${p.version}`).join();

  useEffect(() => {
    const el = container.current;
    if (!el || packs.length === 0) return;
    let cancelled = false;
    let created: MapLibreMap | null = null;
    toMapPacks(packs)
      .then((mapPacks) => {
        if (cancelled) return;
        const glyphs = new URL('fonts/glyphs/', document.baseURI).href;
        created = new MapLibreMap({
          container: el,
          style: buildStyle(mapPacks, DEMO_COALITION, glyphs),
          center: [15, 50],
          zoom: 3,
          minZoom: 1,
          maxZoom: 12,
          attributionControl: false,
          dragRotate: false,
          pitchWithRotate: false,
          touchPitch: false,
          renderWorldCopies: true,
        });
        created.touchZoomRotate.disableRotation();
        // Overlaid, not interleaved: deck.gl 9.4's interleaved path reads custom-layer
        // render arguments that MapLibre 6 no longer passes. Game layers sit above the map.
        const deck = new MapboxOverlay({ interleaved: false, layers: [] });
        created.addControl(deck);
        setOverlay(deck);
        setMap(created);
        (window as unknown as { __wbMap?: MapLibreMap }).__wbMap = created;
      })
      .catch((err: unknown) => onToast(err instanceof Error ? err.message : String(err)));
    return () => {
      cancelled = true;
      setOverlay(null);
      setMap(null);
      created?.remove();
    };
    // packKey stands for packs; onToast is stable.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [packKey]);

  useTerrainOverlay(map, overlay, bridge, geoReady && terrainOverlay, onToast);

  return (
    <div className="map-root">
      <div ref={container} className="map-canvas" />
      {map && (
        <MapChrome
          map={map}
          bridge={bridge}
          geoReady={geoReady}
          terrainOverlay={terrainOverlay}
          onTerrainOverlay={onTerrainOverlay}
          credits={[...new Set(packs.flatMap((p) => p.manifest.attribution))]}
          onToast={onToast}
        />
      )}
    </div>
  );
}
