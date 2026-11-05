// Text helpers shared by the panels: clock strings, event lines, state colours (doc 10 §7–8).
import type { Event, StringTable } from '@warboard/protocol';
import { cssVar } from '@warboard/symbols';

const pad = (n: number) => String(n).padStart(2, '0');

/** `03:40:12Z` from Unix epoch ms. */
export function utcClock(ms: number): string {
  const d = new Date(ms);
  return `${pad(d.getUTCHours())}:${pad(d.getUTCMinutes())}:${pad(d.getUTCSeconds())}Z`;
}

/** `T+0:00:12` from elapsed ms. */
export function elapsed(ms: number): string {
  const s = Math.floor(ms / 1000);
  return `T+${Math.floor(s / 3600)}:${pad(Math.floor(s / 60) % 60)}:${pad(s % 60)}`;
}

/** `12:30` (minutes:seconds) or `1:02:30` for a duration in seconds. */
export function duration(s: number): string {
  const t = Math.max(0, Math.round(s));
  const h = Math.floor(t / 3600);
  const m = Math.floor(t / 60) % 60;
  return h > 0 ? `${h}:${pad(m)}:${pad(t % 60)}` : `${m}:${pad(t % 60)}`;
}

/** Status colour (doc 10 §7). */
export function statusColour(status: string): string {
  switch (status) {
    case 'ON STATION':
      return cssVar('state.station');
    case 'RTB':
    case 'REFUEL':
      return cssVar('state.warn');
    case 'LANDED':
    case 'TURNAROUND':
    case 'IDLE':
      return cssVar('text.secondary');
    default:
      return cssVar('text.primary');
  }
}

/** Fuel colour: ≥ 50 % ok, 25–50 % warn, < 25 % or bingo bad (doc 10 §7). */
export function fuelColour(pct: number | null | undefined, bingo = false): string {
  if (pct == null) return cssVar('text.secondary');
  if (bingo || pct < 25) return cssVar('state.bad');
  if (pct < 50) return cssVar('state.warn');
  return cssVar('state.ok');
}

export type EventCategory = 'Task' | 'Fuel' | 'Air' | 'Base' | 'Alert' | 'Contact' | 'Intel' | 'EMCON' | 'Weapon' | 'ROE';

export const CATEGORY_COLOUR: Record<EventCategory, string> = {
  Task: cssVar('text.secondary'),
  Fuel: cssVar('state.warn'),
  Air: cssVar('side.own'),
  Base: cssVar('ring.sensor'),
  Alert: cssVar('state.bad'),
  Contact: cssVar('side.enemy'),
  Intel: cssVar('sat.label'),
  EMCON: cssVar('ring.sensor'),
  Weapon: cssVar('weapon.offensive'),
  ROE: cssVar('state.warn'),
};

const SOURCE: Record<string, string> = {
  radar: 'radar',
  esm: 'ESM',
  irst: 'IRST',
  eo_ir: 'EO/IR',
  visual: 'visual',
  sonar: 'sonar',
  imagery: 'satellite imagery',
  sigint: 'SIGINT',
};

/** `C3 (Steregushchiy)` or `C3` while unclassified. */
function contactName(id: number, s: StringTable): string {
  const t = s.contactType(id);
  return t ? `C${id} (${t})` : `C${id}`;
}

