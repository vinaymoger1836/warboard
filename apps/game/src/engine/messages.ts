import type { TickReport } from '@warboard/protocol';

/** The three engine files of an installed pack, by OPFS file name. */
export interface PackFiles {
  id: string;
  dem: string;
  terrain: string;
  territory: string;
}

/** Requests that get exactly one `reply` with the same `reqId`. */
export type WorkerRequest =
  | { type: 'openPacks'; packs: PackFiles[] }
  | { type: 'closePacks' }
  | { type: 'terrainCells'; bbox: [number, number, number, number]; max: number }
  | { type: 'point'; lat: number; lon: number };

/** UI thread → engine worker. */
export type ToWorker = { type: 'setSpeed'; speed: number } | (WorkerRequest & { reqId: number });

/** Reply payloads by request type. */
export interface ReplyData {
  openPacks: string;
  closePacks: null;
  /** Packed cells (see GeoQuery.terrain_cells), or null when the box holds too many. */
  terrainCells: Float64Array | null;
  /** [elevation_m, class, owner char codes…]; empty outside every pack. */
  point: number[];
}

/** Engine worker → UI thread. */
export type FromWorker =
  | { type: 'ready'; report: TickReport }
  | { type: 'tick'; report: TickReport }
  | { type: 'error'; message: string }
  | { type: 'reply'; reqId: number; ok: true; data: ReplyData[WorkerRequest['type']] }
  | { type: 'reply'; reqId: number; ok: false; message: string };
