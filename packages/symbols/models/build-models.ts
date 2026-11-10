// Builds the low-poly class models of the 3D view (doc 06 §5, ADR-047) as glTF 2.0 binaries,
// without dependencies: a few primitives (box, lathe, extruded plate) are combined into
// generic class shapes, flat-shaded, with a grey shade per part. The 3D view tints them with
// the side colour. Shapes are generic silhouettes of a class, never of a real design.
//
//   node packages/symbols/models/build-models.ts [outDir]   (run by `pnpm gen`)
//
// Convention (see src/models.ts): nose along +X, up +Y, length 1 (x from −0.5 to 0.5).
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { MODEL_NAMES, type ModelName } from '../src/models.ts';

type V = [number, number, number];
interface Tri {
  a: V;
  b: V;
  c: V;
  shade: number;
}
type Part = Tri[];

const HULL = 0.62;
const DARK = 0.32;
const LIGHT = 0.78;
const GLASS = 0.18;

// ---- Primitives -----------------------------------------------------------------------

function quad(a: V, b: V, c: V, d: V, shade: number): Part {
  return [
    { a, b, c, shade },
    { a, b: c, c: d, shade },
  ];
}

/** Axis-aligned box centred at `c` with size `s`. */
function box(c: V, s: V, shade = HULL): Part {
  const [x, y, z] = c;
  const [hx, hy, hz] = [s[0] / 2, s[1] / 2, s[2] / 2];
  const p = (i: number, j: number, k: number): V => [x + i * hx, y + j * hy, z + k * hz];
  return [
    ...quad(p(-1, -1, -1), p(1, -1, -1), p(1, 1, -1), p(-1, 1, -1), shade),
    ...quad(p(-1, -1, 1), p(-1, 1, 1), p(1, 1, 1), p(1, -1, 1), shade),
    ...quad(p(-1, -1, -1), p(-1, 1, -1), p(-1, 1, 1), p(-1, -1, 1), shade),
    ...quad(p(1, -1, -1), p(1, -1, 1), p(1, 1, 1), p(1, 1, -1), shade),
    ...quad(p(-1, 1, -1), p(1, 1, -1), p(1, 1, 1), p(-1, 1, 1), shade),
    ...quad(p(-1, -1, -1), p(-1, -1, 1), p(1, -1, 1), p(1, -1, -1), shade),
  ];
}

/** A body of revolution along X: `profile` is [x, radius] from tail to nose. */
function lathe(profile: [number, number][], centre: [number, number] = [0, 0], shade = HULL, seg = 10, squashY = 1): Part {
  const [cy, cz] = centre;
  const ring = (x: number, r: number): V[] =>
    Array.from({ length: seg }, (_, i) => {
      const t = (2 * Math.PI * i) / seg;
      return [x, cy + r * Math.cos(t) * squashY, cz + r * Math.sin(t)] as V;
    });
  const out: Part = [];
  for (let k = 1; k < profile.length; k++) {
    const [x0, r0] = profile[k - 1] as [number, number];
    const [x1, r1] = profile[k] as [number, number];
    const a = ring(x0, r0);
    const b = ring(x1, r1);
    for (let i = 0; i < seg; i++) {
      const j = (i + 1) % seg;
      out.push(...quad(a[i] as V, a[j] as V, b[j] as V, b[i] as V, shade));
    }
  }
  const cap = (x: number, r: number) => {
    if (r <= 0) return;
    const pts = ring(x, r);
    for (let i = 1; i < seg - 1; i++) out.push({ a: pts[0] as V, b: pts[i] as V, c: pts[i + 1] as V, shade });
  };
  const first = profile[0];
  const last = profile[profile.length - 1];
  if (first) cap(first[0], first[1]);
  if (last) cap(last[0], last[1]);
  return out;
}

/** A flat convex plate: `outline` in the plane given by `axes`, extruded `t` along the third. */
function plate(outline: [number, number][], plane: 'xz' | 'xy' | 'yz', offset: number, t: number, shade = HULL): Part {
  const put = (u: number, v: number, w: number): V =>
    plane === 'xz' ? [u, w, v] : plane === 'xy' ? [u, v, w] : [w, u, v];
  const top = outline.map(([u, v]) => put(u, v, offset + t / 2));
  const bot = outline.map(([u, v]) => put(u, v, offset - t / 2));
  const out: Part = [];
  for (let i = 1; i < outline.length - 1; i++) {
    out.push({ a: top[0] as V, b: top[i] as V, c: top[i + 1] as V, shade });
    out.push({ a: bot[0] as V, b: bot[i + 1] as V, c: bot[i] as V, shade });
  }
  for (let i = 0; i < outline.length; i++) {
    const j = (i + 1) % outline.length;
    out.push(...quad(bot[i] as V, bot[j] as V, top[j] as V, top[i] as V, shade));
  }
  return out;
}

