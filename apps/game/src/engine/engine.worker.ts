// Engine worker (ADR-002): owns the WebAssembly engine, advances it in a loop
// and posts a tick report to the UI about 10 times a second. It also owns the region
// packs' engine files, read synchronously from OPFS (doc 05 §4, ADR-019).
import type { TickReport } from '@warboard/protocol';
import init, { Engine, GeoQuery } from '@warboard/wb-wasm';
import wasmUrl from '@warboard/wb-wasm/wb_wasm_bg.wasm?url';
import scenarioYaml from '../../../../scenarios/empty.yaml?raw';
import type { FromWorker, PackFiles, ReplyData, ToWorker, WorkerRequest } from './messages';

const FRAME_MS = 16;
const REPORT_MS = 100;
/** Window over which achieved speed is measured. */
const SPEED_WINDOW_MS = 1_000;

let speed = 1;
const ready = init({ module_or_path: wasmUrl });

const post = (msg: FromWorker, transfer: Transferable[] = []) => self.postMessage(msg, { transfer });

// ---- Region packs -------------------------------------------------------------------

let geo: GeoQuery | null = null;
let handles: FileSystemSyncAccessHandle[] = [];

function closePacks(): void {
  for (const h of handles) h.close();
  handles = [];
  geo?.free();
  geo = null;
}

async function syncFile(dir: FileSystemDirectoryHandle, name: string) {
  const handle = await (await dir.getFileHandle(name)).createSyncAccessHandle();
  handles.push(handle);
  return {
    name,
    size: () => handle.getSize(),
    readAt: (buf: Uint8Array, at: number) => handle.read(buf, { at }),
  };
}

async function openPacks(packs: PackFiles[]): Promise<string> {
  await ready;
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
  return q.info();
}

// Sync access handles are exclusive, so opening and closing packs must never overlap.
let packQueue: Promise<unknown> = Promise.resolve();
function queued<T>(job: () => Promise<T>): Promise<T> {
  const next = packQueue.then(job, job);
  packQueue = next.catch(() => undefined);
  return next;
}

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
  }
}

self.addEventListener('message', (e: MessageEvent<ToWorker>) => {
  const msg = e.data;
  if (msg.type === 'setSpeed') {
    speed = msg.speed;
    return;
  }
  handle(msg).then(
    ({ data, transfer }) => post({ type: 'reply', reqId: msg.reqId, ok: true, data }, transfer),
    (err: unknown) =>
      post({ type: 'reply', reqId: msg.reqId, ok: false, message: err instanceof Error ? err.message : String(err) }),
  );
});

// ---- Simulation loop ----------------------------------------------------------------

async function start(): Promise<void> {
  await ready;
  const engine = Engine.create(scenarioYaml);

  const samples: { realMs: number; simMs: number }[] = [];
  const report = (): TickReport => {
    const first = samples[0];
    const last = samples[samples.length - 1];
    const realSpan = first && last ? last.realMs - first.realMs : 0;
    return {
      tick: engine.tick(),
      simTimeMs: engine.sim_time_ms(),
      startUtcMs: engine.start_utc_ms(),
      tickLenMs: engine.tick_len_ms(),
      requestedSpeed: speed,
      achievedSpeed: first && last && realSpan > 0 ? (last.simMs - first.simMs) / realSpan : 0,
      stateHash: engine.state_hash(),
    };
  };

  post({ type: 'ready', report: report() });

  let lastFrame = performance.now();
  let lastReport = lastFrame;
  const loop = () => {
    const now = performance.now();
    engine.advance(now - lastFrame, speed);
    lastFrame = now;

    samples.push({ realMs: now, simMs: engine.sim_time_ms() });
    while (samples.length > 2 && now - (samples[0]?.realMs ?? now) > SPEED_WINDOW_MS) samples.shift();

    if (now - lastReport >= REPORT_MS) {
      lastReport = now;
      post({ type: 'tick', report: report() });
    }
    setTimeout(loop, FRAME_MS);
  };
  loop();
}

start().catch((err: unknown) => {
  post({ type: 'error', message: err instanceof Error ? err.message : String(err) });
});
