// `TableList`: the list that `CWndGuildCombatRank_Class::OnDraw`, `CWndGuildCombatRanking::OnDraw` and `CWndGuildWarState::OnDraw` draw by hand (`src:_Interface/WndField.cpp:17690-18010,
// 18480-18600`; WP-UI-50, WP-UI-51): two thin dark rules (y 5 and y 25), header texts, then rows 18 px apart from y 35 with the selected row behind a translucent bar, 11 rows per page, a
// docked scroll bar and the wheel one row per notch. Every row is plain data (`TableRow`: texts at x positions and one colour); the window builds the rows from its view. An extra row
// (`own`) is drawn below the page at a fixed y (the own rank of the person ranking, in blue).

import { viewOf } from '../../controls/bind';
import { rectWidth, type Point } from '../../layout/geometry';
import { RowList, type RowListState } from '../../lists/RowList';
import {
  RULE_HEIGHT,
  RULE_Y,
  TABLE_HIT_TOP,
  TABLE_PITCH,
  TABLE_TOP,
  TableListView,
} from './TableListView';

export { RULE_HEIGHT, RULE_Y, TABLE_HIT_TOP, TABLE_PITCH, TABLE_TOP };

/** `TextOut( x, y, text, color )`. */
export interface TableCell {
  readonly x: number;
  readonly text: string;
}

export interface TableRow {
  readonly cells: readonly TableCell[];
  /** `0xAARRGGBB`. */
  readonly color: number;
}

export interface TableHeaderCell {
  readonly x: number;
  readonly y: number;
  readonly text: string;
  readonly color: number;
}

/** The fixed numbers of a table (`sx`, the width of the rules, the width of the selection bar ...). */
export interface TableSpec {
  /** `sx`: the left edge of the rules and of the selection bar. */
  readonly left: number;
  /** The width of the two rules. */
  readonly ruleWidth: number;
  /** The width of the selection bar. */
  readonly selectWidth: number;
  /** `MAX_GUILDCOMBAT_LIST_PER_PAGE` / `MAX_GUILDCOMBAT_RANK_PER_PAGE`. */
  readonly page: number;
  /** The y of the own row; absent = none. */
  readonly ownY?: number;
}

export interface TableState extends RowListState<TableRow> {
  header: readonly TableHeaderCell[];
  own: TableRow | null;
}

export class TableList extends RowList<TableRow, TableState> {
  readonly spec: TableSpec;

  constructor(spec: TableSpec) {
    super(
      { pitch: TABLE_PITCH, top: TABLE_TOP, inset: 0, bar: true, frame: false },
      {
        rows: [],
        hover: -1,
        selected: -1,
        scroll: 0,
        page: spec.page,
        clientOffset: { x: 0, y: 0 },
        header: [],
        own: null,
      },
    );
    this.spec = spec;
    this.view = viewOf<TableList>(TableListView);
  }

  /** The page is a constant of the window, not the height of the control. */
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

  /** Replaces the rows; the selection stays when the row still exists (the scroll goes back to the top, `Init`). */
  setRows(rows: readonly TableRow[], scrollTop = false): void {
    this.setRowsState(rows);
    if (scrollTop) this.scrollTo(0);
  }

  setHeader(header: readonly TableHeaderCell[]): void {
    this.state.patch({ header });
  }

  setOwn(own: TableRow | null): void {
    this.state.patch({ own });
  }

  /** The row under a client point: the original's `(y - 32) / 18` from the scroll position. */
  override rowAt(pt: Point): number {
    if (pt.x < 0 || pt.x >= rectWidth(this.rectClientValue)) return -1;
    const index = Math.floor((pt.y - TABLE_HIT_TOP) / TABLE_PITCH);
    if (index < 0 || index >= this.spec.page) return -1;
    const row = this.scrollRow + index;
    return row >= 0 && row < this.rowCount ? row : -1;
  }

  override onLButtonDown(pt: Point): void {
    // `OnLButtonDown`: the selected row stays when the press misses every row
    const row = this.rowAt(pt);
    if (row >= 0) this.select(row);
  }
}
