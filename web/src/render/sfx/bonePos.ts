// World positions and matrices of a mover's BONES from the render view (Plan/41 section 12.4, 12.8; WP-REND-27 `WorldQuery.bonePos`): `CModelObject::GetMatrixBone(b)` followed by the
// mover's world matrix, evaluated from the bone palette of the sim's pose (`boneFromPalette`, rule B1: no pose is evaluated in TypeScript). The set-item auras of REND-33 can use it too.
//
//   bonePos   `(local, 1) * bone[b] * world` of the original (row vectors) = `world x bone x local` in the three.js column convention; the local point and the result are LH
//   bonePose  both matrices as LEFT-HANDED D3D row-major arrays (`mirrorMatrix` of the three.js ones), for the classes that multiply them in their own order (the cannon)
//
// A mover without a pose, a skeleton or such a bone answers `false`; the caller falls back to the body position.

import { Matrix4 } from 'three';
import { boneFromPalette, type SkeletonRender } from '../movers/skeleton';
import { newPoseFrame, type MoverPoseFrame } from '../movers/poseSource';
import { mirrorMatrix, rows3ToMatrix4 } from '../space';
import type { V3 } from './SfxSystem';

/** What the bone lookups need of the movers (the pose source and the visual's skeleton). */
export interface BoneHost {
  /** The pose of a mover for this frame; `false` for an unknown mover. */
  read(id: number, out: MoverPoseFrame): boolean;
  /** The skeleton of a mover's visual, `null` while it loads or when the mover has none. */
  skeletonOf(id: number): Pick<SkeletonRender, 'bindTM' | 'boneCount'> | null;
}

/** `FLOATS_PER_MATRIX` of the palette: one 3x4 row-major block per bone. */
const PALETTE_STRIDE = 12;

export class BoneSource {
  private readonly frame = newPoseFrame();
  private readonly pal = new Matrix4();
  private readonly bone = new Matrix4();

  constructor(private readonly host: BoneHost) {}

  /** The three.js matrices `world` and `bone[b]` of mover `id` into `worldOut` / `boneOut`; `false` without a pose. */
  private matrices(id: number, b: number, worldOut: Matrix4, boneOut: Matrix4): boolean {
    const skel = this.host.skeletonOf(id);
    const f = this.frame;
    if (!skel || b < 0 || b >= skel.boneCount || !this.host.read(id, f)) return false;
    if (f.palette === null || f.paletteBase === 0xffffffff || b >= f.boneCount) return false;
    rows3ToMatrix4(f.palette, f.paletteOffset + b * PALETTE_STRIDE, this.pal);
    boneFromPalette(skel, b, this.pal, boneOut);
    worldOut.copy(f.world);
    return true;
  }

  /** `world * bone[b] * local`, all LH, into `out`. */
  pos(id: number, b: number, local: readonly [number, number, number], out: V3): boolean {
    const world = _world;
    if (!this.matrices(id, b, world, this.bone)) return false;
    world.multiply(this.bone);
    // the local point is LH: three.js space negates z
    _v.set(local[0], local[1], -local[2]).applyMatrix4(world);
    out[0] = _v.x;
    out[1] = _v.y;
    out[2] = -_v.z;
    return true;
  }

  /** The world and the bone matrix as LH D3D row-major arrays. */
  pose(id: number, b: number, worldOut: number[], boneOut: number[]): boolean {
    if (!this.matrices(id, b, _world, this.bone)) return false;
    const w = mirrorMatrix(_world.elements);
    const bn = mirrorMatrix(this.bone.elements);
    for (let i = 0; i < 16; i++) {
      worldOut[i] = w[i] ?? 0;
      boneOut[i] = bn[i] ?? 0;
    }
    return true;
  }
}

import { Vector3 } from 'three';
const _world = new Matrix4();
const _v = new Vector3();
