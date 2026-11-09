// `inventoryStore` (Plan/45 §2.3, §9.1; WP-CFEAT-10): the own bag and equipment as the server's `ItemContainer`, the cooldown
// groups, the hide-costume flags and a change counter. The reducers are pure functions over an immutable state value (they
// clone the 199-entry container when they change it); the handlers of `handlers.ts` call them with the decoded messages.
//
//   SpawnSelf.inventory   the whole container (`applyContainer`)
//   CreateItem            the template copied into each listed storage id with its count (`applyCreateItem`)
//   MoveItemNtf           `Swap(src, dst)` of the bag slots (`applyMoveItem`)
//   UpdateItem(Ex)        one field of one item; `UI_NUM <= 0` removes (unequipping first), `UI_COOLTIME` starts the group
//   DoEquipNtf (own)      the container move `DoEquip` / `UnEquip` (`applyEquip`)

import type { StoreApi } from 'zustand/vanilla';
import type { ItemContainer as WireContainer } from '../../gen/proto/flyff/v1/item_types_pb';
import type { StoreRegistry } from '../../stores/registry';
import { ItemContainer, type PackMax } from './container';
import { itemFromWire, type InvItem, type UpdateEnv, UI, updateItemField } from './items';
import { PARTS_LWEAPON, PARTS_RWEAPON } from '../../gen/defs/neuz';

/** `MAX_COOLTIME_TYPE`: the cooldown groups 1..3 (food, pill, skill). */
export const COOLDOWN_GROUPS = 3;

/** One running cooldown group: absolute client ms of the start and the end. */
export interface GroupCooldown {
  readonly startMs: number;
  readonly endMs: number;
}

export interface InventoryState {
  /** The container was received (`SpawnSelf`): before that the bag is unknown and no ITEM slot is touched. */
  readonly ready: boolean;
  readonly container: ItemContainer<InvItem>;
  /** Cooldown groups 1..3 (`index = group - 1`). */
  readonly cooldowns: readonly (GroupCooldown | null)[];
  /** `m_abHideCoat` of HAT, CLOTH, GLOVE, BOOTS, CLOAK. */
  readonly hideCoat: readonly [boolean, boolean, boolean, boolean, boolean];
  /** Bumps on every change of the container or the cooldowns. */
  readonly version: number;
  /**
   * Bumps when a worn item changed or any field of an item other than its count changed (an upgrade level, an element, the random-option word: the
   * tooltips are cached per `(serial, equipVersion)` and the serial stays the same).
   */
  readonly equipVersion: number;
}

export function initialInventoryState(): InventoryState {
  return {
    ready: false,
    container: new ItemContainer<InvItem>(168),
    cooldowns: [null, null, null],
    hideCoat: [false, false, false, false, false],
    version: 0,
    equipVersion: 0,
  };
}

export type InventoryStore = StoreApi<InventoryState>;

/** Scope `world`: the join batch of every world brings the whole container again. */
export function defineInventoryStore(registry: StoreRegistry): InventoryStore {
  const existing = registry.get('inventory');
  if (existing) return existing as unknown as InventoryStore;
  return registry.defineStore<InventoryState>({
    name: 'inventory',
    scope: 'world',
    owner: '45',
    initial: initialInventoryState,
  });
}

/** What the reducers need to know about item props (the items bundle, a fake in the tests). */
export interface ItemRules {
  /** `dwPackMax`. */
  packMax: PackMax;
  /** `dwParts` of an item id (`-1` = none). */
  parts(itemId: number): number;
  updateEnv: UpdateEnv;
}

const bump = (
  s: InventoryState,
  equip: boolean,
): Pick<InventoryState, 'version' | 'equipVersion'> => ({
  version: s.version + 1,
  equipVersion: equip ? s.equipVersion + 1 : s.equipVersion,
});

// ---- the join batch --------------------------------------------------------------------------------------------------------------

/** `SpawnSelf.inventory`: replaces the whole container. */
export function applyContainer(
  s: InventoryState,
  wire: WireContainer | undefined,
  hideCoat?: readonly boolean[],
): InventoryState {
  const container = wire
    ? ItemContainer.fromWire({
        max: wire.max,
        index: wire.index,
        objIndex: wire.objIndex,
        items: wire.items.map(itemFromWire),
      })
    : new ItemContainer<InvItem>(168);
  return {
    ...s,
    ready: wire !== undefined,
    container,
    hideCoat: hideCoat
      ? [
          hideCoat[0] === true,
          hideCoat[1] === true,
          hideCoat[2] === true,
          hideCoat[3] === true,
          hideCoat[4] === true,
        ]
      : s.hideCoat,
    ...bump(s, true),
  };
}

