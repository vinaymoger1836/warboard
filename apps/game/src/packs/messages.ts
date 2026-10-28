import type { PackFileEntry } from './types';

/** UI thread → pack worker. */
export type ToPackWorker = { type: 'install'; id: string; baseUrl: string; files: PackFileEntry[] };

/** Pack worker → UI thread. */
export type FromPackWorker =
  | { type: 'progress'; id: string; file: string; done: number; total: number }
  | { type: 'done'; id: string }
  | { type: 'error'; id: string; message: string };
