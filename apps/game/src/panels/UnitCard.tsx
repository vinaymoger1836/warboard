// Unit card (doc 10 §7, reference 05): header with the type icon, status and fuel, speed,
// altitude and heading, the three range rows (each toggles its ring), collapsible
// sections, and command buttons by unit type. Base card in the same frame.
import { type BaseDetails, type GroupDetails, SYMBOLS, type SymbolName } from '@warboard/protocol';
import { iconDataUrl } from '@warboard/symbols';
import { useState } from 'react';
import type { EngineBridge } from '../bridge';
import { patrolRadiusKm, sendTask } from '../orders';
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
  if (!details) return null;
  return (
    <aside className="unit-card panel" aria-label="Unit card">
      {details.kind === 'group' ? (
        <GroupCard d={details} bridge={bridge} toast={toast} />
      ) : (
        <BaseCard d={details} />
      )}
    </aside>
  );
}

function Header({ symbol, title, subtitle }: { symbol: SymbolName; title: string; subtitle: string }) {
  return (
    <header className="card-header">
      <img className="card-icon" src={iconDataUrl(SYMBOLS[symbol], 'own')} alt="" width={30} height={30} />
      <div className="card-titles">
        <h2>{title}</h2>
        <p>{subtitle}</p>
      </div>
      <button type="button" className="close" aria-label="Close unit card" onClick={() => ui.select(null)}>
        <CloseGlyph />
      </button>
    </header>
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
        ['EMCON', later('Emission control', 'P4')],
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
        {section('sensors', 'Sensors & EMCON', <p className="muted">Sensors and emission control arrive in P4.</p>)}
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
