// `WndPiercing` (`CWndPiercing`, `src:_Interface/WndPiercing.cpp`, ref/22 §17 L, Plan/45 §18.6; WP-UI-31 / WP-CFEAT-26): `APP_PIERCING` (192 x 256),
// "Pierce Item", the socket window of the piercing NPC. Three 32 x 32 slots: the item (`WIDC_STATIC5`), the moonstone (`WIDC_STATIC6`) and the
// socket protection scroll (`WIDC_STATIC7`, optional); the cost (`WIDC_STATIC9`, 100 000 penya with `__EXT_PIERCING`, `0` without an item) is shown
// with thousands separators. OK (`WIDC_OK`) with the item AND the moonstone asks the OK / Cancel box `TID_PIERCING_ERROR_NOTICE`; its OK sends
// `PiercingSize{item, moonstone, protection | NULL_ID}` and closes the window; Cancel closes it. While it is open the player carries the enchant
// effect (`XI_INT_INCHANT`).
//   * only inventory items are dropped (else the forbid cursor), a worn item is refused with the forbid cursor and the chat line `TID_GAME_EQUIPPUT`
//     (the request's refusal text); the item slot takes an item the request accepts (`IsPierceAble`, a rare id), the moonstone slot only
//     a moonstone and the protection slot only the socket protection scroll, both only once the item is there;
//   * a new item empties the other two slots; a right click on the item slot empties all three, on another slot that slot only;
//   * every item is reserved while it sits in a slot (`SetExtra( 1 )`) and released when it leaves or the window closes;
//   * the tooltips over the filled slots are the item tooltips.

import {
  APP_INVENTORY,
  APP_PIERCING,
  WIDC_CANCEL,
  WIDC_OK,
  WIDC_STATIC5,
  WIDC_STATIC6,
  WIDC_STATIC7,
  WIDC_STATIC9,
} from '../../../gen/defs/resdata';
import type { Static } from '../../controls/Static';
import { SC, type Shortcut } from '../../dnd/shortcut';
import { type Point, type Rect, rectContains, rectOffset } from '../../layout/geometry';
import { WSS_MONEY, Wnm } from '../../layout/styles';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { PaperDoll } from '../char/PaperDoll';
import type { DialogItem } from '../dialogs/ports';
import type { PiercingRequest } from './ports';
import { NO_RECT } from './SlotWindow';

export interface PiercingDeps {
  readonly request: PiercingRequest;
  readonly tooltip: TooltipService;
}

type Slots3 = [DialogItem | null, DialogItem | null, DialogItem | null];
const SLOT_IDS = [WIDC_STATIC5, WIDC_STATIC6, WIDC_STATIC7] as const;

export class WndPiercing extends WndNeuz {
  readonly appId = APP_PIERCING;
  readonly #deps: PiercingDeps;
  #items: Slots3 = [null, null, null];
  #slots: [Rect, Rect, Rect] = [NO_RECT, NO_RECT, NO_RECT];
  #icons: PaperDoll | null = null;
  #removeSfx: (() => void) | null = null;

  constructor(deps: PiercingDeps) {
    super();
    this.#deps = deps;
    this.putRegInfo = false;
  }

  /** `m_pItemElem[0..2]`: the item, the moonstone, the protection scroll. */
  get items(): Readonly<Slots3> {
    return this.#items;
  }
  get slotRects(): readonly [Rect, Rect, Rect] {
    return this.#slots;
  }
  get iconLayer(): PaperDoll | null {
    return this.#icons;
  }
  /** The cost text under the slots. */
  get costText(): string {
    return this.getDlgItem<Static>(WIDC_STATIC9)?.getTitle() ?? '';
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#slots = [
      this.getWndCtrl(SLOT_IDS[0])?.rect ?? NO_RECT,
      this.getWndCtrl(SLOT_IDS[1])?.rect ?? NO_RECT,
      this.getWndCtrl(SLOT_IDS[2])?.rect ?? NO_RECT,
    ];
    this.getDlgItem<Static>(WIDC_STATIC9)?.addWndStyle(WSS_MONEY);
    this.#icons = this.addControl(new PaperDoll(), 0x7e55, this.getClientRect(false));
    this.#removeSfx = this.#deps.request.sfx();
    this.#refresh();
    this.moveParentCenter();
  }

  // ---- the slots ----------------------------------------------------------------------------------------------------------------

  /** `OnDropIcon`: an inventory item onto one of the three slots. */
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
    if (item === null) return false;
    const slot = this.#slots.findIndex((r) => rectContains(r, pt));
    if (slot < 0) return true;
    const target = this.#items[0];
    // the moonstone and the scroll need the item first (the original ignores the drop without it)
    if (slot > 0 && target === null) return true;
    if (!req.accept(slot as 0 | 1 | 2, item.objId, target)) {
      this.env.wm.drag.forbid();
      return false;
    }
    const next: Slots3 = [...this.#items];
    const old = next[slot];
    if (old) req.reserve(old.objId, false);
    req.reserve(item.objId, true);
    next[slot] = item;
    if (slot === 0) {
      // a new item: the moonstone and the scroll of the old one go back
      for (const i of [1, 2] as const) {
        const prev = next[i];
        if (prev) req.reserve(prev.objId, false);
        next[i] = null;
      }
    }
    this.#items = next;
    this.#refresh();
    return true;
  }

  /** `OnRButtonUp`: the item slot empties all three, another slot only itself. */
  override onRButtonUp(pt: Point): void {
    const req = this.#deps.request;
    const slot = this.#slots.findIndex((r) => rectContains(r, pt));
    if (slot < 0 || this.#items[slot] === null) return;
    const next: Slots3 = [...this.#items];
    const clear = slot === 0 ? [0, 1, 2] : [slot];
    for (const i of clear) {
      const it = next[i];
      if (it) req.reserve(it.objId, false);
      next[i] = null;
    }
    this.#items = next;
    this.#refresh();
  }

  override onMouseMove(pt: Point): void {
    if (this.env.wm.isDraggingShortcut()) return;
    const s = this.getScreenClientRect();
    for (let i = 0; i < 3; i += 1) {
      const slot = this.#slots[i];
      const item = this.#items[i];
      if (slot && item && rectContains(slot, pt)) {
        this.#deps.tooltip.putItem(this, item, rectOffset(slot, s.l, s.t), APP_PIERCING);
      }
    }
  }

  // ---- buttons ------------------------------------------------------------------------------------------------------------------

  /** OK: the box asks first; its OK sends and closes the window. */
  ok(): void {
    const [target, moonstone, protection] = this.#items;
    if (!target || !moonstone) return;
    const req = this.#deps.request;
    req.ask(() => {
      req.send(target.objId, moonstone.objId, protection?.objId ?? null);
      this.destroy();
    });
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

  /** `OnDestroy` / the destructor: every item is released and the effect goes. */
  override onDestroy(): void {
    const req = this.#deps.request;
    for (const it of this.#items) if (it) req.reserve(it.objId, false);
    this.#items = [null, null, null];
    this.#removeSfx?.();
    this.#removeSfx = null;
    req.closed?.();
    super.onDestroy();
  }

  #refresh(): void {
    const cost = this.#deps.request.cost(this.#items[0]);
    this.getDlgItem<Static>(WIDC_STATIC9)?.setTitle(String(cost));
    this.#icons?.setLayers(
      this.#items.flatMap((it, i) => {
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
      }),
      false,
    );
  }
}
