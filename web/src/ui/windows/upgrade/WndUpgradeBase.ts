// `WndUpgradeBase` (`CWndUpgradeBase`, `src:_Interface/WndUpgradeBase.cpp`, ref/22 §17 L, Plan/42 §19.4 L; WP-UI-29): `APP_TEST` (240 x 280), the old
// supplement-stone upgrade window. Six 32 x 32 slots: the weapon or armour (`WIDC_CUSTOM3`), the card or dice (`WIDC_CUSTOM4`), the first supplement stone
// twice (`WIDC_CUSTOM1`, `WIDC_CUSTOM5`) and the second one twice (`WIDC_CUSTOM2`, `WIDC_CUSTOM6`). The shell keeps the book-keeping of the original
// (what sits where, how many units each stack reserved, the statics that follow the slots) and asks the request what may go where; OK with the item and the
// card in place sends and closes the window (a request that refuses keeps it open); Cancel closes it.
//   * only inventory items are dropped (else the forbid cursor), a worn item is refused with the forbid cursor and the chat line of the request;
//   * a right click on the item slot empties all six, on another slot that slot only; the stacks are reserved by the units the request gave;
//   * the window is NOT part of the shipped game (`MMI_UPGRADE` is dropped, 45 OQ6; the original handler is empty): nothing opens it, the shell is here for the
//     catalog and keeps its behaviour tested.

import {
  APP_INVENTORY,
  APP_TEST,
  WIDC_CANCEL,
  WIDC_CUSTOM1,
  WIDC_CUSTOM2,
  WIDC_CUSTOM3,
  WIDC_CUSTOM4,
  WIDC_CUSTOM5,
  WIDC_CUSTOM6,
  WIDC_OK,
  WIDC_STATIC10,
  WIDC_STATIC11,
  WIDC_STATIC4,
  WIDC_STATIC5,
  WIDC_STATIC6,
} from '../../../gen/defs/resdata';
import type { Static } from '../../controls/Static';
import { SC, type Shortcut } from '../../dnd/shortcut';
import { type Point, type Rect, rectContains, rectOffset } from '../../layout/geometry';
import { Wnm } from '../../layout/styles';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { PaperDoll } from '../char/PaperDoll';
import type { DialogItem } from '../dialogs/ports';
import type { UpgradeBaseRequest, UpgradeBaseSlot } from './ports';
import { NO_RECT } from './SlotWindow';

export interface UpgradeBaseDeps {
  readonly request: UpgradeBaseRequest;
  readonly tooltip: TooltipService;
}

/** `m_Rect[0..5]`: the controls of the six slots, in the order of the original. */
const SLOT_IDS = [
  WIDC_CUSTOM3,
  WIDC_CUSTOM4,
  WIDC_CUSTOM1,
  WIDC_CUSTOM2,
  WIDC_CUSTOM5,
  WIDC_CUSTOM6,
] as const;
const SLOTS: readonly UpgradeBaseSlot[] = [0, 1, 2, 3, 4, 5];

export class WndUpgradeBase extends WndNeuz {
  readonly appId = APP_TEST;
  readonly #deps: UpgradeBaseDeps;
  #rects: Rect[] = SLOT_IDS.map(() => NO_RECT);
  /** `m_pItemElem[i]`. */
  #items: (DialogItem | null)[] = SLOTS.map(() => null);
  /** The units each slot reserved (`SetExtra( n )`). */
  #extras: number[] = SLOTS.map(() => 0);
  #icons: PaperDoll | null = null;

  constructor(deps: UpgradeBaseDeps) {
    super();
    this.#deps = deps;
    this.putRegInfo = false;
  }

  get items(): readonly (DialogItem | null)[] {
    return this.#items;
  }
  get extras(): readonly number[] {
    return this.#extras;
  }
  get slotRects(): readonly Rect[] {
    return this.#rects;
  }
  get iconLayer(): PaperDoll | null {
    return this.#icons;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#rects = SLOT_IDS.map((id) => this.getWndCtrl(id)?.rect ?? NO_RECT);
    this.#icons = this.addControl(new PaperDoll(), 0x7e59, this.getClientRect(false));
    this.#refresh();
    this.#placeBesideInventory();
  }

