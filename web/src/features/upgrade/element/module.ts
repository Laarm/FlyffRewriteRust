// The element windows on the client (Plan/45 §18.3; WP-CFEAT-25): `APP_REMOVE_ATTRIBUTE` (`MMI_ATTRIBUTE`, 100 000 penya, a confirm, `RemoveAttributeReq{obj_id}`,
// `RemoveAttributeNtf{ok}`) and `APP_COMMERCIAL_ELEM` (`MMI_CHANGEELEM`, `CommercialElemReq{target, stone}`, `CommercialElemNtf{obj_id, stone}`: 9999 clears the element,
// else the stone id is stored for the tooltip). The windows reserve their items and release them when they close; the confirm of the removal blocks a second
// request while one is pending. The change-element scroll (`APP_CHANGE_ATTRIBUTE`) and the remove-element confirm (`APP_REMOVE_ELEM`) are the `itemuse` feature's.

import {
  APP_BANK_PASSWORD,
  APP_BEAUTY_SHOP_EX,
  APP_BEAUTY_SHOP_SKIN,
  APP_COMMERCIAL_ELEM,
  APP_COMMON_BANK,
  APP_CONFIRM_BANK,
  APP_DROP_ITEM,
  APP_GUILD_BANK,
  APP_REMOVE_ATTRIBUTE,
  APP_REMOVE_ELEM,
  APP_SHOPEX,
  APP_TEST,
  APP_TRADE,
} from '../../../gen/defs/resdata';
import { MMI_ATTRIBUTE, MMI_CHANGEELEM } from '../../../gen/defs/neuz';
import {
  IK2_ARMOR,
  IK2_ARMORETC,
  IK2_WEAPON_DIRECT,
  IK2_WEAPON_MAGIC,
} from '../../../gen/defs/itemkind';
import {
  TID_GAME_ALREADYELEM,
  TID_GAME_EQUIPPUT,
  TID_GAME_NOTELEMENT,
  TID_GAME_NOTEQUALELEM,
  TID_GAME_NOTEQUALITEM,
  TID_GAME_REMOVE_ATTRIBUTE,
  TID_GAME_REMOVE_ATTRIBUTE_CONFIRM,
  TID_GAME_REMOVE_ERROR,
  TID_GAME_REMOVEELEMENT,
  TID_UPGRADE_ERROR_NOUNICK,
} from '../../../gen/defs/text';
import type {
  CommercialElemRequest,
  RemoveAttributeRequest,
} from '../../../ui/windows/upgrade/ports';
import type { MenuFeature } from '../../npc/menuModule';
import { formatTid } from '../../common/text';
import { isEleRefineryAble } from '../../inventory/dispatch/upgradeRules';
import type { UpgradeCommon } from '../common';
import { freeOrSay } from '../common';
import type { UpgradeSenders } from '../senders';
import { stoneFits } from './stones';

/** `m_nResistSMItemId == 9999` of `OnCommercialElem`: the element was removed. */
export const ELEMENT_REMOVED = 9999;
/** The windows that make `MMI_CHANGEELEM` do nothing (the original's `GetWndBase` chain). */
const COMMERCIAL_BUSY: readonly number[] = [
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
];

export interface ElementFeature {
  /** A removal was sent and `RemoveAttributeNtf` has not arrived. */
  removalPending(): boolean;
  /** What was sent and received (the probe). */
  stats(): { removals: number; stones: number; removed: number };
}

