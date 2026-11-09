// `RowList`: the base of the specialised vertical lists (WP-UI-23): the skill list (`CWndSkillCtrl`), the premium duration
// list (`CWndCommItemCtrl`) and the member lists (party, guild, friend). They share what the originals copy-pasted: a client
// rect deflated from the window, a docked 15 px scroll bar whose range is the row count and whose page is the rows that fit,
// rows of a fixed pitch from a fixed top, a hovered and a selected row, the wheel scrolling one row per notch and a
// translucent `PaintFrame` behind everything. The rows are plain data the window pushes (`setRows`); the list never looks
// anything up. Subclasses add their pointer behaviour and their row view.

import {
  type FrameRects,
  type Point,
  type Rect,
  rectHeight,
  rectOffset,
  rectWidth,
} from '../layout/geometry';
import { WTYPE_CUSTOM } from '../layout/styles';
import { createOwnerScrollBar, type ScrollBar } from '../controls/ScrollBar';
import { Control, type ControlViewState } from '../wnd/Control';
import { rowAtY } from './geometry';

export interface RowListGeometry {
  /** The distance of two rows. */
  readonly pitch: number;
  /** The y of the first row in client coordinates (the original starts at 3 or 0), below the header. */
  readonly top: number;
  /** The window is deflated by this on every side to get the client rect. */
  readonly inset: number;
  /** A docked scroll bar takes 15 px of the client width. */
  readonly bar: boolean;
  /** `PaintFrame`: the translucent frame behind the rows. */
  readonly frame: boolean;
  /** A header strip above the rows (the member list's sort buttons). */
  readonly header?: number;
}

export interface RowListState<R> extends ControlViewState {
  rows: readonly R[];
  /** The hovered row, `-1` = none. */
  hover: number;
  /** The selected row (`m_nCurSelect`), `-1` = none. */
  selected: number;
  /** The first visible row. */
  scroll: number;
  /** The rows that fit. */
  page: number;
  clientOffset: Point;
}

export class RowList<R, S extends RowListState<R> = RowListState<R>> extends Control<S> {
  readonly geometry: RowListGeometry;
  #bar: ScrollBar | null = null;

  constructor(geometry: RowListGeometry, extra: Omit<S, keyof ControlViewState>) {
    super(WTYPE_CUSTOM, null, extra);
    this.geometry = geometry;
  }

  /** `SetWndRect`: the client is the window deflated by `inset`, minus the bar. */
  protected override computeRects(window: Rect): FrameRects {
    const i = this.geometry.inset;
    const client: Rect = { l: window.l + i, t: window.t + i, r: window.r - i, b: window.b - i };
    if (this.geometry.bar) client.r -= 15;
    return { window: { ...window }, client, layout: { ...client } };
  }

  override onInitialUpdate(): void {
    if (this.geometry.bar) {
      const bar = createOwnerScrollBar(this);
      bar.onScroll = (pos) => {
        this.#setScroll(pos);
      };
      bar.forwardWheel = (n) => {
        this.onMouseWheel(n);
      };
      this.#bar = bar;
    }
    this.relayout();
  }

  override onSize(): void {
    this.#bar?.setWndRect({
      l: rectWidth(this.rectWindowValue) - 15,
      t: 0,
      r: rectWidth(this.rectWindowValue),
      b: rectHeight(this.rectWindowValue),
    });
    this.relayout();
  }

  protected override onRectChanged(): void {
    super.onRectChanged();
    const o = {
      x: this.rectClientValue.l - this.rectWindowValue.l,
      y: this.rectClientValue.t - this.rectWindowValue.t,
    };
    const cur = this.state.get().clientOffset;
    if (cur.x !== o.x || cur.y !== o.y) this.state.patch({ clientOffset: o } as Partial<S>);
  }

  get scrollBar(): ScrollBar | null {
    return this.#bar;
  }

  // ---- the model ---------------------------------------------------------------------------------------------

  get rows(): readonly R[] {
    return this.state.get().rows;
  }
  get rowCount(): number {
    return this.state.get().rows.length;
  }
  get selectedRow(): number {
    return this.state.get().selected;
  }
  get hoverRow(): number {
    return this.state.get().hover;
  }
  get scrollRow(): number {
    return this.state.get().scroll;
  }
  get visibleRows(): number {
    return this.state.get().page;
  }

  /** The y of the first row, below the header. */
  get rowsTop(): number {
    return this.geometry.top + (this.geometry.header ?? 0);
  }

  /** Pushes a new row array: the selection and the hover are clamped to it, the scroll range follows. */
  protected setRowsState(rows: readonly R[], extra: Partial<S> = {}): void {
    const s = this.state.get();
    const selected = s.selected < rows.length ? s.selected : -1;
    const hover = s.hover < rows.length ? s.hover : -1;
    this.state.patch({ rows, selected, hover, ...extra });
    this.relayout();
  }

  select(row: number): void {
    this.state.patch({ selected: row } as Partial<S>);
  }

  /** Scrolls so that `row` is the first visible row (clamped). */
  scrollTo(row: number): void {
    this.#bar?.setScrollPos(this.#clamp(row));
    if (!this.#bar) this.#setScroll(row);
  }

  #clamp(row: number): number {
    const s = this.state.get();
    const max = Math.max(0, s.rows.length - s.page);
    return Math.max(0, Math.min(max, row));
  }

  #setScroll(pos: number): void {
    this.state.patch({ scroll: this.#clamp(pos) } as Partial<S>);
  }

  /** Recomputes the page and the scroll bar after a size or content change (`PaintFrame` does it every frame in the C++). */
  protected relayout(): void {
    const h = rectHeight(this.rectClientValue) - this.rowsTop;
    const page = Math.max(1, Math.floor(h / this.geometry.pitch));
    this.state.patch({ page } as Partial<S>);
    const bar = this.#bar;
    if (bar) {
      const n = this.state.get().rows.length;
      bar.setScrollRange(0, n);
      bar.setScrollPage(page);
      bar.setVisible(n > page);
    }
    this.#setScroll(bar?.getScrollPos() ?? this.state.get().scroll);
  }

  // ---- geometry ----------------------------------------------------------------------------------------------

  /** The row under a client point, `-1` outside the rows or over the header. */
  rowAt(pt: Point): number {
    if (pt.x < 0 || pt.x >= rectWidth(this.rectClientValue)) return -1;
    const s = this.state.get();
    return rowAtY(pt.y, this.rowsTop, this.geometry.pitch, s.scroll, s.rows.length);
  }

  /** The rect of a row in client coordinates at the current scroll, `null` when it is not in the visible rows. */
  rowRect(row: number): Rect | null {
    const s = this.state.get();
    if (row < s.scroll || row >= s.scroll + s.page + 1 || row >= s.rows.length) return null;
    const y = this.rowsTop + (row - s.scroll) * this.geometry.pitch;
    return { l: 0, t: y, r: rectWidth(this.rectClientValue), b: y + this.geometry.pitch };
  }

  /** The rect of a row in root (virtual screen) coordinates, for the tooltip. */
  rowScreenRect(row: number): Rect | null {
    const r = this.rowRect(row);
    if (!r) return null;
    const sc = this.getScreenClientRect();
    return rectOffset(r, sc.l, sc.t);
  }

  // ---- pointer -----------------------------------------------------------------------------------------------

  override onMouseWheel(notches: number): void {
    this.scrollTo(this.state.get().scroll - Math.sign(notches));
  }
}
