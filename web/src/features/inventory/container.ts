// `ItemContainer`: the client's copy of `CItemContainer<CItemElem>` (Plan/19 §3.1-§3.2; ref/13 §6.1; `src:_Common/Item.h:341-949`),
// ported from the server's `flyff_game::item::container` so that both sides keep the SAME layout. Two levels of addressing:
//
//   * the STORAGE entry `items[id]`: an item's `objId` is its storage index for as long as it exists; packets name items by it;
//   * the SLOT `index[slot]`: the storage entry shown in a visible slot. Slots `0..indexNum` are the bag grid (168, or 42 without
//     `features.inventory_168`), the following ones the equipment slots (`indexNum + part`). `-1` = `NULL_ID`.
//
// `objIndex[id]` is the inverse (storage entry -> slot, `-1` = shown nowhere). The class is a plain mutable value: the store
// holds containers as immutable snapshots and the reducers work on a `clone()` (the container is 199 entries).
//
// KEEP-QUIRKs of the original (KQ-15): stacks merge on equal id, flags and charged state only (the stack cap is the prop's
// `pack_max`); two full stacks do not swap; `unEquip` fails with a full bag.

import { MAX_HUMAN_PARTS } from '../../gen/defs/neuz';

/** `NULL_ID` of the slot arrays (the wire uses `-1`). */
export const NO_SLOT = -1;

/** The part of an item the container decides on (`CItemElem`: id, flags, charged, count; the rest rides along). */
export interface StackItem {
  readonly objId: number;
  readonly itemId: number;
  readonly num: number;
  /** `m_byFlag`. */
  readonly flag: number;
  readonly charged: boolean;
}

/** What `swap` did to the two slots. */
export type SwapOutcome =
  | { readonly kind: 'nothing' }
  | { readonly kind: 'swapped' }
  /** Compatible stacks merged: `removed` = the source stack is gone; `moved` pieces went over. */
  | { readonly kind: 'merged'; readonly removed: boolean; readonly moved: number };

/** `dwPackMax` of an item id (`> 1` = stackable). */
export type PackMax = (itemId: number) => number;

export class ItemContainer<T extends StackItem = StackItem> {
  /** `m_dwIndexNum`: the number of bag slots. */
  readonly indexNum: number;
  /** Storage entries by object id (`null` = empty). */
  readonly items: (T | null)[];
  /** Slot -> storage id (`-1` = nothing shown). */
  readonly index: number[];
  /** Storage id -> slot (`-1` = shown nowhere). */
  readonly objIndex: number[];

  /** `SetItemContainer(indexNum, extra)` + `Clear()`. */
  constructor(indexNum: number, extra: number = MAX_HUMAN_PARTS) {
    this.indexNum = indexNum;
    const max = indexNum + extra;
    this.items = Array.from({ length: max }, () => null);
    this.index = Array.from({ length: max }, (_, i) => (i < indexNum ? i : NO_SLOT));
    this.objIndex = Array.from({ length: max }, (_, i) => (i < indexNum ? i : NO_SLOT));
  }

  /**
   * The container of the wire form (`ItemContainer{max, index, items, objIndex}`). The arrays are taken as sent (the server's
   * layout is authoritative); a short or long array is cut or padded with `-1`, an item outside the storage is dropped.
   */
  static fromWire<T extends StackItem>(
    wire: {
      readonly max: number;
      readonly index: readonly number[];
      readonly objIndex: readonly number[];
      readonly items: readonly T[];
    },
    /** The equipment slots behind the bag: 31 for the inventory, `0` for a bank, a pocket or a guild bank (`indexNum = max`). */
    extra: number = MAX_HUMAN_PARTS,
  ): ItemContainer<T> {
    const max = Math.max(wire.max, 0);
    const indexNum = Math.max(max - extra, 0);
    const c = new ItemContainer<T>(indexNum, max - indexNum);
    for (let i = 0; i < max; i += 1) {
      c.index[i] = wire.index[i] ?? NO_SLOT;
      c.objIndex[i] = wire.objIndex[i] ?? NO_SLOT;
    }
    for (const it of wire.items) {
      if (it.objId >= 0 && it.objId < max) c.items[it.objId] = it;
    }
    return c;
  }

