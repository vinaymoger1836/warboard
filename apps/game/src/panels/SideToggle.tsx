// Side toggle (doc 10 §8, doc 12 §2): first item of the top bar in Command both, Hot-seat and
// Watch. Each position is a side's flag and nation id; UMPIRE appears when the umpire view is on
// (Command both only). Hot-seat adds HAND OVER. Tab moves to the next position (App.tsx).
import type { EngineBridge } from '../bridge';
import { play, UMPIRE, usePlay } from '../play';
import { NationFlag } from './NationFlag';

export function SideToggle({ bridge, toast }: { bridge: EngineBridge; toast: (t: string) => void }) {
  const war = usePlay((s) => s.war);
  const mode = usePlay((s) => s.mode);
  const sides = usePlay((s) => s.sides);
  const view = usePlay((s) => s.view);
  const umpireOn = usePlay((s) => s.umpireOn);
  if (!war || mode === 'vs_ai' || mode === null) return null;
  const fail = (err: unknown) => toast(err instanceof Error ? err.message : String(err));
  const pick = (side: number) => void play.choose(bridge, side).catch(fail);
  return (
    <div className="side-toggle">
      <div className="speeds" role="group" aria-label="Side">
        {sides.map((side) => {
          const info = war.sides[side];
          const first = info?.nations[0] ?? '';
          const nation = war.nations.find((n) => n.id === first);
          return (
            <button
              key={side}
              type="button"
              className={view === side ? 'speed active side-pos' : 'speed side-pos'}
              aria-pressed={view === side}
              title={`${info?.name ?? ''}: switch side (Tab)`}
              onClick={() => pick(side)}
            >
              <NationFlag nation={nation} height={12} />
              {first.toUpperCase()}
            </button>
          );
        })}
        {umpireOn && (
          <button
            type="button"
            className={view === UMPIRE ? 'speed active' : 'speed'}
            aria-pressed={view === UMPIRE}
            title="Umpire view: both sides exactly (Tab)"
            onClick={() => pick(UMPIRE)}
          >
            UMPIRE
          </button>
        )}
      </div>
      {mode === 'command_both' && (
        <button
          type="button"
          className="chrome-button"
          aria-pressed={umpireOn}
          title="Show both sides exactly, read-only. The war is marked as umpire used."
          onClick={() => void play.setUmpire(bridge, !umpireOn).catch(fail)}
        >
          {umpireOn ? 'UMPIRE ON' : 'UMPIRE OFF'}
        </button>
      )}
      {mode === 'hot_seat' && (
        <button type="button" className="cmd" onClick={() => play.handOver()}>
          HAND OVER
        </button>
      )}
    </div>
  );
}
