// The environment and the contract of the safe-upgrade machines (Plan/45 §18.2; WP-CFEAT-24). The machines are pure state over this environment: the
// module builds one over the inventory feature, the tests build theirs by hand.

import type { DialogItem } from '../../../ui/windows/dialogs/ports';
import type { SmeltSafetyView, SmeltSlot } from '../../../ui/windows/upgrade/ports';
import type { InvItem } from '../../inventory/items';
import type { SmeltProp } from './rules';

/** `0xFFFF` kept verbatim for "no item" in the u16 ids of `SmeltSafetyReq`. */
export const NO_U16 = 0xffff;

/** What a `SmeltSafetyReq` carries. */
export interface SmeltSend {
  readonly item: number;
  readonly material: number;
  readonly protection: number;
  readonly scroll: number;
}

export interface SmeltEnv {
  /** The client clock in ms. */
  now(): number;
  /** `gameplay.enchant_time_s * 1000`. */
  enchantMs(): number;
  prop(itemId: number): SmeltProp | undefined;
  /** The inventory item of an object id (bag and equipment), `null` when gone. */
  item(objId: number): InvItem | null;
  /** Every stack of an item id in the bag, in slot order (`GetAtItemId` of the original returns the first). */
  stacks(itemId: number): readonly InvItem[];
  /** `SetExtra( n )`. */
  setExtra(objId: number, extra: number): void;
  isWorn(objId: number): boolean;
  /** The icon file of an item id (`item/itm_x.dds`). */
  icon(itemId: number): string | null;
  /** The shell's view of an item (name, icon, tooltip data). */
  dialogItem(item: InvItem): DialogItem | null;
  /** `CItemElem::IsPierceAble()` (no argument). */
  pierceAble(item: InvItem): boolean;
  /** The own penya. */
  gold(): number;
  /** `SendNewSmelt` / `SendSmeltSafety`. */
  send(msg: SmeltSend): void;
  /** `PutString( text )` of a literal. */
  say(text: string): void;
  /** `PutString( prj.GetText( tid ) )`. */
  sayTid(tid: number): void;
  /** The text of a `TID_*`. */
  text(tid: number): string;
}

/** The two machines (the auto window and the one-shot window) behind `SmeltSafetyActions`. */
export interface SmeltMachine {
  readonly kind: 'auto' | 'oneShot';
  /** The view for the window (recomputed at every call, cheap). */
  view(): SmeltSafetyView;
  /** `SetItem`: a drop or a double click. `true` = the item was used. */
  setItem(objId: number): boolean;
  clear(slot: SmeltSlot): void;
  start(): void;
  stop(): void;
  reset(): void;
  adjust(delta: 1 | -1): void;
  /** The pacing: sends the next try when it is due. Called every frame. */
  tick(): void;
  /** `SmeltSafetyNtf{result}`. */
  onResult(result: number): void;
  /** The window closed: every reservation goes. */
  dispose(): void;
  /** The upgrade runs. */
  readonly running: boolean;
  readonly counters: {
    readonly tries: number;
    readonly successes: number;
    readonly failures: number;
  };
}
