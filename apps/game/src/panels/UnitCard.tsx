// Unit card (doc 10 §7, reference 05): header with the type icon, status and fuel, speed,
// altitude and heading, the three range rows (each toggles its ring), collapsible
// sections, and command buttons by unit type. Base, satellite and contact cards in the
// same frame; the contact card shows estimates only, titled "(our estimate)" when estimated.
import {
  type BaseDetails,
  type ContactDetails,
  type Emcon,
  type GroupDetails,
  type SatelliteDetails,
  SYMBOLS,
  type SymbolName,
} from '@warboard/protocol';
import { contactLabel, ESTIMATE_LINE, iconDataUrl, type IconSide } from '@warboard/symbols';
import { useState } from 'react';
import type { EngineBridge } from '../bridge';
import { patrolRadiusKm, sendCommand, sendTask } from '../orders';
import { ui, useUi } from '../store';
import { duration, fuelColour, statusColour } from './format';
import { CloseGlyph, EngageGlyph, RadiusGlyph, SensorGlyph } from './glyphs';

const CATEGORY_SYMBOL: Record<string, SymbolName> = {
  'aircraft/fighter': 'fighter',
  'aircraft/bomber': 'bomber',
  'aircraft/attack': 'attack',
  'aircraft/tanker': 'tanker',
  'aircraft/aew': 'aew',
  'aircraft/ew': 'ew_aircraft',
  'aircraft/transport': 'transport',
  'aircraft/maritime_patrol': 'maritime_patrol',
  'aircraft/uav': 'drone',
  'helicopter/attack': 'helicopter_attack',
  'helicopter/utility': 'helicopter_utility',
  'ship/destroyer': 'destroyer',
  'ship/frigate': 'frigate',
  'ship/corvette': 'corvette',
  'ship/patrol': 'patrol_ship',
  'ship/auxiliary': 'auxiliary',
  'submarine/attack': 'submarine',
};

const BASE_SYMBOL: Record<string, SymbolName> = {
  'Air base': 'air_base',
  'Naval base': 'naval_base',
  'Road strip': 'road_strip',
  Heliport: 'heliport',
  'Expeditionary anchorage': 'anchorage',
  'Army base': 'army_base',
  'SAM site': 'sam_site',
  'Missile launch site': 'missile_site',
  HQ: 'hq',
};

const km = (m: number | null | undefined) => (m == null ? '—' : `${Math.round(m / 1000)} km`);

interface CardProps {
  bridge: EngineBridge;
  toast: (t: string) => void;
}

export function UnitCard({ bridge, toast }: CardProps) {
  const details = useUi((s) => s.details);
  const contact = useUi((s) => s.contact);
  if (contact) {
    return (
      <aside className="unit-card panel" aria-label="Contact card">
        <ContactCard c={contact} symbol={contactSymbol(bridge, contact.contact_id)} />
      </aside>
    );
  }
  if (!details) return null;
  return (
    <aside className="unit-card panel" aria-label="Unit card">
      {details.kind === 'group' ? (
        <GroupCard d={details} bridge={bridge} toast={toast} />
      ) : details.kind === 'satellite' ? (
        <SatelliteCard d={details} />
      ) : (
        <BaseCard d={details} />
      )}
    </aside>
  );
}

function Header({
  symbol,
  title,
  subtitle,
  side = 'own',
  estimate = false,
  onClose = () => ui.select(null),
}: {
  symbol: SymbolName;
  title: string;
  subtitle: string;
  side?: IconSide;
  estimate?: boolean;
  onClose?: () => void;
}) {
  return (
    <header className="card-header">
      <img className="card-icon" src={iconDataUrl(SYMBOLS[symbol], side, { estimate })} alt="" width={30} height={30} />
      <div className="card-titles">
        <h2>{title}</h2>
        <p>{subtitle}</p>
      </div>
      <button type="button" className="close" aria-label="Close card" onClick={onClose}>
        <CloseGlyph />
      </button>
    </header>
  );
}

