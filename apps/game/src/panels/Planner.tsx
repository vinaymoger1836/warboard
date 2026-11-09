// Operation planner (doc 03 §8, doc 10 §8): phases with their tasks, a trigger, an end
// condition and abort conditions. Routes, orbits and targets are drawn on the map with the
// same tools as orders (the tool result comes back here instead of becoming an order). The
// engine checks the plan on every edit (planner validation: trigger cycles, units, stock,
// fuel, ROE) and the issues are listed before CREATE.
import type { CondSpec, Issue, OperationSpec, PhaseSpec, PhaseTaskSpec, TaskOrder } from '@warboard/protocol';
import { useEffect, useMemo, useState } from 'react';
import type { EngineBridge } from '../bridge';
import { type LonLat, type PlanRoute, ui, useUi } from '../store';
import { CloseGlyph } from './glyphs';
import { rememberDraft } from './Operations';

/** Names of units added to a plan (the plan itself holds ids only). */
const unitNames = new Map<number, string>();

type Role = 'trigger' | 'end' | 'abort';
type Leaf = Exclude<CondSpec, { kind: 'all' } | { kind: 'any' }>;
type LeafKind = Leaf['kind'];

const LEAVES: Record<Role, [LeafKind, string][]> = {
  trigger: [
    ['start', 'H + delay'],
    ['phase_ended', 'After phase'],
    ['phase_started', 'With phase'],
    ['at_time', 'At time (UTC)'],
    ['manual', 'Manual (GO)'],
    ['contacts', 'Contacts seen'],
    ['intel_report', 'Satellite report'],
  ],
  end: [
    ['tasks_done', 'All tasks done'],
    ['arrived', 'Arrived'],
    ['weapons_done', 'Weapons ended'],
    ['targets_destroyed', 'Targets destroyed'],
    ['elapsed', 'Time limit'],
    ['manual', 'Manual (END)'],
    ['bingo', 'Bingo'],
    ['phase_ended', 'After phase'],
    ['contacts', 'Contacts seen'],
  ],
  abort: [
    ['losses_pct', 'Losses above'],
    ['under_attack', 'Under attack'],
    ['fuel_below', 'Fuel below'],
    ['elapsed', 'Time limit'],
    ['contacts', 'Contacts seen'],
  ],
};

function defaultLeaf(kind: LeafKind, phases: PhaseSpec[], self: string): Leaf {
  const other = phases.find((p) => p.id !== self)?.id ?? self;
  switch (kind) {
    case 'start':
      return { kind, delay_min: 0 };
    case 'at_time':
      return { kind, utc: new Date().toISOString().slice(0, 19) + 'Z' };
    case 'phase_started':
    case 'phase_ended':
      return { kind, phase: other, delay_min: 0 };
    case 'contacts':
      return { kind, domain: 'surface', level: 'class', count: 1 };
    case 'elapsed':
      return { kind, min: 60 };
    case 'losses_pct':
      return { kind, pct: 50 };
    case 'fuel_below':
      return { kind, pct: 25 };
    default:
      return { kind } as Leaf;
  }
}

/** A condition as a combinator over leaves (the planner edits one level; scenarios may nest). */
function flatten(c: CondSpec | undefined | null, role: Role): { op: 'all' | 'any'; leaves: CondSpec[] } {
  if (!c) return { op: role === 'trigger' ? 'all' : 'any', leaves: [] };
  if (c.kind === 'all' || c.kind === 'any') return { op: c.kind, leaves: c.of };
  return { op: role === 'trigger' ? 'all' : 'any', leaves: [c] };
}

function join(op: 'all' | 'any', leaves: CondSpec[]): CondSpec | null {
  if (leaves.length === 0) return null;
  if (leaves.length === 1) return leaves[0] ?? null;
  return op === 'all' ? { kind: 'all', of: leaves } : { kind: 'any', of: leaves };
}