// ---- items -----------------------------------------------------------------------------------------------------------------------

/**
 * `CreateItem{item, obj_ids, nums}`: the template is copied into every listed storage id with the matching count (the server
 * decides slots and ids, one item may be split over several stacks).
 */
export function applyCreateItem(
  s: InventoryState,
  template: InvItem,
  objIds: readonly number[],
  nums: readonly number[],
): InventoryState {
  const c = s.container.clone();
  let changed = false;
  for (let i = 0; i < objIds.length; i += 1) {
    const id = objIds[i];
    const num = nums[i];
    if (id === undefined || num === undefined) continue;
    // the extra (reservation) of an entry that existed stays
    const old = c.getAtId(id);
    if (c.setAtId(id, { ...template, objId: id, num, extra: old?.extra ?? 0 })) changed = true;
  }
  if (!changed) return s;
  return { ...s, container: c, ...bump(s, false) };
}

/** `MoveItemNtf`: `Swap(src, dst)` of two bag slots. */
export function applyMoveItem(
  s: InventoryState,
  src: number,
  dst: number,
  rules: Pick<ItemRules, 'packMax'>,
): InventoryState {
  const c = s.container.clone();
  const out = c.swap(src, dst, rules.packMax);
  if (out.kind === 'nothing') return s;
  return { ...s, container: c, ...bump(s, false) };
}

/** The result of an `UpdateItem`: the new state and what the caller must do beyond the state. */
export interface UpdateResult {
  readonly state: InventoryState;
  /** `UI_FLAG` of a worn item: the part whose `EquipInfo.flag` changed, with the new flag. */
  readonly equipFlag?: { readonly part: number; readonly flag: number };
}

/**
 * `UpdateItem{obj_id, param, value, time}` of the own bag. `UI_NUM` / `UI_COOLTIME` set the count (<= 0 removes the item, a
 * worn one is taken off first, like `RemoveItemId`); every other parameter edits one field.
 */
export function applyUpdateItem(
  s: InventoryState,
  objId: number,
  param: number,
  value: number,
  time: number,
  rules: ItemRules,
): UpdateResult {
  const c0 = s.container;
  const it = c0.getAtId(objId);
  if (!it) return { state: s };
  if (param === UI.NUM || param === UI.COOLTIME) {
    const c = c0.clone();
    if ((value | 0) <= 0) {
      const part = c.partOf(objId);
      if (part !== null) c.unEquip(part);
      c.removeAtId(objId);
      return { state: { ...s, container: c, ...bump(s, part !== null) } };
    }
    c.replaceAtId(objId, { ...it, num: value & 0xffff });
    return { state: { ...s, container: c, ...bump(s, c0.isEquip(objId)) } };
  }
  const next = updateItemField(it, param, value, time, rules.updateEnv);
  if (!next) return { state: s };
  const c = c0.clone();
  c.replaceAtId(objId, next);
  // every field but the count changes the composed tooltip (`+N`, the element, the sockets, the lines), worn or not
  const state: InventoryState = { ...s, container: c, ...bump(s, true) };
  if (param === UI.FLAG) {
    // value = MAKELONG(slot index, flags): the flag also goes into the `EquipInfo` of `slot - indexNum`
    const slot = (value << 16) >> 16;
    const part = slot - c0.indexNum;
    if (part >= 0 && part < c0.max - c0.indexNum)
      return { state, equipFlag: { part, flag: next.flag } };
  }
  return { state };
}

/** `UpdateItemEx{obj_id, param: UI_RANDOMOPTITEMID, value}`: the whole packed 64-bit word. */
export function applyUpdateItemEx(
  s: InventoryState,
  objId: number,
  param: number,
  value: bigint,
): InventoryState {
  if (param !== UI.RANDOMOPTITEMID) return s;
  const it = s.container.getAtId(objId);
  if (!it) return s;
  const c = s.container.clone();
  c.replaceAtId(objId, { ...it, randomOpt: value });
  // the lines of the word are part of the tooltip
  return { ...s, container: c, ...bump(s, true) };
}

/** `UI_COOLTIME`: the cooldown group of an item starts (`m_cooltimeMgr.SetTime(group, prop->GetCoolTime())`). */
export function applyCooldown(
  s: InventoryState,
  group: number,
  nowMs: number,
  durationMs: number,
): InventoryState {
  if (group < 1 || group > COOLDOWN_GROUPS || durationMs <= 0) return s;
  const cooldowns = s.cooldowns.map((c, i) =>
    i === group - 1 ? { startMs: nowMs, endMs: nowMs + durationMs } : c,
  );
  return { ...s, cooldowns, ...bump(s, false) };
}

