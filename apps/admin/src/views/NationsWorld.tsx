// Nations and world (doc 04 §5): nation list, seeds and weights with a border preview,
// place names, and the bases of each nation.
import { useEffect, useMemo, useRef, useState } from 'react';
import { editFile, entriesOf, fileOfId, get, nameOf, set, useStore } from '../store';
import { applyDelta, toData } from '../yamlEdit';
import type { FromBorders, Seed, ToBorders } from './borders.worker';

type Json = Record<string, unknown>;

/** Applies `change` to the parsed data of an entry's file. */
function editEntry(id: string, change: (d: Json) => void): void {
  const s = get();
  const file = fileOfId(id, s);
  if (!file) return;
  const text = s.files[file] ?? '';
  const before = toData(text);
  const after = structuredClone(before);
  change(after);
  editFile(file, applyDelta(text, before, after));
}

export function NationsWorld() {
  const [tab, setTab] = useState<'nations' | 'world' | 'places' | 'bases'>('nations');
  return (
    <div className="page">
      <div className="tabs">
        {(['nations', 'world', 'places', 'bases'] as const).map((t) => (
          <button key={t} className={tab === t ? 'tab on' : 'tab'} onClick={() => setTab(t)}>
            {{ nations: 'Nations', world: 'Seeds and borders', places: 'Place names', bases: 'Bases' }[t]}
          </button>
        ))}
      </div>
      {tab === 'nations' && <Nations />}
      {tab === 'world' && <World />}
      {tab === 'places' && <Places />}
      {tab === 'bases' && <Bases />}
    </div>
  );
}

