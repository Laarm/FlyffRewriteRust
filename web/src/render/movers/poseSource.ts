// Pose sources of the mover visuals (Plan/41 section 12.1, 12.6; WP-REND-04): where a mover's world matrix, bone
// palette and attachment record come from. A `PoseSource` owns ONE palette texture and answers `read(id)` with the
// per-frame data of a mover; the visuals never evaluate a pose in TS (rule B1).
//
//   BindPoseSource   M1: the identity palette (`bone = bindTM`) and the attachment record of the bind pose, built
//                    from the `FSK1` bind matrices; the world matrix is whatever the caller sets.
//   SimPoseSource    M2: `SimWorld.render_view`: world matrices, the palette area and the attachment records written
//                    by the pose evaluation of `flyff-sim` (WP-SIM-14); also drives the palette texture upload.

import { Matrix4, type Texture } from 'three';
import { RENDER_VIEW } from '../../gen/sim/wire';
import { acquireRenderView, type RenderViewViews, type SimWorldHandle } from '../../wasm/simTypes';
import { rows3ToMatrix4 } from '../space';
import { ATTACH_FLOATS, attachRecordFromSkeleton } from './attachments';
import {
  FLOATS_PER_MATRIX,
  PaletteTexture,
  rowsForFloats,
  type PaletteHost,
} from './PaletteTexture';
import type { SkeletonRender } from './skeleton';

/** `render view` mover flags (`wire.ts`). */
export const MOVER_FLAGS = RENDER_VIEW.moverFlags;
/** `palette_index` of a mover without a palette. */
export const NO_PALETTE = RENDER_VIEW.NO_INDEX;

/** The per-frame data of one mover (a reusable scratch object: `read` overwrites it). */
export interface MoverPoseFrame {
  id: number;
  /** `MOVER_FLAGS` bits. */
  flags: number;
  /** First palette matrix (`NO_PALETTE` when the mover has none). */
  paletteBase: number;
  boneCount: number;
  /** The mover world matrix in three.js space (scale and heel included, interpolated). */
  readonly world: Matrix4;
  /** The attachment record (88 floats at `attachOffset`), valid until the next `update`; `null` when absent. */
  attach: ArrayLike<number> | null;
  attachOffset: number;
  /** The palette area (12 floats per matrix, first matrix of the mover at `paletteOffset`); `null` when absent. */
  palette: ArrayLike<number> | null;
  paletteOffset: number;
  /** Logical position (LH) and angle, for distance and LOD decisions. */
  readonly pos: [number, number, number];
  angle: number;
  radius: number;
  /** Un-offset motion id and the clip frame (probes). */
  motion: number;
  /** The stance-offset (resolved) motion id the clip was looked up with (probes: +200 = sword stand in combat stance). */
  motionResolved: number;
  /** `OBJSTAF_*` state flags (probes: `COMBAT` 0x01 selects the weapon motion set). */
  stateFlags: number;
  /** The `OBJSTA_*` state word (the wing clip of a flyer follows `STAND` / `LTURN` / `RTURN`); absent in hand-built frames. */
  state?: number;
  frameCur: number;
  /** `m_fCurrentHeight` (LH y of the ground under the mover, object collision included; blob shadows, Plan/41 13.1). */
  currentHeight: number;
}

/** A fresh scratch frame. */
export function newPoseFrame(): MoverPoseFrame {
  return {
    id: 0,
    flags: 0,
    paletteBase: NO_PALETTE,
    boneCount: 0,
    world: new Matrix4(),
    attach: null,
    attachOffset: 0,
    palette: null,
    paletteOffset: 0,
    pos: [0, 0, 0],
    angle: 0,
    radius: 0,
    motion: 0,
    motionResolved: 0,
    stateFlags: 0,
    state: 0,
    frameCur: 0,
    currentHeight: 0,
  };
}

/** What skinned materials bind: the palette texture uniform of the source. */
export interface PaletteUniform {
  readonly value: Texture | null;
}

export interface PoseSource {
  /** The palette texture uniform (`uPalette`) shared by the skinned materials that follow this source. */
  readonly palette: { value: Texture | null };
  /** Reads the record of mover `id` into `out`; `false` when the source does not know the mover. */
  read(id: number, out: MoverPoseFrame): boolean;
  /** Matrices in the palette texture (probe, `stats.paletteMatrices`). */
  readonly paletteMatrices: number;
  /** Bytes uploaded by the last frame (`stats.paletteBytes`). */
  readonly paletteBytes: number;
}

// ---- M1: the bind pose ------------------------------------------------------------------------------------

interface BindEntry {
  readonly skeleton: SkeletonRender;
  readonly base: number;
  readonly attachOffset: number;
}

