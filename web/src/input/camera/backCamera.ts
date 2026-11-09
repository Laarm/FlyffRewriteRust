// `CBackCamera` (Plan/43 §8; WP-INPUT-04): the exact port of `src:_Common/Camera.cpp:80-460` and `World3D.cpp:60-110`.
//
// The camera orbits the active mover: pivot = player + 0.9, look-at = player + 1.3, distance `4 + 2 * zoom`, pitch
// `roty + 4 * zoom` (at most 80 degrees), yaw `rotx - angle + 180` (the ground camera is "locked": the angle term is 0, so the
// yaw is absolute and CameraLock sets `rotx = -angle`). The eye follows with exponential smoothing (1/4 per 60 Hz frame on the
// ground, 1/10 flying) and is pulled in by the `camera_check_bound` collision test (terrain +0.6, water +0.3, static meshes),
// recovering 0.37 units per frame once the obstruction is gone.
//
// Everything here is advanced per STEPPED 60 Hz frame and interpolated at display rate (PL-001, PL-009): `process` is called
// once per rendered iteration with the number of frames that were stepped. Arithmetic is IEEE double where the original used
// `float` (client-only presentation, not part of the determinism suite, and invisible: the smoothing converges, no quirk
// depends on the last float bit).
//
// Matrices follow the D3D conventions of `flyff_common::math::D3dMatrix`: row vectors, left-handed,
// `rotationX(phi)` rows `(1,0,0) (0,cos,sin) (0,-sin,cos)`, `rotationY(theta)` rows `(cos,0,-sin) (0,1,0) (sin,0,cos)`,
// `transformCoord(v, M) = v * M`.

import type { CameraState, Vec3LH } from '../../render/types';

type V3 = [number, number, number];

const DEG = Math.PI / 180;
/** The original's `3.1415926f` in `atan(q) * 180 / 3.1415926` (flight auto-track). */
const PI_ORIGINAL = 3.1415926;

/** `m_fZoom` constructor value. */
export const ZOOM_DEFAULT = 6;
/** `m_fRoty` constructor value (degrees). */
export const ROTY_DEFAULT = -12;
/** Smallest zoom reachable with the wheel (`zoom < -1.5` is clamped). */
export const ZOOM_MIN_WHEEL = -1.5;
/**
 * `ZOOMLIMIT 1` (default): 11.5 = 27 m, owner #019 (+50 % distance; the original 7 = 18 m, `WndWorld.cpp:7596-7611`); the camera
 * distance is `4 + 2 * zoom`. `ZOOMLIMIT 0`: 15 = 34 m (the hidden option).
 */
export const ZOOM_MAX_LIMITED = 11.5;
export const ZOOM_MAX_UNLIMITED = 15;
/** `CAMERA_RECOVER_STEP` (`__CAM_FAST_RECOVER`). */
export const CAMERA_RECOVER_STEP = 0.37;
/** Ground / flying smoothing divisors (`CBackCamera::Process(fFactor)`). */
export const SMOOTH_GROUND = 4;
export const SMOOTH_FLYING = 10;
/** The default `quake` size (`m_fQuakeSize`). */
export const QUAKE_SIZE_DEFAULT = 0.06;
/** Per-frame step of the flight FOV divisor (turbo `-0.2`, otherwise `+0.05`). */
export const FDIV_TURBO_STEP = 0.2;
export const FDIV_RELAX_STEP = 0.05;
/** `fDiv` of the telescope levels 0..3. */
export const TELESCOPE_FDIV: readonly [number, number, number, number] = [4, 10, 25, 50];

/** The active mover as the camera sees it (the render view's record of the `ACTIVE` mover, 31 §17.7). */
export interface CameraSubject {
  readonly id: number;
  /** Logical LH position. */
  readonly pos: Vec3LH;
  /** Yaw in degrees. */
  readonly angle: number;
  /** Flight pitch in degrees (`angle_x`). */
  readonly angleX: number;
  readonly flying: boolean;
  /** `OBJSTAF_ACC`: the broom accelerates (flight FOV). */
  readonly acc: boolean;
  /** `OBJSTAF_TURBO`. */
  readonly turbo: boolean;
}

