// `HouseList`: the hand-drawn row list of the housing windows (`CWndHousing::OnDraw`, `CWndGuildHousing::OnDraw`, `src:_Interface/WndHousing.cpp:97-190, 659-789`; WP-UI-58). The originals keep a list box of blank strings
// (selection, scroll) and draw the texts, the icons and the selection by hand on top; here ONE control does both over the (empty) list box of the layout, which stays under it for the frame skin.
//
//   * a `RowList` of fixed pitch (the housing list 16 px / 10 rows, the storage list 40 px / 7 rows), a docked scroll bar and the wheel one row per notch;
//   * a row is plain data: texts at x / y offsets of the row, an optional icon, a colour (and the colour it takes while selected: the housing list turns BLUE, the storage list instead gets a red fill `0x55ff0000`);
//   * a press selects the row under the pointer (`select` of the delegate), a double click reports `activate`;
//   * the window pushes the rows (`setRows`: an identical list changes nothing, the selection is kept by KEY) and reads the selection back.

import { viewOf } from '../../controls/bind';
import { type Point, rectWidth } from '../../layout/geometry';
import { RowList, type RowListState } from '../../lists/RowList';
import { HouseListView } from './HouseListView';

/** `TextOut( x, y, text, color )` inside a row (the row origin is its top-left). */
export interface HouseCell {
  readonly x: number;
  readonly y: number;
  readonly text: string;
}

export interface HouseRow {
  /** The identity of the row (the selection survives a refresh by it). */
  readonly key: number | string;
  readonly cells: readonly HouseCell[];
  /** `0xAARRGGBB` of the texts. */
  readonly color: number;
  /** The colour of the texts while the row is selected (the housing list: blue). */
  readonly selectedColor?: number;
  /** An item icon (`item/<file>`) at (x, y) of the row. */
  readonly icon?: { readonly path: string | null; readonly x: number; readonly y: number };
}

/** The fixed numbers of a list. */
export interface HouseListSpec {
  readonly pitch: number;
  /** The y of the first row. */
  readonly top: number;
  /** The rows that fit (the page). */
  readonly page: number;
  /** A translucent fill behind the selected row: `yOffset` from the row top, `height`, and the margins from the left / right edge of the control. */
  readonly selectFill?: {
    readonly color: string;
    readonly yOffset: number;
    readonly height: number;
    readonly left: number;
    readonly right: number;
  };
}

export interface HouseListDelegate {
  /** A press selected a row. */
  select?(row: HouseRow, index: number): void;
  /** A double click on a row. */
  activate?(row: HouseRow, index: number): void;
}

export interface HouseListState extends RowListState<HouseRow> {
  spec: HouseListSpec;
}

/** Whether two rows draw the same pixels. */
export function sameRow(a: HouseRow, b: HouseRow): boolean {
  if (a === b) return true;
  if (a.key !== b.key || a.color !== b.color || a.selectedColor !== b.selectedColor) return false;
  if (a.icon?.path !== b.icon?.path || a.icon?.x !== b.icon?.x || a.icon?.y !== b.icon?.y)
    return false;
  if (a.cells.length !== b.cells.length) return false;
  return a.cells.every((c, i) => {
    const d = b.cells[i];
    return c.x === d?.x && c.y === d.y && c.text === d.text;
  });
}

export class HouseList extends RowList<HouseRow, HouseListState> {
  #delegate: HouseListDelegate;

  constructor(spec: HouseListSpec, delegate: HouseListDelegate = {}) {
    super(
      { pitch: spec.pitch, top: spec.top, inset: 0, bar: true, frame: false },
      {
        rows: [],
        hover: -1,
        selected: -1,
        scroll: 0,
        page: spec.page,
        clientOffset: { x: 0, y: 0 },
        spec,
      },
    );
    this.#delegate = delegate;
    this.view = viewOf<HouseList>(HouseListView);
  }

  setDelegate(delegate: HouseListDelegate): void {
    this.#delegate = delegate;
  }

  get spec(): HouseListSpec {
    return this.state.get().spec;
  }

  /** The selected row object, `null` for none. */
  get selectedItem(): HouseRow | null {
    return this.state.get().rows[this.selectedRow] ?? null;
  }

  /** The page is a constant of the window (the original draws `GH_MAX_VIEW_CAPACITY` / 10 rows whatever the control's height). */
  protected override relayout(): void {
    super.relayout();
    const page = this.spec.page;
    this.state.patch({ page });
    const bar = this.scrollBar;
    if (bar) {
      bar.setScrollPage(page);
      bar.setVisible(this.rowCount > page);
    }
    this.scrollTo(this.scrollRow);
  }

  /**
   * Replaces the rows. The selection follows the row KEY (a re-sorted list keeps the row selected), the scroll is kept and clamped (`SetScrollPos( currScrollPos )`);
   * an identical list emits nothing. `selectKey` selects that key instead (`SetSelectedByObjID`), `null` clears the selection.
   */
  setRows(rows: readonly HouseRow[], selectKey?: number | string | null): void {
    const prev = this.state.get();
    const keep = selectKey === undefined ? (prev.rows[prev.selected]?.key ?? null) : selectKey;
    const same =
      prev.rows.length === rows.length &&
      prev.rows.every((r, i) => {
        const n = rows[i];
        return n !== undefined && sameRow(r, n);
      });
    const selected = keep === null ? -1 : rows.findIndex((r) => r.key === keep);
    if (same && selected === prev.selected) return;
    this.setRowsState(same ? prev.rows : rows, { selected });
  }

  /** Scrolls so that `row` is visible (`FixScrollBar`: above the page it becomes the first row, below it the last one). */
  ensureVisible(row: number): void {
    if (row < 0) return;
    const s = this.state.get();
    if (row < s.scroll) this.scrollTo(row);
    else if (row >= s.scroll + s.page) this.scrollTo(row - s.page + 1);
  }

  /** A selection that came from outside (the window or the world): no delegate call. */
  selectKey(key: number | string | null): void {
    const row = key === null ? -1 : this.state.get().rows.findIndex((r) => r.key === key);
    this.select(row);
    this.ensureVisible(row);
  }

  override onLButtonDown(pt: Point): void {
    const row = this.rowAt(pt);
    if (row < 0) return;
    this.select(row);
    const item = this.state.get().rows[row];
    if (item) this.#delegate.select?.(item, row);
  }

  override onLButtonDblClk(pt: Point): void {
    const row = this.rowAt(pt);
    if (row < 0) return;
    this.select(row);
    const item = this.state.get().rows[row];
    if (item) this.#delegate.activate?.(item, row);
  }

  /** The width of the rows (the client minus the bar). */
  get rowWidth(): number {
    return rectWidth(this.rectClientValue);
  }
}
