// Engine worker (ADR-002): owns the WebAssembly engine, advances it in a loop
// and posts a tick report to the UI about 10 times a second.
import type { TickReport } from '@warboard/protocol';
import init, { Engine } from '@warboard/wb-wasm';
import wasmUrl from '@warboard/wb-wasm/wb_wasm_bg.wasm?url';
import scenarioYaml from '../../../../scenarios/empty.yaml?raw';
import type { FromWorker, ToWorker } from './messages';

const FRAME_MS = 16;
const REPORT_MS = 100;
/** Window over which achieved speed is measured. */
const SPEED_WINDOW_MS = 1_000;

let speed = 1;

const post = (msg: FromWorker) => self.postMessage(msg);

self.addEventListener('message', (e: MessageEvent<ToWorker>) => {
  if (e.data.type === 'setSpeed') speed = e.data.speed;
});

async function start(): Promise<void> {
  await init({ module_or_path: wasmUrl });
  const engine = Engine.create(scenarioYaml);

  const samples: { realMs: number; simMs: number }[] = [];
  const report = (): TickReport => {
    const first = samples[0];
    const last = samples[samples.length - 1];
    const realSpan = first && last ? last.realMs - first.realMs : 0;
    return {
      tick: engine.tick(),
      simTimeMs: engine.sim_time_ms(),
      startUtcMs: engine.start_utc_ms(),
      tickLenMs: engine.tick_len_ms(),
      requestedSpeed: speed,
      achievedSpeed: first && last && realSpan > 0 ? (last.simMs - first.simMs) / realSpan : 0,
      stateHash: engine.state_hash(),
    };
  };

  post({ type: 'ready', report: report() });

  let lastFrame = performance.now();
  let lastReport = lastFrame;
  const loop = () => {
    const now = performance.now();
    engine.advance(now - lastFrame, speed);
    lastFrame = now;

    samples.push({ realMs: now, simMs: engine.sim_time_ms() });
    while (samples.length > 2 && now - (samples[0]?.realMs ?? now) > SPEED_WINDOW_MS) samples.shift();

    if (now - lastReport >= REPORT_MS) {
      lastReport = now;
      post({ type: 'tick', report: report() });
    }
    setTimeout(loop, FRAME_MS);
  };
  loop();
}

start().catch((err: unknown) => {
  post({ type: 'error', message: err instanceof Error ? err.message : String(err) });
});
