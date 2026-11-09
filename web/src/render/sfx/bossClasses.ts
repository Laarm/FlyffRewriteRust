// The six boss / NPC effect classes of the `CreateSfx` factory (Plan/41 section 17.1 "Bosses" and 17.2, WP-REND-27; `Sfx.cpp:716-830, 1715-1810, 2322-2410, 2993-3135`): each is a
// `ClassSpec` plus an `SfxBehavior`, like the skill classes of `skillClasses.ts`.
//
//   CSfxClockWorksCharge   the charge on the forehead: each tick at event 2 of the src, yaw `-srcAngle`, the model restarts when finished; ends with the src, when the boss leaves the charging
//                          attack state (FIX: the original looped forever without a hit) - `BossFx` also removes it when the boss's hit-continue flag is set
//   CSfxMushmootCharge     the same at the event point `bh.count` (0 for ATK3, 1..4 for ATK4)
//   CSfxClockWorksCannon   each tick at event 1; the 3-axis rotation from `world * bone 9` IN THAT ORDER (`D3DXMatrixMultiply(&m, world, bone)`: KEEP), converted with the NON-normalising
//                          `D3DXQuaternionRotationMatrix` (the matrix keeps the model scale), yaw - 90; plays once; ends with the src
//   CSfxAtkStraight        the cannon ball: `pos += delta` per tick (`delta = AngleToVector(angXZ, angH, speed)`), the model loops, `XI_NPCSP1RANBOOM` at the FIXED destination when it comes
//                          within `0.8` (KEEP: it may overshoot), ends after 180 ticks without exploding
//   CSfxMagicMiAtk1        `XI_SKILL_MUSHMOOT_02` (no V15 data creates it): homing at 0.4 per tick, `XI_HIT_WAND01` at the hit
//   CSfxNpcDirSteam        `XI_NPC_DIR_STEAM`: at event 0 of the src, yaw `180 - srcAngle`, scale 2 for a boss and 1 for the other ranks, aimed at the destination once (x and z capped at +45)
//
// Positions are LEFT-HANDED game space. A missing event record, bone or position falls back to the boss position (`BossFx` counts and warns once per model).

import { RANK_BOSS, RANK_CAPTAIN, RANK_LOW, RANK_NORMAL } from '../../gen/defs/attribute';
import { XI_HIT_WAND01, XI_NPCSP1RANBOOM } from '../../gen/defs/obj';
import { multiply, quaternionRotationMatrix } from '../d3dx';
import { OBJSTA_ATK_ALL } from '../../game/fx/bossIds';
import type { ClassSpec, SfxBehavior, BehaviorHost } from './classSpec';
import { quaternionToYpw, rotationBetween } from './orient';
import type { SfxInstance, V3 } from './SfxSystem';

/** The ticks a source the render view has not shown yet may stay unknown before the effect gives up. */
const UNSEEN_GRACE_TICKS = 30;
/** `IsRangeObj( posDest, 0 )`: `(1 * 0.8 + 0.8 * radius + 0)`; the radius of an effect model is 0 here. */
export const STRAIGHT_HIT_RADIUS = 0.8;
/** `m_nFrame > 60 * 3`. */
export const STRAIGHT_MAX_TICKS = 180;
/** The homing speed of `CSfxMagicMiAtk1` and its life (`SEC1 * 10`). */
export const MAGIC_MI_SPEED = 0.4;
export const MAGIC_MI_MAX_TICKS = 600;

const tmp: V3 = [0, 0, 0];
const tmp2: V3 = [0, 0, 0];
const tmp3: V3 = [0, 0, 0];

// ---- the maths of the originals -----------------------------------------------------------------------------------------

/** `AngleToVector` (`xUtil3D.h:25-35`): the step of speed `speed` at the XZ angle and height angle (degrees). */
export function angleToVector(angXZ: number, angH: number, speed: number): V3 {
  const xz = (angXZ * Math.PI) / 180;
  const h = (angH * Math.PI) / 180;
  const dist = Math.cos(-h) * speed;
  return [Math.sin(xz) * dist, -Math.sin(-h) * speed, -Math.cos(xz) * dist];
}