function Nations() {
  const nations = useStore((s) => entriesOf('nation', s));
  const doctrines = useStore((s) => entriesOf('doctrine', s));
  const links = useStore((s) => entriesOf('datalink', s));
  return (
    <table className="grid wide">
      <thead>
        <tr>
          <th>Nation</th>
          <th>Name</th>
          <th>Colour</th>
          <th>Doctrine</th>
          <th>Data links</th>
          <th>Character</th>
        </tr>
      </thead>
      <tbody>
        {nations.map((n) => {
          const v = n.value;
          const dl = (v['datalinks'] as string[] | undefined) ?? [];
          return (
            <tr key={n.id}>
              <td className="mono">{n.id.toUpperCase()}</td>
              <td>
                <input value={String(v['name'] ?? '')} onChange={(e) => editEntry(n.id, (d) => void (d['name'] = e.target.value))} />
              </td>
              <td>
                <input
                  type="color"
                  value={String(v['colour'] ?? '#000000')}
                  onChange={(e) => editEntry(n.id, (d) => void (d['colour'] = e.target.value.toUpperCase()))}
                />
              </td>
              <td>
                <select value={String(v['doctrine'] ?? '')} onChange={(e) => editEntry(n.id, (d) => void (d['doctrine'] = e.target.value))}>
                  {doctrines.map((d) => (
                    <option key={d.id} value={d.id}>
                      {nameOf(d)}
                    </option>
                  ))}
                </select>
              </td>
              <td>
                {links.map((l) => (
                  <label key={l.id} className="check">
                    <input
                      type="checkbox"
                      checked={dl.includes(l.id)}
                      onChange={(e) =>
                        editEntry(n.id, (d) => {
                          const cur = (d['datalinks'] as string[] | undefined) ?? [];
                          d['datalinks'] = e.target.checked ? [...cur, l.id] : cur.filter((x) => x !== l.id);
                        })
                      }
                    />
                    {nameOf(l)}
                  </label>
                ))}
              </td>
              <td className="dim">{String((v['design'] as Json | undefined)?.['note'] ?? '')}</td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

const COLS = 240;
const ROWS = 288;

function World() {
  const worlds = useStore((s) => entriesOf('world', s));
  const nations = useStore((s) => entriesOf('nation', s));
  const repo = useStore((s) => s.repo);
  const world = worlds[0];
  const v = world?.value ?? {};
  const extent = (v['extent'] as number[] | undefined) ?? [2, 47.5, 42, 71.5];
  const reachKm = Number(v['reach_km'] ?? 500);
  const list = (v['nations'] as { nation: string; seed_at: [number, number]; weight: number }[] | undefined) ?? [];
  const seeds: Seed[] = useMemo(() => list.map((n) => ({ nation: n.nation, lon: n.seed_at[0], lat: n.seed_at[1], weight: n.weight })), [list]);
  const colours = useMemo(() => new Map(nations.map((n) => [n.id, String(n.value['colour'] ?? '#888888')])), [nations]);
  const worker = useRef<Worker | null>(null);
  const [grid, setGrid] = useState<Extract<FromBorders, { type: 'sampled' }> | null>(null);
  const [owner, setOwner] = useState<Int16Array | null>(null);
  const [msg, setMsg] = useState('');

  useEffect(() => {
    const w = new Worker(new URL('./borders.worker.ts', import.meta.url), { type: 'module' });
    w.onmessage = (e: MessageEvent<FromBorders>) => {
      const m = e.data;
      if (m.type === 'progress') setMsg(m.text);
      else if (m.type === 'error') setMsg(m.text);
      else if (m.type === 'sampled') {
        setGrid(m);
        setMsg(m.hasPack ? 'Terrain from the europe pack' : 'No europe pack in build/packs: preview without terrain');
      } else setOwner(m.owner);
    };
    worker.current = w;
    return () => w.terminate();
  }, []);

  const sample = async (withPack: boolean) => {
    setMsg('Opening the pack…');
    const files = withPack && repo ? await repo.packFiles('europe') : null;
    const m: ToBorders = { type: 'sample', files, extent, cols: COLS, rows: ROWS };
    worker.current?.postMessage(m);
  };

  useEffect(() => {
    if (!grid) return;
    const m: ToBorders = { type: 'grow', seeds, reachKm };
    worker.current?.postMessage(m);
  }, [grid, seeds, reachKm]);

  const setSeed = (i: number, key: 'lon' | 'lat' | 'weight', x: number) =>
    world &&
    editEntry(world.id, (d) => {
      const ns = d['nations'] as Json[];
      const n = ns[i];
      if (!n) return;
      if (key === 'weight') n['weight'] = x;
      else {
        const at = [...(n['seed_at'] as number[])];
        at[key === 'lon' ? 0 : 1] = x;
        n['seed_at'] = at;
      }
    });

  if (!world) return <p className="hint">No world entry.</p>;
  return (
    <div className="world">
      <div className="world-side">
        <h3>{nameOf(world)}</h3>
        <div className="row">
          <label>
            Seed{' '}
            <input type="number" value={Number(v['seed'] ?? 0)} onChange={(e) => editEntry(world.id, (d) => void (d['seed'] = Number(e.target.value)))} />
          </label>
          <label>
            Reach km{' '}
            <input type="number" value={reachKm} onChange={(e) => editEntry(world.id, (d) => void (d['reach_km'] = Number(e.target.value)))} />
          </label>
        </div>
        <table className="grid">
          <thead>
            <tr>
              <th>Nation</th>
              <th>Lon</th>
              <th>Lat</th>
              <th>Weight</th>
            </tr>
          </thead>
          <tbody>
            {list.map((n, i) => (
              <tr key={n.nation}>
                <td className="mono">
                  <span className="swatch" style={{ background: colours.get(n.nation) }} /> {n.nation.toUpperCase()}
                </td>
                <td>
                  <input type="number" step="0.1" value={n.seed_at[0]} onChange={(e) => setSeed(i, 'lon', Number(e.target.value))} />
                </td>
                <td>
                  <input type="number" step="0.1" value={n.seed_at[1]} onChange={(e) => setSeed(i, 'lat', Number(e.target.value))} />
                </td>
                <td>
                  <input type="number" step="0.05" value={n.weight} onChange={(e) => setSeed(i, 'weight', Number(e.target.value))} />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        <div className="row">
          <button className="cmd small" onClick={() => void sample(true)} disabled={!repo}>
            LOAD TERRAIN
          </button>
          <button className="cmd small" onClick={() => void sample(false)}>
            PREVIEW WITHOUT TERRAIN
          </button>
        </div>
        <p className="hint">{msg}</p>
        <p className="hint">
          The preview grows regions from the seeds with the terrain costs of ADR-042 on a {COLS}×{ROWS} grid, without noise or smoothing. Run{' '}
          <span className="mono">geodata/run.sh build europe</span> for the real borders.
        </p>
      </div>
      <div className="maps">
        <MapCanvas title="Built (from the pack)" grid={grid} owner={grid?.built ?? null} codes={grid?.builtCodes ?? []} colours={colours} seeds={seeds} extent={extent} />
        <MapCanvas title="Preview (edited seeds and weights)" grid={grid} owner={owner} codes={seeds.map((s) => s.nation)} colours={colours} seeds={seeds} extent={extent} />
      </div>
    </div>
  );
}

function MapCanvas(props: {
  title: string;
  grid: Extract<FromBorders, { type: 'sampled' }> | null;
  owner: Int16Array | null;
  codes: string[];
  colours: Map<string, string>;
  seeds: Seed[];
  extent: number[];
}) {
  const ref = useRef<HTMLCanvasElement | null>(null);
  const { grid, owner, codes, colours, seeds, extent } = props;
  useEffect(() => {
    const cv = ref.current;
    if (!cv || !grid) return;
    const ctx = cv.getContext('2d');
    if (!ctx) return;
    const css = getComputedStyle(document.documentElement);
    const sea = css.getPropertyValue('--map-sea').trim();
    const land = css.getPropertyValue('--map-land').trim();
    const img = ctx.createImageData(grid.cols, grid.rows);
    const rgb = (h: string) => [parseInt(h.slice(1, 3), 16), parseInt(h.slice(3, 5), 16), parseInt(h.slice(5, 7), 16)];
    const pal = codes.map((c) => rgb(colours.get(c) ?? '#888888'));
    const seaC = rgb(sea || '#2C343C');
    const landC = rgb(land || '#0E0E0E');
    for (let i = 0; i < grid.cols * grid.rows; i++) {
      const o = owner?.[i] ?? -1;
      const c = !grid.land[i] ? seaC : o >= 0 ? (pal[o] ?? landC) : landC;
      img.data.set([c[0]!, c[1]!, c[2]!, 255], i * 4);
    }
    ctx.putImageData(img, 0, 0);
    ctx.fillStyle = css.getPropertyValue('--text-primary').trim() || '#FFFFFF';
    for (const s of seeds) {
      const x = ((s.lon - extent[0]!) / (extent[2]! - extent[0]!)) * grid.cols;
      const y = ((extent[3]! - s.lat) / (extent[3]! - extent[1]!)) * grid.rows;
      ctx.fillRect(x - 1.5, y - 1.5, 3, 3);
    }
  }, [grid, owner, codes, colours, seeds, extent]);
  // Equal-area look at the theatre's middle latitude.
  const mid = (((extent[1] ?? 0) + (extent[3] ?? 0)) / 2) * (Math.PI / 180);
  const aspect = ((extent[2]! - extent[0]!) * Math.cos(mid)) / (extent[3]! - extent[1]!);
  return (
    <figure className="map-fig">
      <figcaption>{props.title}</figcaption>
      {grid ? (
        <canvas ref={ref} width={grid.cols} height={grid.rows} style={{ aspectRatio: String(aspect) }} />
      ) : (
        <div className="map-empty hint">Load terrain or preview without it.</div>
      )}
    </figure>
  );
}

function Places() {
  const places = useStore((s) => entriesOf('places', s)[0]);
  const [nation, setNation] = useState('');
  if (!places) return <p className="hint">No places entry.</p>;
  const list = (places.value['places'] as Json[] | undefined) ?? [];
  return (
    <div>
      <div className="row">
        <select value={nation} onChange={(e) => setNation(e.target.value)}>
          <option value="">All</option>
          {[...new Set(list.map((p) => String(p['nation'] ?? '')))].filter(Boolean).sort().map((n) => (
            <option key={n} value={n}>
              {n.toUpperCase()}
            </option>
          ))}
        </select>
        <span className="dim">Generated labels are replaced when `geodata places` runs again; edit one to keep it (doc 11 §2.4).</span>
      </div>
      <table className="grid wide">
        <thead>
          <tr>
            <th>Kind</th>
            <th>Name</th>
            <th>Nation</th>
            <th>Lon, lat</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {list.map((p, i) =>
            nation && p['nation'] !== nation ? null : (
              <tr key={i}>
                <td className="mono">{String(p['kind'])}</td>
                <td>
                  <input
                    value={String(p['name'] ?? '')}
                    onChange={(e) =>
                      editEntry(places.id, (d) => {
                        const it = (d['places'] as Json[])[i];
                        if (it) {
                          it['name'] = e.target.value;
                          delete it['generated'];
                        }
                      })
                    }
                  />
                </td>
                <td className="mono">{String(p['nation'] ?? '').toUpperCase()}</td>
                <td className="mono dim">{JSON.stringify(p['at'])}</td>
                <td>{p['generated'] ? <span className="pill dim">generated</span> : null}</td>
              </tr>
            ),
          )}
        </tbody>
      </table>
    </div>
  );
}

function Bases() {
  const bases = useStore((s) => entriesOf('base', s));
  const byNation = new Map<string, typeof bases>();
  for (const b of bases) {
    const n = String(b.value['nation']);
    byNation.set(n, [...(byNation.get(n) ?? []), b]);
  }
  return (
    <div>
      {[...byNation.entries()].sort().map(([n, list]) => (
        <section key={n}>
          <h3 className="mono">{n.toUpperCase()}</h3>
          <table className="grid wide">
            <thead>
              <tr>
                <th>Base</th>
                <th>Name</th>
                <th>Type</th>
                <th>Runway m</th>
                <th>Aircraft</th>
                <th>Berths</th>
                <th>Lat, lon</th>
              </tr>
            </thead>
            <tbody>
              {list.map((b) => {
                const v = b.value;
                const field = (k: string) => (
                  <input
                    type="number"
                    value={v[k] === undefined ? '' : Number(v[k])}
                    onChange={(e) => editEntry(b.id, (d) => void (d[k] = Number(e.target.value)))}
                  />
                );
                const air = v['base_type'] !== 'port';
                return (
                  <tr key={b.id}>
                    <td>
                      <button className="link mono" onClick={() => set({ view: 'browse', selected: b.id })}>
                        {b.id}
                      </button>
                    </td>
                    <td>
                      <input value={String(v['name'] ?? '')} onChange={(e) => editEntry(b.id, (d) => void (d['name'] = e.target.value))} />
                    </td>
                    <td className="mono">{String(v['base_type'])}</td>
                    <td>{air ? field('runway_length_m') : null}</td>
                    <td>{air ? field('aircraft_capacity') : null}</td>
                    <td>{air ? null : field('ship_berths')}</td>
                    <td className="mono dim">
                      {Number(v['lat_deg']).toFixed(3)}, {Number(v['lon_deg']).toFixed(3)}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </section>
      ))}
    </div>
  );
}
