// `WndCommercialElem` (`CWndCommercialElem`, `src:_Interface/WndCommItem.cpp:1201-1500`, ref/22 §17 L, Plan/45 §18.3; WP-UI-30 / WP-CFEAT-25):
// `APP_COMMERCIAL_ELEM` (176 x 176), "Pay Attribute Upgrading", opened by the NPC menu `MMI_CHANGEELEM`. Two 32 x 32 slots: the weapon or suit
// with an element (`WIDC_CUSTOM1`) and the element stone (`WIDC_CUSTOM2`). OK (`WIDC_ELEM_OK`) with both sends `CommercialElemReq{item, stone}` and
// closes the window; Cancel and the close button close it.
//   * only inventory items are dropped (else the forbid cursor), a worn item and an item the request refuses (`IsRestrictionItem`, `IsUpgradeItem`,
//     `IsSMItem`: the chat lines are the request's) also get the forbid cursor; the stone slot needs the item first;
//   * a new item in the first slot gives the stone back; a right click on the first slot empties both, on the second slot only the stone;
//   * both items are reserved (`SetExtra( 1 )`) while they sit in the slots and released when they leave or the window closes;
//   * the tooltips over the filled slots are the item tooltips.

import {
  APP_COMMERCIAL_ELEM,
  APP_INVENTORY,
  WIDC_CUSTOM1,
  WIDC_CUSTOM2,
  WIDC_ELEM_CANCEL,
  WIDC_ELEM_OK,
} from '../../../gen/defs/resdata';
import { SC, type Shortcut } from '../../dnd/shortcut';
import { type Point, type Rect, rectContains, rectOffset } from '../../layout/geometry';
import { Wnm } from '../../layout/styles';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { PaperDoll } from '../char/PaperDoll';
import type { DialogItem } from '../dialogs/ports';
import type { CommercialElemRequest } from './ports';
import { NO_RECT } from './SlotWindow';

export interface CommercialElemDeps {
  readonly request: CommercialElemRequest;
  readonly tooltip: TooltipService;
}

export class WndCommercialElem extends WndNeuz {
  readonly appId = APP_COMMERCIAL_ELEM;
  readonly #deps: CommercialElemDeps;
  #items: [DialogItem | null, DialogItem | null] = [null, null];
  #slots: [Rect, Rect] = [NO_RECT, NO_RECT];
  #icons: PaperDoll | null = null;

  constructor(deps: CommercialElemDeps) {
    super();
    this.#deps = deps;
    this.putRegInfo = false;
  }

  /** `m_pItemElem[0..1]`: the item and the stone. */
  get target(): DialogItem | null {
    return this.#items[0];
  }
  get stone(): DialogItem | null {
    return this.#items[1];
  }
  get slotRects(): readonly [Rect, Rect] {
    return this.#slots;
  }
  get iconLayer(): PaperDoll | null {
    return this.#icons;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#slots = [
      this.getWndCtrl(WIDC_CUSTOM1)?.rect ?? NO_RECT,
      this.getWndCtrl(WIDC_CUSTOM2)?.rect ?? NO_RECT,
    ];
    this.#icons = this.addControl(new PaperDoll(), 0x7e54, this.getClientRect(false));
    this.#paint();
    this.moveParentCenter();
  }

  // ---- the slots ----------------------------------------------------------------------------------------------------------------

  /** `OnDropIcon`: an inventory item onto one of the two slots. */
  override onDropIcon(sc: Shortcut, ptWindow: Point): boolean {
    if (sc.kind !== SC.ITEM || (sc.from?.getFrameWnd()?.id ?? 0) !== APP_INVENTORY) {
      this.env.wm.drag.forbid();
      return false;
    }
    const pt: Point = {
      x: ptWindow.x - (this.rectClient.l - this.rectWindow.l),
      y: ptWindow.y - (this.rectClient.t - this.rectWindow.t),
    };
    const req = this.#deps.request;
    const item = req.item(sc.id);
    const [first, second] = this.#slots;
    const [target, stone] = this.#items;
    let placed = false;
    if (item !== null && target === null && rectContains(first, pt) && req.acceptItem(item.objId)) {
      // the stone of a previous item is given back first (a stone only fits the item it was chosen for)
      if (stone) req.reserve(stone.objId, false);
      req.reserve(item.objId, true);
      this.#items = [item, null];
      placed = true;
    } else if (
      item !== null &&
      target !== null &&
      stone === null &&
      rectContains(second, pt) &&
      req.acceptStone(target.objId, item.objId)
    ) {
      req.reserve(item.objId, true);
      this.#items = [target, item];
      placed = true;
    }
    if (!placed) {
      this.env.wm.drag.forbid();
      return false;
    }
    this.#paint();
    return true;
  }

  /** `OnRButtonUp`: the first slot empties both, the second only the stone. */
  override onRButtonUp(pt: Point): void {
    if (rectContains(this.#slots[0], pt)) {
      this.#releaseItems();
      this.#paint();
    } else if (rectContains(this.#slots[1], pt) && this.#items[1]) {
      this.#deps.request.reserve(this.#items[1].objId, false);
      this.#items = [this.#items[0], null];
      this.#paint();
    }
  }

  override onMouseMove(pt: Point): void {
    if (this.env.wm.isDraggingShortcut()) return;
    const s = this.getScreenClientRect();
    for (let i = 0; i < 2; i += 1) {
      const slot = this.#slots[i];
      const item = this.#items[i];
      if (slot && item && rectContains(slot, pt)) {
        this.#deps.tooltip.putItem(this, item, rectOffset(slot, s.l, s.t), APP_COMMERCIAL_ELEM);
      }
    }
  }

  // ---- buttons ------------------------------------------------------------------------------------------------------------------

  /** OK: sends when both slots are filled and closes the window. */
  ok(): void {
    const [target, stone] = this.#items;
    if (target && stone) {
      this.#deps.request.send(target.objId, stone.objId);
      this.#releaseItems();
      this.destroy();
    }
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_ELEM_OK) {
      this.ok();
      return true;
    }
    if (msg === Wnm.CLICKED && id === WIDC_ELEM_CANCEL) {
      this.destroy();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  /** `~CWndCommercialElem` -> `InitSetting`: both reservations go. */
  override onDestroy(): void {
    this.#releaseItems();
    this.#deps.request.closed?.();
    super.onDestroy();
  }

  #releaseItems(): void {
    const req = this.#deps.request;
    for (const it of this.#items) if (it) req.reserve(it.objId, false);
    this.#items = [null, null];
  }

  #paint(): void {
    const layers = this.#items.flatMap((it, i) => {
      const slot = this.#slots[i];
      if (!it || !slot) return [];
      return [
        {
          key: `slot${String(i)}`,
          x: slot.l,
          y: slot.t,
          size: 32,
          icon: it.icon,
          alpha: 1,
          tint: false,
        },
      ];
    });
    this.#icons?.setLayers(layers, false);
  }
}
