// Attachments of weapons, shields, wings and rides (Plan/41 section 12.4, WP-REND-04; `SetParent`,
// ModelObject.cpp:893-916; ref/18 section 7.3, 11.2, 11.3; Plan/31 sections 13.4, 17.7).
//
// The attachment record of a mover is 88 floats (Plan/31 section 17.7): `RH, LH, SHIELD, KNUCKLE, WING` as five
// 3x4 matrices (three vec4 rows each, three.js space, model space of the mover), 8 event points x 3, the broom lift
// (`bone[0]._42`) and 3 padding floats. `SimWorld.render_view` writes it for every mover with `HAS_ATTACH`; the
// M1 bind-pose source builds the same record from the skeleton (`attachRecordFromSkeleton`).
//
//   RH = local_rh . bone[r_hand]   LH = local_lh . bone[l_hand]   SHIELD = local_shield . bone[l_arm]
//   KNUCKLE = local_knuckle . bone[r_arm]   WING = Rz(-90) . Rx(180) . bone[4] with the translation of bone[5]
//
// (row-vector D3D products; in three.js column-vector terms `bone x local`). A rigid weapon element is drawn at
// `moverWorld x attach x gm.localTM` (its own node transform stays inside the `ModelInstance` hierarchy).

import { Matrix4 } from 'three';
import { IK3_BOW, IK3_KNUCKLEHAMMER, IK3_STICK, IK3_WING, IK3_YOYO } from '../../gen/defs/itemkind';
import { PARTS_LWEAPON, PARTS_RWEAPON, PARTS_SHIELD } from '../../gen/defs/neuz';
import { mirrorMatrix, matrix4ToRows3, rows3ToMatrix4 } from '../space';
import { rad, rotationX, rotationZ } from '../d3dx';
import type { SkeletonRender } from './skeleton';

/** Floats per attachment record. */
export const ATTACH_FLOATS = 88;
/** Float offsets of the record. */
export const ATTACH_AT = {
  rh: 0,
  lh: 12,
  shield: 24,
  knuckle: 36,
  wing: 48,
  events: 60,
  broomLift: 84,
} as const;

/** The attachment points of a mover. */
export type AttachPoint = 'rh' | 'lh' | 'shield' | 'knuckle' | 'wing';

/** The `DECOR_ADD` attach codes (Plan/31 section 17.4): 0 root, 1 RH, 2 LH, 3 SHIELD, 4 KNUCKLE, 5 WING. */
export const ATTACH_CODE: Readonly<Record<AttachPoint, number>> = {
  rh: 1,
  lh: 2,
  shield: 3,
  knuckle: 4,
  wing: 5,
};

/** Feather wings are nudged by `(0, -0.02, +0.1)` in LH model space (MoverRender.cpp:734-760). */
export const FEATHER_OFFSET_LH: readonly [number, number, number] = [0, -0.02, 0.1];

const _m = new Matrix4();
const _a = new Matrix4();

/** `attach` point matrix of the record starting at `rec[base]` into `out` (three.js column-vector matrix). */
export function attachMatrixOf(
  rec: ArrayLike<number>,
  base: number,
  point: AttachPoint,
  out: Matrix4,
): Matrix4 {
  rows3ToMatrix4(rec, base + ATTACH_AT[point], out);
  return out;
}

/** `moverWorld x attach` (the world of a rigid weapon model whose node transforms live in its instance). */
export function weaponWorldMatrix(
  moverWorld: Matrix4,
  rec: ArrayLike<number>,
  base: number,
  point: AttachPoint,
  out = new Matrix4(),
): Matrix4 {
  attachMatrixOf(rec, base, point, _a);
  return out.multiplyMatrices(moverWorld, _a);
}

/**
 * The wing matrix of the bind pose or any pose: `Rz(-90 deg) . Rx(180 deg) . bone[4]` (row-vector order of the C++;
 * the sim's `wing_matrix` has the same order, which differs from the product printed in Plan/31 section 13.4 and
 * ref/18 section 11.3 by a sign in the xy block) with the translation replaced by `bone[5]`'s. `bone4`/`bone5` are
 * three.js-space bone matrices; `feather` adds the LH offset `(0, -0.02, +0.1)` (z mirrored here).
 */
export function wingMatrix(
  bone4: Matrix4,
  bone5: Matrix4,
  feather = false,
  out = new Matrix4(),
): Matrix4 {
  // rotations mapped like every other D3D matrix; column order: bone x Rx x Rz
  const rz = _m.fromArray(mirrorMatrix(rotationZ(rad(-90))));
  const rx = _a.fromArray(mirrorMatrix(rotationX(rad(180))));
  const rot = new Matrix4().multiplyMatrices(rx, rz); // row `Rz . Rx` = column `Rx x Rz`
  out.multiplyMatrices(bone4, rot);
  const e = out.elements;
  const t = bone5.elements;
  e[12] = t[12];
  e[13] = t[13] + (feather ? FEATHER_OFFSET_LH[1] : 0);
  e[14] = t[14] + (feather ? -FEATHER_OFFSET_LH[2] : 0); // LH +0.1 is -0.1 after the z mirror
  return out;
}