/** Category chip and plain-English text of an event. */
export function describe(e: Event, s: StringTable): { category: EventCategory; text: string } {
  const n = (id: number) => s.name(id) || `unit ${id}`;
  const k = e.kind;
  if ('TaskChanged' in k) return { category: 'Task', text: `${n(k.TaskChanged.group)}: new task, ${k.TaskChanged.task.replace('_', ' ')}` };
  if ('StationChanged' in k)
    return { category: 'Task', text: `${n(k.StationChanged.group)} ${k.StationChanged.on ? 'on station' : 'left station'}` };
  if ('BingoRtb' in k) {
    const b = k.BingoRtb;
    return { category: 'Fuel', text: `${n(b.unit)} bingo at ${Math.round(b.fuel_kg)} kg, returning to ${n(b.base)}` };
  }
  if ('Return' in k) return { category: 'Task', text: `${n(k.Return.group)} returning to ${n(k.Return.base)} (${k.Return.why})` };
  if ('ToTanker' in k) return { category: 'Fuel', text: `${n(k.ToTanker.group)} heading for tanker ${n(k.ToTanker.tanker)}` };
  if ('Refuelled' in k) return { category: 'Fuel', text: `${n(k.Refuelled.unit)} refuelled from ${n(k.Refuelled.tanker)}` };
  if ('TookOff' in k) return { category: 'Air', text: `${n(k.TookOff.unit)} left ${n(k.TookOff.base)}` };
  if ('Landed' in k)
    return { category: 'Air', text: `${n(k.Landed.unit)} arrived at ${n(k.Landed.base)} with ${Math.round(k.Landed.fuel_kg)} kg fuel` };
  if ('TurnaroundDone' in k) return { category: 'Base', text: `${n(k.TurnaroundDone.unit)} ready at ${n(k.TurnaroundDone.base)}` };
  if ('BaseActivated' in k) return { category: 'Base', text: `${n(k.BaseActivated.base)} is active` };
  if ('ContactNew' in k) {
    const c = k.ContactNew;
    return {
      category: 'Contact',
      text: `New ${c.domain} contact C${c.contact}: ${SOURCE[c.source] ?? c.source}, ${n(c.observer)}`,
    };
  }
  if ('ContactClassified' in k) {
    const c = k.ContactClassified;
    return { category: 'Contact', text: `${contactName(c.contact, s)} classified to ${c.level} by ${SOURCE[c.source] ?? c.source}` };
  }
  if ('ContactCount' in k) {
    const c = k.ContactCount;
    return { category: 'Contact', text: `${contactName(c.contact, s)} counted: ${c.min === c.max ? c.min : `${c.min}–${c.max}`}` };
  }
  if ('ContactLost' in k) return { category: 'Contact', text: `${contactName(k.ContactLost.contact, s)} lost; last known position kept` };
  if ('IntelReport' in k) {
    const r = k.IntelReport;
    return { category: 'Intel', text: `${n(r.satellite)} report (pass ${elapsed(r.pass_ms)}): ${r.outcome}` };
  }
  if ('SatelliteTasked' in k) {
    const t = k.SatelliteTasked;
    return { category: 'Intel', text: `${n(t.satellite)} tasked: pass at ${elapsed(t.pass_ms)}, report by ${elapsed(t.report_ms)}` };
  }
  if ('EmconChanged' in k) {
    const m = k.EmconChanged;
    return { category: 'EMCON', text: `${n(m.group)} emission control ${m.emcon === 'on' ? 'ACTIVE' : m.emcon === 'off' ? 'SILENT' : 'BLINK'}` };
  }
  if ('LoadoutChanged' in k) {
    const l = k.LoadoutChanged;
    return { category: 'Base', text: `${n(l.unit)} loadout changed at ${n(l.base)}${l.loadout ? ` (${l.loadout})` : ' (custom)'}` };
  }
  if ('RearmShort' in k) {
    const r = k.RearmShort;
    return { category: 'Base', text: `${n(r.unit)} rearm short at ${n(r.base)}: ${r.short} × ${r.weapon} missing` };
  }
  if ('WeaponLaunched' in k) {
    const l = k.WeaponLaunched;
    const at = l.target && 'Contact' in l.target ? ` at ${contactName(l.target.Contact.contact, s)}` : '';
    const why = LAUNCH_REASON[l.reason] ?? '';
    return { category: 'Weapon', text: `${n(l.shooter)} fired ${l.intercept ? 'INT' : 'M'}-${l.tag} ${l.weapon_type}${at}${why}` };
  }
  if ('WeaponTerminated' in k) {
    const t = k.WeaponTerminated;
    return { category: 'Weapon', text: `${t.intercept ? 'INT' : 'M'}-${t.tag} ${t.weapon_type} ${outcomeText(t)}` };
  }
  if ('SalvoReport' in k) {
    const r = k.SalvoReport;
    const parts = [];
    if (r.intercepted > 0) parts.push(`${r.intercepted} intercepted`);
    if (r.decoyed > 0) parts.push(`${r.decoyed} decoyed`);
    if (r.hits > 0) parts.push(`${r.hits} ${r.hits === 1 ? 'hit' : 'hits'}`);
    if (r.misses > 0) parts.push(`${r.misses} ${r.misses === 1 ? 'miss' : 'misses'}`);
    const summary = parts.length > 0 ? parts.join(', ') : 'no effect';
    return {
      category: 'Weapon',
      text: `Salvo #${r.salvo} (${r.total} ${r.total === 1 ? 'missile' : 'missiles'} from ${n(r.shooter)}): ${summary}`,
    };
  }
  if ('AutonomousMode' in k) {
    const a = k.AutonomousMode;
    return {
      category: 'Alert',
      text:
        a.delay_s > 0
          ? `${n(a.group)} command post destroyed, switching to autonomous mode in ${duration(a.delay_s)}`
          : `${n(a.group)} command post destroyed, now in autonomous mode`,
    };
  }
  if ('IncomingWeapon' in k) {
    const w = k.IncomingWeapon;
    return {
      category: 'Alert',
      text: `${contactName(w.contact, s)} inbound on ${n(w.unit)}, impact in ${duration(w.time_to_impact_s)}`,
    };
  }
  if ('Damage' in k) {
    const d = k.Damage;
    const left = d.points_pct == null ? '' : `, ${Math.round(d.points_pct)}% left`;
    const what = d.subsystem ? `, ${d.subsystem.replace(/_/g, ' ')} out` : '';
    return { category: 'Alert', text: `${n(d.unit)} hit${left}${what}${d.aborted ? ', aborting mission' : ''}` };
  }
  if ('UnitDestroyed' in k) return { category: 'Alert', text: `${k.UnitDestroyed.name} destroyed` };
  if ('BaseComponentDamaged' in k) {
    const b = k.BaseComponentDamaged;
    return { category: 'Base', text: `${n(b.base)}: ${b.component.replace(/_/g, ' ')} damaged, repaired by ${elapsed(b.repaired_at_ms)}` };
  }
  if ('BaseComponentRepaired' in k) {
    const b = k.BaseComponentRepaired;
    return { category: 'Base', text: `${n(b.base)}: ${b.component.replace(/_/g, ' ')} repaired` };
  }
  if ('RoeRequest' in k) {
    const r = k.RoeRequest;
    return {
      category: 'ROE',
      text: `${n(r.group)} asks to engage ${contactName(r.contact, s)} with ${r.weapon_type} (Pk ${Math.round(r.pk * 100)}%)`,
    };
  }
  if ('RoeDecision' in k) return { category: 'ROE', text: `Request ${k.RoeDecision.request} ${k.RoeDecision.decision}` };
  if ('BorderCrossing' in k) {
    const b = k.BorderCrossing;
    return { category: 'Contact', text: `${contactName(b.contact, s)} ${b.inbound ? 'entered' : 'left'} our territory` };
  }
  return { category: 'Alert', text: `Auto-slow: ${k.AutoSlow.reason}` };
}

