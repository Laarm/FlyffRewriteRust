// The double-click dispatch of the inventory (Plan/45 §9.2, WP-CFEAT-13; `CWndInventory::OnChildNotify`, `WIN_DBLCLK`, nID 11,
// `src:_Interface/WndField.cpp:2201-2785`). The steps run in the original's order; "refuse" prints a system line and sends nothing:
//
//   D1 refuse while an upgrade is pending (smelt-safety confirm open) / the equip-bind confirm / the generic use confirm
//   D2 the target is the focus object's id, NULL_ID for none
//   D3 an open window takes the item (`SetItem` of the window) and the dispatch stops
//   D4 a card, dice or scroll that needs a target arms the upgrade cursor (`bAble = FALSE`, nothing is sent)
//   D5 a ride that is neither flying nor on the ground is refused (`bAble = FALSE`)
//   D6 item-specific windows (maps, element change, skill reset, restat, random-option retry, blessing, class change; `bAble = FALSE`)
//   D7 (only with `bAble`) the shop / bank / trade / angel / mini-game / smelt windows and the quiz world refuse
//   D8 the equip-bind confirm, else `CheckConfirm` (vis, pickup-to-buff, awake-safe confirms), else the router
//
// The taskbar shortcut path (`fromShortcut`) skips D1, D3, D4, D6 and D7 (KEEP, 45 §9 preface).

import {
  II_CHR_SYS_SCR_RESTATE,
  II_CHR_SYS_SCR_RESTATE_DEX,
  II_CHR_SYS_SCR_RESTATE_DEX_LOW,
  II_CHR_SYS_SCR_RESTATE_INT,
  II_CHR_SYS_SCR_RESTATE_INT_LOW,
  II_CHR_SYS_SCR_RESTATE_STA,
  II_CHR_SYS_SCR_RESTATE_STA_LOW,
  II_CHR_SYS_SCR_RESTATE_STR,
  II_CHR_SYS_SCR_RESTATE_STR_LOW,
  II_GEN_MAT_DIAMOND,
  II_GEN_MAT_EMERALD,
  II_GEN_MAT_MOONSTONE,
  II_GEN_MAT_MOONSTONE_1,
  II_GEN_MAT_ORICHALCUM01,
  II_GEN_MAT_ORICHALCUM01_1,
  II_GEN_MAT_ORICHALCUM02,
  II_GEN_MAT_RUBY,
  II_GEN_MAT_SAPPHIRE,
  II_GEN_MAT_TOPAZ,
  II_SYS_SYS_EVE_A_CARD,
  II_SYS_SYS_EVE_Z_CARD,
  II_SYS_SYS_MAP_FLARIS,
  II_SYS_SYS_MAP_SAINTMORNING,
  II_SYS_SYS_SCR_AWAKESAFE,
  II_SYS_SYS_SCR_BLESSING,
  II_SYS_SYS_SCR_CHACLA,
  II_SYS_SYS_SCR_RANDOMOPTIONRTRY,
  II_SYS_SYS_SCR_RECCURENCE,
  II_SYS_SYS_SCR_RECCURENCE_LINK,
  II_SYS_SYS_SCR_SOKCHANG,
  II_SYS_SYS_VIS_KEY01,
  II_SYS_SYS_SCR_PET_MAGIC,
} from '../../../gen/defs/item';
import {
  IK1_RIDE,
  IK1_WEAPON,
  IK2_MATERIAL,
  IK3_EGG,
  WEAPON_ULTIMATE,
} from '../../../gen/defs/itemkind';
import { IK3_VIS } from '../../../gen/defs/itemkind';
import { PARTS_HAIR, PARTS_HEAD, PARTS_RIDE } from '../../../gen/defs/neuz';
import {
  APP_COMMITEM_DIALOG,
  APP_COMMON_BANK,
  APP_EQUIP_BIND_CONFIRM,
  APP_HERO_SKILLUP,
  APP_MINIGAME_DICE,
  APP_MINIGAME_KAWIBAWIBO,
  APP_MINIGAME_KAWIBAWIBO_WIN,
  APP_MINIGAME_PUZZLE,
  APP_MINIGAME_WORD,
  APP_PET_FOODMILL,
  APP_PET_TRANS_EGGS,
  APP_REMOVE_ATTRIBUTE,
  APP_SHOPEX,
  APP_SMELT_CHANGEWEAPON,
  APP_SMELT_EXTRACTION,
  APP_SMELT_JEWEL,
  APP_SMELT_MIXJEWEL,
  APP_SMELT_REMOVE_JEWEL,
  APP_SMELT_REMOVE_PIERCING_EX,
  APP_SMELT_SAFETY,
  APP_SMELT_SAFETY_CONFIRM,
  APP_SUMMON_ANGEL,
  APP_TRADE,
} from '../../../gen/defs/resdata';
import {
  TID_GAME_DEPEN_USE,
  TID_GAME_ERROR_SKILLRECCURENCE,
  TID_GAME_LIMITED_USE,
  TID_GAME_NOTUSEVAG,
  TID_GAME_SKILLINIT,
  TID_GAME_TRADELIMITUSING,
  TID_GAME_TRANS_EGGS_ERROR1,
  TID_GAME_TRANS_EGGS_ERROR2,
  TID_GAME_WARNINGCCLS,
  TID_GAME_CHECK_EQUIP,
  TID_MMI_NOTUPGRADE,
  TID_SBEVE_NOTUSEITEM,
  TID_TOOLTIP_EQUIPBIND_ERROR01,
  TID_TOOLTIP_ITEM_USING_ERROR,
  TID_TOOLTIP_RESTATE_ALL_CONFIRM,
  TID_TOOLTIP_RESTATE_DEX_CONFIRM,
  TID_TOOLTIP_RESTATE_INT_CONFIRM,
  TID_TOOLTIP_RESTATE_STA_CONFIRM,
  TID_TOOLTIP_RESTATE_STR_CONFIRM,
  TID_GAME_RESTATE_DEX_LOW_CONFIRM,
  TID_GAME_RESTATE_INT_LOW_CONFIRM,
  TID_GAME_RESTATE_STA_LOW_CONFIRM,
  TID_GAME_RESTATE_STR_LOW_CONFIRM,
} from '../../../gen/defs/text';
import { MAX_HUMAN_PARTS } from '../../../gen/defs/neuz';
import type { ClientItem } from '../../../data/clientData';
import { IP_FLAG, partsOf } from '../info';
import { ITEM_FLAG, type InvItem } from '../items';
import { WI_WORLD_QUIZ } from '../../../gen/defs/world';
import type { DispatchEnv } from './env';
import type { ItemRouter } from './router';
import { isCursorMaterial, type UpgradeCursor } from './upgradeCursor';

