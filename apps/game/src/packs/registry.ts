// Pack registry in IndexedDB (doc 06 §8): which packs are installed, with their manifests.

import { PACKS, tx } from '../db';
import type { InstalledPack } from './types';

export async function listInstalled(): Promise<InstalledPack[]> {
  const all = await tx<InstalledPack[]>(PACKS, 'readonly', (s) => s.getAll() as IDBRequest<InstalledPack[]>);
  return all.sort((a, b) => a.id.localeCompare(b.id));
}

export async function putInstalled(pack: InstalledPack): Promise<void> {
  await tx(PACKS, 'readwrite', (s) => s.put(pack));
}

export async function deleteInstalled(id: string): Promise<void> {
  await tx(PACKS, 'readwrite', (s) => s.delete(id));
}
