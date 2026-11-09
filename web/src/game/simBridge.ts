// SimBridge (Plan/40 section 7, WP-CLIENT-18): the ONLY caller of `SimWorld` (rule CA2). It owns the command writer, the
// queues of loads and rules calls, the render view reader, the event reader and router, the clock offset sent to the
// sim (`SET_CLOCK`) and the counters of the boundary calls of every iteration. The numbers of Plan/40 section 7.3 are
// TELEMETRY (counted by `wasm/call.ts`, reported through `__flyffDebug.loop()`); nothing is refused or deferred because
// of a count (owner 2026-10-29):
//
//   category  calls                                               behaviour
//   F fixed   apply_commands, local_input, step, take_events,     each at most once, inside `frame()`
//             render_view
//   C camera  camera_check_bound                                  the camera asks once per stepped iteration; a second
//                                                                 request in the same iteration returns the first answer
//   Q query   local_act, ray_cast, land_height, full_height, ...  always executed (a missing export answers `undefined`)
//   L loads   set_world, reset_render_smoothing, load_chunk,      every queued load runs at the next iteration, in
//             unload_chunk, add_skeleton, add_clip, memory_report  `Prio` then FIFO order (a load queued by an `onLoad`
//                                                                 callback waits for the iteration after)
//   R rules   RulesContext calls (the items of `enqueueRules`)    every queued item runs at the next `runRules`, FIFO
//
// Every call goes through `wasmCall` (classification of failures, counters). After EVERY call the views are
// re-acquired: the reader and the event reader rebuild their typed arrays when `memory.buffer` changed (rule B4).
// One-time calls (`load_motion_table`, `load_model_bounds`, `load_ctrl_models`) run outside the counters (category O).

import { INPUT_BUTTONS } from '../gen/sim/wire';
import { callStats, categoryCalls, endWasmFrame, wasmCall, type WasmCallError } from '../wasm/call';
import type { SimWorldHandle } from '../wasm/simTypes';
import type {
  BlinkResult,
  CameraBound,
  RayHit,
  RulesItem,
  SimBridge,
  SimFrameResult,
  SimLoad,
  SimStats,
  SimWorldDesc,
} from './ports';
import { SimRenderViewReader, type MemoryLike, type RenderViewReader } from './renderView';
import { SimCmdWriter, type Vec3 } from './simCmd';
import {
  createSimEventRouter,
  SimEventReader,
  type SimEventRecord,
  type SimEventRouter,
} from './simEvents';

/** Exports of `SimWorld` that the input/camera/sim work packages add (Plan/31 17.3); absent ones answer `undefined`. */
export interface SimWorldOptionalExports {
  load_ctrl_models?(bytes: Uint8Array): void;
  ray_cast?(ray: Float32Array): ArrayLike<number>;
  camera_check_bound?(v: Float32Array): ArrayLike<number>;
  height_attr?(x: number, z: number): number;
  blink_point?(v: Float32Array): ArrayLike<number>;
  target_check?(target: number, range: number, coneCos: number): number;
  range_point?(id: number, x: number, y: number, z: number, r: number): boolean;
}

/** The wasm `SimWorld` object as the bridge uses it. */
export type SimWorldFull = SimWorldHandle & SimWorldOptionalExports;

export interface SimBridgeLog {
  warn(scope: string, message: string): void;
  error(scope: string, message: string): void;
  info?(scope: string, message: string): void;
}

