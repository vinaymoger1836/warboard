// Custom form widgets of doc 04 §5: RCS by aspect, station editor, magazine mix,
// archetype picker, and the archetype band bars.
import type { FieldProps, WidgetProps } from '@rjsf/utils';
import { entriesOf, get, nameOf, useStore } from '../store';
import type { BandPosition } from '../wasm';

type Rcs = { front?: number; side?: number; rear?: number; top?: number };
const ASPECTS = ['front', 'side', 'rear', 'top'] as const;

function num(v: string): number | undefined {
  if (v.trim() === '') return undefined;
  const n = Number(v);
  return Number.isFinite(n) ? n : undefined;
}

/** Radius on a log scale from 0.001 m² to 100 000 m². */
function logR(v: number | undefined, r: number): number {
  if (!v || v <= 0) return 2;
  const t = (Math.log10(v) + 3) / 8;
  return Math.max(2, Math.min(1, t) * r);
}

export function RcsField(props: FieldProps<Rcs>) {
  const v = props.formData ?? {};
  const set = (k: keyof Rcs, x: string) => props.onChange({ ...v, [k]: num(x) }, props.fieldPathId.path);
  const R = 46;
  const pts = [
    [0, -logR(v.front, R)],
    [logR(v.side, R), 0],
    [0, logR(v.rear, R)],
    [-logR(v.side, R), 0],
  ];
  return (
    <fieldset className="widget">
      <legend>RCS by aspect (m²)</legend>
      <div className="rcs">
        <svg viewBox="-55 -55 110 110" width="110" height="110" aria-label="RCS by aspect, log scale">
          {[0.25, 0.5, 0.75, 1].map((f) => (
            <circle key={f} r={R * f} className="rcs-ring" />
          ))}
          <polygon points={pts.map((p) => p.join(',')).join(' ')} className="rcs-shape" />
          <text y={-R - 2} className="rcs-label">front</text>
          <text y={R + 8} className="rcs-label">rear</text>
        </svg>
        <div className="rcs-inputs">
          {ASPECTS.map((k) => (
            <label key={k}>
              <span>{k}</span>
              <input type="number" step="any" value={v[k] ?? ''} onChange={(e) => set(k, e.target.value)} />
            </label>
          ))}
          <p className="hint">Log scale: each ring is ×100. Front decides most first detections.</p>
        </div>
      </div>
    </fieldset>
  );
}

type Station = { id: string; kind: string; categories: string[]; max_kg?: number; capacity?: number };
const KINDS = ['internal', 'external', 'vls', 'launcher'];

