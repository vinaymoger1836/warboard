// Pack registry in IndexedDB (doc 06 §8): which packs are installed, with their manifests.

import type { InstalledPack } from './types';

const DB = 'warboard';
const STORE = 'packs';

function open(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB, 1);
    req.onupgradeneeded = () => {
      if (!req.result.objectStoreNames.contains(STORE)) req.result.createObjectStore(STORE, { keyPath: 'id' });
    };
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error ?? new Error('Cannot open the pack registry'));
  });
}

async function tx<T>(mode: IDBTransactionMode, run: (store: IDBObjectStore) => IDBRequest<T>): Promise<T> {
  const db = await open();
  try {
    return await new Promise<T>((resolve, reject) => {
      const req = run(db.transaction(STORE, mode).objectStore(STORE));
      req.onsuccess = () => resolve(req.result);
      req.onerror = () => reject(req.error ?? new Error('Pack registry request failed'));
    });
  } finally {
    db.close();
  }
}

export async function listInstalled(): Promise<InstalledPack[]> {
  const all = await tx<InstalledPack[]>('readonly', (s) => s.getAll() as IDBRequest<InstalledPack[]>);
  return all.sort((a, b) => a.id.localeCompare(b.id));
}

export async function putInstalled(pack: InstalledPack): Promise<void> {
  await tx('readwrite', (s) => s.put(pack));
}

export async function deleteInstalled(id: string): Promise<void> {
  await tx('readwrite', (s) => s.delete(id));
}
