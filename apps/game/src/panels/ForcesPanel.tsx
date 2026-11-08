// Forces (doc 10 §8, left panel): own groups with mini icons, status in state colours and a
// fuel bar; click selects and locates. Read from the side view twice a second.
import { FLAG, statusOf } from '@warboard/protocol';
import { iconDataUrl } from '@warboard/symbols';
import { useEffect, useState } from 'react';
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

export function ForcesPanel({ bridge }: { bridge: EngineBridge }) {
  const [list, setList] = useState<Row[]>([]);
  const [open, setOpen] = useState(true);
  const selected = useUi((s) => s.selected);
  useEffect(() => {
    const tick = () => setList(rows(bridge));
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
            <li key={r.group}>
              <button
                type="button"
                className={r.group === selected ? 'force-row selected' : 'force-row'}
                onClick={() => ui.locate(r.group)}
              >
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
          ))}
        </ul>
      )}
    </aside>
  );
}
