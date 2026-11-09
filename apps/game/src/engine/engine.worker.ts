// Engine worker (ADR-002): owns the WebAssembly engine, advances it in a loop, and posts
// the side view about 10 times a second in one of two transfer buffers (doc 06 §2–3).
// It also owns the region packs' engine files, read synchronously from OPFS (doc 05 §4,
// ADR-019), and writes save slots (autosave every 5 sim minutes, doc 03 §13).
import type { Event, SaveHeader, TickReport } from '@warboard/protocol';
import init, { Engine, GeoQuery, map_labels, save_header } from '@warboard/wb-wasm';
import wasmUrl from '@warboard/wb-wasm/wb_wasm_bg.wasm?url';
// The war and its content pack are chosen at build time (vite.config.ts: WB_SCENARIO,
// WB_CONTENT_PACK); by default the Theatre North straits quick start on build/content.wbpack.
import contentUrl from '@war/content.wbpack?url';
import scenarioYaml from '@war/scenario.yaml?raw';
import { AUTOSAVE_SLOT, readSave, writeSave } from '../saves/store';
import type { FromWorker, PackFiles, PlaceLabel, ReplyData, ToWorker, WarInfo, WorkerRequest } from './messages';

const FRAME_MS = 16;
const VIEW_MS = 100;
/** Window over which achieved speed is measured. */
const SPEED_WINDOW_MS = 1_000;
/** Autosave every 5 sim minutes (doc 03 §13). */
const AUTOSAVE_SIM_MS = 5 * 60_000;

let speed = 1;
const wasm = init({ module_or_path: wasmUrl });
const contentPack = fetch(contentUrl).then(async (r) => {
  if (!r.ok) throw new Error(`Content pack missing (${r.status}); run pnpm gen`);
  return new Uint8Array(await r.arrayBuffer());
});

const post = (msg: FromWorker, transfer: Transferable[] = []) => self.postMessage(msg, { transfer });

let engine: Engine | null = null;
const warName = /^name:\s*(.+?)\s*$/m.exec(scenarioYaml)?.[1] ?? 'War';
let installed: PackFiles[] = [];

// ---- Region packs -------------------------------------------------------------------

let geo: GeoQuery | null = null;
let handles: FileSystemSyncAccessHandle[] = [];

function closePacks(): void {
  engine?.clear_geo();
  for (const h of handles) h.close();
  handles = [];
  geo?.free();
  geo = null;
}

/** After a reload the previous page's worker may still hold the exclusive handles for a
 *  moment; wait for it rather than fail. */
async function syncHandle(file: FileSystemFileHandle): Promise<FileSystemSyncAccessHandle> {
  for (let attempt = 0; ; attempt++) {
    try {
      return await file.createSyncAccessHandle();
    } catch (err) {
      if (attempt >= 20 || !(err instanceof DOMException) || err.name !== 'NoModificationAllowedError') throw err;
      await new Promise((r) => setTimeout(r, 250));
    }
  }
}

async function syncFile(dir: FileSystemDirectoryHandle, name: string) {
  const handle = await syncHandle(await dir.getFileHandle(name));
  handles.push(handle);
  return {
    name,
    size: () => handle.getSize(),
    readAt: (buf: Uint8Array, at: number) => handle.read(buf, { at }),
  };
}

async function openPacks(packs: PackFiles[]): Promise<string> {
  await wasm;
  closePacks();
  const root = await (await navigator.storage.getDirectory()).getDirectoryHandle('packs');
  const q = new GeoQuery();
  try {
    for (const p of packs) {
      const dir = await root.getDirectoryHandle(p.id);
      q.add_pack(p.id, await syncFile(dir, p.dem), await syncFile(dir, p.terrain), await syncFile(dir, p.territory));
    }
  } catch (err) {
    q.free();
    closePacks();
    throw err;
  }
  geo = q;
  installed = packs;
  engine?.set_geo(q);
  return q.info();
}

// Sync access handles are exclusive, so opening and closing packs must never overlap.
let packQueue: Promise<unknown> = Promise.resolve();
function queued<T>(job: () => Promise<T>): Promise<T> {
  const next = packQueue.then(job, job);
  packQueue = next.catch(() => undefined);
  return next;
}

// ---- Saves --------------------------------------------------------------------------

