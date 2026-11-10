// The 3D globe's surface (doc 06 §5, ADR-047): a dark texture drawn in the browser from the
// installed packs' own vector tiles, in the same tokens as the 2D map (land, sea, own and
// enemy territory incl. 12 nm waters), plus the borders as vector polylines. No imagery
// service, no Cesium ion, no new data source: the world pack gives the whole globe and a
// theatre pack a sharper layer over its extent.
import { VectorTile, type VectorTileFeature } from '@mapbox/vector-tile';
import { hex } from '@warboard/symbols';
import { PbfReader } from 'pbf';
import { FileSource, PMTiles } from 'pmtiles';
import type { Coalition } from '../map2d/coalitions';
import { packFile } from '../packs/opfs';
import { fileFor, type InstalledPack } from '../packs/types';

type LonLat = [number, number];

export interface BorderLine {
  path: LonLat[];
  kind: 'land' | 'sea' | 'own' | 'enemy';
}

export interface SurfaceLayer {
  /** [west, south, east, north] degrees. */
  extent: [number, number, number, number];
  canvas: HTMLCanvasElement;
  isWorld: boolean;
}

export interface Surface {
  layers: SurfaceLayer[];
  borders: BorderLine[];
}

/** Texture size: the world at 4096×2048 (≈ 10 km/px), a theatre at ≈ 2.5 km/px. */
const WORLD_PX = 4096;
const THEATRE_PX_PER_DEG = 40;
const THEATRE_MAX_PX = 4096;
/** Tile zoom read per pack: the world at z3 (64 tiles), a theatre at z5 inside its box. */
const WORLD_Z = 3;
const THEATRE_Z = 5;

const isWorld = (p: InstalledPack) => {
  const [w, s, e, n] = p.manifest.extent;
  return w <= -180 && s <= -90 && e >= 180 && n >= 90;
};

/** Tile x/y range of a lon/lat box at zoom z (Web Mercator, clamped to ±85.05°). */
function tileRange([w, s, e, n]: [number, number, number, number], z: number) {
  const k = 2 ** z;
  const tx = (lon: number) => Math.min(k - 1, Math.max(0, Math.floor(((lon + 180) / 360) * k)));
  const ty = (lat: number) => {
    const l = (Math.max(-85.05, Math.min(85.05, lat)) * Math.PI) / 180;
    return Math.min(k - 1, Math.max(0, Math.floor(((1 - Math.log(Math.tan(l) + 1 / Math.cos(l)) / Math.PI) / 2) * k)));
  };
  return { x0: tx(w), x1: tx(e), y0: ty(n), y1: ty(s) };
}

/** Tile-local point → lon/lat. */
function toLonLat(z: number, x: number, y: number, extent: number, px: number, py: number): LonLat {
  const k = 2 ** z;
  const lon = ((x + px / extent) / k) * 360 - 180;
  const yn = Math.PI * (1 - (2 * (y + py / extent)) / k);
  return [lon, (Math.atan(Math.sinh(yn)) * 180) / Math.PI];
}

interface DecodedTile {
  z: number;
  x: number;
  y: number;
  tile: VectorTile;
}

async function readTiles(file: File, box: [number, number, number, number], z: number): Promise<DecodedTile[]> {
  const pm = new PMTiles(new FileSource(file));
  const header = await pm.getHeader();
  const zoom = Math.min(Math.max(z, header.minZoom), header.maxZoom);
  const { x0, x1, y0, y1 } = tileRange(box, zoom);
  const jobs: Promise<DecodedTile | null>[] = [];
  for (let y = y0; y <= y1; y++)
    for (let x = x0; x <= x1; x++)
      jobs.push(
        pm.getZxy(zoom, x, y).then((t) => (t?.data ? { z: zoom, x, y, tile: new VectorTile(new PbfReader(new Uint8Array(t.data))) } : null)),
      );
  return (await Promise.all(jobs)).filter((t): t is DecodedTile => t !== null);
}

interface Feature {
  f: VectorTileFeature;
  /** Rings (polygons) or lines, lon/lat. */
  rings: LonLat[][];
  /** The parts of the outlines this tile owns: segments whose midpoint lies inside the tile
   *  (not in its buffer), so tile seams and duplicates in neighbouring buffers never show. */
  lines: LonLat[][];
}

function features(t: DecodedTile, layer: string): Feature[] {
  const l = t.tile.layers[layer];
  if (!l) return [];
  const out: Feature[] = [];
  const ext = l.extent;
  const owns = (a: { x: number; y: number }, b: { x: number; y: number }) => {
    const mx = (a.x + b.x) / 2;
    const my = (a.y + b.y) / 2;
    return mx >= 0 && mx < ext && my >= 0 && my < ext;
  };
  for (let i = 0; i < l.length; i++) {
    const f = l.feature(i);
    const geom = f.loadGeometry();
    const ll = (p: { x: number; y: number }) => toLonLat(t.z, t.x, t.y, ext, p.x, p.y);
    const rings = geom.map((ring) => ring.map(ll));
    const lines: LonLat[][] = [];
    for (const ring of geom) {
      let cur: LonLat[] = [];
      for (let k = 1; k < ring.length; k++) {
        const a = ring[k - 1];
        const b = ring[k];
        if (a && b && owns(a, b)) {
          if (cur.length === 0) cur.push(ll(a));
          cur.push(ll(b));
        } else if (cur.length > 1) {
          lines.push(cur);
          cur = [];
        } else cur = [];
      }
      if (cur.length > 1) lines.push(cur);
    }
    out.push({ f, rings, lines });
  }
  return out;
}

