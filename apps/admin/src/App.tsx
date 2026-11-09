// Admin shell (doc 04 §5): open the repo folder, switch views, save, build the pack.
import { useEffect, useState } from 'react';
import { Repo, lastRepo } from './repo';
import { changed, openRepo, saveAll, set, unsaved, useStore, type View } from './store';
import { buildPack } from './wasm';
import { Browse } from './views/Browse';
import { Changes } from './views/Changes';
import { Compare } from './views/Compare';
import { Inventories } from './views/Inventories';
import { NationsWorld } from './views/NationsWorld';
import { Problems } from './views/Problems';
import { TestBench } from './views/TestBench';

const VIEWS: [View, string][] = [
  ['browse', 'Catalogue'],
  ['world', 'Nations and world'],
  ['inventories', 'Inventories'],
  ['compare', 'Compare'],
  ['bench', 'Test bench'],
  ['changes', 'Changes'],
  ['problems', 'Problems'],
];

export function App() {
  const s = useStore((x) => x);
  const [remembered, setRemembered] = useState<FileSystemDirectoryHandle | null>(null);
  const supported = 'showDirectoryPicker' in window;
  const nUnsaved = unsaved(s).length;
  const nChanged = changed(s).length;

  useEffect(() => {
    if (supported) void lastRepo().then(setRemembered).catch(() => undefined);
  }, [supported]);

  useEffect(() => {
    const warn = (e: BeforeUnloadEvent) => {
      if (unsaved().length > 0) e.preventDefault();
    };
    window.addEventListener('beforeunload', warn);
    return () => window.removeEventListener('beforeunload', warn);
  }, []);

  const guard = async (what: () => Promise<void>) => {
    try {
      await what();
    } catch (e) {
      set({ status: e instanceof Error ? e.message : String(e) });
    }
  };

  const open = (r: () => Promise<Repo>) => guard(async () => openRepo(await r()));

  const build = () =>
    guard(async () => {
      if (!s.repo) return;
      if (s.report && !s.report.ok) {
        set({ status: `Not built: ${s.report.errors.length} error(s) in the content` });
        return;
      }
      const { bytes, info } = await buildPack(s.files);
      await s.repo.writeBuild('content.wbpack', bytes);
      set({ status: `Wrote build/content.wbpack (${bytes.length} bytes, hash ${info.content_hash.slice(0, 12)}…)` });
    });

  if (!s.repo) {
    return (
      <div className="app">
        <header className="top-bar">
          <span className="brand">WARBOARD · CONTENT EDITOR</span>
        </header>
        <div className="welcome">
          <h1>Content editor</h1>
          <p className="hint">{supported ? s.status : 'This editor needs a Chromium browser (File System Access API).'}</p>
          {supported && (
            <div className="row">
              <button className="cmd" onClick={() => void open(() => Repo.pick())}>
                OPEN REPO FOLDER
              </button>
              {remembered && (
                <button className="cmd" onClick={() => void open(() => Repo.reopen(remembered))}>
                  REOPEN {remembered.name.toUpperCase()}
                </button>
              )}
            </div>
          )}
        </div>
      </div>
    );
  }

  const nErr = s.report?.errors.length ?? 0;
  const nWarn = s.report?.warnings.length ?? 0;
  return (
    <div className="app">
      <header className="top-bar">
        <span className="brand">WARBOARD · CONTENT EDITOR</span>
        <nav className="views">
          {VIEWS.map(([v, label]) => (
            <button key={v} className={s.view === v ? 'tab on' : 'tab'} onClick={() => set({ view: v })}>
              {label}
              {v === 'changes' && nChanged > 0 && <span className="pill dim">{nChanged}</span>}
              {v === 'problems' && nErr > 0 && <span className="pill bad">{nErr}</span>}
              {v === 'problems' && nErr === 0 && nWarn > 0 && <span className="pill warn">{nWarn}</span>}
            </button>
          ))}
        </nav>
        <span className="spacer" />
        <button className="cmd small" disabled={nUnsaved === 0} onClick={() => void guard(async () => void (await saveAll()))}>
          SAVE {nUnsaved > 0 ? `(${nUnsaved})` : ''}
        </button>
        <button className="cmd small" disabled={nErr > 0 || s.checking} onClick={() => void build()}>
          BUILD PACK
        </button>
      </header>
      <main className="main">
        {s.view === 'browse' && <Browse />}
        {s.view === 'world' && <NationsWorld />}
        {s.view === 'inventories' && <Inventories />}
        {s.view === 'compare' && <Compare />}
        {s.view === 'bench' && <TestBench />}
        {s.view === 'changes' && <Changes />}
        {s.view === 'problems' && <Problems />}
      </main>
      <footer className="status-bar">
        <span className={nErr > 0 ? 'bad' : s.checking ? 'dim' : 'ok'}>
          {s.checking ? 'Checking…' : nErr > 0 ? `${nErr} error(s)` : 'Check passes'}
        </span>
        <span className="dim">{s.status}</span>
      </footer>
    </div>
  );
}
