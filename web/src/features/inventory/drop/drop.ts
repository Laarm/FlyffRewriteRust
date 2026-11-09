// Dragging items out of the inventory (Plan/45 §9.4 "Inventory -> elsewhere"; ref/99 §4.2.3; `CWndWorld::OnDropIcon`
// `WndWorld.cpp:7972-8150`, `CWndInventory::OnDropIcon` trash slot `WndField.cpp:3022-3062`; WP-CFEAT-10):
//
//   world (ground)  refuse guild cloaks silently; charged -> `TID_GAME_NOTDROP`; reserved -> forbid cursor; undestructable ->
//                   `TID_GAME_ERROR_UNDESTRUCTABLE_ITEM`; quest item -> `APP_QUEITMWARNING` (then the drop windows); count 1 ->
//                   `APP_DROP_CONFIRM`, else `APP_DROP_ITEM` (quantity); with the event flag 16 (`drop_item_remove`) the windows show
//                   `TID_GAME_DROPITEMREMOVE` / `...REMOVE2`; OK -> `DropItem{0, objId, num, pos}`
//   trash slot      reserved, worn and sealed-character items refuse silently; undestructable -> its text; else
//                   `APP_INVEN_REMOVE_ITEM` (quantity) -> `RemoveInvenItem{objId, num}`
//
// The windows close themselves when the item becomes reserved (`reserved()`, the original's `__DROP_CONFIRM_BUG`).

import {
  APP_BEAUTY_SHOP_EX,
  APP_BEAUTY_SHOP_SKIN,
  APP_COMMON_BANK,
  APP_GUILD_BANK,
  APP_GUILD_MERIT1,
  APP_INVENTORY,
  APP_REPAIR,
  APP_SHOPEX,
  APP_SUMMON_ANGEL,
  APP_TEST,
  APP_TRADE,
} from '../../../gen/defs/resdata';
import { ITYPE_ITEM } from '../../../gen/defs/define';
import { II_SYS_SYS_SCR_SEALCHARACTER } from '../../../gen/defs/item';
import {
  TID_ERROR_NOT_ITEM_NUM,
  TID_GAME_CANNOT_DROPMONEY,
  TID_GAME_DROPITEMREMOVE,
  TID_GAME_DROPITEMREMOVE2,
  TID_GAME_ERROR_UNDESTRUCTABLE_ITEM,
  TID_GAME_NOTDROP,
  TID_GAME_QUEITMWARNING,
  TID_GAME_REPAIR_NOTDROP,
  TID_GAME_TRADELIMITITEM,
} from '../../../gen/defs/text';
import type { ClientItem } from '../../../data';
import { SC, type Shortcut } from '../../../ui/dnd/shortcut';
import type { Point } from '../../../ui/layout/geometry';
import type { ItemDialogsService } from '../../../ui/windows/dialogs';
import { isCharged, isGuildCloak, isQuestItem, isReserved, isUndestructable } from '../info';
import type { InvItem } from '../items';
import type { InventorySenders } from '../senders';
import type { InvItemView } from '../view';

export type Position = readonly [number, number, number];

/** What the drop flows need from the client. */
export interface DropEnv {
  /** The item with this object id (`GetItemId`). */
  item(objId: number): InvItem | null;
  /** The shell's item view (name, icon) for the dialogs. */
  itemView(item: InvItem): InvItemView | null;
  prop(itemId: number): ClientItem | undefined;
  /** `IsEquip(objId)`. */
  isWorn(objId: number): boolean;
  isOpen(app: number): boolean;
  /** `PutString(GETTEXT(tid))`. */
  say(tid: number): void;
  text(tid: number): string;
  /** `SetForbid(TRUE)`: the forbid icon at the pointer. */
  forbid(): void;
  /** The ground point for a drop at the UI point (`ClientPointToVector`); the player's own position when nothing is hit. */
  groundPos(pt: Point): Position;
  /** `g_eLocal.GetState(EVE_DROPITEMREMOVE)`: the drop destroys the item (the shipped setting). */
  dropRemoves(): boolean;
  send: Pick<InventorySenders, 'dropItem' | 'removeItem'>;
  dialogs: Pick<
    ItemDialogsService,
    'openDropItem' | 'openDropConfirm' | 'openQueItemWarning' | 'openInvenRemove'
  >;
  /** `PlaySound(SND_INF_GROUNDDROP)`. */
  dropSound?(): void;
}

/** The APP id of the frame a shortcut came from (`pShortcut->m_pFromWnd->GetFrameWnd()->GetWndId()`), `0` when unknown. */
export function fromApp(sc: Shortcut): number {
  return sc.from?.getFrameWnd()?.id ?? 0;
}

