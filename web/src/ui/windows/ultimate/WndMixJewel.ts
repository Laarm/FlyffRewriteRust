// `WndMixJewel` (`CWndMixJewel`, `src:_Interface/WndField.cpp:19694-20160`, `__CSC_VER9_1`, ref/22 §17 L, Plan/42 §19.4 L, 45 §18.6; WP-UI-32 /
// WP-CFEAT-26): `APP_SMELT_MIXJEWEL` (288 x 288), "Sunstone & Moonstone Use". Ten 32 x 32 slots (`WIDC_PIC_SLOT1..10`): the first five take
// Orichalcum (`II_GEN_MAT_ORICHALCUM01` / `_1`), the last five moonstones (`II_GEN_MAT_MOONSTONE` / `_1`); an empty slot shows the faint
// picture of its kind (alpha 50). Materials are dropped on a slot (Shift fills every free slot of its five with the stack) or double-clicked
// in the bag (the first free slot of the kind: `feed`); a double click on a used slot takes the unit back. A stack that is fully reserved
// answers `TID_GAME_SUMMONANGEL_ERROR1` in a box. Start (enabled only with all ten slots filled) sends `UltimateMakeItem{ids[10]}`: the stacks
// in slot order, each repeated once per unit, so the first five entries are Orichalcum stacks and the last five moonstone stacks. The answer
// (`receiveResult`) empties the window: success opens `APP_SMELT_MIXJEWEL_CONFIRM` (the shining Orichalcum and `TID_GAME_MIXJEWEL_SUCCESS`),
// failure and cancel close it, a full bag prints `TID_GAME_EXTRACTION_ERROR` and keeps it. Closing the window releases every reservation.

import {
  APP_SMELT_MIXJEWEL,
  APP_SMELT_MIXJEWEL_CONFIRM,
  WIDC_OK,
  WIDC_PIC_SLOT1,
  WIDC_PIC_SLOT10,
  WIDC_PIC_SLOT2,
  WIDC_PIC_SLOT3,
  WIDC_PIC_SLOT4,
  WIDC_PIC_SLOT5,
  WIDC_PIC_SLOT6,
  WIDC_PIC_SLOT7,
  WIDC_PIC_SLOT8,
  WIDC_PIC_SLOT9,
  WIDC_START,
  WIDC_TEXT1,
  WIDC_PIC_SLOT,
} from '../../../gen/defs/resdata';
import {
  TID_GAME_EXTRACTION_ERROR,
  TID_GAME_MIXJEWEL_SUCCESS,
  TID_GAME_SUMMONANGEL_ERROR1,
  TID_GAME_SUMMONANGEL_ERROR2,
} from '../../../gen/defs/text';
import { IconStrip } from '../../controls/IconStrip';
import { RectLayer, type RectEntry } from '../../controls/RectLayer';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { type Point, type Rect, rectContains } from '../../layout/geometry';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { ANSWER_TIMEOUT_MS, MAX_JEWEL, PER_KIND } from './constants';
import { ULTIMATE_CANCEL, ULTIMATE_FAILED, ULTIMATE_INVENTORY, ULTIMATE_SUCCESS } from './results';
import type { MixJewelRequest, UltimateItem } from './ports';
import { NO_RECT, fromBag, slotEntry, toClient } from './slots';

const SLOT_IDS = [
  WIDC_PIC_SLOT1,
  WIDC_PIC_SLOT2,
  WIDC_PIC_SLOT3,
  WIDC_PIC_SLOT4,
  WIDC_PIC_SLOT5,
  WIDC_PIC_SLOT6,
  WIDC_PIC_SLOT7,
  WIDC_PIC_SLOT8,
  WIDC_PIC_SLOT9,
  WIDC_PIC_SLOT10,
] as const;
/** `RenderFillRect( rect, 0x60ffff00 )`: the slot under a dragged item. */
const HOT_FILL = 'rgba(255, 255, 0, 0.376)';

interface Slot {
  rect: Rect;
  /** The stack in the slot (`isUse` / `pItemElem`), `null` = free. */
  objId: number | null;
  icon: string | null;
}