const NULL_ID = 0xffff_ffff;

/** The five element gems the jewel windows take. */
const FIVE_JEWELS: readonly number[] = [
  II_GEN_MAT_DIAMOND,
  II_GEN_MAT_EMERALD,
  II_GEN_MAT_SAPPHIRE,
  II_GEN_MAT_RUBY,
  II_GEN_MAT_TOPAZ,
];
const ORICHALCUM_OR_MOONSTONE: readonly number[] = [
  II_GEN_MAT_ORICHALCUM01,
  II_GEN_MAT_MOONSTONE,
  II_GEN_MAT_ORICHALCUM01_1,
  II_GEN_MAT_MOONSTONE_1,
];

/** The restat scrolls and the text of their confirm. */
export const RESTATE_TID: ReadonlyMap<number, number> = new Map([
  [II_CHR_SYS_SCR_RESTATE, TID_TOOLTIP_RESTATE_ALL_CONFIRM],
  [II_CHR_SYS_SCR_RESTATE_STR, TID_TOOLTIP_RESTATE_STR_CONFIRM],
  [II_CHR_SYS_SCR_RESTATE_STA, TID_TOOLTIP_RESTATE_STA_CONFIRM],
  [II_CHR_SYS_SCR_RESTATE_DEX, TID_TOOLTIP_RESTATE_DEX_CONFIRM],
  [II_CHR_SYS_SCR_RESTATE_INT, TID_TOOLTIP_RESTATE_INT_CONFIRM],
  [II_CHR_SYS_SCR_RESTATE_STR_LOW, TID_GAME_RESTATE_STR_LOW_CONFIRM],
  [II_CHR_SYS_SCR_RESTATE_STA_LOW, TID_GAME_RESTATE_STA_LOW_CONFIRM],
  [II_CHR_SYS_SCR_RESTATE_DEX_LOW, TID_GAME_RESTATE_DEX_LOW_CONFIRM],
  [II_CHR_SYS_SCR_RESTATE_INT_LOW, TID_GAME_RESTATE_INT_LOW_CONFIRM],
]);

