// Browse and search by kind, category, nation and archetype (doc 04 §5).
import { useMemo, useState } from 'react';
import { type State, idOfFile, kindOfFile, set, unsaved, useStore } from '../store';
import { EntryEditor } from './EntryEditor';

interface Row {
  id: string;
  kind: string;
  file: string;
  name: string;
  category: string;
  archetype: string;
  nations: string[];
  custom: boolean;
}

const KIND_ORDER = [
  'platform', 'system', 'sensor', 'weapon', 'ew_suite', 'countermeasure', 'datalink', 'loadout',
  'archetype', 'nation', 'inventory', 'doctrine', 'base', 'world', 'places', 'realism',
];

function rows(s: State): Row[] {
  const byId = new Map((s.report?.entries ?? [...s.good.values()]).map((e) => [e.id, e]));
  // Nations that hold each force or munition in their inventory.
  const holders = new Map<string, string[]>();
  for (const e of byId.values()) {
    if (e.kind !== 'inventory') continue;
    const n = String(e.value['nation']);
    for (const key of ['platforms', 'munitions']) {
      for (const id of Object.keys((e.value[key] as Record<string, number> | undefined) ?? {})) {
        holders.set(id, [...(holders.get(id) ?? []), n]);
      }
    }
  }
  const out: Row[] = [];
  for (const [file, text] of Object.entries(s.files)) {
    if (!file.includes('/')) continue;
    const id = idOfFile(text);
    if (!id) continue;
    const e = byId.get(id);
    const v = e?.value ?? {};
    const design = (v['design'] as { archetype?: string } | undefined) ?? {};
    out.push({
      id,
      kind: e?.kind ?? kindOfFile(text) ?? '?',
      file,
      name: typeof v['name'] === 'string' ? v['name'] : id,
      category: String(v['category'] ?? v['deploy_as'] ?? v['sensor_type'] ?? v['base_type'] ?? ''),
      archetype: design.archetype ?? '',
      nations: e?.kind === 'nation' ? [id] : typeof v['nation'] === 'string' ? [v['nation']] : (holders.get(id) ?? []),
      custom: e?.custom ?? /^custom:\s*true/m.test(text),
    });
  }
  return out.sort((a, b) => KIND_ORDER.indexOf(a.kind) - KIND_ORDER.indexOf(b.kind) || a.id.localeCompare(b.id));
}

export function Browse() {
  const s = useStore((x) => x);
  const all = useMemo(() => rows(s), [s]);
  const [q, setQ] = useState('');
  const [kind, setKind] = useState('');
  const [nation, setNation] = useState('');
  const [archetype, setArchetype] = useState('');
  const [category, setCategory] = useState('');
  const dirty = useMemo(() => new Set(unsaved(s)), [s]);
  const errorFiles = useMemo(() => new Set((s.report?.errors ?? []).map((e) => e.file.replace(/^content\//, ''))), [s.report]);
  const warnFiles = useMemo(() => new Set((s.report?.warnings ?? []).map((e) => e.file.replace(/^content\//, ''))), [s.report]);
  const kinds = [...new Set(all.map((r) => r.kind))];
  const nations = [...new Set(all.flatMap((r) => r.nations))].sort();
  const archetypes = [...new Set(all.map((r) => r.archetype).filter(Boolean))].sort();
  const categories = [...new Set(all.filter((r) => !kind || r.kind === kind).map((r) => r.category).filter(Boolean))].sort();
  const needle = q.trim().toLowerCase();
  const shown = all.filter(
    (r) =>
      (!kind || r.kind === kind) &&
      (!nation || r.nations.includes(nation)) &&
      (!archetype || r.archetype === archetype) &&
      (!category || r.category === category) &&
      (!needle || `${r.id} ${r.name} ${r.archetype} ${r.category}`.toLowerCase().includes(needle)),
  );
  return (
    <div className="split">
      <aside className="list-pane">
        <input className="search" placeholder="Search id, name, class" value={q} onChange={(e) => setQ(e.target.value)} />
        <div className="filters">
          <select value={kind} onChange={(e) => (setKind(e.target.value), setCategory(''))}>
            <option value="">All kinds</option>
            {kinds.map((k) => (
              <option key={k}>{k}</option>
            ))}
          </select>
          <select value={category} onChange={(e) => setCategory(e.target.value)}>
            <option value="">All categories</option>
            {categories.map((k) => (
              <option key={k}>{k}</option>
            ))}
          </select>
          <select value={nation} onChange={(e) => setNation(e.target.value)}>
            <option value="">All nations</option>
            {nations.map((k) => (
              <option key={k} value={k}>
                {k.toUpperCase()}
              </option>
            ))}
          </select>
          <select value={archetype} onChange={(e) => setArchetype(e.target.value)}>
            <option value="">All archetypes</option>
            {archetypes.map((k) => (
              <option key={k}>{k}</option>
            ))}
          </select>
        </div>
        <div className="count dim">
          {shown.length} of {all.length}
        </div>
        <ul className="entries">
          {shown.map((r) => (
            <li
              key={r.file}
              className={s.selected === r.id ? 'sel' : ''}
              onClick={() => set({ selected: r.id })}
              title={r.file}
            >
              <span className="mono kind">{r.kind}</span>
              <span className="name">
                {r.name}
                {r.custom && <span className="pill dim">custom</span>}
              </span>
              <span className="mono id">{r.id}</span>
              {errorFiles.has(r.file) && <span className="pill bad">error</span>}
              {!errorFiles.has(r.file) && warnFiles.has(r.file) && <span className="pill warn">band</span>}
              {dirty.has(r.file) && <span className="pill warn">unsaved</span>}
            </li>
          ))}
        </ul>
      </aside>
      <section className="detail-pane">{s.selected ? <EntryEditor id={s.selected} /> : <p className="hint">Pick an entry.</p>}</section>
    </div>
  );
}