  /** Beside the inventory like the other item windows (the right edge of the inventory, or its left when it sits right of the middle). */
  #placeBesideInventory(): void {
    const inv = this.env.wm.get(APP_INVENTORY);
    if (!inv) {
      this.moveParentCenter();
      return;
    }
    const root = this.env.wm.root.rectWindow;
    const iw = inv.rectWindow;
    const w = this.rectWindow.r - this.rectWindow.l;
    const x = iw.l > (root.r - root.l) / 2 ? iw.l - w : iw.l + (iw.r - iw.l);
    this.move({ x, y: iw.t });
  }

  // ---- the slots ----------------------------------------------------------------------------------------------------------------

  /** `OnDropIcon`. */
  override onDropIcon(sc: Shortcut, ptWindow: Point): boolean {
    if (sc.kind !== SC.ITEM || (sc.from?.getFrameWnd()?.id ?? 0) !== APP_INVENTORY) {
      this.env.wm.drag.forbid();
      return false;
    }
    const pt: Point = {
      x: ptWindow.x - (this.rectClient.l - this.rectWindow.l),
      y: ptWindow.y - (this.rectClient.t - this.rectWindow.t),
    };
    const slot = this.#rects.findIndex((r) => rectContains(r, pt));
    if (slot < 0) return true;
    const s = SLOTS[slot];
    if (s === undefined) return true;
    const req = this.#deps.request;
    const item = req.item(sc.id);
    if (item === null || this.#items[slot] !== null) return false;
    const ids = this.#items.map((it) => it?.objId ?? null);
    const extra = req.accept(s, item.objId, ids);
    if (extra <= 0) {
      this.env.wm.drag.forbid();
      return false;
    }
    req.setExtra(item.objId, extra);
    this.#items[slot] = item;
    this.#extras[slot] = extra;
    this.#refresh();
    return true;
  }

  /** `OnRButtonUp`: the item slot empties all six, another slot only itself. */
  override onRButtonUp(pt: Point): void {
    const slot = this.#rects.findIndex((r) => rectContains(r, pt));
    if (slot < 0 || this.#items[slot] === null) return;
    if (slot === 0) this.#releaseAll();
    else this.#release(slot);
    this.#refresh();
  }

  override onMouseMove(pt: Point): void {
    if (this.env.wm.isDraggingShortcut()) return;
    const s = this.getScreenClientRect();
    for (let i = 0; i < this.#rects.length; i += 1) {
      const r = this.#rects[i];
      const it = this.#items[i];
      if (r && it && rectContains(r, pt)) {
        this.#deps.tooltip.putItem(this, it, rectOffset(r, s.l, s.t), APP_TEST);
      }
    }
  }

  // ---- buttons ------------------------------------------------------------------------------------------------------------------

  /** OK: item and card in place -> send; the window closes when the request took it. */
  ok(): void {
    if (this.#items[0] === null || this.#items[1] === null) return;
    const ids = this.#items.map((it) => it?.objId ?? null);
    if (this.#deps.request.send(ids, this.#extras)) {
      this.#releaseAll();
      this.destroy();
    }
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_OK) {
      this.ok();
      return true;
    }
    if (msg === Wnm.CLICKED && id === WIDC_CANCEL) {
      this.destroy();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  /** `OnDestroy`: every stack is released. */
  override onDestroy(): void {
    this.#releaseAll();
    this.#deps.request.closed?.();
    super.onDestroy();
  }

  #release(slot: number): void {
    const it = this.#items[slot];
    if (it) this.#deps.request.setExtra(it.objId, 0);
    this.#items[slot] = null;
    this.#extras[slot] = 0;
  }

  #releaseAll(): void {
    for (let i = 0; i < this.#items.length; i += 1) this.#release(i);
  }

  #refresh(): void {
    const ids = this.#items.map((it) => it?.objId ?? null);
    const info = this.#deps.request.info(ids);
    const set = (id: number, text: string): void => {
      this.getDlgItem<Static>(id)?.setTitle(text);
    };
    set(WIDC_STATIC5, info.cost);
    set(WIDC_STATIC6, info.count0);
    set(WIDC_STATIC4, info.count1);
    set(WIDC_STATIC10, info.name0);
    set(WIDC_STATIC11, info.name1);
    this.#icons?.setLayers(
      this.#items.flatMap((it, i) => {
        const r = this.#rects[i];
        if (!it || !r) return [];
        const extra = this.#extras[i] ?? 0;
        return [
          {
            key: `slot${String(i)}`,
            x: r.l,
            y: r.t,
            size: 32,
            icon: it.icon,
            alpha: 1,
            tint: false,
            // `m_nItemNum > 1`: the reserved units at (right - 11, bottom - 11)
            ...(it.count > 1 ? { count: { text: String(extra), right: r.r, top: r.b - 11 } } : {}),
          },
        ];
      }),
      false,
    );
  }
}
