// Hot-seat handover screen (doc 12 §3, doc 10 §8): full screen in `map.land`, nothing else is
// rendered while it shows (no map, panels, alerts or toasts), and it never says why the device
// is being passed. The clock is paused (play.ts) until the next player continues.
import { type FormEvent, useState } from 'react';
import type { EngineBridge } from './bridge';
import { NationFlag } from './panels/NationFlag';
import { play, usePlay } from './play';

export function Handover({ bridge }: { bridge: EngineBridge }) {
  const side = usePlay((s) => s.handover);
  const war = usePlay((s) => s.war);
  const needsPin = usePlay((s) => (s.handover === null ? false : (s.hasPin[s.handover] ?? false)));
  const [pin, setPin] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  if (side === null || !war) return null;
  const info = war.sides[side];
  const nationIds = info?.nations ?? [];
  const nation = war.nations.find((n) => n.id === nationIds[0]);
  const name = nationIds.length === 1 ? (nation?.name ?? info?.name ?? '') : (info?.name ?? '');

  const submit = (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    play
      .continueAs(bridge, pin, bridge.report?.simTimeMs ?? 0)
      .then((err) => {
        setError(err);
        if (err) setPin('');
      })
      .catch((err: unknown) => setError(err instanceof Error ? err.message : String(err)))
      .finally(() => setBusy(false));
  };

  return (
    <main className="handover" data-testid="handover">
      <form className="handover-body" onSubmit={submit}>
        <NationFlag nation={nation} height={96} />
        <p className="handover-text">Pass the device to the {name} player</p>
        {needsPin && (
          <input
            className="handover-pin"
            type="password"
            inputMode="numeric"
            pattern="[0-9]{4}"
            maxLength={4}
            autoComplete="off"
            aria-label="PIN"
            placeholder="PIN"
            value={pin}
            onChange={(e) => setPin(e.target.value.replace(/\D/g, '').slice(0, 4))}
          />
        )}
        {error && <p className="handover-error">{error}</p>}
        <button type="submit" className="cmd" disabled={busy || (needsPin && pin.length !== 4)}>
          CONTINUE
        </button>
      </form>
    </main>
  );
}
