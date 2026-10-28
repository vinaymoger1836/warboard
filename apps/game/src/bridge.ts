// Bridge between the UI thread and the engine worker (doc 06 §2).
import type { TickReport } from '@warboard/protocol';
import type { FromWorker, ReplyData, ToWorker, WorkerRequest } from './engine/messages';

export type BridgeListener = (state: { report: TickReport | null; error: string | null }) => void;

type Pending = { resolve: (data: unknown) => void; reject: (err: Error) => void };

export class EngineBridge {
  private readonly worker: Worker;
  private report: TickReport | null = null;
  private error: string | null = null;
  private readonly listeners = new Set<BridgeListener>();
  private nextReq = 1;
  private readonly pending = new Map<number, Pending>();

  constructor() {
    this.worker = new Worker(new URL('./engine/engine.worker.ts', import.meta.url), {
      type: 'module',
      name: 'engine',
    });
    this.worker.addEventListener('message', (e: MessageEvent<FromWorker>) => {
      const msg = e.data;
      if (msg.type === 'reply') {
        const p = this.pending.get(msg.reqId);
        this.pending.delete(msg.reqId);
        if (msg.ok) p?.resolve(msg.data);
        else p?.reject(new Error(msg.message));
        return;
      }
      if (msg.type === 'error') this.error = msg.message;
      else this.report = msg.report;
      this.emit();
    });
    this.worker.addEventListener('error', (e) => {
      this.error = e.message || 'Engine worker failed to start';
      this.emit();
    });
  }

  setSpeed(speed: number): void {
    this.send({ type: 'setSpeed', speed });
  }

  /** Sends a request and resolves with the worker's reply. */
  request<T extends WorkerRequest>(req: T): Promise<ReplyData[T['type']]> {
    const reqId = this.nextReq++;
    return new Promise((resolve, reject) => {
      this.pending.set(reqId, { resolve: resolve as (d: unknown) => void, reject });
      this.send({ ...req, reqId });
    });
  }

  subscribe(listener: BridgeListener): () => void {
    this.listeners.add(listener);
    listener({ report: this.report, error: this.error });
    return () => this.listeners.delete(listener);
  }

  dispose(): void {
    this.worker.terminate();
    this.listeners.clear();
    for (const p of this.pending.values()) p.reject(new Error('Engine stopped'));
    this.pending.clear();
  }

  private send(msg: ToWorker): void {
    this.worker.postMessage(msg);
  }

  private emit(): void {
    for (const l of this.listeners) l({ report: this.report, error: this.error });
  }
}
