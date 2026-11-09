// `WndInvenRemoveItem` (`CWndInvenRemoveItem`, `src:_Interface/WndInvenRemoveItem.cpp`, ref/22 §17 D, Plan/42 §19.4 D, 45 §9.4;
// WP-UI-22): `APP_INVEN_REMOVE_ITEM` (240 x 128, MODAL), "really delete this item?" after an item was dropped on the trash
// slot: the item icon (`WIDC_CUSTOM1`, with the item tooltip), a quantity edit that starts at the whole stack (disabled for a
// single item) and OK / Cancel. OK checks the digits and `0 < n <= count`; otherwise the edit is reset to the whole stack and
// the error text is printed.

import {
  APP_INVEN_REMOVE_ITEM,
  WIDC_CANCEL,
  WIDC_CUSTOM1,
  WIDC_EDIT1,
  WIDC_OK,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { type Point, type Rect, rectContains, rectOffset } from '../../layout/geometry';
import { WBS_MODAL, Wnm } from '../../layout/styles';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { PaperDoll } from '../char/PaperDoll';
import type { InvenRemoveRequest } from './ports';

export interface InvenRemoveDeps {
  readonly request: InvenRemoveRequest;
  readonly tooltip: TooltipService;
}

export class WndInvenRemoveItem extends WndNeuz {
  readonly appId = APP_INVEN_REMOVE_ITEM;
  readonly #deps: InvenRemoveDeps;
  #edit: EditCtrl | null = null;
  #iconRect: Rect | null = null;
  #icon: PaperDoll | null = null;

  constructor(deps: InvenRemoveDeps) {
    super();
    this.#deps = deps;
    this.putRegInfo = false;
  }

  /** `InitDialog(APP_INVEN_REMOVE_ITEM, WBS_MODAL)`. */
  override initialize(parent: WndBase, extraStyle = 0): boolean {
    return super.initialize(parent, WBS_MODAL | extraStyle);
  }

  get quantityEdit(): EditCtrl | null {
    return this.#edit;
  }
  get iconLayer(): PaperDoll | null {
    return this.#icon;
  }
  get okButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_OK);
  }
  get cancelButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_CANCEL);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const item = this.#deps.request.item;
    this.#edit = this.getDlgItem<EditCtrl>(WIDC_EDIT1);
    this.#edit?.setString(String(item.count));
    if (item.count <= 1) this.#edit?.enableWindow(false);
    else this.#edit?.setFocus();
    const r = this.getWndCtrl(WIDC_CUSTOM1)?.rect ?? null;
    this.#iconRect = r;
    if (r) {
      const icon = this.addControl(new PaperDoll(), 0x7d01, this.getClientRect(false));
      icon.setLayers(
        [
          {
            key: 'item',
            x: r.l,
            y: r.t,
            size: 32,
            icon: item.icon,
            alpha: 1,
            tint: false,
          },
        ],
        true,
      );
      this.#icon = icon;
    }
    // `MoveParentCenter` wins over the position next to the right edge
    this.moveParentCenter();
  }

  /** `OnMouseWndSurface`: the item tooltip over the icon. */
  override onMouseMove(pt: Point): void {
    const r = this.#iconRect;
    if (!r || !rectContains(r, pt) || this.env.wm.isDraggingShortcut()) return;
    const s = this.getScreenClientRect();
    this.#deps.tooltip.putItem(
      this,
      this.#deps.request.item,
      rectOffset(r, s.l, s.t),
      APP_INVEN_REMOVE_ITEM,
    );
  }

  /** `OnButtonOK`: the typed quantity if it is a number in `1..count`, else `null` (the edit is reset to the whole stack). */
  #checkQuantity(): number | null {
    const edit = this.#edit;
    const count = this.#deps.request.item.count;
    const text = edit?.getString() ?? '';
    const ok = /^[0-9]+$/.test(text) && Number(text) > 0 && Number(text) <= count;
    if (!ok) {
      edit?.setString(String(count));
      return null;
    }
    return Number(text);
  }

  /** OK: sends the removal for a valid quantity and closes; an invalid one prints the error and stays. */
  ok(): void {
    const n = this.#checkQuantity();
    if (n === null) {
      this.#deps.request.invalid?.();
      return;
    }
    this.#deps.request.confirm(n);
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
}
