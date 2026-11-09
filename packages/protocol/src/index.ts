// Types generated from the Rust engine by ts-rs. Run `pnpm gen` after changing them in Rust.
export type { ContactId } from './generated/ContactId';
export type { TickReport } from './generated/TickReport';
export type { UnitId } from './generated/UnitId';

// Engine API (P3): orders, events, card details, saves, side view.
export type { BaseBrief } from './generated/BaseBrief';
export type { BaseDetails } from './generated/BaseDetails';
export type { BaseKind } from './generated/BaseKind';
export type { BaseTarget } from './generated/BaseTarget';
export type { Command } from './generated/Command';
export type { Details } from './generated/Details';
export type { Event } from './generated/Event';
export type { EventKind } from './generated/EventKind';
export type { GroupDetails } from './generated/GroupDetails';
export type { JammerMode } from './generated/JammerMode';
export type { JammerView } from './generated/JammerView';
export type { MemberView } from './generated/MemberView';
export type { Order } from './generated/Order';
export type { OrderAck } from './generated/OrderAck';
export type { Ranges } from './generated/Ranges';
export type { Roe } from './generated/Roe';
export type { RoeChoice } from './generated/RoeChoice';
export type { RoeSource } from './generated/RoeSource';
export type { SaveHeader } from './generated/SaveHeader';
export type { SideInfo } from './generated/SideInfo';
export type { Speed } from './generated/Speed';
export type { StationedView } from './generated/StationedView';
export type { TaskOrder } from './generated/TaskOrder';
export type { TaskView } from './generated/TaskView';
export type { WeaponRounds } from './generated/WeaponRounds';
export { CFLAG, EFFECT, FLAG, RELATION, STATUS, STRING_KIND, SYMBOLS, type SymbolName } from './generated/symbols';
export {
  contactDomain,
  decodeSideView,
  statusOf,
  StringTable,
  type ContactColumns,
  type DomainName,
  type EffectColumns,
  type SideView,
  type StatusName,
  type UnitColumns,
  type WeaponColumns,
} from './sideview';

// Operations, reports and replay (P7).
export type { AreaSpec } from './generated/AreaSpec';
export type { CondSpec } from './generated/CondSpec';
export type { ContactLevel } from './generated/ContactLevel';
export type { Domain } from './generated/Domain';
export type { Issue } from './generated/Issue';
export type { OperationSpec } from './generated/OperationSpec';
export type { OperationView } from './generated/OperationView';
export type { OpState } from './generated/OpState';
export type { PhaseSpec } from './generated/PhaseSpec';
export type { PhaseState } from './generated/PhaseState';
export type { PhaseTally } from './generated/PhaseTally';
export type { PhaseTaskSpec } from './generated/PhaseTaskSpec';
export type { PhaseTaskView } from './generated/PhaseTaskView';
export type { PhaseView } from './generated/PhaseView';
export type { Report } from './generated/Report';
export type { SalvoLine } from './generated/SalvoLine';
export type { Severity } from './generated/Severity';
export type { Stamp } from './generated/Stamp';
export type { Totals } from './generated/Totals';
export type { TruthReview } from './generated/TruthReview';

// Rules of engagement (P5): the approval queue.
export type { ApprovalView } from './generated/ApprovalView';

// Fog of war (P4): contact and satellite cards.
export type { ContactDetails } from './generated/ContactDetails';
export type { ContactSourceView } from './generated/ContactSourceView';
export type { SatelliteDetails } from './generated/SatelliteDetails';
export type { TaskingView } from './generated/TaskingView';
export type { Emcon } from './generated/Emcon';

// Content source format (wb-content). The JSON Schema of the same format is
// generated to ./generated/content.schema.json.
export type { Archetype } from './generated/Archetype';
export type { Base } from './generated/Base';
export type { CellOverride } from './generated/CellOverride';
export type { Countermeasure } from './generated/Countermeasure';
export type { Datalink } from './generated/Datalink';
export type { Design } from './generated/Design';
export type { Doctrine } from './generated/Doctrine';
export type { EntryMeta } from './generated/EntryMeta';
export type { EwSuite } from './generated/EwSuite';
export type { Flag } from './generated/Flag';
export type { FlagPattern } from './generated/FlagPattern';
export type { PlaceKind } from './generated/PlaceKind';
export type { Inventory } from './generated/Inventory';
export type { Kind } from './generated/Kind';
export type { Loadout } from './generated/Loadout';
export type { Manifest } from './generated/Manifest';
export type { Nation } from './generated/Nation';
export type { NationSeed } from './generated/NationSeed';
export type { Platform } from './generated/Platform';
export type { Realism } from './generated/Realism';
export type { Sensor } from './generated/Sensor';
export type { System } from './generated/System';
export type { Weapon } from './generated/Weapon';
export type { World } from './generated/World';