export interface SimBridgeDeps {
  readonly sim: SimWorldFull;
  /** The linear memory of the instance (a getter: the module may be re-created in tests). */
  readonly memory: () => MemoryLike;
  /** The clock estimator (`ctx.net.clock`): `SET_CLOCK` carries `offset + frameSkew` once it is synced. */
  readonly clock: { readonly offset: number; readonly synced: boolean };
  /** Monotonic ms (the `memory_report` cadence). */
  readonly now?: () => number;
  readonly log?: SimBridgeLog;
  /**
   * A command buffer was rejected by the sim (`JsError`): a protocol/sim mismatch, the caller requests an L2 resync
   * (Plan/40 section 7.8). The buffer is dropped.
   */
  readonly onCommandError?: (error: WasmCallError) => void;
  /** A chunk/skeleton/clip load finished (`error` = null) or failed (the streamer retries chunks, Plan/40 section 9.7). */
  readonly onLoad?: (load: SimLoad, error: WasmCallError | null) => void;
}

interface QueuedLoad {
  readonly load: SimLoad;
  readonly seq: number;
}

interface QueuedRules {
  readonly item: RulesItem;
  readonly at: number;
}

const EMPTY_EVENTS: readonly SimEventRecord[] = Object.freeze([]);
const MEMORY_REPORT_MS = 1000;
const MEMORY_WARN_BYTES = 240 * 1024 * 1024;
const EDGE_BITS =
  INPUT_BUTTONS.WALK_TOGGLE |
  INPUT_BUTTONS.ACC_TOGGLE |
  INPUT_BUTTONS.TURBO_TOGGLE |
  INPUT_BUTTONS.LMB_DOWN |
  INPUT_BUTTONS.MOUSE_MOVED;
const NO_CAMERA: CameraBound = { crash: false, out: [0, 0, 0], len: 0 };

function bytesOf(b: ArrayBuffer | Uint8Array): Uint8Array {
  return b instanceof Uint8Array ? b : new Uint8Array(b);
}

export class SimBridgeImpl implements SimBridge {
  readonly cmd = new SimCmdWriter();
  readonly events: SimEventRouter;
  private readonly reader: SimRenderViewReader;
  private readonly eventReader: SimEventReader;
  private readonly sim: SimWorldFull;
  private readonly now: () => number;
  private readonly loads: QueuedLoad[] = [];
  private loadsDirty = false;
  private loadSeq = 0;
  private readonly rulesQueue: QueuedRules[] = [];
  private readonly loadedChunks = new Set<number>();
  private readonly lastInput = new Int32Array(5);
  private inputSent = false;
  private readonly worldDesc = new Float32Array(6);
  private currentWorld = 0;
  private skew = 0;
  private sentClock: number | null = null;
  private resetPending = false;
  private lastMemoryReportAt = Number.NEGATIVE_INFINITY;
  private memBytes = 0;
  private memWarned = false;
  private boundsLoaded = false;
  private lastCamera: CameraBound = NO_CAMERA;
  private freshView = false;
  private eventsDropped = 0;
  private commandsApplied = 0;
  private commandErrors = 0;
  private readonly warnedMissing = new Set<string>();
  private forced: { buttons: number; until: number } | null = null;
  private readonly forcedRecord = new Int32Array(5);
  private readonly ray = new Float32Array(8);
  private readonly v6 = new Float32Array(6);
  private readonly v7 = new Float32Array(7);

  constructor(private readonly deps: SimBridgeDeps) {
    this.sim = deps.sim;
    this.now = deps.now ?? (() => performance.now());
    this.reader = new SimRenderViewReader({
      get buffer() {
        return deps.memory().buffer;
      },
    });
    this.eventReader = new SimEventReader({
      get buffer() {
        return deps.memory().buffer;
      },
    });
    this.events = createSimEventRouter(deps.log ? { log: deps.log } : {});
  }

  // ---- lifecycle --------------------------------------------------------------------------------------------

  get worldId(): number {
    return this.currentWorld;
  }

  get frameSkew(): number {
    return this.skew;
  }

  get modelBoundsLoaded(): boolean {
    return this.boundsLoaded;
  }

  get view(): RenderViewReader | null {
    return this.freshView ? this.reader : null;
  }

  setFrameSkew(skew: number): void {
    this.skew = skew;
  }

  forceInput(buttons: number, durationMs: number): void {
    this.forced = { buttons, until: this.now() + durationMs };
  }