export class WndMixJewel extends WndNeuz {
  readonly appId = APP_SMELT_MIXJEWEL;
  readonly #req: MixJewelRequest;
  readonly #getText: (tid: number) => string;
  readonly #slots: Slot[] = [];
  #strip: IconStrip | null = null;
  #hotLayer: RectLayer | null = null;
  #text: TextCtrl | null = null;
  /** `m_nOrichalcum` / `m_nMoonstone`. */
  #orichalcum = 0;
  #moonstone = 0;
  /** `m_bStart`: the request is out. */
  #started = false;
  /** `host.now()` of the send. */
  #sentAt = 0;
  /** `m_nSelectCtrl`: the slot under a dragged item, `-1` none. */
  #hot = -1;
  #confirmOpen = false;

  constructor(req: MixJewelRequest, getText: (tid: number) => string) {
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
  get started(): boolean {
    return this.#started;
  }
  get slotRects(): readonly Rect[] {
    return this.#slots.map((s) => s.rect);
  }
  /** The object id in slot `i`, `null` for a free slot. */
  slotItem(i: number): number | null {
    return this.#slots[i]?.objId ?? null;
  }
  get strip(): IconStrip | null {
    return this.#strip;
  }
  get descriptionText(): TextCtrl | null {
    return this.#text;
  }
  get hotSlot(): number {
    return this.#hot;
  }
  /** The Start button is enabled: all ten slots are filled and no request is out. */
  get startEnabled(): boolean {
    return this.getDlgItem(WIDC_START)?.isWindowEnabled() ?? false;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    for (const id of SLOT_IDS) {
      this.#slots.push({ rect: this.getWndCtrl(id)?.rect ?? NO_RECT, objId: null, icon: null });
    }
    this.#text = this.getDlgItem<TextCtrl>(WIDC_TEXT1);
    this.#text?.setString(this.#req.description);
    const client = this.getClientRect(false);
    this.#strip = this.addControl(new IconStrip(), 0x7f11, client);
    this.#hotLayer = this.addControl(new RectLayer(), 0x7f12, client);
    this.getDlgItem(WIDC_START)?.enableWindow(false);
    this.#paint();
    this.moveParentCenter();
  }

  // ---- slots --------------------------------------------------------------------------------------------------------------------

  /** `HitTest`: the slot under a client point, `-1` for none. */
  hitTest(pt: Point): number {
    return this.#slots.findIndex((s) => rectContains(s.rect, pt));
  }

  /** Whether a stack of this role belongs into slot `i` (`i < 5`: Orichalcum, else moonstone). */
  static fits(slot: number, item: UltimateItem): boolean {
    return slot < PER_KIND ? item.role === 'orichalcum' : item.role === 'moonstone';
  }

  /**
   * `OnDropIcon`: a stack from the bag onto a free slot of its kind. A full stack answers `TID_GAME_SUMMONANGEL_ERROR1`; Shift fills every free
   * slot of the five.
   */
  override onDropIcon(sc: Shortcut, ptWindow: Point): boolean {
    if (!fromBag(sc)) return false;
    const slot = this.hitTest(toClient(this, ptWindow));
    this.#hot = -1;
    const item = this.#req.item(sc.id);
    if (!item) {
      // a worn item or one the bag does not have: the forbid cursor
      this.env.wm.drag.forbid();
      return false;
    }
    const s = slot >= 0 ? this.#slots[slot] : undefined;
    if (s?.objId === null && WndMixJewel.fits(slot, item)) {
      if (item.num > item.extra) this.#place(item, slot);
      else this.#req.messageBox(TID_GAME_SUMMONANGEL_ERROR1);
    }
    this.#sync();
    return true;
  }

  /** The double click of a bag stack (rule D3 `feed`): the first free slot of its kind. */
  feed(objId: number): boolean {
    const item = this.#req.item(objId);
    if (!item || (item.role !== 'orichalcum' && item.role !== 'moonstone')) return false;
    this.#place(item, -1);
    this.#sync();
    return true;
  }

  #place(item: UltimateItem, slot: number): void {
    const req = this.#req;
    const kind = item.role === 'orichalcum' ? 0 : item.role === 'moonstone' ? PER_KIND : -1;
    if (slot >= 0) {
      const base = slot < PER_KIND ? 0 : PER_KIND;
      if (req.shiftDown()) {
        const free: number[] = [];
        for (let i = base; i < base + PER_KIND; i += 1)
          if (this.#slots[i]?.objId === null) free.push(i);
        const usable = item.num - item.extra;
        if (free.length > 0 && usable > 0) {
          const n = Math.min(free.length, usable);
          for (let k = 0; k < n; k += 1) this.#fill(free[k] ?? 0, item);
          req.setExtra(item.objId, item.extra + n);
          this.#count(base, n);
        }
      } else {
        this.#fill(slot, item);
        req.setExtra(item.objId, item.extra + 1);
        this.#count(base, 1);
      }
      return;
    }
    if (kind < 0) return;
    for (let i = kind; i < kind + PER_KIND; i += 1) {
      if (this.#slots[i]?.objId === null) {
        this.#fill(i, item);
        req.setExtra(item.objId, item.extra + 1);
        this.#count(kind, 1);
        return;
      }
    }
  }

  #fill(i: number, item: UltimateItem): void {
    const s = this.#slots[i];
    if (!s) return;
    s.objId = item.objId;
    s.icon = item.view.icon;
  }

