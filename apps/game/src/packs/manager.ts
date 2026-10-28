// Pack manager (doc 06 §1, §8; ADR-011): catalogue, install with hash checks, removal,
// eviction detection, storage quota and persistence.
import init, { Blake3Hasher } from '@warboard/wb-wasm';
import wasmUrl from '@warboard/wb-wasm/wb_wasm_bg.wasm?url';
import type { FromPackWorker, ToPackWorker } from './messages';
import { packFile, removePackDir } from './opfs';
import { deleteInstalled, listInstalled, putInstalled } from './registry';
import type { Catalog, CatalogEntry, InstalledPack, PackManifest, PackStatus } from './types';

export interface PackState {
  catalog: CatalogEntry[] | null;
  catalogError: string | null;
  installed: InstalledPack[];
  /** Installed packs whose files are all present: the ones the map and engine use. */
  usable: InstalledPack[];
  status: Record<string, PackStatus>;
  usage: { usage: number; quota: number } | null;
  persisted: boolean | null;
}

type Listener = (s: PackState) => void;

const wasmReady = init({ module_or_path: wasmUrl });

async function blake3Hex(bytes: Uint8Array): Promise<string> {
  await wasmReady;
  const h = new Blake3Hasher();
  h.update(bytes);
  const hex = h.finalize_hex();
  h.free();
  return hex;
}

export class PackManager {
  readonly baseUrl: string;
  private state: PackState = {
    catalog: null,
    catalogError: null,
    installed: [],
    usable: [],
    status: {},
    usage: null,
    persisted: null,
  };
  private readonly listeners = new Set<Listener>();
  private worker: Worker | null = null;
  private beforeChange: () => Promise<void> = async () => {};
  private readonly changed = new Set<(usable: InstalledPack[]) => void>();

  constructor(baseUrl = new URL('packs/', document.baseURI).href) {
    this.baseUrl = baseUrl;
  }

  get snapshot(): PackState {
    return this.state;
  }

  subscribe(l: Listener): () => void {
    this.listeners.add(l);
    l(this.state);
    return () => this.listeners.delete(l);
  }

  /** Called before files change, so open handles (engine worker) can be released. */
  setBeforeChange(fn: () => Promise<void>): void {
    this.beforeChange = fn;
  }

  /** Called after the set of usable packs changes. */
  onPacksChanged(fn: (usable: InstalledPack[]) => void): () => void {
    this.changed.add(fn);
    return () => this.changed.delete(fn);
  }

  async refresh(): Promise<void> {
    const installed = await listInstalled();
    const status: Record<string, PackStatus> = {};
    const usable: InstalledPack[] = [];
    for (const p of installed) {
      const present = await Promise.all(
        p.manifest.files.map(async (f) => (await packFile(p.id, f.name))?.size === f.size),
      );
      if (present.every(Boolean)) {
        status[p.id] = { kind: 'installed' };
        usable.push(p);
      } else {
        status[p.id] = { kind: 'evicted' };
      }
    }
    let catalog: CatalogEntry[] | null = null;
    let catalogError: string | null = null;
    try {
      const res = await fetch(new URL('index.json', this.baseUrl), { cache: 'no-store' });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      catalog = ((await res.json()) as Catalog).packs;
      for (const c of catalog) {
        const inst = installed.find((p) => p.id === c.id);
        if (!inst) status[c.id] = { kind: 'available' };
        else if (inst.version !== c.version && status[c.id]?.kind === 'installed')
          status[c.id] = { kind: 'update', from: inst.version };
      }
    } catch (err) {
      catalogError = navigator.onLine ? `Pack list unavailable (${String(err)})` : 'Offline: pack list unavailable';
    }
    for (const [id, s] of Object.entries(this.state.status)) {
      if (s.kind === 'installing' || s.kind === 'error') status[id] = s;
    }
    const before = this.state.usable.map((p) => `${p.id}@${p.version}`).join();
    this.set({ catalog, catalogError, installed, usable, status, ...(await storageInfo()) });
    if (usable.map((p) => `${p.id}@${p.version}`).join() !== before) for (const fn of this.changed) fn(usable);
  }

  async install(id: string): Promise<void> {
    const entry = this.state.catalog?.find((c) => c.id === id);
    if (!entry) throw new Error(`Pack ${id} is not in the pack list`);
    this.setStatus(id, { kind: 'installing', file: 'manifest.json', done: 0, total: entry.size });
    try {
      const res = await fetch(new URL(entry.manifest, this.baseUrl), { cache: 'no-store' });
      if (!res.ok) throw new Error(`manifest: HTTP ${res.status}`);
      const bytes = new Uint8Array(await res.arrayBuffer());
      if ((await blake3Hex(bytes)) !== entry.manifest_blake3) throw new Error('manifest hash does not match the pack list');
      const manifest = JSON.parse(new TextDecoder().decode(bytes)) as PackManifest;
      if (this.state.persisted !== true && navigator.storage.persist) await navigator.storage.persist();
      await this.beforeChange();
      await this.runWorker(manifest);
      await putInstalled({ id, version: manifest.version, manifest });
      this.setStatus(id, { kind: 'installed' });
    } catch (err) {
      this.setStatus(id, { kind: 'error', message: err instanceof Error ? err.message : String(err) });
    }
    await this.refresh();
  }

  async remove(id: string): Promise<void> {
    await this.beforeChange();
    await removePackDir(id);
    await deleteInstalled(id);
    const status = { ...this.state.status };
    delete status[id];
    this.set({ status });
    await this.refresh();
  }

  private runWorker(manifest: PackManifest): Promise<void> {
    this.worker ??= new Worker(new URL('./pack.worker.ts', import.meta.url), { type: 'module', name: 'packs' });
    const worker = this.worker;
    let doneBefore = 0;
    let current = '';
    return new Promise((resolve, reject) => {
      const onMessage = (e: MessageEvent<FromPackWorker>) => {
        const msg = e.data;
        if (msg.id !== manifest.id) return;
        if (msg.type === 'progress') {
          if (msg.file !== current) {
            const prev = manifest.files.find((f) => f.name === current);
            doneBefore += prev?.size ?? 0;
            current = msg.file;
          }
          this.setStatus(manifest.id, {
            kind: 'installing',
            file: msg.file,
            done: doneBefore + msg.done,
            total: manifest.size,
          });
          return;
        }
        worker.removeEventListener('message', onMessage);
        if (msg.type === 'done') resolve();
        else reject(new Error(msg.message));
      };
      worker.addEventListener('message', onMessage);
      const msg: ToPackWorker = { type: 'install', id: manifest.id, baseUrl: this.baseUrl, files: manifest.files };
      worker.postMessage(msg);
    });
  }

  private setStatus(id: string, s: PackStatus): void {
    this.set({ status: { ...this.state.status, [id]: s } });
  }

  private set(patch: Partial<PackState>): void {
    this.state = { ...this.state, ...patch };
    for (const l of this.listeners) l(this.state);
  }
}

async function storageInfo(): Promise<Pick<PackState, 'usage' | 'persisted'>> {
  const est = await navigator.storage?.estimate?.();
  const persisted = (await navigator.storage?.persisted?.()) ?? null;
  return {
    usage: est && est.quota !== undefined ? { usage: est.usage ?? 0, quota: est.quota } : null,
    persisted,
  };
}