  resetRenderSmoothing(): void {
    this.resetPending = true;
  }

  setWorld(desc: SimWorldDesc): void {
    const w = this.worldDesc;
    w[0] = desc.worldId;
    w[1] = desc.mpu;
    w[2] = desc.landsW;
    w[3] = desc.landsH;
    w[4] = desc.flyAllowed ? 1 : 0;
    w[5] = desc.housing ? 1 : 0;
    wasmCall(
      'set_world',
      () => {
        this.sim.set_world(w);
      },
      'L',
    );
    this.afterWorldChange(desc.worldId);
  }

  leaveWorld(): void {
    if (this.currentWorld === 0) return;
    this.worldDesc.fill(0);
    wasmCall(
      'set_world',
      () => {
        this.sim.set_world(this.worldDesc);
      },
      'L',
    );
    this.afterWorldChange(0);
  }

  private afterWorldChange(worldId: number): void {
    this.currentWorld = worldId;
    // everything the old world's movers were addressed by is gone (31 17.3): the pending records are void, the clock stays
    this.cmd.truncateKeepingClock();
    for (let i = this.loads.length - 1; i >= 0; i -= 1) {
      const k = this.loads[i]?.load.kind;
      if (k === 'chunk' || k === 'unchunk') this.loads.splice(i, 1);
    }
    this.loadedChunks.clear();
    this.reader.invalidate();
    this.freshView = false;
    this.lastCamera = NO_CAMERA;
    this.resetPending = false;
    this.inputSent = false;
  }

  private once(name: string, fn: () => void): void {
    wasmCall(name, fn, 'O');
  }

  loadMotionTable(bytes: ArrayBuffer | Uint8Array): void {
    this.once('load_motion_table', () => {
      this.sim.load_motion_table(bytesOf(bytes));
    });
  }

  loadModelBounds(bytes: ArrayBuffer | Uint8Array): void {
    this.once('load_model_bounds', () => {
      this.sim.load_model_bounds(bytesOf(bytes));
    });
    this.boundsLoaded = true;
  }

  loadCtrlModels(bytes: ArrayBuffer | Uint8Array): void {
    if (!this.has('load_ctrl_models')) return;
    this.once('load_ctrl_models', () => {
      this.sim.load_ctrl_models?.(bytesOf(bytes));
    });
  }

  /** `true` when this build of the module exports `name`; a missing export is logged once and its calls are ignored. */
  private has(name: keyof SimWorldOptionalExports): boolean {
    if (typeof this.sim[name] === 'function') return true;
    if (!this.warnedMissing.has(name)) {
      this.warnedMissing.add(name);
      this.deps.log?.warn('sim', `the wasm build has no ${name} yet; the call is ignored`);
    }
    return false;
  }

  // ---- loads and rules ----------------------------------------------------------------------------------------

  enqueueLoad(load: SimLoad): void {
    if (load.kind === 'unchunk') {
      // a chunk that never reached the sim needs no unload: drop the queued load instead
      const i = this.loads.findIndex(
        (q) => q.load.kind === 'chunk' && q.load.lx === load.lx && q.load.lz === load.lz,
      );
      if (i >= 0) {
        this.loads.splice(i, 1);
        return;
      }
    }
    this.loads.push({ load, seq: this.loadSeq++ });
    this.loadsDirty = true;
  }

  enqueueRules(item: RulesItem): void {
    this.rulesQueue.push({ item, at: this.now() });
  }

  /**
   * Runs every rules item that is queued when the call starts, FIFO (the order the sync relies on: a reset before the next
   * drain, a `derived_stats` after the drain that brought its buffs). An item queued by a running item waits for the next
   * call; an exception of an item propagates and the items after it stay queued. The calls an item makes through
   * `rules/facade.ts` are counted by `wasmCall` themselves (category R); an item without a wasm call is not counted.
   */
  runRules(): boolean {
    let n = this.rulesQueue.length;
    let ran = false;
    while (n-- > 0) {
      const q = this.rulesQueue.shift();
      if (!q) break;
      q.item.run();
      ran = true;
    }
    return ran;
  }

