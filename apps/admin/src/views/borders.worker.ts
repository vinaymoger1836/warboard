// Border preview (doc 04 §5): samples the theatre's region pack on a grid (terrain class
// and the built owner), then grows regions from the edited seeds and weights the way
// ADR-042 does: one queue for all nations, step cost = distance × terrain factor, key =
// cost ÷ weight, a nation stops at reach_km × weight. No noise, smoothing or merging, so
// it is a preview; `geodata/run.sh build` makes the real borders.
import init, { GeoQuery } from '@warboard/wb-wasm';
import wasmUrl from '@warboard/wb-wasm/wb_wasm_bg.wasm?url';

export interface Seed {
  nation: string;
  lon: number;
  lat: number;
  weight: number;
}

export type ToBorders =
  | { type: 'sample'; files: { dem: File; terrain: File; territory: File } | null; extent: number[]; cols: number; rows: number }
  | { type: 'grow'; seeds: Seed[]; reachKm: number };

export type FromBorders =
  | { type: 'progress'; text: string }
  | { type: 'sampled'; cols: number; rows: number; land: Uint8Array; built: Int16Array; builtCodes: string[]; hasPack: boolean }
  | { type: 'grown'; owner: Int16Array }
  | { type: 'error'; text: string };

/** Terrain factors of ADR-042 by class code (doc 05 §3 order). */
const FACTOR = [5, 1, 1.25, 1.25, 1, 1, 3.5, 2, 2];

let cols = 0;
let rows = 0;
let extent = [0, 0, 1, 1];
let cls = new Uint8Array(0);

const post = (m: FromBorders) => self.postMessage(m);

function packFile(name: string, file: File) {
  const reader = new FileReaderSync();
  return {
    name,
    size: () => file.size,
    readAt: (buf: Uint8Array, at: number) => {
      const ab = reader.readAsArrayBuffer(file.slice(at, at + buf.length));
      buf.set(new Uint8Array(ab));
      return ab.byteLength;
    },
  };
}

function lonOf(c: number): number {
  return extent[0]! + ((c + 0.5) / cols) * (extent[2]! - extent[0]!);
}
function latOf(r: number): number {
  return extent[3]! - ((r + 0.5) / rows) * (extent[3]! - extent[1]!);
}

async function sample(msg: Extract<ToBorders, { type: 'sample' }>): Promise<void> {
  await init({ module_or_path: wasmUrl });
  ({ cols, rows, extent } = msg);
  cls = new Uint8Array(cols * rows).fill(1);
  const built = new Int16Array(cols * rows).fill(-1);
  const codes: string[] = [];
  let hasPack = false;
  if (msg.files) {
    const geo = new GeoQuery();
    geo.add_pack('europe', packFile('dem', msg.files.dem), packFile('terrain', msg.files.terrain), packFile('territory', msg.files.territory));
    hasPack = true;
    for (let r = 0; r < rows; r++) {
      if (r % 24 === 0) post({ type: 'progress', text: `Reading terrain ${Math.round((100 * r) / rows)}%` });
      for (let c = 0; c < cols; c++) {
        const p = geo.point(latOf(r), lonOf(c));
        const i = r * cols + c;
        if (p.length < 2) {
          cls[i] = 0;
          continue;
        }
        cls[i] = p[1]!;
        if (p.length > 2) {
          const code = String.fromCharCode(...Array.from(p.slice(2)));
          let k = codes.indexOf(code);
          if (k < 0) k = codes.push(code) - 1;
          built[i] = k;
        }
      }
    }
    geo.free();
  }
  const land = new Uint8Array(cols * rows);
  for (let i = 0; i < land.length; i++) land[i] = cls[i] === 0 ? 0 : 1;
  post({ type: 'sampled', cols, rows, land, built, builtCodes: codes, hasPack });
}

/** Binary heap of [key, cell, nation, cost]. */
class Heap {
  private a: [number, number, number, number][] = [];
  get size(): number {
    return this.a.length;
  }
  push(x: [number, number, number, number]): void {
    const a = this.a;
    a.push(x);
    let i = a.length - 1;
    while (i > 0) {
      const p = (i - 1) >> 1;
      if (a[p]![0] <= a[i]![0]) break;
      [a[p], a[i]] = [a[i]!, a[p]!];
      i = p;
    }
  }
  pop(): [number, number, number, number] {
    const a = this.a;
    const top = a[0]!;
    const last = a.pop()!;
    if (a.length) {
      a[0] = last;
      let i = 0;
      for (;;) {
        const l = 2 * i + 1;
        const r = l + 1;
        let m = i;
        if (l < a.length && a[l]![0] < a[m]![0]) m = l;
        if (r < a.length && a[r]![0] < a[m]![0]) m = r;
        if (m === i) break;
        [a[m], a[i]] = [a[i]!, a[m]!];
        i = m;
      }
    }
    return top;
  }
}

function grow(seeds: Seed[], reachKm: number): Int16Array {
  const owner = new Int16Array(cols * rows).fill(-1);
  const heap = new Heap();
  const dLon = (extent[2]! - extent[0]!) / cols;
  const dLat = (extent[3]! - extent[1]!) / rows;
  seeds.forEach((s, k) => {
    const c = Math.floor((s.lon - extent[0]!) / dLon);
    const r = Math.floor((extent[3]! - s.lat) / dLat);
    if (c >= 0 && c < cols && r >= 0 && r < rows) heap.push([0, r * cols + c, k, 0]);
  });
  const done = new Uint8Array(cols * rows);
  while (heap.size) {
    const [, i, k, cost] = heap.pop();
    if (done[i]) continue;
    done[i] = 1;
    if (cls[i] !== 0) owner[i] = k;
    const r = Math.floor(i / cols);
    const c = i % cols;
    const kmLat = dLat * 111.2;
    const kmLon = dLon * 111.2 * Math.cos((latOf(r) * Math.PI) / 180);
    const w = seeds[k]!.weight;
    for (let dr = -1; dr <= 1; dr++) {
      for (let dc = -1; dc <= 1; dc++) {
        if (!dr && !dc) continue;
        const rr = r + dr;
        const cc = c + dc;
        if (rr < 0 || rr >= rows || cc < 0 || cc >= cols) continue;
        const j = rr * cols + cc;
        if (done[j]) continue;
        const dist = Math.hypot(dr * kmLat, dc * kmLon);
        const step = dist * ((FACTOR[cls[i]!] ?? 1) + (FACTOR[cls[j]!] ?? 1)) / 2;
        const nc = cost + step;
        if (nc > reachKm * w) continue;
        heap.push([nc / w, j, k, nc]);
      }
    }
  }
  return owner;
}

self.onmessage = (e: MessageEvent<ToBorders>) => {
  const m = e.data;
  if (m.type === 'sample') {
    sample(m).catch((err: unknown) => post({ type: 'error', text: String(err) }));
  } else {
    const owner = grow(m.seeds, m.reachKm);
    post({ type: 'grown', owner });
  }
};
