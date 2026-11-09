import type {
  ApprovalView,
  ContactDetails,
  Details,
  Event,
  Issue,
  OperationSpec,
  OperationView,
  Order,
  OrderAck,
  PlaceKind,
  Report,
  SaveHeader,
  SideInfo,
  TickReport,
} from '@warboard/protocol';

/** Replay state (doc 03 §12): where the replay sim is and the checkpoints it can seek from. */
export type ReplayInfo =
  | { active: false }
  | {
      active: true;
      tick: number;
      sim_time_ms: number;
      end_tick: number;
      end_ms: number;
      checkpoints: { tick: number; sim_time_ms: number }[];
    };

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
  /** Open ROE approval requests of the player's side. */
  | { type: 'approvals' }
  /** Ground track of an own satellite, minutes before and after now. */
  | { type: 'groundTrack'; id: number; beforeMin: number; afterMin: number }
  | { type: 'order'; order: Order }
  /** The side's operations (doc 03 §8). */
  | { type: 'operations' }
  /** Planner validation of a plan (doc 03 §8). */
  | { type: 'checkOperation'; spec: OperationSpec }
  /** After-action report; `truth` once the war has ended (doc 03 §12). */
  | { type: 'report'; truth: boolean }
  /** The same report as printable HTML or one CSV table (`csv:<table>`). */
  | { type: 'reportText'; truth: boolean; format: string }
  | { type: 'replayStart' }
  | { type: 'replaySeek'; tick: number }
  | { type: 'replayStop' }
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
  approvals: ApprovalView[];
  /** `[lon, lat]` points every minute. */
  groundTrack: [number, number][];
  order: OrderReply;
  operations: OperationView[];
  checkOperation: Issue[];
  report: Report;
  reportText: string;
  replayStart: ReplayInfo;
  replaySeek: ReplayInfo;
  replayStop: ReplayInfo;
  save: SaveHeader;
  load: null;
  exportSave: ArrayBuffer;
  importSave: SaveHeader;
}

/** A map label from the content pack (doc 11 §2.4): geography only. */
export interface PlaceLabel {
  name: string;
  kind: PlaceKind;
  /** Nation id for nation, capital and city labels. */
  nation: string | null;
  lat: number;
  lon: number;
}

/** Who plays what, sent whenever a war starts or loads, with the map labels of its content. */
export interface WarInfo {
  side: number;
  sides: SideInfo[];
  warName: string;
  places: PlaceLabel[];
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