/**
 * The bind-pose attachment record of a skeleton (identity palette: `bone[b] = bindTM[b]`), 88 floats in the layout
 * of the render view. `RH = bindTM[r_hand] x grips.rh` (column order of `local_rh . bone[r_hand]`).
 */
export function attachRecordFromSkeleton(
  skel: SkeletonRender,
  out: Float32Array = new Float32Array(ATTACH_FLOATS),
  offset = 0,
): Float32Array {
  out.fill(0, offset, offset + ATTACH_FLOATS);
  const bone = (b: number): Matrix4 => skel.bindMatrix(b, new Matrix4());
  const sp = skel.special;
  const put = (at: number, m: Matrix4): void => {
    matrix4ToRows3(m, out, offset + at);
  };
  put(ATTACH_AT.rh, new Matrix4().multiplyMatrices(bone(sp.r_hand), skel.grips.rh));
  put(ATTACH_AT.lh, new Matrix4().multiplyMatrices(bone(sp.l_hand), skel.grips.lh));
  put(ATTACH_AT.shield, new Matrix4().multiplyMatrices(bone(sp.l_arm), skel.grips.shield));
  put(ATTACH_AT.knuckle, new Matrix4().multiplyMatrices(bone(sp.r_arm), skel.grips.knuckle));
  if (skel.boneCount > 5) put(ATTACH_AT.wing, wingMatrix(bone(4), bone(5)));
  else put(ATTACH_AT.wing, new Matrix4());
  // event points: TransformCoord(pos, bone[parent]), already three.js space
  const v = new Float32Array(3);
  skel.events.slice(0, 8).forEach((ev, i) => {
    if (ev.parent < 0 || ev.parent >= skel.boneCount) return;
    const e = bone(ev.parent).elements;
    const [x, y, z] = ev.pos;
    v[0] = e[0] * x + e[4] * y + e[8] * z + e[12];
    v[1] = e[1] * x + e[5] * y + e[9] * z + e[13];
    v[2] = e[2] * x + e[6] * y + e[10] * z + e[14];
    out.set(v, offset + ATTACH_AT.events + 3 * i);
  });
  out[offset + ATTACH_AT.broomLift] = skel.bindTM[13] ?? 0; // bone[0]._42
  return out;
}

/** Where the element of an equipment slot hangs (`SetParent`, Plan/41 section 12.4). */
export interface Mount {
  /** The equipment slot whose item model this element shows (`PARTS_*`). */
  readonly slot: number;
  /** The attachment point of the mover's record; `null` = not parented (drawn at the mover's world matrix). */
  readonly point: AttachPoint | null;
  /** A yo-yo's second copy: loaded from the RWEAPON item into the LWEAPON slot. */
  readonly copyOf?: number;
}

/**
 * The mounts of the weapon slots for a right-hand item of kind `ik3` (`UpdateParts`, MoverEquip.cpp:296-339):
 * default RH, `IK3_KNUCKLEHAMMER` the forearm (KNUCKLE), `IK3_BOW` the LEFT hand, `IK3_YOYO` RH plus a second copy in
 * the LWEAPON slot on the left hand. The left-hand slot (dual wield) is LH and the shield SHIELD.
 */
export function rightWeaponMounts(ik3: number): Mount[] {
  switch (ik3) {
    case IK3_KNUCKLEHAMMER:
      return [{ slot: PARTS_RWEAPON, point: 'knuckle' }];
    case IK3_BOW:
      return [{ slot: PARTS_RWEAPON, point: 'lh' }];
    case IK3_YOYO:
      return [
        { slot: PARTS_RWEAPON, point: 'rh' },
        { slot: PARTS_LWEAPON, point: 'lh', copyOf: PARTS_RWEAPON },
      ];
    default:
      return [{ slot: PARTS_RWEAPON, point: 'rh' }];
  }
}

/** The mount of the left-hand (dual wield) slot. */
export const LEFT_WEAPON_MOUNT: Mount = { slot: PARTS_LWEAPON, point: 'lh' };
/** The mount of the shield slot. */
export const SHIELD_MOUNT: Mount = { slot: PARTS_SHIELD, point: 'shield' };

/** `true` for an item kind that is a wing (`IK3_WING`): placed at the WING attachment while flying. */
export const isWing = (ik3: number): boolean => ik3 === IK3_WING;
/** `true` for a broom (`IK3_STICK`): lifted by the broom lift while flying. */
export const isBroom = (ik3: number): boolean => ik3 === IK3_STICK;
