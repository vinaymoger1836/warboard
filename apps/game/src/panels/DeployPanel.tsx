// Deploy yourself (doc 03 §2, ADR-046): the active side's inventory pool and its preset bases.
// Aircraft go to an air base with room, ships to a port, sites and ground forces are set up
// beside the chosen base. The engine holds the clock until every deploying side has finished;
// in Command both and Hot-seat the next human side deploys in turn.
import type { DeploymentView, PoolLine } from '@warboard/protocol';
import { useCallback, useEffect, useState } from 'react';
import type { EngineBridge } from '../bridge';
import { sendCommand } from '../orders';
import { play, usePlay } from '../play';

const REFRESH_MS = 1_000;

function basesFor(view: DeploymentView, line: PoolLine) {
  return view.bases.filter(
    (b) =>
      b.nation === line.nation &&
      (line.place === 'airfield' ? b.air && b.aircraft_room > 0 : line.place === 'port' ? b.port && b.berth_room > 0 : true),
  );
}

function Line({ bridge, view, line, toast, onDone }: { bridge: EngineBridge; view: DeploymentView; line: PoolLine; toast: (t: string) => void; onDone: () => void }) {
  const hosts = basesFor(view, line);
  const [base, setBase] = useState<number | null>(hosts[0]?.id ?? null);
  const [count, setCount] = useState(Math.min(line.place === 'airfield' ? 4 : 1, line.left));
  const [loadout, setLoadout] = useState<string>('');
  const chosen = hosts.find((b) => b.id === base) ? base : (hosts[0]?.id ?? null);
  const room = hosts.find((b) => b.id === chosen);
  const max = Math.min(
    line.left,
    line.place === 'airfield' ? (room?.aircraft_room ?? 0) : line.place === 'port' ? (room?.berth_room ?? 0) : line.left,
    64,
  );
  const deploy = () => {
    if (chosen === null) return;
    void sendCommand(
      bridge,
      { type: 'DeployForce', force: line.id, count: Math.max(1, Math.min(count, max)), base: chosen, ...(loadout ? { loadout } : {}) },
      toast,
    ).then((ok) => ok && onDone());
  };
  return (
    <li className="deploy-line">
      <div className="deploy-what">
        <b>{line.name}</b>
        <span className="muted-inline">
          {line.nation.toUpperCase()} · {line.class_name || line.place} · {line.left} left
        </span>
      </div>
      {hosts.length === 0 ? (
        <span className="muted-inline">No base of {line.nation.toUpperCase()} has room</span>
      ) : (
        <div className="deploy-controls">
          <select aria-label={`Base for ${line.name}`} value={chosen ?? ''} onChange={(e) => setBase(Number(e.target.value))}>
            {hosts.map((b) => (
              <option key={b.id} value={b.id}>
                {b.name}
                {line.place === 'airfield' ? ` (${b.aircraft_room} free)` : line.place === 'port' ? ` (${b.berth_room} berths)` : ''}
              </option>
            ))}
          </select>
          {line.loadouts.length > 1 && (
            <select aria-label={`Loadout for ${line.name}`} value={loadout} onChange={(e) => setLoadout(e.target.value)}>
              <option value="">Default loadout</option>
              {line.loadouts.map((l) => (
                <option key={l.id} value={l.id}>
                  {l.name}
                </option>
              ))}
            </select>
          )}
          <input
            type="number"
            min={1}
            max={Math.max(1, max)}
            aria-label={`How many ${line.name}`}
            value={count}
            onChange={(e) => setCount(Number(e.target.value))}
          />
          <button type="button" className="cmd" disabled={max < 1} onClick={deploy}>
            DEPLOY
          </button>
        </div>
      )}
    </li>
  );
}

export function DeployPanel({ bridge, toast }: { bridge: EngineBridge; toast: (t: string) => void }) {
  const active = usePlay((s) => s.active);
  const sides = usePlay((s) => s.sides);
  const mode = usePlay((s) => s.mode);
  const [view, setView] = useState<DeploymentView | null>(null);
  const [others, setOthers] = useState<number[]>([]);

  const load = useCallback(() => {
    bridge
      .request({ type: 'deployment', side: active })
      .then((v) => {
        setView(v);
        play.setDeploying(v.clock_held);
      })
      .catch(() => setView(null));
    // Which other human sides still deploy (the next in turn).
    Promise.all(
      sides
        .filter((s) => s !== active)
        .map((s) =>
          bridge
            .request({ type: 'deployment', side: s })
            .then((v) => (v.deploying ? s : null))
            .catch(() => null),
        ),
    )
      .then((list) => setOthers(list.filter((s): s is number => s !== null)))
      .catch(() => setOthers([]));
  }, [bridge, active, sides]);

  useEffect(() => {
    load();
    const t = setInterval(load, REFRESH_MS);
    return () => clearInterval(t);
  }, [load]);

  if (!view?.deploying) {
    return view?.clock_held ? (
      <aside className="forces panel deploy-panel" aria-label="Deployment">
        <h2 className="panel-header">Deployment</h2>
        <p className="muted deploy-note">Your side has finished. The clock starts when every side has deployed.</p>
      </aside>
    ) : null;
  }
  const finish = () => {
    void sendCommand(bridge, { type: 'FinishDeployment' }, toast).then((ok) => {
      if (!ok) return;
      load();
      const next = others[0];
      if (next === undefined) return;
      if (mode === 'hot_seat') play.handOver(next);
      else void play.switchTo(bridge, next).catch((err: unknown) => toast(String(err)));
    });
  };
  const total = view.pool.reduce((n, l) => n + l.left, 0);
  return (
    <aside className="forces panel deploy-panel" aria-label="Deployment">
      <h2 className="panel-header">Deploy your forces</h2>
      <p className="muted deploy-note">
        The clock is paused. Station forces at your bases, then finish. {total} still in the pool; whatever is left when
        you finish stays out of the war.
      </p>
      <ul className="deploy-list">
        {view.pool.map((line) => (
          <Line key={`${line.nation}-${line.id}`} bridge={bridge} view={view} line={line} toast={toast} onDone={load} />
        ))}
      </ul>
      <div className="card-buttons">
        <button type="button" className="cmd" onClick={finish}>
          FINISH DEPLOYMENT
        </button>
      </div>
    </aside>
  );
}
