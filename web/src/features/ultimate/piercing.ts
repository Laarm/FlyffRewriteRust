// The piercing windows of WP-UI-31 wired (Plan/45 §11.2, §18.6; WP-CFEAT-26): `APP_PIERCING` (the menu `MMI_PIERCING`, kept although the shipped data
// offers no NPC: with `features.no_piercing` the server refuses every gear target with `TID_PIERCING_POSSIBLE_ITEM`) and `APP_SMELT_REMOVE_PIERCING_EX`
// (`MMI_SMELT_REMOVE_PIERCING`, and the double click of a bag item while it is open). The shells are `ui.upgrade.openPiercing` /
// `openRemovePiercing` (ui-upgrade-a); this file builds their requests over the inventory and sends `PiercingSize{item, moonstone, protection}` and
// `PiercingRemove{item}` (`CWndPiercing::OnDropIcon`, `CPiercingMessageBox`, `CWndRemovePiercing`, `src:_Interface/WndPiercing.cpp`, `WndField.cpp`).

import type { ClientContext } from '../../app/context';
import {
  II_GEN_MAT_MOONSTONE,
  II_GEN_MAT_MOONSTONE_1,
  II_SYS_SYS_SCR_PIEPROT,
} from '../../gen/defs/item';
import { IK3_PET, IK3_SUIT, PET_VIS } from '../../gen/defs/itemkind';
import { MMI_PIERCING, MMI_SMELT_REMOVE_PIERCING } from '../../gen/defs/neuz';
import { XI_INT_INCHANT } from '../../gen/defs/obj';
import {
  APP_BEAUTY_SHOP_EX,
  APP_BEAUTY_SHOP_SKIN,
  APP_COMMERCIAL_ELEM,
  APP_COMMON_BANK,
  APP_CONFIRM_BANK,
  APP_BANK_PASSWORD,
  APP_DROP_ITEM,
  APP_GUILD_BANK,
  APP_INVENTORY,
  APP_PIERCING,
  APP_REMOVE_ELEM,
  APP_SHOPEX,
  APP_SMELT_REMOVE_PIERCING_EX,
  APP_TEST,
  APP_TRADE,
} from '../../gen/defs/resdata';
import {
  TID_GAME_REMOVE_PIERCING_ERROR_EX,
  TID_GAME_TOOLTIP_PIERCINGITEM,
  TID_GAME_WND_OVERLAPED,
  TID_PIERCING_ERROR_NOTICE,
  TID_PIERCING_POSSIBLE,
  TID_PIERCING_POSSIBLE_ITEM,
  TID_SBEVE_NOTUSEITEM,
} from '../../gen/defs/text';
import { peekFxRuntime } from '../../game/fx/runtime';
import { MB, MBID } from '../../ui/msgbox/types';
import type { PiercingRequest, RemovePiercingRequest } from '../../ui/windows/upgrade/ports';
import { type FeatureContext, fullUi } from '../common/module';
import type { InvItem } from '../inventory/items';
import type { ClientItem } from '../../data/clientData';
import type { InventoryFeature } from '../inventory/window/inventoryModule';
import {
  isPierceAble,
  MAX_PIERCING_SUIT,
  MAX_PIERCING_WEAPON,
} from '../inventory/dispatch/upgradeRules';
import { dstPlusLine } from '../tooltip/dst';
import { NO_ITEM, type UltimateSenders } from './senders';

/** `100000` penya: the price of one more socket (`__EXT_PIERCING`). */
export const PIERCING_SIZE_COST = 100_000;

/** The windows `MMI_PIERCING` waits for (`GetWndBase( ... )` -> `break`): none of them may be open. */
const PIERCING_BLOCKERS: readonly number[] = [
  APP_TRADE,
  APP_SHOPEX,
  APP_COMMON_BANK,
  APP_CONFIRM_BANK,
  APP_BANK_PASSWORD,
  APP_DROP_ITEM,
  APP_GUILD_BANK,
  APP_BEAUTY_SHOP_SKIN,
  APP_BEAUTY_SHOP_EX,
  APP_TEST,
  APP_COMMERCIAL_ELEM,
  APP_REMOVE_ELEM,
  APP_PIERCING,
];

/** `IsPierceAble( NULL_ID, TRUE )` of the gear: could the item take one more socket? A vis pet never grows by this window (PL-070). */
export function canGrowSocket(target: InvItem, prop: ClientItem, noPiercing: boolean): boolean {
  const visPet =
    (prop.item_kind3 === IK3_PET && prop.refer_stat1 === PET_VIS) || target.transformVisPet;
  if (visPet) return false;
  // the size test of the original counts the socket to be added
  const grown: InvItem = { ...target, cards: [...target.cards, 0] };
  return isPierceAble(grown, prop, 0xffff_ffff, noPiercing);
}

/** The sockets an item may use: a suit 4, anything else 10. */
export const socketLimit = (prop: ClientItem): number =>
  prop.item_kind3 === IK3_SUIT ? MAX_PIERCING_SUIT : MAX_PIERCING_WEAPON;

export interface PiercingDeps {
  readonly fc: FeatureContext;
  readonly ctx: ClientContext;
  readonly inv: InventoryFeature;
  readonly senders: UltimateSenders;
  readonly text: (tid: number) => string;
  readonly say: (tid: number) => void;
  readonly count: (name: string) => void;
}