function saveBytes(): { bytes: Uint8Array; header: SaveHeader } {
  if (!engine) throw new Error('No war is running');
  const packs = JSON.stringify(installed.map((p) => `${p.id}@${p.version}`));
  const bytes = engine.save(warName, packs, Date.now());
  return { bytes, header: JSON.parse(save_header(bytes)) as SaveHeader };
}

async function saveTo(slot: string): Promise<SaveHeader> {
  const { bytes, header } = saveBytes();
  await writeSave(slot, bytes, header);
  return header;
}

let saving: Promise<unknown> = Promise.resolve();
function autosave(): void {
  saving = saving
    .then(() => saveTo(AUTOSAVE_SLOT))
    .then((header) => post({ type: 'autosaved', header }))
    .catch((err: unknown) => post({ type: 'error', message: `Autosave failed: ${String(err)}` }));
}

// ---- Requests -----------------------------------------------------------------------

async function handle(req: WorkerRequest): Promise<{ data: ReplyData[WorkerRequest['type']]; transfer?: Transferable[] }> {
  switch (req.type) {
    case 'openPacks':
      return { data: await queued(() => openPacks(req.packs)) };
    case 'closePacks':
      await queued(async () => closePacks());
      return { data: null };
    case 'terrainCells': {
      if (!geo) return { data: new Float64Array(0) };
      const [w, s, e, n] = req.bbox;
      const cells = geo.terrain_cells(w, s, e, n, req.max);
      return cells ? { data: cells, transfer: [cells.buffer] } : { data: null };
    }
    case 'point':
      return { data: geo ? Array.from(geo.point(req.lat, req.lon)) : [] };
    case 'details':
      if (!engine) throw new Error('No war is running');
      return { data: JSON.parse(engine.details(req.id)) as ReplyData['details'] };
    case 'contactDetails':
      if (!engine) throw new Error('No war is running');
      return { data: JSON.parse(engine.contact_details(req.id)) as ReplyData['contactDetails'] };
    case 'approvals':
      if (!engine) throw new Error('No war is running');
      return { data: JSON.parse(engine.approvals()) as ReplyData['approvals'] };
    case 'groundTrack':
      if (!engine) throw new Error('No war is running');
      return {
        data: JSON.parse(engine.ground_track(req.id, req.beforeMin, req.afterMin, 60)) as ReplyData['groundTrack'],
      };
    case 'order':
      if (!engine) throw new Error('No war is running');
      // UI orders apply at the next tick the engine runs (doc 04 §7.2).
      return {
        data: JSON.parse(engine.order(JSON.stringify({ ...req.order, tick: engine.tick() }))) as ReplyData['order'],
      };
    case 'operations':
      if (!engine) throw new Error('No war is running');
      return { data: JSON.parse(engine.operations()) as ReplyData['operations'] };
    case 'checkOperation':
      if (!engine) throw new Error('No war is running');
      return { data: JSON.parse(engine.check_operation(JSON.stringify(req.spec))) as ReplyData['checkOperation'] };
    case 'report':
      if (!engine) throw new Error('No war is running');
      return { data: JSON.parse(engine.report(req.truth, 'json')) as ReplyData['report'] };
    case 'reportText':
      if (!engine) throw new Error('No war is running');
      return { data: engine.report(req.truth, req.format) };
    case 'replayStart':
      if (!engine) throw new Error('No war is running');
      return { data: JSON.parse(engine.replay_start()) as ReplyData['replayStart'] };
    case 'replaySeek':
      if (!engine) throw new Error('No war is running');
      return { data: JSON.parse(engine.replay_seek(req.tick)) as ReplyData['replaySeek'] };
    case 'replayStop':
      if (!engine) throw new Error('No war is running');
      return { data: JSON.parse(engine.replay_stop()) as ReplyData['replayStop'] };
    case 'save':
      return { data: await saveTo(req.slot) };
    case 'load': {
      const bytes = await readSave(req.slot);
      const pack = await contentPack;
      start(Engine.load(bytes, pack), pack);
      return { data: null };
    }
    case 'exportSave': {
      const bytes = await readSave(req.slot);
      const copy = bytes.slice().buffer;
      return { data: copy, transfer: [copy] };
    }
    case 'importSave': {
      const bytes = new Uint8Array(req.bytes);
      const header = JSON.parse(save_header(bytes)) as SaveHeader;
      await writeSave(req.slot, bytes, header);
      return { data: header };
    }
  }
}

