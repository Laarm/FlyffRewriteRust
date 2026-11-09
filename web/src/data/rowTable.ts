// Column-oriented bundle tables (Plan/14 section 10.2) and the GETTEXT façade (Plan/14 section 10.6, Plan/40 section 3.4).
// The row and bundle types are generated from the Rust DTOs (`gen/bundles`, `cargo xtask gen-data-types`).
import type { RowTable, TextsBundle } from '../gen/bundles';

/** The property names of `T` whose values are numbers (the keys `indexRows` accepts). */
export type NumericKey<T> = {
  [K in keyof T]-?: NonNullable<T[K]> extends number ? K : never;
}[keyof T] &
  string;

/** `__proto__` is never written as a row property (a bundle column cannot reach the prototype). */
const UNSAFE_COLUMN = '__proto__';

/** The column names of `t` with the position of each cell; unusable names are dropped. */
function columnSlots(t: RowTable<unknown>): { name: string; at: number }[] {
  const slots: { name: string; at: number }[] = [];
  t.columns.forEach((name, at) => {
    if (name !== UNSAFE_COLUMN) {
      slots.push({ name, at });
    }
  });
  return slots;
}

/** One row as an object: columns the table lacks stay `undefined`, columns `T` does not know are kept as written. */
function toRow(slots: readonly { name: string; at: number }[], cells: readonly unknown[]): unknown {
  const row: Record<string, unknown> = {};
  for (const { name, at } of slots) {
    row[name] = cells[at];
  }
  return row;
}

/**
 * Decodes a `RowTable` into one object per row, by column name (14 section 10.2): a column the table does not have is
 * `undefined` in every row and extra columns are carried along, so adding a column to a bundle is non-breaking.
 */
export function decodeRows<T>(t: RowTable<T>): T[] {
  const slots = columnSlots(t);
  const out: T[] = [];
  for (const cells of t.rows) {
    out.push(toRow(slots, cells) as T);
  }
  return out;
}

/** `decodeRows` indexed by `id`. The first row wins when an id repeats (a bundle never repeats one). */
export function indexRows<T extends { id: number }>(t: RowTable<T>): Map<number, T>;
/** `decodeRows` indexed by the numeric column `key`. */
export function indexRows<T>(t: RowTable<T>, key: NumericKey<T>): Map<number, T>;
export function indexRows<T>(t: RowTable<T>, key = 'id'): Map<number, T> {
  const slots = columnSlots(t);
  const keyAt = slots.find((s) => s.name === key)?.at;
  const out = new Map<number, T>();
  if (keyAt === undefined) {
    return out;
  }
  for (const cells of t.rows) {
    const k = cells[keyAt];
    if (typeof k === 'number' && !out.has(k)) {
      out.set(k, toRow(slots, cells) as T);
    }
  }
  return out;
}

/** What `getText` answers for a TID at or above the table size (`GETTEXT`, `"error"`). */
export const TEXT_OUT_OF_RANGE = 'error';
/** What `getTextColor` answers for a TID at or above the table size (opaque white). */
export const COLOR_OUT_OF_RANGE = 0xffffffff;

/** The text table of the `texts` bundle, built once by `buildTextTable`. */
export interface TextTable {
  /** The `GETTEXT` bound: every TID at or above it is out of range. */
  readonly size: number;
  readonly texts: ReadonlyMap<number, string>;
  readonly colors: ReadonlyMap<number, number>;
}

/** Builds the lookups of a `texts` bundle (`data` section). */
export function buildTextTable(bundle: Pick<TextsBundle, 'size' | 'tids'>): TextTable {
  const texts = new Map<number, string>();
  const colors = new Map<number, number>();
  const slots = columnSlots(bundle.tids);
  const at = (name: string): number => slots.find((s) => s.name === name)?.at ?? -1;
  const [idAt, colorAt, textAt] = [at('id'), at('color'), at('text')];
  for (const cells of bundle.tids.rows) {
    const id = cells[idAt];
    if (typeof id !== 'number') {
      continue;
    }
    const text = cells[textAt];
    const color = cells[colorAt];
    texts.set(id, typeof text === 'string' ? text : '');
    colors.set(id, typeof color === 'number' ? color : 0);
  }
  return { size: bundle.size, texts, colors };
}

/** `true` when the original's `DWORD` TID would be at or above the table size (a negative or fractional TID never is a valid one). */
function outOfRange(table: TextTable, tid: number): boolean {
  return !Number.isInteger(tid) || tid < 0 || tid >= table.size;
}

/** `GETTEXT(tid)`: `""` for a hole, `"error"` at or above the table size (14 section 10.6). */
export function getText(table: TextTable, tid: number): string {
  if (outOfRange(table, tid)) {
    return TEXT_OUT_OF_RANGE;
  }
  return table.texts.get(tid) ?? '';
}

/** `GETTEXTCOLOR(tid)`: `0` for a hole, `0xFFFFFFFF` at or above the table size. */
export function getTextColor(table: TextTable, tid: number): number {
  if (outOfRange(table, tid)) {
    return COLOR_OUT_OF_RANGE;
  }
  return table.colors.get(tid) ?? 0;
}
