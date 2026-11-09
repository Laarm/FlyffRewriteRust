// Orientation helpers of the directional effects (Plan/41 section 17.4, 3.1 test (b); Sfx.cpp:780-792, xUtil3D.cpp:7-22).
//
// A directional projectile turns the model's forward axis `(0, 0, 1)` to the flight direction with the quaternion
// `D3DXQuaternionRotationAxis(cross((0,0,1), dir), acos(dot))`, decodes it with `QuaternionRotationToYPW` into yaw, pitch and
// roll and draws the effect through the 3-axis path (`Render2`, rotation vector `(x = pitch, y = yaw, z = roll)` in degrees).
// All of it is evaluated in the LEFT-HANDED game space; the finished matrix is mapped to three.js once by the draw list.

export type V3 = [number, number, number];
export type Quat4 = [number, number, number, number];

/**
 * The 3-axis rotation of a directional effect (`CSfxShoot`, Sfx.cpp:780-792): the quaternion that turns `(0, 0, 1)` to the
 * direction `from -> to`, decoded by `QuaternionRotationToYPW` and given in degrees `(x = pitch, y = yaw, z = roll)`.
 */
export function directionRot3(from: readonly number[], to: readonly number[]): V3 {
  const dx = (to[0] ?? 0) - (from[0] ?? 0);
  const dy = (to[1] ?? 0) - (from[1] ?? 0);
  const dz = (to[2] ?? 0) - (from[2] ?? 0);
  return quaternionToYpw(rotationBetween(0, 0, 1, dx, dy, dz));
}

/** The click-to-move mark's rotation from the surface normal (`up = (0, -1, 0)`, WndWorldControlPlayer.cpp:347-371). */
export function surfaceRot3(normal: readonly number[]): V3 {
  return quaternionToYpw(rotationBetween(0, -1, 0, normal[0] ?? 0, normal[1] ?? 0, normal[2] ?? 0));
}

/** `D3DXQuaternionRotationAxis(cross(a, b), acos(dot(a, b)))` with a normalised `b` (a zero axis stays a zero axis). */
export function rotationBetween(
  ax: number,
  ay: number,
  az: number,
  bx: number,
  by: number,
  bz: number,
): Quat4 {
  const bl = Math.hypot(bx, by, bz);
  if (bl === 0) return [0, 0, 0, 1];
  const nx = bx / bl;
  const ny = by / bl;
  const nz = bz / bl;
  let cx = ay * nz - az * ny;
  let cy = az * nx - ax * nz;
  let cz = ax * ny - ay * nx;
  const dot = Math.max(-1, Math.min(1, ax * nx + ay * ny + az * nz));
  const theta = Math.acos(dot);
  const cl = Math.hypot(cx, cy, cz);
  if (cl > 0) {
    cx /= cl;
    cy /= cl;
    cz /= cl;
  }
  const s = Math.sin(theta / 2);
  return [cx * s, cy * s, cz * s, Math.cos(theta / 2)];
}

/** `QuaternionRotationToYPW` (xUtil3D.cpp:7-22) in degrees. */
export function quaternionToYpw(q: Quat4): V3 {
  const [x, y, z, w] = q;
  const sqw = w * w;
  const sqx = x * x;
  const sqy = y * y;
  const sqz = z * z;
  const pitch = Math.asin(Math.max(-1, Math.min(1, 2 * (w * x - y * z))));
  const yaw = Math.atan2(2 * (x * z + w * y), -sqx - sqy + sqz + sqw);
  const roll = Math.atan2(2 * (x * y + w * z), -sqx + sqy - sqz + sqw);
  const deg = 180 / Math.PI;
  return [pitch * deg, yaw * deg, roll * deg];
}
