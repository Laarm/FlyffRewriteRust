// A binary min-heap ordered by `compareRank` with lazy invalidation (Plan/40 section 8.3): the pool never
// removes from the middle of the heap. Cancelling or re-ranking an item bumps its `version`; entries whose
// version no longer matches are skipped when they surface.

import { compareRank, type Rank } from './priority';

/** An item the queue can hold: it knows whether a heap entry is still the current one. */
export interface Queueable {
  /** Bumped whenever the item is cancelled or re-ranked; stale entries are dropped on `pop`. */
  version: number;
  /** `false` once the item left the queued state. */
  queued: boolean;
}

interface Entry<T extends Queueable> extends Rank {
  readonly item: T;
  readonly version: number;
}

export class PriorityQueue<T extends Queueable> {
  private heap: Entry<T>[] = [];

  /** Entries in the heap, stale ones included (diagnostics). */
  get entries(): number {
    return this.heap.length;
  }

  /** Adds `item` with the rank `rank` (call again after changing the rank and bumping `item.version`). */
  push(item: T, rank: Rank): void {
    const entry: Entry<T> = {
      prio: rank.prio,
      distance: rank.distance,
      seq: rank.seq,
      item,
      version: item.version,
    };
    this.heap.push(entry);
    this.up(this.heap.length - 1);
  }

  private live(e: Entry<T>): boolean {
    return e.item.queued && e.item.version === e.version;
  }

  /**
   * Removes and returns the best live item for which `accept` is true (default: any). Live items that were
   * skipped because `accept` refused them stay queued.
   */
  pop(accept: (item: T) => boolean = () => true): T | undefined {
    const skipped: Entry<T>[] = [];
    let found: T | undefined;
    while (this.heap.length > 0) {
      const top = this.removeTop();
      if (!top || !this.live(top)) continue;
      if (accept(top.item)) {
        found = top.item;
        break;
      }
      skipped.push(top);
    }
    for (const e of skipped) {
      this.heap.push(e);
      this.up(this.heap.length - 1);
    }
    return found;
  }

  /** Drops every stale entry (called when stale entries dominate; O(n)). */
  compact(): void {
    const live = this.heap.filter((e) => this.live(e));
    this.heap = [];
    for (const e of live) {
      this.heap.push(e);
      this.up(this.heap.length - 1);
    }
  }

  private removeTop(): Entry<T> | undefined {
    const top = this.heap[0];
    const last = this.heap.pop();
    if (this.heap.length > 0 && last) {
      this.heap[0] = last;
      this.down(0);
    }
    return top;
  }

  private up(index: number): void {
    let i = index;
    while (i > 0) {
      const parent = (i - 1) >> 1;
      const a = this.heap[i];
      const b = this.heap[parent];
      if (!a || !b || compareRank(a, b) >= 0) return;
      this.heap[i] = b;
      this.heap[parent] = a;
      i = parent;
    }
  }

  private down(index: number): void {
    let i = index;
    for (;;) {
      const left = 2 * i + 1;
      const right = left + 1;
      let best = i;
      const at = (k: number): Entry<T> | undefined => this.heap[k];
      const cur = at(best);
      const l = at(left);
      const r = at(right);
      if (l && cur && compareRank(l, cur) < 0) best = left;
      const bestEntry = at(best);
      if (r && bestEntry && compareRank(r, bestEntry) < 0) best = right;
      if (best === i) return;
      const a = this.heap[i];
      const b = this.heap[best];
      if (!a || !b) return;
      this.heap[i] = b;
      this.heap[best] = a;
      i = best;
    }
  }
}