  /** Queries are never refused (owner 2026-10-29); kept for the callers that still ask. Finite: the probes serialise JSON. */
  queryBudget(): number {
    return Number.MAX_SAFE_INTEGER;
  }

  /**
   * Every load that is queued when the loop starts, in `Prio` then FIFO order, plus `reset_render_smoothing` (first) and the 1 Hz
   * `memory_report` (last). No count cap and no time slice; a load queued by an `onLoad` callback waits for the next iteration.
   */
  private runLoads(): void {
    if (this.resetPending) {
      this.resetPending = false;
      wasmCall(
        'reset_render_smoothing',
        () => {
          this.sim.reset_render_smoothing();
        },
        'L',
      );
    }
    if (this.loadsDirty) {
      this.loads.sort((a, b) => a.load.prio - b.load.prio || a.seq - b.seq);
      this.loadsDirty = false;
    }
    let n = this.loads.length;
    while (n-- > 0) {
      const q = this.loads.shift();
      if (!q) break;
      this.runLoad(q.load);
    }
    const t = this.now();
    if (t - this.lastMemoryReportAt >= MEMORY_REPORT_MS) {
      this.lastMemoryReportAt = t;
      wasmCall('memory_report', () => this.sim.memory_report(), 'L');
      this.memBytes = this.deps.memory().buffer.byteLength;
      if (this.memBytes > MEMORY_WARN_BYTES && !this.memWarned) {
        this.memWarned = true;
        this.deps.log?.warn(
          'sim',
          `wasm memory is ${String(Math.round(this.memBytes / 1048576))} MB (budget 256 MB)`,
        );
      }
    }
  }

  private runLoad(load: SimLoad): void {
    try {
      switch (load.kind) {
        case 'chunk':
          wasmCall(
            'load_chunk',
            () => {
              this.sim.load_chunk(load.lx, load.lz, load.bytes);
            },
            'L',
          );
          this.loadedChunks.add(load.lx * 65536 + load.lz);
          break;
        case 'unchunk':
          wasmCall(
            'unload_chunk',
            () => {
              this.sim.unload_chunk(load.lx, load.lz);
            },
            'L',
          );
          this.loadedChunks.delete(load.lx * 65536 + load.lz);
          break;
        case 'skeleton':
          wasmCall(
            'add_skeleton',
            () => {
              this.sim.add_skeleton(load.id, load.bytes);
            },
            'L',
          );
          break;
        case 'clip':
          wasmCall(
            'add_clip',
            () => {
              this.sim.add_clip(load.id, load.bytes);
            },
            'L',
          );
          break;
      }
      this.deps.onLoad?.(load, null);
    } catch (e) {
      if (isTrap(e)) throw e;
      this.deps.log?.warn(
        'sim',
        `${load.kind} load failed: ${e instanceof Error ? e.message : String(e)}`,
      );
      this.deps.onLoad?.(load, e as WasmCallError);
    }
  }

  // ---- the frame ----------------------------------------------------------------------------------------------