  clone(): ItemContainer<T> {
    const c = new ItemContainer<T>(this.indexNum, this.items.length - this.indexNum);
    for (let i = 0; i < this.items.length; i += 1) {
      c.items[i] = this.items[i] ?? null;
      c.index[i] = this.index[i] ?? NO_SLOT;
      c.objIndex[i] = this.objIndex[i] ?? NO_SLOT;
    }
    return c;
  }

  /** `m_dwItemMax`. */
  get max(): number {
    return this.items.length;
  }

  // ---- reads --------------------------------------------------------------------------------------------------------------

  /** `GetAtId`: the item of a storage entry. */
  getAtId(id: number): T | null {
    return this.items[id] ?? null;
  }

  /** The storage entry shown in `slot` (`-1` / `null` = nothing). */
  objAtSlot(slot: number): number | null {
    const e = this.index[slot];
    return e === undefined || e < 0 || e >= this.items.length ? null : e;
  }

  /** `GetAt(slot)`: the item shown in a slot. */
  getAtSlot(slot: number): T | null {
    const e = this.objAtSlot(slot);
    return e === null ? null : (this.items[e] ?? null);
  }

  /** The slot an entry is shown in (`null` = nowhere). */
  slotOf(id: number): number | null {
    const s = this.objIndex[id];
    return s === undefined || s < 0 ? null : s;
  }

  /** `IsEquip(id)`: the entry is shown in an equipment slot. */
  isEquip(id: number): boolean {
    const s = this.slotOf(id);
    return s !== null && s >= this.indexNum;
  }

  /** The storage id worn in `part` (`null` = nothing). */
  equipId(part: number): number | null {
    if (part < 0 || part >= this.items.length - this.indexNum) return null;
    const e = this.objAtSlot(this.indexNum + part);
    return e !== null && this.items[e] ? e : null;
  }

  /** `GetEquip(part)`. */
  getEquip(part: number): T | null {
    const e = this.equipId(part);
    return e === null ? null : (this.items[e] ?? null);
  }

  /** The part an equipped entry is worn in (`null` = not worn). */
  partOf(id: number): number | null {
    const s = this.slotOf(id);
    return s !== null && s >= this.indexNum ? s - this.indexNum : null;
  }

  /** Bag slots (`0..indexNum`) whose entry is empty. */
  emptyCount(): number {
    let n = 0;
    for (let s = 0; s < this.indexNum; s += 1) {
      const e = this.objAtSlot(s);
      if (e !== null && this.items[e] === null) n += 1;
    }
    return n;
  }

  /** `GetItemCount` / `GetAtItemNum`: the total count of an item id over every entry, equipment included. */
  countOf(itemId: number): number {
    if (itemId === 0) return 0;
    let n = 0;
    for (const it of this.items) if (it && it.itemId === itemId) n += it.num;
    return n;
  }

  /** `Find(itemId)`: the first BAG slot (slot order) that shows the item id. */
  find(itemId: number): T | null {
    if (itemId === 0) return null;
    for (let s = 0; s < this.indexNum; s += 1) {
      const it = this.getAtSlot(s);
      if (it && it.itemId === itemId) return it;
    }
    return null;
  }

  // ---- writes -------------------------------------------------------------------------------------------------------------

  /** `SetAtId` (`OnCreateItem`): the entry takes the item; the slot arrays stay (the server's layout is authoritative). */
  setAtId(id: number, item: T): boolean {
    if (id < 0 || id >= this.items.length) return false;
    this.items[id] = item;
    return true;
  }