export function registerElement(
  common: UpgradeCommon,
  menu: MenuFeature,
  senders: UpgradeSenders,
): ElementFeature {
  const { ctx, inv, fc } = common;
  let pending = false;
  const stats = { removals: 0, stones: 0, removed: 0 };

  // ---- the removal ----------------------------------------------------------------------------------------------------------------

  const removeRequest = (): RemoveAttributeRequest => ({
    description: common.panel('removeattribute'),
    confirmText: common.text(TID_GAME_REMOVE_ATTRIBUTE),
    item: (objId) => common.dialogItem(objId),
    reserve: (objId, on) => {
      common.reserve(objId, on);
    },
    accept: (objId) => {
      const it = inv.item(objId);
      if (!it) return false;
      if (inv.state().container.isEquip(objId)) {
        common.say(TID_GAME_EQUIPPUT);
        return false;
      }
      // `m_nResistAbilityOption > 0` first (else the message box), then `IsEleRefineryAble` (else silent)
      if (it.resistAbilityOption <= 0) {
        common.msgbox(common.text(TID_GAME_REMOVE_ERROR));
        return false;
      }
      const prop = ctx.data?.item(it.itemId);
      return prop !== undefined && isEleRefineryAble(prop);
    },
    pending: () => pending,
    send: (objId) => {
      pending = true;
      stats.removals += 1;
      senders.removeAttribute(objId);
    },
    closed: () => {
      pending = false;
    },
  });

  menu.registerAction(MMI_ATTRIBUTE, () => {
    const ui = common.ui();
    if (!ui || !freeOrSay(common, APP_REMOVE_ATTRIBUTE)) return;
    pending = false;
    ui.upgrade.openRemoveAttribute(removeRequest());
  });

  inv.registerFeed(APP_REMOVE_ATTRIBUTE, (item) => {
    common.ui()?.upgrade.removeAttribute()?.setItem(item.objId);
    return true;
  });

  fc.net.on('removeAttributeNtf', (m) => {
    pending = false;
    const win = common.ui()?.upgrade.removeAttribute();
    // `ReceiveResult`: only an open window reacts
    if (!win) return;
    if (m.success) common.msgbox(common.text(TID_GAME_REMOVE_ATTRIBUTE_CONFIRM));
    win.receiveResult();
  });

  // ---- the commercial stones -------------------------------------------------------------------------------------------------------

  const commercialRequest = (): CommercialElemRequest => ({
    item: (objId) => common.dialogItem(objId),
    reserve: (objId, on) => {
      common.reserve(objId, on);
    },
    acceptItem: (objId) => {
      const it = inv.item(objId);
      if (!it) return false;
      const prop = ctx.data?.item(it.itemId);
      if (!prop) return false;
      // `IsRestrictionItem`: worn items and unique items (`nLog >= 2`)
      if (inv.state().container.isEquip(objId)) {
        common.say(TID_GAME_EQUIPPUT);
        return false;
      }
      if (prop.log >= 2) {
        common.msgbox(common.text(TID_UPGRADE_ERROR_NOUNICK));
        return false;
      }
      // `IsUpgradeItem`: weapons and suits only, with an element, without a stone
      const weapon = prop.item_kind2 === IK2_WEAPON_MAGIC || prop.item_kind2 === IK2_WEAPON_DIRECT;
      const suit =
        (prop.item_kind2 === IK2_ARMOR || prop.item_kind2 === IK2_ARMORETC) &&
        prop.item_kind3 === 18;
      if (!weapon && !suit) {
        common.sayRaw(
          formatTid(fc.texts, TID_GAME_NOTEQUALELEM, [prop.name]),
          fc.texts.textColor(TID_GAME_NOTEQUALELEM),
        );
        return false;
      }
      if (it.itemResist <= 0 || it.itemResist >= 6) {
        common.say(TID_GAME_NOTELEMENT);
        return false;
      }
      if (it.resistSmItemId !== 0) {
        common.say(TID_GAME_ALREADYELEM);
        return false;
      }
      return true;
    },
    acceptStone: (targetObjId, objId) => {
      const target = inv.item(targetObjId);
      const it = inv.item(objId);
      if (!target || !it) return false;
      if (inv.state().container.isEquip(objId)) {
        common.say(TID_GAME_EQUIPPUT);
        return false;
      }
      const prop = ctx.data?.item(it.itemId);
      const targetProp = ctx.data?.item(target.itemId);
      if (!prop || !targetProp) return false;
      const v = stoneFits(it.itemId, prop.item_kind2, {
        kind2: targetProp.item_kind2,
        element: target.itemResist,
        elementLevel: target.resistAbilityOption,
      });
      if (v.ok) return true;
      switch (v.say) {
        case 'notElement':
          common.say(TID_GAME_NOTELEMENT);
          break;
        case 'notEqualItem':
          common.say(TID_GAME_NOTEQUALITEM);
          break;
        case 'notEqualElem':
          common.sayRaw(
            formatTid(fc.texts, TID_GAME_NOTEQUALELEM, [prop.name]),
            fc.texts.textColor(TID_GAME_NOTEQUALELEM),
          );
          break;
        case 'none':
          break;
      }
      return false;
    },
    send: (targetObjId, stoneObjId) => {
      stats.stones += 1;
      senders.commercialElem(targetObjId, stoneObjId);
    },
  });

  menu.registerAction(MMI_CHANGEELEM, () => {
    const ui = common.ui();
    if (!ui) return;
    // the original leaves silently when one of these windows is open
    if (COMMERCIAL_BUSY.some((app) => ui.wm.isOpen(app))) return;
    ui.upgrade.openCommercialElem(commercialRequest());
  });

  // `OnCommercialElem`: 9999 takes the element off (and prints the text), any other value is the stone now applied
  fc.net.on('commercialElemNtf', (m) => {
    if (!inv.item(m.itemObjId)) return;
    if (m.resistSmItemId === ELEMENT_REMOVED) {
      stats.removed += 1;
      common.patchItem(m.itemObjId, { itemResist: 0, resistAbilityOption: 0 });
      common.say(TID_GAME_REMOVEELEMENT);
      return;
    }
    common.patchItem(m.itemObjId, { resistSmItemId: m.resistSmItemId });
  });

  return { removalPending: () => pending, stats: () => ({ ...stats }) };
}
