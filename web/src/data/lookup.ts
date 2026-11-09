// Lazy row lookups over `RowTable`s (Plan/40 section 3.4 rule 4, WP-CLIENT-08). `decodeRows` builds one object per
// row up front: ~50 ms for the 8,000 items of the real `items` bundle, work most sessions never need. A
// `RowLookup` indexes a table by its `id` column once (a cheap pass over one column), and decodes a row into an
// object only when it is asked for (then the object is cached, so lookups stay `===`-stable).

import type { RowTable } from '../gen/bundles';

/** `__proto__` is never written as a row property (a bundle column cannot reach the prototype). */
const UNSAFE_COLUMN = '__proto__';

export class RowLookup<T> {
  private readonly slots: { name: string; at: number }[] = [];
  private readonly byKey = new Map<number, number>();
  private readonly cache = new Map<number, T>();
  private all: T[] | null = null;

  /** `key`: the numeric column the rows are looked up by (default `id`). */
  constructor(
    private readonly table: RowTable<T>,
    readonly key = 'id',
  ) {
    table.columns.forEach((name, at) => {
      if (name !== UNSAFE_COLUMN) this.slots.push({ name, at });
    });
    const keyAt = this.slots.find((s) => s.name === key)?.at;
    if (keyAt === undefined) return;
    table.rows.forEach((cells, i) => {
      const k = cells[keyAt];
      if (typeof k === 'number' && !this.byKey.has(k)) this.byKey.set(k, i);
    });
  }

  /** Number of distinct keys. */
  get size(): number {
    return this.byKey.size;
  }

  /** Number of rows of the table (including rows with a repeated or missing key). */
  get rowCount(): number {
    return this.table.rows.length;
  }

  has(id: number): boolean {
    return this.byKey.has(id);
  }

  /** The row with this key, `undefined` for a hole. The same object on every call. */
  get(id: number): T | undefined {
    const hit = this.cache.get(id);
    if (hit !== undefined) return hit;
    const at = this.byKey.get(id);
    if (at === undefined) return undefined;
    const cells = this.table.rows[at];
    if (!cells) return undefined;
    const row = this.decode(cells);
    this.cache.set(id, row);
    return row;
  }

  /** Every row in table order (decodes all of them once; the objects are the cached ones). */
  rows(): readonly T[] {
    if (this.all) return this.all;
    const out: T[] = [];
    const keyAt = this.slots.find((s) => s.name === this.key)?.at;
    this.table.rows.forEach((cells, i) => {
      const k = keyAt === undefined ? undefined : cells[keyAt];
      if (typeof k === 'number' && this.byKey.get(k) === i) {
        const cached = this.cache.get(k);
        if (cached !== undefined) {
          out.push(cached);
          return;
        }
        const row = this.decode(cells);
        this.cache.set(k, row);
        out.push(row);
      } else {
        out.push(this.decode(cells));
      }
    });
    this.all = out;
    return out;
  }

  /** The keys in table order. */
  keys(): IterableIterator<number> {
    return this.byKey.keys();
  }

  private decode(cells: readonly unknown[]): T {
    const row: Record<string, unknown> = {};
    for (const { name, at } of this.slots) row[name] = cells[at];
    return row as T;
  }
}
