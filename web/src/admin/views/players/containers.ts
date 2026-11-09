// The container model of the Items tab (Plan/25 section 4.4): one container of a character as the console shows it, built from the
// exact frame of a `ContainerSnapshot` (the WebSocket sends it; the REST routes carry the same frame as base64 in `pb`), the diff of
// two snapshots (the cells that flash) and the filters over the cells.
//
// Cells are addressed by their DISPLAY SLOT: `ItemElem.obj_id` of an admin snapshot is the grid position of the cell (the bag position,
// the pocket or bank slot, for the equipment the equipment part `PARTS_*`), and `RemoveItem.slot` uses the same number.
import { equals, fromBinary } from '@bufbuild/protobuf';
import {
  AdminServerMsgSchema,
  ContainerKind,
  type ContainerSnapshot,
} from '../../../gen/proto/admin';
import { ItemElemSchema, type ItemElem } from '../../../gen/proto/flyff/v1/item_types_pb';
import { n64 } from '../../../net/int64';
import type { ContainerLoc, ContainerView } from '../../gen/dto';

export type ContainerKindName = 'equipment' | 'inventory' | 'pocket' | 'bank';

/** One container of a character (or of its account: a bank). */
export interface ContainerModel {
  /** `kind:index:owner`, the identity of the container in the console. */
  readonly key: string;
  readonly kind: ContainerKindName;
  /** Pocket `0..3`, bank slot `0..3`; `0` otherwise. */
  readonly index: number;
  /** The character the container belongs to (a bank: the character of that bank slot). */
  readonly owner: number;
  /** Its name (the REST route labels the banks; live snapshots keep the label of the first load). */
  readonly ownerName: string;
  readonly rev: number;
  /** The grid size from the snapshot: the console never hard-codes one (`features.inventory_168` ...). */
  readonly capacity: number;
  readonly gold: number;
  /** `true`: live from the game; `false`: the saved state. */
  readonly live: boolean;
  /** Unix seconds of the save a saved container shows. */
  readonly savedAt: number;
  /** The online character through which an account-scoped bank is live; `0` = none. */
  readonly liveVia: number;
  /** Its name (the REST route names it; a live snapshot keeps the name of the first load). */
  readonly liveViaName: string;
  /** The cells by display slot. */
  readonly cells: ReadonlyMap<number, ItemElem>;
}

const KIND_NAMES: ReadonlyMap<ContainerKind, ContainerKindName> = new Map([
  [ContainerKind.EQUIPMENT, 'equipment'],
  [ContainerKind.INVENTORY, 'inventory'],
  [ContainerKind.POCKET, 'pocket'],
  [ContainerKind.BANK, 'bank'],
]);

export function containerKey(kind: ContainerKindName, index: number, owner: number): string {
  return `${kind}:${String(index)}:${String(owner)}`;
}

/** The base64 frame of a REST container as a snapshot; `null` when it is not a container frame. */
export function decodeFrame(pb: string): ContainerSnapshot | null {
  try {
    const raw = atob(pb);
    const bytes = new Uint8Array(raw.length);
    for (let i = 0; i < raw.length; i += 1) bytes[i] = raw.charCodeAt(i);
    const msg = fromBinary(AdminServerMsgSchema, bytes);
    return msg.msg.case === 'containerSnapshot' ? msg.msg.value : null;
  } catch {
    return null;
  }
}

/** A single `flyff.v1.ItemElem` from base64 (the attachment of a mail); `null` when it does not decode. */
export function decodeItem(b64text: string): ItemElem | null {
  try {
    const raw = atob(b64text);
    const bytes = new Uint8Array(raw.length);
    for (let i = 0; i < raw.length; i += 1) bytes[i] = raw.charCodeAt(i);
    return fromBinary(ItemElemSchema, bytes);
  } catch {
    return null;
  }
}

/** The model of a snapshot; `null` for a kind the Items tab does not show (guild bank, mailbox). */
export function modelOf(
  snapshot: ContainerSnapshot,
  ownerName = '',
  liveViaName = '',
): ContainerModel | null {
  const ref = snapshot.container;
  const kind = ref ? KIND_NAMES.get(ref.kind) : undefined;
  if (!ref || kind === undefined) return null;
  const owner = n64(ref.ownerCharacterId);
  const cells = new Map<number, ItemElem>();
  for (const item of snapshot.items) cells.set(item.objId, item);
  return {
    key: containerKey(kind, ref.index, owner),
    kind,
    index: ref.index,
    owner,
    ownerName,
    rev: snapshot.rev,
    capacity: snapshot.capacity,
    gold: n64(snapshot.gold),
    live: snapshot.live,
    savedAt: n64(snapshot.savedAt),
    liveVia: n64(snapshot.liveViaCharacterId),
    liveViaName,
    cells,
  };
}

