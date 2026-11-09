// The camera driver (Plan/43 §8.2, §8.5, §8.6; WP-INPUT-04): the glue between the `BackCamera` math and the world input.
// One `process(steps, view)` per RENDERED iteration; the per-frame constants advance once per stepped 60 Hz frame
// (PL-001, PL-009), so a 144 Hz and a 60 Hz display end in the same state.
//
//   applyEventInputs()                accumulated drag, wheel notches, snap-behind, telescope, unlock requests (once)
//   repeat `steps` times:
//     CameraLock follow               rotx = -angle while W/S/A/D is held on the ground and R is up
//     smooth(4 | 10)                  offset glides to its destination
//     applyHeldKeys()                 arrows +-4/+-2 degrees, PgUp/PgDn +-0.6
//     updateFov(), tickQuake()
//   if steps >= 1: collide(steps)     ONE `camera_check_bound` call
//   commit()                          prev = cur

import type { CameraState, Vec3LH } from '../../render/types';
import type { KeyState } from '../keys';
import type { WorldMouse } from '../pointer';
import {
  SMOOTH_FLYING,
  SMOOTH_GROUND,
  type BackCamera,
  type BoundResult,
  type CameraSubject,
} from './backCamera';

/** The mouse speed option: `{0: 1.0, 1: 0.2, 2: 0.1}`, anything else 0.2 (`WndWorld.cpp:7764-7776`). */
export function mouseSpeedFactor(option: number): number {
  return option === 0 ? 1 : option === 2 ? 0.1 : 0.2;
}

/** `TID_GAME_CAMERA_ZOOM_LEVEL` is shown in this colour (`0xff00ff00`). */
export const TELESCOPE_NOTICE_COLOR = 0xff00ff00;

/** The most steps one iteration applies (the loop's backlog cap, `MAX_STEPS_PER_ITERATION`). */
export const MAX_CAMERA_STEPS = 60;

export interface CameraOptions {
  readonly cameraLock: boolean;
  readonly mouseSpeed: number;
  readonly zoomLimit: boolean;
}

export interface CameraDriverDeps {
  readonly camera: BackCamera;
  readonly mouse: WorldMouse;
  readonly keys: KeyState;
  options(): CameraOptions;
  /** The active mover of the render view of this iteration (`null` before it exists). */
  subject(view: unknown): CameraSubject | null;
  /** The focused mover's position (the flight auto-track, the lock rule), `null` without a mover focus. */
  focusPos(): Vec3LH | null;
  /** `MVRF_TRACKING` mirror. */
  tracking(): boolean;
  /** W/S/A/D after the chat-window focus clearing (the CameraLock follow reads it). */
  movementHeld(): boolean;
  /** A housing or guild-house deploy is armed: zoom keys and the wheel are suppressed. */
  deployActive(): boolean;
  /** 1:1 guild-combat participant: the camera does not collide. */
  noCollision(): boolean;
  /** `camera_check_bound` (one call per rendered iteration that stepped). */
  checkBound(eye: Vec3LH, lookAt: Vec3LH): BoundResult;
  /** The chat line of the telescope (`%dX Zoom`, colour `0xff00ff00`). */
  telescopeNotice?(level: number): void;
}

export class CameraDriver {
  /** The id of the subject of the last frame (world entry / `SetPlayer` detection). */
  private subjectId = 0;
  private wasFlying = false;
  private lastSubject: CameraSubject | null = null;
  private pendingSnap = false;
  private pendingTelescope = 0;
  private pendingUnlock = false;
  /** Frames processed since the start (the tests assert the per-step application). */
  steppedFrames = 0;

  constructor(private readonly d: CameraDriverDeps) {}

  get camera(): BackCamera {
    return this.d.camera;
  }

  /** The subject of the last processed frame. */
  get subject(): CameraSubject | null {
    return this.lastSubject;
  }

  // ---- event inputs (consumed by the next `process`) ---------------------------------------------------------------

  /** R double click on the ground. */
  requestSnapBehind(): void {
    this.pendingSnap = true;
  }

  /** ScrollLock key-up. */
  requestTelescope(): void {
    this.pendingTelescope += 1;
  }

  /** R-up / M-up while flying and not locked. */
  requestUnlock(): void {
    this.pendingUnlock = true;
  }