export function StationsField(props: FieldProps<Station[]>) {
  const list = props.formData ?? [];
  const put = (next: Station[]) => props.onChange(next, props.fieldPathId.path);
  const upd = (i: number, patch: Partial<Station>) => put(list.map((s, j) => (j === i ? { ...s, ...patch } : s)));
  return (
    <fieldset className="widget">
      <legend>Stations</legend>
      <table className="grid">
        <thead>
          <tr>
            <th>Id</th>
            <th>Kind</th>
            <th>Categories</th>
            <th>Max kg</th>
            <th>Capacity</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {list.map((s, i) => (
            <tr key={i}>
              <td>
                <input value={s.id} onChange={(e) => upd(i, { id: e.target.value })} />
              </td>
              <td>
                <select value={s.kind} onChange={(e) => upd(i, { kind: e.target.value })}>
                  {KINDS.map((k) => (
                    <option key={k}>{k}</option>
                  ))}
                </select>
              </td>
              <td>
                <input
                  value={s.categories.join(', ')}
                  onChange={(e) => upd(i, { categories: e.target.value.split(',').map((c) => c.trim()).filter(Boolean) })}
                />
              </td>
              <td>
                <input type="number" value={s.max_kg ?? ''} onChange={(e) => upd(i, { max_kg: num(e.target.value) })} />
              </td>
              <td>
                <input type="number" value={s.capacity ?? 1} onChange={(e) => upd(i, { capacity: num(e.target.value) ?? 1 })} />
              </td>
              <td>
                <button type="button" className="link" onClick={() => put(list.filter((_, j) => j !== i))}>
                  Remove
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <button
        type="button"
        className="cmd small"
        onClick={() => put([...list, { id: `station_${list.length + 1}`, kind: 'external', categories: [], capacity: 1 }])}
      >
        ADD STATION
      </button>
    </fieldset>
  );
}

/** Weapon id → rounds per launcher, with the cells it uses against the launcher's tubes. */
export function MagazineField(props: FieldProps<Record<string, number>>) {
  const mag = props.formData ?? {};
  const weapons = useStore((s) => entriesOf('weapon', s));
  const put = (next: Record<string, number>) => props.onChange(next, props.fieldPathId.path);
  // The component's platform is a sibling field: read it from the form's data.
  const path = props.fieldPathId.path;
  const idx = typeof path[1] === 'number' ? path[1] : -1;
  const comps = (props.registry.formContext as { components?: { platform?: string }[] } | undefined)?.components;
  const platformId = comps?.[idx]?.platform;
  const platform = platformId ? entriesOf('platform', get()).find((p) => p.id === platformId) : undefined;
  const stations = (platform?.value['stations'] as { categories: string[]; capacity?: number; kind: string }[] | undefined) ?? [];
  const cells = Object.entries(mag).reduce((sum, [wid, n]) => {
    const w = weapons.find((x) => x.id === wid);
    const rpc = Number(w?.value['rounds_per_cell'] ?? 1) || 1;
    return sum + Math.ceil(n / rpc);
  }, 0);
  const capacity = stations.reduce((s, st) => s + (st.capacity ?? 1), 0);
  const fits = (wid: string) => {
    const cats = (weapons.find((x) => x.id === wid)?.value['station_category'] as string[] | undefined) ?? [];
    return stations.some((st) => st.categories.some((c) => cats.includes(c)));
  };
  return (
    <fieldset className="widget">
      <legend>Magazine per launcher</legend>
      <table className="grid">
        <tbody>
          {Object.entries(mag).map(([wid, n]) => (
            <tr key={wid}>
              <td>
                <select
                  value={wid}
                  onChange={(e) => {
                    const next: Record<string, number> = {};
                    for (const [k, v] of Object.entries(mag)) next[k === wid ? e.target.value : k] = v;
                    put(next);
                  }}
                >
                  {weapons.map((w) => (
                    <option key={w.id} value={w.id}>
                      {w.id} · {nameOf(w)}
                    </option>
                  ))}
                </select>
              </td>
              <td>
                <input type="number" value={n} onChange={(e) => put({ ...mag, [wid]: num(e.target.value) ?? 0 })} />
              </td>
              <td className={fits(wid) ? 'dim' : 'bad'}>{fits(wid) ? 'fits' : 'no station takes it'}</td>
              <td>
                <button
                  type="button"
                  className="link"
                  onClick={() => {
                    const next = { ...mag };
                    delete next[wid];
                    put(next);
                  }}
                >
                  Remove
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <div className="mag-bar" title={`${cells} of ${capacity} cells`}>
        <div className={cells > capacity ? 'mag-fill bad' : 'mag-fill'} style={{ width: `${capacity ? Math.min(100, (100 * cells) / capacity) : 0}%` }} />
      </div>
      <p className="hint">
        {cells} of {capacity || '?'} cells per launcher{platformId ? ` (${platformId})` : ''}
      </p>
      <button
        type="button"
        className="cmd small"
        onClick={() => {
          const w = weapons.find((x) => !(x.id in mag));
          if (w) put({ ...mag, [w.id]: 1 });
        }}
      >
        ADD WEAPON
      </button>
    </fieldset>
  );
}

export function ArchetypeWidget(props: WidgetProps) {
  const archetypes = useStore((s) => entriesOf('archetype', s));
  return (
    <select id={props.id} value={(props.value as string | undefined) ?? ''} onChange={(e) => props.onChange(e.target.value || undefined)}>
      <option value="">(none)</option>
      {archetypes.map((a) => (
        <option key={a.id} value={a.id}>
          {a.id} · {nameOf(a)}
        </option>
      ))}
    </select>
  );
}

function fmt(v: number): string {
  return Math.abs(v) >= 100 ? v.toFixed(0) : Math.abs(v) >= 1 ? v.toFixed(2).replace(/\.?0+$/, '') : v.toPrecision(2);
}

/** Where each key stat sits in its archetype's band (doc 04 §4). */
export function BandBars({ bands, archetype }: { bands: BandPosition[]; archetype: string | null }) {
  const arch = useStore((s) => entriesOf('archetype', s).find((a) => a.id === archetype));
  if (!archetype) return <p className="hint">No archetype.</p>;
  return (
    <div className="bands">
      <div className="bands-title">
        {nameOf(arch)} <span className="mono dim">{archetype}</span>
      </div>
      {bands.length === 0 && <p className="hint">This archetype has no bands.</p>}
      {bands.map((b) => {
        const span = b.max - b.min || 1;
        const lo = b.min - span * 0.5;
        const hi = b.max + span * 0.5;
        const pos = (x: number) => `${Math.max(0, Math.min(100, ((x - lo) / (hi - lo)) * 100))}%`;
        const inside = b.value !== null && b.value >= b.min && b.value <= b.max;
        return (
          <div key={b.field} className="band">
            <div className="band-label">
              <span className="mono">{b.field}</span>
              <span className={b.value === null ? 'dim' : inside ? 'ok' : 'bad'}>{b.value === null ? 'not set' : fmt(b.value)}</span>
            </div>
            <div className="band-track">
              <div className="band-range" style={{ left: pos(b.min), width: `calc(${pos(b.max)} - ${pos(b.min)})` }} />
              {b.value !== null && <div className={inside ? 'band-mark' : 'band-mark bad'} style={{ left: pos(b.value) }} />}
            </div>
            <div className="band-ends mono">
              <span>{fmt(b.min)}</span>
              <span>{fmt(b.max)}</span>
            </div>
          </div>
        );
      })}
    </div>
  );
}
