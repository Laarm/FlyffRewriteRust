// `SkillIconList` (`CWndSkillCtrl`, WndSkillCtrl.cpp; WP-UI-23): the skills of a job as a report list. Each row is 35 + 3 px
// high from y = 3 of the client (deflated by 3 from the window, 15 px for the docked bar) and shows
//   * the 32 x 32 icon at (3, +6), at alpha 64/255 while the skill is locked (`CheckSkill`: the job-level requirement),
//   * the name at (65, +10) and the level `Lv%2d` right-aligned to x = 58 at the icon's lower edge, `Master` in red once the
//     level reached the maximum, otherwise the level gauge at (60, +25) 10 px high,
//   * the selected row's texts in `0xff6060ff` instead of black.
// Behaviour: only an unlocked skill reacts. A press selects the row and arms a drag whose payload is the skill shortcut
// (`SC.SKILL`, `index = skill id`, `id = row`); the release over the row that was pressed notifies the parent (`SELCHANGE`)
// and a double click sends `DBLCLK` (the window puts the skill on the taskbar queue). The hovered row asks for the skill
// tooltip (`delegate.hover`, rect = the 32 px band of the row). What a skill is, the level numbers and the lock come from the
// window's rows.

import type { Point } from '../layout/geometry';
import { Wnm } from '../layout/styles';
import { SC, type Shortcut } from '../dnd/shortcut';
import { viewOf } from '../controls/bind';
import type { Rect } from '../layout/geometry';
import { RowList, type RowListState } from './RowList';
import { SkillIconListView } from './SkillIconListView';

export interface SkillRow {
  /** The skill id (`dwSkill`). */
  readonly id: number;
  readonly icon: string | null;
  readonly name: string;
  readonly level: number;
  /** `dwExpertMax`: at this level the row shows `Master` and no gauge. */
  readonly maxLevel: number;
  /** The fill of the level gauge, 0..1 (the original paints it full: `1`). */
  readonly progress?: number;
  /** `CheckSkill`: `false` draws the icon dim and the row does not react. */
  readonly enabled: boolean;
}

export interface SkillListDelegate {
  /** The pointer entered a row (or left: `null`): put or cancel the skill tooltip over `rect` (screen, the 32 px band). */
  hover?(row: SkillRow | null, rect: Rect | null): void;
  /** A press on an unlocked row; return `false` to refuse. */
  press?(row: SkillRow): boolean;
  /** The release over the pressed row (`WNM_SELCHANGE` to the parent). */
  select?(row: SkillRow): void;
  /** A double click on an unlocked row (`SetSkillQueue`). */
  activate?(row: SkillRow): void;
}

/** The geometry of the original: `m_nFontHeight = 35` plus 3, the first row at y = 3, the window deflated by 3. */
export const SKILL_ROW_HEIGHT = 35;
export const SKILL_ROW_PITCH = 38;
export const SKILL_ROW_TOP = 3;

export interface SkillListState extends RowListState<SkillRow> {
  /** The row that was pressed (the release over the same row selects). */
  pressed: number;
}

export class SkillIconList extends RowList<SkillRow, SkillListState> {
  #delegate: SkillListDelegate;
  #pending = -1;

  constructor(delegate: SkillListDelegate = {}) {
    super(
      { pitch: SKILL_ROW_PITCH, top: SKILL_ROW_TOP, inset: 3, bar: true, frame: true },
      {
        rows: [],
        hover: -1,
        selected: -1,
        scroll: 0,
        page: 1,
        clientOffset: { x: 0, y: 0 },
        pressed: -1,
      },
    );
    this.#delegate = delegate;
    this.view = viewOf<SkillIconList>(SkillIconListView);
  }

  setDelegate(delegate: SkillListDelegate): void {
    this.#delegate = delegate;
  }

  setRows(rows: readonly SkillRow[]): void {
    this.setRowsState(rows);
  }

  /** The 32 px band of a row that is the hover and tooltip rect (`CRect( x + 3, y + 6, +nWidth, y + 6 + 32 )`). */
  override rowRect(row: number): Rect | null {
    const r = super.rowRect(row);
    return r ? { l: r.l, t: r.t + 6, r: r.r, b: r.t + 6 + 32 } : null;
  }

  #unlocked(row: number): SkillRow | null {
    const r = row >= 0 ? (this.rows[row] ?? null) : null;
    return r?.enabled === true ? r : null;
  }

  override onLButtonDown(pt: Point): void {
    if (this.env.wm.isDraggingShortcut()) return;
    const i = this.#rowOfPoint(pt);
    const row = this.#unlocked(i);
    if (!row) return;
    if (this.#delegate.press?.(row) === false) return;
    this.state.patch({ selected: i, pressed: i });
    this.#pending = i;
    // the drag starts at the first pointer move (`m_bDrag` + `OnMouseMove`)
    this.env.wm.drag.arm(this, () => {
      const p = this.#pending;
      this.#pending = -1;
      const r = this.#unlocked(p);
      if (!r) return null;
      const sc: Shortcut = {
        kind: SC.SKILL,
        id: p,
        type: 0,
        index: r.id,
        userId: 0,
        data: 0,
        ...(r.icon ? { icon: r.icon } : {}),
        from: this,
      };
      return sc;
    });
  }

  /** The whole 38 px pitch of the row answers a press (the C++ rect is `nFontHeight` high from the row top). */
  #rowOfPoint(pt: Point): number {
    return this.rowAt(pt);
  }

  override onLButtonUp(pt: Point): void {
    this.#pending = -1;
    const pressed = this.state.get().pressed;
    this.state.patch({ pressed: -1 });
    const i = this.#rowOfPoint(pt);
    const row = this.#unlocked(i);
    if (!row || i !== pressed) return;
    this.#delegate.select?.(row);
    this.parent?.onChildNotify(Wnm.SELCHANGE, this.id, this);
  }

  override onPointerRelease(): void {
    this.#pending = -1;
  }

  override onMouseMove(pt: Point): void {
    if (this.env.wm.isDraggingShortcut()) return;
    const i = this.rowOfIcon(pt);
    if (i === this.state.get().hover) return;
    this.state.patch({ hover: i });
    const row = i >= 0 ? (this.rows[i] ?? null) : null;
    this.#delegate.hover?.(row, row ? this.rowScreenRect(i) : null);
  }

  /** The hover rect is the 32 px band: a point above it in the row (the 6 px gap) hovers nothing. */
  rowOfIcon(pt: Point): number {
    const i = this.rowAt(pt);
    if (i < 0) return -1;
    const r = this.rowRect(i);
    return r && pt.y >= r.t && pt.y < r.b ? i : -1;
  }

  override onMouseLeave(): void {
    if (this.state.get().hover >= 0) {
      this.state.patch({ hover: -1 });
      this.#delegate.hover?.(null, null);
    }
  }

  override onLButtonDblClk(pt: Point): void {
    const i = this.#rowOfPoint(pt);
    const row = this.#unlocked(i);
    if (!row) return;
    this.state.patch({ selected: i });
    this.#delegate.activate?.(row);
    this.parent?.onChildNotify(Wnm.DBLCLK, this.id, this);
  }
}
