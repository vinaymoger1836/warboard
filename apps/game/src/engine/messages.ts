import type { ContactDetails, Details, Event, Order, OrderAck, SaveHeader, SideInfo, TickReport } from '@warboard/protocol';

/** The three engine files of an installed pack, by OPFS file name. */
export interface PackFiles {
  id: string;
  version: string;
  dem: string;
  terrain: string;
  territory: string;
}

/** Requests that get exactly one `reply` with the same `reqId`. */
export type WorkerRequest =
  | { type: 'openPacks'; packs: PackFiles[] }
  | { type: 'closePacks' }
  | { type: 'terrainCells'; bbox: [number, number, number, number]; max: number }
  | { type: 'point'; lat: number; lon: number }
  | { type: 'details'; id: number }
  | { type: 'contactDetails'; id: number }
  /** Ground track of an own satellite, minutes before and after now. */
  | { type: 'groundTrack'; id: number; beforeMin: number; afterMin: number }
  | { type: 'order'; order: Order }
  | { type: 'save'; slot: string }
  | { type: 'load'; slot: string }
  | { type: 'exportSave'; slot: string }
  | { type: 'importSave'; slot: string; bytes: ArrayBuffer };

/** UI thread → engine worker. */
export type ToWorker =
  | { type: 'setSpeed'; speed: number }
  /** A view buffer the UI has finished with (double buffering, doc 06 §2). */
  | { type: 'viewReturn'; buffer: ArrayBuffer }
  /** Best-effort autosave when the page is hidden or closing (doc 03 §13). */
  | { type: 'autosaveNow' }
  | (WorkerRequest & { reqId: number });

export type OrderReply = { ok: true; ack: OrderAck } | { ok: false; reason: string };

/** Reply payloads by request type. */
export interface ReplyData {
  openPacks: string;
  closePacks: null;
  /** Packed cells (see GeoQuery.terrain_cells), or null when the box holds too many. */
  terrainCells: Float64Array | null;
  /** [elevation_m, class, owner char codes…]; empty outside every pack. */
  point: number[];
  details: Details;
  contactDetails: ContactDetails;
  /** `[lon, lat]` points every minute. */
  groundTrack: [number, number][];
  order: OrderReply;
  save: SaveHeader;
  load: null;
  exportSave: ArrayBuffer;
  importSave: SaveHeader;
}

/** Who plays what, sent whenever a war starts or loads. */
export interface WarInfo {
  side: number;
  sides: SideInfo[];
  warName: string;
}

/** Engine worker → UI thread. */
export type FromWorker =
  | { type: 'ready'; report: TickReport; war: WarInfo }
  | { type: 'tick'; report: TickReport }
  | { type: 'view'; buffer: ArrayBuffer }
  | { type: 'events'; events: Event[] }
  | { type: 'autosaved'; header: SaveHeader }
  | { type: 'error'; message: string }
  | { type: 'reply'; reqId: number; ok: true; data: ReplyData[WorkerRequest['type']] }
  | { type: 'reply'; reqId: number; ok: false; message: string };
