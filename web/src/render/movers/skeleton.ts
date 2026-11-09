// `SkeletonRenderType` (Plan/41 section 22.1, WP-REND-04/05): what the renderer keeps of a decoded `.chr` (or of the
// embedded skeleton of an `.o3d`): the special bones, the attachment grips and event points (mapped to three.js
// space by `decode_chr`) and the bind matrices `bindTM[b] = mirrorMat(tm[b])` read from the LEFT-HANDED `FSK1`
// blob (Plan/30 section 13.4 rule 2: `tm` is the only part of the pack TS may read, 16 f32 at byte
// `296 + 196 * b + 4`). The blob itself stays untouched: it goes to `SimWorld.add_skeleton` unchanged.
//
// Spaces (Plan/41 section 3.1): the raw `FSK1` matrices are LH; every one the renderer reads is mapped with
// `mirrorMat` BEFORE it is composed with a render-view, palette or three.js matrix. Row-vector D3D products
// (`a` then `b`) become `Matrix4.multiplyMatrices(b, a)` here (the D3D memory of a matrix IS the column-major
// elements of the same transform in three.js).

import { Matrix4 } from 'three';
import type { ChrMeta, EventMeta, SpecialBones } from '../../gen/formats';
import { mirrorMat } from '../space';

/** The `FSK1` layout (Plan/30 section 13.3). */
export const FSK1 = {
  magic: 0x31_4b_53_46, // "FSK1" little endian
  version: 1,
  headerBytes: 296,
  boneBytes: 196,
  /** Offset of `tm` inside a bone record (after the i32 parent). */
  tmOffset: 4,
  maxBones: 255,
} as const;

/** A skeleton blob that is too short or not an `FSK1`. */
export class SkeletonBuildError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'SkeletonBuildError';
  }
}

/** The attachment grips in three.js space (identity where the file has none). */
export interface SkeletonGrips {
  readonly rh: Matrix4;
  readonly lh: Matrix4;
  readonly shield: Matrix4;
  readonly knuckle: Matrix4;
  /** The `.chr` has its own `local_lh` (ver 7); otherwise `lh` is the identity. */
  readonly hasLh: boolean;
}

/** The renderer's view of one skeleton. */
export interface SkeletonRender {
  /** Canonical VFS path (or any key). */
  readonly key: string;
  readonly boneCount: number;
  readonly parents: readonly number[];
  readonly names: readonly string[];
  readonly special: SpecialBones;
  /** `bindTM` per bone, three.js space: 16 floats (column-major elements) each. */
  readonly bindTM: Float32Array;
  readonly grips: SkeletonGrips;
  readonly events: readonly EventMeta[];
  /** The `FSK1` blob, unchanged (`SimWorld.add_skeleton`). */
  readonly fsk1: Uint8Array;
  /** `bone[b] = bindTM[b]` as a matrix (copy). */
  bindMatrix(b: number, out?: Matrix4): Matrix4;
}

const mat = (a: ArrayLike<number>): Matrix4 => new Matrix4().fromArray(Array.from(a));

/** The `tm` of every bone of an `FSK1`, mapped with `mirrorMat`: `boneCount * 16` floats. */
export function readBindTM(fsk1: Uint8Array, boneCount: number): Float32Array {
  if (fsk1.byteLength < FSK1.headerBytes) {
    throw new SkeletonBuildError(`FSK1 too short (${String(fsk1.byteLength)} bytes)`);
  }
  const dv = new DataView(fsk1.buffer, fsk1.byteOffset, fsk1.byteLength);
  if (dv.getUint32(0, true) !== FSK1.magic) throw new SkeletonBuildError('not an FSK1 blob');
  if (dv.getUint32(4, true) !== FSK1.version) {
    throw new SkeletonBuildError(`FSK1 version ${String(dv.getUint32(4, true))} is not supported`);
  }
  const n = dv.getUint32(8, true);
  if (n !== boneCount || n < 1 || n > FSK1.maxBones) {
    throw new SkeletonBuildError(`FSK1 has ${String(n)} bones, the meta says ${String(boneCount)}`);
  }
  if (fsk1.byteLength < FSK1.headerBytes + FSK1.boneBytes * n) {
    throw new SkeletonBuildError('FSK1 bone table is truncated');
  }
  const out = new Float32Array(n * 16);
  const tm = new Array<number>(16).fill(0);
  const mapped = new Array<number>(16).fill(0);
  for (let b = 0; b < n; b++) {
    const at = FSK1.headerBytes + FSK1.boneBytes * b + FSK1.tmOffset;
    for (let i = 0; i < 16; i++) tm[i] = dv.getFloat32(at + 4 * i, true);
    mirrorMat(tm, mapped);
    out.set(mapped, b * 16);
  }
  return out;
}

/** Builds the render skeleton of a decoded `.chr` (`meta`, whole blob = `FSK1`). */
export function buildSkeletonRender(key: string, meta: ChrMeta, blob: Uint8Array): SkeletonRender {
  const bindTM = readBindTM(blob, meta.bone_count);
  const g = meta.grips;
  const identity = new Matrix4();
  const grips: SkeletonGrips = {
    rh: mat(g.rh),
    lh: g.lh ? mat(g.lh) : identity.clone(),
    shield: mat(g.shield),
    knuckle: mat(g.knuckle),
    hasLh: g.lh !== null,
  };
  const special = meta.special;
  for (const [name, idx] of Object.entries(special)) {
    if (!Number.isInteger(idx) || idx < 0 || idx >= meta.bone_count) {
      throw new SkeletonBuildError(`special bone ${name} = ${String(idx)} is outside the skeleton`);
    }
  }
  return {
    key,
    boneCount: meta.bone_count,
    parents: meta.bones.map((b) => b.parent),
    names: meta.bones.map((b) => b.name),
    special,
    bindTM,
    grips,
    events: meta.events,
    fsk1: blob,
    bindMatrix(b, out = new Matrix4()) {
      if (b < 0 || b >= meta.bone_count) return out.identity();
      return out.fromArray(bindTM, b * 16);
    },
  };
}

/**
 * `bone[b] = bindTM[b] . palette[b]` (row-vector order of Plan/41 section 12.4: the palette entry
 * `mirrorMat(inverse_tm . bone)` applied after the bind matrix). `palette` is the three.js-space palette entry as a
 * `Matrix4`. Since `inverse_tm . tm = I` the product is `mirrorMat(bone)` (the three.js-space bone matrix), which
 * is what rigid parts parented to a bone, attachments and set-item links need.
 */
export function boneFromPalette(
  skel: Pick<SkeletonRender, 'bindTM' | 'boneCount'>,
  b: number,
  palette: Matrix4,
  out = new Matrix4(),
): Matrix4 {
  if (b < 0 || b >= skel.boneCount) return out.identity();
  const bind = _bind.fromArray(skel.bindTM, b * 16);
  // row-vector `bind . palette` = column-vector `palette x bind`
  return out.multiplyMatrices(palette, bind);
}

const _bind = new Matrix4();
