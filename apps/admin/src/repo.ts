// The repo folder the owner opened (File System Access API, ADR-010). The admin reads
// content/**/*.yaml, content/denylist.txt and scenarios/bench/*.yaml, writes content files
// back, and writes the compiled pack to build/content.wbpack for the game's dev server.

const DB = 'warboard-admin';
const STORE = 'handles';

function idb(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB, 1);
    req.onupgradeneeded = () => req.result.createObjectStore(STORE);
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error ?? new Error('IndexedDB'));
  });
}

async function remember(handle: FileSystemDirectoryHandle): Promise<void> {
  const db = await idb();
  await new Promise<void>((resolve, reject) => {
    const tx = db.transaction(STORE, 'readwrite');
    tx.objectStore(STORE).put(handle, 'repo');
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error ?? new Error('IndexedDB'));
  });
}

/** The folder opened last time, if the browser kept it. */
export async function lastRepo(): Promise<FileSystemDirectoryHandle | null> {
  const db = await idb();
  return new Promise((resolve) => {
    const req = db.transaction(STORE).objectStore(STORE).get('repo');
    req.onsuccess = () => resolve((req.result as FileSystemDirectoryHandle | undefined) ?? null);
    req.onerror = () => resolve(null);
  });
}

export class Repo {
  constructor(readonly root: FileSystemDirectoryHandle) {}

  get name(): string {
    return this.root.name;
  }

  /** Asks the owner to pick the repo folder (the one holding content/ and scenarios/). */
  static async pick(): Promise<Repo> {
    const root = await window.showDirectoryPicker({ id: 'warboard-repo', mode: 'readwrite' });
    await Repo.check(root);
    await remember(root);
    return new Repo(root);
  }

  static async reopen(root: FileSystemDirectoryHandle): Promise<Repo> {
    if ((await root.requestPermission({ mode: 'readwrite' })) !== 'granted') {
      throw new Error('Permission to the repo folder was not granted');
    }
    await Repo.check(root);
    return new Repo(root);
  }

  private static async check(root: FileSystemDirectoryHandle): Promise<void> {
    try {
      const content = await root.getDirectoryHandle('content');
      await content.getFileHandle('manifest.yaml');
    } catch {
      throw new Error(`"${root.name}" has no content/manifest.yaml; open the WarBoard repo folder`);
    }
  }

  private async dir(path: string[], create = false): Promise<FileSystemDirectoryHandle> {
    let d = this.root;
    for (const p of path) d = await d.getDirectoryHandle(p, { create });
    return d;
  }

  /** Every content file: path relative to content/ → text. */
  async readContent(): Promise<Record<string, string>> {
    const out: Record<string, string> = {};
    const walk = async (d: FileSystemDirectoryHandle, prefix: string, top: boolean): Promise<void> => {
      const names: [string, FileSystemFileHandle | FileSystemDirectoryHandle][] = [];
      for await (const e of d.entries()) names.push(e);
      names.sort((a, b) => a[0].localeCompare(b[0]));
      for (const [name, h] of names) {
        if (h.kind === 'directory') await walk(h, `${prefix}${name}/`, false);
        else if (/\.ya?ml$/.test(name) || (top && name === 'denylist.txt')) {
          out[`${prefix}${name}`] = await (await h.getFile()).text();
        }
      }
    };
    await walk(await this.dir(['content']), '', true);
    return out;
  }

  /** Test-bench scenarios: file name → text. */
  async readBench(): Promise<Record<string, string>> {
    const out: Record<string, string> = {};
    try {
      const d = await this.dir(['scenarios', 'bench']);
      for await (const [name, h] of d.entries()) {
        if (h.kind === 'file' && name.endsWith('.yaml')) out[name] = await (await h.getFile()).text();
      }
    } catch {
      // No bench folder: nothing to list.
    }
    return out;
  }

  async writeContent(path: string, text: string): Promise<void> {
    const parts = path.split('/');
    const name = parts.pop();
    if (!name) throw new Error(`Bad path ${path}`);
    const d = await this.dir(['content', ...parts], true);
    const w = await (await d.getFileHandle(name, { create: true })).createWritable();
    await w.write(text);
    await w.close();
  }

  async removeContent(path: string): Promise<void> {
    const parts = path.split('/');
    const name = parts.pop();
    if (!name) return;
    const d = await this.dir(['content', ...parts]);
    await d.removeEntry(name);
  }

  async writeBuild(name: string, bytes: Uint8Array): Promise<void> {
    const d = await this.dir(['build'], true);
    const w = await (await d.getFileHandle(name, { create: true })).createWritable();
    await w.write(new Uint8Array(bytes));
    await w.close();
  }

  /** A region pack's engine files under build/packs/<id>/, for the border preview. */
  async packFiles(id: string): Promise<{ dem: File; terrain: File; territory: File } | null> {
    try {
      const d = await this.dir(['build', 'packs', id]);
      const f = async (n: string) => (await d.getFileHandle(n)).getFile();
      return { dem: await f(`dem-${id}.wbdem`), terrain: await f(`terrain-${id}.wbh3`), territory: await f(`territory-${id}.bin`) };
    } catch {
      return null;
    }
  }
}
