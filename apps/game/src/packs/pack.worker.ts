// Pack worker (doc 06 §1 `packs`): downloads pack files into OPFS through synchronous
// access handles and hashes the same bytes with BLAKE3 (wb-wasm). A file whose hash does
// not match the manifest is deleted. Files already present with the right hash are kept,
// so an interrupted install resumes.
import init, { Blake3Hasher } from '@warboard/wb-wasm';
import wasmUrl from '@warboard/wb-wasm/wb_wasm_bg.wasm?url';
import type { FromPackWorker, ToPackWorker } from './messages';
import type { PackFileEntry } from './types';

const CHUNK = 8 << 20;
const PROGRESS_MS = 250;

const post = (msg: FromPackWorker) => self.postMessage(msg);
const ready = init({ module_or_path: wasmUrl });

async function existingHash(handle: FileSystemFileHandle, size: number): Promise<string | null> {
  const access = await handle.createSyncAccessHandle();
  try {
    if (access.getSize() !== size) return null;
    const hasher = new Blake3Hasher();
    const buf = new Uint8Array(CHUNK);
    for (let at = 0; at < size; at += CHUNK) {
      const n = access.read(buf, { at });
      hasher.update(buf.subarray(0, n));
    }
    const hex = hasher.finalize_hex();
    hasher.free();
    return hex;
  } finally {
    access.close();
  }
}

async function download(id: string, url: string, dir: FileSystemDirectoryHandle, file: PackFileEntry): Promise<void> {
  const handle = await dir.getFileHandle(file.name, { create: true });
  if ((await existingHash(handle, file.size)) === file.blake3) {
    post({ type: 'progress', id, file: file.name, done: file.size, total: file.size });
    return;
  }
  const res = await fetch(url, { cache: 'no-store' });
  if (!res.ok || !res.body) throw new Error(`${file.name}: download failed (HTTP ${res.status})`);
  const access = await handle.createSyncAccessHandle();
  const hasher = new Blake3Hasher();
  let at = 0;
  let last = 0;
  try {
    access.truncate(0);
    const reader = res.body.getReader();
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      let off = 0;
      while (off < value.length) off += access.write(value.subarray(off), { at: at + off });
      hasher.update(value);
      at += value.length;
      const now = performance.now();
      if (now - last > PROGRESS_MS) {
        last = now;
        post({ type: 'progress', id, file: file.name, done: at, total: file.size });
      }
    }
    access.flush();
  } finally {
    access.close();
  }
  const hex = hasher.finalize_hex();
  hasher.free();
  if (at !== file.size || hex !== file.blake3) {
    await dir.removeEntry(file.name);
    throw new Error(
      at !== file.size
        ? `${file.name}: got ${at} bytes, expected ${file.size}`
        : `${file.name}: hash does not match the manifest; the file was deleted`,
    );
  }
  post({ type: 'progress', id, file: file.name, done: at, total: file.size });
}

async function install(msg: Extract<ToPackWorker, { type: 'install' }>): Promise<void> {
  await ready;
  const root = await (await navigator.storage.getDirectory()).getDirectoryHandle('packs', { create: true });
  const dir = await root.getDirectoryHandle(msg.id, { create: true });
  for (const file of msg.files) {
    await download(msg.id, new URL(`${msg.id}/${file.name}`, msg.baseUrl).href, dir, file);
  }
  post({ type: 'done', id: msg.id });
}

self.addEventListener('message', (e: MessageEvent<ToPackWorker>) => {
  if (e.data.type === 'install') {
    install(e.data).catch((err: unknown) =>
      post({ type: 'error', id: e.data.id, message: err instanceof Error ? err.message : String(err) }),
    );
  }
});
