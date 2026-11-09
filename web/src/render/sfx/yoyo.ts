// The flying yo-yo of a yo-yo skill (Plan/41 section 17.4, WP-REND-24; `CCtrl::CreateYoyoSkill` Ctrl.cpp:831-870, `CSfxItemYoyoAtk`
// Sfx.cpp:1104-1377, `CreateSfxYoYo` CreateObj.cpp:380-440 with `__YOYO_MODEL_FIX`). Purely visual: the damage is the server's.
//
//   path      `MakePath`: seven control points on a loop in front of the hand (local -Z), Catmull-Rom (alpha 0) with `MAX_SF_SLERP = 3`
//             -> 6 segments x 4 = 24 points, turned to the target by the quaternion `(0, 0, -1) -> dest - start` and moved to the
//             mover's x / z at the hand's height
//   flight    step 0: 0.3 per tick toward the first path point (done at squared distance <= 0.05), then wait 6 ticks;
//             step 1: one path point per tick; step 2: 0.4 per tick back to the hand (done at squared distance <= 0.1);
//             5 s at most (the six waiting ticks do not count)
//   model     the yo-yo effect (`sfx_obj[1]` of the equipped yo-yo) with part 0's mesh replaced by the weapon's own model
//             (`__YOYO_MODEL_FIX`); while it flies the real yo-yo on that hand is hidden
//
// `YoyoFlights` owns the flights; it draws through `SfxSystem.createProjectile` / `placeProjectile` (the instance stays where the
// flight puts it) and asks the world for the hands and the mover.

import { rotationQuaternion, transformCoord } from '../d3dx';
import type { SfxHandle } from '../fx';
import { rotationBetween } from './orient';
import type { SfxSystem } from './SfxSystem';

export type V3 = [number, number, number];

/** `MAX_SF_SLERP`: the subdivisions of a spline segment. */
export const YOYO_SLERP = 3;
/** `m_aSpline[30]`. */
export const YOYO_MAX_SPLINE = 30;
export const YOYO_OUT_SPEED = 0.3;
export const YOYO_OUT_REACH_SQ = 0.05;
export const YOYO_WAIT_TICKS = 6;
export const YOYO_BACK_SPEED = 0.4;
export const YOYO_BACK_REACH_SQ = 0.1;
/** `m_nFrame >= SEC1 * 5`. */
export const YOYO_MAX_TICKS = 300;

/** The y offsets of the seven control points (`aNewPos[i].y`). */
const OFFSETS_Y = [-0.2, -0.2, -0.1, 0, 0.1, 0.2, 0.2] as const;
/** The distances beyond the base length of the seven control points. */
const OFFSETS_Z = [0, 0.5, 1, 1.2, 1, 0.5, 0] as const;

/** `SplineSlerp`: the cubic Hermite spline of the original with tension 0 (`alpha = 0`). */
export function splineSlerp(v1: V3, v2: V3, v3: V3, v4: V3, t: number): V3 {
  const t2 = t * t;
  const t3 = t2 * t;
  const out: V3 = [0, 0, 0];
  for (let k = 0; k < 3; k++) {
    const a = v1[k] ?? 0;
    const b = v2[k] ?? 0;
    const c = v3[k] ?? 0;
    const d = v4[k] ?? 0;
    const m0 = 0.5 * (b - a + (c - b));
    const m1 = 0.5 * (c - b + (d - c));
    out[k] =
      (2 * t3 - 3 * t2 + 1) * b + (t3 - 2 * t2 + t) * m0 + (t3 - t2) * m1 + (-2 * t3 + 3 * t2) * c;
  }
  return out;
}

/**
 * `CSfxItemYoyoAtk::MakePath`: the 24 points the yo-yo flies through. `start` is the hand (`GetPos()` of the effect), `dest` the
 * target + 1 y, `mover` the throwing mover's position (its x / z are the path's; the height is the hand's).
 */