/** Windows that forbid dropping inventory items (a shop, a bank, a trade ... is open). */
const BUSY_APPS: readonly number[] = [
  APP_SHOPEX,
  APP_GUILD_MERIT1,
  APP_GUILD_BANK,
  APP_COMMON_BANK,
  APP_BEAUTY_SHOP_EX,
  APP_BEAUTY_SHOP_SKIN,
  APP_SUMMON_ANGEL,
  APP_TRADE,
];

/**
 * `CWndWorld::OnDropIcon`: a shortcut dropped on the 3-D view. Returns whether the drop was accepted (a refusal shows the forbid
 * icon: the drag controller does that when this returns `false`).
 */
export function dropOnWorld(env: DropEnv, sc: Shortcut, pt: Point): boolean {
  const fromInventory = fromApp(sc) === APP_INVENTORY;
  if (env.isOpen(APP_TEST)) {
    env.say(TID_GAME_TRADELIMITITEM);
    return false;
  }
  if (BUSY_APPS.some((a) => env.isOpen(a))) {
    if (fromInventory) {
      env.say(TID_GAME_TRADELIMITITEM);
      return false;
    }
  } else if (env.isOpen(APP_REPAIR) && fromInventory) {
    env.say(TID_GAME_REPAIR_NOTDROP);
    return false;
  }
  if (sc.kind !== SC.ITEM || !fromInventory) return false;
  const pos = env.groundPos(pt);
  env.dropSound?.();
  if (sc.data === 0) {
    // the purse cannot be dropped (V8 and later)
    env.say(TID_GAME_CANNOT_DROPMONEY);
    return false;
  }
  const item = env.item(sc.id);
  if (!item) return false;
  const prop = env.prop(item.itemId);
  if (isGuildCloak(item, prop)) return false;
  if (isCharged(item)) {
    env.say(TID_GAME_NOTDROP);
    return false;
  }
  if (isReserved(item)) return false; // `IsUsingItem`: the caller's forbid icon
  if (isUndestructable(prop)) {
    env.say(TID_GAME_ERROR_UNDESTRUCTABLE_ITEM);
    return false;
  }
  const view = (): InvItem | null => env.item(item.objId);
  const reserved = (): boolean => {
    const cur = view();
    return cur === null || isReserved(cur);
  };
  const openQuantity = (removeTexts: boolean): void => {
    if (item.num === 1) {
      env.dialogs.openDropConfirm({
        ...(removeTexts && env.dropRemoves() ? { text: env.text(TID_GAME_DROPITEMREMOVE) } : {}),
        reserved,
        yes: () => {
          env.send.dropItem(item.objId, 1, pos);
        },
      });
    } else {
      env.dialogs.openDropItem({
        count: item.num,
        ...(removeTexts && env.dropRemoves() ? { text: env.text(TID_GAME_DROPITEMREMOVE2) } : {}),
        reserved,
        confirm: (n) => {
          env.send.dropItem(item.objId, n, pos);
        },
      });
    }
  };
  if (isQuestItem(prop)) {
    // a quest item asks first; its follow-up windows carry no event text (KEEP, `CWndQuestItemWarning`)
    env.dialogs.openQueItemWarning({
      text: env.text(TID_GAME_QUEITMWARNING),
      yes: () => {
        openQuantity(false);
      },
    });
  } else {
    openQuantity(true);
  }
  return true;
}

/** `CWndInventory::OnDropIcon` on the trash slot (`WIDC_CUSTOM21`): the gates, then `APP_INVEN_REMOVE_ITEM`. */
export function dropOnTrash(env: DropEnv, sc: Shortcut): boolean {
  if (sc.type !== ITYPE_ITEM || sc.data === 0) return false;
  const item = env.item(sc.id);
  if (!item) return false;
  if (item.extra > 0) return false;
  const view = env.itemView(item);
  if (!view) return false;
  // a worn item cannot be deleted
  if (env.isWorn(item.objId)) return false;
  const prop = env.prop(item.itemId);
  if (isUndestructable(prop)) {
    env.say(TID_GAME_ERROR_UNDESTRUCTABLE_ITEM);
    return false;
  }
  if (item.itemId === II_SYS_SYS_SCR_SEALCHARACTER) return false;
  env.dialogs.openInvenRemove({
    item: view,
    confirm: (n) => {
      env.send.removeItem(item.objId, n);
    },
    invalid: () => {
      env.say(TID_ERROR_NOT_ITEM_NUM);
    },
  });
  return true;
}
