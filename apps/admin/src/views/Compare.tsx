// Compare (doc 04 §5): two entries side by side, resolved fields, differences marked,
// each with its archetype band bars.
import { useState } from 'react';
import { entry, get, nameOf, set, useStore } from '../store';
import type { EntryInfo } from '../wasm';
import { BandBars } from './widgets';

function flatten(v: unknown, prefix = '', out = new Map<string, string>()): Map<string, string> {
  if (Array.isArray(v)) {
    if (v.every((x) => typeof x !== 'object')) out.set(prefix, v.join(', '));
    else v.forEach((x, i) => flatten(x, `${prefix}[${i}]`, out));
  } else if (v && typeof v === 'object') {
    for (const [k, x] of Object.entries(v)) flatten(x, prefix ? `${prefix}.${k}` : k, out);
  } else out.set(prefix, String(v));
  return out;
}

function Picker({ value, onPick, entries }: { value: string | null; onPick: (id: string) => void; entries: EntryInfo[] }) {
  const [q, setQ] = useState('');
  const shown = entries.filter((e) => `${e.id} ${nameOf(e)}`.toLowerCase().includes(q.toLowerCase())).slice(0, 400);
  return (
    <div className="picker">
      <input placeholder="Filter" value={q} onChange={(e) => setQ(e.target.value)} />
      <select value={value ?? ''} onChange={(e) => onPick(e.target.value)} size={8}>
        {shown.map((e) => (
          <option key={e.id} value={e.id}>
            {e.kind} · {e.id} · {nameOf(e)}
          </option>
        ))}
      </select>
    </div>
  );
}

export function Compare() {
  const s = useStore((x) => x);
  const [a, b] = s.compare;
  const entries = (s.report?.ok ? s.report.entries : [...s.good.values()]).filter((e) =>
    ['platform', 'system', 'sensor', 'weapon', 'ew_suite', 'countermeasure', 'datalink', 'loadout', 'doctrine', 'inventory'].includes(e.kind),
  );
  const ea = a ? entry(a, s) : undefined;
  const eb = b ? entry(b, s) : undefined;
  const fa = flatten(ea?.value ?? {});
  const fb = flatten(eb?.value ?? {});
  const keys = [...new Set([...fa.keys(), ...fb.keys()])].sort();
  return (
    <div className="page">
      <div className="compare-pick">
        <Picker value={a} entries={entries} onPick={(id) => set({ compare: [id, get().compare[1]] })} />
        <Picker value={b} entries={entries} onPick={(id) => set({ compare: [get().compare[0], id] })} />
      </div>
      {ea && eb && ea.kind !== eb.kind && <p className="warn">Different kinds: only shared fields line up.</p>}
      <div className="compare-bands">
        <div>{ea && <BandBars bands={ea.bands} archetype={ea.archetype} />}</div>
        <div>{eb && <BandBars bands={eb.bands} archetype={eb.archetype} />}</div>
      </div>
      <table className="grid wide">
        <thead>
          <tr>
            <th>Field</th>
            <th>{ea ? nameOf(ea) : 'Pick an entry'}</th>
            <th>{eb ? nameOf(eb) : 'Pick an entry'}</th>
          </tr>
        </thead>
        <tbody>
          {keys.map((k) => {
            const x = fa.get(k);
            const y = fb.get(k);
            return (
              <tr key={k} className={x !== y ? 'diff' : ''}>
                <td className="mono">{k}</td>
                <td>{x ?? <span className="dim">—</span>}</td>
                <td>{y ?? <span className="dim">—</span>}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