const TASK_KINDS: [string, string][] = [
  ['move', 'Move'],
  ['patrol_route', 'Patrol route'],
  ['patrol_circle', 'Patrol circle'],
  ['cap', 'CAP'],
  ['jam', 'Stand-off jamming'],
  ['strike', 'Strike'],
  ['suppress', 'Air-defence suppression'],
  ['rtb', 'Return to base'],
];

function newTask(kind: string, prev: TaskOrder | null): TaskOrder {
  const pts: LonLat[] =
    prev && 'waypoints' in prev ? prev.waypoints : prev && 'centre' in prev ? [prev.centre] : prev && 'point' in prev && prev.point ? [prev.point] : [];
  const at = pts[0] ?? [0, 0];
  switch (kind) {
    case 'move':
      return { kind: 'move', waypoints: pts };
    case 'patrol_route':
      return { kind: 'patrol_route', waypoints: pts, loop: true };
    case 'patrol_circle':
      return { kind: 'patrol_circle', centre: at, radius_km: 20 };
    case 'cap':
      return { kind: 'cap', centre: at, radius_km: 30, commit_km: 60 };
    case 'jam':
      return { kind: 'jam', centre: at, radius_km: 15 };
    case 'strike':
      return { kind: 'strike', point: at };
    case 'suppress':
      return { kind: 'suppress', point: at };
    default:
      return { kind: 'rtb' };
  }
}

/** Where a task goes on the map. */
function taskRoute(t: TaskOrder): { route: LonLat[]; radiusKm?: number } {
  if ('waypoints' in t) return { route: t.waypoints };
  if ('centre' in t && 'radius_km' in t) return { route: [t.centre], radiusKm: t.radius_km };
  if ('point' in t && t.point) return { route: [t.point] };
  return { route: [] };
}