// ---- Simulation loop ----------------------------------------------------------------

/** Two view buffers, passed back and forth with the UI (doc 06 §2). */
const freeBuffers: ArrayBuffer[] = [];
let loopTimer: ReturnType<typeof setTimeout> | null = null;

self.addEventListener('message', (e: MessageEvent<ToWorker>) => {
  const msg = e.data;
  if (msg.type === 'setSpeed') {
    speed = msg.speed;
    return;
  }
  if (msg.type === 'viewReturn') {
    if (freeBuffers.length < 2) freeBuffers.push(msg.buffer);
    return;
  }
  if (msg.type === 'autosaveNow') {
    if (engine) autosave();
    return;
  }
  handle(msg).then(
    ({ data, transfer }) => post({ type: 'reply', reqId: msg.reqId, ok: true, data }, transfer),
    (err: unknown) =>
      post({ type: 'reply', reqId: msg.reqId, ok: false, message: err instanceof Error ? err.message : String(err) }),
  );
});

function postView(e: Engine, memory: WebAssembly.Memory): void {
  const len = e.encode_view();
  const bytes = new Uint8Array(memory.buffer, e.view_ptr(), len);
  let buffer = freeBuffers.pop();
  if (!buffer || buffer.byteLength < len) buffer = new ArrayBuffer(Math.max(len, 64 * 1024) * 2);
  new Uint8Array(buffer).set(bytes);
  post({ type: 'view', buffer }, [buffer]);
}

/** Runs `next` as the war; replaces any war running. `pack` is its content pack. */
function start(next: Engine, pack: Uint8Array): void {
  if (loopTimer !== null) clearTimeout(loopTimer);
  engine?.free();
  engine = next;
  if (geo) engine.set_geo(geo);
  freeBuffers.length = 0;

  const samples: { realMs: number; simMs: number }[] = [];
  const report = (e: Engine): TickReport => {
    const first = samples[0];
    const last = samples[samples.length - 1];
    const realSpan = first && last ? last.realMs - first.realMs : 0;
    return {
      tick: e.tick(),
      simTimeMs: e.sim_time_ms(),
      startUtcMs: e.start_utc_ms(),
      tickLenMs: e.tick_len_ms(),
      requestedSpeed: speed,
      achievedSpeed: first && last && realSpan > 0 ? (last.simMs - first.simMs) / realSpan : 0,
      stateHash: '',
    };
  };

  const war = JSON.parse(engine.sides()) as Omit<WarInfo, 'warName' | 'places'>;
  const places = JSON.parse(map_labels(pack)) as PlaceLabel[];
  post({ type: 'ready', report: report(engine), war: { ...war, warName, places } });

  let lastFrame = performance.now();
  let lastView = -Infinity;
  let autosaveBlock = Math.floor(engine.sim_time_ms() / AUTOSAVE_SIM_MS);
  const memory = wasmMemory;
  const loop = () => {
    const e = engine;
    if (!e || !memory) return;
    const now = performance.now();
    e.advance(now - lastFrame, speed);
    lastFrame = now;

    samples.push({ realMs: now, simMs: e.sim_time_ms() });
    while (samples.length > 2 && now - (samples[0]?.realMs ?? now) > SPEED_WINDOW_MS) samples.shift();

    if (now - lastView >= VIEW_MS) {
      lastView = now;
      postView(e, memory);
      post({ type: 'tick', report: report(e) });
      const events = JSON.parse(e.take_events()) as Event[];
      if (events.length > 0) post({ type: 'events', events });
    }
    const block = Math.floor(e.sim_time_ms() / AUTOSAVE_SIM_MS);
    if (block > autosaveBlock) {
      autosaveBlock = block;
      autosave();
    }
    loopTimer = setTimeout(loop, FRAME_MS);
  };
  loop();
}

let wasmMemory: WebAssembly.Memory | null = null;

async function boot(): Promise<void> {
  const exports = await wasm;
  wasmMemory = exports.memory;
  const pack = await contentPack;
  start(Engine.create(scenarioYaml, pack), pack);
}

boot().catch((err: unknown) => {
  post({ type: 'error', message: err instanceof Error ? err.message : String(err) });
});