/** Mirror of a part across the XY plane (left/right). */
const mirror = (p: Part): Part => p.map((t) => ({ a: [t.a[0], t.a[1], -t.a[2]], b: [t.b[0], t.b[1], -t.b[2]], c: [t.c[0], t.c[1], -t.c[2]], shade: t.shade }));
const both = (p: Part): Part => [...p, ...mirror(p)];

/** Rotates a part about the Z axis (pitch) around `pivot`. */
function pitch(p: Part, deg: number, pivot: V): Part {
  const r = (deg * Math.PI) / 180;
  const [cs, sn] = [Math.cos(r), Math.sin(r)];
  const f = ([x, y, z]: V): V => {
    const dx = x - pivot[0];
    const dy = y - pivot[1];
    return [pivot[0] + dx * cs - dy * sn, pivot[1] + dx * sn + dy * cs, z];
  };
  return p.map((t) => ({ a: f(t.a), b: f(t.b), c: f(t.c), shade: t.shade }));
}

/** A flat disc in the XZ plane (rotor, dome). */
function disc(c: V, r: number, t: number, shade: number, seg = 16): Part {
  return plate(
    Array.from({ length: seg }, (_, i) => [c[0] + r * Math.cos((2 * Math.PI * i) / seg), c[2] + r * Math.sin((2 * Math.PI * i) / seg)] as [number, number]),
    'xz',
    c[1],
    t,
    shade,
  );
}

// ---- Class shapes ---------------------------------------------------------------------

function fighter(straight = false): Part {
  const body = lathe([[-0.5, 0.035], [-0.3, 0.05], [0.15, 0.05], [0.38, 0.03], [0.5, 0]]);
  const canopy = lathe([[0.12, 0], [0.2, 0.028], [0.3, 0]], [0.04, 0], GLASS, 8);
  const wing: [number, number][] = straight
    ? [[0.05, 0.04], [0.15, 0.04], [0.08, 0.42], [-0.02, 0.42]]
    : [[0.2, 0.04], [-0.25, 0.4], [-0.32, 0.4], [-0.3, 0.04]];
  const tail: [number, number][] = [[-0.36, 0.04], [-0.48, 0.18], [-0.5, 0.18], [-0.5, 0.04]];
  const fin: [number, number][] = [[-0.28, 0.03], [-0.44, 0.2], [-0.5, 0.2], [-0.5, 0.03]];
  return [
    ...body,
    ...canopy,
    ...both(plate(wing, 'xz', 0, 0.015)),
    ...both(plate(tail, 'xz', 0, 0.012)),
    ...(straight ? plate(fin, 'xy', 0, 0.012) : both(plate(fin, 'xy', 0.06, 0.012))),
  ];
}

function bomber(): Part {
  return [
    ...lathe([[-0.5, 0.025], [-0.3, 0.045], [0.25, 0.045], [0.45, 0.03], [0.5, 0]]),
    ...both(plate([[0.15, 0.04], [-0.2, 0.5], [-0.27, 0.5], [-0.08, 0.04]], 'xz', 0, 0.018)),
    ...both(plate([[-0.38, 0.03], [-0.47, 0.17], [-0.5, 0.17], [-0.5, 0.03]], 'xz', 0.02, 0.012)),
    ...plate([[-0.35, 0.03], [-0.47, 0.17], [-0.5, 0.17], [-0.5, 0.03]], 'xy', 0, 0.012),
    ...both(lathe([[-0.05, 0.022], [0.06, 0.022]], [-0.03, 0.2], DARK, 8)),
  ];
}

function transport(dome = false): Part {
  return [
    ...lathe([[-0.5, 0.02], [-0.35, 0.06], [0.35, 0.06], [0.47, 0.035], [0.5, 0]]),
    ...both(plate([[0.08, 0.05], [0.02, 0.5], [-0.05, 0.5], [-0.08, 0.05]], 'xz', 0.03, 0.016)),
    ...both(plate([[-0.4, 0.02], [-0.47, 0.18], [-0.5, 0.18], [-0.5, 0.02]], 'xz', 0.14, 0.012)),
    ...plate([[-0.36, 0.04], [-0.46, 0.16], [-0.5, 0.16], [-0.5, 0.04]], 'xy', 0, 0.012),
    ...both(lathe([[-0.02, 0.025], [0.1, 0.025]], [0.0, 0.2], DARK, 8)),
    ...(dome ? [...disc([-0.12, 0.16, 0], 0.13, 0.035, DARK), ...box([-0.12, 0.1, 0], [0.04, 0.08, 0.02])] : []),
  ];
}

