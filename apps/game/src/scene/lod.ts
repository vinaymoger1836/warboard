// Level of detail shared by both views (doc 06 §5): the 3D camera's height is turned into the
// 2D map's zoom, so the same thresholds (bases from z5, flight members from z11, tags from z7)
// decide what is drawn and selectable in either view, and a 2D↔3D switch keeps the scale.

const EARTH_CIRCUMFERENCE_M = 40_075_016.686;
/** MapLibre's tile size: metres per CSS px at zoom z is C·cos(lat) / (512 · 2^z). */
const TILE_PX = 512;

/** Metres per CSS px on the ground at the screen centre for a camera `heightM` above it
 *  looking straight down with vertical field of view `fovY` (rad) on `screenPx` px. */
const metresPerPx = (heightM: number, fovY: number, screenPx: number) => (2 * heightM * Math.tan(fovY / 2)) / screenPx;

/** The 2D zoom that shows the ground at the same scale as the 3D camera. */
export function zoomForHeight(heightM: number, latDeg: number, fovY: number, screenPx: number): number {
  const m = Math.max(metresPerPx(Math.max(heightM, 1), fovY, screenPx), 1e-6);
  const z = Math.log2((EARTH_CIRCUMFERENCE_M * Math.cos((latDeg * Math.PI) / 180)) / (TILE_PX * m));
  return Math.min(22, Math.max(0, z));
}

/** Inverse of `zoomForHeight`: the camera height for a 2D zoom. */
export function heightForZoom(zoom: number, latDeg: number, fovY: number, screenPx: number): number {
  const m = (EARTH_CIRCUMFERENCE_M * Math.cos((latDeg * Math.PI) / 180)) / (TILE_PX * 2 ** zoom);
  return (m * screenPx) / (2 * Math.tan(fovY / 2));
}

/** Web Mercator metres per pixel at a zoom and latitude (tag merging, doc 10 §5). */
export const mercatorMetresPerPx = (zoom: number, latDeg: number) =>
  (156_543 * Math.cos((latDeg * Math.PI) / 180)) / 2 ** zoom;
