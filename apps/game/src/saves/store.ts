// Save slots (doc 06 §8, doc 03 §13): `.wbsave` blobs in OPFS (/saves/<slot>.wbsave),
// slot metadata in IndexedDB. The engine worker writes (sync access handles); the UI
// lists, exports and deletes.
import type { SaveHeader } from '@warboard/protocol';
import { SAVES, tx } from '../db';

export const AUTOSAVE_SLOT = 'autosave';
export const MANUAL_SLOTS = ['slot-1', 'slot-2', 'slot-3'] as const;

export interface SaveMeta {
  slot: string;
  header: SaveHeader;
  bytes: number;
}

async function savesDir(): Promise<FileSystemDirectoryHandle> {
  const root = await navigator.storage.getDirectory();
  return root.getDirectoryHandle('saves', { create: true });
}

const fileName = (slot: string) => `${slot}.wbsave`;

/** Worker only: writes the blob, then the metadata. */
export async function writeSave(slot: string, bytes: Uint8Array, header: SaveHeader): Promise<void> {
  const handle = await (await savesDir()).getFileHandle(fileName(slot), { create: true });
  const sync = await handle.createSyncAccessHandle();
  try {
    sync.truncate(0);
    sync.write(bytes, { at: 0 });
    sync.flush();
  } finally {
    sync.close();
  }
  const meta: SaveMeta = { slot, header, bytes: bytes.length };
  await tx(SAVES, 'readwrite', (s) => s.put(meta));
}

export async function readSave(slot: string): Promise<Uint8Array> {
  const file = await (await (await savesDir()).getFileHandle(fileName(slot))).getFile();
  return new Uint8Array(await file.arrayBuffer());
}

export async function listSaves(): Promise<SaveMeta[]> {
  const all = await tx<SaveMeta[]>(SAVES, 'readonly', (s) => s.getAll() as IDBRequest<SaveMeta[]>);
  return all.sort((a, b) => b.header.created_utc_ms - a.header.created_utc_ms);
}

export async function deleteSave(slot: string): Promise<void> {
  try {
    await (await savesDir()).removeEntry(fileName(slot));
  } catch {
    // Already gone.
  }
  await tx(SAVES, 'readwrite', (s) => s.delete(slot));
}