function drone(): Part {
  return [
    ...lathe([[-0.5, 0.02], [-0.35, 0.04], [0.3, 0.05], [0.5, 0]]),
    ...both(plate([[0.06, 0.03], [0.04, 0.5], [-0.04, 0.5], [-0.06, 0.03]], 'xz', 0.02, 0.012)),
    ...both(plate([[-0.38, 0.02], [-0.47, 0.13], [-0.5, 0.13], [-0.5, 0.02]], 'xy', 0.02, 0.01)),
  ];
}

function helicopter(attack: boolean): Part {
  const w = attack ? 0.05 : 0.09;
  return [
    ...lathe(
      [[-0.5, 0.015], [-0.1, 0.03], [0.05, w], [0.3, w], [0.45, w * 0.6], [0.5, 0]],
      [0, 0],
      HULL,
      10,
      attack ? 1.6 : 1.3,
    ),
    ...lathe([[0.2, 0], [0.32, w * 0.8], [0.44, 0]], [w * 0.6, 0], GLASS, 8),
    ...disc([0.12, w * 1.6 + 0.04, 0], 0.45, 0.006, DARK, 20),
    ...box([0.12, w * 1.3 + 0.02, 0], [0.04, 0.06, 0.04], DARK),
    ...plate([[-0.42, 0.0], [-0.5, 0.12], [-0.52, 0.12], [-0.5, 0.0]], 'xy', 0, 0.01),
    ...(attack ? both(plate([[0.15, 0.04], [0.15, 0.16], [0.08, 0.16], [0.08, 0.04]], 'xz', 0, 0.012)) : []),
  ];
}

/** A hull with a pointed bow, deck at y = 0, `beam` wide, `draught` deep. */
function hull(beam: number, freeboard: number): Part {
  const b = beam / 2;
  const outline: [number, number][] = [[0.5, 0], [0.32, b], [-0.46, b], [-0.5, b * 0.8], [-0.5, -b * 0.8], [-0.46, -b], [0.32, -b]];
  return plate(outline, 'xz', -freeboard / 2, freeboard, HULL);
}

function ship(size: 'large' | 'medium' | 'small' | 'auxiliary'): Part {
  const beam = size === 'auxiliary' ? 0.16 : size === 'small' ? 0.15 : 0.12;
  const out: Part = [...hull(beam, 0.05)];
  if (size === 'auxiliary') {
    out.push(...box([-0.38, 0.05, 0], [0.12, 0.1, beam * 0.8], LIGHT), ...box([0.0, 0.025, 0], [0.5, 0.05, beam * 0.6], DARK));
    return out;
  }
  const s = size === 'large' ? 1 : size === 'medium' ? 0.85 : 0.7;
  out.push(
    ...box([0.08, 0.04 * s, 0], [0.22, 0.08 * s, beam * 0.75], LIGHT),
    ...box([-0.12, 0.035 * s, 0], [0.16, 0.07 * s, beam * 0.7], LIGHT),
    ...box([0.08, 0.12 * s, 0], [0.03, 0.12 * s, 0.02], DARK),
    ...box([0.3, 0.015, 0], [0.06, 0.03, beam * 0.4], DARK),
    ...lathe([[0.3, 0.012], [0.4, 0.008]], [0.03, 0], DARK, 6),
  );
  if (size !== 'small') out.push(...box([-0.32, 0.01, 0], [0.18, 0.02, beam * 0.85], DARK));
  return out;
}

function submarine(): Part {
  return [
    ...lathe([[-0.5, 0], [-0.45, 0.03], [-0.3, 0.06], [0.35, 0.06], [0.48, 0.035], [0.5, 0]]),
    ...plate([[0.18, 0.05], [0.24, 0.14], [0.08, 0.14], [0.06, 0.05]], 'xy', 0, 0.025, DARK),
    ...both(plate([[-0.42, 0.0], [-0.48, 0.1], [-0.5, 0.1], [-0.49, 0.0]], 'xz', 0, 0.01, DARK)),
    ...plate([[-0.42, 0.0], [-0.48, 0.1], [-0.5, 0.1], [-0.49, 0.0]], 'xy', 0, 0.01, DARK),
  ];
}

