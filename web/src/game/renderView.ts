// `RenderViewReader` (Plan/40 section 7.5, Plan/31 section 17.7): zero-copy typed-array views over the buffer
// `SimWorld.render_view` rebuilt, valid until the next call into the module. Any call can grow the linear memory and
// detach `memory.buffer` (rule B4), so the arrays are never cached across calls: every getter compares the buffer it
// last built from with the current one and rebuilds after growth. The header is validated (`'SIMV'` magic, version,
// strides) once per `render_view` call and again whenever the arrays are rebuilt. The reader is one mutable object;
// `update(ptr)` repoints it. It satisfies the structural `RenderViewViews` of `wasm/simTypes` (header + `f32` + `u32`), so
// 41's `SimPoseSource.useView` can consume it directly.

import { RENDER_VIEW, REPORT } from '../gen/sim/wire';
import {
  parseRenderViewHeader,
  RenderViewError,
  type LocalReport,
  type RenderViewHeader,
  type RenderViewViews,
} from '../wasm/simTypes';
import { readReport } from '../wasm/simTypes';

const MV = RENDER_VIEW.mover;
const STRIDE = RENDER_VIEW.MOVER_STRIDE;

/** The memory of the instance (`WebAssembly.Memory`; only `buffer` is read). */
export interface MemoryLike {
  readonly buffer: ArrayBufferLike;
}

/** What 31 section 17.7 says about the render view; the reader of 40 section 7.5. */
export interface RenderViewReader extends RenderViewViews {
  /** The sim frame of the view (`frame_lo` + `frame_hi`). */
  readonly frame: number;
  readonly moverCount: number;
  readonly projCount: number;
  readonly decorCount: number;
  /** Mover records, stride 32 (`RENDER_VIEW.MOVER_STRIDE`), as floats and as `u32` (ids, flags). */
  readonly movers: Float32Array;
  readonly moversU32: Uint32Array;
  /** Consecutive 12-float matrices. */
  readonly palette: Float32Array;
  readonly attach: Float32Array;
  readonly projectiles: Float32Array;
  readonly report: Float32Array;
  readonly reportU32: Uint32Array;
  /** Per-frame id -> record index map (rebuilt lazily after `update`); `-1` for an unknown id. */
  indexOf(objId: number): number;
  /** The `MOVER_FLAGS` of a mover, `0` for an unknown id. */
  flagsOf(objId: number): number;
  /** The logical position (LH) of a mover into `out`; `false` for an unknown id. */
  posOf(objId: number, out: [number, number, number]): boolean;
  /**
   * Where the picture draws a mover in this frame (LH, the units of {@link posOf}): the translation of its world matrix, which
   * `render_view(alpha)` interpolates between the previous and the current smoothed position (PL-009; the heel of a walking
   * player is included in `y`). `false` for an unknown id.
   */
  drawnPosOf(objId: number, out: [number, number, number]): boolean;
  /** The yaw in degrees (LH) of a mover; `NaN` for an unknown id. */
  angleOf(objId: number): number;
  /** The `OBJSTA_*` state word of a mover (slot 20), `0` for an unknown id. */
  stateOf(objId: number): number;
  /** The `OBJSTAF_*` state flags of a mover (slot 21), `0` for an unknown id. */
  stateFlagsOf(objId: number): number;
  /** The `report` bits of the last `step` call (cheap: no decoding). */
  readonly reportBits: number;
  /** The decoded report (allocates; call only when `reportBits` is non-zero). */
  readReport(): LocalReport;
  /** Calls `fn` for every mover record index with its id (iteration without allocation). */
  forEachMover(fn: (index: number, id: number, flags: number) => void): void;
  /** `true` once `update` was called with a valid header. */
  readonly valid: boolean;
}

interface ViewCache {
  readonly f32: Float32Array;
  readonly u32: Uint32Array;
  readonly movers: Float32Array;
  readonly moversU32: Uint32Array;
  readonly palette: Float32Array;
  readonly attach: Float32Array;
  readonly projectiles: Float32Array;
  readonly report: Float32Array;
  readonly reportU32: Uint32Array;
}

/** The reader that `SimBridge` owns. `update` is the bridge's call after `render_view`. */
export class SimRenderViewReader implements RenderViewReader {
  private head: RenderViewHeader | null = null;
  private ptr = 0;
  private builtOn: ArrayBufferLike | null = null;
  private cache: ViewCache | null = null;
  private readonly index = new Map<number, number>();
  private indexFor = -1;
  private generation = 0;

  constructor(private readonly memory: MemoryLike) {}

  get valid(): boolean {
    return this.head !== null;
  }

  /** Repoints the reader at the view of the last `render_view` call (validates the header). */
  update(ptr: number): void {
    this.ptr = ptr;
    const words = new Uint32Array(this.memory.buffer, ptr, RENDER_VIEW.HEADER_WORDS);
    this.head = parseRenderViewHeader(words);
    this.generation += 1;
    this.builtOn = null; // the arrays are rebuilt on first use
    this.cache = null;
  }

  /** Forgets the view (the world was left; the pointer may be reused). */
  invalidate(): void {
    this.head = null;
    this.builtOn = null;
    this.cache = null;
    this.index.clear();
    this.indexFor = -1;
  }

  get header(): RenderViewHeader {
    if (this.head === null) throw new RenderViewError('render view: no view yet');
    return this.head;
  }

