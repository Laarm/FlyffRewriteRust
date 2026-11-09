// `GuildMemberList` (the `OnDraw` / mouse code of `CWndGuildTabMember`, `src:_Interface/WndGuildTabMember.cpp:190-480, 590-700`, Plan/42 §19.4 J; WP-UI-45): the rows of
// the member page as ONE control over the area below the five sort buttons: 18 px rows, 14 visible, the wheel steps three rows, a docked scroll bar, a press selects (a press
// below the last row selects the last one, above the first the first: the original clamped), a right click opens the member menu, the hover is the tooltip of the member.
// The pictures (rank marks, job marks) and the colours of `GuildMemberListView` follow `memberIcons.ts`.

import { viewOf } from '../../controls/bind';
import type { Point, Rect } from '../../layout/geometry';
import { RowList, type RowListState } from '../../lists/RowList';
import { GuildMemberListView } from './GuildMemberListView';
import { ROW_PITCH, ROWS_TOP, WHEEL_ROWS } from './memberIcons';
import type { GuildMemberRow } from './ports';

export interface GuildMemberListState extends RowListState<GuildMemberRow> {
  /** The character id of the selected member, `null` for none (`m_nSelect` follows the member over a resort). */
  selectedId: number | null;
}

/** What the list asks of the page that owns it. */
export interface GuildMemberListHooks {
  /** A right click (`OnRButtonUp`): the menu of member `id`, `at` in root coordinates. */
  context(id: number, at: Point): void;
  /** The pointer is over a row (the tooltip of the member) or left the rows (`null`). */
  hover(row: GuildMemberRow | null, rect: Rect | null): void;
}

export class GuildMemberList extends RowList<GuildMemberRow, GuildMemberListState> {
  readonly #hooks: GuildMemberListHooks;

  constructor(hooks: GuildMemberListHooks) {
    super(
      { pitch: ROW_PITCH, top: 0, inset: 0, bar: true, frame: false },
      {
        rows: [],
        hover: -1,
        selected: -1,
        scroll: 0,
        page: 1,
        clientOffset: { x: 0, y: 0 },
        selectedId: null,
      },
    );
    this.#hooks = hooks;
    this.view = viewOf<GuildMemberList>(GuildMemberListView);
  }

  /** The first row sits 8 px below the top of the control (page y 32 in the original). */
  override get rowsTop(): number {
    return ROWS_TOP;
  }

  get selectedId(): number | null {
    return this.state.get().selectedId;
  }

  /** The selected row (by character id), `null` for none. */
  get selectedMember(): GuildMemberRow | null {
    const id = this.selectedId;
    return id === null ? null : (this.rows.find((r) => r.id === id) ?? null);
  }

  /** Pushes the rows in display order; the selection follows its member and goes when the member left. */
  setMembers(rows: readonly GuildMemberRow[]): void {
    const id = this.state.get().selectedId;
    const selected = id === null ? -1 : rows.findIndex((r) => r.id === id);
    this.setRowsState(rows, { selected, selectedId: selected < 0 ? null : id });
  }

  /** `m_nSelect`: selects a member by id (tests, the probe). */
  selectId(id: number | null): void {
    const i = id === null ? -1 : this.rows.findIndex((r) => r.id === id);
    this.state.patch({ selected: i, selectedId: i < 0 ? null : id });
  }

  /** The row a press at `pt` selects: `scroll + (y - top) / 18`, clamped to the rows. */
  pressRow(pt: Point): number {
    const n = this.rowCount;
    if (n === 0) return -1;
    const i = this.scrollRow + Math.trunc((pt.y - this.rowsTop) / ROW_PITCH);
    return Math.max(0, Math.min(n - 1, i));
  }

  override onLButtonDown(pt: Point): void {
    const i = this.pressRow(pt);
    const row = i >= 0 ? this.rows[i] : undefined;
    if (!row) return;
    this.state.patch({ selected: i, selectedId: row.id });
  }

  override onRButtonDown(pt: Point): void {
    this.onLButtonDown(pt);
  }

  override onRButtonUp(pt: Point): void {
    const row = this.selectedMember;
    if (!row) return;
    const sc = this.getScreenClientRect();
    this.#hooks.context(row.id, { x: sc.l + pt.x, y: sc.t + pt.y });
  }

  override onMouseMove(pt: Point): void {
    const i = this.rowAt(pt);
    if (i === this.state.get().hover) return;
    this.state.patch({ hover: i });
    const row = i >= 0 ? (this.rows[i] ?? null) : null;
    this.#hooks.hover(row, row ? this.rowScreenRect(i) : null);
  }

  override onMouseLeave(): void {
    if (this.state.get().hover < 0) return;
    this.state.patch({ hover: -1 });
    this.#hooks.hover(null, null);
  }

  /** The wheel steps three rows (`m_nCurrentList -= 3`). */
  override onMouseWheel(notches: number): void {
    this.scrollTo(this.scrollRow - Math.sign(notches) * WHEEL_ROWS);
  }
}