/** `AngleToVectorXZ`. */
export function angleToVectorXZ(angXZ: number, speed: number): V3 {
  const xz = (angXZ * Math.PI) / 180;
  return [Math.sin(xz) * speed, 0, -Math.cos(xz) * speed];
}

/** `xGetDegree` (`xUtil3D.h:46-58`): the XZ angle and the height angle (degrees) of the vector `dist`. */
export function xGetDegree(dist: readonly number[]): { angXZ: number; angH: number } {
  const dx = dist[0] ?? 0;
  const dy = dist[1] ?? 0;
  const dz = dist[2] ?? 0;
  const angXZ = (Math.atan2(dx, -dz) * 180) / Math.PI;
  const lenXZ = Math.hypot(dx, dz);
  const angH = (Math.atan2(lenXZ, dy) * 180) / Math.PI - 90;
  return { angXZ, angH };
}

/** The 3-axis rotation (degrees `x y z`) that turns `(0, 0, 1)` to the direction `d`: the quaternion path of every directional effect. */
export function rotationToward(d: readonly number[]): V3 {
  return quaternionToYpw(rotationBetween(0, 0, 1, d[0] ?? 0, d[1] ?? 0, d[2] ?? 0));
}

/** `ShootSfx(angXZ, angH, speed, explosion)` of a `CSfxAtkStraight`: the step, the model rotation and the explosion. */
export function shootStraight(
  inst: SfxInstance,
  angXZ: number,
  angH: number,
  speed: number,
  explosion: number,
): void {
  const d = angleToVector(angXZ, angH, speed);
  inst.delta[0] = d[0];
  inst.delta[1] = d[1];
  inst.delta[2] = d[2];
  inst.rot3 = rotationToward(d);
  inst.explosion = explosion;
  inst.shot = true;
}

// ---- helpers --------------------------------------------------------------------------------------------------------------

function setPos(inst: SfxInstance, p: ArrayLike<number>): void {
  inst.pos[0] = p[0] ?? 0;
  inst.pos[1] = p[1] ?? 0;
  inst.pos[2] = p[2] ?? 0;
}

/** `IsValidObj(src)` with the grace for a source the view has not shown yet. */
function srcAlive(inst: SfxInstance, host: BehaviorHost): boolean {
  if (inst.idSrc === 0) return false;
  if (host.world.posOf(inst.idSrc, tmp)) {
    inst.bh.seen = 1;
    return true;
  }
  return inst.bh.seen === 0 && inst.frame < UNSEEN_GRACE_TICKS;
}

/** The point `index` of the src (its event record), else the src position (the pose fallback). Returns `false` without the src. */
function eventOrBody(inst: SfxInstance, host: BehaviorHost, index: number, out: V3): boolean {
  if (host.world.eventPos?.(inst.idSrc, index, out) === true) return true;
  return host.world.posOf(inst.idSrc, out);
}

/** `CreateSfx` of a child from a class: the sound hook first (T41), then the effect. */
function child(host: BehaviorHost, xi: number, pos: V3): void {
  host.hooks.sfxCreated?.(xi, pos);
  host.create(xi, { pos });
}

// ---- the charge classes -----------------------------------------------------------------------------------------------------

/**
 * `CSfxClockWorksCharge` / `CSfxMushmootCharge::Process`: the effect rides the event point (`eventIndex`) of the boss, turned `-srcAngle`; the model plays in a loop. The boss's
 * attack state when `BossFx` made the effect is `inst.bh.timer` (an `OBJSTA_ATK*` word, 0 = unknown): the effect ends when the boss leaves it.
 */
function chargeBehavior(eventIndex: (inst: SfxInstance) => number): SfxBehavior {
  const anchor = (inst: SfxInstance, host: BehaviorHost): boolean => {
    if (!eventOrBody(inst, host, eventIndex(inst), tmp2)) return false;
    setPos(inst, tmp2);
    const a = host.world.angleOf(inst.idSrc);
    if (!Number.isNaN(a)) inst.angle = -a;
    return true;
  };
  return {
    ownLifetime: true,
    tick(inst, host) {
      if (!srcAlive(inst, host)) return 'end';
      const want = inst.bh.timer;
      if (want !== 0) {
        const st = host.world.stateOf?.(inst.idSrc);
        if (st !== undefined && (st & OBJSTA_ATK_ALL) !== want) return 'end';
      }
      anchor(inst, host);
      return undefined;
    },
    place(inst, host) {
      anchor(inst, host);
      return true;
    },
    afterModel: (_inst, _host, finished) => (finished ? 'restart' : undefined),
  };
}