/** The model of a REST container (`GET /characters/{id}/items`), labelled with the owner's name. */
export function modelOfView(view: ContainerView): ContainerModel | null {
  const snapshot = decodeFrame(view.pb);
  if (!snapshot) return null;
  return modelOf(snapshot, view.owner_name, view.live_via_name ?? '');
}

/** The display slots whose cell differs between two snapshots of one container (added, removed or changed). */
export function changedSlots(prev: ContainerModel | undefined, next: ContainerModel): number[] {
  if (!prev) return [];
  const slots = new Set<number>([...prev.cells.keys(), ...next.cells.keys()]);
  const out: number[] = [];
  for (const slot of slots) {
    const a = prev.cells.get(slot);
    const b = next.cells.get(slot);
    if (a === undefined || b === undefined) {
      out.push(slot);
    } else if (!equals(ItemElemSchema, a, b)) {
      out.push(slot);
    }
  }
  return out.sort((x, y) => x - y);
}

/** The `ContainerLoc` of the remove and penya routes for a container. */
export function locOf(c: Pick<ContainerModel, 'kind' | 'index'>): ContainerLoc {
  switch (c.kind) {
    case 'equipment':
      return 'equipment';
    case 'inventory':
      return 'inventory';
    case 'pocket':
      return { pocket: c.index };
    case 'bank':
      return { bank: { slot: c.index } };
  }
}

// ---- the cells -------------------------------------------------------------------------------------------------------------------

/** `CItemElem::m_byFlag` bits (`Item.h`): expired 1, binds 2, using 4. */
export const FLAG_EXPIRED = 0x01;
export const FLAG_BINDS = 0x02;
export const FLAG_USING = 0x04;

/** The upgrade level at or above which a removal asks for a typed confirmation (Plan/25 section 4.5). */
export const PRECIOUS_UPGRADE = 7;

export const isExpired = (e: ItemElem): boolean => (e.flag & FLAG_EXPIRED) !== 0;
export const isBound = (e: ItemElem): boolean => (e.flag & FLAG_BINDS) !== 0;

/** An awakened or random-option item carries a non-zero option word (`m_bItemResist` is the element, not an option). */
export const hasRandomOptions = (e: ItemElem): boolean => e.randomOptItemId !== 0n;

/** An item with an ultimate gem socket. */
export const hasUltimateSockets = (e: ItemElem): boolean =>
  e.piercing !== undefined && e.piercing.ultimateGemIds.length > 0;

/** Would removing this item ask for a typed confirmation (+7 and above, or ultimate)? */
export function isPrecious(e: ItemElem): boolean {
  return e.abilityOption >= PRECIOUS_UPGRADE || hasUltimateSockets(e);
}

export type KindFilter = 'any' | 'weapon' | 'armor' | 'general' | 'other';

/** The filter of the search box over a container (name, kind, upgrade >= N, random options). */
export interface CellFilter {
  readonly text: string;
  readonly kind: KindFilter;
  readonly minUpgrade: number;
  readonly randomOptions: boolean;
}

export const NO_FILTER: CellFilter = { text: '', kind: 'any', minUpgrade: 0, randomOptions: false };

export function isFiltering(f: CellFilter): boolean {
  return f.text.trim() !== '' || f.kind !== 'any' || f.minUpgrade > 0 || f.randomOptions;
}

/** What the filter needs to know about an item besides the cell. */
export interface CellInfo {
  readonly name: string;
  /** `ItemProp.item_kind1` (`IK1_*`), `-1` when unknown. */
  readonly kind1: number;
}

const IK1_WEAPON = 1;
const IK1_ARMOR = 2;
const IK1_GENERAL = 3;

/** Does the cell pass the filter? A cell the filter hides is shown dimmed, not removed, so the grid keeps its shape. */
export function matchesFilter(f: CellFilter, e: ItemElem, info: CellInfo): boolean {
  const text = f.text.trim().toLowerCase();
  if (text !== '') {
    const byId = /^\d+$/.test(text) && e.itemId === Number(text);
    const bySerial = /^#\d+$/.test(text) && e.serial === BigInt(text.slice(1));
    if (!byId && !bySerial && !info.name.toLowerCase().includes(text)) return false;
  }
  if (f.minUpgrade > 0 && e.abilityOption < f.minUpgrade) return false;
  if (f.randomOptions && !hasRandomOptions(e)) return false;
  switch (f.kind) {
    case 'any':
      return true;
    case 'weapon':
      return info.kind1 === IK1_WEAPON;
    case 'armor':
      return info.kind1 === IK1_ARMOR;
    case 'general':
      return info.kind1 === IK1_GENERAL;
    case 'other':
      return info.kind1 !== IK1_WEAPON && info.kind1 !== IK1_ARMOR && info.kind1 !== IK1_GENERAL;
  }
}
