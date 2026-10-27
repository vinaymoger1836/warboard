// Bridge between the UI thread and the engine worker (doc 06 §2).
import type { TickReport } from '@warboard/protocol';
import type { FromWorker, ToWorker } from './engine/messages';

export type BridgeListener = (state: { report: TickReport | null; error: string | null }) => void;

export class EngineBridge {
  private readonly worker: Worker;
  private report: TickReport | null = null;
  private error: string | null = null;
  private readonly listeners = new Set<BridgeListener>();

  constructor() {
    this.worker = new Worker(new URL('./engine/engine.worker.ts', import.meta.url), {
      type: 'module',
      name: 'engine',
    });
    this.worker.addEventListener('message', (e: MessageEvent<FromWorker>) => {
      const msg = e.data;
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

  subscribe(listener: BridgeListener): () => void {
    this.listeners.add(listener);
    listener({ report: this.report, error: this.error });
    return () => this.listeners.delete(listener);
  }

  dispose(): void {
    this.worker.terminate();
    this.listeners.clear();
  }

  private send(msg: ToWorker): void {
    this.worker.postMessage(msg);
  }

  private emit(): void {
    for (const l of this.listeners) l({ report: this.report, error: this.error });
  }
}