  frame(due: number, record: unknown, alpha: number | null): SimFrameResult {
    const n = Math.max(0, Math.trunc(due));
    const wantView = n >= 1 || alpha !== null;
    this.freshView = false;
    if (!wantView) return { stepped: 0, viewFresh: false, view: null, events: EMPTY_EVENTS };

    this.runLoads();
    this.syncClock();

    // F1: apply_commands
    if (!this.cmd.isEmpty) {
      const bytes = this.cmd.peek();
      try {
        wasmCall(
          'apply_commands',
          () => {
            this.commandsApplied += this.sim.apply_commands(bytes);
          },
          'F',
        );
      } catch (e) {
        if (isTrap(e)) throw e;
        this.commandErrors += 1;
        this.deps.log?.error(
          'sim',
          `apply_commands rejected: ${e instanceof Error ? e.message : String(e)}`,
        );
        this.cmd.reset();
        this.deps.onCommandError?.(e as WasmCallError);
      }
      this.cmd.reset();
    }

    // F2: local_input (changed record or edge bits)
    if (n >= 1) {
      const rec = this.forcedInput() ?? (record as ArrayLike<number> | null | undefined);
      if (rec !== null && rec !== undefined) this.sendInput(rec);
    }

    // F3/F4: step and take_events
    let stepped = 0;
    let events: readonly SimEventRecord[] = EMPTY_EVENTS;
    if (n >= 1) {
      stepped = wasmCall('step', () => this.sim.step(n), 'F');
      const ptr = wasmCall('take_events', () => this.sim.take_events(), 'F');
      const batch = this.eventReader.read(ptr);
      events = batch.events;
      this.eventsDropped += batch.dropped;
    }

    // F5: render_view (also in a non-rendered iteration that stepped: it rebuilds the report area, 40 section 7.3)
    const a = alpha ?? 1;
    const vp = wasmCall('render_view', () => this.sim.render_view(a), 'F');
    this.reader.update(vp);
    this.freshView = true;
    return { stepped, viewFresh: true, view: this.reader, events };
  }

  /** The record of a `forceInput` while it is active, one released record when it ends, else `null`. */
  private forcedInput(): ArrayLike<number> | null {
    const f = this.forced;
    if (!f) return null;
    const r = this.forcedRecord;
    r[3] = 1;
    if (this.now() < f.until) {
      r[0] = f.buttons;
    } else {
      r[0] = 0;
      this.forced = null;
    }
    return r;
  }

  private sendInput(rec: ArrayLike<number>): void {
    const last = this.lastInput;
    let same = this.inputSent;
    for (let i = 0; i < 5; i += 1) {
      if ((rec[i] ?? 0) !== last[i]) same = false;
    }
    // edge bits (toggles, the LMB press) and mouse deltas apply to ONE frame: a repeated record must be sent again
    const edges = (rec[0] ?? 0) & EDGE_BITS;
    if (same && edges === 0) return;
    for (let i = 0; i < 5; i += 1) last[i] = rec[i] ?? 0;
    this.inputSent = true;
    wasmCall(
      'local_input',
      () => {
        this.sim.local_input(last);
      },
      'F',
    );
  }

  /** `SET_CLOCK` at the start of the buffer when `clock.offset + frameSkew` changed (at most one per frame). */
  private syncClock(): void {
    if (!this.deps.clock.synced) return;
    const offset = Math.round(this.deps.clock.offset + this.skew);
    if (offset === this.sentClock) return;
    this.sentClock = offset;
    this.cmd.prependClock(offset);
  }

  /** Step 12 of the loop: folds the iteration's counters into the budget statistics and starts the next iteration. */
  endFrame(): void {
    endWasmFrame();
  }

  // ---- queries (category Q, never refused) ------------------------------------------------------------------------

  private query<T>(name: string, fn: () => T): T | undefined {
    return wasmCall(name, fn, 'Q');
  }

  localAct(msg: number, p: readonly [number, number, number, number, number]): number | undefined {
    return this.query('local_act', () => this.sim.local_act(msg, p[0], p[1], p[2], p[3], p[4]));
  }

  rayCast(origin: Vec3, dir: Vec3, maxT: number, mask: number): RayHit | null | undefined {
    if (!this.has('ray_cast')) return undefined;
    const r = this.ray;
    r[0] = origin[0];
    r[1] = origin[1];
    r[2] = origin[2];
    r[3] = dir[0];
    r[4] = dir[1];
    r[5] = dir[2];
    r[6] = maxT;
    r[7] = mask;
    const out = this.query('ray_cast', () => this.sim.ray_cast?.(r));
    if (out === undefined) return undefined;
    if ((out[0] ?? 0) === 0) return null;
    return {
      pos: [out[1] ?? 0, out[2] ?? 0, out[3] ?? 0],
      t: out[4] ?? 0,
      kind: out[5] ?? 0,
      id: floatBits(out[6] ?? 0),
      normal: [out[7] ?? 0, out[8] ?? 0, out[9] ?? 0],
    };
  }

