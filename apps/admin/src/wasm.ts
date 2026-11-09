// wb-content compiled to WebAssembly (ADR-010): the same check and compiler as
// `wb content check` and `wb content build`.
import init, { TestBench, build_content_pack, check_content, content_pack_info } from '@warboard/wb-wasm';
import wasmUrl from '@warboard/wb-wasm/wb_wasm_bg.wasm?url';

export interface Problem {
  file: string;
  path: string;
  message: string;
}

export interface BandPosition {
  field: string;
  min: number;
  max: number;
  value: number | null;
}

export interface EntryInfo {
  id: string;
  kind: string;
  /** Path relative to content/. */
  file: string;
  custom: boolean;
  extends: string | null;
  archetype: string | null;
  /** Resolved fields after extends and overrides. */
  value: Record<string, unknown>;
  bands: BandPosition[];
}

export interface Report {
  ok: boolean;
  content_version: string | null;
  errors: Problem[];
  warnings: Problem[];
  entries: EntryInfo[];
}

export interface PackInfo {
  content_hash: string;
  content_version: string;
  schema_version: number;
}

let ready: Promise<unknown> | null = null;

export function wasmReady(): Promise<unknown> {
  ready ??= init({ module_or_path: wasmUrl });
  return ready;
}

function files(contents: Record<string, string>): string {
  return JSON.stringify(Object.entries(contents).map(([path, text]) => ({ path, text })));
}

export async function checkContent(contents: Record<string, string>): Promise<Report> {
  await wasmReady();
  return JSON.parse(check_content(files(contents))) as Report;
}

export async function buildPack(contents: Record<string, string>): Promise<{ bytes: Uint8Array; info: PackInfo }> {
  await wasmReady();
  const bytes = build_content_pack(files(contents));
  return { bytes, info: JSON.parse(content_pack_info(bytes)) as PackInfo };
}

// ---- Test bench ------------------------------------------------------------------------

export interface BenchInfo {
  id: string;
  description: string | null;
  variants: string[];
  seeds: number;
  max_ticks: number;
}

export interface BenchRun {
  variant: string;
  n: number;
  ticks: number;
  sim_time_ms: number;
  hash: string;
}

export interface BenchCheck {
  variant: string;
  metric: string;
  subject: string;
  label: string;
  aggregate: string;
  /** Per-seed values (null: the metric never happened in that run). */
  values: (number | null)[];
  value: number | null;
  min: number | null;
  max: number | null;
  ok: boolean;
}

export interface BenchReport {
  runs: BenchRun[];
  checks: BenchCheck[];
}

/** A test-bench scenario running seed by seed against the edited content (ADR-045). */
export class Bench {
  private constructor(private readonly tb: TestBench) {}

  static async create(scenarioYaml: string, pack: Uint8Array, seeds: number): Promise<Bench> {
    await wasmReady();
    return new Bench(TestBench.create(scenarioYaml, pack, seeds));
  }

  info(): BenchInfo {
    return JSON.parse(this.tb.info()) as BenchInfo;
  }

  remaining(): number {
    return this.tb.remaining();
  }

  step(n: number): number {
    return this.tb.step(n);
  }

  report(): BenchReport {
    return JSON.parse(this.tb.report()) as BenchReport;
  }

  free(): void {
    this.tb.free();
  }
}
