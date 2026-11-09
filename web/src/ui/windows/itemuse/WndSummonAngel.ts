// `WndSummonAngel` (`CWndSummonAngel`, `src:_Interface/WndSummonAngel.cpp`, ref/22 §17 D, Plan/42 §19.4 D, 45 §9.4; WP-UI-28):
// `APP_SUMMON_ANGEL` (464 x 464), the angel summoning window. Twenty 32 x 32 slots: the first ten (`WIDC_STATIC11..20`) take
// Orichalcum, the last ten (`WIDC_STATIC21..30`) Moonstone. Materials are dropped on a slot (Shift fills every free slot of that
// kind with the stack) or double-clicked in the inventory (the first free slot of its kind: `feed`); a double click on a used slot
// takes it back. The rates are `green = orichalcum + moonstone`, `white = green / 10`, `blue = 2 * green`, `red = 100 - green - blue`
// (one decimal). Start needs three materials (`TID_GAME_SUMMONANGEL_ERROR2`) and sends the stacks with their counts, then resets;
// Reset gives everything back; closing releases the reserved units. A stack that is full (`extra >= count`) answers
// `TID_GAME_SUMMONANGEL_ERROR1`.

import {
  APP_SUMMON_ANGEL,
  WIDC_CANCEL,
  WIDC_RESET,
  WIDC_START,
  WIDC_STATIC1,
  WIDC_STATIC3,
  WIDC_STATIC4,
  WIDC_STATIC5,
  WIDC_STATIC6,
  WIDC_STATIC7,
  WIDC_STATIC11,
  WIDC_STATIC12,
  WIDC_STATIC13,
  WIDC_STATIC14,
  WIDC_STATIC15,
  WIDC_STATIC16,
  WIDC_STATIC17,
  WIDC_STATIC18,
  WIDC_STATIC19,
  WIDC_STATIC20,
  WIDC_STATIC21,
  WIDC_STATIC22,
  WIDC_STATIC23,
  WIDC_STATIC24,
  WIDC_STATIC25,
  WIDC_STATIC26,
  WIDC_STATIC27,
  WIDC_STATIC28,
  WIDC_STATIC29,
  WIDC_STATIC30,
  WIDC_STATIC_BLUE,
  WIDC_STATIC_GREEN,
  WIDC_STATIC_RED,
  WIDC_TEXT1,
} from '../../../gen/defs/resdata';
import {
  TID_GAME_BLUEANGEL_RATE,
  TID_GAME_GREENANGEL_RATE,
  TID_GAME_REDANGEL_RATE,
  TID_GAME_SUMMONANGEL_ERROR1,
  TID_GAME_SUMMONANGEL_ERROR2,
  TID_GAME_WHITEANGEL_INFO1,
  TID_GAME_WHITEANGEL_INFO2,
  TID_GAME_WHITEANGEL_INFO3,
} from '../../../gen/defs/text';
import { IconStrip, type StripEntry } from '../../controls/IconStrip';
import type { Static } from '../../controls/Static';
import type { TextCtrl } from '../../controls/TextCtrl';
import { TextLayer, type TextItem } from '../../controls/TextLayer';
import { SC, type Shortcut } from '../../dnd/shortcut';
import { type Point, type Rect, rectContains } from '../../layout/geometry';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { MAX_MATDIE, angelRates } from './angelRates';
import type { AngelStack, SummonAngelRequest } from './ports';

const SLOT_IDS = [
  WIDC_STATIC11,
  WIDC_STATIC12,
  WIDC_STATIC13,
  WIDC_STATIC14,
  WIDC_STATIC15,
  WIDC_STATIC16,
  WIDC_STATIC17,
  WIDC_STATIC18,
  WIDC_STATIC19,
  WIDC_STATIC20,
  WIDC_STATIC21,
  WIDC_STATIC22,
  WIDC_STATIC23,
  WIDC_STATIC24,
  WIDC_STATIC25,
  WIDC_STATIC26,
  WIDC_STATIC27,
  WIDC_STATIC28,
  WIDC_STATIC29,
  WIDC_STATIC30,
] as const;
const PERCENT_COLOR = 0xff2e70a9;

interface Slot {
  rect: Rect;
  used: boolean;
  stack: AngelStack | null;
}

export { MAX_MATDIE, angelRates };

export class WndSummonAngel extends WndNeuz {
  readonly appId = APP_SUMMON_ANGEL;
  readonly #req: SummonAngelRequest;
  readonly #getText: (tid: number) => string;
  readonly #slots: Slot[] = [];
  #orichalcum = 0;
  #moonstone = 0;
  #strip: IconStrip | null = null;
  #texts: TextLayer | null = null;
  #intro: TextCtrl | null = null;
  /** `m_nowStarting`. */
  #starting = false;
  /** The slot under the pointer while a drag is over the window (`m_nSelecCtrl`). */
  #hot = -1;

