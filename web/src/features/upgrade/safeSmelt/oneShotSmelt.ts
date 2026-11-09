// The one-shot safe-upgrade machine (the vanilla `CWndSmeltSafety`, `src:_Interface/WndField.cpp:24202-25830`, `__SMELT_SAFETY`; Plan/45 §18.2 and PL-169; WP-CFEAT-24):
// the window of the accessory, piercing and element NPC menus (and of the general one when `features.better_safe_upgrades = false`). One of four modes is fixed
// by the menu; the target must suit the mode, the material, the protection scroll and (normal and element mode) the smelting scroll are dropped one unit at a
// time, Start waits `enchant_time_s` and sends ONE `SmeltSafetyReq`. The original has ten slots per kind; the shipped layout of `APP_SMELT_SAFETY` has one
// (`SMELT_SLOTS`), so a try uses the slot up and Reset clears the slots for the next one (the original's "no room left, press Reset", `TID_GAME_SMELT_SAFETY_ERROR15`).
//   * the texts of every refusal are the `TID_GAME_SMELT_SAFETY_ERROR01..20` rows; the piercing mode needs 100 000 penya;
//   * a try gives the result image (`SafetySuccess.bmp` / `SafetyFailure.bmp`) of its slot and refreshes the level from the item (`UpdateItem` came first);
//   * the maximum edit follows the `+` / `-` buttons (read-only here); `+` stops at the default maximum, `-` at 0.

import {
  TID_GAME_EQUIPPUT,
  TID_GAME_LACKMONEY,
  TID_GAME_SMELT_SAFETY_ERROR01,
  TID_GAME_SMELT_SAFETY_ERROR02,
  TID_GAME_SMELT_SAFETY_ERROR03,
  TID_GAME_SMELT_SAFETY_ERROR04,
  TID_GAME_SMELT_SAFETY_ERROR05,
  TID_GAME_SMELT_SAFETY_ERROR06,
  TID_GAME_SMELT_SAFETY_ERROR07,
  TID_GAME_SMELT_SAFETY_ERROR08,
  TID_GAME_SMELT_SAFETY_ERROR09,
  TID_GAME_SMELT_SAFETY_ERROR10,
  TID_GAME_SMELT_SAFETY_ERROR11,
  TID_GAME_SMELT_SAFETY_ERROR12,
  TID_GAME_SMELT_SAFETY_ERROR13,
  TID_GAME_SMELT_SAFETY_ERROR14,
  TID_GAME_SMELT_SAFETY_ERROR15,
  TID_GAME_SMELT_SAFETY_ERROR17,
  TID_GAME_SMELT_SAFETY_ERROR18,
  TID_GAME_SMELT_SAFETY_ERROR19,
  TID_GAME_SMELT_SAFETY_ERROR20,
  TID_GAME_SMELT_SAFETY_MAX_GRADE,
  TID_GAME_SMELT_SAFETY_MAX_PIERCING,
  TID_GAME_SMELT_SAFETY_NOW_GRADE,
  TID_GAME_SMELT_SAFETY_NOW_PIERCING,
  TID_GAME_SMELTSAFETY_ACCESSARY,
  TID_GAME_SMELTSAFETY_ELEMENT,
  TID_GAME_SMELTSAFETY_NORMAL,
  TID_GAME_SMELTSAFETY_PIERCING,
  TID_GAME_TOOLTIP_SMELT_SAFETY_ITEM,
} from '../../../gen/defs/text';
import {
  II_GEN_MAT_MOONSTONE,
  II_GEN_MAT_ORICHALCUM01,
  II_GEN_MAT_ORICHALCUM02,
  II_SYS_SYS_SCR_PIEPROT,
  II_SYS_SYS_SCR_SMELPROT,
  II_SYS_SYS_SCR_SMELPROT3,
  II_SYS_SYS_SCR_SMELPROT4,
  II_SYS_SYS_SCR_SMELTING,
  II_SYS_SYS_SCR_SMELTING2,
} from '../../../gen/defs/item';
import { WEAPON_ULTIMATE } from '../../../gen/defs/itemkind';
import type {
  SmeltSafetyView,
  SmeltSetting,
  SmeltSlot,
  SmeltSlotView,
} from '../../../ui/windows/upgrade/ports';
import type { InvItem } from '../../inventory/items';
import {
  cardOfElement,
  ELEMENTAL_NON_USING_SCROLL2_LEVEL,
  GENERAL_NON_USING_SCROLL2_LEVEL,
  isCollectorId,
  isElementalCard,
  oneShotMaterial,
  oneShotProtection,
  oneShotSmelting,
  oneShotTarget,
  type OneShotMode,
  type SmeltProp,
} from './rules';
import { MAXED_OUT } from './texts';
import { NO_U16, type SmeltEnv, type SmeltMachine } from './types';

