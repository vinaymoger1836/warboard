import type { TickReport } from '@warboard/protocol';

/** UI thread → engine worker. */
export type ToWorker = { type: 'setSpeed'; speed: number };

/** Engine worker → UI thread. */
export type FromWorker =
  | { type: 'ready'; report: TickReport }
  | { type: 'tick'; report: TickReport }
  | { type: 'error'; message: string };