/** Fills polygon rings on an equirectangular canvas covering `box`. */
function fillRings(ctx: CanvasRenderingContext2D, box: [number, number, number, number], w: number, h: number, rings: LonLat[][]) {
  const [west, south, east, north] = box;
  const sx = w / (east - west);
  const sy = h / (north - south);
  ctx.beginPath();
  for (const ring of rings) {
    ring.forEach(([lon, lat], i) => {
      const x = (lon - west) * sx;
      const y = (north - lat) * sy;
      if (i === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    });
    ctx.closePath();
  }
  ctx.fill('evenodd');
}

async function drawLayer(pack: InstalledPack, coalition: Coalition, borders: BorderLine[]): Promise<SurfaceLayer | null> {
  const world = isWorld(pack);
  const extent: [number, number, number, number] = world ? [-180, -90, 180, 90] : pack.manifest.extent;
  const [west, south, east, north] = extent;
  const w = world ? WORLD_PX : Math.min(THEATRE_MAX_PX, Math.round((east - west) * THEATRE_PX_PER_DEG));
  const h = world ? WORLD_PX / 2 : Math.round((w * (north - south)) / (east - west));
  const basemap = await packFile(pack.id, fileFor(pack.manifest, 'basemap').name);
  const territory = await packFile(pack.id, fileFor(pack.manifest, 'territory_tiles').name);
  if (!basemap || !territory) return null;
  const z = world ? WORLD_Z : THEATRE_Z;
  const [base, terr] = await Promise.all([readTiles(basemap, extent, z), readTiles(territory, extent, z)]);

  const canvas = document.createElement('canvas');
  canvas.width = w;
  canvas.height = h;
  const ctx = canvas.getContext('2d');
  if (!ctx) return null;
  ctx.fillStyle = hex('map.sea');
  ctx.fillRect(0, 0, w, h);
  // Beyond Web Mercator's ±85° the world texture continues the last row of land (polar ice).
  ctx.fillStyle = hex('map.land');
  if (world) ctx.fillRect(0, h * ((90 - -85.05) / 180), w, h);
  for (const t of base) for (const { rings } of features(t, 'earth')) fillRings(ctx, extent, w, h, rings);
  ctx.fillStyle = hex('map.sea');
  for (const t of base)
    for (const { f, rings } of features(t, 'water')) if (f.type === 3) fillRings(ctx, extent, w, h, rings);
  const own = new Set(coalition.own);
  const tinted = new Set([...coalition.own, ...coalition.enemy]);
  for (const t of terr)
    for (const { f, rings, lines } of features(t, 'territory')) {
      const code = String(f.properties.code ?? '');
      if (!tinted.has(code)) continue;
      ctx.fillStyle = hex(own.has(code) ? 'terr.own.fill' : 'terr.enemy.fill');
      fillRings(ctx, extent, w, h, rings);
      for (const line of lines) borders.push({ path: line, kind: own.has(code) ? 'own' : 'enemy' });
    }
  for (const t of terr)
    for (const { f, lines } of features(t, 'borders')) {
      const kind = f.properties.kind === 'sea' ? 'sea' : 'land';
      for (const line of lines) borders.push({ path: line, kind });
    }
  return { extent, canvas, isWorld: world };
}

/** Draws the globe surface of the installed packs, world first. */
export async function buildSurface(packs: InstalledPack[], coalition: Coalition): Promise<Surface> {
  const ordered = [...packs].sort((a, b) => Number(isWorld(b)) - Number(isWorld(a)));
  const layers: SurfaceLayer[] = [];
  let worldBorders: BorderLine[] = [];
  const theatreBorders: BorderLine[] = [];
  for (const p of ordered) {
    const lines: BorderLine[] = [];
    const layer = await drawLayer(p, coalition, lines);
    if (!layer) continue;
    layers.push(layer);
    if (layer.isWorld) worldBorders = lines;
    else theatreBorders.push(...lines);
  }
  // A theatre's borders are sharper than the world's inside its box: the world's are kept
  // only outside every theatre.
  const theatres = layers.filter((l) => !l.isWorld).map((l) => l.extent);
  const inside = ([lon, lat]: LonLat, [w, s, e, n]: [number, number, number, number]) => lon >= w && lon <= e && lat >= s && lat <= n;
  const outside = worldBorders.filter((b) => !theatres.some((box) => b.path.every((p) => inside(p, box))));
  return { layers, borders: [...outside, ...theatreBorders] };
}
