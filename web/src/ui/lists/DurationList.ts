// `DurationList` (`CWndCommItemCtrl`, WndCommItem.cpp; WP-UI-23): the premium items and scroll effects that are active, one
// 30 px row each from y = 3 of the client (deflated by 3; the docked bar takes 15 px): the 32 x 32 icon at x = 2 at alpha
// 192/255, the name in blue (`#cff0000ff`) at (40, +3) and under it, at (40, +18), the remaining time in black
// (`TID_TOOLTIP_DATE` filled with days, hours, minutes, seconds; `TID_GAME_COND_USE` for an item that lasts until a
// condition). The list is display-only in the original (no click, no selection): it shows the item tooltip over the hovered
// row (`delegate.hover`, rect = the 32 px band) and scrolls one row per wheel notch. The window pushes the rows and the
// finished text of the second line (`formatDuration(template, seconds)` fills the template), once a second.

import type { Point, Rect } from '../layout/geometry';
import { viewOf } from '../controls/bind';
import { DurationListView } from './DurationListView';
import { RowList } from './RowList';

export interface DurationRow {
  /** A unique key (React key, hover identity). */
  readonly key: string | number;
  readonly icon: string | null;
  /** The item name (drawn in blue). */
  readonly name: string;
  /** The second line: the remaining time or the condition text. */
  readonly text: string;
}

export interface DurationListDelegate {
  /** The pointer entered a row (or left: `null`): put or cancel the item tooltip over `rect` (screen). */
  hover?(row: DurationRow | null, rect: Rect | null): void;
}

export const DURATION_ROW_PITCH = 30;
export const DURATION_ROW_TOP = 3;

export class DurationList extends RowList<DurationRow> {
  #delegate: DurationListDelegate;

  constructor(delegate: DurationListDelegate = {}) {
    super(
      { pitch: DURATION_ROW_PITCH, top: DURATION_ROW_TOP, inset: 3, bar: true, frame: false },
      {
        rows: [],
        hover: -1,
        selected: -1,
        scroll: 0,
        page: 1,
        clientOffset: { x: 0, y: 0 },
      },
    );
    this.#delegate = delegate;
    this.view = viewOf<DurationList>(DurationListView);
  }

  setDelegate(delegate: DurationListDelegate): void {
    this.#delegate = delegate;
  }

  /** Pushes the rows; an equal list (same keys, names, icons and texts) changes nothing. */
  setRows(rows: readonly DurationRow[]): void {
    const cur = this.rows;
    if (
      cur.length === rows.length &&
      cur.every((r, i) => {
        const n = rows[i];
        return n?.key === r.key && n.icon === r.icon && n.name === r.name && n.text === r.text;
      })
    )
      return;
    this.setRowsState(rows);
  }

  /** The hover rect: the 32 px icon band of the row, the full client width (`CRect( x + 3, y, +nWidth, y + 32 )`). */
  override rowRect(row: number): Rect | null {
    const r = super.rowRect(row);
    return r ? { l: r.l, t: r.t, r: r.r, b: r.t + 32 } : null;
  }

  override onMouseMove(pt: Point): void {
    const i = this.rowAt(pt);
    const r = i >= 0 ? this.rowRect(i) : null;
    const next = r && pt.y >= r.t && pt.y < r.b ? i : -1;
    if (next === this.state.get().hover) return;
    this.state.patch({ hover: next });
    const row = next >= 0 ? (this.rows[next] ?? null) : null;
    this.#delegate.hover?.(row, row ? this.rowScreenRect(next) : null);
  }

  override onMouseLeave(): void {
    if (this.state.get().hover >= 0) {
      this.state.patch({ hover: -1 });
      this.#delegate.hover?.(null, null);
    }
  }
}
