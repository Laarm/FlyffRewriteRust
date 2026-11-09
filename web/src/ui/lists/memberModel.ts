// The pure model of the member list (WP-UI-23): the column and row data the shells pass, the sort comparator, the stable
// merge of a new row array into the previous one (so unchanged rows keep their identity and React skips them) and the
// selection arithmetic (single, ctrl-toggle, shift-range). No React and no DOM here: the numbers are goldens of the tests.

export type MemberKey = string | number;

export interface MemberColumn {
  readonly id: string;
  /** The text of the header button (`''` for none). */
  readonly title: string;
  /** The column width in px; the columns are laid out left to right from x = 0. */
  readonly width: number;
  readonly align?: 'left' | 'center' | 'right';
  /** A header click sorts by this column (the arrow shows the direction). */
  readonly sortable?: boolean;
  /** `icon`: the cell shows a sprite (a status or job icon) instead of text. */
  readonly kind?: 'text' | 'icon';
}

export interface MemberCell {
  readonly text?: string;
  /** The sprite of an icon cell (a `file` path of the UI sprite cache). */
  readonly icon?: string | null;
  /** `0xAARRGGBB` of the text; the row colour is the default. */
  readonly color?: number;
  /** The value the sort compares (number or string); the text when absent. */
  readonly sort?: number | string;
}

export interface MemberRow {
  readonly key: MemberKey;
  readonly cells: Readonly<Record<string, MemberCell>>;
  /** The default text colour of the row (`0xff000000` black, dead red, grey for absent members). */
  readonly color?: number;
  /** The small HP-like gauge under the row text, 0..1 (the party list). */
  readonly gauge?: number;
  /** The gauge fill colour (`0x64ff0000` of the original: translucent red). */
  readonly gaugeColor?: number;
  /** A dimmed row (an offline friend). */
  readonly dim?: boolean;
}

export type SortDir = 1 | -1;
export interface MemberSort {
  readonly column: string;
  readonly dir: SortDir;
}

/** The value a row sorts by in a column: the explicit `sort`, else the text, else `''`. */
function sortValue(row: MemberRow, column: string): number | string {
  const c = row.cells[column];
  return c?.sort ?? c?.text ?? '';
}

/** Numbers sort numerically, everything else by text with `localeCompare` (digits natural: `Item 2` before `Item 10`). */
export function compareValues(a: number | string, b: number | string): number {
  if (typeof a === 'number' && typeof b === 'number') return a - b;
  return String(a).localeCompare(String(b), undefined, { numeric: true, sensitivity: 'base' });
}

/** The rows in display order: stable, `sort` null = as given. */
export function sortRows(
  rows: readonly MemberRow[],
  sort: MemberSort | null,
): readonly MemberRow[] {
  if (!sort) return rows;
  const withIndex = rows.map((row, index) => ({ row, index }));
  withIndex.sort((x, y) => {
    const c =
      compareValues(sortValue(x.row, sort.column), sortValue(y.row, sort.column)) * sort.dir;
    return c !== 0 ? c : x.index - y.index;
  });
  return withIndex.map((e) => e.row);
}

/** The next sort after a click on a sortable header: a new column sorts ascending, the same column flips. */
export function nextSort(cur: MemberSort | null, column: string): MemberSort {
  if (cur?.column === column) return { column, dir: cur.dir === 1 ? -1 : 1 };
  return { column, dir: 1 };
}

function cellEq(a: MemberCell | undefined, b: MemberCell | undefined): boolean {
  if (a === b) return true;
  if (!a || !b) return false;
  return a.text === b.text && a.icon === b.icon && a.color === b.color && a.sort === b.sort;
}

/** Whether two rows draw the same pixels (so the previous object can be kept). */
export function rowEq(a: MemberRow, b: MemberRow): boolean {
  if (a === b) return true;
  if (
    a.key !== b.key ||
    a.color !== b.color ||
    a.gauge !== b.gauge ||
    a.gaugeColor !== b.gaugeColor ||
    a.dim !== b.dim
  )
    return false;
  const ak = Object.keys(a.cells);
  const bk = Object.keys(b.cells);
  if (ak.length !== bk.length) return false;
  return ak.every((k) => cellEq(a.cells[k], b.cells[k]));
}

/**
 * Merges `next` into `prev` (matched by key): a row that did not change keeps its previous object. Returns `prev` itself when
 * nothing changed at all (same keys in the same order, every row equal), so the caller can skip the state patch.
 */
export function mergeRows(
  prev: readonly MemberRow[],
  next: readonly MemberRow[],
): readonly MemberRow[] {
  const byKey = new Map<MemberKey, MemberRow>();
  for (const r of prev) byKey.set(r.key, r);
  let same = prev.length === next.length;
  const out = next.map((n, i) => {
    const old = byKey.get(n.key);
    const kept = old && rowEq(old, n) ? old : n;
    if (same && prev[i] !== kept) same = false;
    return kept;
  });
  return same ? prev : out;
}

/** The selection after a click on row `index` (display order): plain = that row, ctrl = toggle, shift = the range from `anchor`. */
export function selectionAfterClick(
  rows: readonly MemberRow[],
  current: readonly MemberKey[],
  index: number,
  anchor: number,
  mods: { readonly ctrl: boolean; readonly shift: boolean },
  multi: boolean,
): readonly MemberKey[] {
  const row = rows[index];
  if (!row) return current;
  if (!multi || (!mods.ctrl && !mods.shift)) return [row.key];
  if (mods.shift && anchor >= 0) {
    const [lo, hi] = anchor <= index ? [anchor, index] : [index, anchor];
    return rows.slice(lo, hi + 1).map((r) => r.key);
  }
  return current.includes(row.key) ? current.filter((k) => k !== row.key) : [...current, row.key];
}