  /**
   * `Add(elem)` (Item.h:660-759) for a container without equipment (a bank compartment, a pocket): the compatible stacks are
   * filled in slot order (stackable items only), then the rest goes into empty bag slots in slot order, at most `packMax` each.
   * Nothing changes and `null` comes back when the item does not fit (`IsFull`). Otherwise the touched storage ids with their NEW
   * total counts, in the order they were filled (`created` = the ones that were empty before). The client re-runs the server's
   * `Add` on `PutItemBank` (the message names only the first touched entry), so both sides place the item identically.
   */
  add(
    item: T,
    packMax: number,
  ): {
    readonly touched: readonly { readonly id: number; readonly count: number }[];
    readonly created: readonly number[];
  } | null {
    const cap = Math.max(1, packMax);
    const plan: { id: number; count: number; isNew: boolean }[] = [];
    let left = item.num;
    if (cap > 1) {
      for (let slot = 0; slot < this.indexNum && left > 0; slot += 1) {
        const id = this.objAtSlot(slot);
        const x = id === null ? null : (this.items[id] ?? null);
        if (
          id !== null &&
          x &&
          x.itemId === item.itemId &&
          x.flag === item.flag &&
          x.charged === item.charged &&
          x.num < cap
        ) {
          const put = Math.min(cap - x.num, left);
          plan.push({ id, count: x.num + put, isNew: false });
          left -= put;
        }
      }
    }
    for (let slot = 0; slot < this.indexNum && left > 0; slot += 1) {
      const id = this.objAtSlot(slot);
      if (id !== null && this.items[id] === null) {
        const put = Math.min(cap, left);
        plan.push({ id, count: put, isNew: true });
        left -= put;
      }
    }
    if (left > 0) return null;
    for (const p of plan) {
      const cur = this.items[p.id];
      if (p.isNew) this.items[p.id] = { ...item, objId: p.id, num: p.count };
      else if (cur) this.items[p.id] = { ...cur, num: p.count };
    }
    return {
      touched: plan.map((p) => ({ id: p.id, count: p.count })),
      created: plan.filter((p) => p.isNew).map((p) => p.id),
    };
  }

  /** Replaces the item of an occupied entry (an update of its fields). */
  replaceAtId(id: number, item: T): boolean {
    if (this.items[id] === null || this.items[id] === undefined) return false;
    this.items[id] = item;
    return true;
  }

  /** `RemoveAtId`: the entry empties; an equipped one also frees its equipment slot. */
  removeAtId(id: number): void {
    if (id < 0 || id >= this.items.length) return;
    const slot = this.objIndex[id];
    if (slot === undefined || slot < 0 || slot >= this.items.length) return;
    this.items[id] = null;
    if (slot >= this.indexNum) {
      this.index[slot] = NO_SLOT;
      this.objIndex[id] = NO_SLOT;
    }
  }

  /** `Swap2`: exchanges the entries shown in two slots and fixes the inverse index. */
  private swapSlots(src: number, dst: number): void {
    const a = this.index[src];
    const b = this.index[dst];
    if (a === undefined || b === undefined) return;
    this.index[src] = b;
    this.index[dst] = a;
    if (b >= 0 && b < this.objIndex.length) this.objIndex[b] = src;
    if (a >= 0 && a < this.objIndex.length) this.objIndex[a] = dst;
  }

  /**
   * `Swap(src, dst)` (`OnMoveItem`): compatible stacks merge (same id, flag and charged state, `pack_max > 1`): the source goes
   * over as far as the destination has room; everything else exchanges the two slots.
   */
  swap(src: number, dst: number, packMax: PackMax): SwapOutcome {
    if (src === dst || src < 0 || dst < 0 || src >= this.items.length || dst >= this.items.length) {
      return { kind: 'nothing' };
    }
    const a = this.getAtId(this.index[src] ?? -1);
    const b = this.getAtId(this.index[dst] ?? -1);
    if (a && b && a.itemId === b.itemId && a.flag === b.flag && a.charged === b.charged) {
      const cap = packMax(a.itemId);
      if (cap > 1) {
        const room = cap - b.num;
        if (room >= a.num) {
          this.items[b.objId] = { ...b, num: b.num + a.num };
          this.removeAtId(a.objId);
          return { kind: 'merged', removed: true, moved: a.num };
        }
        // two full stacks stay (KQ-15); a partial merge moves what fits
        this.items[b.objId] = { ...b, num: b.num + room };
        this.items[a.objId] = { ...a, num: a.num - room };
        return { kind: 'merged', removed: false, moved: room };
      }
    }
    this.swapSlots(src, dst);
    return { kind: 'swapped' };
  }