/**
 * The identity palette and the bind-pose attachment records of the skeletons that are registered with it. Movers
 * (previews, the viewer) are registered by id with their skeleton; `setWorld` sets the world matrix of an id.
 * `commit()` uploads the palette after skeletons were added (growth of the table re-uploads everything).
 */
export class BindPoseSource implements PoseSource {
  readonly palette: { value: Texture | null };
  private readonly texture: PaletteTexture;
  private readonly bySkeleton = new Map<string, BindEntry>();
  private readonly movers = new Map<number, { entry: BindEntry; world: Matrix4; flags: number }>();
  private matrices = 0;
  private data = new Float32Array(0);
  private attach = new Float32Array(0);
  private dirty = false;

  constructor(host: PaletteHost) {
    this.texture = new PaletteTexture(host);
    this.palette = { value: this.texture.texture };
  }

  get paletteMatrices(): number {
    return this.matrices;
  }

  get paletteBytes(): number {
    return this.texture.last.bytes;
  }

  get paletteTexture(): PaletteTexture {
    return this.texture;
  }

  /** Registers (once per skeleton) the identity palette block and the attachment record of `skeleton`. */
  private entryOf(skeleton: SkeletonRender): BindEntry {
    const known = this.bySkeleton.get(skeleton.key);
    if (known) return known;
    const base = this.matrices;
    const n = skeleton.boneCount;
    const grown = new Float32Array((base + n) * FLOATS_PER_MATRIX);
    grown.set(this.data);
    for (let b = 0; b < n; b++) {
      const at = (base + b) * FLOATS_PER_MATRIX;
      grown[at] = 1; // rows of the identity: (1,0,0,0) (0,1,0,0) (0,0,1,0)
      grown[at + 5] = 1;
      grown[at + 10] = 1;
    }
    this.data = grown;
    this.matrices = base + n;
    const attachOffset = this.attach.length;
    const attach = new Float32Array(attachOffset + ATTACH_FLOATS);
    attach.set(this.attach);
    attachRecordFromSkeleton(skeleton, attach, attachOffset);
    this.attach = attach;
    const entry: BindEntry = { skeleton, base, attachOffset };
    this.bySkeleton.set(skeleton.key, entry);
    this.dirty = true;
    return entry;
  }

  /** Adds mover `id` with `skeleton` (bind pose); a second call replaces the skeleton. */
  add(
    id: number,
    skeleton: SkeletonRender,
    flags: number = MOVER_FLAGS.VISIBLE | MOVER_FLAGS.HAS_POSE,
  ): void {
    const entry = this.entryOf(skeleton);
    const prev = this.movers.get(id);
    this.movers.set(id, { entry, world: prev?.world ?? new Matrix4(), flags });
  }

  remove(id: number): void {
    this.movers.delete(id);
  }

  /** Sets the three.js world matrix of mover `id`. */
  setWorld(id: number, m: Matrix4): void {
    this.movers.get(id)?.world.copy(m);
  }

  /** Uploads the identity palette when skeletons were added since the last upload. */
  commit(): void {
    if (!this.dirty) return;
    this.texture.uploadMatrices(this.data, this.matrices);
    this.dirty = false;
  }

  read(id: number, out: MoverPoseFrame): boolean {
    const m = this.movers.get(id);
    if (!m) return false;
    this.commit();
    out.id = id;
    out.flags = m.flags;
    out.paletteBase = m.entry.base;
    out.boneCount = m.entry.skeleton.boneCount;
    out.world.copy(m.world);
    out.attach = this.attach;
    out.attachOffset = m.entry.attachOffset;
    out.palette = this.data;
    out.paletteOffset = m.entry.base * FLOATS_PER_MATRIX;
    const e = m.world.elements;
    out.pos[0] = e[12];
    out.pos[1] = e[13];
    out.pos[2] = -e[14]; // LH
    out.angle = 0;
    out.radius = 0;
    out.motion = 0;
    out.motionResolved = 0;
    out.stateFlags = 0;
    out.state = 0;
    out.frameCur = 0;
    out.currentHeight = e[13];
    return true;
  }

  onContextLost(): void {
    this.texture.onContextLost();
  }

  onContextRestored(): void {
    this.texture.onContextRestored();
    this.dirty = true;
  }

  dispose(): void {
    this.texture.dispose();
    this.movers.clear();
    this.bySkeleton.clear();
  }
}

// ---- M2: SimWorld.render_view -----------------------------------------------------------------------------

/** The layout slots of a mover record (`wire.ts`). */
const MV = RENDER_VIEW.mover;

/**
 * The pose source over `SimWorld.render_view` (Plan/31 section 17.7). `update(alpha)` calls `render_view`, re-acquires
 * the typed views from `memory.buffer` (rule B4: any call into the module can grow the memory), indexes the mover
 * records by id and uploads the palette rows to the palette texture.
 */
