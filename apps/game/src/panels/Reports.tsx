// After-action reports (doc 03 §12): totals, operations, salvos, losses, our hits,
// munitions, sorties and the intel log of our side; the truth review once the war has
// ended. Exports: JSON, one CSV per table, and a printable HTML page, all made by the
// engine (wb-sim reports.rs) so the browser and `wb report` give the same files.
import type { Report } from '@warboard/protocol';
import { useCallback, useEffect, useState } from 'react';
import type { EngineBridge } from '../bridge';
import { sendCommand } from '../orders';
import { ui } from '../store';
import { elapsed } from './format';
import { CloseGlyph } from './glyphs';

const TABS = [
  ['summary', 'Summary'],
  ['operations', 'Operations'],
  ['salvos', 'Salvos'],
  ['losses', 'Losses and hits'],
  ['munitions', 'Munitions'],
  ['sorties', 'Sorties'],
  ['intel', 'Intel log'],
  ['truth', 'Truth review'],
] as const;
type Tab = (typeof TABS)[number][0];

const CSV = ['intel', 'salvos', 'weapons', 'losses', 'hits', 'munitions', 'sorties', 'operations'];
const TRUTH_CSV = ['truth_hits', 'truth_contacts'];

function download(name: string, text: string, type: string) {
  const url = URL.createObjectURL(new Blob([text], { type }));
  const a = document.createElement('a');
  a.href = url;
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1_000);
}

