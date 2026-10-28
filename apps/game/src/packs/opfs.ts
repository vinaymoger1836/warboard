// Pack files in the Origin Private File System: /packs/<id>/<file> (doc 06 §8).

export async function packsRoot(): Promise<FileSystemDirectoryHandle> {
  const root = await navigator.storage.getDirectory();
  return root.getDirectoryHandle('packs', { create: true });
}

export async function packDir(id: string, create = false): Promise<FileSystemDirectoryHandle | null> {
  try {
    return await (await packsRoot()).getDirectoryHandle(id, { create });
  } catch {
    return null;
  }
}

/** The stored file, or null if it is missing (never installed, or evicted). */
export async function packFile(id: string, name: string): Promise<File | null> {
  const dir = await packDir(id);
  if (!dir) return null;
  try {
    return await (await dir.getFileHandle(name)).getFile();
  } catch {
    return null;
  }
}

export async function removePackDir(id: string): Promise<void> {
  try {
    await (await packsRoot()).removeEntry(id, { recursive: true });
  } catch {
    // Already gone.
  }
}
