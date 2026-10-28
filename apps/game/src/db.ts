// The app's IndexedDB database (doc 06 §8): pack registry and save slot metadata.
// Usable from the UI thread and the engine worker.

const DB = 'warboard';
/** 1: packs (P2). 2: saves (P3). */
const VERSION = 2;
export const PACKS = 'packs';
export const SAVES = 'saves';

function open(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB, VERSION);
    req.onupgradeneeded = () => {
      const db = req.result;
      if (!db.objectStoreNames.contains(PACKS)) db.createObjectStore(PACKS, { keyPath: 'id' });
      if (!db.objectStoreNames.contains(SAVES)) db.createObjectStore(SAVES, { keyPath: 'slot' });
    };
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error ?? new Error('Cannot open the app database'));
  });
}

/** Runs one request in its own transaction. */
export async function tx<T>(
  store: string,
  mode: IDBTransactionMode,
  run: (s: IDBObjectStore) => IDBRequest<T>,
): Promise<T> {
  const db = await open();
  try {
    return await new Promise<T>((resolve, reject) => {
      const req = run(db.transaction(store, mode).objectStore(store));
      req.onsuccess = () => resolve(req.result);
      req.onerror = () => reject(req.error ?? new Error(`Database request on ${store} failed`));
    });
  } finally {
    db.close();
  }
}