/** `CCooltimeMgr::GetGroup`: food 1, pill 2, skill 3 (only items with a `skill_ready`). */
export function cooldownGroup(
  kind2: number,
  kind3: number,
  skillReady: number,
  ik2Food: number,
  ik2Skill: number,
  ik3Pill: number,
): number {
  if (skillReady <= 0) return 0;
  if (kind2 === ik2Food) return kind3 === ik3Pill ? 2 : 1;
  if (kind2 === ik2Skill) return 3;
  return 0;
}

/** `m_Inventory.IsEmpty` checks etc. use this: the cooldown group is running at `nowMs`. */
export function cooldownRunning(s: InventoryState, group: number, nowMs: number): boolean {
  if (group < 1 || group > COOLDOWN_GROUPS) return false;
  const c = s.cooldowns[group - 1];
  return c !== null && c !== undefined && nowMs <= c.endMs;
}

// ---- equipment ------------------------------------------------------------------------------------------------------------------

/** What an own `DoEquipNtf` did, for the visual and the stat sync. */
export interface EquipResult {
  readonly state: InventoryState;
  /** Parts whose worn item changed, with the item now worn there (`null` = empty). */
  readonly changed: readonly { readonly part: number; readonly item: InvItem | null }[];
}

/**
 * `DoEquipNtf` of the own player: `CMover::DoEquip(item, equip, part)` on the container. An equip first takes off whatever
 * the target part holds (the server sends the displaced parts' own notifications first, so normally the part is empty); an
 * unequip of the right weapon while the left hand holds one moves the left weapon over (`swap_left_to_right`).
 * `part < 0` derives the part from the item prop.
 */
export function applyEquip(
  s: InventoryState,
  objId: number,
  equip: boolean,
  part: number,
  rules: Pick<ItemRules, 'parts'>,
): EquipResult | null {
  const c = s.container.clone();
  const it = c.getAtId(objId);
  if (!it) return null;
  const p = part >= 0 ? part : rules.parts(it.itemId);
  if (p < 0 || p >= c.max - c.indexNum) return null;
  const changed: { part: number; item: InvItem | null }[] = [];
  if (equip) {
    const slot = c.slotOf(objId);
    if (slot === null || slot >= c.indexNum) return null; // not in the bag
    const old = c.getEquip(p);
    if (old) {
      if (!c.unEquip(p)) return null;
      changed.push({ part: p, item: null });
    }
    if (!c.doEquip(slot, p)) return null;
    changed.push({ part: p, item: c.getAtId(objId) });
  } else {
    if (c.equipId(p) !== objId) return null;
    const hadLeft = p === PARTS_RWEAPON && c.getEquip(PARTS_LWEAPON) !== null;
    if (!c.unEquip(p)) return null;
    changed.push({ part: p, item: null });
    if (hadLeft) {
      const moved =
        c.equipId(PARTS_RWEAPON) === null
          ? c.moveEquip(PARTS_LWEAPON, PARTS_RWEAPON)
          : c.swapEquip(PARTS_RWEAPON, PARTS_LWEAPON);
      if (moved) {
        changed.push({ part: PARTS_RWEAPON, item: c.getEquip(PARTS_RWEAPON) });
        changed.push({ part: PARTS_LWEAPON, item: c.getEquip(PARTS_LWEAPON) });
      }
    }
  }
  return { state: { ...s, container: c, ...bump(s, true) }, changed };
}

/** `HideCoatNtf{position, hide}` of the own player. */
export function applyHideCoat(s: InventoryState, position: number, hide: boolean): InventoryState {
  if (position < 0 || position > 4 || s.hideCoat[position] === hide) return s;
  const next = [...s.hideCoat] as [boolean, boolean, boolean, boolean, boolean];
  next[position] = hide;
  return { ...s, hideCoat: next, ...bump(s, false) };
}

/**
 * `m_Inventory.Add( &itemElem )` (the client-side add of `OnGetItemGuildBank` for an item taken out of the guild bank; the server sends no `CreateItem` for it): the compatible
 * stacks are filled in slot order, the rest goes into empty bag slots, at most `packMax` per stack. Nothing changes when it does not fit.
 */
export function applyAddItem(s: InventoryState, item: InvItem, packMax: number): InventoryState {
  const c = s.container.clone();
  if (c.add(item, packMax) === null) return s;
  return { ...s, container: c, ...bump(s, false) };
}

/** The reservation of a client window (`SetExtra`): the item is dimmed and cannot be dragged while `extra > 0`. */
export function applyExtra(s: InventoryState, objId: number, extra: number): InventoryState {
  const it = s.container.getAtId(objId);
  if (!it || it.extra === extra) return s;
  const c = s.container.clone();
  c.replaceAtId(objId, { ...it, extra });
  return { ...s, container: c, ...bump(s, false) };
}
