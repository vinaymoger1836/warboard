// Main menu and war setup (doc 03 §1–2, doc 12 §4): coalitions from any nations, the Players
// step (Human or AI per coalition, difficulty, "same person or two players", hot-seat settings),
// start mode, start time, default ROE per side, and the pack check (ADR-022). Same panel family
// as the unit card (doc 10 §7–§9). The engine checks the setup again when the war starts.
import type {
  Controller,
  Difficulty,
  HandOver,
  HotSeat,
  NationOption,
  PlayMode,
  Roe,
  StartMode,
  Urgent,
  WarSetup,
} from '@warboard/protocol';
import { useEffect, useMemo, useState } from 'react';
import type { EngineBridge } from '../bridge';
import type { PinChoice, SetupPreview } from '../engine/messages';
import type { InstalledPack } from '../packs/types';
import { NationFlag } from '../panels/NationFlag';
import { ROE_LEVELS } from '../panels/TopBar';

interface Props {
  bridge: EngineBridge;
  packs: InstalledPack[];
  toast: (t: string) => void;
  onPacks: () => void;
  onSaves: () => void;
}

/** A coalition while it is being built. */
interface Coalition {
  name: string;
  nations: string[];
  controller: Controller;
  difficulty: Difficulty;
  roe: Roe;
}

interface Draft {
  name: string;
  sides: [Coalition, Coalition];
  /** Two humans: the same person or two players. */
  together: Extract<PlayMode, 'command_both' | 'hot_seat'>;
  handOver: HandOver;
  urgent: Urgent;
  pins: [string, string];
  startMode: StartMode;
  startNow: boolean;
  /** `YYYY-MM-DDTHH:MM` in UTC. */
  startAt: string;
  allowNoPacks: boolean;
  seed: number;
}

const STEPS = ['Coalitions', 'Players', 'Start', 'Check'] as const;
type Step = (typeof STEPS)[number];

const utcNow = () => new Date().toISOString().slice(0, 16);

/** The quick-start pairing when the content has it, else the first two nations. */
function defaultDraft(nations: NationOption[]): Draft {
  const has = (id: string) => nations.some((n) => n.id === id);
  const a = ['ald', 'hlv', 'mrv'].filter(has);
  const b = ['vsm', 'kld'].filter(has);
  const pick = (ids: string[], i: number) => (ids.length > 0 ? ids : nations[i] ? [nations[i].id] : []);
  return {
    name: 'New war',
    sides: [
      { name: 'Coalition A', nations: pick(a, 0), controller: 'human', difficulty: 'normal', roe: 'bravo' },
      { name: 'Coalition B', nations: pick(b, 1), controller: 'ai', difficulty: 'normal', roe: 'bravo' },
    ],
    together: 'command_both',
    handOver: 'OnRequest',
    urgent: 'interrupt',
    pins: ['', ''],
    startMode: 'garrisoned',
    startNow: true,
    startAt: utcNow(),
    allowNoPacks: false,
    seed: Math.floor(Math.random() * 2 ** 31),
  };
}

const humans = (d: Draft) => d.sides.filter((s) => s.controller === 'human').length;

export function modeOf(d: Draft): PlayMode {
  const h = humans(d);
  return h === 0 ? 'watch' : h === 1 ? 'vs_ai' : d.together;
}

export function toSetup(d: Draft): WarSetup {
  return {
    name: d.name.trim() || 'New war',
    seed: d.seed,
    start_utc: `${(d.startNow ? utcNow() : d.startAt).slice(0, 16)}:00Z`,
    start_mode: d.startMode,
    mode: modeOf(d),
    sides: d.sides.map((s, i) => ({
      name: s.name.trim() || `Coalition ${'AB'[i]}`,
      nations: s.nations,
      controller: s.controller,
      difficulty: s.difficulty,
      roe: s.roe,
    })),
  };
}

