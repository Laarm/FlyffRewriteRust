// The dispatch of the inventory (Plan/45 §9; WP-CFEAT-13, -14): the double-click dispatch D1-D8, the upgrade cursor and the item-use
// router R0-R31 behind one facade, `ItemDispatch`, which `features/inventory` exposes as `use` (the old `ItemUse`).
//
//   double click on a bag item        `doubleClick(item)`        D1-D8, then the router
//   taskbar ITEM shortcut (F-keys)    `fromShortcut(objId)`      D2, D5, D8, the router (the other steps are skipped, KEEP)
//   equip / unequip                   `equip(item, part)`        the equip-bind confirm, `SendDoEquip`
//   press on a bag item               `pressBagItem(item)`       the D1 guards of `CWndItemCtrl::OnLButtonDown`, the cursor click
//   every tick                        `process(now)`             the due upgrade request

import { IK1_RIDE } from '../../../gen/defs/itemkind';
import {
  APP_COMMITEM_DIALOG,
  APP_EQUIP_BIND_CONFIRM,
  APP_SMELT_SAFETY_CONFIRM,
} from '../../../gen/defs/resdata';
import {
  TID_MMI_NOTUPGRADE,
  TID_TOOLTIP_EQUIPBIND_ERROR01,
  TID_TOOLTIP_ITEM_USING_ERROR,
} from '../../../gen/defs/text';
import { IP_FLAG, isReserved, NULL_ID } from '../info';
import { ITEM_FLAG, type InvItem } from '../items';
import type { DispatchEnv } from './env';
import { finishUse, onItemDoubleClick, type DoubleClickDeps } from './doubleClick';
import { ItemRouter, type Decision } from './router';
import { UpgradeCursor } from './upgradeCursor';

export type { DispatchEnv, DispatchSenders, DispatchWindows, SelfFacts, UpgradeRules } from './env';
export { onItemDoubleClick, finishUse } from './doubleClick';
export { ItemRouter, type Decision } from './router';
export {
  UpgradeCursor,
  isCursorMaterial,
  isElementalCard,
  isNeedTarget,
  type UpgradeCursorState,
} from './upgradeCursor';
export {
  createUpgradeRules,
  isDiceRefineryAble,
  isEleRefineryAble,
  isPierceAble,
  MAX_PIERCING_SUIT,
  MAX_PIERCING_WEAPON,
} from './upgradeRules';

export class ItemDispatch {
  readonly cursor: UpgradeCursor;
  readonly router: ItemRouter;
  /** The last decision of the dispatch or the router (the probe's `router.lastDecision`: step and outcome). */
  last: Decision | null = null;
  readonly #env: DispatchEnv;
  readonly #deps: DoubleClickDeps;

  constructor(env: DispatchEnv) {
    this.#env = env;
    this.cursor = new UpgradeCursor(env);
    this.router = new ItemRouter(env);
    this.router.onDecision = (d) => {
      this.last = d;
    };
    this.#deps = {
      env,
      cursor: this.cursor,
      router: this.router,
      decide: (step, outcome) => {
        this.last = { step, outcome };
      },
    };
  }

  /** `WIN_DBLCLK` of the bag grid. */
  doubleClick(item: InvItem): void {
    onItemDoubleClick(this.#deps, item);
  }

  /** `items.useFromShortcut(objId)` (`WndManager.cpp:2803-2860`). */
  fromShortcut(objId: number): void {
    const e = this.#env;
    if (e.ownVendorOpen()) return;
    const item = e.state().container.getAtId(objId);
    if (!item || isReserved(item)) return;
    const prop = e.prop(item.itemId);
    if (!prop) return;
    const target = e.focusId() ?? NULL_ID;
    if (prop.item_kind1 === IK1_RIDE) {
      const s = e.self();
      if (!s.flying && !s.onGround) return;
    }
    finishUse(this.#deps, item, prop, target);
  }

  /** The router: `SendDoUseItem(MAKELONG(ITYPE_ITEM, objId), target, part, userAction)`. */
  useItem(item: InvItem, target: number, part: number, userAction: boolean): void {
    this.router.useItem(item, target, part, userAction);
  }

  /**
   * Wearing (`part = -1`) or taking off an item: an unbound `IP_FLAG_EQUIP_BIND` item asks first (`EQUIP_DRAG_AND_DROP`), then
   * `SendDoEquip`.
   */
  equip(item: InvItem, part = -1): void {
    const e = this.#env;
    const prop = e.prop(item.itemId);
    if (
      part === -1 &&
      prop &&
      (prop.flag & IP_FLAG.EQUIP_BIND) !== 0 &&
      (item.flag & ITEM_FLAG.BINDS) === 0
    ) {
      e.windows.equipBindConfirm(() => {
        const fresh = e.state().container.getAtId(item.objId);
        if (fresh) this.router.equip(fresh, -1);
      });
      this.last = { step: 'EQUIP', outcome: 'equip bind' };
      return;
    }
    this.router.equip(item, part);
  }

  /**
   * A press on a bag item (`CWndItemCtrl::OnLButtonDown` of the inventory): the D1 guards refuse and select nothing; otherwise an
   * armed upgrade cursor completes with this item. Returns whether the press may select / drag the item.
   */
  pressBagItem(item: InvItem): boolean {
    const e = this.#env;
    if (this.cursor.waiting || e.isOpen(APP_SMELT_SAFETY_CONFIRM)) {
      e.say(TID_MMI_NOTUPGRADE);
      return false;
    }
    if (e.isOpen(APP_EQUIP_BIND_CONFIRM)) {
      e.say(TID_TOOLTIP_EQUIPBIND_ERROR01);
      return false;
    }
    if (e.isOpen(APP_COMMITEM_DIALOG)) {
      e.say(TID_TOOLTIP_ITEM_USING_ERROR);
      return false;
    }
    if (this.cursor.armed) this.cursor.press(item);
    return true;
  }

  /** Every UI tick: the due request of the upgrade cursor. */
  process(now: number): void {
    this.cursor.process(now);
  }
}
