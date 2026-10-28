// OPFS synchronous access handles (dedicated workers only). The DOM lib used by this
// app does not declare them; these match the WHATWG File System spec.

interface FileSystemReadWriteOptions {
  at?: number;
}

interface FileSystemSyncAccessHandle {
  read(buffer: ArrayBufferView, options?: FileSystemReadWriteOptions): number;
  write(buffer: ArrayBufferView, options?: FileSystemReadWriteOptions): number;
  truncate(newSize: number): void;
  getSize(): number;
  flush(): void;
  close(): void;
}

interface FileSystemFileHandle {
  createSyncAccessHandle(): Promise<FileSystemSyncAccessHandle>;
}