/** A theatre pack (not the world pack) covers the war's area. */
function packCovers(packs: InstalledPack[], e: [number, number, number, number]): InstalledPack | undefined {
  return packs.find((p) => {
    const [w, s, east, n] = p.manifest.extent;
    const world = w <= -180 && east >= 180;
    return !world && w <= e[0] && s <= e[1] && east >= e[2] && n >= e[3];
  });
}

function Seg<T extends string>({
  value,
  options,
  onChange,
  label,
}: {
  value: T;
  options: [T, string][];
  onChange: (v: T) => void;
  label: string;
}) {
  return (
    <div className="segmented" role="group" aria-label={label}>
      {options.map(([v, text]) => (
        <button
          key={v}
          type="button"
          className={v === value ? 'seg active' : 'seg'}
          aria-pressed={v === value}
          onClick={() => onChange(v)}
        >
          {text}
        </button>
      ))}
    </div>
  );
}

export function Setup({ bridge, packs, toast, onPacks, onSaves }: Props) {
  const [nations, setNations] = useState<NationOption[] | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [step, setStep] = useState<Step | null>(null);
  // The preview of the setup it was made for; a changed setup shows "checking" until its own arrives.
  const [checked, setChecked] = useState<{ key: string; result: SetupPreview } | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    bridge
      .request({ type: 'nationOptions' })
      .then((n) => {
        setNations(n);
        setDraft(defaultDraft(n));
      })
      .catch((err: unknown) => toast(err instanceof Error ? err.message : String(err)));
  }, [bridge, toast]);

  const setup = useMemo(() => (draft ? toSetup(draft) : null), [draft]);
  const setupKey = setup ? JSON.stringify({ ...setup, start_utc: '' }) : '';
  const preview = checked?.key === setupKey ? checked.result : null;
  useEffect(() => {
    if (step !== 'Check' || !setup) return;
    let live = true;
    bridge
      .request({ type: 'previewSetup', setup })
      .then((result) => live && setChecked({ key: setupKey, result }))
      .catch((err: unknown) => live && setChecked({ key: setupKey, result: { ok: false, error: String(err) } }));
    return () => {
      live = false;
    };
    // setupKey stands for the setup (its start time ticks with the clock when "now").
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [bridge, step, setupKey]);

  const fail = (err: unknown) => toast(err instanceof Error ? err.message : String(err));
  const quickStart = () => {
    setBusy(true);
    bridge
      .request({ type: 'quickStart' })
      .catch(fail)
      .finally(() => setBusy(false));
  };

  if (step === null || !draft || !nations) {
    return (
      <main className="setup-screen">
        <div className="menu panel" role="dialog" aria-label="Main menu">
          <h1 className="menu-title">WARBOARD</h1>
          <p className="muted">Strategy on real terrain with fictional nations.</p>
          <div className="menu-buttons">
            <button type="button" className="cmd" disabled={!draft} onClick={() => setStep('Coalitions')}>
              NEW WAR
            </button>
            <button type="button" className="cmd" disabled={busy} onClick={quickStart} title="Human against the AI">
              QUICK START
            </button>
            <button type="button" className="cmd" onClick={onSaves}>
              LOAD WAR
            </button>
            <button type="button" className="cmd" onClick={onPacks}>
              MAP PACKS
            </button>
          </div>
        </div>
      </main>
    );
  }

  const update = (patch: Partial<Draft>) => setDraft({ ...draft, ...patch });
  const updateSide = (i: 0 | 1, patch: Partial<Coalition>) => {
    const sides: [Coalition, Coalition] = [...draft.sides];
    sides[i] = { ...sides[i], ...patch };
    update({ sides });
  };
  const assign = (id: string, to: 0 | 1 | null) => {
    const sides = draft.sides.map((s, i) => ({
      ...s,
      nations: i === to ? (s.nations.includes(id) ? s.nations : [...s.nations, id]) : s.nations.filter((n) => n !== id),
    })) as [Coalition, Coalition];
    update({ sides });
  };
  const nation = (id: string) => nations.find((n) => n.id === id);
  const mode = modeOf(draft);
  const extent = preview?.ok ? preview.extent : null;
  const covering = extent ? packCovers(packs, extent) : undefined;
  const ready = preview?.ok === true && (covering !== undefined || draft.allowNoPacks);
  const at = STEPS.indexOf(step);
  const canNext =
    step === 'Coalitions'
      ? draft.sides.every((s) => s.nations.length > 0)
      : step === 'Players'
        ? mode !== 'hot_seat' || draft.pins.every((p) => p === '' || /^\d{4}$/.test(p))
        : true;

  const start = () => {
    if (!setup) return;
    setBusy(true);
    const hotSeat: HotSeat | undefined = mode === 'hot_seat' ? { hand_over: draft.handOver, urgent: draft.urgent } : undefined;
    const pins: PinChoice[] =
      mode === 'hot_seat' ? draft.pins.map((pin, side) => ({ side, pin })).filter((p) => p.pin !== '') : [];
    bridge
      .request({ type: 'newWar', setup, pins, ...(hotSeat ? { hotSeat } : {}) })
      .catch(fail)
      .finally(() => setBusy(false));
  };

  return (
    <main className="setup-screen">
      <div className="setup panel" role="dialog" aria-label="War setup">
        <h2 className="panel-header">
          War setup
          <span className="setup-steps">
            {STEPS.map((s, i) => (
              <span key={s} className={s === step ? 'setup-step active' : i < at ? 'setup-step done' : 'setup-step'}>
                {i + 1} {s}
              </span>
            ))}
          </span>
        </h2>
        <div className="setup-body">
          {step === 'Coalitions' && (
            <>
              <p className="muted">
                Put any nations into coalition A or B. Nations in neither stay neutral. A nation without forces can be
                chosen but has nothing to field.
              </p>
              <div className="coalition-names">
                {([0, 1] as const).map((i) => (
                  <label key={i} className="field">
                    <span className="field-label">Coalition {'AB'[i]}</span>
                    <input
                      value={draft.sides[i].name}
                      maxLength={40}
                      onChange={(e) => updateSide(i, { name: e.target.value })}
                    />
                  </label>
                ))}
              </div>
              <table className="nation-table">
                <thead>
                  <tr>
                    <th>Nation</th>
                    <th>Forces</th>
                    <th>Doctrine</th>
                    <th>Coalition</th>
                  </tr>
                </thead>
                <tbody>
                  {nations.map((n) => {
                    const where = draft.sides[0].nations.includes(n.id) ? 'a' : draft.sides[1].nations.includes(n.id) ? 'b' : 'none';
                    return (
                      <tr key={n.id}>
                        <td>
                          <span className="nation-cell" title={n.note}>
                            <NationFlag nation={n} height={14} />
                            <b>{n.name}</b>
                            <span className="muted-inline mono">{n.id.toUpperCase()}</span>
                          </span>
                        </td>
                        <td className="muted-inline">
                          {n.platforms} platforms · {n.systems} systems · {n.bases} bases
                        </td>
                        <td className="muted-inline">{n.doctrine}</td>
                        <td>
                          <Seg
                            label={`Coalition of ${n.name}`}
                            value={where}
                            options={[
                              ['a', 'A'],
                              ['b', 'B'],
                              ['none', '—'],
                            ]}
                            onChange={(v) => assign(n.id, v === 'a' ? 0 : v === 'b' ? 1 : null)}
                          />
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </>
          )}

          {step === 'Players' && (
            <>
              {([0, 1] as const).map((i) => {
                const s = draft.sides[i];
                return (
                  <div key={i} className="setup-row">
                    <span className="setup-side">
                      {s.nations.map((id) => (
                        <NationFlag key={id} nation={nation(id)} height={14} title={nation(id)?.name} />
                      ))}
                      <b>{s.name}</b>
                    </span>
                    <Seg
                      label={`Who plays ${s.name}`}
                      value={s.controller}
                      options={[
                        ['human', 'HUMAN'],
                        ['ai', 'AI'],
                      ]}
                      onChange={(controller) => updateSide(i, { controller })}
                    />
                    {s.controller === 'ai' && (
                      <Seg
                        label={`Difficulty of ${s.name}`}
                        value={s.difficulty}
                        options={[
                          ['easy', 'EASY'],
                          ['normal', 'NORMAL'],
                          ['hard', 'HARD'],
                        ]}
                        onChange={(difficulty) => updateSide(i, { difficulty })}
                      />
                    )}
                  </div>
                );
              })}
              <p className="muted">
                {mode === 'vs_ai' && 'You play one coalition against the AI.'}
                {mode === 'watch' && 'Two AIs: you watch (Watch mode). Tab switches between their pictures.'}
                {(mode === 'command_both' || mode === 'hot_seat') && 'Two humans: same person or two players?'}
                {' AI difficulty changes reaction time and coordination, never what the AI knows.'}
              </p>
              {humans(draft) === 2 && (
                <div className="setup-block">
                  <Seg
                    label="Same person or two players"
                    value={draft.together}
                    options={[
                      ['command_both', 'SAME PERSON (COMMAND BOTH)'],
                      ['hot_seat', 'TWO PLAYERS (HOT-SEAT)'],
                    ]}
                    onChange={(together) => update({ together })}
                  />
                  {draft.together === 'hot_seat' && (
                    <div className="setup-grid">
                      <span className="field-label">Hand over</span>
                      <Seg
                        label="Hand over"
                        value={typeof draft.handOver === 'string' ? 'request' : String(draft.handOver.Turns.minutes)}
                        options={[
                          ['request', 'ON REQUEST'],
                          ['5', 'EVERY 5 MIN'],
                          ['15', '15 MIN'],
                          ['30', '30 MIN'],
                        ]}
                        onChange={(v) => update({ handOver: v === 'request' ? 'OnRequest' : { Turns: { minutes: Number(v) } } })}
                      />
                      <span className="field-label">Urgent events for the waiting side</span>
                      <Seg
                        label="Urgent events"
                        value={draft.urgent}
                        options={[
                          ['interrupt', 'INTERRUPT'],
                          ['hold', "DON'T INTERRUPT"],
                        ]}
                        onChange={(urgent) => update({ urgent })}
                      />
                      {([0, 1] as const).map((i) => (
                        <label key={i} className="field contents">
                          <span className="field-label">PIN for {draft.sides[i].name} (optional)</span>
                          <input
                            className="pin-input"
                            inputMode="numeric"
                            maxLength={4}
                            placeholder="none"
                            value={draft.pins[i]}
                            onChange={(e) => {
                              const pins: [string, string] = [...draft.pins];
                              pins[i] = e.target.value.replace(/\D/g, '').slice(0, 4);
                              update({ pins });
                            }}
                          />
                        </label>
                      ))}
                      <span className="muted span-2">A PIN is a courtesy between players, not security.</span>
                    </div>
                  )}
                </div>
              )}
            </>
          )}

          {step === 'Start' && (
            <div className="setup-grid">
              <span className="field-label">War name</span>
              <input value={draft.name} maxLength={60} onChange={(e) => update({ name: e.target.value })} />
              <span className="field-label">Start mode</span>
              <div>
                <Seg
                  label="Start mode"
                  value={draft.startMode}
                  options={[
                    ['garrisoned', 'GARRISONED'],
                    ['deploy', 'DEPLOY YOURSELF'],
                  ]}
                  onChange={(startMode) => update({ startMode })}
                />
                <p className="muted">
                  {draft.startMode === 'garrisoned'
                    ? 'Every force starts at its nation’s bases, air defence on home soil.'
                    : 'Bases start open and empty: each human side stations its forces in turn, with the clock paused. AI sides start garrisoned.'}
                </p>
              </div>
              <span className="field-label">Start time (UTC)</span>
              <div className="inline-fields">
                <Seg
                  label="Start time"
                  value={draft.startNow ? 'now' : 'chosen'}
                  options={[
                    ['now', 'NOW'],
                    ['chosen', 'CHOSEN'],
                  ]}
                  onChange={(v) => update({ startNow: v === 'now' })}
                />
                {!draft.startNow && (
                  <input
                    type="datetime-local"
                    aria-label="Start time (UTC)"
                    value={draft.startAt}
                    onChange={(e) => update({ startAt: e.target.value })}
                  />
                )}
              </div>
              {([0, 1] as const).map((i) => (
                <div key={i} className="contents">
                  <span className="field-label">Default ROE, {draft.sides[i].name}</span>
                  <Seg
                    label={`Default ROE of ${draft.sides[i].name}`}
                    value={draft.sides[i].roe}
                    options={ROE_LEVELS.map(([letter, roe]) => [roe, letter] as [Roe, string])}
                    onChange={(roe) => updateSide(i, { roe })}
                  />
                </div>
              ))}
            </div>
          )}

          {step === 'Check' && (
            <div className="setup-check">
              {!preview && <p className="muted">Checking the setup…</p>}
              {preview && !preview.ok && <p className="warn">{preview.error}</p>}
              {preview?.ok && (
                <>
                  <p>
                    {mode === 'vs_ai' && 'Human against the AI'}
                    {mode === 'command_both' && 'Command both: you give the orders of both coalitions'}
                    {mode === 'hot_seat' && 'Hot-seat: two players take turns on this device'}
                    {mode === 'watch' && 'Watch: two AIs'}
                    {' · '}
                    {preview.report.platforms} platforms and {preview.report.systems} systems start in the war
                    {preview.report.deploy_sides.length > 0 && ', the rest is deployed by hand'}.
                  </p>
                  {preview.report.unplaced.length > 0 && (
                    <div className="card-note">
                      <b>Not placed</b>
                      <ul className="plain-list">
                        {preview.report.unplaced.map((u) => (
                          <li key={`${u.nation}-${u.id}`}>
                            {u.nation.toUpperCase()} {u.id} × {u.count}: {u.reason}
                          </li>
                        ))}
                      </ul>
                    </div>
                  )}
                  <h3 className="block-title">Region packs</h3>
                  {covering ? (
                    <p className="ok">
                      {covering.manifest.name} covers the war ({covering.id}@{covering.version}).
                    </p>
                  ) : (
                    <>
                      <p className="warn">
                        No installed region pack covers the war ({extent?.map((v) => v.toFixed(1)).join(', ')}). Without
                        it, line of sight ignores terrain (ADR-022).
                      </p>
                      <div className="inline-fields">
                        <button type="button" className="cmd" onClick={onPacks}>
                          OPEN MAP PACKS
                        </button>
                        <label className="toggle">
                          <input
                            type="checkbox"
                            checked={draft.allowNoPacks}
                            onChange={(e) => update({ allowNoPacks: e.target.checked })}
                          />
                          Start without the region pack
                        </label>
                      </div>
                    </>
                  )}
                </>
              )}
            </div>
          )}
        </div>
        <div className="card-buttons setup-buttons">
          <button type="button" className="cmd" onClick={() => setStep(at === 0 ? null : (STEPS[at - 1] ?? null))}>
            {at === 0 ? 'MENU' : 'BACK'}
          </button>
          {step !== 'Check' ? (
            <button type="button" className="cmd" disabled={!canNext} onClick={() => setStep(STEPS[at + 1] ?? 'Check')}>
              NEXT
            </button>
          ) : (
            <button type="button" className="cmd" disabled={!ready || busy} onClick={start}>
              START WAR
            </button>
          )}
        </div>
      </div>
    </main>
  );
}
