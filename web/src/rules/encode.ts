// Encoders and decoders for the array arguments of `RulesContext` (Plan/16 section 18.4, Plan/40 section 7.10):
// the bridge collects one network drain's DST snapshots, ops, forgets and dirty movers in a `DrainBatch` and
// sends them with ONE `drainSync` call.

import {
  DRAIN_ROW_WORDS,
  DST_OP,
  PARAMS_ROW_WORDS,
  type DrainRow,
  type DstTriple,
  type ParamRowIn,
} from './types';

/** The arrays of one `drainSync` call. */
export interface DrainArrays {
  /** `[obj, n, (dst, adj, chg) x n]*`. */
  readonly snapshots: Int32Array;
  /** `[obj, kind, dst, a, b]*`. */
  readonly ops: Int32Array;
  /** `[obj]*`. */
  readonly forgets: Uint32Array;
  /** `[obj, job, level, base_dex, weapon_item]*`. */
  readonly paramRows: Int32Array;
}

/** Collects the contents of one network drain. Object ids are `u32`; they travel as their `i32` bit pattern. */
export class DrainBatch {
  private snapshots: number[] = [];
  private ops: number[] = [];
  private forgets: number[] = [];
  private rows: number[] = [];

  /** A spawn snapshot of `obj` (replaces its replica). */
  addSnapshot(obj: number, triples: readonly DstTriple[]): void {
    this.snapshots.push(obj | 0, triples.length);
    for (const t of triples) this.snapshots.push(t.dst | 0, t.adj | 0, t.chg | 0);
  }

  /** `SetDestParam{dst, adj, chg}` of `obj`. */
  addSet(obj: number, dst: number, adj: number, chg: number): void {
    this.ops.push(obj | 0, DST_OP.set, dst | 0, adj | 0, chg | 0);
  }

  /** `ResetDestParam{dst, adj}` of `obj`. */
  addReset(obj: number, dst: number, adj: number): void {
    this.ops.push(obj | 0, DST_OP.reset, dst | 0, adj | 0, 0);
  }

  /** `ResetDestParamSync{dst, adj, total}` of `obj` (SPEED only). */
  addSync(obj: number, dst: number, adj: number, total: number): void {
    this.ops.push(obj | 0, DST_OP.sync, dst | 0, adj | 0, total | 0);
  }

  /** `obj` despawned. */
  addForget(obj: number): void {
    this.forgets.push(obj >>> 0);
  }

  /** A dirty mover whose derived parameters are wanted back. */
  addParamRow(row: ParamRowIn): void {
    this.rows.push(row.obj | 0, row.job | 0, row.level | 0, row.baseDex | 0, row.weaponItem | 0);
  }

  /** Number of param rows (the result of `drainSync` has {@link DRAIN_ROW_WORDS} words per row). */
  get rowCount(): number {
    return this.rows.length / 5;
  }

  /** `true` when nothing was collected: no call is needed. */
  get isEmpty(): boolean {
    return (
      this.snapshots.length === 0 &&
      this.ops.length === 0 &&
      this.forgets.length === 0 &&
      this.rows.length === 0
    );
  }

  /** The arrays for the call; the batch stays filled until {@link clear}. */
  build(): DrainArrays {
    return {
      snapshots: Int32Array.from(this.snapshots),
      ops: Int32Array.from(this.ops),
      forgets: Uint32Array.from(this.forgets),
      paramRows: Int32Array.from(this.rows),
    };
  }

  /** Empties the batch (after the call). */
  clear(): void {
    this.snapshots = [];
    this.ops = [];
    this.forgets = [];
    this.rows = [];
  }
}

/** The arguments of `dstSnapshot(objs, triples)` for a list of snapshots. */
export function encodeSplitSnapshots(
  list: readonly { readonly obj: number; readonly triples: readonly DstTriple[] }[],
): { readonly objs: Uint32Array; readonly triples: Int32Array } {
  const words: number[] = [];
  for (const s of list) {
    words.push(s.triples.length);
    for (const t of s.triples) words.push(t.dst | 0, t.adj | 0, t.chg | 0);
  }
  return { objs: Uint32Array.from(list.map((s) => s.obj >>> 0)), triples: Int32Array.from(words) };
}

/** The `inputs` argument of `moverParams` for the rows (4 words per object: job, level, base dex, weapon). */
export function encodeMoverParamInputs(rows: readonly ParamRowIn[]): Int32Array {
  const out = new Int32Array(rows.length * 4);
  rows.forEach((r, i) => {
    out.set([r.job | 0, r.level | 0, r.baseDex | 0, r.weaponItem | 0], i * 4);
  });
  return out;
}

/** The row `i` of a `drainSync` result. */
export function readDrainRow(rows: Float32Array, i: number): DrainRow {
  const at = i * DRAIN_ROW_WORDS;
  if (at < 0 || at + DRAIN_ROW_WORDS > rows.length) {
    throw new RangeError(`drain row ${String(i)} is outside ${String(rows.length)} words`);
  }
  const w = (k: number): number => rows[at + k] ?? 0;
  return {
    speedAdjPct: w(0),
    speedChgSet: w(1) !== 0,
    chrState: w(2),
    hawkeyeRate: w(3),
    jumping: w(4),
    attackSpeed: w(5),
    castingAniSpeed: w(6),
  };
}

/** The row `i` of a `moverParams` result (slots 5-6 are `f32` bits, read through a view of the same buffer). */
export function readParamsRow(rows: Int32Array, i: number): DrainRow {
  const at = i * PARAMS_ROW_WORDS;
  if (at < 0 || at + PARAMS_ROW_WORDS > rows.length) {
    throw new RangeError(`params row ${String(i)} is outside ${String(rows.length)} words`);
  }
  const f = new Float32Array(rows.buffer, rows.byteOffset, rows.length);
  const w = (k: number): number => rows[at + k] ?? 0;
  return {
    speedAdjPct: w(0),
    speedChgSet: w(1) !== 0,
    jumping: w(2),
    chrState: w(3),
    hawkeyeRate: w(4),
    attackSpeed: f[at + 5] ?? 0,
    castingAniSpeed: f[at + 6] ?? 0,
  };
}
