// `VendorGrid` (`CWndVendorCtrl`, WndVendorCtrl.cpp; `__S8_VENDOR_REVISION` layout; WP-UI-23): the item list of a personal
// shop. 20 slots (`MAX_VENDOR_REVISION`) in two columns: the left icon at x = 15, the right one at x = 235, rows 36 px
// apart from y = 8, each slot a 180 x 32 hit rect whose first 32 px are the icon. Beside the icon the grid prints the item
// name (`+n` for an upgraded item, cut to 108 px with an ellipsis) at (icon + 50, +3) and the price with thousands
// separators at (icon + 50, +17) in a colour that grows with the amount. The cells carry everything the window knows (the
// vendor owner decides prices and names); the grid decides nothing:
//   * a press selects (cyan rectangle) and arms a drag of the item (the owner's own grid only: `delegate.shortcutOf`);
//   * a double click on somebody else's shop opens the purchase box (`delegate.dblClick`), a right click on the own shop
//     unregisters the item (`delegate.rightClick`);
//   * a drop on an EMPTY slot registers the item there (`delegate.drop(sc, slot)`; the shell asks for the price).
// Deviations: the 1.15 enlargement of the hovered icon is the 36 px of every `IconCell`; the stack count uses the shared
// cell count (the original prints it in blue at a hand-tuned offset).

import type { Point, Rect } from '../layout/geometry';
import type { IconCellData } from '../icons/IconCell';
import { FONT, textWidth } from '../text/fonts';
import { viewOf } from '../controls/bind';
import { ellipsize, type SlotLayout } from './geometry';
import { SlotGrid, type SlotGridDelegate } from './SlotGrid';
import { VendorGridView } from './VendorGridView';

/** `MAX_VENDOR_REVISION` and the geometry of the original (`OnDraw`, `HitTest`, `OnDropIcon`). */
export const VENDOR_SLOTS = 20;
export const VENDOR_NAME_EXTENT = 108;

export const VENDOR_LAYOUT: SlotLayout = {
  count: VENDOR_SLOTS,
  columns: 2,
  rowPitch: 36,
  origin: (slot): Point => ({ x: slot % 2 === 0 ? 15 : 235, y: Math.floor(slot / 2) * 36 + 8 }),
  hit: (slot): Rect => {
    const x = slot % 2 === 0 ? 15 : 235;
    const y = Math.floor(slot / 2) * 36 + 8;
    return { l: x, t: y, r: x + 180, b: y + 32 };
  },
};

/** One registered item of the shop. */
export interface VendorCell extends IconCellData {
  readonly name: string;
  /** `GetAbilityOption()`: shown as `name +n` when not 0. */
  readonly option?: number;
  /** The price of one piece (`m_nCost`). */
  readonly price: number;
}

/** The text of the name line: `Name +3`, cut to 108 px (`OnDrawItemInfo`). */
export function vendorNameText(cell: VendorCell): string {
  const full =
    cell.option !== undefined && cell.option !== 0
      ? `${cell.name} +${String(cell.option)}`
      : cell.name;
  return ellipsize(full, VENDOR_NAME_EXTENT, (s) => textWidth(FONT.text, s));
}

export class VendorGrid extends SlotGrid<VendorCell> {
  /** `frame = false` is the vendor window (`WBS_NODRAWFRAME`: the grid sits on the window's own picture, `PaintFrame` does not run). */
  constructor(delegate: SlotGridDelegate = {}, newFontColor = true, frame = true) {
    super(
      {
        layout: VENDOR_LAYOUT,
        frame,
        scrolls: false,
        selectColor: '#00ffff',
        selectRect: [0, 0, 32, 32],
        dropOnEmptyOnly: true,
        newFontColor,
      },
      delegate,
    );
    this.view = viewOf<SlotGrid>(VendorGridView);
  }
}