export function Reports({ bridge, toast }: { bridge: EngineBridge; toast: (t: string) => void }) {
  const [tab, setTab] = useState<Tab>('summary');
  const [r, setR] = useState<Report | null>(null);
  const [confirmEnd, setConfirmEnd] = useState(false);
  const [csv, setCsv] = useState('salvos');

  const [refresh, setRefresh] = useState(0);
  // The truth review comes with the report once the war has ended.
  const fetchReport = useCallback(
    () =>
      bridge
        .request({ type: 'report', truth: false })
        .then((plain) => (plain.war_ended ? bridge.request({ type: 'report', truth: true }) : plain)),
    [bridge],
  );
  useEffect(() => {
    let live = true;
    const load = () =>
      fetchReport()
        .then((x) => live && setR(x))
        .catch((err: unknown) => live && toast(err instanceof Error ? err.message : String(err)));
    load();
    const t = setInterval(load, 2_000);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, [fetchReport, toast, refresh]);
  const load = () => setRefresh((n) => n + 1);

  const truth = r?.war_ended ?? false;
  const text = (format: string) => bridge.request({ type: 'reportText', truth, format });
  const stamp = r ? `t${Math.floor(r.sim_time_ms / 1000)}` : 'report';

  return (
    <div className="panel reports" role="dialog" aria-label="After-action reports">
      <h2 className="panel-header">
        After-action report{r ? ` · ${r.side_name} · T+${elapsed(r.sim_time_ms).slice(2)}` : ''}
        <button type="button" className="close" aria-label="Close reports" onClick={() => ui.set({ reportsOpen: false })}>
          <CloseGlyph />
        </button>
      </h2>
      <div className="report-bar">
        <div className="report-tabs" role="tablist">
          {TABS.map(([id, label]) => (
            <button
              key={id}
              type="button"
              role="tab"
              aria-selected={tab === id}
              className={tab === id ? 'dock-tab active' : 'dock-tab'}
              onClick={() => setTab(id)}
            >
              {label}
            </button>
          ))}
        </div>
        <div className="report-export">
          <button type="button" className="cmd" disabled={!r} onClick={() => r && download(`warboard-${stamp}.json`, JSON.stringify(r, null, 2), 'application/json')}>
            JSON
          </button>
          <select aria-label="CSV table" value={csv} onChange={(e) => setCsv(e.target.value)}>
            {[...CSV, ...(truth ? TRUTH_CSV : [])].map((t) => (
              <option key={t} value={t}>
                {t.replace('_', ' ')}
              </option>
            ))}
          </select>
          <button
            type="button"
            className="cmd"
            disabled={!r}
            onClick={() =>
              void text(`csv:${csv}`)
                .then((t) => download(`warboard-${stamp}-${csv}.csv`, t, 'text/csv'))
                .catch((e: unknown) => toast(String(e)))
            }
          >
            CSV
          </button>
          <button
            type="button"
            className="cmd"
            disabled={!r}
            onClick={() =>
              void text('html')
                .then((html) => {
                  const w = window.open('', '_blank');
                  if (!w) {
                    download(`warboard-${stamp}.html`, html, 'text/html');
                    return;
                  }
                  w.document.write(html);
                  w.document.close();
                  w.focus();
                  w.print();
                })
                .catch((e: unknown) => toast(String(e)))
            }
          >
            PRINT
          </button>
        </div>
      </div>
      <div className="report-body">
        {!r ? (
          <p className="muted">Building the report…</p>
        ) : tab === 'summary' ? (
          <Summary r={r} />
        ) : tab === 'operations' ? (
          <Table
            head={['Operation', 'Phase', 'State', 'Start', 'End', 'Why it ended', 'Results']}
            rows={r.operations.flatMap((o) =>
              o.phases.map((p) => [
                o.name,
                p.name,
                p.state,
                p.started_ms != null ? elapsed(p.started_ms) : '—',
                p.ended_ms != null ? elapsed(p.ended_ms) : '—',
                p.reason,
                `${p.tally.launched} fired, ${p.tally.hits} hit, ${p.tally.intercepted} intercepted, ${p.tally.lost} lost`,
              ]),
            )}
          />
        ) : tab === 'salvos' ? (
          <ol className="salvos">
            {r.salvos.length === 0 && <li className="muted">No salvos fired.</li>}
            {r.salvos.map((s) => (
              <li key={s.salvo}>
                <div>
                  <time>{elapsed(s.t_ms)}</time> {s.summary} <span className="muted">({s.reason})</span>
                </div>
                <ul className="salvo-weapons">
                  {s.weapons.map((w) => (
                    <li key={w.tag}>
                      <strong>{w.tag}</strong> {w.outcome.replace('_', ' ')} <span className="muted">{w.factors}</span>
                    </li>
                  ))}
                </ul>
              </li>
            ))}
          </ol>
        ) : tab === 'losses' ? (
          <>
            <h3>Losses</h3>
            <Table head={['Time', 'Unit', 'Type', 'Cause']} rows={r.losses.map((l) => [elapsed(l.t_ms), l.name, l.platform, l.cause])} />
            <h3>Our hits</h3>
            <Table
              head={['Time', 'Target', 'Weapon', 'Shooter', 'Contact']}
              rows={r.hits.map((h) => [elapsed(h.t_ms), h.target, h.weapon_name, h.shooter_name, h.gone ? 'left our picture' : 'still tracked'])}
            />
          </>
        ) : tab === 'munitions' ? (
          <Table
            head={['Weapon', 'Expended', 'On units', 'In stock', 'By base']}
            rows={r.munitions.map((m) => [
              m.name,
              String(m.expended),
              String(m.on_units),
              String(m.in_stock),
              m.by_base.map(([b, x, s]) => `${b}: ${x} fired, ${s} stock`).join('; '),
            ])}
          />
        ) : tab === 'sorties' ? (
          <Table
            head={['Unit', 'Base', 'Took off', 'Landed', 'Fuel left', 'Refuels', 'Bingo', 'Fired', 'Loadout']}
            rows={r.sorties.map((s) => [
              s.name,
              s.base,
              elapsed(s.took_off_ms),
              s.landed_ms != null ? `${elapsed(s.landed_ms)} ${s.landed_at ?? ''}` : 'airborne',
              s.fuel_at_landing_kg != null ? `${Math.round(s.fuel_at_landing_kg)} kg` : '—',
              String(s.refuels),
              s.bingo ? 'yes' : '',
              String(s.launched),
              s.loadout,
            ])}
          />
        ) : tab === 'intel' ? (
          <Table head={['Time', 'Source', 'Entry']} rows={r.intel.map((l) => [elapsed(l.t_ms), l.source, l.text])} />
        ) : r.truth ? (
          <>
            <h3>What we thought vs what was there</h3>
            <Table
              head={['Side', 'Contact', 'We thought', 'It was', 'Position error']}
              rows={r.truth.contacts.map((c) => [
                r.truth?.sides[c.side]?.name ?? String(c.side),
                `C${c.contact}`,
                c.thought,
                c.was,
                c.error_km != null ? `${c.error_km.toFixed(1)} km` : '—',
              ])}
            />
            <h3>Every hit</h3>
            <Table
              head={['Time', 'Shooter', 'Weapon', 'Target', 'Fate']}
              rows={r.truth.hits.map((h) => [elapsed(h.t_ms), h.shooter, h.weapon, h.target, h.destroyed ? 'lost' : 'survived'])}
            />
            {r.truth.sides.map((s) => (
              <div key={s.side}>
                <h3>
                  {s.name}: {s.units.length} units at the end, {s.lost.length} lost
                </h3>
                <Table head={['Unit', 'Type']} rows={s.units.map((u) => [u.name, u.platform])} />
              </div>
            ))}
          </>
        ) : (
          <div className="truth-locked">
            <p className="muted">The truth review shows both sides once the war has ended. Ending the war cannot be undone.</p>
            {confirmEnd ? (
              <>
                <span className="warn-text">End the war now?</span>
                <button
                  type="button"
                  className="cmd danger"
                  onClick={() => {
                    setConfirmEnd(false);
                    void sendCommand(bridge, { type: 'EndWar' }, toast).then(() => setTimeout(() => void load(), 300));
                  }}
                >
                  END WAR
                </button>
                <button type="button" className="cmd" onClick={() => setConfirmEnd(false)}>
                  KEEP FIGHTING
                </button>
              </>
            ) : (
              <button type="button" className="cmd" onClick={() => setConfirmEnd(true)}>
                END WAR
              </button>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

function Summary({ r }: { r: Report }) {
  const t = r.totals;
  const cells: [string, number][] = [
    ['Launched', t.launched],
    ['Hits', t.hits],
    ['Intercepted', t.intercepted],
    ['Decoyed', t.decoyed],
    ['Jammed', t.jammed],
    ['Missed or lost', t.misses + t.other],
    ['In flight', t.in_flight],
    ['Interceptions made', t.intercepts_made],
    ['Units lost', t.losses],
  ];
  return (
    <>
      <div className="report-totals">
        {cells.map(([label, n]) => (
          <div key={label}>
            <b>{n}</b>
            <span>{label}</span>
          </div>
        ))}
      </div>
      <p className="muted">
        {r.war_ended
          ? 'The war has ended: the truth review is unlocked.'
          : 'During the war the report shows only what our side knows: our own weapons, losses and picture.'}
      </p>
    </>
  );
}

function Table({ head, rows }: { head: string[]; rows: string[][] }) {
  if (rows.length === 0) return <p className="muted">None.</p>;
  return (
    <table className="report-table">
      <thead>
        <tr>
          {head.map((h) => (
            <th key={h}>{h}</th>
          ))}
        </tr>
      </thead>
      <tbody>
        {rows.map((row, i) => (
          <tr key={i}>
            {row.map((c, j) => (
              <td key={j}>{c}</td>
            ))}
          </tr>
        ))}
      </tbody>
    </table>
  );
}
