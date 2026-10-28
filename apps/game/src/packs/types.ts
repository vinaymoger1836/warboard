// Pack catalogue and manifest, as written by geodata/manifest.py (doc 05 §2.1).

export type FileRole = 'basemap' | 'territory_tiles' | 'dem' | 'terrain' | 'territory';

export interface PackFileEntry {
  name: string;
  role: FileRole;
  size: number;
  blake3: string;
}

export interface PackSource {
  id: string;
  name: string;
  version: string;
  url: string;
  licence: string;
  accessed: string;
}

export interface PackManifest {
  format: number;
  id: string;
  name: string;
  version: string;
  /** [west, south, east, north] degrees. */
  extent: [number, number, number, number];
  basemap_zoom: [number, number];
  dem_res_arcsec: number;
  h3_res: number;
  files: PackFileEntry[];
  size: number;
  sources: PackSource[];
  attribution: string[];
  notes: { estimated_territorial_seas: string[]; terrain_classes: Record<string, number> };
}

export interface CatalogEntry {
  id: string;
  name: string;
  version: string;
  size: number;
  manifest: string;
  manifest_blake3: string;
}

export interface Catalog {
  format: number;
  packs: CatalogEntry[];
}

/** What the registry remembers about an installed pack. */
export interface InstalledPack {
  id: string;
  version: string;
  manifest: PackManifest;
}

export type PackStatus =
  | { kind: 'available' }
  | { kind: 'installing'; file: string; done: number; total: number }
  | { kind: 'installed' }
  | { kind: 'update'; from: string }
  | { kind: 'evicted' }
  | { kind: 'error'; message: string };

export function fileFor(manifest: PackManifest, role: FileRole): PackFileEntry {
  const f = manifest.files.find((x) => x.role === role);
  if (!f) throw new Error(`Pack ${manifest.id} has no ${role} file`);
  return f;
}