  #count(base: number, n: number): void {
    if (base === 0) this.#orichalcum += n;
    else this.#moonstone += n;
  }

  /** `OnLButtonDblClk`: a used slot gives its unit back. */
  override onLButtonDblClk(pt: Point): void {
    const i = this.hitTest(pt);
    const s = i >= 0 ? this.#slots[i] : undefined;
    const used = s?.objId ?? null;
    if (!s || used === null) return;
    const cur = this.#req.item(used);
    if (cur) this.#req.setExtra(cur.objId, Math.max(0, cur.extra - 1));
    s.objId = null;
    s.icon = null;
    if (i < PER_KIND) this.#orichalcum -= 1;
    else this.#moonstone -= 1;
    this.#sync();
  }

  /** The slot under a dragged item is yellow (`OnDraw`: `m_nSelectCtrl = testnum` while `m_GlobalShortcut.m_dwData`). */
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

  // ---- Start --------------------------------------------------------------------------------------------------------------------

  /** The ids of the request: the stacks in slot order, each once per unit (`m_ItemInfo` walked as `extracount` copies). */
  requestIds(): number[] {
    const order: number[] = [];
    const count = new Map<number, number>();
    for (const s of this.#slots) {
      if (s.objId === null) continue;
      if (!count.has(s.objId)) order.push(s.objId);
      count.set(s.objId, (count.get(s.objId) ?? 0) + 1);
    }
    return order.flatMap((id) => Array.from({ length: count.get(id) ?? 0 }, () => id));
  }

  /** Start: fewer than ten units answer `TID_GAME_SUMMONANGEL_ERROR2`; otherwise `UltimateMakeItem` goes out. */
  start(): void {
    if (this.#orichalcum + this.#moonstone < MAX_JEWEL) {
      this.#req.messageBox(TID_GAME_SUMMONANGEL_ERROR2);
      return;
    }
    if (this.#started) return;
    this.#started = true;
    this.#sentAt = this.env.host.now();
    this.#req.make(this.requestIds());
    this.#sync();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_START) {
      this.start();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  /** `Process`: a request without an answer for `ANSWER_TIMEOUT_MS` is given up. */
  override process(now: number): void {
    super.process(now);
    if (this.#started && now - this.#sentAt >= ANSWER_TIMEOUT_MS) {
      this.#started = false;
      this.#sync();
    }
  }

  // ---- the answer ---------------------------------------------------------------------------------------------------------------

  /** `ReceiveResult`: empties the window (the units are released), then success confirms, failure and cancel close, a full bag prints. */
  receiveResult(result: number): void {
    for (const s of this.#slots) {
      if (s.objId !== null) this.#req.setExtra(s.objId, 0);
      s.objId = null;
      s.icon = null;
    }
    this.#orichalcum = 0;
    this.#moonstone = 0;
    this.#started = false;
    this.#sync();
    switch (result) {
      case ULTIMATE_SUCCESS:
        this.#openConfirm();
        break;
      case ULTIMATE_FAILED:
      case ULTIMATE_CANCEL:
        this.destroy();
        break;
      case ULTIMATE_INVENTORY:
        this.#req.say(TID_GAME_EXTRACTION_ERROR);
        break;
      default:
        break;
    }
  }

  /** `new CWndMixJewelConfirm; Initialize( this )`: one confirmation at a time. */
  #openConfirm(): void {
    if (this.#confirmOpen) return;
    const w = this.env.wm.open(
      APP_SMELT_MIXJEWEL_CONFIRM,
      () =>
        new WndMixJewelConfirm({
          text: this.#getText(TID_GAME_MIXJEWEL_SUCCESS),
          icon: this.#req.icons.shining,
          closed: () => {
            this.#confirmOpen = false;
          },
        }),
    );
    this.#confirmOpen = w !== null;
  }

  /** `OnDestroy`: every reserved unit goes back; the confirmation closes with its parent. */
  override onDestroy(): void {
    for (const s of this.#slots) if (s.objId !== null) this.#req.setExtra(s.objId, 0);
    const c = this.env.wm.get(APP_SMELT_MIXJEWEL_CONFIRM);
    if (c && this.#confirmOpen) c.destroy();
    this.#req.closed?.();
    super.onDestroy();
  }

  // ---- the pixels ---------------------------------------------------------------------------------------------------------------

  /** `OnDraw`: Start is enabled with every slot filled and no request out. */
  #sync(): void {
    const all = this.#slots.length === MAX_JEWEL && this.#slots.every((s) => s.objId !== null);
    this.getDlgItem(WIDC_START)?.enableWindow(all && !this.#started);
    this.#paint();
  }

  #paint(): void {
    const icons = this.#req.icons;
    this.#strip?.setEntries(
      this.#slots.map((s, i) =>
        s.objId !== null
          ? slotEntry(`slot${String(i)}`, s.icon, s.rect)
          : slotEntry(
              `slot${String(i)}`,
              i < PER_KIND ? icons.orichalcum : icons.moonstone,
              s.rect,
              50,
            ),
      ),
    );
    const hot: RectEntry[] = [];
    const h = this.#slots[this.#hot];
    if (h) {
      hot.push({
        key: 'hot',
        x: h.rect.l,
        y: h.rect.t,
        w: h.rect.r - h.rect.l,
        h: h.rect.b - h.rect.t,
        fill: HOT_FILL,
      });
    }
    this.#hotLayer?.setEntries(hot);
  }
}