const SYMBOL_BY_CODE = new Map<number, SymbolName>(
  (Object.entries(SYMBOLS) as [SymbolName, number][]).map(([name, code]) => [code, name]),
);
const IDENTITY_SIDE: Record<string, IconSide> = { hostile: 'enemy', neutral: 'neutral', friendly: 'own', unknown: 'unknown' };
const LEVEL_TEXT: Record<string, string> = {
  unknown: 'Unknown',
  domain: 'Domain',
  class: 'Class',
  type: 'Type',
  precise: 'Precise',
};

/** Contact card (doc 10 §7): what our side believes, never the truth. */
function contactSymbol(bridge: EngineBridge, id: number): number {
  const v = bridge.frame?.view;
  const i = v?.contactIndex.get(id);
  return v && i !== undefined ? (v.contacts.symbol[i] ?? 0) : 0;
}

function ContactCard({ c, symbol }: { c: ContactDetails; symbol: number }) {
  const threatRings = useUi((s) => s.threatRings);
  const label = contactLabel({
    symbol,
    typeName: c.label,
    domain: c.domain,
    countMin: c.count_min,
    countMax: c.count_max,
    countKnown: c.count_known,
  });
  const title = c.estimated ? `${label} ${ESTIMATE_LINE}` : label;
  const pm = c.uncertainty_a_m == null ? '—' : `± ${(c.uncertainty_a_m / 1000).toFixed(1)} km`;
  return (
    <>
      <Header
        symbol={SYMBOL_BY_CODE.get(symbol) ?? 'unknown'}
        title={title}
        subtitle={`Contact C${c.contact_id} · ${c.identity} · ${c.domain}`}
        side={IDENTITY_SIDE[c.identity] ?? 'unknown'}
        estimate={c.estimated}
        onClose={() => ui.selectContact(null)}
      />
      <div className="card-body">
        <div className="card-row">
          <span>
            <span className="label">Classification: </span>
            <b>{LEVEL_TEXT[c.level] ?? c.level}</b>
          </span>
          <span>
            <span className="label">Last update: </span>
            <b>{c.lost ? `lost ${duration(c.age_s)} ago` : `${duration(c.age_s)} ago`}</b>
          </span>
        </div>
        <div className="card-row">
          <span>
            <span className="label">Uncertainty: </span>
            <b>{c.bearing_only ? 'bearing only' : pm}</b>
          </span>
          {c.speed_mps != null && !c.fixed && (
            <span>
              <span className="label">Speed: </span>
              <b>{Math.round(c.speed_mps * 3.6)} km/h</b>
            </span>
          )}
          {c.domain === 'air' && c.alt_m != null && (
            <span>
              <span className="label">Altitude: </span>
              <b>{(c.alt_m / 1000).toFixed(1)} km</b>
            </span>
          )}
        </div>
        {c.threat_m != null && (
          <div className="ranges">
            <button
              type="button"
              className={`range-row threat${threatRings ? '' : ' off'}`}
              aria-pressed={threatRings}
              onClick={() => ui.set({ threatRings: !threatRings })}
            >
              <EngageGlyph />
              <span>Estimated threat range:</span>
              <b>{km(c.threat_m)}</b>
            </button>
          </div>
        )}
        <dl>
          <dt>Class</dt>
          <dd>{c.class_name ?? '—'}</dd>
          <dt>Type</dt>
          <dd>{c.type_name ?? '—'}</dd>
          <dt>Count</dt>
          <dd>{c.count_known ? c.count_max : `${c.count_min}–${c.count_max} (estimate)`}</dd>
          <dt>Quality</dt>
          <dd>{c.quality.replace('_', ' ')}</dd>
          <dt>First seen</dt>
          <dd>{duration(c.first_seen_s_ago)} ago</dd>
        </dl>
        <h3 className="card-subhead">Sources</h3>
        <ul className="sources">
          {c.sources.map((s) => (
            <li key={`${s.kind}-${s.unit}-${s.sensor ?? ''}`}>
              <b>{s.kind}</b> · {s.unit}
              {s.sensor ? ` (${s.sensor})` : ''}
              <span className="muted-inline">
                {s.network ? ` via ${s.network}` : ' via C2 report'} · {duration(s.age_s)} ago
              </span>
            </li>
          ))}
        </ul>
      </div>
    </>
  );
}

