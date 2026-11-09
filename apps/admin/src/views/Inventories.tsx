// Inventories (doc 04 §5): per nation, quantities, munitions and basing, edited in place;
// and the systems-per-nation-and-archetype matrix used in the P8 report.
import { useMemo, useState } from 'react';
import { type State, editFile, entriesOf, entry, fileOfId, get, nameOf, set, useStore } from '../store';
import type { EntryInfo } from '../wasm';
import { applyDelta, toData } from '../yamlEdit';

type Json = Record<string, unknown>;

function editInventory(id: string, change: (d: Json) => void): void {
  const s = get();
  const file = fileOfId(id, s);
  if (!file) return;
  const text = s.files[file] ?? '';
  const before = toData(text);
  const after = structuredClone(before);
  change(after);
  editFile(file, applyDelta(text, before, after));
}

function className(e: EntryInfo | undefined, s: State): string {
  const a = (e?.value['design'] as { archetype?: string } | undefined)?.archetype;
  return a ? nameOf(entriesOf('archetype', s).find((x) => x.id === a)) : '';
}

export function Inventories() {
  const s = useStore((x) => x);
  const inventories = entriesOf('inventory', s);
  const [nation, setNation] = useState<string>('');
  const [matrix, setMatrix] = useState(false);
  const inv = inventories.find((i) => i.value['nation'] === nation) ?? inventories[0];
  return (
    <div className="page">
      <div className="tabs">
        {inventories.map((i) => {
          const n = String(i.value['nation']);
          return (
            <button key={i.id} className={!matrix && inv?.id === i.id ? 'tab on' : 'tab'} onClick={() => (setNation(n), setMatrix(false))}>
              <span className="mono">{n.toUpperCase()}</span> {nameOf(entry(n, s))}
            </button>
          );
        })}
        <button className={matrix ? 'tab on' : 'tab'} onClick={() => setMatrix(true)}>
          By archetype
        </button>
      </div>
      {matrix ? <Matrix /> : inv ? <Inventory inv={inv} /> : <p className="hint">No inventories.</p>}
    </div>
  );
}

