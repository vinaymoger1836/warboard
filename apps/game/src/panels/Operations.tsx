// Operations tab of the dock (doc 03 §8, doc 10 §8): the side's operations, each as a
// timeline of phases (bars in side.own, the running phase outlined amber, aborted ones in
// state.bad, trigger dependencies as arrows) with START / PAUSE / ABORT, a manual GO for
// phases waiting on the player, and the phases' routes on the map.
import type {
  OperationSpec,
  OperationView,
  PhaseView,
} from "@warboard/protocol";
import { useEffect, useMemo, useRef, useState } from "react";
import type { EngineBridge } from "../bridge";
import { sendCommand } from "../orders";
import { type PlanRoute, ui, useUi } from "../store";
import { elapsed } from "./format";

const POLL_MS = 1_000;
/** Width a waiting phase is drawn with on the timeline until it runs (ms of sim time). */
const NOMINAL_MS = 10 * 60_000;

/** Plans this client created, so a planned operation can be edited again. */
const drafts = new Map<number, OperationSpec>();
export const rememberDraft = (id: number, spec: OperationSpec) =>
  drafts.set(id, spec);

export function useOperations(bridge: EngineBridge): OperationView[] {
  const [ops, setOps] = useState<OperationView[]>([]);
  useEffect(() => {
    let live = true;
    const load = () =>
      bridge
        .request({ type: "operations" })
        .then((o) => live && setOps(o))
        .catch(() => undefined);
    load();
    const t = setInterval(load, POLL_MS);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, [bridge]);
  return ops;
}

/** The map routes of an operation's phases. */
export function routesOf(op: OperationView): PlanRoute[] {
  return op.phases.flatMap((p, i) =>
    p.state === "done" || p.state === "cancelled" || p.state === "aborted"
      ? []
      : p.tasks.map((t) => ({
          phase: i + 1,
          kind: t.kind,
          route: t.route,
          ...(t.radius_km != null ? { radiusKm: t.radius_km } : {}),
          active: p.state === "running",
        })),
  );
}

const STATE_LABEL: Record<string, string> = {
  planned: "PLANNED",
  running: "RUNNING",
  paused: "PAUSED",
  completed: "COMPLETED",
  aborted: "ABORTED",
  waiting: "WAITING",
  done: "DONE",
  cancelled: "CANCELLED",
};

export function Operations({
  bridge,
  toast,
  now,
}: {
  bridge: EngineBridge;
  toast: (t: string) => void;
  now: number;
}) {
  const ops = useOperations(bridge);
  const planner = useUi((s) => s.planner);
  const [sel, setSel] = useState<number | null>(null);
  const [confirmAbort, setConfirmAbort] = useState(false);
  const op = ops.find((o) => o.id === sel) ?? ops[ops.length - 1] ?? null;

  // Routes of the selected operation on the map, unless the planner draws its draft.
  useEffect(() => {
    if (planner) return;
    ui.set({ planRoutes: op ? routesOf(op) : [] });
  }, [op, planner]);
  useEffect(() => () => ui.set({ planRoutes: [] }), []);

  const cmd = (c: Parameters<typeof sendCommand>[1]) =>
    void sendCommand(bridge, c, toast);
  const newPlan = () =>
    ui.set({
      planner: {
        replace: null,
        draft: {
          name: `Operation ${ops.length + 1}`,
          phases: [
            {
              id: "p1",
              name: "Phase 1",
              tasks: [],
              trigger: { kind: "start" },
            },
          ],
        },
      },
    });

  return (
    <div className="ops">
      <div className="ops-list">
        {ops.map((o) => (
          <button
            key={o.id}
            type="button"
            className={o.id === op?.id ? "ops-item active" : "ops-item"}
            onClick={() => {
              setSel(o.id);
              setConfirmAbort(false);
            }}
          >
            <span className="ops-name">{o.name}</span>
            <span className={`ops-state st-${o.state}`}>
              {STATE_LABEL[o.state]}
            </span>
          </button>
        ))}
        <button
          type="button"
          className="cmd"
          onClick={newPlan}
          disabled={planner !== null}
        >
          NEW OPERATION
        </button>
      </div>
      {op ? (
        <div className="ops-main">
          <div className="ops-controls">
            <strong>{op.name}</strong>
            <span className="muted">
              {op.started ? `started ${elapsed(op.started.ms)}` : "not started"}
              {op.reason ? ` · ${op.reason}` : ""}
            </span>
            <span className="ops-buttons">
              {op.state === "planned" && (
                <>
                  <button
                    type="button"
                    className="cmd"
                    onClick={() =>
                      cmd({ type: "StartOperation", operation: op.id })
                    }
                  >
                    START
                  </button>
                  {drafts.has(op.id) && (
                    <button
                      type="button"
                      className="cmd"
                      onClick={() =>
                        ui.set({
                          planner: {
                            replace: op.id,
                            draft: structuredClone(
                              drafts.get(op.id) as OperationSpec,
                            ),
                          },
                        })
                      }
                    >
                      EDIT
                    </button>
                  )}
                </>
              )}
              {op.state === "running" && (
                <button
                  type="button"
                  className="cmd"
                  onClick={() =>
                    cmd({ type: "PauseOperation", operation: op.id })
                  }
                >
                  PAUSE
                </button>
              )}
              {op.state === "paused" && (
                <button
                  type="button"
                  className="cmd"
                  onClick={() =>
                    cmd({ type: "ResumeOperation", operation: op.id })
                  }
                >
                  RESUME
                </button>
              )}
              {(op.state === "planned" ||
                op.state === "running" ||
                op.state === "paused") &&
                (confirmAbort ? (
                  <>
                    <span className="warn-text">
                      Abort {op.name}? Aircraft return to base.
                    </span>
                    <button
                      type="button"
                      className="cmd danger"
                      onClick={() => {
                        setConfirmAbort(false);
                        cmd({ type: "AbortOperation", operation: op.id });
                      }}
                    >
                      CONFIRM ABORT
                    </button>
                    <button
                      type="button"
                      className="cmd"
                      onClick={() => setConfirmAbort(false)}
                    >
                      KEEP
                    </button>
                  </>
                ) : (
                  <button
                    type="button"
                    className="cmd"
                    onClick={() => setConfirmAbort(true)}
                  >
                    ABORT
                  </button>
                ))}
            </span>
          </div>
          <Timeline op={op} now={now} />
          <ol className="phase-rows">
            {op.phases.map((p, i) => (
              <PhaseRow key={p.id} p={p} n={i + 1} op={op} onCmd={cmd} />
            ))}
          </ol>
        </div>
      ) : (
        <p className="muted">No operations yet. Plan one with NEW OPERATION.</p>
      )}
    </div>
  );
}

function PhaseRow({
  p,
  n,
  op,
  onCmd,
}: {
  p: PhaseView;
  n: number;
  op: OperationView;
  onCmd: (c: Parameters<typeof sendCommand>[1]) => void;
}) {
  const t = p.tally;
  return (
    <li className={`phase-row ps-${p.state}`}>
      <span className="phase-n">P{n}</span>
      <span className="phase-name">{p.name}</span>
      <span className={`ops-state st-${p.state}`}>{STATE_LABEL[p.state]}</span>
      <span className="phase-text">
        <span className="muted">Starts</span> {p.trigger} ·{" "}
        <span className="muted">ends</span> {p.end}
        {p.abort ? (
          <>
            {" "}
            · <span className="muted">aborts</span> {p.abort}
          </>
        ) : null}
        {p.tasks.length > 0 && (
          <>
            {" "}
            ·{" "}
            {p.tasks
              .map(
                (x) =>
                  `${x.kind.replace("_", " ")} (${x.unit_names.join(", ")})`,
              )
              .join("; ")}
          </>
        )}
        {t.launched > 0 &&
          ` · ${t.launched} fired, ${t.hits} hit, ${t.intercepted} intercepted, ${t.decoyed} decoyed`}
        {p.in_flight > 0 && ` · ${p.in_flight} in flight`}
        {t.lost > 0 && ` · ${t.lost} lost`}
        {p.reason ? <span className="muted"> · {p.reason}</span> : null}
      </span>
      {p.state === "waiting" && p.manual_go && op.state !== "planned" && (
        <button
          type="button"
          className="cmd"
          onClick={() =>
            onCmd({ type: "PhaseGo", operation: op.id, phase: n - 1 })
          }
        >
          GO
        </button>
      )}
      {p.state === "running" && (
        <button
          type="button"
          className="cmd"
          onClick={() =>
            onCmd({ type: "EndPhase", operation: op.id, phase: n - 1 })
          }
        >
          END
        </button>
      )}
    </li>
  );
}

const ROW = 20;
const LABEL_W = 150;

/** Phases as bars on a time axis; dependencies as arrows from the phase waited for. */
function Timeline({ op, now }: { op: OperationView; now: number }) {
  // Drawn at its real pixel width, so text keeps its size whatever the dock's width.
  const box = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(900);
  useEffect(() => {
    const el = box.current;
    if (!el) return;
    const ro = new ResizeObserver((e) =>
      setWidth(Math.max(400, Math.round(e[0]?.contentRect.width ?? 900))),
    );
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  const layout = useMemo(() => {
    const t0 = op.started?.ms ?? now;
    const spans: { start: number; end: number; est: boolean }[] = [];
    // Phases in order; a waiting phase sits after what it waits for (or now).
    for (let pass = 0; pass < 2; pass++) {
      op.phases.forEach((p, i) => {
        if (p.started) {
          spans[i] = {
            start: p.started.ms,
            end: p.ended?.ms ?? Math.max(now, p.started.ms),
            est: false,
          };
          return;
        }
        let start = Math.max(t0, op.state === "planned" ? t0 : now);
        for (const [j, what] of p.deps) {
          const d = spans[j];
          if (d) start = Math.max(start, what === "started" ? d.start : d.end);
        }
        spans[i] = { start, end: start + NOMINAL_MS, est: true };
      });
    }
    const t1 = Math.max(t0 + NOMINAL_MS, now, ...spans.map((s) => s.end));
    return { t0, t1, spans };
  }, [op, now]);
  const height = op.phases.length * ROW + 18;
  const x = (ms: number) =>
    LABEL_W +
    ((ms - layout.t0) / Math.max(1, layout.t1 - layout.t0)) *
      (width - LABEL_W - 10);
  const nowX = x(Math.min(Math.max(now, layout.t0), layout.t1));
  return (
    <div ref={box} className="timeline-box">
      <svg
        className="timeline"
        width={width}
        height={height}
        viewBox={`0 0 ${width} ${height}`}
        role="img"
        aria-label={`Timeline of ${op.name}`}
      >
        <defs>
          <marker
            id="tl-arrow"
            viewBox="0 0 8 8"
            refX="7"
            refY="4"
            markerWidth="6"
            markerHeight="6"
            orient="auto"
          >
            <path d="M0,0 L8,4 L0,8 z" className="tl-arrowhead" />
          </marker>
        </defs>
        <line
          x1={LABEL_W}
          x2={width - 10}
          y1={height - 14}
          y2={height - 14}
          className="tl-axis"
        />
        <text x={LABEL_W} y={height - 2} className="tl-tick">
          {op.started ? `T${elapsed(layout.t0).slice(1)}` : "H+0"}
        </text>
        <text
          x={width - 10}
          y={height - 2}
          className="tl-tick"
          textAnchor="end"
        >
          {op.started
            ? `T${elapsed(layout.t1).slice(1)}`
            : `H+${Math.round((layout.t1 - layout.t0) / 60_000)} min`}
        </text>
        {op.state !== "planned" && (
          <line
            x1={nowX}
            x2={nowX}
            y1={0}
            y2={height - 14}
            className="tl-now"
          />
        )}
        {op.phases.map((p, i) => {
          const s = layout.spans[i];
          if (!s) return null;
          const y = i * ROW + 3;
          return (
            <g key={p.id}>
              <text x={4} y={y + 12} className="tl-label">
                {`P${i + 1} ${p.name}`.slice(0, 22)}
              </text>
              <rect
                x={x(s.start)}
                y={y}
                width={Math.max(4, x(s.end) - x(s.start))}
                height={ROW - 7}
                rx={3}
                className={`tl-bar tb-${p.state}${s.est ? " tl-est" : ""}`}
              />
            </g>
          );
        })}
        {op.phases.flatMap((p, i) =>
          p.deps.map(([j, what], k) => {
            const a = layout.spans[j];
            const b = layout.spans[i];
            if (!a || !b) return null;
            const x1 = x(what === "started" ? a.start : a.end);
            const y1 = j * ROW + 3 + (ROW - 7) / 2;
            const x2 = x(b.start);
            const y2 = i * ROW + 3 + (ROW - 7) / 2;
            const mid = Math.max(x1, x2) + 8;
            return (
              <path
                key={`${p.id}-${j}-${k}`}
                d={
                  x2 > x1 + 2
                    ? `M${x1},${y1} C${(x1 + x2) / 2},${y1} ${(x1 + x2) / 2},${y2} ${x2 - 1},${y2}`
                    : `M${x1},${y1} L${mid},${y1} L${mid},${y2} L${x2 - 1},${y2}`
                }
                className="tl-dep"
                markerEnd="url(#tl-arrow)"
              />
            );
          }),
        )}
      </svg>
    </div>
  );
}