/** D7 rows: windows that refuse the use. */
const D7_TRADE: readonly number[] = [APP_SHOPEX, APP_COMMON_BANK, APP_TRADE];
const D7_NOTUSE: readonly number[] = [
  APP_MINIGAME_DICE,
  APP_MINIGAME_KAWIBAWIBO,
  APP_MINIGAME_KAWIBAWIBO_WIN,
  APP_MINIGAME_PUZZLE,
  APP_MINIGAME_WORD,
  APP_SMELT_EXTRACTION,
  APP_SMELT_JEWEL,
  APP_SMELT_MIXJEWEL,
  APP_PET_FOODMILL,
  APP_SMELT_SAFETY,
];

export interface DoubleClickDeps {
  readonly env: DispatchEnv;
  readonly cursor: UpgradeCursor;
  readonly router: ItemRouter;
  /** The probe's last decision. */
  decide(step: string, outcome: string): void;
}

/** `extra < count`: the stack is not fully reserved by a trade, vendor or other window. */
const hasFree = (item: InvItem): boolean => item.extra < item.num;
/** `IsUsableItem`: nothing of the stack is reserved. */
const usable = (item: InvItem): boolean => item.extra === 0;

/** D3: the window that takes the item instead of the use. `null` = no window applies. */
function feedTarget(d: DoubleClickDeps, item: InvItem, prop: ClientItem): boolean | null {
  const e = d.env;
  const open = (app: number): boolean => e.isOpen(app);
  const give = (app: number): boolean => e.windows.feed?.(app, item) ?? false;
  const id = item.itemId;
  // `APP_SUMMON_ANGEL`: orichalcum / moonstone
  if (
    open(APP_SUMMON_ANGEL) &&
    ORICHALCUM_OR_MOONSTONE.includes(id) &&
    hasFree(item) &&
    give(APP_SUMMON_ANGEL)
  ) {
    return true;
  }
  if (
    open(APP_MINIGAME_WORD) &&
    id >= II_SYS_SYS_EVE_A_CARD &&
    id <= II_SYS_SYS_EVE_Z_CARD &&
    hasFree(item) &&
    give(APP_MINIGAME_WORD)
  ) {
    return true;
  }
  if (open(APP_MINIGAME_PUZZLE) && hasFree(item) && give(APP_MINIGAME_PUZZLE)) return true;
  if (
    open(APP_REMOVE_ATTRIBUTE) &&
    e.rules.isEleRefineryAble(prop) &&
    hasFree(item) &&
    give(APP_REMOVE_ATTRIBUTE)
  ) {
    return true;
  }
  if (
    open(APP_SMELT_REMOVE_PIERCING_EX) &&
    e.rules.isEleRefineryAble(prop) &&
    usable(item) &&
    give(APP_SMELT_REMOVE_PIERCING_EX)
  ) {
    return true;
  }
  if (
    open(APP_SMELT_REMOVE_JEWEL) &&
    (prop.refer_stat1 === WEAPON_ULTIMATE || prop.item_kind2 === IK2_MATERIAL) &&
    usable(item) &&
    give(APP_SMELT_REMOVE_JEWEL)
  ) {
    return true;
  }
  if (open(APP_PET_TRANS_EGGS)) {
    if ((item.flag & ITEM_FLAG.ISUSING) !== 0) {
      e.say(TID_GAME_TRANS_EGGS_ERROR2);
      return true;
    }
    const egg = prop.item_kind3 === IK3_EGG && (!item.pet || item.pet.level === 0);
    if (egg) {
      if (usable(item) && give(APP_PET_TRANS_EGGS)) return true;
    } else {
      e.say(TID_GAME_TRANS_EGGS_ERROR1);
      return true;
    }
  }
  if (open(APP_SMELT_SAFETY) && hasFree(item) && give(APP_SMELT_SAFETY)) return true;
  if (
    open(APP_SMELT_MIXJEWEL) &&
    ORICHALCUM_OR_MOONSTONE.includes(id) &&
    hasFree(item) &&
    give(APP_SMELT_MIXJEWEL)
  ) {
    return true;
  }
  if (open(APP_SMELT_JEWEL) && FIVE_JEWELS.includes(id) && hasFree(item) && give(APP_SMELT_JEWEL))
    return true;
  if (
    open(APP_SMELT_CHANGEWEAPON) &&
    (prop.item_kind1 === IK1_WEAPON ||
      id === II_GEN_MAT_ORICHALCUM02 ||
      FIVE_JEWELS.includes(id)) &&
    hasFree(item) &&
    give(APP_SMELT_CHANGEWEAPON)
  ) {
    return true;
  }
  if (open(APP_HERO_SKILLUP) && FIVE_JEWELS.includes(id) && hasFree(item) && give(APP_HERO_SKILLUP))
    return true;
  if (
    open(APP_SMELT_EXTRACTION) &&
    prop.item_kind1 === IK1_WEAPON &&
    hasFree(item) &&
    give(APP_SMELT_EXTRACTION)
  ) {
    return true;
  }
  return null;
}

