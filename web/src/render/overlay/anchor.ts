// Head anchors and screen projection (Plan/41 §19.1; `D3DXVec3Project` of `CMover::RenderName`).
//
// The anchor of an object is the projection of the model-space point `(0, bb.max.y, 0)` through
// `Scale * Rotation * Translate(logicalPos)`. A yaw rotation leaves a point on the Y axis where it is, so in world
// space the anchor is `(pos.x, pos.y + headY * scale, pos.z)` (`headAnchor`). KEEP-QUIRK: the anchor follows the
// LOGICAL position, not the smoothed model (and a pet's smoothed screen position, which the caller passes in).

import type { Vec3LH } from '../types';

/** `MakeEven`: clears the lowest bit. */
export function even(x: number): number {
  return x & ~1;
}

/** `(pos.x, pos.y + headY * scale, pos.z)`. */
export function headAnchor(pos: Vec3LH, headY: number, scale = 1): Vec3LH {
  return [pos[0], pos[1] + headY * scale, pos[2]];
}

/** A point on the screen in CSS pixels (origin top-left) with its view depth. */
export interface ScreenPos {
  readonly x: number;
  readonly y: number;
  /** Distance in front of the camera plane (> 0 in front). */
  readonly depth: number;
}

/** Projects LH world points to the screen of the current frame. */
export interface Projector {
  readonly width: number;
  readonly height: number;
  /** `null` behind the camera (view z at or behind the plane). */
  project(p: Vec3LH): ScreenPos | null;
}

type M16 = ArrayLike<number>;

/**
 * A projector from the two three.js matrices of the frame (column-major `Matrix4.elements`): `viewProjection` and the
 * view matrix (`matrixWorldInverse`), over a canvas of `width x height` CSS pixels. Points are given in LH game space
 * and mirrored (`z -> -z`) like everything that enters three.js.
 */
export function createProjector(
  viewProjection: M16,
  view: M16,
  width: number,
  height: number,
): Projector {
  const vp = viewProjection;
  const v = view;
  return {
    width,
    height,
    project(p) {
      const x = p[0];
      const y = p[1];
      const z = -p[2];
      const vz = (v[2] ?? 0) * x + (v[6] ?? 0) * y + (v[10] ?? 0) * z + (v[14] ?? 0);
      // three.js looks down -Z: a point in front has a negative view z
      if (vz >= 0) return null;
      const cx = (vp[0] ?? 0) * x + (vp[4] ?? 0) * y + (vp[8] ?? 0) * z + (vp[12] ?? 0);
      const cy = (vp[1] ?? 0) * x + (vp[5] ?? 0) * y + (vp[9] ?? 0) * z + (vp[13] ?? 0);
      const cw = (vp[3] ?? 0) * x + (vp[7] ?? 0) * y + (vp[11] ?? 0) * z + (vp[15] ?? 0);
      if (cw <= 0) return null;
      const nx = cx / cw;
      const ny = cy / cw;
      return { x: (nx * 0.5 + 0.5) * width, y: (1 - (ny * 0.5 + 0.5)) * height, depth: -vz };
    },
  };
}

/** The distance between two LH points (`m_fDistCamera`). */
export function distance(a: Vec3LH, b: Vec3LH): number {
  const dx = a[0] - b[0];
  const dy = a[1] - b[1];
  const dz = a[2] - b[2];
  return Math.sqrt(dx * dx + dy * dy + dz * dz);
}
