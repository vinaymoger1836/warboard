// Admin state: the content files as edited, as saved on disk, and as they were when the
// folder was opened (for the diff), plus the live check report.
import { useSyncExternalStore } from 'react';
import { Repo } from './repo';
import { checkContent, type EntryInfo, type Report } from './wasm';

export type View = 'browse' | 'world' | 'inventories' | 'compare' | 'bench' | 'changes' | 'problems';

export interface State {
  repo: Repo | null;
  /** Content files as edited: path relative to content/ → text. */
  files: Record<string, string>;
  /** As on disk (last load or save). */
  disk: Record<string, string>;
  /** As when the folder was opened. */
  baseline: Record<string, string>;
  bench: Record<string, string>;
  report: Report | null;
  /** Entries of the last report without errors (band bars stay while a file is broken). */
  good: Map<string, EntryInfo>;
  checking: boolean;
  view: View;
  selected: string | null;
  compare: [string | null, string | null];
  status: string;
}

let state: State = {
  repo: null,
  files: {},
  disk: {},
  baseline: {},
  bench: {},
  report: null,
  good: new Map(),
  checking: false,
  view: 'browse',
  selected: null,
  compare: [null, null],
  status: 'Open the WarBoard repo folder to start',
};

const listeners = new Set<() => void>();

export function get(): State {
  return state;
}

export function set(patch: Partial<State>): void {
  state = { ...state, ...patch };
  for (const l of listeners) l();
}

function subscribe(l: () => void): () => void {
  listeners.add(l);
  return () => listeners.delete(l);
}

/** Subscribes to the whole state (a stable object until `set`) and picks in render. */
export function useStore<T>(pick: (s: State) => T): T {
  const s = useSyncExternalStore(subscribe, get);
  return pick(s);
}

// ---- Live check ------------------------------------------------------------------------

let timer: ReturnType<typeof setTimeout> | null = null;
let generation = 0;

export function scheduleCheck(delay = 350): void {
  if (timer) clearTimeout(timer);
  timer = setTimeout(() => void runCheck(), delay);
}

async function runCheck(): Promise<void> {
  const gen = ++generation;
  set({ checking: true });
  try {
    const report = await checkContent(state.files);
    if (gen !== generation) return;
    const good = report.ok ? new Map(report.entries.map((e) => [e.id, e])) : state.good;
    set({ report, good, checking: false });
  } catch (e) {
    set({ checking: false, status: `Check failed: ${String(e)}` });
  }
}

// ---- Files -----------------------------------------------------------------------------

export async function openRepo(repo: Repo): Promise<void> {
  set({ status: `Reading ${repo.name}…` });
  const [files, bench] = await Promise.all([repo.readContent(), repo.readBench()]);
  set({
    repo,
    files,
    disk: { ...files },
    baseline: { ...files },
    bench,
    status: `${repo.name}: ${Object.keys(files).length} content files`,
    selected: null,
  });
  scheduleCheck(0);
}

export function editFile(path: string, text: string): void {
  set({ files: { ...state.files, [path]: text } });
  scheduleCheck();
}

export function deleteFile(path: string): void {
  const files = { ...state.files };
  delete files[path];
  set({ files });
  scheduleCheck();
}

/** Files whose edited text differs from disk (deleted files included). */
export function unsaved(s: State = state): string[] {
  const paths = new Set([...Object.keys(s.files), ...Object.keys(s.disk)]);
  return [...paths].filter((p) => s.files[p] !== s.disk[p]).sort();
}

/** Files that differ from the folder as opened. */
export function changed(s: State = state): string[] {
  const paths = new Set([...Object.keys(s.files), ...Object.keys(s.baseline)]);
  return [...paths].filter((p) => s.files[p] !== s.baseline[p]).sort();
}

export async function saveAll(): Promise<number> {
  const repo = state.repo;
  if (!repo) return 0;
  const list = unsaved();
  for (const p of list) {
    const text = state.files[p];
    if (text === undefined) await repo.removeContent(p);
    else await repo.writeContent(p, text);
  }
  set({ disk: { ...state.files }, status: `Saved ${list.length} file(s)` });
  return list.length;
}

// ---- Lookups ---------------------------------------------------------------------------

/** Entry id → file path, from the YAML `id:` line (works while the check fails). */
export function idOfFile(text: string): string | null {
  return /^id:\s*["']?([a-z0-9-]+)/m.exec(text)?.[1] ?? null;
}

export function kindOfFile(text: string): string | null {
  return /^kind:\s*([a-z_]+)/m.exec(text)?.[1] ?? null;
}

export function fileOfId(id: string, s: State = state): string | null {
  const e = s.report?.entries.find((x) => x.id === id) ?? s.good.get(id);
  if (e && s.files[e.file] !== undefined) return e.file;
  for (const [p, t] of Object.entries(s.files)) if (idOfFile(t) === id) return p;
  return null;
}

export function entry(id: string, s: State = state): EntryInfo | undefined {
  return s.report?.entries.find((x) => x.id === id) ?? s.good.get(id);
}

export function entriesOf(kind: string, s: State = state): EntryInfo[] {
  const list = s.report?.ok ? s.report.entries : [...s.good.values()];
  return list.filter((e) => e.kind === kind);
}

export function nameOf(e: EntryInfo | undefined): string {
  const n = e?.value['name'];
  return typeof n === 'string' ? n : (e?.id ?? '');
}