const PAYLOAD_SYMBOL: Record<string, SymbolName> = {
  optical: 'satellite_eo',
  sar: 'satellite_sar',
  sigint: 'satellite_sigint',
};

function SatelliteCard({ d }: { d: SatelliteDetails }) {
  return (
    <>
      <Header symbol={PAYLOAD_SYMBOL[d.payload] ?? 'satellite_eo'} title={`${d.owner} ${d.name}`} subtitle={d.payload_label} />
      <div className="card-body">
        <div className="card-row">
          <span>
            <span className="label">Altitude: </span>
            <b>{Math.round(d.alt_m / 1000)} km</b>
          </span>
          <span>
            <span className="label">Strip: </span>
            <b>{Math.round(d.swath_km)} km</b>
          </span>
        </div>
        {d.payload === 'sigint' ? (
          <p className="muted">SIGINT reports emitters on every pass; it takes no imaging tasks.</p>
        ) : d.taskings.length === 0 ? (
          <p className="muted">No imaging tasks. Right-click the map to task this satellite.</p>
        ) : (
          <dl>
            {d.taskings.map((t, i) => (
              <span key={i} className="dl-pair">
                <dt>
                  Task {i + 1} ({t.centre[1].toFixed(2)}°, {t.centre[0].toFixed(2)}°)
                </dt>
                <dd>pass in {duration(t.pass_in_s)}</dd>
              </span>
            ))}
          </dl>
        )}
        <dl>
          <dt>Data confidence</dt>
          <dd>{d.confidence}</dd>
        </dl>
      </div>
    </>
  );
}

const EMCON_STATES: [Emcon, string][] = [
  ['on', 'ACTIVE'],
  ['blink', 'BLINK'],
  ['off', 'SILENT'],
];

function EmconControl({ d, bridge, toast }: { d: GroupDetails } & CardProps) {
  if (d.emcon == null) return <p className="muted">No emitters to control.</p>;
  return (
    <div className="segmented" role="group" aria-label="Emission control">
      {EMCON_STATES.map(([state, label]) => (
        <button
          key={state}
          type="button"
          className={d.emcon === state ? 'seg active' : 'seg'}
          aria-pressed={d.emcon === state}
          onClick={() => void sendCommand(bridge, { type: 'SetEmcon', units: [d.id], emcon: state }, toast)}
        >
          {label}
        </button>
      ))}
    </div>
  );
}

