// The DST replica sync of one network drain (Plan/40 section 7.10, Plan/16 section 18.4; WP-CLIENT-30).
//
// The handlers hand over what a drain brought - the spawn snapshots of the DST replicas, the `SetDestParam` /
// `ResetDestParam` / `ResetDestParamSync` ops, the despawned movers and the movers whose attack-speed inputs changed (the
// dirty set) - and `endDrain()` turns it into R-category items for the sim bridge (every queued item runs at the next
// `runRules`, FIFO; the R counter is telemetry):
//
//   * with `RulesContext.drain_sync`: ONE item, ONE boundary call: snapshots, then ops, then forgets, then the param rows
//     (one `[speed_adj_pct, speed_chg_set, chr_state, hawkeye_rate, jumping, attack_speed, casting_ani_speed]` row each),
//   * without it (an older wasm build): the batched exports in the same order - `dst_snapshot`, `dst_apply_ops`,
//     `dst_forget`, then one `mover_params` over the dirty set (the objects `dst_apply_ops` reported, the snapshot objects
//     and the movers whose inputs changed) - one item per call, run in this order.
//
// `dst_view` is debug only and never used. The param rows are resolved when the item RUNS (the inputs of a mover are read
// at that moment, a mover that is gone meanwhile is skipped) and handed to `apply`, which writes them into the `SET_PARAMS`
// of the mover. A reset (`dst_reset_all`, world change and re-attach) is its own item, queued ahead of the next drain.

import type { DrainRow, DstTriple, ParamRowIn, RulesFacade } from '../../rules';
import {
  DrainBatch,
  encodeMoverParamInputs,
  encodeSplitSnapshots,
  readDrainRow,
  readParamsRow,
} from '../../rules';
import type { RulesItem } from '../ports';

/** The part of {@link RulesFacade} the sync needs (a fake in the tests). */
export type RulesPort = Pick<
  RulesFacade,
  'drainSync' | 'dstSnapshot' | 'dstApplyOps' | 'dstForget' | 'dstResetAll' | 'moverParams'
> & {
  /** `false` when the wasm build lacks `drain_sync` (default `true`). */
  readonly hasDrainSync?: boolean;
};

export interface RulesSyncDeps {
  /** The rules, `null` until the context is built (collected work waits). */
  rules(): RulesPort | null;
  /** `SimBridge.enqueueRules`. */
  enqueue(item: RulesItem): void;
  /** The attack-speed inputs of a mover now; `null` when it is gone or has no row (items, ground objects). */
  rowOf(obj: number): ParamRowIn | null;
  /** Writes a returned row into the mover's `SET_PARAMS` inputs. */
  apply(obj: number, row: DrainRow): void;
  /** After the rows of one item were applied (the manager flushes its dirty movers into the command buffer). */
  applied?(): void;
  readonly log?: { warn(scope: string, message: string): void };
}

export interface RulesSyncStats {
  /** `drain_sync` calls. */
  drainCalls: number;
  /** Calls of the fallback path by export name. */
  fallbackCalls: Record<string, number>;
  /** `dst_reset_all` calls. */
  resets: number;
  /** Param rows applied. */
  rowsApplied: number;
  /** Work packages (drains) dropped because the held backlog was too large. */
  dropped: number;
}

/** One drain's collected work. */
interface Collected {
  snapshots: Map<number, readonly DstTriple[]>;
  ops: { obj: number; kind: 0 | 1 | 2; dst: number; a: number; b: number }[];
  forgets: Set<number>;
  dirty: Set<number>;
}

function emptyCollected(): Collected {
  return { snapshots: new Map(), ops: [], forgets: new Set(), dirty: new Set() };
}

/** Bound of the work held while the context does not exist (about a minute of a busy world). */
export const MAX_HELD_OPS = 50_000;

export class RulesSync {
  private cur: Collected = emptyCollected();
  /** Work collected while the rules context did not exist yet. */
  private held: Collected | null = null;
  private resetQueued = false;
  /** The context holds replicas that a reset would drop (something was synced since the last reset). */
  private used = false;
  readonly stats: RulesSyncStats = {
    drainCalls: 0,
    fallbackCalls: {},
    resets: 0,
    rowsApplied: 0,
    dropped: 0,
  };

