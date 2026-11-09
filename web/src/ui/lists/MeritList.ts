// `MeritList` (`CWndGuildMeritCtrl`, WndGuildMerit.cpp; WP-UI-23): the gems a player contributes to the guild, six per row at an
// offset of (13, 7) on a 32 px pitch, with a docked scroll bar (one row per notch) when there are more rows than fit. The
// window fills it with the gem stacks of the bag (`setCells`) and reads the totals back; the grid:
//   * draws icon, stack count and the cyan selection rectangle (a cell is `(7, 11) 30 x 30` of its slot in the original; the
//     rectangle is the icon box here);
//   * arms a drag from a pressed cell only when the shell allows it (`delegate.shortcutOf` returns `null` for a cell without
//     pieces: the original needs `GetExtra() > 0`);
//   * accepts nothing dropped (`OnDropIcon` returns TRUE without acting);
//   * shows the tooltip of the hovered item.

import type { Point, Rect } from '../layout/geometry';
import { SlotGrid, type SlotGridDelegate } from './SlotGrid';
import type { SlotLayout } from './geometry';

export const MERIT_COLUMNS = 6;
/** The container of the original is a vector; the grid is sized by the slots the shell pushes. */
export const MERIT_MAX_SLOTS = 50;

/** The layout of a list of `count` merit slots. */
export function meritLayout(count: number): SlotLayout {
  return {
    count,
    columns: MERIT_COLUMNS,
    rowPitch: 32,
    origin: (slot): Point => ({
      x: (slot % MERIT_COLUMNS) * 32 + 13,
      y: Math.floor(slot / MERIT_COLUMNS) * 32 + 7,
    }),
    hit: (slot): Rect => {
      const x = (slot % MERIT_COLUMNS) * 32 + 13;
      const y = Math.floor(slot / MERIT_COLUMNS) * 32 + 7;
      return { l: x, t: y, r: x + 32, b: y + 32 };
    },
  };
}

export class MeritList extends SlotGrid {
  constructor(
    delegate: Pick<SlotGridDelegate, 'shortcutOf' | 'press' | 'hover'> = {},
    slots = MERIT_MAX_SLOTS,
    newFontColor = true,
  ) {
    super(
      {
        layout: meritLayout(slots),
        frame: false,
        scrolls: true,
        selectColor: '#00ffff',
        selectRect: [0, 0, 32, 32],
        dropOnEmptyOnly: false,
        newFontColor,
      },
      delegate,
    );
  }

  /** `OnDropIcon` returns TRUE and does nothing. */
  override onDropIcon(): boolean {
    return true;
  }
}
