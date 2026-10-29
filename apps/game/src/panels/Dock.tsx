// Bottom dock (doc 10 §8): Alerts · Approvals · Intel log · Event log · Operations. Event
// lines start with mono sim time and a coloured category chip; click locates the unit, or
// the contact for intel lines (doc 03 §10).
import type { StringTable } from '@warboard/protocol';
import { useState } from 'react';
import { type EventLine, ui, useUi } from '../store';
import { CATEGORY_COLOUR, contactOf, describe, elapsed, subjectOf } from './format';

const TABS = [
  ['alerts', 'Alerts'],
  ['approvals', 'Approvals'],
  ['intel', 'Intel log'],
  ['events', 'Event log'],
  ['operations', 'Operations'],
] as const;
type Tab = (typeof TABS)[number][0];

const LATER: Partial<Record<Tab, string>> = {
  approvals: 'The approval queue arrives with rules of engagement in P5.',
  operations: 'Operations arrive in P7.',
};

export function Dock({ strings }: { strings: StringTable }) {
  const [tab, setTab] = useState<Tab>('events');
  const [open, setOpen] = useState(true);
  const events = useUi((s) => s.events);
  const alerts = useUi((s) => s.alerts);
  const intel = useUi((s) => s.intel);
  const lines = tab === 'alerts' ? alerts : tab === 'events' ? events : tab === 'intel' ? intel : [];
  return (
    <section className={open ? 'dock' : 'dock closed'} aria-label="Dock">
      <div className="dock-tabs" role="tablist">
        {TABS.map(([id, label]) => (
          <button
            key={id}
            type="button"
            role="tab"
            aria-selected={tab === id}
            className={tab === id ? 'dock-tab active' : 'dock-tab'}
            onClick={() => {
              setTab(id);
              setOpen(true);
            }}
          >
            {label}
            {id === 'alerts' && alerts.length > 0 ? ` (${alerts.length})` : ''}
          </button>
        ))}
        <button type="button" className="dock-tab dock-toggle" aria-expanded={open} onClick={() => setOpen((o) => !o)}>
          {open ? 'HIDE' : 'SHOW'}
        </button>
      </div>
      {open && (
        <div className="dock-body" role="tabpanel">
          {LATER[tab] ? (
            <p className="muted">{LATER[tab]}</p>
          ) : lines.length === 0 ? (
            <p className="muted">
              {tab === 'alerts' ? 'No alerts.' : tab === 'intel' ? 'No contacts or reports yet.' : 'Nothing has happened yet.'}
            </p>
          ) : (
            <ol className="event-lines">
              {[...lines].reverse().map((l) => (
                <Line key={l.key} line={l} strings={strings} />
              ))}
            </ol>
          )}
        </div>
      )}
    </section>
  );
}

function Line({ line, strings }: { line: EventLine; strings: StringTable }) {
  const { category, text } = describe(line.event, strings);
  const subject = subjectOf(line.event);
  const contact = contactOf(line.event);
  const go = () => {
    if (contact !== null) ui.locateContact(contact);
    else if (subject !== null) ui.locate(subject);
  };
  return (
    <li>
      <button type="button" onClick={go} disabled={subject === null && contact === null}>
        <time>{elapsed(line.event.sim_time_ms)}</time>
        <span className="chip" style={{ color: CATEGORY_COLOUR[category], borderColor: CATEGORY_COLOUR[category] }}>
          {category}
        </span>
        <span>{text}</span>
      </button>
    </li>
  );
}
