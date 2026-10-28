// Pack manager panel (doc 06 §8): install, update, remove, re-download after eviction,
// storage quota and persistence.
import type { PackManager, PackState } from './manager';
import type { PackStatus } from './types';

const MB = 1024 * 1024;

function size(bytes: number): string {
  return bytes >= 1024 * MB ? `${(bytes / 1024 / MB).toFixed(1)} GB` : `${Math.round(bytes / MB).toLocaleString('en-GB')} MB`;
}

function statusText(s: PackStatus | undefined): string {
  switch (s?.kind) {
    case 'installed':
      return 'Installed';
    case 'installing':
      return `Installing ${s.file}`;
    case 'update':
      return `Update available (installed ${s.from})`;
    case 'evicted':
      return 'Files missing: the browser removed them';
    case 'error':
      return s.message;
    case 'available':
      return 'Not installed';
    default:
      return '';
  }
}

export function PackPanel({ manager, state, onClose }: { manager: PackManager; state: PackState; onClose: () => void }) {
  const ids = [...new Set([...(state.catalog ?? []).map((c) => c.id), ...state.installed.map((p) => p.id)])];
  const busy = Object.values(state.status).some((s) => s.kind === 'installing');
  return (
    <section className="panel pack-panel" aria-labelledby="pack-panel-title">
      <h2 id="pack-panel-title" className="panel-header">
        Region packs
        <button type="button" className="close" aria-label="Close region packs" onClick={onClose}>
          ×
        </button>
      </h2>
      <div className="pack-body">
        <p className="muted">
          Packs are downloaded once, checked against their BLAKE3 hashes and kept in browser storage, so the map works
          offline. A war needs the world pack and the pack of its theatre.
        </p>
        {state.catalogError && <p className="warn">{state.catalogError}</p>}
        <ul className="pack-list">
          {ids.map((id) => {
            const entry = state.catalog?.find((c) => c.id === id);
            const inst = state.installed.find((p) => p.id === id);
            const s = state.status[id];
            const name = entry?.name ?? inst?.manifest.name ?? id;
            const version = entry?.version ?? inst?.version ?? '';
            const bytes = entry?.size ?? inst?.manifest.size ?? 0;
            return (
              <li key={id} className="pack-row">
                <div className="pack-info">
                  <span className="pack-name">{name}</span>
                  <span className="pack-meta">
                    {version} · {size(bytes)}
                  </span>
                  <span className={s?.kind === 'error' || s?.kind === 'evicted' ? 'pack-status bad' : 'pack-status'}>
                    {statusText(s)}
                  </span>
                  {s?.kind === 'installing' && (
                    <div className="progress" role="progressbar" aria-valuemin={0} aria-valuemax={s.total} aria-valuenow={s.done}>
                      <div style={{ width: `${(100 * s.done) / Math.max(1, s.total)}%` }} />
                    </div>
                  )}
                </div>
                <div className="pack-actions">
                  {entry && (s?.kind === 'available' || s?.kind === 'error' || s?.kind === 'evicted' || s?.kind === 'update') && (
                    <button type="button" className="cmd" disabled={busy} onClick={() => void manager.install(id)}>
                      {s.kind === 'update' ? 'UPDATE' : s.kind === 'evicted' ? 'RE-DOWNLOAD' : 'INSTALL'}
                    </button>
                  )}
                  {inst && s?.kind !== 'installing' && (
                    <button type="button" className="cmd" disabled={busy} onClick={() => void manager.remove(id)}>
                      REMOVE
                    </button>
                  )}
                </div>
              </li>
            );
          })}
        </ul>
        <p className="storage">
          {state.usage
            ? `Storage: ${size(state.usage.usage)} used of ${size(state.usage.quota)}`
            : 'Storage: usage unknown'}
          {' · '}
          {state.persisted === true
            ? 'kept by the browser'
            : state.persisted === false
              ? 'the browser may clear it when space runs low'
              : 'persistence unknown'}
        </p>
      </div>
    </section>
  );
}