/** How a weapon's flight ended, in plain words with the numbers behind it (rule 4): the kill
 *  chance when the weapon fuzed, the miss distance, the countermeasures that drew it off. */
function outcomeText(t: Extract<Event['kind'], { WeaponTerminated: unknown }>['WeaponTerminated']): string {
  const f = t.factors;
  const pk = f.endgame ? `Pk ${Math.round(f.endgame.pk * 100)}%` : null;
  const miss = f.miss_distance_m == null ? null : `missed by ${Math.round(f.miss_distance_m)} m`;
  const detail = (...parts: (string | null)[]) => {
    const text = parts.filter(Boolean).join(', ');
    return text ? ` (${text})` : '';
  };
  switch (t.outcome) {
    case 'hit':
      return `${t.intercept ? 'intercepted its target' : 'hit its target'}${detail(pk)}`;
    case 'miss':
      return `${t.intercept ? 'missed its target' : 'missed'}${detail(miss, pk)}`;
    case 'decoyed':
      return `was decoyed by countermeasures${detail(f.cm_used > 0 ? `${f.cm_used} released` : null, pk)}`;
    case 'jammed':
      return `was jammed${detail(pk)}`;
    case 'intercepted':
      return 'was shot down by an interceptor';
    case 'out_of_energy':
      return 'ran out of energy';
    case 'lost_track':
      return 'lost its track and self-destructed';
    case 'self_destruct':
      return 'self-destructed';
  }
}