/** The result of `camera_check_bound` (`SimBridge.cameraCheckBound`). */
export interface BoundResult {
  readonly crash: boolean;
  readonly out: Vec3LH;
  readonly len: number;
}

export type CameraMode = 'normal' | 'myroom';

const rad = (d: number): number => d * DEG;

/** `transformCoord(v, rotationX(phi))`. */
function rotX(v: V3, phi: number): V3 {
  const c = Math.cos(phi);
  const s = Math.sin(phi);
  return [v[0], v[1] * c - v[2] * s, v[1] * s + v[2] * c];
}

/** `transformCoord(v, rotationY(theta))`. */
function rotY(v: V3, theta: number): V3 {
  const c = Math.cos(theta);
  const s = Math.sin(theta);
  return [v[0] * c + v[2] * s, v[1], -v[0] * s + v[2] * c];
}

function normalized(v: V3): V3 {
  const l = Math.hypot(v[0], v[1], v[2]);
  return l > 0 ? [v[0] / l, v[1] / l, v[2] / l] : [0, 0, 0];
}

const lerp = (a: number, b: number, t: number): number => a + (b - a) * t;

/** The pose of one stepped frame: the interpolation endpoints. */
interface Pose {
  eye: V3;
  lookAt: V3;
}

export interface BackCameraOptions {
  /** Cosmetic quake jitter source (`Math.random`); tests inject a fixed one. */
  readonly random?: () => number;
}

export class BackCamera {
  /** `m_fRotx`: yaw in degrees, `[0, 360]` after a drag. */
  rotx = 0;
  /** `m_fRoty`: pitch in degrees. */
  roty = ROTY_DEFAULT;
  /** `m_fZoom`. */
  zoom = ZOOM_DEFAULT;
  /** `m_bLock`: the yaw is absolute (the ground camera). */
  locked = false;
  curRotx = 0;
  curRoty = 0;
  /** `m_vOffset` (smoothed) and `m_vOffsetDest`. */
  offset: V3 = [0, 0, 0];
  offsetDest: V3 = [0, 0, 0];
  /** `m_vPos` after the collision and `m_vLookAt`. */
  eye: V3 = [0, 0, 0];
  lookAt: V3 = [0, 0, 0];
  /** The uncorrected eye of the last `smooth` (the collision input). */
  eyeRaw: V3 = [0, 0, 0];
  len1 = 0;
  len2 = 0;
  old = false;
  start = false;
  mode: CameraMode = 'normal';
  quakeTicks = 0;
  quakeSize = QUAKE_SIZE_DEFAULT;
  /** `CWorld::m_nZoomLevel` (the telescope): global, survives the world object. */
  telescope: 0 | 1 | 2 | 3 = 0;
  /** `fDiv`: `fovY = pi / fDiv`. */
  fDiv = 4;
  private prev: Pose | null = null;
  private cur: Pose | null = null;
  private jitter: V3 = [0, 0, 0];
  private readonly random: () => number;

  constructor(options: BackCameraOptions = {}) {
    this.random = options.random ?? Math.random;
  }

  // ---- lock, telescope, quake --------------------------------------------------------------------------------------

  /** `Lock()`: the yaw becomes absolute; unless the player flies with a focus target `rotx = -angle` (§8.6). */
  lock(subject: Pick<CameraSubject, 'angle' | 'flying'> | null, hasFocus: boolean): void {
    this.locked = true;
    if (subject && !(hasFocus && subject.flying)) this.rotx = -subject.angle;
  }

  /** `Unlock()`: the camera is relative to the broom heading again. */
  unlock(): void {
    this.locked = false;
    this.rotx = 0;
    this.roty = 0;
    this.mode = 'normal';
  }

  /** R double click on the ground: snap behind the player. */
  snapBehind(angle: number): void {
    this.rotx = -angle;
  }

