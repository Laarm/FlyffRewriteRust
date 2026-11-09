// `WndGuildCombatRankClass` (`CWndGuildCombatRank_Class`, `src:_Interface/WndField.cpp:18340-18700`, ref/22 §17 K, Plan/42 §19.4 K, 45 §28.4; WP-UI-50): one page of the person ranking,
// `APP_GUILDCOMBAT_RANKINGCLASS` (555, 368 x 288, a layout without controls: the original draws everything by hand). The header (rate, name, job, point) sits at y 10 between two rules, the
// rows 18 px apart from y 35 (eleven per page, the scroll bar scrolls), the first row in red, the rank shown only on the first row of a group of equal points; the own row repeats at
// y 250 in blue. The page shows what the view of the window gives it (`RankRow`): names are read from the player-data cache when the view is computed, so a name that arrives after the
// ranking shows up (FIX: the own row used a name cached before the query was answered), and the own row is found anywhere in the list (FIX: the original only saw rows up to the visible page).
// At most 100 rows per page (`MAX_GUILDCOMBAT_RANK`).

import { TID_GAME_JOB, TID_GAME_NAME, TID_GAME_POINT, TID_GAME_RATE } from '../../../gen/defs/text';
import { CodePage } from '../char/CodePage';
import { rectHeight, rectWidth } from '../../layout/geometry';
import { WBS_NOFRAME } from '../../layout/styles';
import { padLeft } from './format';
import type { RankRow } from './ports';
import { TableList, type TableRow, type TableSpec } from './TableList';

/** `MAX_GUILDCOMBAT_RANK_PER_PAGE`. */
export const RANK_PAGE_ROWS = 11;
/** `MAX_GUILDCOMBAT_RANK`. */
export const RANK_MAX_ROWS = 100;
/** The x positions of the columns (`sx + 4`, `sx + 40`, `sx + 180`, `sx + 275` with `sx = 8`). */
export const RANK_COLUMNS = { rate: 12, name: 48, job: 188, point: 283 } as const;

const SPEC: TableSpec = {
  left: 8,
  ruleWidth: 330,
  selectWidth: 320,
  page: RANK_PAGE_ROWS,
  ownY: 250,
};
const BLACK = 0xff000000;
const RED = 0xffc80000;
const BLUE = 0xff0000ff;
const ID_TABLE = 0x7f50;

/** The row of the table for a ranking row. */
export function rankTableRow(r: RankRow, color: number, ownRow = false): TableRow {
  return {
    color,
    cells: [
      // a continuation row of a tie draws three blanks one pixel further right (`TextOut( sx + 5, ... "   " )`)
      ownRow || !r.tie
        ? { x: RANK_COLUMNS.rate, text: padLeft(r.rank, 3) }
        : { x: RANK_COLUMNS.rate + 1, text: '   ' },
      { x: RANK_COLUMNS.name, text: r.name },
      { x: RANK_COLUMNS.job, text: r.job },
      { x: RANK_COLUMNS.point, text: padLeft(r.points, 2) },
    ],
  };
}

export interface RankClassDeps {
  readonly getText: (tid: number) => string;
}

export class WndGuildCombatRankClass extends CodePage {
  readonly #getText: (tid: number) => string;
  #table: TableList | null = null;
  #rows: readonly RankRow[] = [];
  #own: RankRow | null = null;

  constructor(deps: RankClassDeps = { getText: () => '' }) {
    super();
    this.#getText = deps.getText;
  }

  get table(): TableList | null {
    return this.#table;
  }
  /** The rows shown. */
  get rows(): readonly RankRow[] {
    return this.#rows;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const rect = this.rectClient;
    const table = new TableList(SPEC);
    this.#table = this.addControl(
      table,
      ID_TABLE,
      { l: 0, t: 0, r: rectWidth(rect), b: rectHeight(rect) },
      WBS_NOFRAME,
    );
    const t = this.#getText;
    table.setHeader([
      { x: RANK_COLUMNS.rate, y: 10, text: t(TID_GAME_RATE), color: BLACK },
      { x: RANK_COLUMNS.name, y: 10, text: t(TID_GAME_NAME), color: BLACK },
      { x: RANK_COLUMNS.job, y: 10, text: t(TID_GAME_JOB), color: BLACK },
      { x: RANK_COLUMNS.point, y: 10, text: t(TID_GAME_POINT), color: BLACK },
    ]);
    this.apply(this.#rows, this.#own);
  }

  /** The rows of the page (a page keeps at most 100) and the own row. */
  apply(rows: readonly RankRow[], own: RankRow | null): void {
    this.#rows = rows.slice(0, RANK_MAX_ROWS);
    this.#own = own;
    const table = this.#table;
    if (!table) return;
    table.setRows(this.#rows.map((r, i) => rankTableRow(r, i === 0 ? RED : BLACK)));
    table.setOwn(own ? rankTableRow(own, BLUE, true) : null);
  }
}