export function yoyoPath(
  start: readonly number[],
  dest: readonly number[],
  mover: readonly number[],
): V3[] {
  const sx = start[0] ?? 0;
  const sy = start[1] ?? 0;
  const sz = start[2] ?? 0;
  const dx = (dest[0] ?? 0) - sx;
  const dy = (dest[1] ?? 0) - sy;
  const dz = (dest[2] ?? 0) - sz;
  // KEEP: the base length is the SQUARED distance, 49 above 7 m, 12 below 3 m (and unchanged between 9 and 12), then / 5
  let length = dx * dx + dy * dy + dz * dz;
  if (length > 49) length = 49;
  if (length < 9) length = 12;
  length /= 5;
  const control: V3[] = [];
  for (let i = 0; i < 7; i++) {
    // `AngleToVectorXZ(&v, 0, len)` = (0, 0, -len)
    control.push([0, OFFSETS_Y[i] ?? 0, -(length + (OFFSETS_Z[i] ?? 0))]);
  }
  const local: V3[] = [];
  for (let i = 0; i < 6; i++) {
    const a = Math.max(i - 1, 0);
    const d = Math.min(i + 2, 6);
    for (let j = 0; j <= YOYO_SLERP; j++) {
      if (local.length >= YOYO_MAX_SPLINE) break;
      local.push(
        splineSlerp(
          control[a] ?? [0, 0, 0],
          control[i] ?? [0, 0, 0],
          control[i + 1] ?? [0, 0, 0],
          control[d] ?? [0, 0, 0],
          j / YOYO_SLERP,
        ),
      );
    }
  }
  const q = rotationBetween(0, 0, -1, dx, dy, dz);
  const rot = rotationQuaternion(q);
  const ox = mover[0] ?? 0;
  const oz = mover[2] ?? 0;
  return local.map((p) => {
    const r = transformCoord(p, rot);
    return [r[0] + ox, r[1] + sy, r[2] + oz];
  });
}

/** One flight (`CSfxItemYoyoAtk::Process`), a pure state machine over positions. */
export class YoyoFlight {
  readonly path: V3[];
  readonly pos: V3;
  step: 0 | 1 | 2 = 0;
  /** `m_nCount`: the next path point of step 1. */
  private count = 0;
  /** `m_nDelayCount`. */
  private delay = 0;
  /** `m_nFrame`. */
  frame = 0;
  done = false;

  constructor(start: readonly number[], dest: readonly number[], mover: readonly number[]) {
    this.path = yoyoPath(start, dest, mover);
    this.pos = [start[0] ?? 0, start[1] ?? 0, start[2] ?? 0];
  }

  /**
   * One 60 Hz tick. `hand` is the hand the yo-yo returns to (read each tick of step 2). Returns `true` while the flight goes on.
   */
  tick(hand: readonly number[]): boolean {
    if (this.done) return false;
    if (this.delay > 0) {
      this.delay--; // the waiting ticks return before `m_nFrame++`
      return true;
    }
    const pos = this.pos;
    if (this.step === 0) {
      const first = this.path[0] ?? pos;
      moveToward(pos, first, YOYO_OUT_SPEED);
      if (distSq(first, pos) <= YOYO_OUT_REACH_SQ) {
        this.step = 1;
        this.delay = YOYO_WAIT_TICKS;
      }
    } else if (this.step === 1) {
      const p = this.path[this.count];
      if (p) {
        pos[0] = p[0];
        pos[1] = p[1];
        pos[2] = p[2];
      }
      this.count++;
      if (this.count >= this.path.length) this.step = 2;
    } else {
      moveToward(pos, hand, YOYO_BACK_SPEED);
      if (distSq(hand, pos) <= YOYO_BACK_REACH_SQ) {
        this.done = true;
        return false;
      }
    }
    this.frame++;
    if (this.frame >= YOYO_MAX_TICKS) {
      this.done = true;
      return false;
    }
    return true;
  }
}

/** `vPos += normalize(to - vPos) * speed` (a zero vector stays zero). */
function moveToward(pos: V3, to: readonly number[], speed: number): void {
  const dx = (to[0] ?? 0) - pos[0];
  const dy = (to[1] ?? 0) - pos[1];
  const dz = (to[2] ?? 0) - pos[2];
  const len = Math.hypot(dx, dy, dz);
  if (len === 0) return;
  const k = speed / len;
  pos[0] += dx * k;
  pos[1] += dy * k;
  pos[2] += dz * k;
}

function distSq(a: readonly number[], b: readonly number[]): number {
  const x = (a[0] ?? 0) - (b[0] ?? 0);
  const y = (a[1] ?? 0) - (b[1] ?? 0);
  const z = (a[2] ?? 0) - (b[2] ?? 0);
  return x * x + y * y + z * z;
}

// ---- the runtime ------------------------------------------------------------------------------------------------------------------

/** What the flights ask of the world. */
export interface YoyoWorld {
  /** The logical position (LH) of a mover; `false` when unknown. */
  posOf(id: number, out: V3): boolean;
  /** The right (0) / left (1) hand of a mover in the world (LH); `false` without an attachment record. */
  handPos?(id: number, hand: 0 | 1, out: V3): boolean;
}