  /**
   * ScrollLock: the telescope level 0 -> 1 -> 2 -> 3 -> 0. A non-zero level resets the zoom to 0 (the chat notice is the
   * caller's). Returns the new level.
   */
  cycleTelescope(): 0 | 1 | 2 | 3 {
    this.telescope = ((this.telescope + 1) % 4) as 0 | 1 | 2 | 3;
    if (this.telescope !== 0) this.zoom = 0;
    return this.telescope;
  }

  /** `Replace` (a world change) resets the telescope (`DPClient.cpp:2343`). */
  resetTelescope(): void {
    this.telescope = 0;
    this.fDiv = 4;
  }

  /** `CCamera::SetQuake`: only when no quake runs. */
  setQuake(ticks: number, size: number = QUAKE_SIZE_DEFAULT): void {
    if (this.quakeTicks <= 0) {
      this.quakeTicks = ticks;
      this.quakeSize = size;
    }
  }

  // ---- controls (Plan/43 §8.5) -------------------------------------------------------------------------------------

  /** `rotx` wrap of the non-`__Y_CAMERA_SLOW_8` build. */
  private wrapRotx(): void {
    if (this.rotx > 360) this.rotx -= 360;
    if (this.rotx < 0) this.rotx += 360;
  }

  private clampRoty(): void {
    const hi = 80 - this.zoom * 4;
    if (this.roty > hi) this.roty = hi;
    if (this.roty < -80) this.roty = -80;
  }

  /** A drag: `dx`, `dy` accumulated pixels times the mouse speed `s` (1.0 / 0.2 / 0.1). */
  drag(dx: number, dy: number, s: number): void {
    this.rotx += dx * s;
    this.wrapRotx();
    this.roty += dy * s;
    this.clampRoty();
  }

  /** `VK_LEFT` / `VK_RIGHT` held for one stepped frame: `rotx -= 4` / `+= 4`. */
  arrowYaw(dir: -1 | 1): void {
    this.rotx += 4 * dir;
    this.wrapRotx();
  }

  /** `VK_UP` / `VK_DOWN` held for one stepped frame: `roty += 2` / `-= 2`. */
  arrowPitch(dir: -1 | 1): void {
    this.roty += 2 * dir;
    this.clampRoty();
  }

  /**
   * `VK_PRIOR` (`towards = true`: zoom in, -0.6) / `VK_NEXT` (+0.6) held for one stepped frame. KEEP-QUIRK: the clamp to 0
   * is outside the deploy test, so a negative wheel zoom jumps to 0 even during a deploy; the step itself is suppressed
   * during a housing or guild-house deploy.
   */
  zoomKey(towards: boolean, deployActive: boolean): void {
    if (towards) {
      if (!deployActive) this.zoom -= 0.6;
      if (this.zoom < 0) this.zoom = 0;
    } else {
      if (!deployActive) this.zoom += 0.6;
      if (this.zoom > ZOOM_MAX_LIMITED) this.zoom = ZOOM_MAX_LIMITED;
    }
  }

  /**
   * The wheel: `notches > 0` is towards the user (zoom -0.5, minimum -1.5), `< 0` away (+0.5, `ZOOM_MAX_LIMITED` 11.5 with `zoomLimit`, else 15).
   * Ignored at a telescope level and while a deploy rotates furniture with the wheel.
   */
  wheel(notches: number, zoomLimit: boolean, deployActive: boolean): void {
    if (this.telescope !== 0 || deployActive) return;
    const max = zoomLimit ? ZOOM_MAX_LIMITED : ZOOM_MAX_UNLIMITED;
    const n = Math.trunc(notches);
    for (let i = 0; i < Math.abs(n); i += 1) {
      if (n > 0) {
        this.zoom -= 0.5;
        if (this.zoom < ZOOM_MIN_WHEEL) this.zoom = ZOOM_MIN_WHEEL;
      } else {
        this.zoom += 0.5;
        if (this.zoom > max) this.zoom = max;
      }
    }
  }

  // ---- the per-frame process (Plan/43 §8.3) ------------------------------------------------------------------------

