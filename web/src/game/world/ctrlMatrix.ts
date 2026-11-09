// The world matrix of a ctrl as the sim wants it in `CTRL_ADD` (Plan/31 section 10.10, 17.4; WP-SIM-24): the server registers its
// collider with `flyff_sim::collision::dynamic::ctrl_world_mat43(pos, angle_x10, scale_x100)` - `Scale(s) * RotYawPitchRoll(-yaw) *
// Translate(pos)` of `CObj::UpdateMatrix` with the QUANTISED values `SpawnCtrl` carries (`(i16)(angle * 10)`, `(u16)(scale * 100)`).
// This is the same arithmetic in `f32` (every operation rounded with `Math.fround`, the sine and cosine of the f32 argument), so the
// client's collider is the server's collider.

const f = Math.fround;
const DEG2RAD = f(Math.PI / 180);

/** The LH `Mat43` (12 floats, the first three columns of the four rows of the D3D matrix) of a ctrl. */
export function ctrlWorldMat43(
  pos: readonly [number, number, number],
  angleX10: number,
  scaleX100: number,
): number[] {
  const yaw = f(f(angleX10) / 10);
  const scale = f(f(scaleX100) / 100);
  const rad = f(f(-yaw) * DEG2RAD);
  const sy = f(Math.sin(rad));
  const cy = f(Math.cos(rad));
  // roll = pitch = 0: sin(-0) = -0, cos(0) = 1
  const sp = -0;
  const cp = 1;
  return [
    f(scale * cy),
    0,
    f(scale * -sy),
    f(scale * f(sp * sy)),
    f(scale * cp),
    f(scale * f(sp * cy)),
    f(scale * f(cp * sy)),
    f(scale * -sp),
    f(scale * f(cp * cy)),
    f(pos[0]),
    f(pos[1]),
    f(pos[2]),
  ];
}

/** `CTRL_ADD.aabb` of "the model's own box": the sim then uses the box of the `FLYK` model (the server's rule). */
export const ZERO_AABB: readonly number[] = [0, 0, 0, 0, 0, 0];
