// `WndItemTransy` (`CWndItemTransy`, `src:_Interface/WndChangeFace.cpp:330-575`, `__SYS_ITEMTRANSY`, ref/22 §17 L, Plan/42 §19.4 L, 45 §18.4;
// WP-UI-35 / WP-CFEAT-15): `APP_ITEM_TRANSY` (176 x 160), "Change Sex of Item": the scroll of item transy (`II_CHR_SYS_SCR_ITEMTRANSY_A` for
// armour of level 1-60, `_B` for 61-120, rule R25). Two 32 x 32 slots with an arrow between them: the armour to turn (`WIDC_STATIC1`) takes an
// inventory item by drop, the item it becomes (`WIDC_STATIC4`) is drawn by the window from the twin the request found.
//   * only an item of the inventory window is dropped on the first slot (else the forbid cursor); the request refuses a worn item, an item
//     without an opposite-gender twin and one of the wrong level band for the scroll, and says why; a second drop replaces the first;
//   * the tooltips: over the first slot the dropped item (else `TID_GAME_TRANSITEM_PUT`), over the second the twin (else `TID_GAME_TRANSITEM_PUT1`);
//   * a right click on the first slot takes the armour out again;
//   * OK sends `ItemTransy{armour, scroll, twin id, cash}` when the armour is there and closes either way; Cancel and the close button close;
//   * the scroll is reserved while the window is open, the armour while it sits in the slot; both are released when the window closes.

import {
  APP_INVENTORY,
  APP_ITEM_TRANSY,
  WIDC_CANCEL,
  WIDC_OK,
  WIDC_STATIC1,
  WIDC_STATIC4,
} from '../../../gen/defs/resdata';
import { SC, type Shortcut } from '../../dnd/shortcut';
import { type Point, type Rect, rectContains, rectOffset } from '../../layout/geometry';
import { Wnm } from '../../layout/styles';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { PaperDoll } from '../char/PaperDoll';
import type { ItemTransyRequest, TransyPair } from './ports';

export interface ItemTransyDeps {
  readonly request: ItemTransyRequest;
  readonly tooltip: TooltipService;
}

const NO_RECT: Rect = { l: 0, t: 0, r: 0, b: 0 };

export class WndItemTransy extends WndNeuz {
  readonly appId = APP_ITEM_TRANSY;
  readonly #deps: ItemTransyDeps;
  #pair: TransyPair | null = null;
  /** `m_Rect[0]` (the armour) and `m_Rect[1]` (the result). */
  #slots: [Rect, Rect] = [NO_RECT, NO_RECT];
  #icons: PaperDoll | null = null;
  #released = false;

  constructor(deps: ItemTransyDeps) {
    super();
    this.#deps = deps;
    this.putRegInfo = false;
  }

  /** The armour in the first slot and its twin, `null` for an empty slot. */
  get pair(): TransyPair | null {
    return this.#pair;
  }
  get slotRects(): readonly [Rect, Rect] {
    return this.#slots;
  }
  get iconLayer(): PaperDoll | null {
    return this.#icons;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const rect = (id: number): Rect => this.getWndCtrl(id)?.rect ?? NO_RECT;
    this.#slots = [rect(WIDC_STATIC1), rect(WIDC_STATIC4)];
    this.#icons = this.addControl(new PaperDoll(), 0x7e31, this.getClientRect(false));
    // `Init( pItemElem )`: the scroll is reserved while the window is open
    this.#deps.request.reserve(this.#deps.request.scroll.objId, true);
    this.#paint();
    this.moveParentCenter();
  }

  // ---- the slots ----------------------------------------------------------------------------------------------------------------

  /** `OnDropIcon`: an inventory item onto the first slot. */
  override onDropIcon(sc: Shortcut, ptWindow: Point): boolean {
    if (sc.kind !== SC.ITEM || (sc.from?.getFrameWnd()?.id ?? 0) !== APP_INVENTORY) return false;
    const pt: Point = {
      x: ptWindow.x - (this.rectClient.l - this.rectWindow.l),
      y: ptWindow.y - (this.rectClient.t - this.rectWindow.t),
    };
    const req = this.#deps.request;
    if (req.refuse(sc.id)) {
      this.env.wm.drag.forbid();
      return false;
    }
    if (!rectContains(this.#slots[0], pt)) return true;
    const pair = req.accept(sc.id);
    if (pair === null) {
      this.env.wm.drag.forbid();
      return false;
    }
    if (this.#pair) req.reserve(this.#pair.source.objId, false);
    req.reserve(pair.source.objId, true);
    this.#pair = pair;
    this.#paint();
    return true;
  }

  /** `OnRButtonUp` on the first slot: the armour goes back. */
  override onRButtonUp(pt: Point): void {
    if (!this.#pair || !rectContains(this.#slots[0], pt)) return;
    this.#deps.request.reserve(this.#pair.source.objId, false);
    this.#pair = null;
    this.#paint();
  }

  /** `OnDraw`: the tooltips over the two slots. */
  override onMouseMove(pt: Point): void {
    if (this.env.wm.isDraggingShortcut()) return;
    const s = this.getScreenClientRect();
    const { tips } = this.#deps.request;
    const pair = this.#pair;
    const first = this.#slots[0];
    const second = this.#slots[1];
    if (rectContains(first, pt)) {
      const at = rectOffset(first, s.l, s.t);
      if (pair) this.#deps.tooltip.putItem(this, pair.source, at, APP_ITEM_TRANSY);
      else this.#deps.tooltip.put(this, tips.putItem, at, 1);
    } else if (rectContains(second, pt)) {
      const at = rectOffset(second, s.l, s.t);
      if (pair) this.#deps.tooltip.putItem(this, pair.twin, at, APP_ITEM_TRANSY);
      else this.#deps.tooltip.put(this, tips.putScroll, at, 1);
    }
  }

  // ---- buttons ------------------------------------------------------------------------------------------------------------------

  /** OK: sends the transy when the armour is there; the window closes either way. */
  ok(): void {
    const pair = this.#pair;
    if (pair) {
      this.#deps.request.send(pair.source.objId, this.#deps.request.scroll.objId, pair.twinItemId);
    }
    this.destroy();
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

  /** `OnDestroy`: both items are released. */
  override onDestroy(): void {
    if (!this.#released) {
      this.#released = true;
      const req = this.#deps.request;
      if (this.#pair) req.reserve(this.#pair.source.objId, false);
      req.reserve(req.scroll.objId, false);
    }
    this.#deps.request.closed?.();
    super.onDestroy();
  }

  #paint(): void {
    const pair = this.#pair;
    this.#icons?.setLayers(
      pair
        ? [
            {
              key: 'source',
              x: this.#slots[0].l,
              y: this.#slots[0].t,
              size: 32,
              icon: pair.source.icon,
              alpha: 1,
              tint: false,
            },
            {
              key: 'twin',
              x: this.#slots[1].l,
              y: this.#slots[1].t,
              size: 32,
              icon: pair.twin.icon,
              alpha: 1,
              tint: false,
            },
          ]
        : [],
      true,
    );
  }
}