export function Planner({ bridge, toast }: { bridge: EngineBridge; toast: (t: string) => void }) {
  const planner = useUi((s) => s.planner);
  const planTool = useUi((s) => s.planTool);
  const tool = useUi((s) => s.tool);
  const capture = useUi((s) => s.capture);
  const details = useUi((s) => s.details);
  const [issues, setIssues] = useState<Issue[]>([]);
  /** Which task is drawing: `[phase, task]`. */
  const [drawing, setDrawing] = useState<[number, number] | null>(null);
  const draft = planner?.draft ?? null;

  const set = (d: OperationSpec) => planner && ui.set({ planner: { ...planner, draft: d } });
  const setPhase = (i: number, p: PhaseSpec) => draft && set({ ...draft, phases: draft.phases.map((x, k) => (k === i ? p : x)) });
  const setTask = (i: number, j: number, t: PhaseTaskSpec) => {
    const p = draft?.phases[i];
    if (p) setPhase(i, { ...p, tasks: p.tasks.map((x, k) => (k === j ? t : x)) });
  };

  // The plan's routes on the map while it is edited.
  const routes: PlanRoute[] = useMemo(
    () =>
      draft?.phases.flatMap((p, i) => p.tasks.map((t) => ({ phase: i + 1, kind: t.fire ? 'fire' : t.task.kind, ...taskRoute(t.task), active: true }))) ?? [],
    [draft],
  );
  useEffect(() => {
    ui.set({ planRoutes: routes });
  }, [routes]);
  useEffect(() => () => ui.set({ planRoutes: [], capture: false, planTool: null }), []);

  // Engine validation on every edit (debounced).
  useEffect(() => {
    if (!draft) return;
    const t = setTimeout(() => {
      bridge
        .request({ type: 'checkOperation', spec: draft })
        .then(setIssues)
        .catch((err: unknown) => setIssues([{ severity: 'error', text: String(err) }]));
    }, 250);
    return () => clearTimeout(t);
  }, [bridge, draft]);

  // A route or circle drawn for a task.
  useEffect(() => {
    if (!planTool || !drawing || !draft) return;
    const [i, j] = drawing;
    const t = draft.phases[i]?.tasks[j];
    ui.set({ planTool: null });
    setDrawing(null);
    if (!t) return;
    const task = t.task;
    if (planTool.kind === 'route' && 'waypoints' in task) setTask(i, j, { ...t, task: { ...task, waypoints: planTool.points } });
    if (planTool.kind === 'circle' && planTool.centre && 'centre' in task)
      setTask(i, j, { ...t, task: { ...task, centre: planTool.centre, radius_km: Math.max(1, Math.round(planTool.radiusKm)) } });
    // set* read the draft of this render; the effect runs once per tool result.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [planTool]);

  // A target picked for a strike: a contact of our picture or a point.
  useEffect(() => {
    if (!capture || !drawing || !draft || tool?.kind !== 'strike' || !tool.aim) return;
    const [i, j] = drawing;
    const t = draft.phases[i]?.tasks[j];
    ui.set({ tool: null, capture: false });
    setDrawing(null);
    if (!t || (t.task.kind !== 'strike' && t.task.kind !== 'suppress')) return;
    const aim = tool.aim;
    const base = { ...t.task, contact: undefined, point: undefined };
    setTask(i, j, { ...t, task: 'contact' in aim ? { ...base, contact: aim.contact } : { ...base, point: aim.point } });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tool, capture]);

  if (!planner || !draft) return null;
  const errors = issues.filter((x) => x.severity === 'error');

  const draw = (i: number, j: number, kind: 'route' | 'circle' | 'strike') => {
    setDrawing([i, j]);
    ui.set({
      capture: true,
      planTool: null,
      tool: kind === 'route' ? { kind: 'route', patrol: false, points: [] } : kind === 'circle' ? { kind: 'circle', centre: null, radiusKm: 20 } : { kind: 'strike', aim: null },
    });
    toast(kind === 'route' ? 'Click waypoints, double-click or Enter to finish' : kind === 'circle' ? 'Click the centre, then the edge' : 'Click a contact or a point');
  };

  const create = async (start: boolean) => {
    const ack = await bridge.request({
      type: 'order',
      order: { tick: 0, side: 0, type: 'CreateOperation', operation: draft, replace: planner.replace ?? undefined, start },
    });
    if (!ack.ok) {
      toast(ack.reason);
      return;
    }
    rememberDraft(ack.ack.seq, draft);
    toast(start ? `${draft.name} started` : `${draft.name} planned`);
    ui.set({ planner: null });
  };

  return (
    <div className="panel planner" role="dialog" aria-label="Operation planner">
      <h2 className="panel-header">
        {planner.replace === null ? 'New operation' : `Edit ${draft.name}`}
        <button type="button" className="close" aria-label="Close planner" onClick={() => ui.set({ planner: null, tool: null, capture: false })}>
          <CloseGlyph />
        </button>
      </h2>
      <div className="planner-body">
        <label className="field">
          <span>Name</span>
          <input value={draft.name} onChange={(e) => set({ ...draft, name: e.target.value })} />
        </label>
        {draft.phases.map((p, i) => (
          <section key={i} className="plan-phase">
            <div className="plan-phase-head">
              <span className="phase-n">P{i + 1}</span>
              <input aria-label={`Phase ${i + 1} name`} value={p.name} onChange={(e) => setPhase(i, { ...p, name: e.target.value })} />
              <button
                type="button"
                className="cmd"
                disabled={draft.phases.length === 1}
                onClick={() => set({ ...draft, phases: draft.phases.filter((_, k) => k !== i) })}
              >
                REMOVE
              </button>
            </div>
            <CondEditor role="trigger" value={p.trigger} phases={draft.phases} self={p.id} onChange={(c) => c && setPhase(i, { ...p, trigger: c })} />
            <CondEditor role="end" value={p.end ?? { kind: 'tasks_done' }} phases={draft.phases} self={p.id} onChange={(c) => setPhase(i, { ...p, end: c ?? undefined })} />
            <CondEditor role="abort" value={p.abort ?? null} phases={draft.phases} self={p.id} onChange={(c) => setPhase(i, { ...p, abort: c ?? undefined })} />
            {p.tasks.map((t, j) => (
              <TaskEditor
                key={j}
                t={t}
                drawing={drawing?.[0] === i && drawing[1] === j}
                onChange={(x) => setTask(i, j, x)}
                onRemove={() => setPhase(i, { ...p, tasks: p.tasks.filter((_, k) => k !== j) })}
                onDraw={(k) => draw(i, j, k)}
                weapons={details?.kind === 'group' && t.units.includes(details.id) ? details.weapons.map((w) => [w.id, w.name] as [string, string]) : []}
              />
            ))}
            <button
              type="button"
              className="cmd"
              disabled={details?.kind !== 'group'}
              title={details?.kind === 'group' ? `Give ${details.name} a task in this phase` : 'Select a group on the map first'}
              onClick={() => {
                if (details?.kind !== 'group') return;
                unitNames.set(details.id, details.name);
                setPhase(i, { ...p, tasks: [...p.tasks, { units: [details.id], task: newTask('move', null) }] });
              }}
            >
              {details?.kind === 'group' ? `ADD TASK FOR ${details.name.toUpperCase()}` : 'SELECT A GROUP TO ADD A TASK'}
            </button>
          </section>
        ))}
        <button
          type="button"
          className="cmd"
          onClick={() => {
            const n = draft.phases.length + 1;
            let id = `p${n}`;
            while (draft.phases.some((p) => p.id === id)) id += 'x';
            const last = draft.phases[draft.phases.length - 1];
            set({
              ...draft,
              phases: [...draft.phases, { id, name: `Phase ${n}`, tasks: [], trigger: last ? { kind: 'phase_ended', phase: last.id } : { kind: 'start' } }],
            });
          }}
        >
          ADD PHASE
        </button>
        <ul className="issues" aria-label="Planner checks">
          {issues.length === 0 ? (
            <li className="muted">No findings.</li>
          ) : (
            issues.map((x, k) => (
              <li key={k} className={`issue is-${x.severity}`}>
                {x.phase != null ? `P${x.phase + 1}: ` : ''}
                {x.text}
              </li>
            ))
          )}
        </ul>
        <div className="planner-actions">
          <button type="button" className="cmd" disabled={errors.length > 0} onClick={() => void create(false)}>
            {planner.replace === null ? 'CREATE' : 'SAVE CHANGES'}
          </button>
          <button type="button" className="cmd" disabled={errors.length > 0} onClick={() => void create(true)}>
            CREATE AND START
          </button>
        </div>
      </div>
    </div>
  );
}

function TaskEditor({
  t,
  drawing,
  onChange,
  onRemove,
  onDraw,
  weapons,
}: {
  t: PhaseTaskSpec;
  drawing: boolean;
  onChange: (t: PhaseTaskSpec) => void;
  onRemove: () => void;
  onDraw: (kind: 'route' | 'circle' | 'strike') => void;
  weapons: [string, string][];
}) {
  const task = t.task;
  const names = t.units.map((u) => unitNames.get(u) ?? `unit ${u}`).join(', ');
  const strike = task.kind === 'strike' || task.kind === 'suppress';
  return (
    <div className="plan-task">
      <div className="plan-task-head">
        <span className="plan-units">{names}</span>
        <select
          aria-label="Task"
          value={task.kind}
          onChange={(e) => onChange({ ...t, task: newTask(e.target.value, task), fire: undefined, near_km: undefined })}
        >
          {TASK_KINDS.map(([k, label]) => (
            <option key={k} value={k}>
              {label}
            </option>
          ))}
        </select>
        <button type="button" className="cmd" onClick={onRemove}>
          REMOVE
        </button>
      </div>
      <div className="plan-task-params">
        {'waypoints' in task && (
          <>
            <span>{task.waypoints.length} waypoint{task.waypoints.length === 1 ? '' : 's'}</span>
            <button type="button" className="cmd" onClick={() => onDraw('route')}>
              {drawing ? 'DRAWING…' : 'DRAW ROUTE'}
            </button>
          </>
        )}
        {'centre' in task && 'radius_km' in task && (
          <>
            <span>
              {task.centre[1].toFixed(2)}°, {task.centre[0].toFixed(2)}° · {task.radius_km} km
            </span>
            <button type="button" className="cmd" onClick={() => onDraw('circle')}>
              {drawing ? 'DRAWING…' : 'DRAW ORBIT'}
            </button>
          </>
        )}
        {strike && (
          <>
            <span>{task.contact != null ? `Contact C${task.contact}` : task.point ? `Point ${task.point[1].toFixed(2)}°, ${task.point[0].toFixed(2)}°` : 'No target'}</span>
            <button type="button" className="cmd" onClick={() => onDraw('strike')}>
              {drawing ? 'PICKING…' : 'PICK TARGET'}
            </button>
            <select
              aria-label="Weapon"
              value={task.weapon ?? ''}
              onChange={(e) => onChange({ ...t, task: { ...task, weapon: e.target.value || undefined } })}
            >
              <option value="">Any suitable weapon</option>
              {weapons.map(([id, name]) => (
                <option key={id} value={id}>
                  {name}
                </option>
              ))}
              {task.weapon && !weapons.some(([id]) => id === task.weapon) && <option value={task.weapon}>{task.weapon}</option>}
            </select>
            <label className="inline">
              Rounds
              <input
                type="number"
                min={1}
                max={64}
                value={task.count ?? 1}
                onChange={(e) => onChange({ ...t, task: { ...task, count: Math.max(1, Number(e.target.value) || 1) } })}
              />
            </label>
            <label className="inline" title="Fire from where the units are and keep their task (ADR-043)">
              <input type="checkbox" checked={t.fire ?? false} onChange={(e) => onChange({ ...t, fire: e.target.checked || undefined })} />
              Fire mission
            </label>
            {task.point && (
              <label className="inline" title="At phase start, aim at our nearest contact within this radius of the point">
                Nearest contact within
                <input
                  type="number"
                  min={0}
                  max={500}
                  value={t.near_km ?? 0}
                  onChange={(e) => onChange({ ...t, near_km: Number(e.target.value) > 0 ? Number(e.target.value) : undefined })}
                />
                km
              </label>
            )}
          </>
        )}
      </div>
    </div>
  );
}

function CondEditor({
  role,
  value,
  phases,
  self,
  onChange,
}: {
  role: Role;
  value: CondSpec | null;
  phases: PhaseSpec[];
  self: string;
  onChange: (c: CondSpec | null) => void;
}) {
  const { op, leaves } = flatten(value, role);
  const label = role === 'trigger' ? 'Starts' : role === 'end' ? 'Ends' : 'Aborts';
  const update = (o: 'all' | 'any', l: CondSpec[]) => onChange(join(o, l));
  return (
    <div className="plan-cond">
      <span className="plan-cond-label">{label}</span>
      {leaves.length > 1 && (
        <select aria-label={`${label}: combine`} value={op} onChange={(e) => update(e.target.value as 'all' | 'any', leaves)}>
          <option value="all">when all of</option>
          <option value="any">when any of</option>
        </select>
      )}
      {leaves.map((leaf, k) => (
        <LeafEditor
          key={k}
          role={role}
          leaf={leaf}
          phases={phases}
          self={self}
          onChange={(x) => update(op, leaves.map((y, m) => (m === k ? x : y)))}
          onRemove={role === 'trigger' && leaves.length === 1 ? null : () => update(op, leaves.filter((_, m) => m !== k))}
        />
      ))}
      <button type="button" className="cmd small" onClick={() => update(op, [...leaves, defaultLeaf(LEAVES[role][0]?.[0] ?? 'manual', phases, self)])}>
        {leaves.length === 0 ? `ADD ${role.toUpperCase()}` : role === 'trigger' ? '+ AND' : '+ OR'}
      </button>
    </div>
  );
}

function LeafEditor({
  role,
  leaf,
  phases,
  self,
  onChange,
  onRemove,
}: {
  role: Role;
  leaf: CondSpec;
  phases: PhaseSpec[];
  self: string;
  onChange: (c: CondSpec) => void;
  onRemove: (() => void) | null;
}) {
  if (leaf.kind === 'all' || leaf.kind === 'any') return <span className="muted">nested conditions (edit in the scenario file)</span>;
  const num = (v: number | null | undefined, set: (n: number) => void, unit: string, min = 0) => (
    <label className="inline">
      <input type="number" min={min} value={v ?? 0} onChange={(e) => set(Math.max(min, Number(e.target.value) || 0))} />
      {unit}
    </label>
  );
  const phaseSel = (v: string, set: (p: string) => void) => (
    <select aria-label="Phase" value={v} onChange={(e) => set(e.target.value)}>
      {phases.map((p, i) => (
        <option key={p.id} value={p.id} disabled={p.id === self && role === 'trigger'}>
          P{i + 1} {p.name}
        </option>
      ))}
    </select>
  );
  return (
    <span className="plan-leaf">
      <select aria-label="Condition" value={leaf.kind} onChange={(e) => onChange(defaultLeaf(e.target.value as LeafKind, phases, self))}>
        {LEAVES[role].map(([k, l]) => (
          <option key={k} value={k}>
            {l}
          </option>
        ))}
      </select>
      {leaf.kind === 'start' && num(leaf.delay_min, (n) => onChange({ ...leaf, delay_min: n }), 'min')}
      {(leaf.kind === 'phase_started' || leaf.kind === 'phase_ended') && (
        <>
          {phaseSel(leaf.phase, (ph) => onChange({ ...leaf, phase: ph }))}
          {num(leaf.delay_min, (n) => onChange({ ...leaf, delay_min: n }), '+ min')}
        </>
      )}
      {leaf.kind === 'at_time' && <input aria-label="UTC time" value={leaf.utc} onChange={(e) => onChange({ ...leaf, utc: e.target.value })} />}
      {leaf.kind === 'elapsed' && num(leaf.min, (n) => onChange({ ...leaf, min: n }), 'min', 1)}
      {(leaf.kind === 'losses_pct' || leaf.kind === 'fuel_below') && num(leaf.pct, (n) => onChange({ ...leaf, pct: Math.min(100, n) }), '%')}
      {leaf.kind === 'contacts' && (
        <>
          {num(leaf.count, (n) => onChange({ ...leaf, count: Math.max(1, n) }), '×', 1)}
          <select aria-label="Domain" value={leaf.domain ?? ''} onChange={(e) => onChange({ ...leaf, domain: (e.target.value || undefined) as typeof leaf.domain })}>
            <option value="">any</option>
            <option value="air">air</option>
            <option value="surface">surface</option>
            <option value="ground">ground</option>
          </select>
          <select aria-label="Level" value={leaf.level ?? 'class'} onChange={(e) => onChange({ ...leaf, level: e.target.value as typeof leaf.level })}>
            <option value="domain">detected</option>
            <option value="class">classified</option>
            <option value="type">identified to type</option>
            <option value="precise">located precisely</option>
          </select>
        </>
      )}
      {onRemove && (
        <button type="button" className="close" aria-label="Remove condition" onClick={onRemove}>
          <CloseGlyph />
        </button>
      )}
    </span>
  );
}