/** A wheeled or tracked vehicle chassis. */
const chassis = (h = 0.16, w = 0.3): Part => [...box([0, h / 2 + 0.05, 0], [1, h, w]), ...both(box([0, 0.05, w / 2], [0.92, 0.1, 0.04], DARK))];

function vehicle(kind: 'radar' | 'launcher' | 'c2' | 'tank' | 'artillery' | 'infantry'): Part {
  switch (kind) {
    case 'radar':
      return [...chassis(), ...box([0.36, 0.3, 0], [0.22, 0.14, 0.28], LIGHT), ...pitch(box([-0.12, 0.42, 0], [0.04, 0.36, 0.46], DARK), -20, [-0.12, 0.25, 0])];
    case 'launcher':
      return [...chassis(), ...box([0.38, 0.3, 0], [0.2, 0.14, 0.28], LIGHT), ...pitch(box([-0.12, 0.3, 0], [0.62, 0.16, 0.26], DARK), 35, [-0.4, 0.25, 0])];
    case 'c2':
      return [...chassis(), ...box([0.36, 0.3, 0], [0.22, 0.14, 0.28], LIGHT), ...box([-0.12, 0.36, 0], [0.62, 0.24, 0.3], LIGHT), ...box([-0.3, 0.55, 0], [0.02, 0.18, 0.02], DARK)];
    case 'tank':
      return [
        ...chassis(0.2, 0.42),
        ...box([-0.05, 0.36, 0], [0.45, 0.13, 0.34], LIGHT),
        ...lathe([[0.15, 0.022], [0.62, 0.018]], [0.37, 0], DARK, 8),
      ];
    case 'artillery':
      return [
        ...chassis(0.2, 0.38),
        ...box([-0.1, 0.38, 0], [0.4, 0.16, 0.34], LIGHT),
        ...pitch(lathe([[0.1, 0.022], [0.62, 0.018]], [0.4, 0], DARK, 8), 25, [0.1, 0.4, 0]),
      ];
    default:
      return [...chassis(0.24, 0.36), ...box([0.25, 0.36, 0], [0.2, 0.08, 0.3], DARK)];
  }
}

function satellite(): Part {
  return [
    ...box([0, 0, 0], [0.3, 0.3, 0.3], LIGHT),
    ...both(plate([[0.12, 0.2], [0.12, 0.5], [-0.12, 0.5], [-0.12, 0.2]], 'xz', 0, 0.01, GLASS)),
    ...both(box([0, 0, 0.17], [0.02, 0.02, 0.06], DARK)),
    ...lathe([[0.15, 0.04], [0.26, 0.11]], [0, 0], DARK, 12),
  ];
}

function missile(kind: 'missile' | 'bomb' | 'torpedo'): Part {
  const r = kind === 'bomb' ? 0.09 : kind === 'torpedo' ? 0.05 : 0.04;
  const body = lathe(
    kind === 'bomb'
      ? [[-0.5, 0.03], [-0.3, r], [0.3, r], [0.5, 0]]
      : [[-0.5, r * 0.8], [-0.45, r], [0.38, r], [0.5, 0]],
    [0, 0],
    kind === 'missile' ? LIGHT : HULL,
    10,
  );
  const fin: [number, number][] = [[-0.3, r * 0.6], [-0.48, r * 3.2], [-0.5, r * 3.2], [-0.5, r * 0.6]];
  const fins = [...both(plate(fin, 'xz', 0, 0.01, DARK)), ...plate(fin, 'xy', 0, 0.01, DARK), ...plate(fin.map(([u, v]) => [u, -v] as [number, number]), 'xy', 0, 0.01, DARK)];
  return [...body, ...fins];
}

const SHAPES: Record<ModelName, () => Part> = {
  fighter: () => fighter(false),
  attack: () => fighter(true),
  bomber,
  transport: () => transport(false),
  aew: () => transport(true),
  drone,
  'helicopter-attack': () => helicopter(true),
  'helicopter-utility': () => helicopter(false),
  'ship-large': () => ship('large'),
  'ship-medium': () => ship('medium'),
  'ship-small': () => ship('small'),
  auxiliary: () => ship('auxiliary'),
  submarine,
  'radar-vehicle': () => vehicle('radar'),
  'launcher-vehicle': () => vehicle('launcher'),
  'c2-vehicle': () => vehicle('c2'),
  tank: () => vehicle('tank'),
  artillery: () => vehicle('artillery'),
  'infantry-vehicle': () => vehicle('infantry'),
  satellite,
  missile: () => missile('missile'),
  bomb: () => missile('bomb'),
  torpedo: () => missile('torpedo'),
};

