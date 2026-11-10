// Ground tracks and orbits of own satellites (doc 10 §4, §11), fetched from the engine (SGP4
// lives there) and cached for both views. Points carry altitude, so the 3D view can draw the
// orbit arc; the 2D map drops it.
import type { EngineBridge } from '../bridge';
import { splitAntimeridian } from './scene';
import type { LonLatAlt } from './weapons';

/** Tracks are refreshed this often (real ms), ±minutes around now. */
const TRACK_REFRESH_MS = 30_000;
const TRACK_BEFORE_MIN = 45;
const TRACK_AFTER_MIN = 90;

interface Entry {
  paths: LonLatAlt[][];
  at: number;
  pending: boolean;
}

const cache = new WeakMap<EngineBridge, Map<number, Entry>>();

/** The cached track of satellite `id`, split at the antimeridian; refreshes in the background. */
export function satelliteTrack(bridge: EngineBridge, id: number): LonLatAlt[][] {
  let tracks = cache.get(bridge);
  if (!tracks) {
    tracks = new Map();
    cache.set(bridge, tracks);
  }
  const now = performance.now();
  const t = tracks.get(id);
  if (!t || (!t.pending && now - t.at > TRACK_REFRESH_MS)) {
    const entry: Entry = { paths: t?.paths ?? [], at: now, pending: true };
    tracks.set(id, entry);
    bridge
      .request({ type: 'groundTrack', id, beforeMin: TRACK_BEFORE_MIN, afterMin: TRACK_AFTER_MIN })
      .then((pts) => {
        const full = pts.map((p) => [p[0] ?? 0, p[1] ?? 0, p[2] ?? 0] as LonLatAlt);
        entry.paths = splitAntimeridian(full);
        entry.pending = false;
      })
      .catch(() => {
        entry.pending = false;
      });
  }
  return tracks.get(id)?.paths ?? [];
}
