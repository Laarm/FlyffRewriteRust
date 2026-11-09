// `MemberList` (`CWndPartyCtrl`, `CWndGuildCtrl(Ex)`, `CWndFriendCtrl(Ex)`; WP-UI-23): the list of the people of a party, a
// guild or a friend list as ONE parameterised control. The shells pass
//   * the columns (`MemberColumn`: id, header title, width, alignment, sortable, text or icon),
//   * the rows (`MemberRow`: a key and a cell per column, a row colour, an optional gauge under the text, a dim flag),
// and read back what the user did. The control owns what the originals each re-implemented:
//   * the header strip of sort buttons: a click on a sortable header sorts by it (ascending, a second click descending; a
//     triangle shows the direction); `externalSort` leaves the order to the shell and only reports the click;
//   * selection BY KEY (so it survives a resort and a value update): a click selects, with `multi` ctrl toggles and shift
//     extends from the anchor; a click on the empty area clears it;
//   * double click (`activate`: open the whisper window), right click (`context`: the member menu), hover (the tooltip);
//   * the wheel and the docked scroll bar, and a keyboard step (up / down) when the control has the focus;
//   * the rendering cost: only the visible rows exist in the DOM and a row that did not change keeps its object, so a list of
//     200 rows whose values change 10 times a second commits once per update and re-renders only what changed.
// Row heights and columns are the shell's: the party window uses 20 px rows with a gauge, the messenger 18 px rows with the
// status, level, job and channel icons in their own columns.

import type { Point, Rect } from '../layout/geometry';
import { Wnm } from '../layout/styles';
import type { KeyInfo } from '../wnd/WndBase';
import { viewOf } from '../controls/bind';
import { MemberListView } from './MemberListView';
import {
  type MemberColumn,
  type MemberKey,
  type MemberRow,
  type MemberSort,
  mergeRows,
  nextSort,
  selectionAfterClick,
  sortRows,
} from './memberModel';
import { RowList, type RowListState } from './RowList';

export interface MemberListDelegate {
  /** The pointer entered a row (or left: `null`): put or cancel the member tooltip over `rect` (screen). */
  hover?(row: MemberRow | null, rect: Rect | null): void;
  /** The selection changed (a click, ctrl / shift click, a key step). */
  select?(keys: readonly MemberKey[]): void;
  /** A double click on a row. */
  activate?(row: MemberRow): void;
  /** A right click on a row (the member menu), `at` in root coordinates. */
  context?(row: MemberRow, at: Point): void;
  /** A header click: the new sort (the shell sorts itself when `externalSort`). */
  sort?(sort: MemberSort): void;
}

export interface MemberListOptions {
  readonly columns: readonly MemberColumn[];
  /** The row pitch (party 23, messenger 18...). */
  readonly rowHeight?: number;
  /** The height of the header strip, 0 = no header. */
  readonly header?: number;
  /** Ctrl and shift extend the selection. */
  readonly multi?: boolean;
  /** The shell orders the rows: a header click is only reported. */
  readonly externalSort?: boolean;
  readonly frame?: boolean;
}

export interface MemberListState extends RowListState<MemberRow> {
  columns: readonly MemberColumn[];
  sort: MemberSort | null;
  /** The selected keys (display order is not implied). */
  selection: readonly MemberKey[];
}

export class MemberList extends RowList<MemberRow, MemberListState> {
  #delegate: MemberListDelegate;
  readonly multi: boolean;
  readonly externalSort: boolean;
  /** The rows as the shell gave them (before the control's own sort). */
  #raw: readonly MemberRow[] = [];
  #anchor = -1;

  constructor(options: MemberListOptions, delegate: MemberListDelegate = {}) {
    super(
      {
        pitch: options.rowHeight ?? 20,
        top: 0,
        inset: 0,
        bar: true,
        frame: options.frame ?? false,
        header: options.header ?? 18,
      },
      {
        rows: [],
        hover: -1,
        selected: -1,
        scroll: 0,
        page: 1,
        clientOffset: { x: 0, y: 0 },
        columns: options.columns,
        sort: null,
        selection: [],
      },
    );
    this.#delegate = delegate;
    this.multi = options.multi ?? false;
    this.externalSort = options.externalSort ?? false;
    this.view = viewOf<MemberList>(MemberListView);
  }

  setDelegate(delegate: MemberListDelegate): void {
    this.#delegate = delegate;
  }

  get columns(): readonly MemberColumn[] {
    return this.state.get().columns;
  }
  get sort(): MemberSort | null {
    return this.state.get().sort;
  }
  get selection(): readonly MemberKey[] {
    return this.state.get().selection;
  }

  /** The rows as the shell pushed them (unsorted), for tests and the shell's own bookkeeping. */
  get rawRows(): readonly MemberRow[] {
    return this.#raw;
  }

  setColumns(columns: readonly MemberColumn[]): void {
    this.state.patch({ columns });
  }

