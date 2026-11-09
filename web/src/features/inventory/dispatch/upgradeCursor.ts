// The upgrade-cursor state machine (Plan/45 §9.3, WP-CFEAT-13; `CWndInventory::RunUpgrade`, `Process`,
// `src:_Interface/WndField.cpp:1743-1838, 13778-14073`, and the press handler of `WndItemCtrl.cpp:790-960`).
//
//   idle --D4 (double click on a card, a dice, a scroll that needs a target)--> armed(material)
//   armed --click on an inventory item--> validation by material --ok--> waiting(material, target, due = now + enchant_time_s)
//   armed --click on nothing / right click / Esc / the material is gone--> idle + TID_UPGRADE_CANCLE
//   waiting --due--> the send by material (Enchant, Piercing, UltimateEnchantWeapon, DoUseItemTarget, RandomScroll confirm) --> idle
//
// While `waiting` every item use is refused with `TID_MMI_NOTUPGRADE` (D1). The upgrade sound effect plays from D4 to the end.

import { EXT_ITEM } from '../../../gen/defs/define';
import {
  IK1_WEAPON,
  IK2_ARMOR,
  IK2_ARMORETC,
  IK2_JEWELRY,
  IK3_ELECARD,
  IK3_ENCHANT,
  IK3_PIERDICE,
  IK3_RANDOM_SCROLL,
  IK3_SMELT_ACCESSORY_MATERIAL,
  IK3_SMELT_GENERAL_MATERIAL,
  IK3_SMELT_ULTIMATE_MATERIAL,
  IK3_SOCKETCARD,
  IK3_SOCKETCARD2,
  WEAPON_ULTIMATE,
} from '../../../gen/defs/itemkind';
import {
  II_GEN_MAT_ELE_CYCLON,
  II_GEN_MAT_ELE_DESERT,
  II_GEN_MAT_ELE_FLAME,
  II_GEN_MAT_ELE_GENERATOR,
  II_GEN_MAT_ELE_RIVER,
  II_GEN_MAT_MOONSTONE,
  II_GEN_MAT_MOONSTONE_1,
  II_GEN_MAT_ORICHALCUM01,
  II_GEN_MAT_ORICHALCUM01_1,
  II_GEN_MAT_ORICHALCUM02,
  II_SYS_SYS_SCR_SMELPROT,
  II_SYS_SYS_SCR_SMELPROT3,
  II_SYS_SYS_SCR_SMELPROT4,
} from '../../../gen/defs/item';
import { APP_SMELT_JEWEL } from '../../../gen/defs/resdata';
import {
  TID_GAME_NOTEQUALITEM,
  TID_GAME_RANDOMSCROLL_ERROR,
  TID_PIERCING_ERROR_NOPIERCING,
  TID_PIERCING_POSSIBLE_ITEM,
  TID_UPGRADE_CANCLE,
  TID_UPGRADE_ERROR_TWOELEMENT,
  TID_UPGRADE_ERROR_WRONGUPLEVEL,
} from '../../../gen/defs/text';
import type { ClientItem } from '../../../data/clientData';
import { BUFF_ITEM } from '../../buffs/types';
import { isCollector } from '../info';
import type { InvItem } from '../items';
import type { DispatchEnv } from './env';

export type UpgradeCursorState =
  | { readonly kind: 'idle' }
  | { readonly kind: 'armed'; readonly material: InvItem }
  | {
      readonly kind: 'waiting';
      readonly material: InvItem;
      readonly target: InvItem;
      readonly dueMs: number;
    };

/** `IsNeedTarget(prop)`: the item is applied to another inventory item (`dwExeTarget == EXT_ITEM`). */
export const isNeedTarget = (prop: ClientItem): boolean => prop.exe_target === EXT_ITEM;

