// `TradeGrid` (`CWndTradeCtrl`, WndTradeCtrl.cpp; WP-UI-23): the 25 slots (`MAX_TRADE`, 5 x 5) of one side of the trade window
// on a 32 px pitch with the translucent frame of `PaintFrame`. A trade window holds TWO of them (the own offer and the
// partner's, which is read-only: no delegate) and the gold amounts are the shell's statics beside them. The grid:
//   * draws the stack count of a stackable item and a white selection rectangle on the selected slot;
//   * accepts a drop on an EMPTY slot and reports it (`delegate.drop(sc, slot)`: the shell decides between the quantity box
//     for a stack and `TradePut` for a single item; an expired item and an open gold box are refused there);
//   * shows the tooltip of the hovered item (`delegate.hover`).
// There is no press, drag or double click (the original handlers are empty).

import type { Point, Rect } from '../layout/geometry';
import { SlotGrid, type SlotGridDelegate } from './SlotGrid';
import type { SlotLayout } from './geometry';

export const TRADE_SLOTS = 25;
export const TRADE_COLUMNS = 5;

export const TRADE_LAYOUT: SlotLayout = {
  count: TRADE_SLOTS,
  columns: TRADE_COLUMNS,
  rowPitch: 32,
  origin: (slot): Point => ({
    x: (slot % TRADE_COLUMNS) * 32,
    y: Math.floor(slot / TRADE_COLUMNS) * 32,
  }),
  hit: (slot): Rect => {
    const x = (slot % TRADE_COLUMNS) * 32;
    const y = Math.floor(slot / TRADE_COLUMNS) * 32;
    return { l: x, t: y, r: x + 32, b: y + 32 };
  },
};

export class TradeGrid extends SlotGrid {
  /** `frame = false` is the trade window (`WBS_NODRAWFRAME`: its picture holds the slot backgrounds, `PaintFrame` does not run). */
  constructor(
    delegate: Pick<SlotGridDelegate, 'drop' | 'hover'> = {},
    newFontColor = true,
    frame = true,
  ) {
    super(
      {
        layout: TRADE_LAYOUT,
        frame,
        scrolls: false,
        selectColor: '#ffffff',
        selectRect: [0, 0, 30, 30],
        dropOnEmptyOnly: true,
        newFontColor,
      },
      delegate,
    );
  }

  /** The grid never selects by itself (the C++ handlers are empty): a press does nothing. */
  override onLButtonDown(): void {
    // intentionally empty
  }
  override onLButtonDblClk(): void {
    // intentionally empty
  }
  // eslint-disable-next-line @typescript-eslint/no-unused-vars -- the signature is the one the trade window overrides
  override onRButtonUp(_pt?: Point): void {
    // intentionally empty (the trade window overrides it: a right click on an own offer takes it back)
  }
}
