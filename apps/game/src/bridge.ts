// Bridge between the UI thread and the engine worker (doc 06 §2). Side views arrive as
// transferred buffers; the bridge decodes them in place and hands the previous buffer
// back to the worker (double buffering). Map layers read views here, never via React.
import { decodeSideView, type Event, type SideView, StringTable, type TickReport } from '@warboard/protocol';
import type { FromWorker, ReplyData, ToWorker, WarInfo, WorkerRequest } from './engine/messages';

export type BridgeListener = (state: { report: TickReport | null; error: string | null }) => void;

/** A decoded view and when it arrived (performance.now()). */
export interface ViewFrame {
  view: SideView;
  receivedAt: number;
}

type Pending = { resolve: (data: unknown) => void; reject: (err: Error) => void };

export class EngineBridge {
  private readonly worker: Worker;
  /** Latest clock report (also read every frame by the map layers). */
  report: TickReport | null = null;
  private error: string | null = null;
  private readonly listeners = new Set<BridgeListener>();
  private readonly viewListeners = new Set<(f: ViewFrame) => void>();
  private readonly eventListeners = new Set<(e: Event[]) => void>();
  private readonly warListeners = new Set<(w: WarInfo) => void>();
  private readonly autosaveListeners = new Set<() => void>();
  private nextReq = 1;
  private readonly pending = new Map<number, Pending>();
  readonly strings = new StringTable();
  frame: ViewFrame | null = null;
  war: WarInfo | null = null;

  constructor() {
    this.worker = new Worker(new URL('./engine/engine.worker.ts', import.meta.url), {
      type: 'module',
      name: 'engine',
    });
    this.worker.addEventListener('message', (e: MessageEvent<FromWorker>) => this.onMessage(e.data));
    this.worker.addEventListener('error', (e) => {
      this.error = e.message || 'Engine worker failed to start';
      this.emit();
    });
  }

  private onMessage(msg: FromWorker): void {
    switch (msg.type) {
      case 'reply': {
        const p = this.pending.get(msg.reqId);
        this.pending.delete(msg.reqId);
        if (msg.ok) p?.resolve(msg.data);
        else p?.reject(new Error(msg.message));
        return;
      }
      case 'view': {
        let view: SideView;
        try {
          view = decodeSideView(msg.buffer, this.strings);
        } catch (err) {
          this.error = err instanceof Error ? err.message : String(err);
          this.emit();
          return;
        }
        const old = this.frame?.view.buffer;
        this.frame = { view, receivedAt: performance.now() };
        if (old) this.send({ type: 'viewReturn', buffer: old }, [old]);
        for (const l of this.viewListeners) l(this.frame);
        return;
      }
      case 'events':
        for (const l of this.eventListeners) l(msg.events);
        return;
      case 'autosaved':
        for (const l of this.autosaveListeners) l();
        return;
      case 'ready':
        this.strings.clear();
        this.frame = null;
        this.war = msg.war;
        this.report = msg.report;
        for (const l of this.warListeners) l(msg.war);
        this.emit();
        return;
      case 'error':
        this.error = msg.message;
        this.emit();
        return;
      case 'tick':
        this.report = msg.report;
        this.emit();
        return;
    }
  }

  setSpeed(speed: number): void {
    this.send({ type: 'setSpeed', speed });
  }

  autosaveNow(): void {
    this.send({ type: 'autosaveNow' });
  }

  /** Sends a request and resolves with the worker's reply. */
  request<T extends WorkerRequest>(req: T, transfer: Transferable[] = []): Promise<ReplyData[T['type']]> {
    const reqId = this.nextReq++;
    return new Promise((resolve, reject) => {
      this.pending.set(reqId, { resolve: resolve as (d: unknown) => void, reject });
      this.send({ ...req, reqId }, transfer);
    });
  }

  subscribe(listener: BridgeListener): () => void {
    this.listeners.add(listener);
    listener({ report: this.report, error: this.error });
    return () => this.listeners.delete(listener);
  }

  onView(listener: (f: ViewFrame) => void): () => void {
    this.viewListeners.add(listener);
    if (this.frame) listener(this.frame);
    return () => this.viewListeners.delete(listener);
  }

  onEvents(listener: (e: Event[]) => void): () => void {
    this.eventListeners.add(listener);
    return () => this.eventListeners.delete(listener);
  }

  onWar(listener: (w: WarInfo) => void): () => void {
    this.warListeners.add(listener);
    if (this.war) listener(this.war);
    return () => this.warListeners.delete(listener);
  }

  onAutosave(listener: () => void): () => void {
    this.autosaveListeners.add(listener);
    return () => this.autosaveListeners.delete(listener);
  }

  dispose(): void {
    this.worker.terminate();
    this.listeners.clear();
    for (const p of this.pending.values()) p.reject(new Error('Engine stopped'));
    this.pending.clear();
  }

  private send(msg: ToWorker, transfer: Transferable[] = []): void {
    this.worker.postMessage(msg, { transfer });
  }

  private emit(): void {
    for (const l of this.listeners) l({ report: this.report, error: this.error });
  }
}
