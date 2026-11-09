// Trails of the followed or selected entities (Plan/25 section 3.6, MAY): the last 60 seconds of positions, kept on the client from the frames
// it already receives. A trail samples at most twice a second and only when the entity moved.

/** How long a trail is kept. */
export const TRAIL_MS = 60_000;
/** Samples are at least this far apart in time and distance. */
export const MIN_SAMPLE_MS = 500;
export const MIN_SAMPLE_M = 0.5;

export interface TrailPoint {
  readonly x: number;
  readonly z: number;
  /** 1 for the newest point, fading to 0 at the end of the 60 seconds. */
  readonly age01: number;
}

export class Trail {
  readonly id: number;
  readonly #t: number[] = [];
  readonly #x: number[] = [];
  readonly #z: number[] = [];

  constructor(id: number) {
    this.id = id;
  }

  get length(): number {
    return this.#t.length;
  }

  /** Records a position at client time `t`; `true` when a sample was added. */
  push(t: number, x: number, z: number): boolean {
    const n = this.#t.length;
    if (n > 0) {
      const lt = this.#t[n - 1] ?? 0;
      const lx = this.#x[n - 1] ?? 0;
      const lz = this.#z[n - 1] ?? 0;
      if (t - lt < MIN_SAMPLE_MS) return false;
      if (Math.hypot(x - lx, z - lz) < MIN_SAMPLE_M) return false;
    }
    this.#t.push(t);
    this.#x.push(x);
    this.#z.push(z);
    this.#drop(t);
    return true;
  }

  #drop(now: number): void {
    let cut = 0;
    while (cut < this.#t.length && now - (this.#t[cut] ?? 0) > TRAIL_MS) cut += 1;
    if (cut > 0) {
      this.#t.splice(0, cut);
      this.#x.splice(0, cut);
      this.#z.splice(0, cut);
    }
  }

  /** The points still inside the 60 seconds at `now`, oldest first. */
  points(now: number): TrailPoint[] {
    const out: TrailPoint[] = [];
    for (let i = 0; i < this.#t.length; i += 1) {
      const age = now - (this.#t[i] ?? 0);
      if (age > TRAIL_MS) continue;
      out.push({ x: this.#x[i] ?? 0, z: this.#z[i] ?? 0, age01: 1 - age / TRAIL_MS });
    }
    return out;
  }

  clear(): void {
    this.#t.length = 0;
    this.#x.length = 0;
    this.#z.length = 0;
  }
}

/** A set of trails by entity id with the cap of the plan's use (the followed and the selected ones). */
export class TrailSet {
  readonly #trails = new Map<number, Trail>();
  /** At most this many trails are kept at once. */
  static readonly MAX = 16;

  /** The trail of `id`, created on first use; the oldest-created trail goes when the set is full. */
  get(id: number): Trail {
    let t = this.#trails.get(id);
    if (!t) {
      t = new Trail(id);
      this.#trails.set(id, t);
      if (this.#trails.size > TrailSet.MAX) {
        const first = this.#trails.keys().next().value;
        if (first !== undefined) this.#trails.delete(first);
      }
    }
    return t;
  }

  /** Forgets the trails of entities that are not in `keep`. */
  keepOnly(keep: ReadonlySet<number>): void {
    for (const id of [...this.#trails.keys()]) if (!keep.has(id)) this.#trails.delete(id);
  }

  all(): Trail[] {
    return [...this.#trails.values()];
  }

  clear(): void {
    this.#trails.clear();
  }
}
