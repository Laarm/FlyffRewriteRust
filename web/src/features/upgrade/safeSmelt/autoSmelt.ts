// The auto safe-upgrade machine (`Kia::CWndNewUpgrade`, `src:_Interface/WndNewUpgrade.cpp`, `__BETTER_SAFE_UPGRADES`; Plan/45 §18.2; WP-CFEAT-24): a target, a
// material, a protection scroll and an optional smelting scroll; Start sends one `SmeltSafetyReq` per `enchant_time_s` until the target reaches the maximum
// (`maxValue` or the default maximum of the setting), the materials or the protection scrolls run out, or the server stops it (result 0 or 3).
//   * the setting (`none | normal | ultimate | jewel | element | piercing`) follows from the target and the material; a new target replaces the old one and
//     clears a material that does not fit it; every stack of the material and of the scrolls is RESERVED whole (`SetExtra( num )`);
//   * `SmeltSafetyNtf` 1 / 2 count a success / a failure and one unit of each material; the run ends when the default maximum is reached or a count is 0,
//     keeping the tries line in the title; 0 stops; 3 stops and prints "Failed to upgrade. It's already maxed out";
//   * deviations (listed in NOTES): the tries line and the last result image stay after a finished run (the original reset the title and cleared the
//     image at once), the maximum edit is read-only (the buttons change it), the unit counts under the scroll icons are their own (the original printed the
//     material count there, a missing brace), a worn item is refused with `TID_GAME_EQUIPPUT` before the server has to.

import { TID_GAME_EQUIPPUT } from '../../../gen/defs/text';
import type { SmeltSafetyView, SmeltSetting, SmeltSlot } from '../../../ui/windows/upgrade/ports';
import type { InvItem } from '../../inventory/items';
import {
  currentValue,
  defaultMax,
  isAutoMaterial,
  isAutoTarget,
  isCollectorId,
  materialSetting,
  protectionOf,
  scrollRole,
  smeltingScrollOf,
  type SmeltProp,
} from './rules';
import {
  MAX_CAPTION,
  MAXED_OUT,
  MODE_TEXTS,
  RESET_CAPTION,
  RESET_TITLE,
  TARGET_TITLE,
  triesLine,
} from './texts';
import { NO_U16, type SmeltEnv, type SmeltMachine } from './types';

/** `maxValue( 20 )` of the constructor. */
const INITIAL_MAX = 20;

export class AutoSmelt implements SmeltMachine {
  readonly kind = 'auto';
  readonly #env: SmeltEnv;
  #setting: SmeltSetting = 'none';
  /** `itemElem`: the object id of the target. */
  #target: number | null = null;
  /** `matId`, `scrollId`, `scroll2Id`: ITEM ids (every stack of them is reserved). */
  #matId = 0;
  #protectId = 0;
  #scrollId = 0;
  #matCount = 0;
  #protectCount = 0;
  #scrollCount = 0;
  #maxValue = INITIAL_MAX;
  #running = false;
  #startMs = 0;
  /** `enchantEnd`: when the next try is due, `null` after it was sent (waiting for the answer). */
  #dueMs: number | null = null;
  #result: 0 | 1 | 2 = 0;
  #successes = 0;
  #failures = 0;
  #title = RESET_TITLE;
  #caption = RESET_CAPTION;

  constructor(env: SmeltEnv) {
    this.#env = env;
  }

  get running(): boolean {
    return this.#running;
  }
  get counters(): { tries: number; successes: number; failures: number } {
    return {
      tries: this.#successes + this.#failures,
      successes: this.#successes,
      failures: this.#failures,
    };
  }
  /** The machine's setting (the tests read it). */
  get setting(): SmeltSetting {
    return this.#setting;
  }
  get target(): number | null {
    return this.#target;
  }
  get materialId(): number {
    return this.#matId;
  }
  get maxValue(): number {
    return this.#maxValue;
  }

  // ---- reservations -------------------------------------------------------------------------------------------------------------