  constructor(private readonly d: RulesSyncDeps) {}

  // ---- collection (the handlers of one drain) ---------------------------------------------------------------------------

  /**
   * The DST snapshot of a spawned mover (replaces its replica, applied before the ops of the same drain). `wantRow` is false for
   * a monster or NPC without any DST value: its derived parameters are the defaults, no row is needed.
   */
  addSnapshot(obj: number, triples: readonly DstTriple[], wantRow = true): void {
    this.cur.snapshots.set(obj, triples);
    if (wantRow) this.cur.dirty.add(obj);
    // a respawn after a despawn in the same drain keeps its replica
    this.cur.forgets.delete(obj);
  }

  /** A DST op of `obj` (`kind` 0 set, 1 reset, 2 sync). */
  addOp(obj: number, kind: 0 | 1 | 2, dst: number, a: number, b: number): void {
    this.cur.ops.push({ obj, kind, dst, a, b });
    this.cur.dirty.add(obj);
  }

  /** The mover despawned: its replica is dropped (after the ops of the drain). */
  addForget(obj: number): void {
    this.cur.forgets.add(obj);
    this.cur.dirty.delete(obj);
  }

  /** The attack-speed inputs (job, level, dex, weapon) of `obj` changed: its row is wanted back. */
  markDirty(obj: number): void {
    this.cur.dirty.add(obj);
  }

  /**
   * `dst_reset_all` ahead of the next drain's sync (world change and re-attach): what this drain collected so far is
   * stale (it belongs to the old world) and is dropped.
   */
  reset(): void {
    this.cur = emptyCollected();
    this.held = null;
    // nothing was synced since the last reset (the first join of a session): the context is empty, no call is needed
    if (!this.used) return;
    if (this.resetQueued) return;
    this.resetQueued = true;
    this.d.enqueue({
      name: 'dst_reset_all',
      run: () => {
        this.resetQueued = false;
        this.used = false;
        const rules = this.d.rules();
        if (!rules) return;
        rules.dstResetAll();
        this.stats.resets += 1;
      },
    });
  }

  /** `true` when the current drain collected anything. */
  get pending(): boolean {
    return (
      this.cur.snapshots.size > 0 ||
      this.cur.ops.length > 0 ||
      this.cur.forgets.size > 0 ||
      this.cur.dirty.size > 0
    );
  }

  // ---- the end of a drain ---------------------------------------------------------------------------------------------

  /** Turns the collected work into R items (or holds it while the context is missing). */
  endDrain(): void {
    const rules = this.d.rules();
    if (!this.pending && this.held === null) return;
    const work = this.cur;
    this.cur = emptyCollected();
    if (!rules) {
      this.hold(work);
      return;
    }
    const merged = this.held ? mergeCollected(this.held, work) : work;
    this.held = null;
    if (rules.hasDrainSync === false) this.enqueueFallback(merged);
    else this.enqueueSync(merged);
  }

  /** The rules context just came into existence: whatever waited is queued now. */
  onRulesReady(): void {
    if (this.held === null && !this.pending) return;
    this.endDrain();
  }

  private hold(work: Collected): void {
    const merged = this.held ? mergeCollected(this.held, work) : work;
    if (merged.ops.length > MAX_HELD_OPS) {
      // the oldest ops are useless without their snapshots: start over from the snapshots (still correct for new spawns)
      this.stats.dropped += 1;
      this.d.log?.warn(
        'rules',
        `${String(merged.ops.length)} DST ops held without a rules context; dropped`,
      );
      merged.ops.length = 0;
    }
    this.held = merged;
  }

  // ---- the path with drain_sync -----------------------------------------------------------------------------------------