export interface YoyoHooks {
  /** The `.o3d` of the weapon in `hand` of `src` (`__YOYO_MODEL_FIX`); `null` keeps the effect's own mesh. */
  weaponModel?(src: number, hand: 0 | 1): string | null;
  /** The real yo-yo of `hand` is hidden while its twin flies (`SetEffect(type, XE_HIDE)`). */
  hideWeapon?(src: number, hand: 0 | 1, hidden: boolean): void;
}

export interface YoyoStats {
  launched: number;
  finished: number;
  /** Flights refused (no hand position, no target). */
  refused: number;
}

interface Live {
  readonly flight: YoyoFlight;
  readonly src: number;
  readonly hand: 0 | 1;
  readonly handle: SfxHandle;
}

export interface YoyoDeps {
  readonly sfx: Pick<SfxSystem, 'createProjectile' | 'placeProjectile' | 'remove'>;
  readonly world: () => YoyoWorld;
  readonly hooks?: () => YoyoHooks;
}

export class YoyoFlights {
  readonly stats: YoyoStats = { launched: 0, finished: 0, refused: 0 };
  private readonly live: Live[] = [];
  private readonly tmp: V3 = [0, 0, 0];

  constructor(private readonly deps: YoyoDeps) {}

  /** Flights in the air. */
  get count(): number {
    return this.live.length;
  }

  /** The positions of the flights (probe, tests). */
  positions(): readonly V3[] {
    return this.live.map((l) => [l.flight.pos[0], l.flight.pos[1], l.flight.pos[2]]);
  }

  /**
   * `CreateYoyoSkill`: one flight from each of `hands` of `src` at `target` with the yo-yo effect `xi`. The path starts at the hand
   * and ends at the hand; the target is aimed at one metre above its feet.
   */
  launch(src: number, target: number, xi: number, hands: readonly (0 | 1)[]): void {
    const world = this.deps.world();
    const mover: V3 = [0, 0, 0];
    const dest: V3 = [0, 0, 0];
    if (!world.posOf(src, mover) || !world.posOf(target, dest)) {
      this.stats.refused += hands.length;
      return;
    }
    dest[1] += 1;
    for (const hand of hands) {
      const start: V3 = [0, 0, 0];
      if (world.handPos?.(src, hand, start) !== true) {
        // without an attachment record the hand is where the weapon would be: beside the body at hip height
        start[0] = mover[0];
        start[1] = mover[1] + 1;
        start[2] = mover[2];
      }
      const flight = new YoyoFlight(start, dest, mover);
      const model = this.deps.hooks?.().weaponModel?.(src, hand) ?? null;
      const handle = this.deps.sfx.createProjectile(xi, {
        pos: flight.pos,
        rot3: [0, 0, 0],
        yaw: 0,
        cls: 'skill',
        ...(model !== null ? { meshPath: model } : {}),
      });
      this.deps.hooks?.().hideWeapon?.(src, hand, true);
      this.live.push({ flight, src, hand, handle });
      this.stats.launched++;
    }
  }

  /** `steps` 60 Hz ticks. */
  tick(steps: number): void {
    const world = this.deps.world();
    for (let s = 0; s < steps; s++) {
      for (const l of this.live) {
        if (l.flight.done) continue;
        // the hand moves with the thrower: step 2 aims at where it is now
        const hand = this.tmp;
        if (world.handPos?.(l.src, l.hand, hand) !== true) {
          if (!world.posOf(l.src, hand)) {
            l.flight.done = true; // `IsValidObj(pObjSrc)` failed: the effect is deleted
            continue;
          }
          hand[1] += 1;
        }
        l.flight.tick(hand);
      }
    }
    let w = 0;
    for (const l of this.live) {
      if (l.flight.done) {
        this.deps.sfx.remove(l.handle);
        this.deps.hooks?.().hideWeapon?.(l.src, l.hand, false);
        this.stats.finished++;
      } else {
        this.live[w++] = l;
      }
    }
    this.live.length = w;
  }

  /** Puts the models where the flights are (once per frame). */
  update(): void {
    for (const l of this.live) this.deps.sfx.placeProjectile(l.handle, l.flight.pos, [0, 0, 0], 0);
  }

  clear(): void {
    for (const l of this.live) {
      this.deps.sfx.remove(l.handle);
      this.deps.hooks?.().hideWeapon?.(l.src, l.hand, false);
    }
    this.live.length = 0;
  }
}
