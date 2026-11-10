// Small spherical helpers for drawing (UI only; the engine has its own, deterministic ones).

const R = 6_371_008.8;
const RAD = Math.PI / 180;

export type LonLat = [number, number];

/** Point `distM` from `[lon, lat]` along `bearingDeg`. */
export function destination([lon, lat]: LonLat, bearingDeg: number, distM: number): LonLat {
  const d = distM / R;
  const b = bearingDeg * RAD;
  const p1 = lat * RAD;
  const p2 = Math.asin(Math.sin(p1) * Math.cos(d) + Math.cos(p1) * Math.sin(d) * Math.cos(b));
  const l2 = lon * RAD + Math.atan2(Math.sin(b) * Math.sin(d) * Math.cos(p1), Math.cos(d) - Math.sin(p1) * Math.sin(p2));
  return [((((l2 / RAD + 540) % 360) + 360) % 360) - 180, p2 / RAD];
}

/** Great-circle distance, metres. */
export function distance([lon1, lat1]: LonLat, [lon2, lat2]: LonLat): number {
  const dp = (lat2 - lat1) * RAD;
  const dl = (lon2 - lon1) * RAD;
  const a = Math.sin(dp / 2) ** 2 + Math.cos(lat1 * RAD) * Math.cos(lat2 * RAD) * Math.sin(dl / 2) ** 2;
  return 2 * R * Math.asin(Math.min(1, Math.sqrt(a)));
}

/** A true geodesic circle (doc 10 §6: not a screen circle), closed. */
export function geodesicCircle(centre: LonLat, radiusM: number, steps = 128): LonLat[] {
  const out: LonLat[] = [];
  for (let i = 0; i <= steps; i++) out.push(destination(centre, (360 * i) / steps, radiusM));
  return out;
}