  private enqueueSync(w: Collected): void {
    this.used = true;
    this.d.enqueue({
      name: 'drain_sync',
      run: () => {
        const rules = this.d.rules();
        if (!rules) {
          this.hold(w);
          return;
        }
        const batch = new DrainBatch();
        for (const [obj, triples] of w.snapshots) batch.addSnapshot(obj, triples);
        for (const o of w.ops) {
          if (o.kind === 0) batch.addSet(o.obj, o.dst, o.a, o.b);
          else if (o.kind === 1) batch.addReset(o.obj, o.dst, o.a);
          else batch.addSync(o.obj, o.dst, o.a, o.b);
        }
        for (const obj of w.forgets) batch.addForget(obj);
        const rows = this.resolveRows(w.dirty);
        for (const r of rows) batch.addParamRow(r);
        if (batch.isEmpty) return;
        const a = batch.build();
        const out = rules.drainSync(a.snapshots, a.ops, a.forgets, a.paramRows);
        this.stats.drainCalls += 1;
        rows.forEach((r, i) => {
          this.d.apply(r.obj, readDrainRow(out, i));
        });
        this.stats.rowsApplied += rows.length;
        if (rows.length > 0) this.d.applied?.();
      },
    });
  }

  // ---- the fallback path: dst_snapshot, dst_apply_ops, dst_forget, mover_params -------------------------------------------------

  private enqueueFallback(w: Collected): void {
    this.used = true;
    const changed = new Set<number>();
    const call = (name: string): void => {
      this.stats.fallbackCalls[name] = (this.stats.fallbackCalls[name] ?? 0) + 1;
    };
    if (w.snapshots.size > 0) {
      this.d.enqueue({
        name: 'dst_snapshot',
        run: () => {
          const rules = this.d.rules();
          if (!rules) return;
          const list = [...w.snapshots].map(([obj, triples]) => ({ obj, triples }));
          const enc = encodeSplitSnapshots(list);
          rules.dstSnapshot(enc.objs, enc.triples);
          call('dst_snapshot');
        },
      });
    }
    if (w.ops.length > 0) {
      this.d.enqueue({
        name: 'dst_apply_ops',
        run: () => {
          const rules = this.d.rules();
          if (!rules) return;
          const words: number[] = [];
          for (const o of w.ops) words.push(o.obj | 0, o.kind, o.dst | 0, o.a | 0, o.b | 0);
          const objs = rules.dstApplyOps(Int32Array.from(words));
          call('dst_apply_ops');
          for (const o of objs) changed.add(o);
        },
      });
    }
    if (w.forgets.size > 0) {
      this.d.enqueue({
        name: 'dst_forget',
        run: () => {
          const rules = this.d.rules();
          if (!rules) return;
          rules.dstForget(Uint32Array.from([...w.forgets].map((o) => o >>> 0)));
          call('dst_forget');
        },
      });
    }
    if (w.snapshots.size > 0 || w.ops.length > 0 || w.dirty.size > 0) {
      this.d.enqueue({
        name: 'mover_params',
        run: () => {
          const rules = this.d.rules();
          if (!rules) return;
          // the dirty set: movers whose inputs changed, the ones the ops changed and the freshly spawned
          const set = new Set<number>(w.dirty);
          for (const o of changed) set.add(o);
          const rows = this.resolveRows(set);
          if (rows.length === 0) return;
          const out = rules.moverParams(
            Uint32Array.from(rows.map((r) => r.obj >>> 0)),
            encodeMoverParamInputs(rows),
          );
          call('mover_params');
          rows.forEach((r, i) => {
            this.d.apply(r.obj, readParamsRow(out, i));
          });
          this.stats.rowsApplied += rows.length;
          this.d.applied?.();
        },
      });
    }
  }

  /** The rows of the objects that still exist, ascending object id (a stable order for the arrays). */
  private resolveRows(objs: Iterable<number>): ParamRowIn[] {
    const rows: ParamRowIn[] = [];
    for (const obj of [...objs].sort((a, b) => a - b)) {
      const row = this.d.rowOf(obj);
      if (row) rows.push(row);
    }
    return rows;
  }
}

function mergeCollected(a: Collected, b: Collected): Collected {
  const snapshots = new Map(a.snapshots);
  for (const [obj, t] of b.snapshots) snapshots.set(obj, t);
  const forgets = new Set(a.forgets);
  for (const obj of b.forgets) forgets.add(obj);
  for (const obj of b.snapshots.keys()) forgets.delete(obj);
  return {
    snapshots,
    ops: [...a.ops, ...b.ops],
    forgets,
    dirty: new Set([...a.dirty, ...b.dirty]),
  };
}