/** The text of a card (`CWndRemovePiercing::OnDraw`): `"Attack+5  HP+10  "`, `''` for an empty socket or an unknown card. */
export function cardText(
  params: readonly (readonly [number, number])[],
  text: (tid: number) => string,
): string {
  // the helper prints a leading new line for the tooltip; the window prints the pieces on one line
  return params.map(([dst, adj]) => `${dstPlusLine(dst, adj, text).slice(1)}  `).join('');
}

export function registerPiercing(d: PiercingDeps): {
  openPiercing(): boolean;
  openRemove(): boolean;
} {
  const { ctx, inv, senders } = d;
  const noPiercing = (): boolean => ctx.features.features.no_piercing;
  const prop = (it: InvItem): ClientItem | undefined => ctx.data?.item(it.itemId);

  /** A bag item that is not worn, as the shells' `DialogItem`. */
  const bagView = (objId: number) => {
    const it = inv.item(objId);
    if (!it || inv.state().container.isEquip(objId)) return null;
    return inv.itemView(it);
  };
  const reserve = (objId: number, on: boolean): void => {
    const it = inv.item(objId);
    if (it) inv.setExtra(objId, on ? it.extra + 1 : 0);
  };

  // ---- APP_PIERCING --------------------------------------------------------------------------------------------------------------

  const piercingRequest = (): PiercingRequest => ({
    item: bagView,
    reserve,
    accept: (slot, objId) => {
      const it = inv.item(objId);
      if (!it) return false;
      const p = prop(it);
      if (slot === 0) {
        if (!p || !canGrowSocket(it, p, noPiercing())) {
          d.say(TID_PIERCING_POSSIBLE);
          return false;
        }
        // `dwItemRare == 0xffffffff`
        if (p.item_rare === 0xffff_ffff) {
          d.say(TID_PIERCING_POSSIBLE_ITEM);
          return false;
        }
        return true;
      }
      if (slot === 1) {
        if (it.itemId === II_GEN_MAT_MOONSTONE || it.itemId === II_GEN_MAT_MOONSTONE_1) return true;
        d.say(TID_SBEVE_NOTUSEITEM);
        return false;
      }
      if (it.itemId === II_SYS_SYS_SCR_PIEPROT) return true;
      d.say(TID_SBEVE_NOTUSEITEM);
      return false;
    },
    cost: (target) => (target ? PIERCING_SIZE_COST : 0),
    ask: (yes) => {
      fullUi(ctx)?.msgbox.open(d.text(TID_PIERCING_ERROR_NOTICE), MB.OKCANCEL, undefined, (id) => {
        if (id === MBID.OK) yes();
      });
    },
    send: (target, moonstone, protection) => {
      senders.piercingSize(target, moonstone, protection ?? NO_ITEM);
      d.count('piercingSize');
    },
    // `CreateSfx( XI_INT_INCHANT )` on the player while the window is open
    sfx: () => {
      const fx = peekFxRuntime(ctx)?.fx();
      const src = ctx.entities.self()?.objId;
      if (!fx || src === undefined) return null;
      const handle = fx.sfx(XI_INT_INCHANT, { src });
      return () => {
        fx.removeSfx(handle);
      };
    },
  });

  const openPiercing = (): boolean => {
    const ui = fullUi(ctx);
    if (!ui) return false;
    if (PIERCING_BLOCKERS.some((app) => ui.wm.isOpen(app))) return false;
    return ui.upgrade.openPiercing(piercingRequest()) !== null;
  };

  // ---- APP_SMELT_REMOVE_PIERCING_EX ----------------------------------------------------------------------------------------------

  const removeRequest = (): RemovePiercingRequest => ({
    description: ctx.data?.bundle('help')?.panels['smeltremovepiercing'] ?? '',
    extended: true,
    slotTip: d.text(TID_GAME_TOOLTIP_PIERCINGITEM),
    item: bagView,
    reserve,
    accept: (objId) => {
      const it = inv.item(objId);
      const p = it ? prop(it) : undefined;
      // `IsPierceAble() && GetPiercingItem( 0 ) != 0`: a pierced item
      if (it && p && isPierceAble(it, p, 0xffff_ffff, noPiercing()) && (it.cards[0] ?? 0) !== 0) {
        return true;
      }
      d.say(TID_GAME_REMOVE_PIERCING_ERROR_EX);
      return false;
    },
    view: (objId) => {
      const it = inv.item(objId);
      const p = it ? prop(it) : undefined;
      const defs = ctx.data?.bundle('item-ext')?.piercing ?? [];
      const lines = (it?.cards ?? []).map((card) => {
        const def = defs.find((x) => x.item === card);
        return def ? cardText(def.params, d.text) : '';
      });
      return { max: p ? socketLimit(p) : MAX_PIERCING_WEAPON, lines };
    },
    send: (objId) => {
      senders.piercingRemove(objId);
      d.count('piercingRemove');
    },
  });

  const openRemove = (): boolean => {
    const ui = fullUi(ctx);
    if (!ui) return false;
    if (ui.wm.isOpen(APP_SMELT_REMOVE_PIERCING_EX)) {
      d.say(TID_GAME_WND_OVERLAPED);
      return false;
    }
    // the bag opens next to the removal window like the other windows of this feature
    if (!ui.wm.isOpen(APP_INVENTORY)) ui.wm.openApplet(APP_INVENTORY);
    return ui.upgrade.openRemovePiercing(removeRequest()) !== null;
  };

  return { openPiercing, openRemove };
}

/** `MMI_PIERCING` and `MMI_SMELT_REMOVE_PIERCING`, for the menu registration. */
export const PIERCING_MENU = {
  piercing: MMI_PIERCING,
  removePiercing: MMI_SMELT_REMOVE_PIERCING,
} as const;