function Inventory({ inv }: { inv: EntryInfo }) {
  const s = useStore((x) => x);
  const v = inv.value;
  const forces = (v['platforms'] as Record<string, number> | undefined) ?? {};
  const munitions = (v['munitions'] as Record<string, number> | undefined) ?? {};
  const basing = (v['basing'] as { platform: string; base: string; count: number }[] | undefined) ?? [];
  const based = new Map<string, number>();
  for (const b of basing) based.set(b.platform, (based.get(b.platform) ?? 0) + b.count);
  const catalogue = [...entriesOf('platform', s), ...entriesOf('system', s)].filter((e) => !(e.id in forces));
  const weapons = entriesOf('weapon', s).filter((e) => !(e.id in munitions));
  const [addForce, setAddForce] = useState('');
  const [addWeapon, setAddWeapon] = useState('');
  const row = (key: 'platforms' | 'munitions', id: string, n: number) => {
    const e = entry(id, s);
    return (
      <tr key={id}>
        <td>
          <button className="link mono" onClick={() => set({ view: 'browse', selected: id })}>
            {id}
          </button>
        </td>
        <td>
          {nameOf(e)}
          {e?.custom && <span className="pill dim">custom</span>}
        </td>
        <td className="dim">{className(e, s)}</td>
        <td>
          <input
            type="number"
            min={0}
            value={n}
            onChange={(ev) => editInventory(inv.id, (d) => void ((d[key] as Json)[id] = Math.max(0, Number(ev.target.value))))}
          />
        </td>
        {key === 'platforms' && <td className={(based.get(id) ?? 0) > n ? 'bad mono' : 'mono dim'}>{based.get(id) ?? 0}</td>}
        <td>
          <button className="link" onClick={() => editInventory(inv.id, (d) => void delete (d[key] as Json)[id])}>
            Remove
          </button>
        </td>
      </tr>
    );
  };
  return (
    <div>
      <p className="hint">{String((v['design'] as Json | undefined)?.['note'] ?? '')}</p>
      <div className="inv">
        <section>
          <h3>Forces</h3>
          <table className="grid">
            <thead>
              <tr>
                <th>Id</th>
                <th>Name</th>
                <th>Class</th>
                <th>Count</th>
                <th>At bases</th>
                <th />
              </tr>
            </thead>
            <tbody>{Object.entries(forces).map(([id, n]) => row('platforms', id, n))}</tbody>
          </table>
          <div className="row">
            <select value={addForce} onChange={(e) => setAddForce(e.target.value)}>
              <option value="">Add a platform or system…</option>
              {catalogue.map((e) => (
                <option key={e.id} value={e.id}>
                  {e.id} · {nameOf(e)}
                  {e.custom ? ' (custom)' : ''}
                </option>
              ))}
            </select>
            <button
              className="cmd small"
              disabled={!addForce}
              onClick={() => {
                editInventory(inv.id, (d) => void (((d['platforms'] ??= {}) as Json)[addForce] = 1));
                setAddForce('');
              }}
            >
              ADD
            </button>
          </div>
        </section>
        <section>
          <h3>Munitions</h3>
          <table className="grid">
            <thead>
              <tr>
                <th>Id</th>
                <th>Name</th>
                <th>Class</th>
                <th>Rounds</th>
                <th />
              </tr>
            </thead>
            <tbody>{Object.entries(munitions).map(([id, n]) => row('munitions', id, n))}</tbody>
          </table>
          <div className="row">
            <select value={addWeapon} onChange={(e) => setAddWeapon(e.target.value)}>
              <option value="">Add a weapon…</option>
              {weapons.map((e) => (
                <option key={e.id} value={e.id}>
                  {e.id} · {nameOf(e)}
                </option>
              ))}
            </select>
            <button
              className="cmd small"
              disabled={!addWeapon}
              onClick={() => {
                editInventory(inv.id, (d) => void (((d['munitions'] ??= {}) as Json)[addWeapon] = 10));
                setAddWeapon('');
              }}
            >
              ADD
            </button>
          </div>
        </section>
        <section>
          <h3>Basing</h3>
          <table className="grid">
            <thead>
              <tr>
                <th>Base</th>
                <th>Platform</th>
                <th>Count</th>
              </tr>
            </thead>
            <tbody>
              {basing.map((b, i) => (
                <tr key={i}>
                  <td className="mono">{b.base}</td>
                  <td className="mono">{b.platform}</td>
                  <td>
                    <input
                      type="number"
                      min={0}
                      value={b.count}
                      onChange={(e) =>
                        editInventory(inv.id, (d) => {
                          const it = (d['basing'] as Json[])[i];
                          if (it) it['count'] = Math.max(0, Number(e.target.value));
                        })
                      }
                    />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      </div>
    </div>
  );
}

/** Systems per nation and archetype: catalogue entries in each inventory, by class. */
function Matrix() {
  const s = useStore((x) => x);
  const { nations, rows } = useMemo(() => {
    const inventories = entriesOf('inventory', s);
    const nations = inventories.map((i) => String(i.value['nation']));
    const rows = new Map<string, Map<string, string[]>>();
    for (const inv of inventories) {
      const n = String(inv.value['nation']);
      for (const id of Object.keys((inv.value['platforms'] as Json | undefined) ?? {})) {
        const e = entry(id, s);
        const a = (e?.value['design'] as { archetype?: string } | undefined)?.archetype ?? '(none)';
        const r = rows.get(a) ?? new Map<string, string[]>();
        r.set(n, [...(r.get(n) ?? []), id]);
        rows.set(a, r);
      }
    }
    return { nations, rows: [...rows.entries()].sort((a, b) => a[0].localeCompare(b[0])) };
  }, [s]);
  return (
    <table className="grid wide matrix">
      <thead>
        <tr>
          <th>Archetype</th>
          {nations.map((n) => (
            <th key={n} className="mono">
              {n.toUpperCase()}
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {rows.map(([a, r]) => (
          <tr key={a}>
            <td>{nameOf(entriesOf('archetype', s).find((x) => x.id === a)) || a}</td>
            {nations.map((n) => (
              <td key={n} className="mono">
                {(r.get(n) ?? []).join(', ')}
              </td>
            ))}
          </tr>
        ))}
      </tbody>
    </table>
  );
}
