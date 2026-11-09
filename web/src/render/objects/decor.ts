// The decor entries of the sim as the renderer sees them (Plan/41 sections 11.7-11.8, Plan/31 sections 13.7, 17.4, 17.7; WP-REND-23):
// animated static objects ("decor") and the implicit entries of ctrls. Pose evaluation is the sim's (rule B1): the renderer registers
// an object (`DECOR_ADD`, or `CTRL_ADD` by the game's spawn handler), says whether it is drawn (`DECOR_VISIBLE`) and reads the node
// matrices and the world matrix of the entry from the `decor` area of the render view.
//
//   DecorSink    what the renderer tells the sim (the game implements it over `SimBridge.cmd`; the dev pages over a local SimWorld)
//   DecorView    the zero-copy reader of the decor records of one render view (rebuilt per frame, never kept across a call into the module)
//   nodeLocals   the node matrices of an entry -> the parent-relative matrices of the node groups of a `ModelInstance`
//
// Node matrices are `update[i]` of `Object3D::Animate`: the MODEL-space matrix of node `i` including its parents (Plan/31 13.6),
// written in three.js space like every render-view matrix. A `ModelInstance` nests its node groups, so the local matrix of a node is
// `inverse(parent model matrix) * model matrix`.
//
// This module is importable from `src/render/**` only (R15).

import { Matrix4 } from 'three';
import { DECOR as DECOR_BITS, RENDER_VIEW } from '../../gen/sim/wire';
import { rows3ToMatrix4 } from '../space';

export const DECOR_FLAGS = RENDER_VIEW.decorFlags;
export { DECOR_BITS };

const D = RENDER_VIEW.decor;
const STRIDE = RENDER_VIEW.DECOR_STRIDE;
const ROW = 12;

/** `DECOR_ADD` fields (the same shape as `game/simCmd.ts` `DecorAddCmd`). */
export interface DecorAdd {
  readonly id: number;
  readonly kind: number;
  readonly flags: number;
  readonly attach: number;
  readonly modelType: number;
  readonly modelIndex: number;
  readonly clipId: number;
  readonly skelId: number;
  readonly parentMover: number;
  readonly startFrame: number;
  /** The LH `Mat43` (12 floats). */
  readonly local: ArrayLike<number>;
}

/** What the renderer tells the sim about decor entries. */
export interface DecorSink {
  add(c: DecorAdd): void;
  remove(id: number): void;
  /** `DECOR_VISIBLE` of a decor entry. */
  setVisible(id: number, visible: boolean): void;
  /** `SET_VISIBLE` of the implicit entry of ctrl `id` (a ctrl entry is toggled like a mover, 31 section 13.7). */
  setCtrlVisible(id: number, visible: boolean): void;
  /** Hands the node clip (`FMO1`, bit 2) of a model to the sim as the clip `clipId` (once per clip). */
  loadClip(clipId: number, bytes: Uint8Array): void;
  /** The built-in clip of the model `(ot, index)` in the motion table, `null` when it has none. */
  builtinClip(ot: number, index: number): number | null;
}

/** The part of a `RenderViewViews` the decor reader needs. */
export interface DecorViewLike {
  readonly header: {
    readonly frame: number;
    readonly decorCount: number;
    readonly decorOff: number;
    readonly paletteOff: number;
  };
  readonly f32: Float32Array;
  readonly u32: Uint32Array;
}

/** `true` when `view` is a render view with the decor area (`SimRenderViewReader`, `acquireRenderView`). */
export function asDecorView(view: unknown): DecorViewLike | null {
  const v = view as Partial<DecorViewLike> | null;
  if (!v || typeof v !== 'object' || !v.header || !v.f32 || !v.u32) return null;
  if (typeof v.header.decorCount !== 'number' || typeof v.header.decorOff !== 'number') return null;
  const valid = (view as { valid?: boolean }).valid;
  if (valid === false) return null;
  return v as DecorViewLike;
}

const SCRATCH = new Matrix4();

/** The key of an entry in the id index: ctrl entries live in their own namespace. */
const keyOf = (id: number, ctrl: boolean): number => (ctrl ? id + 2 ** 32 : id);

/** The reader over the decor records of one render view. */
export class DecorView {
  private view: DecorViewLike | null = null;
  private readonly index = new Map<number, number>();
  private builtFor: Float32Array | null = null;
  private builtFrame = -1;
  private builtCount = -1;

