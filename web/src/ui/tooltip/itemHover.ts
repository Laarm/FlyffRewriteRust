// The binding between an item cell and the item tooltip (WP-UI-20, `CWndItemCtrl::OnMouseWndSurface` / `UpdateTooltip`,
// WndItemCtrl.cpp:65-250): the grid reports the hovered slot and its 30 x 30 rect, this adapter turns that into
// `tooltip.putItem(owner, item, rect, flag)`; leaving the cells cancels it. Equipment slots of the inventory use the same
// adapter with their own rect (`hoverRect`).

import type { Rect } from '../layout/geometry';
import type { ItemTooltipCtx, ItemView } from './types';
import type { TooltipService } from './TooltipService';

export interface ItemHoverOptions {
  readonly tooltip: TooltipService;
  /** The window or control that owns the tooltip (it disappears when the pointer leaves the rect or the owner closes). */
  readonly owner: object;
  /** The APP id of the source window (`APP_INVENTORY`, `APP_SHOPEX`, ...). */
  readonly flag: number | (() => number);
  /** The item in a slot (`null` = nothing to show). */
  readonly itemAt: (slot: number) => ItemView | null;
  /** More context per hover (the shop, the owner, the equipment version, the worn-item lookup). */
  readonly ctx?: () => Partial<ItemTooltipCtx>;
}

/** An `ItemGridDelegate.hover` implementation. */
export function itemHover(o: ItemHoverOptions): (slot: number | null, rect: Rect | null) => void {
  return (slot, rect) => {
    if (slot === null || rect === null) {
      o.tooltip.cancel();
      return;
    }
    const item = o.itemAt(slot);
    if (!item) {
      o.tooltip.cancel();
      return;
    }
    const flag = typeof o.flag === 'function' ? o.flag() : o.flag;
    o.tooltip.putItem(o.owner, item, rect, flag, o.ctx?.() ?? {});
  };
}
