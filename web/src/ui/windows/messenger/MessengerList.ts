// `MessengerList` (`CWndFriendCtrlEx`, `CWndGuildCtrlEx`, the lists of `CWndCampus`; `src:_Interface/WndMessengerCtrl.cpp:227-1640`, Plan/42 §19.4 I; WP-UI-43): the
// friend and guild pages of the messenger as a `MemberList` with the layout of the originals - no header (the five sort buttons are buttons of the window),
// 20 px rows from y = 3, the channel digit at x 20, the small state icon at 76, the level at 127, the job marks at 174 and 194, the name at 226 (cut at 10
// characters). The shell pushes the sorted rows (`externalSort`); a row is a bag of cells the view draws at those positions:
//
//   channel / state / jobType / job   an `a:<n>` reference to sprite `n` of `Icon/icon_PlayerData.inc` (`null` draws nothing), `job` carries the job name as its text
//   level / name                      the text; the colour of a name is set for a blocked friend (red) and a housing visitor (green), a selected row is blue
//
// The control owns what the original's pages did: selection, double click, the right-click menu (the shell decides what it holds), the wheel and the docked
// scroll bar, the job-name tooltip, and the click beside the rows (which clears the world target).

import { type Point, type Rect } from '../../layout/geometry';
import { viewOf } from '../../controls/bind';
import { MemberList } from '../../lists/MemberList';
import type { MemberCell, MemberColumn, MemberKey, MemberRow } from '../../lists/memberModel';
import {
  channelSprite,
  COL,
  cutName,
  JOB_TIP_W,
  JOB_TIP_X,
  jobMarks,
  nameColor,
  NAME_COLOR,
  ROW_PITCH,
  ROWS_TOP,
  smallStateSprite,
  spriteRef,
} from './icons';
import type { MessengerRow } from './ports';
import { MessengerListView } from './MessengerListView';

/** The columns of the original's rows; the header is off, the widths only give `columnAt` something to answer. */
const COLUMNS: readonly MemberColumn[] = [
  { id: 'channel', title: '', width: COL.state - COL.channel, kind: 'icon' },
  { id: 'state', title: '', width: COL.level - COL.state, kind: 'icon' },
  { id: 'level', title: '', width: COL.jobType - COL.level },
  { id: 'jobType', title: '', width: COL.job - COL.jobType, kind: 'icon' },
  { id: 'job', title: '', width: COL.name - COL.job, kind: 'icon' },
  { id: 'name', title: '', width: 90 },
];

/** What the list asks of the page that owns it. */
export interface MessengerListHooks {
  /** A double click on a row (`OnLButtonDblClk`). */
  activate(id: number): void;
  /** A right click on a row (`OnRButtonUp`), `at` in root coordinates. */
  context?(id: number, at: Point): void;
  /** The pointer is over the job icon of a row (`name` of the job), or left it (`null`): the job-name tooltip. */
  jobTip?(name: string | null, rect: Rect | null): void;
  /** A click beside the rows (`g_WorldMng()->SetObjFocus( NULL )`). */
  clearTarget?(): void;
}

/** A row of the original's list as the cells of a `MemberRow`. */
export function memberRowOf(r: MessengerRow): MemberRow {
  const marks = jobMarks(r.jobType, r.job, r.level);
  const base = nameColor(r, false);
  const colored = base !== NAME_COLOR.normal ? { color: base } : {};
  const cells: Record<string, MemberCell> = {
    channel: { icon: spriteRef(channelSprite(r.channel)) },
    state: { icon: spriteRef(smallStateSprite(r.state)) },
    level: {
      text: String(r.level),
      ...(r.visitAllowed === true ? { color: NAME_COLOR.visitor } : {}),
    },
    jobType: { icon: spriteRef(marks.type) },
    job: { icon: spriteRef(marks.job), text: r.jobName },
    name: { text: cutName(r.name), ...colored },
  };
  return { key: r.id, cells };
}

export class MessengerList extends MemberList {
  readonly #hooks: MessengerListHooks;

  /** `frame`: the translucent `PaintFrame` behind the rows (the campus lists sit on the list boxes of their layout instead). */
  constructor(hooks: MessengerListHooks, frame = true) {
    super(
      { columns: COLUMNS, rowHeight: ROW_PITCH, header: 0, externalSort: true, frame },
      {
        activate: (row) => {
          hooks.activate(Number(row.key));
        },
        context: (row, at) => {
          hooks.context?.(Number(row.key), at);
        },
      },
    );
    this.#hooks = hooks;
    this.view = viewOf<MessengerList>(MessengerListView);
  }

  /** The rows start 3 px below the top (`CPoint pt( 3, 3 )`). */
  override get rowsTop(): number {
    return ROWS_TOP;
  }

  /** Pushes the (sorted) rows of the page. */
  setMessengerRows(rows: readonly MessengerRow[]): void {
    this.setRows(rows.map(memberRowOf));
  }

  /** The character id of the selected row, `null` for none. */
  get selectedId(): number | null {
    const key: MemberKey | undefined = this.selection[0];
    return key === undefined ? null : Number(key);
  }

  override onLButtonDown(pt: Point, info?: { ctrl: boolean; shift: boolean }): void {
    super.onLButtonDown(pt, info);
    // a click beside the rows (`OnLButtonUp` found no row): the target of the world is cleared
    if (this.rowAt(pt) < 0) this.#hooks.clearTarget?.();
  }

  override onMouseMove(pt: Point): void {
    super.onMouseMove(pt);
    const i = this.rowAt(pt);
    const row = i >= 0 ? this.rows[i] : undefined;
    const rect = i >= 0 ? this.rowScreenRect(i) : null;
    if (row && rect && pt.x >= JOB_TIP_X && pt.x < JOB_TIP_X + JOB_TIP_W) {
      this.#hooks.jobTip?.(row.cells['job']?.text ?? null, {
        l: rect.l + JOB_TIP_X,
        t: rect.t,
        r: rect.l + JOB_TIP_X + JOB_TIP_W,
        b: rect.b,
      });
    } else {
      this.#hooks.jobTip?.(null, null);
    }
  }

  override onMouseLeave(): void {
    super.onMouseLeave();
    this.#hooks.jobTip?.(null, null);
  }
}