// ---- APP_SMELT_MIXJEWEL_CONFIRM -----------------------------------------------------------------------------------------------------

export interface MixJewelConfirmDeps {
  /** `prj.GetText( TID_GAME_MIXJEWEL_SUCCESS )`. */
  readonly text: string;
  /** The shining Orichalcum drawn in `WIDC_PIC_SLOT`. */
  readonly icon: string | null;
  closed?(): void;
}

/**
 * `CWndMixJewelConfirm` (`WndField.cpp:20160-20235`): `APP_SMELT_MIXJEWEL_CONFIRM` (272 x 176), "Confirm Composing": the product (the shining
 * Orichalcum in `WIDC_PIC_SLOT`) and the success text; OK closes it.
 */
export class WndMixJewelConfirm extends WndNeuz {
  readonly appId = APP_SMELT_MIXJEWEL_CONFIRM;
  readonly #deps: MixJewelConfirmDeps;
  #strip: IconStrip | null = null;

  constructor(deps: MixJewelConfirmDeps) {
    super();
    this.#deps = deps;
    this.putRegInfo = false;
  }

  get strip(): IconStrip | null {
    return this.#strip;
  }
  get text(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT1);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.text?.setString(this.#deps.text);
    this.#strip = this.addControl(new IconStrip(), 0x7f13, this.getClientRect(false));
    const rect = this.getWndCtrl(WIDC_PIC_SLOT)?.rect ?? NO_RECT;
    this.#strip.setEntries([slotEntry('product', this.#deps.icon, rect)]);
    this.moveParentCenter();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_OK) {
      this.destroy();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#deps.closed?.();
    super.onDestroy();
  }
}