/** `SMELT_MAX` of the layout: one slot per kind (the original has ten). */
export const SMELT_SLOTS = 1;
/** `nGoldNumber < 100000` of the piercing mode. */
export const PIERCING_FEE = 100_000;

const SETTING_OF: Readonly<Record<OneShotMode, SmeltSetting>> = {
  normal: 'normal',
  jewel: 'jewel',
  piercing: 'piercing',
  element: 'element',
};

const TITLE_TID: Readonly<Record<OneShotMode, number>> = {
  normal: TID_GAME_SMELTSAFETY_NORMAL,
  jewel: TID_GAME_SMELTSAFETY_ACCESSARY,
  piercing: TID_GAME_SMELTSAFETY_PIERCING,
  element: TID_GAME_SMELTSAFETY_ELEMENT,
};

/** One filled slot of a kind (`GENMATDIEINFO`). */
interface Slot {
  readonly objId: number;
}

export class OneShotSmelt implements SmeltMachine {
  readonly kind = 'oneShot';
  readonly #env: SmeltEnv;
  readonly #mode: OneShotMode;
  /** `m_pItemElem`. */
  #target: number | null = null;
  /** `m_Material`, `m_Scroll1`, `m_Scroll2`: the units of the slots, one entry per slot from the current one on. */
  #materials: Slot[] = [];
  #protections: Slot[] = [];
  #scrolls: Slot[] = [];
  /** `m_nCurrentSmeltNumber` and the result of each done slot. */
  #current = 0;
  #results: (1 | 2)[] = [];
  #selectedCard: number | null = null;
  #maxValue = 0;
  #started = false;
  #resultSwitch = false;
  #startMs = 0;
  #dueMs: number | null = null;
  #valid = 0;

  constructor(env: SmeltEnv, mode: OneShotMode) {
    this.#env = env;
    this.#mode = mode;
  }

  get running(): boolean {
    return this.#started;
  }
  get counters(): { tries: number; successes: number; failures: number } {
    const successes = this.#results.filter((r) => r === 1).length;
    return { tries: this.#results.length, successes, failures: this.#results.length - successes };
  }
  get mode(): OneShotMode {
    return this.#mode;
  }
  get target(): number | null {
    return this.#target;
  }
  /** `m_nValidSmeltCounter`: the tries still possible. */
  get validTries(): number {
    return this.#valid;
  }
  get maxValue(): number {
    return this.#maxValue;
  }
  get slotsUsed(): number {
    return this.#current;
  }

  // ---- reservations -------------------------------------------------------------------------------------------------------------

  /** `AddListItem`: one more unit of the stack is reserved. */
  #reserveOne(objId: number): void {
    const it = this.#env.item(objId);
    if (it) this.#env.setExtra(objId, it.extra + 1);
  }

