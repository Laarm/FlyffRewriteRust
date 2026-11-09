// `CPartsLaser` (Plan/41 section 17.1 "Bosses", `ModelGlobal.cpp:924-1072`, WP-REND-27): the ClockWorks laser beam. FIX of the original's single global `g_Laser`: one beam PER BOSS, its
// pulse phase advanced once per 60 Hz tick (the original advanced `m_fRadian` once per RENDERED mover, so it pulsed faster with the number of movers in view), drawn ONCE per frame per
// armed boss (the original drew the one global beam from `CMover::RenderPartsEffect` of EVERY rendered mover, additively, so it brightened with their number).
//
//   width   `0.5 * (0.875 + sin(r) / 8)` with `r += 0.2` per tick (`SetPos`: 0.75 .. 1.0 of the base width)
//   quad    `v1 -+ w * vR`, `v2 -+ w * vR` as a triangle strip; UV (0,0) (1,0) at `v1`, (0,1) (1,1) at `v2`
//           `vB = v1 - v2`, `vE = eye - v1`, `vP = vB x vE`, `vFxP = normalize(vF x vP)`, `vR = vF x vFxP` (`vF` = the camera's forward axis)
//   texture `Model/Texture/etc_Laser01.tga` (24 bit: no alpha), ONE / ONE (here the additive run, which is SRCALPHA / ONE: the same with alpha 1), depth write off, fog off, cull none
//
// LEFT-HANDED game space.

import type { DrawList } from './DrawList';

/** The base width of the beam (`g_Laser.SetPos(v1, v2, 0.5f)`). */
export const LASER_BASE_WIDTH = 0.5;
/** `m_fRadian += 0.2f` per tick. */
export const LASER_RADIAN_STEP = 0.2;
/** The texture of the beam (`RestoreDeviceObjects`). */
export const LASER_TEXTURE = 'model/texture/etc_laser01.tga';

type V3 = readonly [number, number, number];

const f32 = Math.fround;

const cross = (a: V3, b: V3): [number, number, number] => [
  a[1] * b[2] - a[2] * b[1],
  a[2] * b[0] - a[0] * b[2],
  a[0] * b[1] - a[1] * b[0],
];

/** The four strip vertices `x y z u v` of a beam (LH): `[v1 - w R (0,0), v1 + w R (1,0), v2 - w R (0,1), v2 + w R (1,1)]`. */
export function laserCorners(
  v1: V3,
  v2: V3,
  width: number,
  eye: V3,
  forward: V3,
  out: Float64Array = new Float64Array(20),
): Float64Array {
  const vB: V3 = [v1[0] - v2[0], v1[1] - v2[1], v1[2] - v2[2]];
  const vE: V3 = [eye[0] - v1[0], eye[1] - v1[1], eye[2] - v1[2]];
  const vP = cross(vB, vE);
  const fxp = cross(forward, vP);
  const len = Math.hypot(fxp[0], fxp[1], fxp[2]);
  const vFxP: V3 = len > 0 ? [fxp[0] / len, fxp[1] / len, fxp[2] / len] : [0, 0, 0];
  const vR = cross(forward, vFxP);
  const put = (i: number, p: V3, sign: number, u: number, v: number): void => {
    out[i * 5] = p[0] + sign * width * vR[0];
    out[i * 5 + 1] = p[1] + sign * width * vR[1];
    out[i * 5 + 2] = p[2] + sign * width * vR[2];
    out[i * 5 + 3] = u;
    out[i * 5 + 4] = v;
  };
  put(0, v1, -1, 0, 0);
  put(1, v1, 1, 1, 0);
  put(2, v2, -1, 0, 1);
  put(3, v2, 1, 1, 1);
  return out;
}

/** One boss's beam. */
export class LaserBeam {
  /** `m_fRadian`. */
  radian = 0;
  /** Segment end points (LH), valid while `active`. */
  readonly v1: [number, number, number] = [0, 0, 0];
  readonly v2: [number, number, number] = [0, 0, 0];
  /** `m_fSize`: the half width of the last `set`. */
  size = 0;
  active = false;
  /** Ticks the beam has been on (the sparks fire on every second one). */
  ticks = 0;
  private readonly corners = new Float64Array(20);

  /** The width `SetPos` gives for the current phase. */
  width(base = LASER_BASE_WIDTH): number {
    return f32(base * f32(0.875 + f32(Math.sin(this.radian) / 8)));
  }

  /** `SetPos(v1, v2, 0.5f)` of one tick: the segment and the pulsed width; then the phase advances (FIX: once per tick). */
  tick(v1: V3, v2: V3): void {
    this.v1[0] = v1[0];
    this.v1[1] = v1[1];
    this.v1[2] = v1[2];
    this.v2[0] = v2[0];
    this.v2[1] = v2[1];
    this.v2[2] = v2[2];
    this.size = this.width();
    this.active = true;
    this.ticks++;
    this.radian = f32(this.radian + LASER_RADIAN_STEP);
  }

  off(): void {
    this.active = false;
    this.ticks = 0;
  }

  /** One draw of the beam (`tex < 0` draws nothing). Returns the strips written (0 or 1). */
  emit(list: DrawList, tex: number, eye: V3, forward: V3): number {
    if (!this.active || tex < 0) return 0;
    laserCorners(this.v1, this.v2, this.size, eye, forward, this.corners);
    list.strip(tex, true, this.corners, 4, 255);
    return 1;
  }
}