  /** `Replace` (a world change): the telescope is off, the next frame is a fresh `SetPlayer` (lock/unlock, no glide). */
  onWorldChanged(): void {
    this.d.camera.resetTelescope();
    this.subjectId = 0;
  }

  // ---- the iteration -----------------------------------------------------------------------------------------------

  /** Processes `steps` stepped frames against the render view `view` (a rendered iteration, after the sim step). */
  process(steps: number, view: unknown): void {
    const d = this.d;
    const cam = d.camera;
    const subject = d.subject(view);
    this.lastSubject = subject;
    if (subject === null) return;
    const hasFocus = d.focusPos() !== null;
    let fresh = false;
    if (subject.id !== this.subjectId) {
      // `SetPlayer`: flying ? Unlock : Lock (`WndManager.cpp:3767-3771`)
      this.subjectId = subject.id;
      if (subject.flying) cam.unlock();
      else cam.lock(subject, hasFocus);
      this.wasFlying = subject.flying;
      fresh = true;
    } else if (subject.flying !== this.wasFlying) {
      // take-off: Unlock; landing: Lock (`ActionMoverMsg.cpp:1087`, `ActionMoverMsg2.cpp:304`)
      if (subject.flying) cam.unlock();
      else cam.lock(subject, hasFocus);
      this.wasFlying = subject.flying;
    }
    this.applyEventInputs(subject);
    const n = Math.min(MAX_CAMERA_STEPS, Math.max(0, Math.trunc(steps)));
    const o = d.options();
    for (let i = 0; i < n; i += 1) {
      if (o.cameraLock && !d.mouse.R.down && d.movementHeld() && !subject.flying) {
        cam.rotx = -subject.angle; // `WndWorld.cpp:8218-8222`
      }
      cam.smooth(
        subject,
        d.focusPos(),
        d.tracking(),
        subject.flying ? SMOOTH_FLYING : SMOOTH_GROUND,
      );
      this.applyHeldKeys();
      cam.updateFov(subject);
      cam.tickQuake();
      this.steppedFrames += 1;
    }
    // an iteration without a stepped frame changes nothing: the display only re-interpolates; a fresh subject shows its
    // camera from its first stepped frame (no extra smoothing pass: the cadence of the display must not matter)
    if (n >= 1) {
      cam.collide(n, (eye, look) => d.checkBound(eye, look), d.noCollision());
      if (fresh || !cam.hasPose) cam.snap();
      else cam.commit();
    }
  }

  private applyEventInputs(subject: CameraSubject): void {
    const d = this.d;
    const cam = d.camera;
    const m = d.mouse;
    const o = d.options();
    if (this.pendingUnlock) {
      this.pendingUnlock = false;
      if (!cam.locked) cam.unlock();
    }
    if (m.dragDX !== 0 || m.dragDY !== 0) {
      cam.drag(m.dragDX, m.dragDY, mouseSpeedFactor(o.mouseSpeed));
      m.dragDX = 0;
      m.dragDY = 0;
    }
    if (m.wheelNotches !== 0) {
      cam.wheel(m.wheelNotches, o.zoomLimit, d.deployActive());
      m.wheelNotches = 0;
    }
    if (this.pendingSnap) {
      this.pendingSnap = false;
      cam.snapBehind(subject.angle);
    }
    // one cycle per key-up, even when several arrive within one frame
    for (; this.pendingTelescope > 0; this.pendingTelescope -= 1) {
      const level = cam.cycleTelescope();
      if (level !== 0) d.telescopeNotice?.(level);
    }
  }

  /** The held camera keys of one stepped frame (§8.5): arrows turn and tilt, PgUp/PgDn zoom. */
  private applyHeldKeys(): void {
    const k = this.d.keys;
    const cam = this.d.camera;
    if (k.isHeld('ArrowLeft')) cam.arrowYaw(-1);
    if (k.isHeld('ArrowRight')) cam.arrowYaw(1);
    if (k.isHeld('ArrowUp')) cam.arrowPitch(1);
    if (k.isHeld('ArrowDown')) cam.arrowPitch(-1);
    if (k.isHeld('PageUp')) cam.zoomKey(true, this.d.deployActive());
    if (k.isHeld('PageDown')) cam.zoomKey(false, this.d.deployActive());
  }

  /** The state to render at the interpolation factor `alpha` (`null` before the first frame). */
  sample(alpha: number): CameraState | null {
    return this.d.camera.sample(alpha);
  }
}
