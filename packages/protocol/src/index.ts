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
export type { MemberView } from './generated/MemberView';
export type { Order } from './generated/Order';
export type { OrderAck } from './generated/OrderAck';
export type { Ranges } from './generated/Ranges';
export type { SaveHeader } from './generated/SaveHeader';
export type { SideInfo } from './generated/SideInfo';
export type { Speed } from './generated/Speed';
export type { StationedView } from './generated/StationedView';
export type { TaskOrder } from './generated/TaskOrder';
export type { TaskView } from './generated/TaskView';
export { FLAG, STATUS, SYMBOLS, type SymbolName } from './generated/symbols';
export { decodeSideView, statusOf, StringTable, type SideView, type StatusName, type UnitColumns } from './sideview';

// Content source format (wb-content). The JSON Schema of the same format is
// generated to ./generated/content.schema.json.
export type { Base } from './generated/Base';
export type { Confidence } from './generated/Confidence';
export type { Countermeasure } from './generated/Countermeasure';
export type { Country } from './generated/Country';
export type { Datalink } from './generated/Datalink';
export type { Doctrine } from './generated/Doctrine';
export type { EntryMeta } from './generated/EntryMeta';
export type { EwSuite } from './generated/EwSuite';
export type { Inventory } from './generated/Inventory';
export type { Kind } from './generated/Kind';
export type { Loadout } from './generated/Loadout';
export type { Manifest } from './generated/Manifest';
export type { Platform } from './generated/Platform';
export type { Provenance } from './generated/Provenance';
export type { Realism } from './generated/Realism';
export type { Sensor } from './generated/Sensor';
export type { Source } from './generated/Source';
export type { System } from './generated/System';
export type { Weapon } from './generated/Weapon';