/** The kind3 values of the materials the cursor takes (D4 / `Process`): the safe-upgrade materials only with `better_safe_upgrades`. */
export function isCursorMaterial(prop: ClientItem, betterSafe: boolean): boolean {
  switch (prop.item_kind3) {
    case IK3_ELECARD:
    case IK3_ENCHANT:
    case IK3_SOCKETCARD:
    case IK3_SOCKETCARD2:
    case IK3_RANDOM_SCROLL:
    case IK3_PIERDICE:
      return true;
    case IK3_SMELT_ULTIMATE_MATERIAL:
    case IK3_SMELT_GENERAL_MATERIAL:
    case IK3_SMELT_ACCESSORY_MATERIAL:
      return betterSafe;
    default:
      return isNeedTarget(prop);
  }
}

/** The element card the V12+ rule wants for the element of the card in hand. */
function wantedCard(element: number): number {
  switch (element) {
    case 1:
      return II_GEN_MAT_ELE_FLAME;
    case 2:
      return II_GEN_MAT_ELE_RIVER;
    case 3:
      return II_GEN_MAT_ELE_GENERATOR;
    case 5:
      return II_GEN_MAT_ELE_DESERT;
    case 4:
      return II_GEN_MAT_ELE_CYCLON;
    default:
      return 0;
  }
}

export class UpgradeCursor {
  #state: UpgradeCursorState = { kind: 'idle' };
  #stopSfx: (() => void) | null = null;
  readonly #env: DispatchEnv;
  /** Tells the probe / the UI (`InventoryView.upgradeMode`, the hammer cursor) that the state changed. */
  onChange: (() => void) | null = null;

  constructor(env: DispatchEnv) {
    this.#env = env;
  }

  get state(): UpgradeCursorState {
    return this.#state;
  }

  /** `m_bIsUpgradeMode`: the next press on an inventory item completes the upgrade. */
  get armed(): boolean {
    return this.#state.kind === 'armed';
  }

  /** `m_dwEnchantWaitTime != 0xffffffff`: the request is waiting to be sent. */
  get waiting(): boolean {
    return this.#state.kind === 'waiting';
  }

