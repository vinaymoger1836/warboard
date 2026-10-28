// Save slots (doc 03 §13, doc 06 §1 `saves`): the autosave and three manual slots; save,
// load, export and import `.wbsave` files.
import { useCallback, useEffect, useRef, useState } from 'react';
import type { EngineBridge } from '../bridge';
import { AUTOSAVE_SLOT, deleteSave, listSaves, MANUAL_SLOTS, type SaveMeta } from '../saves/store';
import { ui } from '../store';
import { elapsed, utcClock } from './format';
import { CloseGlyph } from './glyphs';

interface Props {
  bridge: EngineBridge;
  toast: (t: string) => void;
  onClose: () => void;
}

const SLOTS = [AUTOSAVE_SLOT, ...MANUAL_SLOTS];
const slotName = (slot: string) => (slot === AUTOSAVE_SLOT ? 'Autosave' : `Slot ${slot.slice(5)}`);

export function SavesPanel({ bridge, toast, onClose }: Props) {
  const [saves, setSaves] = useState<SaveMeta[]>([]);
  const [busy, setBusy] = useState(false);
  const importSlot = useRef<string>(MANUAL_SLOTS[0]);
  const file = useRef<HTMLInputElement>(null);
  const refresh = useCallback(() => {
    listSaves()
      .then(setSaves)
      .catch((err: unknown) => toast(String(err)));
  }, [toast]);
  useEffect(() => {
    refresh();
    return bridge.onAutosave(refresh);
  }, [bridge, refresh]);

  const run = async (what: string, job: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await job();
      toast(what);
    } catch (err) {
      toast(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
      refresh();
    }
  };
  const exportSlot = (slot: string) =>
    run('Save exported', async () => {
      const bytes = await bridge.request({ type: 'exportSave', slot });
      const url = URL.createObjectURL(new Blob([bytes], { type: 'application/octet-stream' }));
      const a = document.createElement('a');
      a.href = url;
      a.download = `warboard-${slot}.wbsave`;
      a.click();
      setTimeout(() => URL.revokeObjectURL(url), 1_000);
    });

  return (
    <div className="saves-panel panel" role="dialog" aria-label="Saves">
      <h2 className="panel-header">
        Saves
        <button type="button" className="close" aria-label="Close saves" onClick={onClose}>
          <CloseGlyph />
        </button>
      </h2>
      <div className="pack-body">
        <p className="muted">The war autosaves every 5 sim minutes and when you close the page.</p>
        <ul className="pack-list">
          {SLOTS.map((slot) => {
            const s = saves.find((x) => x.slot === slot);
            return (
              <li key={slot} className="pack-row">
                <div className="pack-info">
                  <span className="pack-name">{slotName(slot)}</span>
                  {s ? (
                    <span className="pack-meta">
                      {s.header.war_name} · {utcClock(s.header.start_utc_ms + s.header.sim_time_ms)}{' '}
                      {elapsed(s.header.sim_time_ms)} · saved {new Date(s.header.created_utc_ms).toLocaleString('en-GB')}
                    </span>
                  ) : (
                    <span className="pack-status">Empty</span>
                  )}
                </div>
                <div className="pack-actions">
                  {slot !== AUTOSAVE_SLOT && (
                    <button type="button" className="cmd" disabled={busy} onClick={() => void run('Saved', () => bridge.request({ type: 'save', slot }))}>
                      SAVE
                    </button>
                  )}
                  <button
                    type="button"
                    className="cmd"
                    disabled={busy || !s}
                    onClick={() =>
                      void run('Loaded', async () => {
                        ui.clearLog();
                        await bridge.request({ type: 'load', slot });
                      })
                    }
                  >
                    LOAD
                  </button>
                  <button type="button" className="cmd" disabled={busy || !s} onClick={() => void exportSlot(slot)}>
                    EXPORT
                  </button>
                  {slot !== AUTOSAVE_SLOT && (
                    <button
                      type="button"
                      className="cmd"
                      disabled={busy}
                      onClick={() => {
                        importSlot.current = slot;
                        file.current?.click();
                      }}
                    >
                      IMPORT
                    </button>
                  )}
                  {s && (
                    <button type="button" className="cmd" disabled={busy} onClick={() => void run('Deleted', () => deleteSave(slot))}>
                      DELETE
                    </button>
                  )}
                </div>
              </li>
            );
          })}
        </ul>
        <input
          ref={file}
          type="file"
          accept=".wbsave"
          hidden
          onChange={(e) => {
            const f = e.target.files?.[0];
            e.target.value = '';
            if (!f) return;
            void run('Save imported', async () => {
              const bytes = await f.arrayBuffer();
              await bridge.request({ type: 'importSave', slot: importSlot.current, bytes }, [bytes]);
            });
          }}
        />
      </div>
    </div>
  );
}