function GroupCard({ d, bridge, toast }: { d: GroupDetails } & CardProps) {
  const rings = useUi((s) => s.rings);
  const [open, setOpen] = useState<string | null>(null);
  const air = d.domain === 'air';
  const title = d.count > 1 ? `${d.count} × ${d.type_name}` : d.type_name;
  const subtitle = [d.name, d.crew ? `${d.crew} Personnel` : null].filter(Boolean).join(' · ');
  const toggle = (k: keyof typeof rings) => ui.set({ rings: { ...rings, [k]: !rings[k] } });
  const units = [d.id];
  const rtb = () => void sendTask(bridge, units, { kind: 'rtb' }, toast);
  const routeTool = (patrol: boolean) => {
    ui.set({ tool: { kind: 'route', patrol, points: [] } });
    toast(patrol ? 'Click waypoints for the patrol loop; double-click or Enter to finish' : 'Click waypoints; double-click or Enter to finish');
  };
  const circleTool = () => {
    ui.set({ tool: { kind: 'circle', centre: null, radiusKm: patrolRadiusKm(d.domain) } });
    toast('Click the patrol centre, then click again at the radius');
  };
  const later = (what: string, phase: string) => () => toast(`${what} arrives in ${phase}`);
  const buttons: [string, () => void][] = air
    ? [
        ['ORDER RTB', rtb],
        ['RETASK PATROL', () => routeTool(true)],
        ['STRIKE', later('Strike', 'P5')],
        ['HOLD FIRE', later('Rules of engagement', 'P5')],
      ]
    : [
        ['MOVE', () => routeTool(false)],
        ['PATROL', circleTool],
        ['STRIKE', later('Strike', 'P5')],
        [
          'EMCON',
          () => {
            const next: Emcon = d.emcon === 'on' ? 'off' : d.emcon === 'off' ? 'blink' : 'on';
            if (d.emcon == null) toast('No emitters to control');
            else void sendCommand(bridge, { type: 'SetEmcon', units: [d.id], emcon: next }, toast);
          },
        ],
      ];
  const section = (key: string, label: string, body: React.ReactNode) => (
    <div className="card-section">
      <button type="button" className="section-toggle" aria-expanded={open === key} onClick={() => setOpen(open === key ? null : key)}>
        {open === key ? '▾' : '▸'} {label}
      </button>
      {open === key && <div className="section-body">{body}</div>}
    </div>
  );
  return (
    <>
      <Header symbol={CATEGORY_SYMBOL[d.category] ?? 'unknown'} title={title} subtitle={subtitle} />
      <div className="card-body">
        <div className="card-row">
          <span>
            <span className="label">Status: </span>
            <b style={{ color: statusColour(d.status) }}>{d.status || '—'}</b>
          </span>
          <span>
            <span className="label">Fuel: </span>
            <b style={{ color: fuelColour(d.fuel_pct, d.bingo) }}>
              {d.fuel_pct == null ? '—' : `${Math.round(d.fuel_pct)}%`}
              {d.bingo ? ' BINGO' : ''}
            </b>
          </span>
        </div>
        <div className="card-row">
          <span>
            <span className="label">Speed: </span>
            <b>{Math.round(d.speed_mps * 3.6)} km/h</b>
          </span>
          {air && (
            <span>
              <span className="label">Altitude: </span>
              <b>{(d.alt_m / 1000).toFixed(1)} km</b>
            </span>
          )}
          <span>
            <span className="label">Heading: </span>
            <b>{Math.round(d.heading_deg)}°</b>
          </span>
        </div>
        <div className="ranges">
          <button type="button" className={`range-row sensor${rings.sensor ? '' : ' off'}`} aria-pressed={rings.sensor} onClick={() => toggle('sensor')}>
            <SensorGlyph />
            <span>Radar / Sensor Horizon:</span>
            <b>{km(d.ranges.sensor_m)}</b>
          </button>
          <button type="button" className={`range-row engage${rings.engage ? '' : ' off'}`} aria-pressed={rings.engage} onClick={() => toggle('engage')}>
            <EngageGlyph />
            <span>Max Engagement Range:</span>
            <b>{km(d.ranges.engage_m)}</b>
          </button>
          <button type="button" className={`range-row radius${rings.radius ? '' : ' off'}`} aria-pressed={rings.radius} onClick={() => toggle('radius')}>
            <RadiusGlyph />
            <span>Combat Radius Reach:</span>
            <b>{km(d.ranges.combat_radius_m)}</b>
          </button>
        </div>
        {section(
          'task',
          'Task',
          <dl>
            <dt>Task</dt>
            <dd>{d.task.kind.replace('_', ' ')}</dd>
            {d.task.waypoints.length > 0 && (
              <>
                <dt>Waypoints</dt>
                <dd>
                  {d.task.waypoints.length}
                  {d.task.looped ? ', loop' : ''}
                </dd>
              </>
            )}
            {d.task.radius_m != null && (
              <>
                <dt>Radius</dt>
                <dd>{km(d.task.radius_m)}</dd>
              </>
            )}
            {d.task.base && (
              <>
                <dt>Base</dt>
                <dd>{d.task.base.name}</dd>
              </>
            )}
            <dt>Home base</dt>
            <dd>{d.home_base?.name ?? '—'}</dd>
            <dt>Bingo return</dt>
            <dd>{d.bingo_rtb ? 'on' : 'off'}</dd>
          </dl>,
        )}
        {section(
          'weapons',
          'Weapons',
          d.weapons.length === 0 ? (
            <p className="muted">No weapons loaded.</p>
          ) : (
            <dl>
              <dt>Loadout</dt>
              <dd>{d.loadout ?? '—'}</dd>
              {d.weapons.map((w) => (
                <span key={w.name} className="dl-pair">
                  <dt>{w.name}</dt>
                  <dd>{w.count} per aircraft</dd>
                </span>
              ))}
            </dl>
          ),
        )}
        {section(
          'sensors',
          'Sensors & EMCON',
          <>
            <EmconControl d={d} bridge={bridge} toast={toast} />
            {d.sensors.length === 0 ? (
              <p className="muted">No sensors besides the crew.</p>
            ) : (
              <ul className="sources">
                {d.sensors.map((s) => (
                  <li key={s}>{s}</li>
                ))}
              </ul>
            )}
          </>,
        )}
        {section('roe', 'ROE', <p className="muted">Rules of engagement arrive in P5.</p>)}
        {section(
          'specs',
          'Specs',
          <dl>
            <dt>Type</dt>
            <dd>{d.type_name}</dd>
            <dt>Data confidence</dt>
            <dd>{d.confidence}</dd>
            {d.mach != null && (
              <>
                <dt>Mach</dt>
                <dd>{d.mach.toFixed(2)}</dd>
              </>
            )}
            {d.members.map((m) => (
              <span key={m.id} className="dl-pair">
                <dt>{m.name}</dt>
                <dd style={{ color: fuelColour(m.fuel_pct) }}>
                  {m.status} {m.fuel_pct == null ? '' : `· ${Math.round(m.fuel_pct)}%`}
                </dd>
              </span>
            ))}
          </dl>,
        )}
        <div className="card-buttons">
          {buttons.map(([label, run]) => (
            <button key={label} type="button" className="cmd" onClick={run}>
              {label}
            </button>
          ))}
        </div>
      </div>
    </>
  );
}

