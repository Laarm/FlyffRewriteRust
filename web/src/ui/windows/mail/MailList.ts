// `MailList` (the inbox of `CWndPostReceive`, `src:_Interface/WndField.cpp:15531-15931`; WP-UI-27): six rows of 40 px from y = 26 of the page, over the
// table picture `WndPostTable.tga` (alpha 150 at (13, 26)). A row shows the attached item (or the penya coin) at (19, +6) with the stack count, the "read"
// mark over it, the sender (black, (69, +7)), the time that is left (a colour from red to green, (199, +7), 3 px further right while days are left),
// and the title in blue ((69, +22)). A press selects, a double click opens the mail, the wheel scrolls a row, the pointer shows the date of arrival.
// The docked scroll bar sits at the right edge of the page. The rows are plain data the window pushes (`setRows`).

import { viewOf } from '../../controls/bind';
import type { Point, Rect } from '../../layout/geometry';
import { RowList, type RowListState } from '../../lists/RowList';
import { MailListView } from './MailListView';
import type { MailRow } from './ports';

/** The geometry of the original: the first row band starts at `sy - 4 = 26`, rows are 40 px apart, six of them make a page. */
export const MAIL_ROW_TOP = 26;
export const MAIL_ROW_PITCH = 40;
/** The row band: `sx + 4 = 13` to `sx + 240 = 249`. */
export const MAIL_ROW_LEFT = 13;
export const MAIL_ROW_RIGHT = 249;
/** The picture behind the rows. */
export const MAIL_TABLE_TEXTURE = 'WndPostTable.tga';
export const MAIL_TABLE_ALPHA = 150 / 255;
/** `WndNotUse.tga` over the icon of a mail that was read (alpha 110). */
export const MAIL_READ_TEXTURE = 'WndNotUse.tga';
export const MAIL_READ_ALPHA = 110 / 255;
/** `itm_GolGolSeed.dds`: the picture of a mail that carries penya only. */
export const MAIL_GOLD_ICON = 'item/itm_golgolseed.dds';

export interface MailListDelegate {
  /** A press selected a row (`OnLButtonDown`). */
  select?(row: MailRow): void;
  /** A double click on a row (`OnLButtonDblClk`): the reader opens. */
  open?(row: MailRow): void;
  /** The pointer is over a row (or left them: `null`): put or cancel the tooltip with the date of arrival; `rect` is in root coordinates. */
  hover?(row: MailRow | null, rect: Rect | null): void;
}

export interface MailListState extends RowListState<MailRow> {
  /** `features.new_font_color`: the colour of the stack count. */
  newFontColor: boolean;
}

export class MailList extends RowList<MailRow, MailListState> {
  #delegate: MailListDelegate;
  #selectedId = -1;

  constructor(delegate: MailListDelegate = {}) {
    super(
      { pitch: MAIL_ROW_PITCH, top: MAIL_ROW_TOP, inset: 0, bar: true, frame: false },
      {
        rows: [],
        hover: -1,
        selected: -1,
        scroll: 0,
        page: 1,
        clientOffset: { x: 0, y: 0 },
        newFontColor: true,
      },
    );
    this.#delegate = delegate;
    this.view = viewOf<MailList>(MailListView);
  }

  setDelegate(delegate: MailListDelegate): void {
    this.#delegate = delegate;
  }

  setNewFontColor(on: boolean): void {
    this.state.patch({ newFontColor: on });
  }

  /** The mail id of the selected row (`m_nSelect`), `-1` for none. The selection follows the MAIL, not the position, when the box changes. */
  get selectedMailId(): number {
    return this.#selectedId;
  }

  /** Pushes the rows; an equal list (same rows by identity) changes nothing. */
  setRows(rows: readonly MailRow[]): void {
    const cur = this.rows;
    if (cur.length === rows.length && cur.every((r, i) => r === rows[i])) return;
    const selected = rows.findIndex((r) => r.id === this.#selectedId);
    this.setRowsState(rows, { selected });
    if (selected < 0) this.#selectedId = -1;
  }

  /** The band of a row: the hover and the click rect (x from 13 to 249). */
  override rowRect(row: number): Rect | null {
    const r = super.rowRect(row);
    return r ? { l: MAIL_ROW_LEFT, t: r.t, r: MAIL_ROW_RIGHT, b: r.b } : null;
  }

  #rowAtBand(pt: Point): number {
    if (pt.x < MAIL_ROW_LEFT || pt.x >= MAIL_ROW_RIGHT) return -1;
    return this.rowAt(pt);
  }

  override onLButtonDown(pt: Point): void {
    const i = this.#rowAtBand(pt);
    const row = i >= 0 ? (this.rows[i] ?? null) : null;
    if (!row) return;
    this.#selectedId = row.id;
    this.select(i);
    this.#delegate.select?.(row);
  }

  override onLButtonDblClk(pt: Point): void {
    const i = this.#rowAtBand(pt);
    const row = i >= 0 ? (this.rows[i] ?? null) : null;
    if (!row) return;
    this.#selectedId = row.id;
    this.select(i);
    this.#delegate.open?.(row);
  }

  override onMouseMove(pt: Point): void {
    const i = this.#rowAtBand(pt);
    if (i === this.state.get().hover) return;
    this.state.patch({ hover: i });
    const row = i >= 0 ? (this.rows[i] ?? null) : null;
    this.#delegate.hover?.(row, row ? this.rowScreenRect(i) : null);
  }

  override onMouseLeave(): void {
    if (this.state.get().hover >= 0) {
      this.state.patch({ hover: -1 });
      this.#delegate.hover?.(null, null);
    }
  }
}
