// Unit card (doc 10 §7, reference 05): header with the type icon, status and fuel, speed,
// altitude and heading, the three range rows (each toggles its ring), collapsible
// sections, and command buttons by unit type. Base, satellite and contact cards in the
// same frame; the contact card shows estimates only, titled "(our estimate)" when estimated.
import {
  type BaseDetails,
  type ContactDetails,
  type Emcon,
  type GroupDetails,
  type JammerMode,
  type Roe,
  type RoeChoice,
  type SatelliteDetails,
  SYMBOLS,
  type SymbolName,
} from '@warboard/protocol';
import { contactLabel, cssVar, ESTIMATE_LINE, iconDataUrl, type IconSide, type TokenName } from '@warboard/symbols';
import { type ReactNode, useEffect, useState } from 'react';
import type { EngineBridge } from '../bridge';
import { patrolRadiusKm, sendCommand, sendCommandAck, sendTask } from '../orders';
import { distance } from '../scene/geo';
import { play } from '../play';
import { drawnInView, identifiedContact } from '../scene/scene';
import { type CamTarget, type LonLat, type StrikeAim, ui, useUi } from '../store';
import { duration, fuelColour, healthColour, statusColour, weaponFacts, weaponTags } from './format';
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
  'ground/radar': 'radar',
  'ground/launcher': 'launcher',
  'ground/c2': 'c2',
  'ground/armour': 'armour',
  'ground/artillery': 'artillery',
  'ground/infantry': 'infantry',
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

/**
 * The card's frame. On a touch screen (`html[data-input='touch']`, set in main.tsx) the card is
 * a bottom sheet with the same layout (doc 10 §7): it opens with its header and status rows and
 * a handle expands it to the full card. On a mouse screen the handle is hidden and the card is
 * the floating panel of the reference.
 */