function BaseCard({ d }: { d: BaseDetails }) {
  const subtitle = [d.kind_name, d.runway_m ? `Runway ${Math.round(d.runway_m)} m` : null].filter(Boolean).join(' · ');
  return (
    <>
      <Header symbol={BASE_SYMBOL[d.kind_name] ?? 'air_base'} title={d.name} subtitle={subtitle} />
      <div className="card-body">
        <div className="card-row">
          <span>
            <span className="label">Status: </span>
            <b>{d.active ? 'ACTIVE' : `ACTIVATING (${duration(d.active_in_s)})`}</b>
          </span>
          <span>
            <span className="label">Capacity: </span>
            <b>
              {d.aircraft_capacity > 0 ? `${d.aircraft_capacity} aircraft` : ''}
              {d.aircraft_capacity > 0 && d.ship_berths > 0 ? ' · ' : ''}
              {d.ship_berths > 0 ? `${d.ship_berths} berths` : ''}
            </b>
          </span>
        </div>
        {d.stationed.length === 0 ? (
          <p className="muted">No units stationed here.</p>
        ) : (
          <ul className="stationed">
            {d.stationed.map((s) => (
              <li key={s.id}>
                <button type="button" onClick={() => ui.select(s.group || s.id)}>
                  <span>{s.name}</span>
                  <span className="muted-inline">{s.type_name}</span>
                  <span style={{ color: s.ready ? statusColour('LANDED') : statusColour('RTB') }}>
                    {s.ready ? 'READY' : `TURNAROUND ${duration(s.turnaround_left_s)}`}
                  </span>
                  <span style={{ color: fuelColour(s.fuel_pct) }}>{s.fuel_pct == null ? '' : `${Math.round(s.fuel_pct)}%`}</span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </>
  );
}