  /** Binds the view of this frame (`false` when there is none). The id index is rebuilt when the view changed. */
  bind(view: unknown): boolean {
    const v = asDecorView(view);
    this.view = v;
    if (!v) {
      this.index.clear();
      this.builtFor = null;
      return false;
    }
    // the view arrays are re-created after every call into the module: a new array or frame invalidates the index
    if (
      this.builtFor !== v.f32 ||
      this.builtFrame !== v.header.frame ||
      this.builtCount !== v.header.decorCount
    ) {
      this.index.clear();
      const n = v.header.decorCount;
      for (let i = 0; i < n; i++) {
        const at = v.header.decorOff + i * STRIDE;
        const ctrl = ((v.u32[at + D.flags] ?? 0) & DECOR_FLAGS.CTRL) !== 0;
        this.index.set(keyOf(v.u32[at + D.id] ?? 0, ctrl), i);
      }
      this.builtFor = v.f32;
      this.builtFrame = v.header.frame;
      this.builtCount = n;
    }
    return true;
  }

  get count(): number {
    return this.view?.header.decorCount ?? 0;
  }

  /**
   * The record index of entry `id`, `-1` when the sim has no such entry. A decor id (client-local) and a ctrl's `ObjId` are different
   * namespaces: `ctrl` picks the implicit entries of ctrls (`DF_CTRL`).
   */
  find(id: number, ctrl = false): number {
    return this.index.get(keyOf(id, ctrl)) ?? -1;
  }

  private at(i: number, slot: number): number {
    const v = this.view;
    return v ? (v.f32[v.header.decorOff + i * STRIDE + slot] ?? 0) : 0;
  }

  flags(i: number): number {
    const v = this.view;
    return v ? (v.u32[v.header.decorOff + i * STRIDE + D.flags] ?? 0) : 0;
  }

  /** `frame_cur` of the entry's clock (probes). */
  frameCur(i: number): number {
    return this.at(i, D.frameCur);
  }

  /** The palette has an evaluated pose (clip and skeleton loaded, entry visible). */
  hasPose(i: number): boolean {
    return (this.flags(i) & DECOR_FLAGS.HAS_POSE) !== 0;
  }

  /** The clock reached the end of a non-looping play. */
  ended(i: number): boolean {
    return (this.flags(i) & DECOR_FLAGS.ENDED) !== 0;
  }

  /** The first matrix of the entry in the palette area (the `uPaletteBase` of a skinned entry such as a wing). */
  paletteBase(i: number): number {
    const v = this.view;
    return v ? (v.u32[v.header.decorOff + i * STRIDE + D.paletteIndex] ?? 0) : 0;
  }

  /** Number of palette matrices of the entry (nodes of a rigid entry). */
  matrixCount(i: number): number {
    const v = this.view;
    return v ? (v.u32[v.header.decorOff + i * STRIDE + D.matrixCount] ?? 0) : 0;
  }

  /** The world matrix of the entry (three.js space) into `out`. */
  world(i: number, out: Matrix4): Matrix4 {
    const v = this.view;
    if (v) rows3ToMatrix4(v.f32, v.header.decorOff + i * STRIDE + D.world, out);
    return out;
  }

  /** Palette matrix `k` of the entry (three.js space) into `out`; `false` when the entry has no such matrix. */
  matrix(i: number, k: number, out: Matrix4): boolean {
    const v = this.view;
    if (!v || k < 0 || k >= this.matrixCount(i)) return false;
    const first = v.u32[v.header.decorOff + i * STRIDE + D.paletteIndex] ?? 0;
    const off = v.header.paletteOff + (first + k) * ROW;
    if (off + ROW > v.f32.length) return false;
    rows3ToMatrix4(v.f32, off, out);
    return true;
  }
}

/** The part of an object's hierarchy the local-matrix computation needs. */
export interface NodeParent {
  /** The parent object index of the node, `-1` for a root (or a bone-parented, frozen node). */
  readonly parent: number;
}

/**
 * Turns the model-space node matrices of an entry into the parent-relative matrices of a nested node hierarchy:
 * `local[i] = inverse(model[parent]) * model[i]` (`model[i]` for a root). `read(k, out)` fills the palette matrix `k`
 * (`DecorView.matrix`); the result is written with `write(i, local)` for every node with a matrix. Parents precede their
 * children in an `.o3d` (the group nesting relies on it too). Returns the number of nodes written.
 */
export function nodeLocals(
  nodes: readonly NodeParent[],
  read: (k: number, out: Matrix4) => boolean,
  write: (i: number, local: Matrix4) => void,
  scratch: Matrix4[] = [],
): number {
  let written = 0;
  for (let i = 0; i < nodes.length; i++) {
    const m = (scratch[i] ??= new Matrix4());
    if (!read(i, m)) continue;
    const parent = nodes[i]?.parent ?? -1;
    const pm = parent >= 0 && parent < i ? scratch[parent] : undefined;
    if (pm) {
      SCRATCH.copy(pm).invert().multiply(m);
      write(i, SCRATCH);
    } else {
      write(i, m);
    }
    written++;
  }
  return written;
}
