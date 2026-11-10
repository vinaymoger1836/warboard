// The Cesium globe of the 3D view (doc 06 §5): self-hosted, no Cesium ion, no default imagery,
// no widgets (doc 10 §12). Terrain comes from the packs' DEM through the engine worker (which
// holds the DEM files open), the surface is drawn from the packs' vector tiles (basemap.ts),
// borders are polylines.
import { Cartesian3, Color, Rectangle } from '@cesium/core';
import {
  CesiumWidget,
  CustomHeightmapTerrainProvider,
  EllipsoidTerrainProvider,
  GeographicTilingScheme,
  ImageryLayer,
  Ion,
  Material,
  PolylineCollection,
  SingleTileImageryProvider,
} from '@cesium/engine';
import { rgba, type TokenName } from '@warboard/symbols';
import type { EngineBridge } from '../bridge';
import type { Coalition } from '../map2d/coalitions';
import type { InstalledPack } from '../packs/types';
import { type BorderLine, buildSurface } from './basemap';

/** Heightmap samples per terrain tile edge (65 = 64 cells, Cesium's usual). */
const TILE_SAMPLES = 65;

/** A Cesium colour from a token. */
export const colour = (t: TokenName, a = 1) => {
  const [r, g, b, al] = rgba(t, a);
  return new Color(r / 255, g / 255, b / 255, al / 255);
};

// Self-hosted static files (vite.config.ts `cesiumAssets`); set before Cesium resolves a URL.
(globalThis as { CESIUM_BASE_URL?: string }).CESIUM_BASE_URL = new URL('cesium/', document.baseURI).href;
// No Cesium ion: no token, so nothing can reach the service.
Ion.defaultAccessToken = '';

export function createWidget(container: HTMLElement, bridge: EngineBridge, hasDem: boolean): CesiumWidget {
  const tilingScheme = new GeographicTilingScheme();
  const terrainProvider = hasDem
    ? new CustomHeightmapTerrainProvider({
        width: TILE_SAMPLES,
        height: TILE_SAMPLES,
        tilingScheme,
        callback: (x, y, level) => {
          const r = tilingScheme.tileXYToRectangle(x, y, level);
          const deg = (v: number) => (v * 180) / Math.PI;
          return bridge
            .request({ type: 'heightTile', bbox: [deg(r.west), deg(r.south), deg(r.east), deg(r.north)], size: TILE_SAMPLES })
            .then((h) => h ?? new Float32Array(TILE_SAMPLES * TILE_SAMPLES));
        },
      })
    : new EllipsoidTerrainProvider();
  // Credits are on the app's credits panel; Cesium's own display goes into a detached node.
  const credits = document.createElement('div');
  const widget = new CesiumWidget(container, {
    baseLayer: false,
    terrainProvider,
    skyBox: false,
    skyAtmosphere: false,
    creditContainer: credits,
    showRenderLoopErrors: false,
    scene3DOnly: true,
    shouldAnimate: false,
    msaaSamples: 4,
  });
  const scene = widget.scene;
  scene.backgroundColor = colour('map.land');
  scene.globe.baseColor = colour('map.sea');
  scene.globe.showGroundAtmosphere = false;
  scene.globe.enableLighting = false;
  scene.globe.depthTestAgainstTerrain = false;
  scene.fog.enabled = false;
  if (scene.sun) scene.sun.show = false;
  if (scene.moon) scene.moon.show = false;
  return widget;
}

const BORDER_STYLE: Record<BorderLine['kind'], { token: TokenName; width: number; dashed: boolean }> = {
  land: { token: 'map.border', width: 1.2, dashed: false },
  sea: { token: 'map.border', width: 1, dashed: true },
  own: { token: 'terr.own.line', width: 1.6, dashed: false },
  enemy: { token: 'terr.enemy.line', width: 1.6, dashed: false },
};

/** Draws the packs' surface and borders on the globe; returns a cleanup. */
export async function addSurface(widget: CesiumWidget, packs: InstalledPack[], coalition: Coalition): Promise<() => void> {
  const surface = await buildSurface(packs, coalition);
  if (widget.isDestroyed()) return () => undefined;
  const urls: string[] = [];
  const layers: ImageryLayer[] = [];
  for (const l of surface.layers) {
    const blob = await new Promise<Blob | null>((resolve) => l.canvas.toBlob(resolve, 'image/png'));
    if (!blob || widget.isDestroyed()) continue;
    const url = URL.createObjectURL(blob);
    urls.push(url);
    const [w, s, e, n] = l.extent;
    const provider = new SingleTileImageryProvider({
      url,
      rectangle: Rectangle.fromDegrees(w, s, e, n),
      tileWidth: l.canvas.width,
      tileHeight: l.canvas.height,
    });
    const layer = new ImageryLayer(provider, {});
    widget.scene.imageryLayers.add(layer);
    layers.push(layer);
  }
  const lines = new PolylineCollection();
  for (const b of surface.borders) {
    const st = BORDER_STYLE[b.kind];
    lines.add({
      positions: Cartesian3.fromDegreesArray(b.path.flat()),
      width: st.width,
      material: st.dashed
        ? Material.fromType('PolylineDash', { color: colour(st.token), dashLength: 10 })
        : Material.fromType('Color', { color: colour(st.token) }),
    });
  }
  widget.scene.primitives.add(lines);
  return () => {
    if (!widget.isDestroyed()) {
      for (const l of layers) widget.scene.imageryLayers.remove(l, true);
      widget.scene.primitives.remove(lines);
    }
    for (const u of urls) URL.revokeObjectURL(u);
  };
}