  /**
   * `DoEquip(src, part)` (Item.h:545-569): the item shown in bag slot `src` moves to equipment slot `indexNum + part`; the first
   * storage entry that is empty and shown nowhere takes its place in the bag. `false` (nothing changed) when there is no such
   * entry, the source shows nothing, or the equipment slot is occupied.
   */
  doEquip(src: number, part: number): boolean {
    const dst = this.indexNum + part;
    if (
      part < 0 ||
      src === dst ||
      src < 0 ||
      src >= this.items.length ||
      dst >= this.items.length
    ) {
      return false;
    }
    const itemId = this.objAtSlot(src);
    if (itemId === null || this.objAtSlot(dst) !== null) return false;
    let free = -1;
    for (let i = 0; i < this.items.length; i += 1) {
      if (this.items[i] === null && (this.objIndex[i] ?? NO_SLOT) < 0) {
        free = i;
        break;
      }
    }
    if (free < 0) return false;
    this.index[dst] = itemId;
    this.index[src] = free;
    this.objIndex[free] = src;
    this.objIndex[itemId] = dst;
    return true;
  }

  /**
   * `UnEquip(part)` (Item.h:571-597): the item in equipment part `part` moves to the first bag slot whose storage entry is
   * empty; that entry becomes unshown. `false` when the bag has no such slot (a full bag) or nothing is worn there.
   */
  unEquip(part: number): boolean {
    const eq = this.indexNum + part;
    if (part < 0 || eq >= this.items.length) return false;
    const itemId = this.objAtSlot(eq);
    if (itemId === null) return false;
    for (let i = 0; i < this.indexNum; i += 1) {
      const e = this.objAtSlot(i);
      if (e === null) continue;
      if (this.items[e] === null) {
        this.objIndex[e] = NO_SLOT;
        this.index[eq] = NO_SLOT;
        this.objIndex[itemId] = i;
        this.index[i] = itemId;
        return true;
      }
    }
    return false;
  }

  /** Moves the item of equipment part `from` to the EMPTY part `to` (the left weapon becoming the right one). */
  moveEquip(from: number, to: number): boolean {
    const f = this.indexNum + from;
    const t = this.indexNum + to;
    if (f >= this.index.length || t >= this.index.length) return false;
    if (this.objAtSlot(f) === null || this.objAtSlot(t) !== null) return false;
    this.swapSlots(f, t);
    return true;
  }

  /** Exchanges the items of two occupied equipment parts. */
  swapEquip(a: number, b: number): boolean {
    const x = this.indexNum + a;
    const y = this.indexNum + b;
    if (x === y || x >= this.index.length || y >= this.index.length) return false;
    if (this.objAtSlot(x) === null || this.objAtSlot(y) === null) return false;
    this.swapSlots(x, y);
    return true;
  }

  /**
   * The structural invariants (tests): `index` and `objIndex` are inverse, no entry is shown twice and every occupied entry is
   * shown. Returns `null` when they hold, else a description of the first violation.
   */
  checkInvariants(): string | null {
    const n = this.items.length;
    if (this.index.length !== n || this.objIndex.length !== n) return 'array lengths differ';
    for (let slot = 0; slot < n; slot += 1) {
      const e = this.index[slot] ?? NO_SLOT;
      if (e < 0) continue;
      if (e >= n) return `slot ${String(slot)} shows out-of-range entry ${String(e)}`;
      if (this.objIndex[e] !== slot) {
        return `slot ${String(slot)} shows ${String(e)} but objIndex[${String(e)}] = ${String(this.objIndex[e])}`;
      }
    }
    for (let e = 0; e < n; e += 1) {
      const s = this.objIndex[e] ?? NO_SLOT;
      if (s >= 0) {
        if (this.index[s] !== e) {
          return `objIndex[${String(e)}] = ${String(s)} but index[${String(s)}] = ${String(this.index[s])}`;
        }
      } else if (this.items[e] !== null) {
        return `occupied entry ${String(e)} is shown nowhere`;
      }
    }
    return null;
  }
}
