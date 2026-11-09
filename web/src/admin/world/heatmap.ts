// The player heatmap (Plan/25 section 3.6, MAY): computed on the client from the frames received since the map was opened, labelled
// "since HH:MM". No server work and no new subscription: every frame adds the players it shows to a grid of 32 m cells, weighted by the
// seconds the frame stands for, so a tier change does not skew it.

/** Cell edge in metres. */
export const HEAT_CELL_M = 32;
/** The grid keeps at most this many cells (a full day of a crowded town stays well below); the sparsest are dropped beyond it. */
export const MAX_HEAT_CELLS = 60_000;

export interface HeatCell {
  /** The south-west corner of the cell. */
  readonly x0: number;
  readonly z0: number;
  readonly value: number;
}

const OFFSET = 16384;

function key(cx: number, cz: number): number {
  return (cx + OFFSET) * 32768 + (cz + OFFSET);
}

export class HeatGrid {
  readonly cellM: number;
  /** When the accumulation started (ms, wall clock): "since HH:MM". */
  since: number;
  readonly #cells = new Map<number, number>();
  #max = 0;

  constructor(since: number, cellM = HEAT_CELL_M) {
    this.since = since;
    this.cellM = cellM;
  }

  get size(): number {
    return this.#cells.size;
  }

  /** The largest cell value. */
  get max(): number {
    return this.#max;
  }

  /** Adds `weight` to the cell holding (x, z). */
  add(x: number, z: number, weight: number): void {
    if (!Number.isFinite(x) || !Number.isFinite(z) || !(weight > 0)) return;
    const k = key(Math.floor(x / this.cellM), Math.floor(z / this.cellM));
    const v = (this.#cells.get(k) ?? 0) + weight;
    this.#cells.set(k, v);
    if (v > this.#max) this.#max = v;
    if (this.#cells.size > MAX_HEAT_CELLS) this.#trim();
  }

  /** The cells that overlap `rect`. */
  cellsIn(rect: readonly [number, number, number, number]): HeatCell[] {
    const out: HeatCell[] = [];
    const cx0 = Math.floor(rect[0] / this.cellM);
    const cx1 = Math.floor(rect[2] / this.cellM);
    const cz0 = Math.floor(rect[1] / this.cellM);
    const cz1 = Math.floor(rect[3] / this.cellM);
    if ((cx1 - cx0 + 1) * (cz1 - cz0 + 1) <= this.#cells.size) {
      for (let cx = cx0; cx <= cx1; cx += 1) {
        for (let cz = cz0; cz <= cz1; cz += 1) {
          const v = this.#cells.get(key(cx, cz));
          if (v !== undefined) out.push({ x0: cx * this.cellM, z0: cz * this.cellM, value: v });
        }
      }
      return out;
    }
    for (const [k, v] of this.#cells) {
      const cx = Math.floor(k / 32768) - OFFSET;
      const cz = (k % 32768) - OFFSET;
      if (cx >= cx0 && cx <= cx1 && cz >= cz0 && cz <= cz1) {
        out.push({ x0: cx * this.cellM, z0: cz * this.cellM, value: v });
      }
    }
    return out;
  }

  clear(since: number): void {
    this.#cells.clear();
    this.#max = 0;
    this.since = since;
  }

  /** Drops the weakest tenth of the cells. */
  #trim(): void {
    const values = [...this.#cells.values()].sort((a, b) => a - b);
    const cut = values[Math.floor(values.length / 10)] ?? 0;
    for (const [k, v] of this.#cells) if (v <= cut) this.#cells.delete(k);
  }
}

/** "since 14:05" for the legend of the layer. */
export function sinceLabel(sinceMs: number): string {
  const d = new Date(sinceMs);
  const p = (n: number): string => (n < 10 ? `0${String(n)}` : String(n));
  return `since ${p(d.getHours())}:${p(d.getMinutes())}`;
}