  constructor(req: SummonAngelRequest, getText: (tid: number) => string) {
    super();
    this.#req = req;
    this.#getText = getText;
    this.putRegInfo = false;
  }

  get orichalcum(): number {
    return this.#orichalcum;
  }
  get moonstone(): number {
    return this.#moonstone;
  }
  get rates(): ReturnType<typeof angelRates> {
    return angelRates(this.#orichalcum, this.#moonstone);
  }
  get strip(): IconStrip | null {
    return this.#strip;
  }
  get slotRects(): readonly Rect[] {
    return this.#slots.map((s) => s.rect);
  }
  /** Whether slot `i` holds a material. */
  slotUsed(i: number): boolean {
    return this.#slots[i]?.used ?? false;
  }
  get introText(): TextCtrl | null {
    return this.#intro;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#intro = this.getDlgItem<TextCtrl>(WIDC_TEXT1);
    this.#strip = this.addControl(new IconStrip(), 0x7f01, this.getClientRect(false));
    this.#texts = this.addControl(new TextLayer(), 0x7f02, this.getClientRect(false));
    for (const id of SLOT_IDS) {
      const rect = this.getWndCtrl(id)?.rect ?? { l: 0, t: 0, r: 0, b: 0 };
      this.#slots.push({ rect, used: false, stack: null });
    }
    const set = (id: number, tid: number): void => {
      this.getDlgItem<Static>(id)?.setTitle(this.#getText(tid));
    };
    set(WIDC_STATIC3, TID_GAME_REDANGEL_RATE);
    set(WIDC_STATIC4, TID_GAME_BLUEANGEL_RATE);
    set(WIDC_STATIC5, TID_GAME_GREENANGEL_RATE);
    set(WIDC_STATIC6, TID_GAME_WHITEANGEL_INFO1);
    set(WIDC_STATIC1, TID_GAME_WHITEANGEL_INFO2);
    set(WIDC_STATIC7, TID_GAME_WHITEANGEL_INFO3);
    this.#refresh();
    // beside the inventory: on the side that has more room
    const inv = this.#req.inventoryRect();
    const w = this.rectWindow.r - this.rectWindow.l;
    if (inv) {
      const x = inv.l > w / 2 ? inv.l - w : inv.r;
      this.move({ x, y: inv.t });
    }
  }

  /** `SetQuestText`: adds text to the information box. */
  setQuestText(text: string): void {
    this.#intro?.addString(text);
  }

  // ---- slots ---------------------------------------------------------------------------------------------------------------

  /** `HitTest(point)`: the slot under a client point, `-1` for none. */
  hitTest(pt: Point): number {
    return this.#slots.findIndex((s) => rectContains(s.rect, pt));
  }

  /**
   * `SetDie`: puts `stack` into the slot under the pointer (`slot >= 0`: Shift fills the free slots of its ten), else into the first
   * free slot of its kind (the double click in the inventory).
   */
  setDie(stack: AngelStack, slot = -1): void {
    const fresh = this.#req.stack(stack.objId) ?? stack;
    if (slot >= 0) {
      const base = slot < 10 ? 0 : 10;
      if (this.#req.shiftDown()) {
        const free: number[] = [];
        for (let i = base; i < base + 10; i += 1) if (!this.#slots[i]?.used) free.push(i);
        const usable = fresh.count - fresh.extra;
        if (free.length > 0 && usable > 0) {
          const n = Math.min(free.length, usable);
          for (let k = 0; k < n; k += 1) {
            const s = this.#slots[free[k] ?? 0];
            if (s) {
              s.used = true;
              s.stack = fresh;
            }
          }
          this.#req.setExtra(fresh.objId, fresh.extra + n);
          this.#add(base, n);
        }
      } else {
        const s = this.#slots[slot];
        if (s) {
          s.used = true;
          s.stack = fresh;
        }
        this.#req.setExtra(fresh.objId, fresh.extra + 1);
        this.#add(base, 1);
      }
    } else {
      const base = AngelSlots.base(fresh.kind);
      if (base < 0) return;
      for (let i = base; i < base + 10; i += 1) {
        const s = this.#slots[i];
        if (s && !s.used) {
          s.used = true;
          s.stack = fresh;
          this.#req.setExtra(fresh.objId, fresh.extra + 1);
          this.#add(base, 1);
          break;
        }
      }
    }
    this.#refresh();
  }

  #add(base: number, n: number): void {
    if (base === 0) this.#orichalcum += n;
    else this.#moonstone += n;
  }

  /** `OnLButtonDblClk`: a used slot gives its unit back. */
  override onLButtonDblClk(pt: Point): void {
    const i = this.hitTest(pt);
    const s = i >= 0 ? this.#slots[i] : undefined;
    if (!s?.used || !s.stack) return;
    s.used = false;
    if (i < 10) this.#orichalcum -= 1;
    else this.#moonstone -= 1;
    const cur = this.#req.stack(s.stack.objId);
    if (cur) this.#req.setExtra(cur.objId, Math.max(0, cur.extra - 1));
    s.stack = null;
    this.#refresh();
  }

  /** `OnDropIcon`: a stack from the inventory onto a free slot of the matching kind. */
  override onDropIcon(sc: Shortcut, ptWindow: Point): boolean {
    if (sc.kind !== SC.ITEM) return false;
    const co = {
      x: this.rectClient.l - this.rectWindow.l,
      y: this.rectClient.t - this.rectWindow.t,
    };
    const slot = this.hitTest({ x: ptWindow.x - co.x, y: ptWindow.y - co.y });
    const stack = this.#req.stack(sc.id);
    if (!stack || slot < 0) return true;
    const s = this.#slots[slot];
    if (!s || s.used) return true;
    const kind = stack.kind;
    const fits = (slot < 10 && kind === 'orichalcum') || (slot >= 10 && kind === 'moonstone');
    if (!fits) return true;
    if (stack.count > stack.extra) this.setDie(stack, slot);
    else this.#req.message(TID_GAME_SUMMONANGEL_ERROR1);
    return true;
  }

  /** `ReFreshAll(extracheck)`: empties the slots; `release` gives the reserved units of every stack back. */
  reset(release = true): void {
    for (const s of this.#slots) {
      if (s.stack && release) this.#req.setExtra(s.stack.objId, 0);
      s.used = false;
      s.stack = null;
    }
    this.#orichalcum = 0;
    this.#moonstone = 0;
    this.#starting = false;
    this.#refresh();
  }

  /** The Start button: three materials are the minimum; the stacks go out with their counts. */
  start(): void {
    if (this.#orichalcum + this.#moonstone < 3) {
      this.#req.message(TID_GAME_SUMMONANGEL_ERROR2);
      return;
    }
    if (this.#starting) return;
    this.#starting = true;
    const sums = new Map<number, number>();
    for (const s of this.#slots) {
      if (s.used && s.stack) sums.set(s.stack.objId, (sums.get(s.stack.objId) ?? 0) + 1);
    }
    this.#req.create([...sums].map(([objId, count]) => ({ objId, count })));
    this.reset(true);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_RESET) {
        this.reset(true);
        return true;
      }
      if (id === WIDC_START) {
        this.start();
        return true;
      }
      if (id === WIDC_CANCEL) {
        this.destroy();
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  override onMouseMove(pt: Point): void {
    const hot = this.env.wm.isDraggingShortcut() ? this.hitTest(pt) : -1;
    if (hot !== this.#hot) {
      this.#hot = hot;
      this.#paint();
    }
  }

  override onMouseLeave(): void {
    if (this.#hot >= 0) {
      this.#hot = -1;
      this.#paint();
    }
  }

  /** `OnDestroy`: the reserved units of every used stack are released (a stack in a trade keeps its reservation). */
  override onDestroy(): void {
    for (const s of this.#slots) {
      if (s.stack) this.#req.setExtra(s.stack.objId, 0);
    }
    super.onDestroy();
  }

  // ---- the pixels ----------------------------------------------------------------------------------------------------------

  #refresh(): void {
    const r = this.rates;
    const f = (n: number): string => n.toFixed(1);
    this.getDlgItem<Static>(WIDC_STATIC_RED)?.setTitle(f(r.red));
    this.getDlgItem<Static>(WIDC_STATIC_BLUE)?.setTitle(f(r.blue));
    this.getDlgItem<Static>(WIDC_STATIC_GREEN)?.setTitle(f(r.green));
    this.#paint();
  }

  #paint(): void {
    const entries: StripEntry[] = [];
    entries.push({
      key: 'orichalcum',
      icon: this.#req.orichalcumIcon,
      x: 30,
      y: 218,
      w: 32,
      h: 32,
      alpha: 255,
    });
    entries.push({
      key: 'moonstone',
      icon: this.#req.moonstoneIcon,
      x: 30,
      y: 306,
      w: 32,
      h: 32,
      alpha: 255,
    });
    this.#slots.forEach((s, i) => {
      if (s.used && s.stack) {
        entries.push({
          key: `slot${String(i)}`,
          icon: s.stack.icon,
          x: s.rect.l,
          y: s.rect.t,
          w: 32,
          h: 32,
          alpha: 255,
        });
      }
    });
    this.#strip?.setEntries(entries);
    const texts: TextItem[] = [];
    for (let k = 0; k < 3; k += 1) {
      texts.push({ x: 415, y: 206 + 24 * k, text: '%', color: PERCENT_COLOR });
    }
    this.#texts?.setItems(texts);
  }
}

/** Kept apart so `setDie` can call the static helper without the private-name rules getting in the way. */
const AngelSlots = {
  base: (kind: AngelStack['kind']): number =>
    kind === 'orichalcum' ? 0 : kind === 'moonstone' ? 10 : -1,
};