/** Why a weapon was fired, after the line (doc 03 §7). */
const LAUNCH_REASON: Record<string, string> = {
  self_defence: ' (self-defence)',
  ordered: ' (ordered)',
  territory: ' (in our territory)',
  return_fire: ' (return fire)',
  approved: ' (approved)',
};

/** Events that also go to the intel log (doc 03 §10). */
export const isIntel = (e: Event) =>
  'ContactNew' in e.kind ||
  'ContactClassified' in e.kind ||
  'ContactCount' in e.kind ||
  'ContactLost' in e.kind ||
  'IntelReport' in e.kind ||
  'SatelliteTasked' in e.kind;

/** The contact an event is about, for click-to-locate. */
export function contactOf(e: Event): number | null {
  const k = e.kind;
  if ('ContactNew' in k) return k.ContactNew.contact;
  if ('ContactClassified' in k) return k.ContactClassified.contact;
  if ('ContactCount' in k) return k.ContactCount.contact;
  if ('ContactLost' in k) return k.ContactLost.contact;
  if ('IntelReport' in k) return k.IntelReport.contacts[0] ?? null;
  if ('IncomingWeapon' in k) return k.IncomingWeapon.contact;
  if ('RoeRequest' in k) return k.RoeRequest.contact;
  if ('BorderCrossing' in k) return k.BorderCrossing.contact;
  return null;
}

/** Events that also go to the alert feed. */
export const isAlert = (e: Event) =>
  'AutoSlow' in e.kind ||
  'BingoRtb' in e.kind ||
  'RearmShort' in e.kind ||
  'IncomingWeapon' in e.kind ||
  'Damage' in e.kind ||
  'UnitDestroyed' in e.kind ||
  'AutonomousMode' in e.kind ||
  'RoeRequest' in e.kind;

/** The unit an event is about, for click-to-locate. */
export function subjectOf(e: Event): number | null {
  const k = e.kind;
  if ('TaskChanged' in k) return k.TaskChanged.group;
  if ('StationChanged' in k) return k.StationChanged.group;
  if ('BingoRtb' in k) return k.BingoRtb.unit;
  if ('Return' in k) return k.Return.group;
  if ('ToTanker' in k) return k.ToTanker.group;
  if ('Refuelled' in k) return k.Refuelled.unit;
  if ('TookOff' in k) return k.TookOff.unit;
  if ('Landed' in k) return k.Landed.unit;
  if ('TurnaroundDone' in k) return k.TurnaroundDone.unit;
  if ('BaseActivated' in k) return k.BaseActivated.base;
  if ('EmconChanged' in k) return k.EmconChanged.group;
  if ('SatelliteTasked' in k) return k.SatelliteTasked.satellite;
  if ('LoadoutChanged' in k) return k.LoadoutChanged.unit;
  if ('RearmShort' in k) return k.RearmShort.unit;
  if ('WeaponLaunched' in k) return k.WeaponLaunched.shooter;
  if ('AutonomousMode' in k) return k.AutonomousMode.group;
  if ('IncomingWeapon' in k) return k.IncomingWeapon.unit;
  if ('SalvoReport' in k) return k.SalvoReport.shooter;
  if ('Damage' in k) return k.Damage.unit;
  if ('BaseComponentDamaged' in k) return k.BaseComponentDamaged.base;
  if ('BaseComponentRepaired' in k) return k.BaseComponentRepaired.base;
  return null;
}
