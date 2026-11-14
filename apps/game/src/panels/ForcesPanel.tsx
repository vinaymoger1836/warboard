// Forces (doc 10 §8, left panel): own groups with mini icons, status in state colours and a
// fuel bar; click selects and locates. Read from the side view twice a second.
import { FLAG, statusOf } from '@warboard/protocol';
import { iconDataUrl } from '@warboard/symbols';
import { memo, useEffect, useState } from 'react';
import type { EngineBridge } from '../bridge';
import { ui, useUi } from '../store';
import { fuelColour, statusColour } from './format';

interface Row {
  group: number;
  name: string;
  type: string;
  symbol: number;
  count: number;
  status: string;
  fuel: number | null;
  bingo: boolean;
}

function rows(bridge: EngineBridge): Row[] {
  const v = bridge.frame?.view;
  if (!v) return [];
  const u = v.units;
  const out = new Map<number, Row>();
  for (let i = 0; i < u.count; i++) {
    const flags = u.flags[i] ?? 0;
    if (flags & FLAG.BASE) continue;
    const id = u.id[i] ?? 0;
    const group = u.group[i] || id;
    const fuel = u.fuelPct[i] ?? 255;
    const r = out.get(group);
    if (!r) {
      out.set(group, {
        group,
        name: bridge.strings.name(group),
        type: bridge.strings.type(id),
        symbol: u.symbol[i] ?? 0,
        count: 1,
        status: statusOf(flags),
        fuel: fuel <= 100 ? fuel : null,
        bingo: (flags & FLAG.BINGO) !== 0,
      });
    } else {
      r.count++;
      if (fuel <= 100) r.fuel = Math.min(r.fuel ?? 100, fuel);
      r.bingo ||= (flags & FLAG.BINGO) !== 0;
      if (flags & FLAG.GROUP_LEAD) r.status = statusOf(flags);
    }
  }
  return [...out.values()].sort((a, b) => a.name.localeCompare(b.name));
}

const same = (a: Row, b: Row) =>
  a.name === b.name &&
  a.type === b.type &&
  a.symbol === b.symbol &&
  a.count === b.count &&
  a.status === b.status &&
  a.fuel === b.fuel &&
  a.bingo === b.bingo;

/** The new rows, reusing the old row objects that did not change, and the old list itself when
 *  nothing did: React then skips the 500-row list (and the memoised rows) entirely. */
function merge(prev: Row[], next: Row[]): Row[] {
  const before = new Map(prev.map((r) => [r.group, r]));
  let unchanged = prev.length === next.length;
  const out = next.map((r, i) => {
    const old = before.get(r.group);
    if (old && same(old, r)) {
      if (prev[i] !== old) unchanged = false;
      return old;
    }
    unchanged = false;
    return r;
  });
  return unchanged ? prev : out;
}

/** One group row; memoised so a refresh redraws only the rows whose state changed. */
const ForceRow = memo(function ForceRow({ r, selected }: { r: Row; selected: boolean }) {
  return (
    <li>
      <button type="button" className={selected ? 'force-row selected' : 'force-row'} onClick={() => ui.locate(r.group)}>
        <img src={iconDataUrl(r.symbol, 'own')} alt="" width={20} height={20} />
        <span className="force-name">
          {r.count > 1 ? `${r.count} × ` : ''}
          {r.type}
          <small>{r.name}</small>
        </span>
        <span className="force-status" style={{ color: statusColour(r.status) }}>
          {r.status}
        </span>
        <span className="fuel-bar" aria-label={r.fuel == null ? 'No fuel state' : `Fuel ${r.fuel}%`}>
          <span style={{ width: `${r.fuel ?? 0}%`, background: fuelColour(r.fuel, r.bingo) }} />
        </span>
      </button>
    </li>
  );
});

export function ForcesPanel({ bridge }: { bridge: EngineBridge }) {
  const [list, setList] = useState<Row[]>([]);
  const [open, setOpen] = useState(true);
  const selected = useUi((s) => s.selected);
  useEffect(() => {
    const tick = () => setList((prev) => merge(prev, rows(bridge)));
    tick();
    const t = setInterval(tick, 500);
    return () => clearInterval(t);
  }, [bridge]);
  return (
    <aside className={open ? 'forces panel' : 'forces panel closed'} aria-label="Forces">
      <button type="button" className="panel-header forces-header" aria-expanded={open} onClick={() => setOpen((o) => !o)}>
        <span>Forces</span>
        <span className="muted-inline">{list.length} groups</span>
      </button>
      {open && (
        <ul className="forces-list">
          {list.map((r) => (
            <ForceRow key={r.group} r={r} selected={r.group === selected} />
          ))}
        </ul>
      )}
    </aside>
  );
}