// ---- the cannon ---------------------------------------------------------------------------------------------------------------

/** The `world * bone 9` rotation of the cannon (degrees `x y z`), `null` without a bone record. */
function cannonRot3(inst: SfxInstance, host: BehaviorHost): V3 | null {
  const world = new Array<number>(16);
  const bone = new Array<number>(16);
  if (host.world.bonePose?.(inst.idSrc, CANNON_BONE, world, bone) !== true) return null;
  // `D3DXMatrixMultiply( &mLArm, world, &mLArm )`: world first, then the bone (KEEP: the reverse of the usual bone-then-world product)
  const ypw = quaternionToYpw(quaternionRotationMatrix(multiply(world, bone)));
  return [ypw[0], ypw[1] - 90, ypw[2]];
}
/** `GetMatrixBone( 9 )`: the left fore arm of the ClockWorks. */
export const CANNON_BONE = 9;

const cannon: SfxBehavior = {
  tick(inst, host) {
    if (!srcAlive(inst, host)) return 'end';
    place(inst, host);
    return undefined;
  },
  place(inst, host) {
    place(inst, host);
    return true;
  },
};
function place(inst: SfxInstance, host: BehaviorHost): void {
  if (eventOrBody(inst, host, 1, tmp2)) setPos(inst, tmp2);
  const rot = cannonRot3(inst, host);
  if (rot) {
    inst.rot3 = rot;
  } else {
    const a = host.world.angleOf(inst.idSrc);
    inst.rot3 = [0, Number.isNaN(a) ? inst.angle : -a - 90, 0];
  }
}

// ---- the cannon ball ----------------------------------------------------------------------------------------------------------

const straight: SfxBehavior = {
  ownLifetime: true,
  tick(inst, host) {
    if (!inst.shot) return undefined;
    inst.pos[0] += inst.delta[0];
    inst.pos[1] += inst.delta[1];
    inst.pos[2] += inst.delta[2];
    const dx = inst.posDest[0] - inst.pos[0];
    const dy = inst.posDest[1] - inst.pos[1];
    const dz = inst.posDest[2] - inst.pos[2];
    if (dx * dx + dy * dy + dz * dz < STRAIGHT_HIT_RADIUS * STRAIGHT_HIT_RADIUS) {
      child(host, inst.explosion > 0 ? inst.explosion : XI_NPCSP1RANBOOM, [
        inst.posDest[0],
        inst.posDest[1],
        inst.posDest[2],
      ]);
      return 'end';
    }
    // `m_nFrame++; if (m_nFrame > 180)`: `inst.frame` counts the steps before this one
    return inst.frame >= STRAIGHT_MAX_TICKS ? 'end' : undefined;
  },
  place: () => true,
  afterModel: (_inst, _host, finished) => (finished ? 'restart' : undefined),
};

// ---- the Mushmoot's homing ball ---------------------------------------------------------------------------------------------