function CardFrame({ label, children }: { label: string; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  return (
    <aside className="unit-card panel" aria-label={label} data-sheet={open ? 'open' : 'peek'}>
      <button
        type="button"
        className="sheet-handle"
        aria-label={open ? 'Show less of the card' : 'Show the full card'}
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
      >
        <span aria-hidden="true" />
      </button>
      {children}
    </aside>
  );
}

export function UnitCard({ bridge, toast }: CardProps) {
  const details = useUi((s) => s.details);
  const contact = useUi((s) => s.contact);
  if (contact) {
    return (
      <CardFrame key={`c${contact.contact_id}`} label="Contact card">
        <ContactCard
          c={contact}
          symbol={contactSymbol(bridge, contact.contact_id)}
          inspectable={!!bridge.frame && identifiedContact(bridge.frame.view, bridge.strings, contact.contact_id)}
        />
      </CardFrame>
    );
  }
  if (!details) return null;
  return (
    <CardFrame key={`u${details.id}`} label="Unit card">
      {details.kind === 'group' ? (
        <GroupCard d={details} bridge={bridge} toast={toast} />
      ) : details.kind === 'satellite' ? (
        <SatelliteCard d={details} inspectable={!!bridge.frame && drawnInView(bridge.frame.view, details.id)} />
      ) : (
        <BaseCard d={details} />
      )}
    </CardFrame>
  );
}

function Header({
  symbol,
  title,
  subtitle,
  side = 'own',
  estimate = false,
  onClose = () => ui.select(null),
  inspect = null,
}: {
  symbol: SymbolName;
  title: string;
  subtitle: string;
  side?: IconSide;
  estimate?: boolean;
  onClose?: () => void;
  /** Opens the 3D inspect view on this object (ADR-047); null when it cannot be inspected. */
  inspect?: CamTarget | null;
}) {
  return (
    <header className="card-header">
      <img className="card-icon" src={iconDataUrl(SYMBOLS[symbol], side, { estimate })} alt="" width={30} height={30} />
      <div className="card-titles">
        <h2>{title}</h2>
        <p>{subtitle}</p>
      </div>
      {inspect && (
        <button
          type="button"
          className="cmd small inspect-3d"
          onClick={() => {
            ui.set({ cam3d: { mode: 'inspect', target: inspect } });
            play.switchView('3d');
          }}
        >
          INSPECT 3D
        </button>
      )}
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

const IDENTITY_TEXT: Record<string, string> = { hostile: 'Hostile', neutral: 'Neutral', friendly: 'Friendly', unknown: 'Unknown' };
const IDENTITY_TOKEN: Record<string, TokenName> = { hostile: 'side.enemy', neutral: 'side.neutral', friendly: 'side.own', unknown: 'side.unknown' };
const QUALITY_TEXT: Record<string, string> = { fire_control: 'Fire-control track', track: 'Track', report: 'Report only' };

function ContactCard({ c, symbol, inspectable }: { c: ContactDetails; symbol: number; inspectable: boolean }) {
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
  const idColour = cssVar(IDENTITY_TOKEN[c.identity] ?? 'side.unknown');
  return (
    <>
      <Header
        symbol={SYMBOL_BY_CODE.get(symbol) ?? 'unknown'}
        title={title}
        subtitle={`Contact C${c.contact_id} · ${c.domain}`}
        side={IDENTITY_SIDE[c.identity] ?? 'unknown'}
        estimate={c.estimated}
        onClose={() => ui.selectContact(null)}
        inspect={inspectable ? { kind: 'contact', id: c.contact_id } : null}
      />
      <div className="card-body">
        <div className="pill-row">
          <span className="pill solid" style={{ borderColor: idColour, color: idColour }}>
            {IDENTITY_TEXT[c.identity] ?? c.identity}
          </span>
          <span className="pill dim">Classified: {LEVEL_TEXT[c.level] ?? c.level}</span>
          <span className="pill dim">{QUALITY_TEXT[c.quality] ?? c.quality}</span>
          <span className={c.lost ? 'pill warn' : 'pill dim'}>
            {c.lost ? `Lost ${duration(c.age_s)} ago` : `Updated ${duration(c.age_s)} ago`}
          </span>
        </div>
        <div className="card-row">
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
          {c.heading_deg != null && !c.fixed && (
            <span>
              <span className="label">Heading: </span>
              <b>{Math.round(c.heading_deg)}°</b>
            </span>
          )}
          <span>
            <span className="label">Position: </span>
            <b>{c.bearing_only ? 'bearing only' : pm}</b>
          </span>
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
        <div className="block">
          <span className="block-title">Our hits on it</span>
          {c.our_shots === 0 ? (
            <p className="tab-line muted-inline">No weapons fired at it yet.</p>
          ) : (
            <>
              <div className="hits">
                <b style={{ color: cssVar(c.our_hits > 0 ? 'state.ok' : 'text.secondary') }}>
                  {c.our_hits} hit{c.our_hits === 1 ? '' : 's'}
                </b>
                <span className="muted-inline">
                  {' '}
                  of {c.our_shots} weapon{c.our_shots === 1 ? '' : 's'} fired
                  {c.last_hit_s_ago != null ? ` · last hit ${duration(c.last_hit_s_ago)} ago` : ''}
                </span>
              </div>
              <p className="hint">Its true condition is not known: only our own weapon results are reported.</p>
            </>
          )}
        </div>
        <div className="block">
          <span className="block-title">Identification</span>
          <dl className="facts">
            <dt>Class</dt>
            <dd>{c.class_name ?? '—'}</dd>
            <dt>Type</dt>
            <dd>{c.type_name ?? '—'}</dd>
            <dt>Count</dt>
            <dd>{c.count_known ? c.count_max : `${c.count_min}–${c.count_max} (estimate)`}</dd>
            <dt>First seen</dt>
            <dd>{duration(c.first_seen_s_ago)} ago</dd>
          </dl>
        </div>
        <div className="block">
          <span className="block-title">Sources</span>
          <ul className="source-list">
            {c.sources.map((s) => (
              <li key={`${s.kind}-${s.unit}-${s.sensor ?? ''}`}>
                <span className="pill dim">{s.kind}</span>
                <span className="src-unit">
                  {s.unit}
                  {s.sensor ? ` · ${s.sensor}` : ''}
                </span>
                <span className="muted-inline">
                  {s.network ? `via ${s.network}` : 'C2 report'} · {duration(s.age_s)} ago
                </span>
              </li>
            ))}
          </ul>
        </div>
      </div>
    </>
  );
}

const PAYLOAD_SYMBOL: Record<string, SymbolName> = {
  optical: 'satellite_eo',
  sar: 'satellite_sar',
  sigint: 'satellite_sigint',
};

function SatelliteCard({ d, inspectable }: { d: SatelliteDetails; inspectable: boolean }) {
  return (
    <>
      <Header
        symbol={PAYLOAD_SYMBOL[d.payload] ?? 'satellite_eo'}
        title={`${d.owner} ${d.name}`}
        subtitle={d.payload_label}
        inspect={inspectable ? { kind: 'unit', id: d.id } : null}
      />
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
          <dl className="facts">
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
        <dl className="facts">
          <dt>Class</dt>
          <dd>{d.class_name || '—'}</dd>
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

const JAMMER_MODES: [JammerMode, string][] = [
  ['self_protect', 'SELF-PROTECT'],
  ['escort', 'ESCORT'],
  ['stand_off', 'STAND-OFF'],
];

function JammerControl({ d, bridge, toast }: { d: GroupDetails } & CardProps) {
  if (!d.jammer) return null;
  const j = d.jammer;
  const availableModes = JAMMER_MODES.filter(([m]) => j.available_modes.includes(m));

  return (
    <div className="jammer-control">
      <div className="card-row" style={{ marginBottom: 6 }}>
        <span>
          <span className="label">State: </span>
          <b style={{ color: j.active ? cssVar('ring.sensor') : undefined }}>
            {j.active ? 'ACTIVE' : 'STANDBY'}
          </b>
        </span>
        <span>
          <span className="label">Strength: </span>
          <b>{j.jam_strength}</b>
        </span>
      </div>
      <div className="card-row" style={{ marginBottom: 8 }}>
        <span>
          <span className="label">Coverage: </span>
          <b>{j.coverage_km > 0 ? `${Math.round(j.coverage_km)} km` : 'Self only'}</b>
        </span>
        <span>
          <span className="label">Sector: </span>
          <b>{j.sector_deg < 360 ? `${Math.round(j.sector_deg)}°` : 'Omni (360°)'}</b>
        </span>
      </div>
      {availableModes.length > 1 && (
        <div className="segmented" role="group" aria-label="Jammer mode">
          {availableModes.map(([mode, label]) => (
            <button
              key={mode}
              type="button"
              className={j.mode === mode ? 'seg active' : 'seg'}
              aria-pressed={j.mode === mode}
              onClick={() =>
                void sendCommand(
                  bridge,
                  { type: 'SetJammer', units: [d.id], on: true, mode },
                  toast,
                )
              }
            >
              {label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

const ROE_NAMES: Record<Roe, string> = {
  alpha: 'A — weapons free',
  bravo: 'B — weapons tight',
  charlie: 'C — weapons hold',
};

/**
 * ROE section (doc 03 §7): the effective ROE and where it comes from, the group's own
 * ROE (or inherit), and self-defence against incoming weapons.
 */
function RoeControl({ d, bridge, toast }: { d: GroupDetails } & CardProps) {
  const set = (cmd: { roe?: RoeChoice; self_defence?: boolean }) =>
    void sendCommand(bridge, { type: 'SetRoe', units: [d.id], ...cmd }, toast);
  const own: RoeChoice = d.roe_override ?? 'inherit';
  const choices: [RoeChoice, string][] = [
    ['alpha', 'A'],
    ['bravo', 'B'],
    ['charlie', 'C'],
    ['inherit', 'INHERIT'],
  ];
  return (
    <div className="roe-control">
      <dl className="facts">
        <dt>Effective</dt>
        <dd>
          {ROE_NAMES[d.roe]} <span className="muted-inline">(from the {d.roe_source})</span>
        </dd>
        <dt>Self-defence</dt>
        <dd>
          {d.self_defence ? 'on' : 'off'}{' '}
          <span className="muted-inline">({d.self_defence_override == null ? 'inherited' : 'set on this group'})</span>
        </dd>
      </dl>
      <div className="strike-label">Group ROE</div>
      <div className="segmented" role="group" aria-label="Group rules of engagement">
        {choices.map(([c, label]) => (
          <button key={c} type="button" className={own === c ? 'seg active' : 'seg'} aria-pressed={own === c} onClick={() => set({ roe: c })}>
            {label}
          </button>
        ))}
      </div>
      <div className="strike-label">Self-defence</div>
      <div className="segmented" role="group" aria-label="Self-defence">
        {([true, false] as const).map((v) => (
          <button
            key={String(v)}
            type="button"
            className={d.self_defence === v ? 'seg active' : 'seg'}
            aria-pressed={d.self_defence === v}
            onClick={() => set({ self_defence: v })}
          >
            {v ? 'ON' : 'OFF'}
          </button>
        ))}
      </div>
    </div>
  );
}

const STRIKE_MAX_ROUNDS = 64; // the engine accepts 1–64 rounds per strike order

/**
 * Strike panel (doc 03 §6, ADR-029): pick a target on the map (one of our contacts, or a
 * point), a weapon that can engage it and how many rounds, then LAUNCH STRIKE. The engine
 * validates the order and its reason is shown if it refuses (CLAUDE.md rule 5).
 */
function StrikePanel({ d, aim, bridge, toast }: { d: GroupDetails; aim: StrikeAim | null } & CardProps) {
  const contactId = aim && 'contact' in aim ? aim.contact : null;
  const [loaded, setTarget] = useState<ContactDetails | null>(null);
  // Only the details of the contact aimed at now (a previous aim's answer may still be held).
  const target = loaded && loaded.contact_id === contactId ? loaded : null;
  const [weaponId, setWeaponId] = useState<string | null>(null);
  const [count, setCount] = useState(2);
  const [busy, setBusy] = useState(false);

  // The aimed contact's estimate (label, domain, position), refreshed while aiming.
  useEffect(() => {
    if (contactId === null) return;
    let live = true;
    const load = () =>
      bridge
        .request({ type: 'contactDetails', id: contactId })
        .then((c) => live && setTarget(c))
        .catch(() => live && setTarget(null));
    load();
    const t = setInterval(load, 1000);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, [bridge, contactId]);

  // Weapons with rounds left that can engage what is aimed at (a point: surface or ground).
  const domains: string[] = aim === null ? [] : 'point' in aim ? ['surface', 'ground'] : target ? [target.domain] : [];
  const usable = d.weapons.filter((w) => w.count > 0 && (domains.length === 0 || w.targets.some((t) => domains.includes(t))));
  const weapon = usable.find((w) => w.id === weaponId) ?? usable[0] ?? null;
  const maxRounds = weapon ? Math.min(STRIKE_MAX_ROUNDS, weapon.count * Math.max(1, d.count)) : 1;
  const rounds = Math.min(Math.max(1, count), maxRounds);

  const at: LonLat | null =
    aim === null ? null : 'point' in aim ? aim.point : target?.lon_deg != null && target.lat_deg != null ? [target.lon_deg, target.lat_deg] : null;
  const rangeM = at ? distance([d.lon_deg, d.lat_deg], at) : null;
  const rangeNote =
    weapon && rangeM != null
      ? rangeM > weapon.max_range_m
        ? `Beyond ${weapon.name} max range (${km(weapon.max_range_m)})`
        : rangeM < weapon.min_range_m
          ? `Inside ${weapon.name} minimum range (${km(weapon.min_range_m)})`
          : null
      : null;

  const targetText =
    aim === null
      ? null
      : 'point' in aim
        ? `Point ${aim.point[1].toFixed(2)}°N ${aim.point[0].toFixed(2)}°E`
        : target
          ? `${target.label || target.class_name || 'Contact'} (C${target.contact_id}, ${target.domain})${target.estimated ? ` ${ESTIMATE_LINE}` : ''}`
          : `Contact C${aim.contact}`;

  const launch = async () => {
    if (!aim || !weapon) return;
    setBusy(true);
    const note = await sendCommandAck(
      bridge,
      {
        type: 'AssignTask',
        units: [d.id],
        task: { kind: 'strike', ...('contact' in aim ? { contact: aim.contact } : { point: aim.point }), weapon: weapon.id, count: rounds },
      },
      toast,
    );
    setBusy(false);
    if (note !== null) {
      // The engine says why the strike cannot fire at once (out of range, no own track).
      toast(`Strike ordered: ${rounds} × ${weapon.name}${note ? `. ${note}` : ''}`);
      ui.set({ tool: null });
    }
  };

  return (
    <div className="strike-panel">
      <div className="strike-title">Strike</div>
      <dl className="facts">
        <dt>Target</dt>
        <dd>{targetText ?? <span className="muted-inline">Click an enemy contact on the map, or open sea for a point. Esc cancels.</span>}</dd>
        {rangeM != null && (
          <>
            <dt>Range</dt>
            <dd style={{ color: rangeNote ? cssVar('state.warn') : undefined }}>
              {km(rangeM)}
              {rangeNote ? ` · ${rangeNote}` : ''}
            </dd>
          </>
        )}
      </dl>
      {weapon && aim && 'contact' in aim && !weapon.engage_on_remote && (
        <p className="muted">
          {weapon.name} needs a track from this unit&apos;s own sensors: it closes in and fires once it holds one, and the strike is
          cancelled if its launchers are knocked out.
        </p>
      )}
      {usable.length === 0 ? (
        <p className="muted">
          {d.weapons.some((w) => w.count > 0) ? `No weapon left that can engage ${domains.join(' or ')} targets.` : 'No weapons left.'}
        </p>
      ) : (
        <>
          <div className="strike-label">Weapon</div>
          <div className="segmented strike-weapons" role="group" aria-label="Weapon">
            {usable.map((w) => (
              <button
                key={w.id}
                type="button"
                className={weapon?.id === w.id ? 'seg active' : 'seg'}
                aria-pressed={weapon?.id === w.id}
                onClick={() => setWeaponId(w.id)}
                title={`${w.category.toUpperCase()} · ${km(w.max_range_m)} · ${w.targets.join(', ')}`}
              >
                {w.name} ({w.count})
              </button>
            ))}
          </div>
          <div className="strike-label">Rounds</div>
          <div className="strike-count">
            <button type="button" className="seg" aria-label="Fewer rounds" onClick={() => setCount(Math.max(1, rounds - 1))} disabled={rounds <= 1}>
              −
            </button>
            <input
              type="number"
              min={1}
              max={maxRounds}
              value={rounds}
              aria-label="Rounds"
              onChange={(e) => setCount(Number(e.target.value) || 1)}
            />
            <button type="button" className="seg" aria-label="More rounds" onClick={() => setCount(Math.min(maxRounds, rounds + 1))} disabled={rounds >= maxRounds}>
              +
            </button>
            <span className="muted-inline">of {maxRounds}</span>
          </div>
        </>
      )}
      <div className="card-buttons">
        <button type="button" className="cmd" disabled={!aim || !weapon || busy} onClick={() => void launch()}>
          LAUNCH STRIKE
        </button>
        <button type="button" className="cmd" onClick={() => ui.set({ tool: null })}>
          CANCEL
        </button>
      </div>
    </div>
  );
}

function GroupCard({ d, bridge, toast }: { d: GroupDetails } & CardProps) {
  const rings = useUi((s) => s.rings);
  const weaponRing = useUi((s) => s.weaponRing);
  const picked = d.weapons.find((w) => w.id === weaponRing) ?? null;
  const formation = d.formation ?? null;
  const [tab, setTab] = useState(formation ? 'force' : 'weapons');
  const air = d.domain === 'air';
  const title = d.count > 1 ? `${d.count} × ${d.type_name}` : d.type_name;
  const personnel = formation ? formation.personnel : d.crew;
  const subtitle = [d.name, personnel ? `${personnel} Personnel` : null].filter(Boolean).join(' · ');
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
  // HOLD FIRE puts the group on Charlie (weapons hold, doc 03 §7); RESUME FIRE drops the
  // group's own ROE so it inherits the side's again.
  const holding = d.roe_override === 'charlie';
  const holdFire = () =>
    void sendCommand(bridge, { type: 'SetRoe', units: [d.id], roe: holding ? 'inherit' : 'charlie' }, (t) => toast(t)).then(
      (ok) => ok && toast(holding ? `${d.name}: ROE back to the side's` : `${d.name}: holding fire (ROE C)`),
    );
  const tool = useUi((s) => s.tool);
  const striking = tool?.kind === 'strike' ? tool : null;
  const strikeTool = () => {
    ui.set({ tool: { kind: 'strike', aim: null } });
    toast('Click an enemy contact on the map, or open sea for a point');
  };
  // Ground formations (doc 03 §5): tasks without a target are buttons; move, attack, recon,
  // raid and designate take their target from a right-click on the map.
  const groundTask = (task: Parameters<typeof sendTask>[2], done: string) =>
    void sendTask(bridge, units, task, toast).then((ok) => ok && toast(`${d.name}: ${done}`));
  const buttons: [string, () => void][] = formation
    ? [
        ['HOLD', () => groundTask({ kind: 'hold' }, 'holding its ground')],
        ['DEFEND', () => groundTask({ kind: 'defend' }, 'digging in')],
        ['WITHDRAW', () => groundTask({ kind: 'withdraw' }, 'falling back')],
        ...(d.weapons.length > 0 ? ([['STRIKE', strikeTool]] as [string, () => void][]) : []),
      ]
    : air
    ? [
        ['ORDER RTB', rtb],
        ['RETASK PATROL', () => routeTool(true)],
        ...(d.jammer
          ? ([
              [
                d.jammer.active ? 'STANDBY JAMMER' : 'ACTIVATE JAMMER',
                () =>
                  void sendCommand(
                    bridge,
                    { type: 'SetJammer', units: [d.id], on: !d.jammer?.active },
                    toast,
                  ),
              ],
            ] as [string, () => void][])
          : []),
        ['STRIKE', strikeTool],
        [holding ? 'RESUME FIRE' : 'HOLD FIRE', holdFire],
      ]
    : [
        ['MOVE', () => routeTool(false)],
        ['PATROL', circleTool],
        ...(d.jammer
          ? ([
              [
                d.jammer.active ? 'STANDBY JAMMER' : 'ACTIVATE JAMMER',
                () =>
                  void sendCommand(
                    bridge,
                    { type: 'SetJammer', units: [d.id], on: !d.jammer?.active },
                    toast,
                  ),
              ],
            ] as [string, () => void][])
          : []),
        ['STRIKE', strikeTool],
        [
          'EMCON',
          () => {
            const next: Emcon = d.emcon === 'on' ? 'off' : d.emcon === 'off' ? 'blink' : 'on';
            if (d.emcon == null) toast('No emitters to control');
            else void sendCommand(bridge, { type: 'SetEmcon', units: [d.id], emcon: next }, toast);
          },
        ],
      ];
  const tabs: [string, string][] = [
    ...(formation ? ([['force', 'Force']] as [string, string][]) : []),
    ['weapons', 'Weapons'],
    ['task', 'Task'],
    ['sensors', 'Sensors'],
    ...(d.jammer ? ([['ew', 'EW']] as [string, string][]) : []),
    ['roe', 'ROE'],
    ['specs', 'Specs'],
  ];
  const current = tabs.some(([k]) => k === tab) ? tab : formation ? 'force' : 'weapons';
  const health = groupHealth(d);
  return (
    <>
      <Header
        symbol={CATEGORY_SYMBOL[d.category] ?? 'unknown'}
        title={title}
        subtitle={subtitle}
        inspect={bridge.frame && drawnInView(bridge.frame.view, d.id) ? { kind: 'unit', id: d.id } : null}
      />
      <div className="card-body">
        {d.strike_note && <p className="card-note">{d.strike_note}</p>}
        <div className="card-row">
          <span>
            <span className="label">Status: </span>
            <b style={{ color: statusColour(d.status) }}>{d.status || '—'}</b>
          </span>
          {d.fuel_pct != null ? (
            <span>
              <span className="label">Fuel: </span>
              <b style={{ color: fuelColour(d.fuel_pct, d.bingo) }}>
                {Math.round(d.fuel_pct)}%{d.bingo ? ' BINGO' : ''}
              </b>
            </span>
          ) : formation ? (
            <span>
              <span className="label">Strength: </span>
              <b style={{ color: healthColour(formation.strength_pct) }}>{Math.round(formation.strength_pct)}%</b>
            </span>
          ) : (
            health && (
              <span>
                <span className="label">Hull: </span>
                <b style={{ color: healthColour(health.pct) }}>{Math.round(health.pct)}%</b>
              </span>
            )
          )}
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
        {formation ? <FormationState f={formation} /> : <Condition d={d} />}
        <div className="ranges">
          <button type="button" className={`range-row sensor${rings.sensor ? '' : ' off'}`} aria-pressed={rings.sensor} onClick={() => toggle('sensor')}>
            <SensorGlyph />
            <span>Radar / Sensor Horizon:</span>
            <b>
              {km(d.ranges.sensor_m)}
              {d.ranges.surface_horizon_m != null && <span className="range-sub"> · {km(d.ranges.surface_horizon_m)} vs ships</span>}
            </b>
          </button>
          <button type="button" className={`range-row engage${rings.engage ? '' : ' off'}`} aria-pressed={rings.engage} onClick={() => toggle('engage')}>
            <EngageGlyph />
            <span>{picked ? `${picked.name} Range:` : 'Max Engagement Range:'}</span>
            <b>{km(picked ? picked.max_range_m : d.ranges.engage_m)}</b>
          </button>
          <button type="button" className={`range-row radius${rings.radius ? '' : ' off'}`} aria-pressed={rings.radius} onClick={() => toggle('radius')}>
            <RadiusGlyph />
            <span>Combat Radius Reach:</span>
            <b>{km(d.ranges.combat_radius_m)}</b>
          </button>
        </div>
        <div className="card-tabs" role="tablist" aria-label="Unit details">
          {tabs.map(([key, label]) => (
            <button key={key} type="button" role="tab" aria-selected={current === key} className={current === key ? 'tab active' : 'tab'} onClick={() => setTab(key)}>
              {label}
            </button>
          ))}
        </div>
        <div className="tab-body" role="tabpanel">
          {current === 'force' && formation && (
            <dl className="facts">
              {formation.equipment.map((e) => (
                <span key={e.id} className="dl-pair">
                  <dt>{e.name}</dt>
                  <dd style={{ color: healthColour((100 * e.count) / Math.max(1, e.full)) }}>
                    {e.count} / {e.full}
                  </dd>
                </span>
              ))}
              <dt>Personnel</dt>
              <dd>
                {formation.personnel} / {formation.personnel_full}
              </dd>
              <dt>Fatigue</dt>
              <dd>{Math.round(formation.fatigue_pct)}%</dd>
            </dl>
          )}
          {current === 'weapons' && <WeaponsTab d={d} />}
          {current === 'task' && (
            <dl className="facts">
              <dt>Task</dt>
              <dd>{d.task.kind.replace(/_/g, ' ')}</dd>
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
              {air && (
                <>
                  <dt>Bingo return</dt>
                  <dd>{d.bingo_rtb ? 'on' : 'off'}</dd>
                </>
              )}
            </dl>
          )}
          {current === 'sensors' && (
            <>
              <EmconControl d={d} bridge={bridge} toast={toast} />
              {d.sensors.length === 0 ? (
                <p className="muted">No sensors besides the crew.</p>
              ) : (
                <ul className="tag-list">
                  {d.sensors.map((s) => (
                    <li key={s}>{s}</li>
                  ))}
                </ul>
              )}
            </>
          )}
          {current === 'ew' && <JammerControl d={d} bridge={bridge} toast={toast} />}
          {current === 'roe' && <RoeControl d={d} bridge={bridge} toast={toast} />}
          {current === 'specs' && (
            <dl className="facts">
              <dt>Type</dt>
              <dd>{d.type_name}</dd>
              <dt>Class</dt>
              <dd>{d.class_name || '—'}</dd>
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
            </dl>
          )}
        </div>
        {striking ? (
          <StrikePanel d={d} aim={striking.aim} bridge={bridge} toast={toast} />
        ) : (
          <>
            {formation && <p className="card-note">Right-click the map or a contact for move, attack and recon.</p>}
            <div className="card-buttons">
              {buttons.map(([label, run]) => (
                <button key={label} type="button" className="cmd" onClick={run}>
                  {label}
                </button>
              ))}
            </div>
          </>
        )}
      </div>
    </>
  );
}

const POSTURE_TEXT: Record<string, string> = {
  moving: 'On the move',
  attack: 'Attacking',
  hasty: 'Hasty defence',
  prepared: 'Dug in',
};

/** State of a ground formation (doc 02 §14): posture and contact pills, then strength,
 * morale and supply bars in the style of the Condition block. */
function FormationState({ f }: { f: NonNullable<GroupDetails['formation']> }) {
  const bars: [string, number][] = [
    ['Strength', f.strength_pct],
    ['Morale', f.morale_pct],
    ['Supply', f.supply_pct],
  ];
  return (
    <div className="condition">
      <span className="block-title">Condition</span>
      <div className="cond-member">
        <span className="cond-flags">
          <span className="pill dim">{POSTURE_TEXT[f.posture] ?? f.posture}</span>
          {f.in_contact && <span className="pill bad">In contact</span>}
          {f.suppressed && <span className="pill warn">Suppressed</span>}
          {f.designating != null && <span className="pill ok">Designating C{f.designating}</span>}
        </span>
      </div>
      {bars.map(([label, pct]) => (
        <div key={label} className="cond-member">
          <span className="cond-name">{label}</span>
          <span className="hp" title={`${label} ${Math.round(pct)}%`}>
            <span className="hp-bar">
              <span style={{ width: `${pct}%`, background: healthColour(pct) }} />
            </span>
            <b style={{ color: healthColour(pct) }}>{Math.round(pct)}%</b>
          </span>
        </div>
      ))}
    </div>
  );
}

/** Lowest hull of a ship or ground group; none for aircraft. */
function groupHealth(d: GroupDetails): { pct: number } | null {
  const pcts = d.members.map((m) => m.health_pct).filter((p): p is number => p != null);
  return pcts.length === 0 ? null : { pct: Math.min(...pcts) };
}

const SHIP_SUBSYSTEMS = ['radar', 'VLS', 'launcher', 'propulsion'];

/** Condition of each member after hits (doc 02 §13): hull bar and knocked-out subsystems for
 * ships and ground units; intact or damaged for aircraft. */
function Condition({ d }: { d: GroupDetails }) {
  const ship = d.category.startsWith('ship');
  const hurt = d.members.some((m) => m.damaged);
  if (!hurt) {
    return (
      <div className="condition compact">
        <span className="block-title">Condition</span>
        <span className="pill ok">{d.count > 1 ? 'All intact' : 'Intact'}</span>
      </div>
    );
  }
  return (
    <div className="condition">
      <span className="block-title">Condition</span>
      {d.members.map((m) => (
        <div key={m.id} className="cond-member">
          {d.count > 1 && <span className="cond-name">{m.name}</span>}
          {m.health_pct != null ? (
            <span className="hp" title={`${Math.round(m.health_pct)}% of damage points left`}>
              <span className="hp-bar">
                <span style={{ width: `${m.health_pct}%`, background: healthColour(m.health_pct) }} />
              </span>
              <b style={{ color: healthColour(m.health_pct) }}>{Math.round(m.health_pct)}%</b>
            </span>
          ) : (
            <span className={m.damaged ? 'pill bad' : 'pill ok'}>{m.damaged ? 'Damaged · mission aborted' : 'Intact'}</span>
          )}
          <span className="cond-flags">
            {m.sinking_in_s != null && <span className="pill bad">Sinking in {duration(m.sinking_in_s)}</span>}
            {m.out_of_action && m.sinking_in_s == null && <span className="pill bad">Out of action</span>}
            {ship &&
              SHIP_SUBSYSTEMS.map((s) => {
                const out = m.out.includes(s);
                return (
                  <span key={s} className={out ? 'pill bad' : 'pill dim'} title={out ? `${s} knocked out` : `${s} working`}>
                    {s}
                    {out ? ' out' : ''}
                  </span>
                );
              })}
          </span>
        </div>
      ))}
    </div>
  );
}

/** Weapons tab: one row per weapon with rounds and key numbers; picking one shows its full
 * specs and its range ring on the map (doc 10 §7). */
function WeaponsTab({ d }: { d: GroupDetails }) {
  const picked = useUi((s) => s.weaponRing);
  const air = d.domain === 'air';
  if (d.weapons.length === 0 && d.countermeasures.length === 0) return <p className="muted">No weapons loaded.</p>;
  return (
    <>
      {d.loadout && (
        <p className="tab-line">
          <span className="label">Loadout </span>
          {d.loadout}
        </p>
      )}
      <ul className="weapon-list">
        {d.weapons.map((w) => {
          const full = d.stations.filter((s) => s.weapon === w.name).reduce((sum, s) => sum + s.full, 0);
          const on = picked === w.id;
          const left = full > 0 ? w.count / full : 1;
          return (
            <li key={w.id}>
              <button
                type="button"
                className={on ? 'weapon-row on' : 'weapon-row'}
                aria-pressed={on}
                title={on ? 'Hide this weapon’s range ring' : 'Show this weapon’s range on the map'}
                onClick={() => ui.set({ weaponRing: on ? null : w.id })}
              >
                <span className="w-head">
                  <span className="w-name">{w.name}</span>
                  <span className="w-count">
                    <b>{w.count}</b>
                    {full > 0 ? ` / ${full}` : ''} {air ? 'per aircraft' : d.count > 1 ? 'per ship' : ''}
                  </span>
                </span>
                <span className="ammo-bar">
                  <span style={{ width: `${Math.round(left * 100)}%` }} />
                </span>
                <span className="w-tags">
                  {weaponTags(w).map((t) => (
                    <span key={t} className="pill dim">
                      {t}
                    </span>
                  ))}
                  {!w.engage_on_remote && <span className="pill warn">own track</span>}
                </span>
                {on && (
                  <dl className="facts w-facts">
                    {weaponFacts(w).map((f) => (
                      <span key={f.label} className="dl-pair">
                        <dt>{f.label}</dt>
                        <dd>{f.value}</dd>
                      </span>
                    ))}
                  </dl>
                )}
              </button>
            </li>
          );
        })}
      </ul>
      {d.weapons.length > 0 && !picked && <p className="hint">Select a weapon to see its range on the map.</p>}
      {d.fire_channels && (
        <p className="tab-line">
          <span className="label">Fire channels </span>
          <b style={{ color: d.fire_channels.used >= d.fire_channels.total ? cssVar('state.warn') : undefined }}>
            {d.fire_channels.used} of {d.fire_channels.total} in use
          </b>
        </p>
      )}
      {d.countermeasures.length > 0 && (
        <p className="tab-line">
          <span className="label">Countermeasures </span>
          {d.countermeasures.map((m) => `${m.name} ${m.count}`).join(' · ')}
        </p>
      )}
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