  #set(s: UpgradeCursorState): void {
    this.#state = s;
    this.onChange?.();
  }

  #stop(): void {
    this.#stopSfx?.();
    this.#stopSfx = null;
  }

  /** D4: `m_bIsUpgradeMode = TRUE`, remember the material, play `XI_INT_INCHANT` on the player unless transparent. */
  arm(material: InvItem): void {
    this.#stop();
    if (!this.#env.self().transparent) this.#stopSfx = this.#env.playUpgradeSfx();
    this.#set({ kind: 'armed', material });
  }

  /** `BaseMouseCursor()`: leaves the mode without a message (also drops a waiting request, as destroying the confirm does). */
  cancel(): void {
    this.#stop();
    if (this.#state.kind !== 'idle') this.#set({ kind: 'idle' });
  }

  /** Cancel with the message of a click on nothing, a right click or Esc. */
  abort(): void {
    if (this.#state.kind !== 'armed') return;
    this.#env.say(TID_UPGRADE_CANCLE);
    this.cancel();
  }

  /** Every tick: an armed cursor whose material left the inventory is cancelled; a waiting request is sent when due. */
  process(now: number): void {
    const s = this.#state;
    if (s.kind === 'armed') {
      if (!this.#env.state().container.getAtId(s.material.objId)) this.abort();
      return;
    }
    if (s.kind !== 'waiting' || !(now > s.dueMs)) return;
    this.#stop();
    this.#set({ kind: 'idle' });
    const c = this.#env.state().container;
    const material = c.getAtId(s.material.objId);
    const target = c.getAtId(s.target.objId);
    if (material && target) this.#send(material, target);
  }

  /** `Process()`: the packet by material once the wait is over. */
  #send(material: InvItem, target: InvItem): void {
    const e = this.#env;
    const prop = e.prop(material.itemId);
    const targetProp = e.prop(target.itemId);
    if (!prop || !targetProp) return;
    const betterSafe = e.features().better_safe_upgrades;
    const k3 = prop.item_kind3;
    const enchantLike =
      k3 === IK3_ELECARD ||
      k3 === IK3_ENCHANT ||
      k3 === IK3_PIERDICE ||
      (betterSafe &&
        (k3 === IK3_SMELT_ULTIMATE_MATERIAL ||
          k3 === IK3_SMELT_GENERAL_MATERIAL ||
          k3 === IK3_SMELT_ACCESSORY_MATERIAL));
    if (enchantLike) {
      if (material.itemId === II_GEN_MAT_ORICHALCUM02) {
        e.send.ultimateEnchantWeapon(target.objId, material.objId);
      } else if (
        material.itemId === II_GEN_MAT_MOONSTONE ||
        material.itemId === II_GEN_MAT_MOONSTONE_1
      ) {
        // since V12 the moonstone works on collectors and jewelry only (the ultimate gem removal has its own window)
        if (isCollector(target.itemId, true) || targetProp.item_kind2 === IK2_JEWELRY) {
          e.send.enchant(target.objId, material.objId);
        }
      } else {
        e.send.enchant(target.objId, material.objId);
      }
    } else if (k3 === IK3_SOCKETCARD || k3 === IK3_SOCKETCARD2) {
      e.send.piercing(target.objId, material.objId);
    } else if (isNeedTarget(prop)) {
      e.send.doUseItemTarget(material.objId, target.objId);
    } else if (k3 === IK3_RANDOM_SCROLL) {
      const had = Number(target.randomOpt & 0xffn) > 0;
      e.windows.randomScrollConfirm(target, had, () => {
        e.send.randomScroll(target.objId, material.objId);
      });
    }
  }

  /**
   * `RunUpgrade(pItem)` (a press on an inventory item while armed): the validation of the material against the target, then the
   * wait. `target = null` (a press on nothing) cancels. Failures print their text and leave the mode.
   */
  click(target: InvItem | null): void {
    const s = this.#state;
    if (s.kind !== 'armed') return;
    const e = this.#env;
    const material = s.material;
    this.#set({ kind: 'idle' });
    if (!target) {
      e.say(TID_UPGRADE_CANCLE);
      this.cancel();
      return;
    }
    const prop = e.prop(material.itemId);
    const tprop = e.prop(target.itemId);
    if (!prop || !tprop) {
      this.cancel();
      return;
    }
    const fail = (tid: number): void => {
      e.say(tid);
      this.cancel();
    };
    const wait = (): void => {
      this.#set({
        kind: 'waiting',
        material,
        target,
        dueMs: e.now() + e.enchantTimeMs(),
      });
    };
    const k3 = prop.item_kind3;
    const betterSafe = e.features().better_safe_upgrades;
    if (k3 === IK3_SOCKETCARD || k3 === IK3_SOCKETCARD2) {
      if (!e.rules.isPierceAble(target, tprop, k3)) {
        fail(TID_PIERCING_POSSIBLE_ITEM);
        return;
      }
      const filled = target.cards.filter((c) => c !== 0).length;
      // no free socket (also when the item has none): the mode ends
      if (filled === target.cards.length) {
        fail(TID_PIERCING_ERROR_NOPIERCING);
        return;
      }
      wait();
      return;
    }
    if (k3 === IK3_ELECARD) {
      if (!e.rules.isEleRefineryAble(tprop)) {
        fail(TID_GAME_NOTEQUALITEM);
        return;
      }
      if (target.itemResist !== 0 && target.itemResist !== prop.item_type) {
        fail(TID_UPGRADE_ERROR_TWOELEMENT);
        return;
      }
      if (prop.id !== wantedCard(prop.item_type)) {
        fail(TID_UPGRADE_ERROR_WRONGUPLEVEL);
        return;
      }
      wait();
      return;
    }
    if (
      k3 === IK3_ENCHANT ||
      (betterSafe && (k3 === IK3_SMELT_ULTIMATE_MATERIAL || k3 === IK3_SMELT_GENERAL_MATERIAL))
    ) {
      if (material.itemId === II_GEN_MAT_ORICHALCUM02) {
        // the weapon of an open jewel-smelting window cannot be enchanted at the same time
        if (e.isOpen(APP_SMELT_JEWEL) && e.windows.smeltJewelItem?.() === target.objId) {
          fail(TID_GAME_NOTEQUALITEM);
          return;
        }
        if (tprop.refer_stat1 !== WEAPON_ULTIMATE) {
          fail(TID_GAME_NOTEQUALITEM);
          return;
        }
      } else if (!e.rules.isDiceRefineryAble(tprop)) {
        fail(TID_GAME_NOTEQUALITEM);
        return;
      }
      wait();
      return;
    }
    if (k3 === IK3_RANDOM_SCROLL) {
      if (!(
        tprop.item_kind1 === IK1_WEAPON ||
        tprop.item_kind2 === IK2_ARMOR ||
        tprop.item_kind2 === IK2_ARMORETC
      )) {
        fail(TID_GAME_RANDOMSCROLL_ERROR);
        return;
      }
      wait();
      return;
    }
    if (isNeedTarget(prop)) {
      wait();
      return;
    }
    if (
      k3 === IK3_PIERDICE &&
      (material.itemId === II_GEN_MAT_MOONSTONE || material.itemId === II_GEN_MAT_MOONSTONE_1)
    ) {
      if (isCollector(target.itemId, true) || tprop.item_kind2 === IK2_JEWELRY) {
        wait();
        return;
      }
      fail(TID_GAME_NOTEQUALITEM);
      return;
    }
    wait();
  }

  /**
   * The press on an inventory item while armed (`CWndItemCtrl::OnLButtonDown`, `__SMELT_SAFETY`): without the matching protection
   * scroll an upgrade of +3 or more asks first (`CWndSmeltSafetyConfirm`); Yes runs `RunUpgrade`, No leaves the cursor mode.
   * Otherwise it is `RunUpgrade(target)` at once.
   */
  press(target: InvItem): void {
    const s = this.#state;
    if (s.kind !== 'armed') return;
    const e = this.#env;
    const material = s.material;
    const prop = e.prop(material.itemId);
    const tprop = e.prop(target.itemId);
    let mode: 1 | 2 | 3 | null = null;
    if (prop && tprop) {
      const id = material.itemId;
      if (
        !e.hasBuff(BUFF_ITEM, II_SYS_SYS_SCR_SMELPROT) &&
        (id === II_GEN_MAT_ORICHALCUM01 || id === II_GEN_MAT_ORICHALCUM01_1) &&
        target.abilityOption >= 3 &&
        e.rules.isDiceRefineryAble(tprop)
      ) {
        mode = 1;
      } else if (
        !e.hasBuff(BUFF_ITEM, II_SYS_SYS_SCR_SMELPROT3) &&
        id === II_GEN_MAT_ORICHALCUM02 &&
        tprop.refer_stat1 === WEAPON_ULTIMATE
      ) {
        mode = 2;
      } else if (
        !e.hasBuff(BUFF_ITEM, II_SYS_SYS_SCR_SMELPROT4) &&
        target.abilityOption >= 3 &&
        (id === II_GEN_MAT_MOONSTONE || id === II_GEN_MAT_MOONSTONE_1) &&
        tprop.item_kind2 === IK2_JEWELRY
      ) {
        mode = 3;
      } else if (
        !e.hasBuff(BUFF_ITEM, II_SYS_SYS_SCR_SMELPROT) &&
        target.resistAbilityOption >= 3 &&
        isElementalCard(id) &&
        e.rules.isEleRefineryAble(tprop)
      ) {
        mode = 1;
      }
    }
    if (mode === null) {
      this.click(target);
      return;
    }
    e.windows.smeltSafetyConfirm(
      mode,
      () => {
        this.click(target);
      },
      () => {
        this.cancel();
      },
    );
  }
}

/** `CItemElem::IsElementalCard(id)`: the five V12+ element cards. */
export function isElementalCard(id: number): boolean {
  return (
    id === II_GEN_MAT_ELE_FLAME ||
    id === II_GEN_MAT_ELE_RIVER ||
    id === II_GEN_MAT_ELE_GENERATOR ||
    id === II_GEN_MAT_ELE_DESERT ||
    id === II_GEN_MAT_ELE_CYCLON
  );
}