export class SimPoseSource implements PoseSource {
  readonly palette: { value: Texture | null };
  private readonly texture: PaletteTexture;
  private views: RenderViewViews | null = null;
  private readonly index = new Map<number, number>();
  private matrices = 0;

  constructor(
    private readonly sim: Pick<SimWorldHandle, 'render_view'>,
    private readonly memory: () => WebAssembly.Memory,
    host: PaletteHost,
  ) {
    this.texture = new PaletteTexture(host);
    this.palette = { value: this.texture.texture };
  }

  /**
   * A source that is only ever FED views (`useView`): the game's `SimBridge` owns the `render_view` call, the renderer
   * reads the reader it hands to `draw` (rule B4: every wasm call is the bridge's). `update()` throws.
   */
  static followingView(host: PaletteHost): SimPoseSource {
    const never = (): never => {
      throw new Error(
        'this pose source follows the views of the SimBridge: it never calls the sim itself',
      );
    };
    return new SimPoseSource({ render_view: never }, never, host);
  }

  get paletteTexture(): PaletteTexture {
    return this.texture;
  }

  get paletteMatrices(): number {
    return this.matrices;
  }

  get paletteBytes(): number {
    return this.texture.last.bytes;
  }

  /** The current views (valid until the next call into the wasm module). */
  get current(): RenderViewViews | null {
    return this.views;
  }

  /** Number of movers in the current view. */
  get moverCount(): number {
    return this.views?.header.moverCount ?? 0;
  }

  /** Calls `render_view(alpha)` and uploads the palette. Returns the views. */
  update(alpha: number): RenderViewViews {
    const ptr = this.sim.render_view(alpha);
    return this.useView(acquireRenderView(this.memory(), ptr));
  }

  /** Takes an already acquired view (tests, a caller that owns the `render_view` call). */
  useView(v: RenderViewViews): RenderViewViews {
    this.views = v;
    const h = v.header;
    this.index.clear();
    for (let i = 0; i < h.moverCount; i++) {
      this.index.set(v.u32[h.moversOff + i * RENDER_VIEW.MOVER_STRIDE + MV.id] ?? 0, i);
    }
    this.matrices = Math.floor(h.paletteFloats / FLOATS_PER_MATRIX);
    const rows = Math.max(h.paletteRows, rowsForFloats(h.paletteFloats));
    if (rows > 0) this.texture.upload(v.f32, h.paletteOff, rows);
    return v;
  }

  read(id: number, out: MoverPoseFrame): boolean {
    const v = this.views;
    if (!v) return false;
    const i = this.index.get(id);
    if (i === undefined) return false;
    const h = v.header;
    const at = h.moversOff + i * RENDER_VIEW.MOVER_STRIDE;
    out.id = id;
    out.flags = v.u32[at + MV.flags] ?? 0;
    out.paletteBase = v.u32[at + MV.paletteIndex] ?? NO_PALETTE;
    out.boneCount = v.u32[at + MV.boneCount] ?? 0;
    rows3ToMatrix4(v.f32, at + MV.world, out.world);
    const attachIndex = v.u32[at + MV.attachIndex] ?? NO_PALETTE;
    if (attachIndex === NO_PALETTE || (out.flags & MOVER_FLAGS.HAS_ATTACH) === 0) {
      out.attach = null;
      out.attachOffset = 0;
    } else {
      out.attach = v.f32;
      out.attachOffset = h.attachOff + attachIndex * RENDER_VIEW.ATTACH_STRIDE;
    }
    if (out.paletteBase === NO_PALETTE) {
      out.palette = null;
      out.paletteOffset = 0;
    } else {
      out.palette = v.f32;
      out.paletteOffset = h.paletteOff + out.paletteBase * FLOATS_PER_MATRIX;
    }
    out.pos[0] = v.f32[at + MV.pos] ?? 0;
    out.pos[1] = v.f32[at + MV.pos + 1] ?? 0;
    out.pos[2] = v.f32[at + MV.pos + 2] ?? 0;
    out.angle = v.f32[at + MV.angle] ?? 0;
    out.radius = v.f32[at + MV.radius] ?? 0;
    out.motion = v.u32[at + MV.motion] ?? 0;
    out.motionResolved = v.u32[at + MV.motionResolved] ?? 0;
    out.stateFlags = v.u32[at + MV.stateFlags] ?? 0;
    out.state = v.u32[at + MV.state] ?? 0;
    out.frameCur = v.f32[at + MV.frameCur] ?? 0;
    out.currentHeight = v.f32[at + MV.currentHeight] ?? 0;
    return true;
  }

  onContextLost(): void {
    this.texture.onContextLost();
  }

  onContextRestored(): void {
    this.texture.onContextRestored();
  }

  dispose(): void {
    this.texture.dispose();
    this.index.clear();
    this.views = null;
  }
}