/** `CWndMgr::CheckConfirm(item)` (D8): vis confirms and the awake-safe confirm; `true` = a window took over. */
function checkConfirm(d: DoubleClickDeps, item: InvItem, prop: ClientItem): boolean {
  const e = d.env;
  const activeVisPet = e.windows.hasActivatedVisPet?.() === true;
  if (activeVisPet && prop.item_kind3 === IK3_VIS && e.windows.confirmEnter) {
    e.windows.confirmEnter('vis', item);
    return true;
  }
  if (activeVisPet && prop.id === II_SYS_SYS_VIS_KEY01 && e.windows.confirmEnter) {
    e.windows.confirmEnter('viskey', item);
    return true;
  }
  if (prop.id === II_SYS_SYS_SCR_PET_MAGIC && e.windows.confirmEnter) {
    e.windows.confirmEnter('pickupToBuff', item);
    return true;
  }
  if (item.itemId === II_SYS_SYS_SCR_AWAKESAFE && e.windows.confirmEnter) {
    e.windows.confirmEnter('protectAwake', item);
    return true;
  }
  return false;
}

/** D8 for an inventory double click and a shortcut. */
export function finishUse(
  d: DoubleClickDeps,
  item: InvItem,
  prop: ClientItem,
  target: number,
): void {
  const e = d.env;
  // an item with `IP_FLAG_EQUIP_BIND` that is not bound yet asks first
  if ((prop.flag & IP_FLAG.EQUIP_BIND) !== 0 && (item.flag & ITEM_FLAG.BINDS) === 0) {
    e.windows.equipBindConfirm(() => {
      const fresh = e.state().container.getAtId(item.objId);
      if (fresh) applyWithPart(d, fresh, prop, target);
    });
    d.decide('D8', 'equip bind');
    return;
  }
  applyWithPart(d, item, prop, target);
}

/** `nPart = prop.dwParts`, or the equipment index of a worn item; `CheckConfirm`, then the router. */
function applyWithPart(d: DoubleClickDeps, item: InvItem, prop: ClientItem, target: number): void {
  const e = d.env;
  let part = partsOf(prop);
  const worn = e.state().container.partOf(item.objId);
  if (worn !== null) part = worn;
  if (checkConfirm(d, item, prop)) {
    d.decide('D8', 'confirm');
    return;
  }
  d.router.useItem(item, target, part, true);
}

/**
 * `OnChildNotify(WIN_DBLCLK)`: the dispatch D1-D8 for a bag item.
 */
export function onItemDoubleClick(d: DoubleClickDeps, item: InvItem): void {
  const e = d.env;
  // D1
  if (d.cursor.waiting || e.isOpen(APP_SMELT_SAFETY_CONFIRM)) {
    e.say(TID_MMI_NOTUPGRADE);
    d.decide('D1', 'refused');
    return;
  }
  if (e.isOpen(APP_EQUIP_BIND_CONFIRM)) {
    e.say(TID_TOOLTIP_EQUIPBIND_ERROR01);
    d.decide('D1', 'refused');
    return;
  }
  if (e.isOpen(APP_COMMITEM_DIALOG)) {
    e.say(TID_TOOLTIP_ITEM_USING_ERROR);
    d.decide('D1', 'refused');
    return;
  }
  // D2
  const target = e.focusId() ?? NULL_ID;
  const prop = e.prop(item.itemId);
  if (!prop) return;
  // D3
  const fed = feedTarget(d, item, prop);
  if (fed === true) {
    d.decide('D3', 'handled');
    return;
  }
  let able = true;
  // D4
  if (hasFree(item) && isCursorMaterial(prop, e.features().better_safe_upgrades)) {
    if (e.quizWorld()) {
      e.say(TID_SBEVE_NOTUSEITEM);
      d.decide('D4', 'refused');
      return;
    }
    able = false;
    d.cursor.arm(item);
    d.decide('D4', 'cursor');
  }
  // D5: a ride needs the air or the ground
  if (prop.item_kind1 === IK1_RIDE) {
    const s = e.self();
    if (!s.flying && !s.onGround) {
      able = false;
      d.decide('D5', 'refused');
    }
  }
  // D6: item-specific windows
  if (specialWindows(d, item, prop, target)) able = false;
  if (!able) return;
  // D7
  if (D7_TRADE.some((a) => e.isOpen(a))) {
    e.say(TID_GAME_TRADELIMITUSING);
    d.decide('D7', 'refused');
    return;
  }
  if (e.isOpen(APP_SUMMON_ANGEL)) {
    e.say(TID_GAME_TRADELIMITUSING);
    d.decide('D7', 'refused');
    return;
  }
  if (D7_NOTUSE.some((a) => e.isOpen(a)) || e.self().worldId === WI_WORLD_QUIZ) {
    e.say(TID_SBEVE_NOTUSEITEM);
    d.decide('D7', 'refused');
    return;
  }
  // D8
  finishUse(d, item, prop, target);
}

