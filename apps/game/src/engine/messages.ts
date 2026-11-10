import type {
  ApprovalView,
  ContactDetails,
  DeploymentView,
  Details,
  Event,
  HotSeat,
  Issue,
  NationOption,
  OperationSpec,
  OperationView,
  Order,
  OrderAck,
  PlaceKind,
  PlayMode,
  Report,
  SaveHeader,
  SessionView,
  SetupReport,
  SideInfo,
  TickReport,
  WarSetup,
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

/** A preview of a war setup (doc 12 §4): refused with a reason, or the forces with no base and
 *  the area the region packs must cover (`[west, south, east, north]`, ADR-022). */
export type SetupPreview =
  | { ok: false; error: string }
  | { ok: true; report: SetupReport; extent: [number, number, number, number]; mode: string };

/** PIN for a hot-seat side, set when the war starts (doc 12 §3). */
export interface PinChoice {
  side: number;
  pin: string;
}

/** Requests that get exactly one `reply` with the same `reqId`. Requests about one side take
 *  `side`; without it the worker uses the active side. */
export type WorkerRequest =
  | { type: 'openPacks'; packs: PackFiles[] }
  /** The nations of the content pack for the coalition builder. */
  | { type: 'nationOptions' }
  | { type: 'previewSetup'; setup: WarSetup }
  /** Starts a war from the setup screens. */
  | { type: 'newWar'; setup: WarSetup; hotSeat?: HotSeat; pins?: PinChoice[] }
  /** Starts the build's quick-start war (Human vs AI). */
  | { type: 'quickStart' }
  /** Orders and the picture move to a side (doc 12 §2.1). */
  | { type: 'setActiveSide'; side: number }
  /** Look at a side's picture or the umpire view (255) without changing who gives orders. */
  | { type: 'setViewSide'; side: number }
  | { type: 'setUmpire'; on: boolean }
  | { type: 'setPin'; side: number; pin: string }
  | { type: 'checkPin'; side: number; pin: string }
  | { type: 'deployment'; side?: number }
  | { type: 'closePacks' }
  | { type: 'terrainCells'; bbox: [number, number, number, number]; max: number }
  | { type: 'point'; lat: number; lon: number }
  /** Heights of one 3D terrain tile, `size × size` from the north-west corner (doc 06 §5). */
  | { type: 'heightTile'; bbox: [number, number, number, number]; size: number }
  | { type: 'details'; id: number; side?: number }
  | { type: 'contactDetails'; id: number; side?: number }
  /** Open ROE approval requests of the player's side. */
  | { type: 'approvals'; side?: number }
  /** Ground track of an own satellite, minutes before and after now. */
  | { type: 'groundTrack'; id: number; beforeMin: number; afterMin: number; side?: number }
  | { type: 'order'; order: Order }
  /** The side's operations (doc 03 §8). */
  | { type: 'operations'; side?: number }
  /** Planner validation of a plan (doc 03 §8). */
  | { type: 'checkOperation'; spec: OperationSpec }
  /** After-action report; `truth` once the war has ended (doc 03 §12). */
  | { type: 'report'; truth: boolean; side?: number }
  /** The same report as printable HTML or one CSV table (`csv:<table>`). */
  | { type: 'reportText'; truth: boolean; format: string; side?: number }
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
  nationOptions: NationOption[];
  previewSetup: SetupPreview;
  newWar: null;
  quickStart: null;
  setActiveSide: null;
  setViewSide: null;
  setUmpire: null;
  setPin: null;
  checkPin: boolean;
  deployment: DeploymentView;
  closePacks: null;
  /** Packed cells (see GeoQuery.terrain_cells), or null when the box holds too many. */
  terrainCells: Float64Array | null;
  /** [elevation_m, class, owner char codes…]; empty outside every pack. */
  point: number[];
  /** Metres; null without packs (the globe stays an ellipsoid). */
  heightTile: Float32Array | null;
  details: Details;
  contactDetails: ContactDetails;
  approvals: ApprovalView[];
  /** `[lon, lat, alt_m]` per minute. */
  groundTrack: [number, number, number][];
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
  /** The active side: whose orders the client gives. */
  side: number;
  mode: PlayMode;
  sides: SideInfo[];
  session: SessionView;
  warName: string;
  places: PlaceLabel[];
  /** Every nation of the content (flags, names). */
  nations: NationOption[];
  /** Some side deploys itself: the clock is held until it has finished (doc 03 §2). */
  deploying: boolean;
}

/** Engine worker → UI thread. */
export type FromWorker =
  | { type: 'ready'; report: TickReport; war: WarInfo }
  | { type: 'tick'; report: TickReport }
  | { type: 'view'; buffer: ArrayBuffer }
  /** Events one side may see; each human side's stream is kept apart (doc 12 §2). */
  | { type: 'events'; side: number; events: Event[] }
  | { type: 'autosaved'; header: SaveHeader }
  | { type: 'error'; message: string }
  | { type: 'reply'; reqId: number; ok: true; data: ReplyData[WorkerRequest['type']] }
  | { type: 'reply'; reqId: number; ok: false; message: string };