  private build(): ViewCache {
    const buffer = this.memory.buffer;
    const hit = this.cache;
    if (hit !== null && this.builtOn === buffer) return hit;
    const h = this.header;
    // re-validate: after growth the header must still read the same (a module bug would show here)
    const check = parseRenderViewHeader(
      new Uint32Array(buffer, this.ptr, RENDER_VIEW.HEADER_WORDS),
    );
    if (check.length !== h.length) throw new RenderViewError('render view: header changed');
    const f32 = new Float32Array(buffer, this.ptr, h.length);
    const u32 = new Uint32Array(buffer, this.ptr, h.length);
    const moversLen = h.moverCount * STRIDE;
    const next: ViewCache = {
      f32,
      u32,
      movers: f32.subarray(h.moversOff, h.moversOff + moversLen),
      moversU32: u32.subarray(h.moversOff, h.moversOff + moversLen),
      palette: f32.subarray(
        h.paletteOff,
        h.paletteOff + h.paletteRows * RENDER_VIEW.PALETTE_ROW_FLOATS,
      ),
      attach: f32.subarray(h.attachOff, h.attachOff + attachCount(h) * RENDER_VIEW.ATTACH_STRIDE),
      projectiles: f32.subarray(h.projOff, h.projOff + h.projCount * RENDER_VIEW.PROJ_STRIDE),
      report: f32.subarray(h.reportOff, h.reportOff + RENDER_VIEW.REPORT_FLOATS),
      reportU32: u32.subarray(h.reportOff, h.reportOff + RENDER_VIEW.REPORT_FLOATS),
    };
    this.cache = next;
    this.builtOn = buffer;
    return next;
  }

  get f32(): Float32Array {
    return this.build().f32;
  }

  get u32(): Uint32Array {
    return this.build().u32;
  }

  get frame(): number {
    return this.header.frame;
  }
  get moverCount(): number {
    return this.head?.moverCount ?? 0;
  }
  get projCount(): number {
    return this.head?.projCount ?? 0;
  }
  get decorCount(): number {
    return this.head?.decorCount ?? 0;
  }
  get movers(): Float32Array {
    return this.build().movers;
  }
  get moversU32(): Uint32Array {
    return this.build().moversU32;
  }
  get palette(): Float32Array {
    return this.build().palette;
  }
  get attach(): Float32Array {
    return this.build().attach;
  }
  get projectiles(): Float32Array {
    return this.build().projectiles;
  }
  get report(): Float32Array {
    return this.build().report;
  }
  get reportU32(): Uint32Array {
    return this.build().reportU32;
  }

  get reportBits(): number {
    if (this.head === null) return 0;
    return this.build().reportU32[REPORT.slot.bits] ?? 0;
  }

  readReport(): LocalReport {
    return readReport(this);
  }

  private ensureIndex(): void {
    if (this.indexFor === this.generation) return;
    this.index.clear();
    const u = this.moversU32;
    const n = this.moverCount;
    for (let i = 0; i < n; i += 1) this.index.set(u[i * STRIDE + MV.id] ?? 0, i);
    this.indexFor = this.generation;
  }

  indexOf(objId: number): number {
    if (this.head === null) return -1;
    this.ensureIndex();
    return this.index.get(objId) ?? -1;
  }

  flagsOf(objId: number): number {
    const i = this.indexOf(objId);
    return i < 0 ? 0 : (this.moversU32[i * STRIDE + MV.flags] ?? 0);
  }

  posOf(objId: number, out: [number, number, number]): boolean {
    const i = this.indexOf(objId);
    if (i < 0) return false;
    const f = this.movers;
    const at = i * STRIDE + MV.pos;
    out[0] = f[at] ?? 0;
    out[1] = f[at + 1] ?? 0;
    out[2] = f[at + 2] ?? 0;
    return true;
  }

  drawnPosOf(objId: number, out: [number, number, number]): boolean {
    const i = this.indexOf(objId);
    if (i < 0) return false;
    const f = this.movers;
    // `render_rows`: three rows of four floats in three.js space, the translation is the fourth column, z negated
    const at = i * STRIDE + MV.world;
    out[0] = f[at + 3] ?? 0;
    out[1] = f[at + 7] ?? 0;
    out[2] = -(f[at + 11] ?? 0);
    return true;
  }

  angleOf(objId: number): number {
    const i = this.indexOf(objId);
    return i < 0 ? Number.NaN : (this.movers[i * STRIDE + MV.angle] ?? 0);
  }

  stateOf(objId: number): number {
    const i = this.indexOf(objId);
    return i < 0 ? 0 : (this.moversU32[i * STRIDE + MV.state] ?? 0);
  }

  stateFlagsOf(objId: number): number {
    const i = this.indexOf(objId);
    return i < 0 ? 0 : (this.moversU32[i * STRIDE + MV.stateFlags] ?? 0);
  }

  forEachMover(fn: (index: number, id: number, flags: number) => void): void {
    if (this.head === null) return;
    const u = this.moversU32;
    const n = this.moverCount;
    for (let i = 0; i < n; i += 1) fn(i, u[i * STRIDE + MV.id] ?? 0, u[i * STRIDE + MV.flags] ?? 0);
  }
}

/** The number of attachment records the view carries: the area ends where the next area starts. */
function attachCount(h: RenderViewHeader): number {
  const next = [h.projOff, h.decorOff, h.reportOff, h.paletteOff]
    .filter((o) => o > h.attachOff)
    .reduce((m, o) => Math.min(m, o), h.length);
  const words = next - h.attachOff;
  return words > 0 ? Math.floor(words / RENDER_VIEW.ATTACH_STRIDE) : 0;
}

/** Layout slots of a mover record, re-exported for the consumers of the reader (pose feeder, probe). */
export const MOVER_SLOT = MV;
export const MOVER_STRIDE = STRIDE;