  /**
   * `CBackCamera::Process` up to the offset update (no collision): `factor` is 4 on the ground and 10 flying.
   * `focusPos` is the focused mover's position (a mover only), `tracking` the flight auto-track flag.
   */
  smooth(subject: CameraSubject, focusPos: Vec3LH | null, tracking: boolean, factor: number): void {
    const vPos: V3 = [subject.pos[0], subject.pos[1] + 0.9, subject.pos[2]];
    if (this.mode === 'myroom' && this.zoom <= 0.5) this.zoom = 0.5;
    let a: number;
    let angleY: number;
    if (focusPos !== null && subject.flying && tracking) {
      const tx = vPos[0] - focusPos[0];
      const ty = vPos[1] - focusPos[1];
      const tz = vPos[2] - focusPos[2];
      let q = tx / tz; // IEEE: +-Infinity when tz == 0 and tx != 0
      if (tz === 0 && tx === 0) q = 0; // FIX: the original produced NaN here
      a = -((Math.atan(q) * 180) / PI_ORIGINAL);
      if (!(tz > 0)) a += 180;
      angleY = (Math.atan2(Math.hypot(tx, tz), ty) * 180) / Math.PI;
      const reg = Math.min(2, Math.max(-2, ty / 40));
      this.curRoty = this.roty + this.zoom * reg;
      if (this.locked) a = 0;
    } else {
      a = this.locked ? 0 : subject.angle;
      angleY = 90;
      this.curRoty = this.roty + this.zoom * 4;
    }
    this.curRotx = this.rotx; // `__Y_CAMERA_SLOW_8` is off: no yaw smoothing
    this.curRoty += (1 - angleY / 90) * 45 + subject.angleX;
    if (this.curRoty > 80) this.curRoty = 80;
    if (this.mode === 'myroom' && this.curRoty <= 10) {
      this.curRoty = 10;
      this.roty = Math.min(0, Math.max(-30, this.roty));
    }
    const yaw = this.curRotx - a + 180;
    const d: V3 = this.fDiv === 2 ? [0, 0, -2] : [0, 0, -4 - this.zoom * 2];
    const pitched = rotX(d, rad(this.curRoty));
    this.offsetDest = rotY(pitched, rad(yaw));
    for (let i = 0; i < 3; i += 1) {
      const o = this.offset[i] ?? 0;
      this.offset[i] = o + ((this.offsetDest[i] ?? 0) - o) / factor;
    }
    this.eyeRaw = [vPos[0] + this.offset[0], vPos[1] + this.offset[1], vPos[2] + this.offset[2]];
    this.lookAt = [vPos[0], vPos[1] + 0.4, vPos[2]];
  }

  /**
   * The collision of one rendered iteration (§8.4): `check` is `camera_check_bound` (called once), `noCollision` the 1:1
   * guild-combat participant (the raw distance is kept). `steps` stepped frames advance the recovery by `0.37 * steps`.
   */
  collide(
    steps: number,
    check: (eye: Vec3LH, lookAt: Vec3LH) => BoundResult,
    noCollision = false,
  ): void {
    let crash: boolean;
    let out: V3 = this.eyeRaw;
    let len: number;
    if (noCollision) {
      crash = false;
      len = Math.hypot(
        this.eyeRaw[0] - this.lookAt[0],
        this.eyeRaw[1] - this.lookAt[1],
        this.eyeRaw[2] - this.lookAt[2],
      );
    } else {
      const r = check(this.eyeRaw, this.lookAt);
      crash = r.crash;
      out = [r.out[0], r.out[1], r.out[2]];
      len = r.len;
    }
    if (crash) this.len2 = len;
    if (this.old && !crash) {
      this.len1 = len;
      this.start = true;
    }
    if (this.start) {
      const dir = normalized([
        this.eyeRaw[0] - this.lookAt[0],
        this.eyeRaw[1] - this.lookAt[1],
        this.eyeRaw[2] - this.lookAt[2],
      ]);
      this.len2 += CAMERA_RECOVER_STEP * steps;
      if (this.len2 > len) this.start = false;
      out = [
        this.lookAt[0] + dir[0] * this.len2,
        this.lookAt[1] + dir[1] * this.len2,
        this.lookAt[2] + dir[2] * this.len2,
      ];
    }
    this.old = crash;
    this.eye = out;
  }

