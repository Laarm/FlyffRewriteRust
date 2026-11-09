// The view of the sky pass (Plan/41 section 14.1; SkyBox.cpp:278-310). The original builds
// `D3DXMatrixLookAtLH((0, camY, 0), RotY(angle1) * (0, look.y, |look.xz|) + (0, camY, 0), up)` and zeroes the translation: that is the
// camera's own look direction (`lookAt - pos`, yaw AND pitch) as a pure rotation at the origin, `up = (0, 1, 0)`, no roll. The
// camera height and position do not matter; the sky is infinitely far away.
//
// (Plan/41 14.1 and ref/19 OQ5 call the view "yaw-only"; the source keeps the vertical component `vecLookAt.y`, so looking up
// shows the upper sky and looking down the lower band. This module follows the source; NOTES.md records the correction.)

import { lookAtLH, type Mat4 } from '../d3dx';
import type { Vec3LH } from '../types';

/** Looking straight up or down leaves no horizontal direction: the yaw of the previous direction is lost, +Z is used. */
const MIN_HORIZONTAL = 1e-6;

/** The look direction `lookAt - pos`, never degenerate (`+Z` when the two points coincide). */
export function lookDirection(pos: Vec3LH, lookAt: Vec3LH): [number, number, number] {
  const dx = lookAt[0] - pos[0];
  const dy = lookAt[1] - pos[1];
  const dz = lookAt[2] - pos[2];
  if (!(dx * dx + dy * dy + dz * dz > 0)) return [0, 0, 1];
  // `up x z` needs a horizontal part
  if (Math.hypot(dx, dz) < MIN_HORIZONTAL) return [0, dy, MIN_HORIZONTAL * 10];
  return [dx, dy, dz];
}

/** The sky view (LH, D3D row-major memory order): a rotation, translation zero. */
export function skyViewLH(pos: Vec3LH, lookAt: Vec3LH): Mat4 {
  return lookAtLH([0, 0, 0], lookDirection(pos, lookAt), [0, 1, 0]);
}

/**
 * The point the camera looks at on the sphere of radius `radius` around the origin of the sky view (`normalize(lookAt - eye) * 30`,
 * the reference point of the lens flare alpha, SkyBox.cpp:918-921).
 */
export function viewCentre(pos: Vec3LH, lookAt: Vec3LH, radius: number): [number, number, number] {
  const d = lookDirection(pos, lookAt);
  const len = Math.hypot(d[0], d[1], d[2]);
  return [(d[0] / len) * radius, (d[1] / len) * radius, (d[2] / len) * radius];
}

/** The yaw-only right axis of the rain billboards (`LookAtLH` with `y = 0`, inverted: the x axis `up x z`), LH. */
export function yawRight(
  pos: Vec3LH,
  lookAt: Vec3LH,
  previous: readonly [number, number],
): [number, number] {
  const fx = lookAt[0] - pos[0];
  const fz = lookAt[2] - pos[2];
  const len = Math.hypot(fx, fz);
  if (len < MIN_HORIZONTAL) return [previous[0], previous[1]];
  // `normalize(cross((0, 1, 0), (fx, 0, fz) / len))` = `(fz, 0, -fx) / len`
  return [fz / len, -fx / len];
}