/** D6: returns `true` when one of the item-specific windows took the item (`bAble = FALSE`). */
function specialWindows(
  d: DoubleClickDeps,
  item: InvItem,
  prop: ClientItem,
  target: number,
): boolean {
  const e = d.env;
  const id = item.itemId;
  let taken = false;
  /** A confirm whose Yes uses the item through the router (`userAction`, the original's default `bResult`). */
  const confirmThenUse = (tid: number, part: number, userAction: boolean): void => {
    e.windows.commItemConfirm(tid, () => {
      const fresh = e.state().container.getAtId(item.objId);
      if (fresh) d.router.useItem(fresh, target, part, userAction);
    });
  };
  // the picture maps open their window and the dispatch goes on (`bAble` stays TRUE in the original)
  if (id === II_SYS_SYS_MAP_FLARIS) e.windows.pictureMap('map_flaris.tga');
  if (id === II_SYS_SYS_MAP_SAINTMORNING) e.windows.pictureMap('map_saintmorning.tga');
  if (id === II_SYS_SYS_SCR_SOKCHANG) {
    if (e.windows.changeAttribute) e.windows.changeAttribute(item);
    else e.say(TID_SBEVE_NOTUSEITEM);
    taken = true;
  }
  if (id === II_SYS_SYS_SCR_RECCURENCE || id === II_SYS_SYS_SCR_RECCURENCE_LINK) {
    const s = e.self();
    // the skill-point cap is a rule of M5: unknown, the server decides
    if (s.maxSkillPoints === null || s.skillPoints < s.maxSkillPoints) {
      confirmThenUse(TID_GAME_SKILLINIT, -1, false);
    } else {
      e.say(TID_GAME_ERROR_SKILLRECCURENCE);
    }
    taken = true;
  }
  const restate = RESTATE_TID.get(id);
  if (restate !== undefined) {
    const part = partsOf(prop);
    if (e.windows.restateConfirm) e.windows.restateConfirm(item, target, part);
    else confirmThenUse(restate, part, true);
    taken = true;
  }
  if (id === II_SYS_SYS_SCR_RANDOMOPTIONRTRY && e.features().randomoption_retry) {
    if (e.windows.randomOptionRetry) e.windows.randomOptionRetry(item);
    else e.say(TID_SBEVE_NOTUSEITEM);
    taken = true;
  }
  // `m_bCharged != 1`: the blessing and class-change scrolls of the shop ask nothing here
  if (!item.charged) {
    if (id === II_SYS_SYS_SCR_BLESSING) {
      if (e.self().smRevival) e.say(TID_GAME_LIMITED_USE);
      else confirmThenUse(TID_GAME_DEPEN_USE, -1, false);
      taken = true;
    } else if (id === II_SYS_SYS_SCR_CHACLA) {
      let ok = true;
      if (e.self().vagrant) {
        e.say(TID_GAME_NOTUSEVAG);
        ok = false;
      } else {
        const c = e.state().container;
        for (let p = 0; p < MAX_HUMAN_PARTS; p += 1) {
          if (p === PARTS_HEAD || p === PARTS_HAIR || p === PARTS_RIDE) continue;
          if (c.getEquip(p)) {
            e.say(TID_GAME_CHECK_EQUIP);
            ok = false;
          }
        }
      }
      if (ok) {
        if (e.windows.changeClass) {
          const open = e.windows.changeClass;
          e.windows.commItemConfirm(TID_GAME_WARNINGCCLS, () => {
            open();
          });
        } else {
          // the class-change windows are WP-CFEAT-33's (M5)
          e.say(TID_SBEVE_NOTUSEITEM);
        }
      }
      taken = true;
    }
  }
  return taken;
}