  /** `SetExtra( m_nItemNum )` on every stack of an item id; the total units. */
  #reserveAll(itemId: number): number {
    let total = 0;
    for (const s of this.#env.stacks(itemId)) {
      this.#env.setExtra(s.objId, s.num);
      total += s.num;
    }
    return total;
  }

  #releaseAll(itemId: number): void {
    for (const s of this.#env.stacks(itemId)) this.#env.setExtra(s.objId, 0);
  }

  #releaseTarget(): void {
    if (this.#target === null) return;
    const it = this.#env.item(this.#target);
    if (it) this.#env.setExtra(it.objId, Math.max(0, it.extra - 1));
    this.#target = null;
  }

  // ---- the setting --------------------------------------------------------------------------------------------------------------

  #setMode(s: SmeltSetting): void {
    this.#setting = s;
    this.#title = MODE_TEXTS[s].title;
    this.#caption = MODE_TEXTS[s].caption;
  }

  /** `ResetMats`. */
  #resetMats(): void {
    if (this.#matId !== 0) {
      this.#releaseAll(this.#matId);
      this.#matCount = 0;
      this.#matId = 0;
    }
    if (this.#protectId !== 0) {
      this.#releaseAll(this.#protectId);
      this.#protectCount = 0;
      this.#protectId = 0;
    }
    if (this.#scrollId !== 0) {
      this.#releaseAll(this.#scrollId);
      this.#scrollCount = 0;
      this.#scrollId = 0;
    }
    this.#setting = 'none';
    this.#title = RESET_TITLE;
    this.#caption = RESET_CAPTION;
    this.#result = 0;
  }

  /** `CheckScroll` / `CheckScroll2`: a scroll that does not belong to the new setting goes. */
  #checkScrolls(): void {
    const keepProt = protectionOf(this.#setting);
    if (this.#protectId !== 0 && keepProt !== this.#protectId) {
      this.#protectCount = 0;
      this.#releaseAll(this.#protectId);
      this.#protectId = 0;
    }
    const keepScroll = smeltingScrollOf(this.#setting);
    if (this.#scrollId !== 0 && keepScroll !== this.#scrollId) {
      this.#scrollCount = 0;
      this.#releaseAll(this.#scrollId);
      this.#scrollId = 0;
    }
  }

  #targetItem(): InvItem | null {
    return this.#target === null ? null : this.#env.item(this.#target);
  }

  #targetProp(): SmeltProp | null {
    const it = this.#targetItem();
    return it ? (this.#env.prop(it.itemId) ?? null) : null;
  }

  #max(): number {
    return defaultMax(this.#setting, this.#targetProp());
  }

  #value(): number {
    const it = this.#targetItem();
    return it ? currentValue(this.#setting, it) : 0;
  }

  // ---- SetItem ------------------------------------------------------------------------------------------------------------------

  setItem(objId: number): boolean {
    if (this.#running) return false;
    const item = this.#env.item(objId);
    if (!item) return false;
    const prop = this.#env.prop(item.itemId);
    if (!prop) return false;
    const oldSetting = this.#setting;
    const collector = isCollectorId(item.itemId);
    if (isAutoTarget(prop, collector)) {
      if (this.#env.isWorn(objId)) {
        // the server would refuse; the chat line is `TID_GAME_EQUIPPUT` (the original let the request go)
        this.#env.sayTid(TID_GAME_EQUIPPUT);
        return false;
      }
      if (this.#target !== null) {
        this.#releaseTarget();
        this.#resetMats();
        this.#successes = 0;
        this.#failures = 0;
      }
      this.#target = objId;
      this.#env.setExtra(objId, item.extra + 1);
      // the material of the old target went with it (`ResetMats` above), so there is nothing to test it against: the original's compatibility test
      // after this point never ran (`matId` is 0), and a first target starts clean as well
      this.#resetMats();
      this.#title = TARGET_TITLE;
    } else if (isAutoMaterial(prop)) {
      const targetItem = this.#targetItem();
      if (!targetItem) return false;
      if (this.#matId === item.itemId) return false;
      this.#resetMats();
      const main = this.#env.prop(targetItem.itemId);
      const s = main ? materialSetting(prop, main, isCollectorId(targetItem.itemId)) : null;
      if (s !== null) {
        this.#setMode(s);
        this.#matId = item.itemId;
        this.#matCount = this.#reserveAll(item.itemId);
        this.#checkScrolls();
        this.#maxValue = this.#max();
      }
    } else {
      const targetItem = this.#targetItem();
      if (targetItem && this.#matId !== 0) {
        const mat = this.#env.prop(this.#matId);
        const main = this.#env.prop(targetItem.itemId);
        const role =
          mat && main ? scrollRole(item.itemId, mat, main, isCollectorId(targetItem.itemId)) : null;
        if (role === 'protection') {
          this.#protectId = item.itemId;
          this.#protectCount = this.#reserveAll(item.itemId);
        } else if (role === 'scroll') {
          this.#scrollId = item.itemId;
          this.#scrollCount = this.#reserveAll(item.itemId);
        }
      }
    }
    if (oldSetting !== this.#setting) this.#maxValue = this.#max();
    return true;
  }

  // ---- the buttons --------------------------------------------------------------------------------------------------------------

  clear(slot: SmeltSlot): void {
    if (this.#running) return;
    switch (slot) {
      case 'target':
        if (this.#target !== null) {
          this.#releaseTarget();
          this.#resetMats();
          this.#successes = 0;
          this.#failures = 0;
        }
        break;
      case 'material':
        if (this.#matId !== 0) this.#resetMats();
        break;
      case 'protection':
        if (this.#protectId !== 0) {
          this.#protectCount = 0;
          this.#releaseAll(this.#protectId);
          this.#protectId = 0;
          this.#result = 0;
        }
        break;
      case 'scroll':
        if (this.#scrollId !== 0) {
          this.#scrollCount = 0;
          this.#releaseAll(this.#scrollId);
          this.#scrollId = 0;
        }
        break;
    }
  }

  get #canStart(): boolean {
    return (
      !this.#running && this.#targetItem() !== null && this.#matCount > 0 && this.#protectCount > 0
    );
  }

  start(): void {
    if (!this.#canStart) return;
    this.#running = true;
    const now = this.#env.now();
    this.#startMs = now;
    this.#dueMs = now + this.#env.enchantMs();
    this.#successes = 0;
    this.#failures = 0;
  }

  stop(): void {
    if (!this.#running) return;
    this.#running = false;
    this.#dueMs = null;
  }

  reset(): void {
    this.#releaseTarget();
    this.#resetMats();
    this.#successes = 0;
    this.#failures = 0;
    this.#result = 0;
  }

  adjust(delta: 1 | -1): void {
    if (delta === 1) {
      this.#maxValue = Math.min(this.#maxValue + 1, this.#max());
    } else {
      this.#maxValue = this.#maxValue - 1 === 0 ? 1 : this.#maxValue - 1;
    }
  }

  // ---- the pacing ---------------------------------------------------------------------------------------------------------------

  tick(): void {
    if (!this.#running) return;
    if (this.#targetItem() === null) {
      // the target is gone (dropped, destroyed): nothing to upgrade
      this.#running = false;
      this.#dueMs = null;
      this.#releaseTarget();
      this.#resetMats();
      return;
    }
    // the maximum is checked first: in the original `Process` runs every 100 ms, so the level was seen long before the next try was due
    if (this.#value() >= this.#maxValue || this.#value() >= this.#max()) {
      this.#running = false;
      this.#dueMs = null;
      return;
    }
    const now = this.#env.now();
    if (this.#dueMs !== null && this.#dueMs < now) {
      this.#dueMs = null;
      this.#send();
    }
  }

  #send(): void {
    const first = (itemId: number): number =>
      itemId === 0 ? NO_U16 : (this.#env.stacks(itemId)[0]?.objId ?? NO_U16);
    this.#env.send({
      item: this.#target ?? NO_U16,
      material: first(this.#matId),
      protection: first(this.#protectId),
      scroll: first(this.#scrollId),
    });
  }

  // ---- the answer ---------------------------------------------------------------------------------------------------------------

  onResult(result: number): void {
    if (result !== 1 && result !== 2) {
      this.#running = false;
      this.#dueMs = null;
      if (result === 3) this.#env.say(MAXED_OUT);
      return;
    }
    this.#result = result;
    if (result === 2) this.#failures += 1;
    else this.#successes += 1;
    this.#title = triesLine(this.#successes, this.#failures);
    if (this.#matCount > 0) this.#matCount -= 1;
    if (this.#protectCount > 0) this.#protectCount -= 1;
    if (this.#scrollCount > 0) this.#scrollCount -= 1;
    if (this.#max() === this.#value() || this.#matCount === 0 || this.#protectCount === 0) {
      // the run is over: the materials go back, the tries line and the last result stay
      this.#running = false;
      this.#dueMs = null;
      const title = this.#title;
      this.#resetMats();
      this.#title = title;
      this.#result = result;
    } else {
      const now = this.#env.now();
      this.#startMs = now;
      this.#dueMs = now + this.#env.enchantMs();
    }
  }

  dispose(): void {
    this.#releaseTarget();
    this.#resetMats();
    this.#running = false;
    this.#dueMs = null;
  }

  // ---- the view -----------------------------------------------------------------------------------------------------------------

  view(): SmeltSafetyView {
    const target = this.#targetItem();
    const slot = (id: number, count: number) =>
      id === 0 ? null : { icon: this.#env.icon(id), count };
    let gauge = 0;
    if (this.#running) {
      if (this.#dueMs === null) gauge = 1;
      else {
        const span = this.#dueMs - this.#startMs;
        gauge = span > 0 ? Math.max(0, Math.min(1, (this.#env.now() - this.#startMs) / span)) : 1;
      }
    }
    return {
      title: this.#title,
      nowCaption: this.#caption,
      maxCaption: MAX_CAPTION,
      nowValue: target && this.#setting !== 'none' ? String(this.#value()) : '',
      maxValue: String(this.#maxValue),
      target: target ? this.#env.dialogItem(target) : null,
      material: slot(this.#matId, this.#matCount),
      protection: slot(this.#protectId, this.#protectCount),
      scroll: slot(this.#scrollId, this.#scrollCount),
      running: this.#running,
      gauge,
      result: this.#result,
      canStart: this.#canStart,
      canStop: this.#running,
      canReset: !this.#running,
      canAdjust: true,
      emptyTip: null,
    };
  }
}