  /** `World3D.cpp:60-110`, once per stepped frame: the FOV divisor of the flight turbo and the telescope. */
  updateFov(subject: Pick<CameraSubject, 'flying' | 'acc' | 'turbo'> | null): void {
    if (subject?.flying === true && subject.acc) {
      if (subject.turbo) {
        this.fDiv -= FDIV_TURBO_STEP;
        if (this.fDiv <= 2) this.fDiv = 2;
      } else {
        this.fDiv += FDIV_RELAX_STEP;
        if (this.fDiv >= 4) this.fDiv = 4;
      }
    } else {
      this.fDiv = TELESCOPE_FDIV[this.telescope];
    }
  }

  /** One stepped frame of the quake (`CCamera::Transform`): the jitter goes to the VIEW eye only. */
  tickQuake(): void {
    if (this.quakeTicks > 0) {
      const s = this.quakeSize;
      this.jitter = [
        s / 2 - this.random() * s,
        s / 2 - this.random() * s,
        s / 2 - this.random() * s,
      ];
      this.quakeTicks -= 1;
    } else {
      this.jitter = [0, 0, 0];
    }
  }

  // ---- interpolation and output (§8.8) -----------------------------------------------------------------------------

  /** Ends a rendered iteration that stepped: `prev = cur`, `cur = {eye, lookAt}`. */
  commit(): void {
    this.prev = this.cur;
    this.cur = {
      eye: [this.eye[0], this.eye[1], this.eye[2]],
      lookAt: [this.lookAt[0], this.lookAt[1], this.lookAt[2]],
    };
    this.prev ??= this.cur;
  }

  /** A pose has been committed (the display has something to interpolate). */
  get hasPose(): boolean {
    return this.cur !== null;
  }

  /** A teleport, a hidden-tab return, the first frame of a world: no interpolation across the jump. */
  snap(): void {
    this.cur = {
      eye: [this.eye[0], this.eye[1], this.eye[2]],
      lookAt: [this.lookAt[0], this.lookAt[1], this.lookAt[2]],
    };
    this.prev = this.cur;
  }

  /** The camera state at the interpolation factor `alpha` of the last two stepped frames (PL-009). */
  sample(alpha: number): CameraState | null {
    const cur = this.cur;
    if (!cur) return null;
    const prev = this.prev ?? cur;
    const t = Math.min(1, Math.max(0, alpha));
    const eye: Vec3LH = [
      lerp(prev.eye[0], cur.eye[0], t) + this.jitter[0],
      lerp(prev.eye[1], cur.eye[1], t) + this.jitter[1],
      lerp(prev.eye[2], cur.eye[2], t) + this.jitter[2],
    ];
    const lookAt: Vec3LH = [
      lerp(prev.lookAt[0], cur.lookAt[0], t),
      lerp(prev.lookAt[1], cur.lookAt[1], t),
      lerp(prev.lookAt[2], cur.lookAt[2], t),
    ];
    return {
      pos: eye,
      lookAt,
      up: [0, 1, 0],
      fovDiv: this.fDiv,
      telescope: this.telescope,
      yawDeg: this.curRotx,
      mode: this.mode,
    };
  }

  /** A debug copy of the state of §8.1. */
  snapshot(): Record<string, unknown> {
    return {
      rotx: this.rotx,
      roty: this.roty,
      zoom: this.zoom,
      locked: this.locked,
      curRotx: this.curRotx,
      curRoty: this.curRoty,
      offset: [...this.offset],
      eye: [...this.eye],
      lookAt: [...this.lookAt],
      len1: this.len1,
      len2: this.len2,
      old: this.old,
      start: this.start,
      mode: this.mode,
      telescope: this.telescope,
      fDiv: this.fDiv,
      quakeTicks: this.quakeTicks,
    };
  }
}