  /**
   * Pushes the rows (at most one state patch): unchanged rows keep their identity, the control's sort is applied and the
   * selection is kept for the keys that remain. An identical list changes nothing.
   */
  setRows(rows: readonly MemberRow[]): void {
    const merged = mergeRows(this.#raw, rows);
    if (merged === this.#raw) return;
    this.#raw = merged;
    this.#publish();
  }

  /** `sort` from the shell (e.g. restoring a saved order); a header click does the same. */
  setSort(sort: MemberSort | null): void {
    this.state.patch({ sort });
    this.#publish();
  }

  #publish(): void {
    const s = this.state.get();
    const rows = this.externalSort ? this.#raw : sortRows(this.#raw, s.sort);
    const present = new Set(rows.map((r) => r.key));
    const selection = s.selection.filter((k) => present.has(k));
    const first = selection[0];
    const selected = first === undefined ? -1 : rows.findIndex((r) => r.key === first);
    this.setRowsState(rows, { selection, selected });
  }

  /** Replaces the selection (the shell selects the player's own row, for instance). */
  setSelection(keys: readonly MemberKey[]): void {
    const rows = this.rows;
    const first = keys[0];
    const selected = first === undefined ? -1 : rows.findIndex((r) => r.key === first);
    this.state.patch({ selection: keys, selected });
    this.#anchor = selected;
  }

  // ---- header ------------------------------------------------------------------------------------------------

  /** The column under a client x, `null` outside the columns. */
  columnAt(x: number): MemberColumn | null {
    let left = 0;
    for (const c of this.columns) {
      if (x >= left && x < left + c.width) return c;
      left += c.width;
    }
    return null;
  }

  /** The left edge of a column. */
  columnLeft(id: string): number {
    let left = 0;
    for (const c of this.columns) {
      if (c.id === id) return left;
      left += c.width;
    }
    return -1;
  }

  #headerHit(pt: Point): boolean {
    const h = this.geometry.header ?? 0;
    return h > 0 && pt.y >= this.geometry.top && pt.y < this.geometry.top + h;
  }

  // ---- pointer -----------------------------------------------------------------------------------------------

  override onLButtonDown(pt: Point, info?: { ctrl: boolean; shift: boolean }): void {
    if (this.#headerHit(pt)) {
      const col = this.columnAt(pt.x);
      if (col?.sortable !== true) return;
      const sort = nextSort(this.sort, col.id);
      this.setSort(sort);
      this.#delegate.sort?.(sort);
      return;
    }
    const i = this.rowAt(pt);
    if (i < 0) {
      // the empty area clears the selection
      if (this.selection.length > 0) {
        this.state.patch({ selection: [], selected: -1 });
        this.#delegate.select?.([]);
      }
      return;
    }
    const mods = { ctrl: info?.ctrl ?? false, shift: info?.shift ?? false };
    const selection = selectionAfterClick(
      this.rows,
      this.selection,
      i,
      this.#anchor,
      mods,
      this.multi,
    );
    if (!mods.shift || !this.multi) this.#anchor = i;
    const first = selection[0];
    this.state.patch({
      selection,
      selected: first === undefined ? -1 : this.rows.findIndex((r) => r.key === first),
    });
    this.#delegate.select?.(selection);
    this.parent?.onChildNotify(Wnm.SELCHANGE, this.id, this);
  }

  override onLButtonDblClk(pt: Point): void {
    // two quick clicks on a header are two sorts (a header is a button), not a double click
    if (this.#headerHit(pt)) {
      this.onLButtonDown(pt);
      return;
    }
    const i = this.rowAt(pt);
    const row = i >= 0 ? (this.rows[i] ?? null) : null;
    if (!row) return;
    this.#delegate.activate?.(row);
    this.parent?.onChildNotify(Wnm.DBLCLK, this.id, this);
  }

  override onRButtonUp(pt: Point): void {
    const i = this.rowAt(pt);
    const row = i >= 0 ? (this.rows[i] ?? null) : null;
    if (!row) return;
    if (!this.selection.includes(row.key)) {
      this.state.patch({ selection: [row.key], selected: i });
      this.#anchor = i;
      this.#delegate.select?.([row.key]);
    }
    const sc = this.getScreenClientRect();
    this.#delegate.context?.(row, { x: sc.l + pt.x, y: sc.t + pt.y });
  }

  override onMouseMove(pt: Point): void {
    const i = this.rowAt(pt);
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

  /** Up / down step the single selection and keep the row in view. */
  override onKeyDown(code: string, info?: KeyInfo): void {
    if (info?.ctrl === true || info?.alt === true) return;
    if (code !== 'ArrowUp' && code !== 'ArrowDown') return;
    const n = this.rowCount;
    if (n === 0) return;
    const cur = this.state.get().selected;
    const next = Math.max(
      0,
      Math.min(
        n - 1,
        (cur < 0 ? (code === 'ArrowDown' ? -1 : n) : cur) + (code === 'ArrowDown' ? 1 : -1),
      ),
    );
    const row = this.rows[next];
    if (!row) return;
    this.state.patch({ selection: [row.key], selected: next });
    this.#anchor = next;
    const s = this.state.get();
    if (next < s.scroll) this.scrollTo(next);
    else if (next >= s.scroll + s.page) this.scrollTo(next - s.page + 1);
    this.#delegate.select?.([row.key]);
  }
}