  /** `SubtractListItem`: one unit goes back. */
  #releaseOne(objId: number): void {
    const it = this.#env.item(objId);
    if (it) this.#env.setExtra(objId, Math.max(0, it.extra - 1));
  }

  #targetItem(): InvItem | null {
    return this.#target === null ? null : this.#env.item(this.#target);
  }

  #targetProp(): SmeltProp | null {
    const it = this.#targetItem();
    return it ? (this.#env.prop(it.itemId) ?? null) : null;
  }

  // ---- values -------------------------------------------------------------------------------------------------------------------

  /** `GetNowSmeltValue`. */
  #value(): number {
    const it = this.#targetItem();
    if (!it) return 0;
    switch (this.#mode) {
      case 'normal':
      case 'jewel':
        return it.abilityOption;
      case 'piercing':
        return it.cards.length;
      case 'element':
        return it.resistAbilityOption;
    }
  }

  /** `GetDefaultMaxSmeltValue`. */
  #defaultMax(): number {
    switch (this.#mode) {
      case 'normal':
        return 10;
      case 'jewel':
      case 'element':
        return 20;
      case 'piercing':
        // a suit has four sockets, anything else ten
        return this.#targetProp()?.item_kind3 === 18 ? 4 : 10;
    }
  }

  /** `RefreshValidSmeltCounter`. */
  #refreshValid(): void {
    let valid = Math.min(this.#materials.length, this.#protections.length);
    valid = Math.max(0, Math.min(valid, SMELT_SLOTS - this.#current));
    const left = Math.max(0, this.#maxValue - this.#value());
    this.#valid = Math.min(left, valid);
  }

  // ---- SetItem ------------------------------------------------------------------------------------------------------------------

  setItem(objId: number): boolean {
    if (this.#started) {
      this.#env.sayTid(TID_GAME_SMELT_SAFETY_ERROR12);
      return false;
    }
    const item = this.#env.item(objId);
    if (!item) return false;
    const prop = this.#env.prop(item.itemId);
    if (!prop) return false;
    if (this.#target === null) {
      const ok = oneShotTarget(
        this.#mode,
        prop,
        isCollectorId(item.itemId, true),
        this.#env.pierceAble(item),
      );
      if (!ok) {
        this.#env.sayTid(
          this.#mode === 'piercing' ? TID_GAME_SMELT_SAFETY_ERROR02 : TID_GAME_SMELT_SAFETY_ERROR01,
        );
        return false;
      }
      if (this.#env.isWorn(objId)) {
        this.#env.sayTid(TID_GAME_EQUIPPUT);
        return false;
      }
      this.#target = objId;
      this.#reserveOne(objId);
      this.#maxValue = this.#defaultMax();
      this.#refreshValid();
      return true;
    }
    const targetItem = this.#targetItem();
    const main = this.#targetProp();
    if (!targetItem || !main) return false;
    const free = item.extra < item.num;
    const mat = oneShotMaterial(
      this.#mode,
      prop,
      main,
      targetItem.itemResist,
      this.#selectedCard,
      this.#materials.length > 0,
    );
    if (mat.select !== null) this.#selectedCard = mat.select;
    if (mat.ok) {
      if (this.#materials.length < SMELT_SLOTS - this.#current && free) {
        this.#materials.push({ objId });
        this.#reserveOne(objId);
      }
    } else if (oneShotProtection(this.#mode, prop.id, main)) {
      if (this.#protections.length < this.#materials.length) {
        if (free) {
          this.#protections.push({ objId });
          this.#reserveOne(objId);
        }
      } else if (this.#protections.length + this.#current !== SMELT_SLOTS) {
        this.#sayNeedMaterial(main);
      }
    } else if (
      (this.#mode === 'normal' || this.#mode === 'element') &&
      oneShotSmelting(this.#mode, prop.id, main, targetItem)
    ) {
      if (this.#scrolls.length < this.#materials.length) {
        if (free) {
          this.#scrolls.push({ objId });
          this.#reserveOne(objId);
        }
      } else if (this.#scrolls.length + this.#current !== SMELT_SLOTS) {
        this.#sayNeedMaterial(main);
      }
    } else if (this.#mode === 'element' && isElementalCard(prop.id)) {
      this.#env.sayTid(
        targetItem.itemResist !== 0 ? TID_GAME_SMELT_SAFETY_ERROR18 : TID_GAME_SMELT_SAFETY_ERROR19,
      );
    } else {
      this.#env.sayTid(TID_GAME_SMELT_SAFETY_ERROR06);
    }
    this.#refreshValid();
    return true;
  }

  /** "register the material first" (`ERROR03` / `04` / `05` / `17` by the mode). */
  #sayNeedMaterial(main: SmeltProp): void {
    switch (this.#mode) {
      case 'normal':
        this.#env.sayTid(
          main.refer_stat1 !== WEAPON_ULTIMATE
            ? TID_GAME_SMELT_SAFETY_ERROR03
            : TID_GAME_SMELT_SAFETY_ERROR04,
        );
        break;
      case 'jewel':
      case 'piercing':
        this.#env.sayTid(TID_GAME_SMELT_SAFETY_ERROR05);
        break;
      case 'element':
        this.#env.sayTid(TID_GAME_SMELT_SAFETY_ERROR17);
        break;
    }
  }

  // ---- the buttons --------------------------------------------------------------------------------------------------------------

  clear(slot: SmeltSlot): void {
    if (this.#started) return;
    switch (slot) {
      case 'target':
        // a double click on the target: the item goes, everything resets (`ResetData`)
        if (this.#target !== null) {
          this.#releaseSlots();
          this.#releaseOne(this.#target);
          this.#target = null;
          this.#maxValue = 0;
          this.#refreshValid();
        }
        break;
      case 'material': {
        const m = this.#materials.pop();
        if (m) this.#releaseOne(m.objId);
        // a protection or a scroll without its material goes too
        while (this.#protections.length > this.#materials.length) {
          const p = this.#protections.pop();
          if (p) this.#releaseOne(p.objId);
        }
        while (this.#scrolls.length > this.#materials.length) {
          const s = this.#scrolls.pop();
          if (s) this.#releaseOne(s.objId);
        }
        if (this.#materials.length === 0 && this.#current === 0) this.#selectedCard = null;
        break;
      }
      case 'protection': {
        const p = this.#protections.pop();
        if (p) this.#releaseOne(p.objId);
        break;
      }
      case 'scroll': {
        const s = this.#scrolls.pop();
        if (s) this.#releaseOne(s.objId);
        break;
      }
    }
    this.#refreshValid();
  }

  /** `ResetData` without the target: the slots, the counters and the results. */
  #releaseSlots(): void {
    for (const s of [...this.#materials, ...this.#protections, ...this.#scrolls]) {
      this.#releaseOne(s.objId);
    }
    this.#materials = [];
    this.#protections = [];
    this.#scrolls = [];
    this.#current = 0;
    this.#results = [];
    this.#selectedCard = null;
  }

  start(): void {
    if (this.#started) return;
    if (this.#target === null) {
      this.#env.sayTid(TID_GAME_SMELT_SAFETY_ERROR07);
      return;
    }
    this.#refreshValid();
    if (this.#valid > 0) {
      if (this.#mode === 'piercing' && this.#env.gold() < PIERCING_FEE) {
        this.#env.sayTid(TID_GAME_LACKMONEY);
        return;
      }
      this.#started = true;
      this.#resultSwitch = true;
      const now = this.#env.now();
      this.#startMs = now;
      this.#dueMs = now + this.#env.enchantMs();
      return;
    }
    if (this.#value() === this.#defaultMax()) {
      this.#env.sayTid(TID_GAME_SMELT_SAFETY_ERROR13);
    } else if (this.#current >= SMELT_SLOTS) {
      this.#env.sayTid(TID_GAME_SMELT_SAFETY_ERROR15);
    } else if (
      this.#materials.length <= this.#current ||
      this.#protections.length <= this.#current
    ) {
      switch (this.#mode) {
        case 'normal':
          this.#env.sayTid(
            this.#targetProp()?.refer_stat1 !== WEAPON_ULTIMATE
              ? TID_GAME_SMELT_SAFETY_ERROR08
              : TID_GAME_SMELT_SAFETY_ERROR09,
          );
          break;
        case 'jewel':
          this.#env.sayTid(TID_GAME_SMELT_SAFETY_ERROR10);
          break;
        case 'piercing':
          this.#env.sayTid(TID_GAME_SMELT_SAFETY_ERROR11);
          break;
        case 'element':
          this.#env.sayTid(TID_GAME_SMELT_SAFETY_ERROR20);
          break;
      }
    } else if (this.#value() >= this.#maxValue) {
      this.#env.sayTid(TID_GAME_SMELT_SAFETY_ERROR14);
    }
  }

  stop(): void {
    if (this.#started) this.#stopSmelting();
  }

  #stopSmelting(): void {
    this.#started = false;
    this.#resultSwitch = false;
    this.#dueMs = null;
  }

  reset(): void {
    if (this.#started) return;
    this.#releaseSlots();
    this.#refreshValid();
  }

  adjust(delta: 1 | -1): void {
    if (this.#started || this.#target === null) return;
    if (delta === 1) this.#maxValue = Math.min(this.#maxValue + 1, this.#defaultMax());
    else this.#maxValue = Math.max(0, this.#maxValue - 1);
    this.#refreshValid();
  }

  // ---- the pacing ---------------------------------------------------------------------------------------------------------------

  tick(): void {
    if (!this.#started || this.#targetItem() === null) return;
    if (this.#valid === 0 || this.#value() === this.#defaultMax()) {
      this.#stopSmelting();
      return;
    }
    if (this.#mode === 'piercing' && this.#env.gold() < PIERCING_FEE) {
      this.#env.sayTid(TID_GAME_LACKMONEY);
      this.#stopSmelting();
      return;
    }
    const now = this.#env.now();
    if (this.#dueMs !== null && this.#dueMs < now) {
      this.#dueMs = null;
      this.#resultSwitch = false;
      const material = this.#materials[0];
      const protection = this.#protections[0];
      const scroll = this.#scrolls[0];
      if (material && protection && this.#target !== null) {
        this.#env.send({
          item: this.#target,
          material: material.objId,
          protection: protection.objId,
          scroll: scroll?.objId ?? NO_U16,
        });
        // the units are spent: the reservations of the slots go (`SetExtra( GetExtra() - 1 )`)
        this.#releaseOne(material.objId);
        this.#releaseOne(protection.objId);
        if (scroll) this.#releaseOne(scroll.objId);
        this.#materials.shift();
        this.#protections.shift();
        this.#scrolls.shift();
      }
    }
  }

  // ---- the answer ---------------------------------------------------------------------------------------------------------------

  onResult(result: number): void {
    if (result === 1 || result === 2) {
      this.#results.push(result);
      this.#current += 1;
      this.#resultSwitch = true;
      this.#refreshValid();
      if (this.#valid === 0) this.#stopSmelting();
      return;
    }
    // 0 refused, 3 already at the maximum: the run ends
    this.#stopSmelting();
    if (result === 3) this.#env.say(MAXED_OUT);
  }

  dispose(): void {
    this.#releaseSlots();
    if (this.#target !== null) this.#releaseOne(this.#target);
    this.#target = null;
    this.#started = false;
    this.#dueMs = null;
  }

  // ---- the view -----------------------------------------------------------------------------------------------------------------

  /** The hint icons of `DrawListItem`: what to drop next, translucent. */
  #hintIcons(): { material: number | null; protection: number | null; scroll: number | null } {
    const main = this.#targetProp();
    const item = this.#targetItem();
    let material: number | null = null;
    let protection: number | null = null;
    let scroll: number | null = null;
    switch (this.#mode) {
      case 'normal': {
        if (!main) break;
        const ultimate = main.refer_stat1 === WEAPON_ULTIMATE;
        material = ultimate ? II_GEN_MAT_ORICHALCUM02 : II_GEN_MAT_ORICHALCUM01;
        protection = ultimate ? II_SYS_SYS_SCR_SMELPROT3 : II_SYS_SYS_SCR_SMELPROT;
        if (!ultimate && item && item.abilityOption < GENERAL_NON_USING_SCROLL2_LEVEL) {
          scroll = II_SYS_SYS_SCR_SMELTING;
        }
        break;
      }
      case 'jewel':
        material = II_GEN_MAT_MOONSTONE;
        protection = II_SYS_SYS_SCR_SMELPROT4;
        break;
      case 'piercing':
        material = II_GEN_MAT_MOONSTONE;
        protection = II_SYS_SYS_SCR_PIEPROT;
        break;
      case 'element': {
        material = this.#selectedCard;
        if (item) {
          const card = cardOfElement(item.itemResist);
          if (card !== 0) material = card;
        }
        protection = II_SYS_SYS_SCR_SMELPROT;
        if (item && item.resistAbilityOption < ELEMENTAL_NON_USING_SCROLL2_LEVEL) {
          scroll = II_SYS_SYS_SCR_SMELTING2;
        }
        break;
      }
    }
    return { material, protection, scroll };
  }

  view(): SmeltSafetyView {
    const target = this.#targetItem();
    const piercing = this.#mode === 'piercing';
    const hints = this.#hintIcons();
    const slot = (placed: Slot[], hint: number | null): SmeltSlotView | null => {
      const first = placed[0];
      if (first) {
        const it = this.#env.item(first.objId);
        return { icon: it ? this.#env.icon(it.itemId) : null, count: 0 };
      }
      // nothing placed: a translucent hint while a slot is still free (the normal mode needs a target first)
      if (this.#current < SMELT_SLOTS && hint !== null && (this.#mode !== 'normal' || target)) {
        return { icon: this.#env.icon(hint), count: 0, ghost: true };
      }
      return null;
    };
    let gauge = 0;
    if (this.#started && this.#resultSwitch && this.#dueMs !== null) {
      const span = this.#dueMs - this.#startMs;
      gauge = span > 0 ? Math.max(0, Math.min(1, (this.#env.now() - this.#startMs) / span)) : 1;
    } else if (this.#started) {
      gauge = 1;
    }
    const last = this.#results[this.#results.length - 1];
    return {
      title: this.#env.text(TITLE_TID[this.#mode]),
      nowCaption: this.#env.text(
        piercing ? TID_GAME_SMELT_SAFETY_NOW_PIERCING : TID_GAME_SMELT_SAFETY_NOW_GRADE,
      ),
      maxCaption: this.#env.text(
        piercing ? TID_GAME_SMELT_SAFETY_MAX_PIERCING : TID_GAME_SMELT_SAFETY_MAX_GRADE,
      ),
      nowValue: target ? String(this.#value()) : '--',
      maxValue: target ? String(this.#maxValue) : '',
      target: target ? this.#env.dialogItem(target) : null,
      material: slot(this.#materials, hints.material),
      protection: slot(this.#protections, hints.protection),
      scroll:
        hints.scroll === null && this.#scrolls.length === 0
          ? null
          : slot(this.#scrolls, hints.scroll),
      running: this.#started,
      gauge,
      result: last ?? 0,
      canStart: !this.#started,
      canStop: this.#started,
      canReset: !this.#started,
      canAdjust: !this.#started && target !== null,
      emptyTip: this.#env.text(TID_GAME_TOOLTIP_SMELT_SAFETY_ITEM),
    };
  }

  /** The setting this window stands for (the tests). */
  get setting(): SmeltSetting {
    return SETTING_OF[this.#mode];
  }
}