const magicMi: SfxBehavior = {
  ownLifetime: true,
  tick(inst, host) {
    if (inst.idDest === 0 || !host.world.posOf(inst.idDest, tmp)) return 'end';
    // the aim point is the middle of the target; a target that died ends the effect (`IsDie`)
    const centre = (host.world.headOf?.(inst.idDest) ?? 1.4) / 2;
    const tx = tmp[0];
    const ty = tmp[1] + centre;
    const tz = tmp[2];
    let dx = tx - inst.pos[0];
    let dy = ty - inst.pos[1];
    let dz = tz - inst.pos[2];
    const len = Math.hypot(dx, dy, dz);
    if (len > 0) {
      dx /= len;
      dy /= len;
      dz /= len;
    }
    inst.pos[0] += dx * MAGIC_MI_SPEED;
    inst.pos[1] += dy * MAGIC_MI_SPEED;
    inst.pos[2] += dz * MAGIC_MI_SPEED;
    inst.rot3 = rotationToward([tx - inst.pos[0], ty - inst.pos[1], tz - inst.pos[2]]);
    // the hit radius: a third of the average XZ size of the target (here its radius stands in for the box)
    const r = ((host.world.radiusOf?.(inst.idDest) ?? 0.3) * 2) / 3;
    const hx = inst.pos[0] - tx;
    const hy = inst.pos[1] - ty;
    const hz = inst.pos[2] - tz;
    if (hx * hx + hy * hy + hz * hz < r * r) {
      child(host, XI_HIT_WAND01, [inst.pos[0], inst.pos[1] + 1, inst.pos[2]]);
      return 'end';
    }
    return inst.frame + 1 >= MAGIC_MI_MAX_TICKS ? 'end' : undefined;
  },
  place: () => true,
  afterModel: (_inst, _host, finished) => (finished ? 'restart' : undefined),
};

// ---- the steamwalker's jet ----------------------------------------------------------------------------------------------------

const RANKS_FOLLOWED: ReadonlySet<number> = new Set([
  RANK_LOW,
  RANK_NORMAL,
  RANK_CAPTAIN,
  RANK_BOSS,
]);

const steam: SfxBehavior = {
  tick(inst, host) {
    // the destination (the victim) must exist; the src may be unknown for a while
    if (inst.idDest === 0 || !host.world.posOf(inst.idDest, tmp3)) return 'end';
    if (!srcAlive(inst, host)) return 'end';
    const rank = host.world.rankOf?.(inst.idSrc);
    if (rank === undefined || RANKS_FOLLOWED.has(rank)) {
      if (eventOrBody(inst, host, 0, tmp2)) setPos(inst, tmp2);
      const a = host.world.angleOf(inst.idSrc);
      const yaw = Number.isNaN(a) ? 180 : 180 - a;
      const s = rank === RANK_BOSS ? 2 : 1;
      inst.scale[0] = s;
      inst.scale[1] = s;
      inst.scale[2] = s;
      if (!inst.bh.flag) {
        // `m_fInit`: the 3-axis rotation toward the destination; x and z are capped at +45 (no lower bound: KEEP), the yaw is the rule above
        inst.bh.flag = true;
        const r = rotationToward([
          tmp3[0] - inst.pos[0],
          tmp3[1] - inst.pos[1],
          tmp3[2] - inst.pos[2],
        ]);
        inst.rot3 = [Math.min(r[0], 45), r[1], Math.min(r[2], 45)];
      }
      if (inst.rot3) inst.rot3[1] = yaw;
    }
    return undefined;
  },
  place: () => true,
};

// ---- the table ----------------------------------------------------------------------------------------------------------------

const BOSS_BASE: ClassSpec = {
  follow: 'none',
  lift: 0,
  yaw: true,
  render2: false,
  once: false,
  singleton: false,
  removable: false,
};

const SPEC_BY_CLASS = new Map<string, ClassSpec>([
  ['CSfxClockWorksCharge', { ...BOSS_BASE, behavior: chargeBehavior(() => 2) }],
  ['CSfxMushmootCharge', { ...BOSS_BASE, behavior: chargeBehavior((inst) => inst.bh.count) }],
  [
    'CSfxClockWorksCannon',
    { ...BOSS_BASE, yaw: false, render2: true, once: true, behavior: cannon },
  ],
  ['CSfxAtkStraight', { ...BOSS_BASE, yaw: false, render2: true, behavior: straight }],
  ['CSfxMagicMiAtk1', { ...BOSS_BASE, yaw: false, render2: true, behavior: magicMi }],
  ['CSfxNpcDirSteam', { ...BOSS_BASE, yaw: false, render2: true, once: true, behavior: steam }],
]);

/** The names of the six classes. */
export const BOSS_CLASS_NAMES: readonly string[] = [...SPEC_BY_CLASS.keys()];

/** The spec of a boss class by its C++ name (`skillSpecOf` asks it for the rows whose impl is `'boss'`). */
export function bossSpecOf(cls: string): ClassSpec | undefined {
  return SPEC_BY_CLASS.get(cls);
}
