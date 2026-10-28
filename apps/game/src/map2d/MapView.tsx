// 2D strategic map (doc 06 §4, ADR-008): MapLibre GL JS v6 reading PMTiles from OPFS,
// with a deck.gl overlay for game layers. No default MapLibre controls (doc 10 §12);
// the chrome is ours.
import { MapboxOverlay } from '@deck.gl/mapbox';
import { buildAtlas, type IconAtlas } from '@warboard/symbols';
import { addProtocol, Map as MapLibreMap, setWorkerUrl } from 'maplibre-gl';
import workerUrl from 'maplibre-gl/dist/maplibre-gl-worker.mjs?url';
import 'maplibre-gl/dist/maplibre-gl.css';
import { FileSource, PMTiles, Protocol } from 'pmtiles';
import { useEffect, useRef, useState } from 'react';
import type { EngineBridge } from '../bridge';
import { patrolRadiusKm, sendTask } from '../orders';
import { packFile } from '../packs/opfs';
import { fileFor, type InstalledPack } from '../packs/types';
import { ContextMenu } from '../panels/ContextMenu';
import { type Tool, ui } from '../store';
import type { Coalition } from './coalitions';
import { LayerHub } from './layerHub';
import { MapChrome } from './MapChrome';
import { buildStyle, type MapPack } from './style';
import { useTerrainOverlay } from './terrainOverlay';
import { type ContextRequest, UnitLayers } from './units/UnitLayers';

setWorkerUrl(workerUrl);

const protocol = new Protocol();
addProtocol('pmtiles', protocol.tile);

/** One atlas per page, built on first use (ADR-020). */
let atlasPromise: Promise<IconAtlas> | null = null;
const atlas = () => (atlasPromise ??= buildAtlas());

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
  bridge: EngineBridge;
  coalition: Coalition;
  geoReady: boolean;
  terrainOverlay: boolean;
  onTerrainOverlay: (on: boolean) => void;
  onToast: (text: string) => void;
}

export function MapView({ packs, bridge, coalition, geoReady, terrainOverlay, onTerrainOverlay, onToast }: MapViewProps) {
  const container = useRef<HTMLDivElement>(null);
  const [map, setMap] = useState<MapLibreMap | null>(null);
  const [hub, setHub] = useState<LayerHub | null>(null);
  const [menu, setMenu] = useState<ContextRequest | null>(null);
  const packKey = packs.map((p) => `${p.id}@${p.version}`).join();
  const coalitionKey = `${coalition.own.join()}|${coalition.enemy.join()}`;

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
          style: buildStyle(mapPacks, coalition, glyphs),
          center: [18.5, 54.0],
          zoom: 6,
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
        setHub(new LayerHub(deck));
        setMap(created);
        (window as unknown as { __wbMap?: MapLibreMap }).__wbMap = created;
      })
      .catch((err: unknown) => onToast(err instanceof Error ? err.message : String(err)));
    return () => {
      cancelled = true;
      setHub(null);
      setMap(null);
      created?.remove();
    };
    // packKey and coalitionKey stand for packs and coalition; onToast is stable.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [packKey, coalitionKey]);

  useTerrainOverlay(map, hub, bridge, geoReady && terrainOverlay, onToast);

  // Units, rings, routes and tools.
  useEffect(() => {
    if (!map || !hub) return;
    let layers: UnitLayers | null = null;
    let cancelled = false;
    const done = (tool: Tool) => {
      const s = ui.get();
      ui.set({ tool: null });
      const d = s.details;
      if (d?.kind !== 'group') return;
      if (tool.kind === 'route') {
        if (tool.patrol && tool.points.length < 2) {
          onToast('A patrol route needs at least two waypoints');
          return;
        }
        void sendTask(
          bridge,
          [d.id],
          tool.patrol ? { kind: 'patrol_route', waypoints: tool.points, loop: true } : { kind: 'move', waypoints: tool.points },
          onToast,
        );
      } else if (tool.centre) {
        void sendTask(
          bridge,
          [d.id],
          { kind: 'patrol_circle', centre: tool.centre, radius_km: Math.round(tool.radiusKm) || patrolRadiusKm(d.domain) },
          onToast,
        );
      }
    };
    // The label font atlas is built once; build it with the bundled fonts, not a fallback.
    const fonts = ['italic 400 32px "Noto Sans"', '400 32px "Noto Sans"', '600 32px "Noto Sans"', '600 32px "JetBrains Mono"'];
    Promise.all([atlas(), ...fonts.map((f) => document.fonts.load(f))])
      .then(([a]) => {
        if (!cancelled) layers = new UnitLayers(map, hub, bridge, a, setMenu, done);
      })
      .catch((err: unknown) => onToast(`Icons failed to load: ${String(err)}`));
    return () => {
      cancelled = true;
      layers?.dispose();
    };
  }, [map, hub, bridge, onToast]);

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
      {menu && <ContextMenu req={menu} bridge={bridge} toast={onToast} onClose={() => setMenu(null)} />}
    </div>
  );
}
