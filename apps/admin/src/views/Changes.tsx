// Diff of changed files (doc 04 §5) against the folder as it was opened. The owner
// commits with git; the admin never touches git (CLAUDE.md rule 10).
import { structuredPatch } from 'diff';
import { useState } from 'react';
import { changed, editFile, deleteFile, idOfFile, set, unsaved, useStore } from '../store';

export function Changes() {
  const s = useStore((x) => x);
  const list = changed(s);
  const dirty = new Set(unsaved(s));
  const [open, setOpen] = useState<string | null>(null);
  const shown = open && list.includes(open) ? open : (list[0] ?? null);
  if (list.length === 0) return <p className="hint page">No changes since the folder was opened.</p>;
  const before = shown ? (s.baseline[shown] ?? '') : '';
  const after = shown ? (s.files[shown] ?? '') : '';
  const patch = shown ? structuredPatch(`a/content/${shown}`, `b/content/${shown}`, before, after, '', '', { context: 3 }) : null;
  return (
    <div className="split">
      <aside className="list-pane">
        <div className="count dim">{list.length} changed file(s); commit them with git</div>
        <ul className="entries">
          {list.map((p) => {
            const state = s.baseline[p] === undefined ? 'added' : s.files[p] === undefined ? 'deleted' : 'modified';
            return (
              <li key={p} className={p === shown ? 'sel' : ''} onClick={() => setOpen(p)}>
                <span className={`pill ${state === 'deleted' ? 'bad' : state === 'added' ? 'ok' : 'dim'}`}>{state}</span>
                <span className="mono name">content/{p}</span>
                {dirty.has(p) && <span className="pill warn">unsaved</span>}
              </li>
            );
          })}
        </ul>
      </aside>
      <section className="detail-pane">
        {shown && patch && (
          <>
            <header className="card-head">
              <div>
                <h2 className="mono">content/{shown}</h2>
              </div>
              <div className="actions">
                {s.files[shown] !== undefined && idOfFile(s.files[shown] ?? '') && (
                  <button className="cmd small" onClick={() => set({ view: 'browse', selected: idOfFile(s.files[shown] ?? '') })}>
                    OPEN
                  </button>
                )}
                <button
                  className="cmd small"
                  onClick={() => {
                    const b = s.baseline[shown];
                    if (b === undefined) deleteFile(shown);
                    else editFile(shown, b);
                  }}
                >
                  REVERT FILE
                </button>
              </div>
            </header>
            <pre className="diff-view">
              {patch.hunks.map((h, i) => (
                <div key={i}>
                  <div className="hunk">
                    @@ -{h.oldStart},{h.oldLines} +{h.newStart},{h.newLines} @@
                  </div>
                  {h.lines.map((l, j) => (
                    <div key={j} className={l.startsWith('+') ? 'add' : l.startsWith('-') ? 'del' : 'ctx'}>
                      {l}
                    </div>
                  ))}
                </div>
              ))}
            </pre>
          </>
        )}
      </section>
    </div>
  );
}
