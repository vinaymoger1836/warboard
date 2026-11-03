// Approvals tab (doc 03 §7, doc 10 §8): the ROE approval queue. One approval allows one
// engagement; a request expires when the contact is expected to leave the envelope.
import type { ApprovalView } from '@warboard/protocol';
import { useEffect, useState } from 'react';
import type { EngineBridge } from '../bridge';
import { sendCommand } from '../orders';
import { ui } from '../store';

/** The queue refreshes this often (countdowns) and on every ROE event. */
const REFRESH_MS = 1_000;

/** The side's open requests, kept fresh. */
export function useApprovals(bridge: EngineBridge): ApprovalView[] {
  const [list, setList] = useState<ApprovalView[]>([]);
  useEffect(() => {
    let live = true;
    const load = () =>
      bridge
        .request({ type: 'approvals' })
        .then((l) => live && setList(l))
        .catch(() => live && setList([]));
    load();
    const t = setInterval(load, REFRESH_MS);
    const offEvents = bridge.onEvents((events) => {
      if (events.some((e) => 'RoeRequest' in e.kind || 'RoeDecision' in e.kind)) load();
    });
    const offWar = bridge.onWar(() => load());
    return () => {
      live = false;
      clearInterval(t);
      offEvents();
      offWar();
    };
  }, [bridge]);
  return list;
}

const countdown = (ms: number) => {
  const s = Math.max(0, Math.round(ms / 1_000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
};

export function Approvals({
  bridge,
  list,
  toast,
}: {
  bridge: EngineBridge;
  list: ApprovalView[];
  toast: (t: string) => void;
}) {
  if (list.length === 0) return <p className="muted">No engagements waiting for approval.</p>;
  const now = bridge.report?.simTimeMs ?? 0;
  const decide = (r: ApprovalView, approve: boolean) =>
    sendCommand(bridge, { type: approve ? 'ApproveEngagement' : 'DenyEngagement', request: r.request }, toast);
  return (
    <ol className="approvals">
      {list.map((r) => (
        <li key={r.request}>
          <button type="button" className="approval-what" onClick={() => ui.locateContact(r.contact)}>
            <span className="chip approval-chip">ROE</span>
            <span>
              {r.group_name} → C{r.contact}
              {r.contact_type ? ` ${r.contact_type}` : ''} · {r.weapon_type} · Pk {Math.round(r.pk * 100)}%
            </span>
            <time>{r.approved ? 'APPROVED' : `expires in ${countdown(r.expires_ms - now)}`}</time>
          </button>
          {!r.approved && (
            <span className="approval-actions">
              <button type="button" className="cmd" onClick={() => decide(r, true)}>
                APPROVE
              </button>
              <button type="button" className="cmd" onClick={() => decide(r, false)}>
                DENY
              </button>
            </span>
          )}
        </li>
      ))}
    </ol>
  );
}