// ---- glTF writer ----------------------------------------------------------------------

function glb(part: Part, name: string): Buffer {
  const n = part.length * 3;
  const pos = new Float32Array(n * 3);
  const nor = new Float32Array(n * 3);
  const col = new Float32Array(n * 3);
  const min = [Infinity, Infinity, Infinity];
  const max = [-Infinity, -Infinity, -Infinity];
  let k = 0;
  for (const t of part) {
    const u = [t.b[0] - t.a[0], t.b[1] - t.a[1], t.b[2] - t.a[2]];
    const v = [t.c[0] - t.a[0], t.c[1] - t.a[1], t.c[2] - t.a[2]];
    let nx = (u[1] ?? 0) * (v[2] ?? 0) - (u[2] ?? 0) * (v[1] ?? 0);
    let ny = (u[2] ?? 0) * (v[0] ?? 0) - (u[0] ?? 0) * (v[2] ?? 0);
    let nz = (u[0] ?? 0) * (v[1] ?? 0) - (u[1] ?? 0) * (v[0] ?? 0);
    const len = Math.hypot(nx, ny, nz) || 1;
    [nx, ny, nz] = [nx / len, ny / len, nz / len];
    for (const p of [t.a, t.b, t.c]) {
      for (let i = 0; i < 3; i++) {
        pos[k * 3 + i] = p[i] as number;
        min[i] = Math.min(min[i] as number, p[i] as number);
        max[i] = Math.max(max[i] as number, p[i] as number);
        col[k * 3 + i] = t.shade;
      }
      nor[k * 3] = nx;
      nor[k * 3 + 1] = ny;
      nor[k * 3 + 2] = nz;
      k++;
    }
  }
  const bin = Buffer.concat([Buffer.from(pos.buffer), Buffer.from(nor.buffer), Buffer.from(col.buffer)]);
  const view = (i: number) => ({ buffer: 0, byteOffset: i * n * 12, byteLength: n * 12, target: 34962 });
  const json = {
    asset: { version: '2.0', generator: 'warboard build-models' },
    scene: 0,
    scenes: [{ nodes: [0] }],
    nodes: [{ mesh: 0, name }],
    meshes: [{ name, primitives: [{ attributes: { POSITION: 0, NORMAL: 1, COLOR_0: 2 }, material: 0, mode: 4 }] }],
    materials: [{ name: 'class-grey', pbrMetallicRoughness: { baseColorFactor: [1, 1, 1, 1], metallicFactor: 0.1, roughnessFactor: 0.8 }, doubleSided: true }],
    buffers: [{ byteLength: bin.length }],
    bufferViews: [view(0), view(1), view(2)],
    accessors: [
      { bufferView: 0, componentType: 5126, count: n, type: 'VEC3', min, max },
      { bufferView: 1, componentType: 5126, count: n, type: 'VEC3' },
      { bufferView: 2, componentType: 5126, count: n, type: 'VEC3' },
    ],
  };
  let text = Buffer.from(JSON.stringify(json));
  if (text.length % 4) text = Buffer.concat([text, Buffer.alloc(4 - (text.length % 4), 0x20)]);
  const header = Buffer.alloc(12);
  header.writeUInt32LE(0x46546c67, 0);
  header.writeUInt32LE(2, 4);
  header.writeUInt32LE(12 + 8 + text.length + 8 + bin.length, 8);
  const chunk = (data: Buffer, type: number) => {
    const h = Buffer.alloc(8);
    h.writeUInt32LE(data.length, 0);
    h.writeUInt32LE(type, 4);
    return Buffer.concat([h, data]);
  };
  return Buffer.concat([header, chunk(text, 0x4e4f534a), chunk(bin, 0x004e4942)]);
}

const here = dirname(fileURLToPath(import.meta.url));
const outDir = resolve(process.argv[2] ?? resolve(here, '../../../apps/game/public/models'));
mkdirSync(outDir, { recursive: true });
let triangles = 0;
for (const name of MODEL_NAMES) {
  const part = SHAPES[name]();
  triangles += part.length;
  writeFileSync(resolve(outDir, `${name}.glb`), glb(part, name));
}
console.log(`models: ${MODEL_NAMES.length} glTF files, ${triangles} triangles → ${outDir}`);