  landHeight(x: number, z: number): number | undefined {
    return this.query('land_height', () => this.sim.land_height(x, z));
  }

  fullHeight(p: Vec3): number | undefined {
    return this.query('full_height', () => this.sim.full_height(p[0], p[1], p[2]));
  }

  heightAttr(x: number, z: number): number | undefined {
    if (!this.has('height_attr')) return undefined;
    return this.query('height_attr', () => this.sim.height_attr?.(x, z));
  }

  blinkPoint(self: Vec3, to: Vec3, range: number): BlinkResult | undefined {
    if (!this.has('blink_point')) return undefined;
    const v = this.v7;
    v.set([self[0], self[1], self[2], to[0], to[1], to[2], range]);
    const out = this.query('blink_point', () => this.sim.blink_point?.(v));
    if (out === undefined) return undefined;
    return { pos: [out[0] ?? 0, out[1] ?? 0, out[2] ?? 0], ok: (out[3] ?? 0) !== 0 };
  }

  targetCheck(target: number, range: number, coneCos: number): number | undefined {
    if (!this.has('target_check')) return undefined;
    return this.query('target_check', () => this.sim.target_check?.(target, range, coneCos));
  }

  rangePoint(id: number, p: Vec3, r: number): boolean | undefined {
    if (!this.has('range_point')) return undefined;
    return this.query('range_point', () => this.sim.range_point?.(id, p[0], p[1], p[2], r));
  }

  cameraCheckBound(eye: Vec3, lookAt: Vec3): CameraBound {
    if (!this.has('camera_check_bound')) return { crash: false, out: eye, len: 0 };
    if (categoryCalls('C') >= 1) return this.lastCamera;
    const v = this.v6;
    v.set([eye[0], eye[1], eye[2], lookAt[0], lookAt[1], lookAt[2]]);
    const out = wasmCall('camera_check_bound', () => this.sim.camera_check_bound?.(v), 'C');
    if (out === undefined) return this.lastCamera;
    this.lastCamera = {
      crash: (out[0] ?? 0) !== 0,
      out: [out[1] ?? 0, out[2] ?? 0, out[3] ?? 0],
      len: out[4] ?? 0,
    };
    return this.lastCamera;
  }

  // ---- stats --------------------------------------------------------------------------------------------------

  get stats(): SimStats {
    const c = callStats();
    return {
      callsThisFrame: c.callsThisFrame,
      maxCalls: c.maxCalls,
      byCategory: c.byCategory,
      overBudget: c.overBudget,
      uiCalls: c.uiCalls,
      eventsDropped: this.eventsDropped,
      memBytes: this.memBytes,
      chunksLoaded: this.loadedChunks.size,
      loadsPending: this.loads.length,
      commandsApplied: this.commandsApplied,
      commandErrors: this.commandErrors,
    };
  }
}

const scratch = new DataView(new ArrayBuffer(4));

/** The `u32` whose bits are the `f32` `v` (`ray_cast` returns the object id bit-cast into a float slot). */
function floatBits(v: number): number {
  scratch.setFloat32(0, v, true);
  return scratch.getUint32(0, true);
}

function isTrap(e: unknown): boolean {
  return typeof e === 'object' && e !== null && (e as { kind?: unknown }).kind === 'trap';
}

/** Creates the bridge over a `SimWorld`. */
export function createSimBridge(deps: SimBridgeDeps): SimBridgeImpl {
  return new SimBridgeImpl(deps);
}
