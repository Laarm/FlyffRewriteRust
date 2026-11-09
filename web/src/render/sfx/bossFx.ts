// `BossFx` (Plan/41 section 17.1 "Bosses", 17.3; WP-REND-27): the client halves of the boss melee handlers of `MoverActEvent.cpp` (`OnAttackMelee_ClockWorks / BigMuscle / Krrr / Bear /
// Meteonyker`), the charge effects of `ActionMoverState.cpp:852-912` and the continue logic of `OnAttackMeleeContinue` (laser, sparks, fire breath) - with PER-BOSS state where the original
// had statics and singletons (`v3OldPos`, `vLocal`, `g_Laser`, `g_FireDragon`: FIX).
//
//   meleeHit(attacker, target, state, hitCount)   the `AttrHit{Melee}` sim event of a boss swing (`hitCount` = `m_nMotionHitCount` BEFORE the handler's increment): the effects of the
//                                                 handler; sets the boss's hit-continue flag (`MVRF_HITCONT`)
//   charge(id, state)                             `BossChargeFx`: the charge effects (ClockWorks ATK3 also arms the laser)
//   tick(steps)                                   60 Hz: the continue logic (laser, sparks, breath) and the end rules; "the continue runs before the hit": a tick that carries a
//                                                 hit produces no laser and no breath
//   draw(list, frame, textureId)                  once per frame: the laser strip of every armed ClockWorks and the quads of every breath pool
//
// A missing event point, bone or position falls back to the boss position (one `warn` per model, `stats.poseMissing`). Every effect made here calls `hooks.sfxCreated` first (T41: the
// audio wrapper only sees `RenderFx.sfx`). Quakes go through `hooks.quake` (the host gates them by `features.no_quake`).

import {
  bossKindOf,
  OBJSTA_ATK1,
  OBJSTA_ATK2,
  OBJSTA_ATK3,
  OBJSTA_ATK4,
  OBJSTA_ATK_ALL,
  type BossKind,
} from '../../game/fx/bossIds';
import {
  XI_NAT_FIRE01_ADV,
  XI_NPCMETEONYKER,
  XI_NPCSP1DIRAMP,
  XI_NPCSP1DIRCANNON,
  XI_NPCSP1DIRCIRCLE,
  XI_NPCSP1DIRFIRESP,
  XI_NPCSP1RANBALL,
  XI_NPCSP1RANBOOM,
  XI_NPCSP1RANSPARK,
  XI_SKILL_CIRCLE_DUST,
  XI_SKILL_ELE_MULTI_METEOSHOWER02,
  XI_SKILL_MAG_EARTH_ROCKCRASH01,
  XI_SKILL_MUSHMOOT_CHARGE,
} from '../../gen/defs/obj';
import { SI_GEN_BURN } from '../../gen/defs/skill';
import type { SfxHandle, SfxOptions } from '../fx';
import { angleToVectorXZ, shootStraight, xGetDegree } from './bossClasses';
import type { DrawList } from './DrawList';
import type { SkillFxHooks } from './classSpec';
import { FIRE_DRAGON_TEXTURE, FireDragonPool } from './fireDragon';
import { LASER_TEXTURE, LaserBeam } from './laser';
import { Rand15, RAND_MAX } from './rng';
import type { SfxFrame, SfxInstance, V3, WorldQuery } from './SfxSystem';

/** The bone and offset of the laser end (`GetMatrixBone( 6 )`, `(0, 0, 40)`). */
export const LASER_BONE = 6;
export const LASER_REACH = 40;
/** The BigMuscle's fists (`GetMatrixBone( 12 / 16 )`) and the ground offset of their dust (`v.y -= 5`). */
export const BIGMUSCLE_BONES: readonly [number, number] = [12, 16];
export const BIGMUSCLE_DUST_DROP = 5;
/** The Krrr's meteor point: 5 m in front of the boss. */
export const KRRR_METEOR_DISTANCE = 5;
/** `xRandomF( 12.0f ) - 6.0f`. */
export const KRRR_METEOR_SPREAD = 12;
/** `CSfx` scales of the effects (`SetScale`). */
const SCALE_BALL = 4;
const SCALE_CHARGE = 2;
/** `ShootSfx( ..., 2.0f, XI_NPCSP1RANBOOM )`. */
export const BALL_SPEED = 2;
/** Breath: `vTemp = vDir * (xRandomF( 0.2f ) + 0.1f) * 0.8; vTemp.y -= xRandomF( 0.05f ) + 0.01f`, two per tick, scale `0.3 x GetScale().x`. */
export const BREATH_PER_TICK = 2;

/** What the boss effects need (the renderer supplies it; the tests fake it). */
export interface BossFxDeps {
  /** `SfxSystem.create` (no sound: this class calls `hooks.sfxCreated` itself). */
  create(xi: number, o: SfxOptions): SfxHandle;
  remove(handle: SfxHandle): void;
  instanceOf(handle: SfxHandle): SfxInstance | undefined;
  /** The frame world (event points, bones, state, heights); read at call time. */
  world(): WorldQuery;
  /** The mover index (`MI_*`) of an object, `undefined` for an unknown one. */
  propOf(id: number): number | undefined;
  hooks(): SkillFxHooks;
  /** `features.better_clockwork_alpha`. */
  better(): boolean;
  /** `xRandomF(1)`: the CRT `rand() / RAND_MAX` (cosmetic). */
  uniform?(): number;
  warn?(message: string): void;
}

/** `render.boss()`. */
export interface BossProbe {
  /** Charge effects made. */
  charges: number;
  /** Ticks the laser was on (all bosses). */
  laserTicks: number;
  /** Sparks made at the laser's terrain hits. */
  sparks: number;
  /** Fire-breath particles alive now / made in total. */
  breathLive: number;
  breathEmitted: number;
  /** Event points, bones or bodies the render view did not have. */
  poseMissing: number;
  /** Melee hits handled, per boss kind. */
  hits: number;
  /** The `XI_` ids of the last effects made, oldest first. */
  effects: number[];
  /** Bosses with state. */
  bosses: number;
  /** The segment of the last laser tick (LH world), `null` before the first one: the QA camera aims at it. */
  laser: { from: [number, number, number]; to: [number, number, number] } | null;
}

interface Boss {
  readonly id: number;
  readonly kind: BossKind;
  /** `MVRF_HITCONT`. */
  hitCont: boolean;
  /** The first tick at which the continue logic may run (the tick that carries the hit is skipped). */
  runFrom: number;
  /** ClockWorks: the laser was armed by `BossChargeFx`. */
  armed: boolean;
  readonly laser: LaserBeam;
  /** BigMuscle ATK4 (`v3OldPos`) and Krrr ATK4 (`vLocal`), per boss. */
  remembered: V3 | null;
  /** Meteonyker: the breath is on from tick `breathFrom` (exclusive) to `breathUntil` (inclusive, `Infinity` while open). */
  breathing: boolean;
  breathFrom: number;
  breathUntil: number;
  readonly breath: FireDragonPool;
  readonly charges: SfxHandle[];
  /** The `OBJSTA_ATK_ALL` bits the render view showed at the last tick. */
  attack: number;
  /** The tick of the last hit or charge request: the render view shows the state of the last DRAWN frame, so for a few ticks "no attack state" is not yet the end of the swing. */
  lastEvent: number;
}

/** Ticks after an event during which a missing attack state in the render view does not end the swing (the view lags the sim by about a frame). */
export const STATE_GRACE_TICKS = 12;

const OBJSTA_ATK_MASK = OBJSTA_ATK_ALL;
const MAX_EFFECT_LOG = 128;

export class BossFx {
  private readonly bosses = new Map<number, Boss>();
  private readonly warned = new Set<number>();
  private readonly rand = new Rand15(0x5eed);
  private tickCount = 0;
  private breathEmitted = 0;
  private readonly counters = { charges: 0, laserTicks: 0, sparks: 0, poseMissing: 0, hits: 0 };
  private lastLaser: BossProbe['laser'] = null;
  private readonly log: number[] = [];
  private readonly a: V3 = [0, 0, 0];
  private readonly b: V3 = [0, 0, 0];
  private readonly c: V3 = [0, 0, 0];

  constructor(private readonly d: BossFxDeps) {}

  private uniform(): number {
    return this.d.uniform ? this.d.uniform() : this.rand.next() / RAND_MAX;
  }

  // ---- state ---------------------------------------------------------------------------------------------------------------

  private bossOf(id: number): Boss | undefined {
    const known = this.bosses.get(id);
    if (known) return known;
    const prop = this.d.propOf(id);
    const kind = prop === undefined ? undefined : bossKindOf(prop, this.d.better());
    if (kind === undefined) return undefined;
    const b: Boss = {
      id,
      kind,
      hitCont: false,
      runFrom: 0,
      armed: false,
      laser: new LaserBeam(),
      remembered: null,
      breathing: false,
      breathFrom: 0,
      breathUntil: Number.POSITIVE_INFINITY,
      breath: new FireDragonPool(),
      charges: [],
      attack: 0,
      lastEvent: this.tickCount,
    };
    this.bosses.set(id, b);
    return b;
  }

  // ---- poses ---------------------------------------------------------------------------------------------------------------

  private missing(id: number): void {
    this.counters.poseMissing++;
    const prop = this.d.propOf(id) ?? -1;
    if (!this.warned.has(prop)) {
      this.warned.add(prop);
      this.d.warn?.(
        `boss fx: no event point or bone for mover model ${String(prop)}; using its position`,
      );
    }
  }

  /** The position of mover `id` (LH) into `out`; `false` when the render view does not know it. */
  private body(id: number, out: V3): boolean {
    return this.d.world().posOf(id, out);
  }

  /** Event point `index` of the boss in the world, else the body position (counted). */
  private event(id: number, index: number, out: V3): boolean {
    const w = this.d.world();
    if (w.eventPos?.(id, index, out) === true) return true;
    this.missing(id);
    return w.posOf(id, out);
  }

  /** The origin of bone `bone` in the world, else the body position (counted). */
  private bone(
    id: number,
    bone: number,
    local: readonly [number, number, number],
    out: V3,
  ): boolean {
    const w = this.d.world();
    if (w.bonePos?.(id, bone, local, out) === true) return true;
    this.missing(id);
    return w.posOf(id, out);
  }

  // ---- making effects ------------------------------------------------------------------------------------------------------

  private make(xi: number, o: SfxOptions, at: readonly [number, number, number]): SfxHandle {
    this.d.hooks().sfxCreated?.(xi, at);
    this.log.push(xi);
    if (this.log.length > MAX_EFFECT_LOG) this.log.shift();
    return this.d.create(xi, o);
  }

  private at(xi: number, pos: V3, o: SfxOptions = {}): SfxHandle {
    return this.make(xi, { pos: [pos[0], pos[1], pos[2]], ...o }, pos);
  }

  /** `CreateSfx( XI_NPCSP1RANBALL, ... )` + `ShootSfx(...)`: the ball from `from` to the target (also a Meteonyker's fire ball). */
  private ball(boss: Boss, target: number, from: V3): void {
    const tpos: V3 = [0, 0, 0];
    if (target === 0 || !this.body(target, tpos)) return;
    const h = this.make(
      XI_NPCSP1RANBALL,
      {
        src: boss.id,
        pos: [from[0], from[1], from[2]],
        dest: target,
        posDest: [tpos[0], tpos[1], tpos[2]],
        nSec: -1,
        scale: SCALE_BALL,
      },
      from,
    );
    const inst = this.d.instanceOf(h);
    if (!inst) return;
    const { angXZ, angH } = xGetDegree([tpos[0] - from[0], tpos[1] - from[1], tpos[2] - from[2]]);
    shootStraight(inst, angXZ, -angH, BALL_SPEED, XI_NPCSP1RANBOOM);
  }

  // ---- the hit handlers ----------------------------------------------------------------------------------------------------

  /** One melee hit of a boss swing. */
  meleeHit(attacker: number, target: number, state: number, hitCount: number): void {
    const boss = this.bossOf(attacker);
    if (!boss) return;
    this.counters.hits++;
    // a hit after the first of the swing: the breath ends (the continue of this frame ran before the hit)
    if (boss.kind === 'meteonyker' && state === OBJSTA_ATK2 && boss.breathing && hitCount >= 1) {
      boss.breathUntil = Math.min(boss.breathUntil, this.tickCount + 1);
    }
    if (!boss.hitCont) {
      boss.hitCont = true;
      // the first tick after the event is the tick of the hit itself: the continue runs BEFORE the hit in a frame, so it makes no laser and no breath
      boss.runFrom = this.tickCount + 2;
    }
    boss.lastEvent = this.tickCount;
    switch (boss.kind) {
      case 'clockworks':
        this.clockworks(boss, target, state);
        break;
      case 'bigmuscle':
        this.bigMuscle(boss, target, state, hitCount);
        break;
      case 'krrr':
        this.krrr(boss, target, state, hitCount);
        break;
      case 'mushmoot':
        if (state === OBJSTA_ATK3 && hitCount === 0 && this.body(boss.id, this.a)) {
          this.at(XI_SKILL_CIRCLE_DUST, this.a, { scale: 12 });
        }
        break;
      case 'meteonyker':
        this.meteonyker(boss, target, state, hitCount);
        break;
    }
  }

  private angleOf(id: number): number {
    const a = this.d.world().angleOf(id);
    return Number.isNaN(a) ? 0 : a;
  }

  private clockworks(boss: Boss, target: number, state: number): void {
    const id = boss.id;
    const hooks = this.d.hooks();
    switch (state) {
      case OBJSTA_ATK1:
        if (this.event(id, 0, this.a)) {
          hooks.quake?.(15, 0.5);
          this.at(XI_NPCSP1DIRFIRESP, this.a, { angle: -this.angleOf(id) });
        }
        break;
      case OBJSTA_ATK2: {
        if (!this.event(id, 1, this.a)) break;
        this.at(XI_NPCSP1DIRCANNON, this.a, {
          src: id,
          angle: -this.angleOf(id),
          scale: SCALE_BALL,
        });
        this.ball(boss, target, this.a);
        break;
      }
      case OBJSTA_ATK3:
        hooks.quake?.(30, 0.7);
        break;
      case OBJSTA_ATK4:
        hooks.quake?.(60, 1.5);
        if (this.event(id, 3, this.a)) this.at(XI_NPCSP1DIRCIRCLE, this.a, { scale: 2 });
        if (this.event(id, 4, this.a)) this.at(XI_NPCSP1DIRCIRCLE, this.a, { scale: 2 });
        break;
    }
  }

  private bigMuscle(boss: Boss, target: number, state: number, hitCount: number): void {
    const id = boss.id;
    const dust = (bone: number, scale: number): void => {
      if (!this.bone(id, bone, [0, 0, 0], this.a)) return;
      this.a[1] -= BIGMUSCLE_DUST_DROP;
      this.at(XI_SKILL_CIRCLE_DUST, this.a, { scale });
    };
    if (state === OBJSTA_ATK3) {
      dust(BIGMUSCLE_BONES[0], 12);
      dust(BIGMUSCLE_BONES[1], 12);
    } else if (state === OBJSTA_ATK4) {
      if (hitCount === 0) {
        // `v3OldPos = pHitObj->GetPos()`: per boss
        boss.remembered = this.body(target, this.b) ? [this.b[0], this.b[1], this.b[2]] : null;
      } else if (hitCount === 1) {
        dust(BIGMUSCLE_BONES[0], 9);
        if (boss.remembered) this.at(XI_SKILL_MAG_EARTH_ROCKCRASH01, boss.remembered, { scale: 2 });
      }
    }
  }

  private krrr(boss: Boss, target: number, state: number, hitCount: number): void {
    const id = boss.id;
    if (state === OBJSTA_ATK3) {
      // the axe lands at the HIT TARGET
      if (this.body(target, this.a)) {
        this.at(XI_NPCSP1DIRFIRESP, this.a, { angle: -this.angleOf(id) });
      }
    } else if (state === OBJSTA_ATK4) {
      if (hitCount === 0) {
        // `vLocal` = 5 m in front of the boss, on the ground there: per boss
        if (!this.body(id, this.a)) return;
        const fwd = angleToVectorXZ(this.angleOf(id), KRRR_METEOR_DISTANCE);
        const x = this.a[0] + fwd[0];
        const z = this.a[2] + fwd[2];
        const y = this.d.world().heightAt?.(x, z) ?? this.a[1];
        boss.remembered = [x, y, z];
      } else if (hitCount === 2 && boss.remembered) {
        const base = boss.remembered;
        this.at(XI_SKILL_ELE_MULTI_METEOSHOWER02, base, { scale: 0.8 });
        const p1: V3 = [
          base[0] + (this.uniform() * KRRR_METEOR_SPREAD - KRRR_METEOR_SPREAD / 2),
          base[1],
          base[2],
        ];
        this.at(XI_SKILL_ELE_MULTI_METEOSHOWER02, p1, { scale: 0.8 });
        const p2: V3 = [
          base[0],
          base[1],
          base[2] + (this.uniform() * KRRR_METEOR_SPREAD - KRRR_METEOR_SPREAD / 2),
        ];
        this.at(XI_SKILL_ELE_MULTI_METEOSHOWER02, p2, { scale: 0.8 });
      }
    }
  }

  private meteonyker(boss: Boss, target: number, state: number, hitCount: number): void {
    const id = boss.id;
    if (state === OBJSTA_ATK3 && hitCount === 0) {
      if (target !== 0 && this.event(id, 0, this.a)) this.ball(boss, target, this.a);
    } else if (state === OBJSTA_ATK4 && hitCount === 0) {
      // `GetSkillProp( SI_GEN_BURN, 1 )`: without the row the original makes nothing
      if (this.d.hooks().hasSkillRow?.(SI_GEN_BURN, 1) !== false && this.body(id, this.a)) {
        this.at(XI_NPCMETEONYKER, this.a);
      }
    } else if (state === OBJSTA_ATK2 && hitCount === 0 && !boss.breathing) {
      // `OnAttackMeleeContinue` breathes while `m_nMotionHitCount == 1`: from the tick after this hit
      boss.breathing = true;
      boss.breathFrom = this.tickCount + 1;
      boss.breathUntil = Number.POSITIVE_INFINITY;
    }
  }

  // ---- charges -------------------------------------------------------------------------------------------------------------

  /** `BossChargeFx{state}`: the charge effects. */
  charge(id: number, state: number): void {
    const boss = this.bossOf(id);
    if (!boss) return;
    if (boss.kind === 'clockworks' && state === OBJSTA_ATK3) {
      boss.armed = true;
      boss.lastEvent = this.tickCount;
      this.chargeFx(boss, XI_NPCSP1DIRAMP, 2, state);
    } else if (boss.kind === 'mushmoot') {
      boss.lastEvent = this.tickCount;
      if (state === OBJSTA_ATK3) this.chargeFx(boss, XI_SKILL_MUSHMOOT_CHARGE, 0, state);
      else if (state === OBJSTA_ATK4)
        for (let i = 1; i <= 4; i++) this.chargeFx(boss, XI_SKILL_MUSHMOOT_CHARGE, i, state);
    }
  }

  private chargeFx(boss: Boss, xi: number, eventIndex: number, state: number): void {
    if (!this.body(boss.id, this.a)) return;
    const h = this.make(
      xi,
      { src: boss.id, pos: [this.a[0], this.a[1], this.a[2]], nSec: -1, scale: SCALE_CHARGE },
      this.a,
    );
    const inst = this.d.instanceOf(h);
    if (inst) {
      inst.bh.count = eventIndex;
      inst.bh.timer = state;
    }
    boss.charges.push(h);
    this.counters.charges++;
  }

  // ---- the 60 Hz step ------------------------------------------------------------------------------------------------------

  tick(steps: number): void {
    for (let s = 0; s < steps; s++) {
      this.tickCount++;
      for (const boss of this.bosses.values()) this.tickOne(boss);
      for (const [id, boss] of this.bosses) {
        if (
          boss.charges.length === 0 &&
          boss.breath.live === 0 &&
          !boss.hitCont &&
          !boss.armed &&
          !boss.breathing &&
          !this.body(id, this.a)
        ) {
          this.bosses.delete(id);
        }
      }
    }
  }

  private tickOne(boss: Boss): void {
    const w = this.d.world();
    const present = w.posOf(boss.id, this.a);
    const state = present ? (w.stateOf?.(boss.id) ?? 0) : 0;
    const attack = state & OBJSTA_ATK_MASK;
    boss.attack = attack;
    const settled = this.tickCount - boss.lastEvent > STATE_GRACE_TICKS;
    if (!present || (attack === 0 && settled)) {
      // the swing is over (or the boss left the view / died): the flags go
      boss.hitCont = false;
      boss.armed = boss.armed && present && attack !== 0;
      boss.breathing = false;
      boss.laser.off();
    }
    if (boss.hitCont) {
      // the end of a charge without a hit is the class's business; the first hit ends the charge for good (`MVRF_HITCONT` in `Process`)
      this.dropCharges(boss);
    } else if (boss.charges.length > 0 && attack === 0 && settled) {
      this.dropCharges(boss);
    }
    const run = this.tickCount >= boss.runFrom;
    if (boss.kind === 'clockworks') {
      if (attack !== OBJSTA_ATK3) {
        // off with the state; within the grace the view may simply not show the swing yet
        if (settled) {
          boss.armed = false;
          boss.laser.off();
        }
      } else if (boss.armed && boss.hitCont && run) {
        this.laserTick(boss);
      }
    } else if (boss.kind === 'meteonyker') {
      if (attack !== OBJSTA_ATK2 && settled) boss.breathing = false;
      if (boss.breathing && boss.hitCont) {
        if (this.tickCount > boss.breathUntil) boss.breathing = false;
        else if (this.tickCount > boss.breathFrom) this.breathTick(boss);
      }
      boss.breath.step(() => this.uniform());
    }
  }

  private dropCharges(boss: Boss): void {
    for (const h of boss.charges) this.d.remove(h);
    boss.charges.length = 0;
  }

  private laserTick(boss: Boss): void {
    const id = boss.id;
    if (!this.event(id, 2, this.a)) return;
    const v1: V3 = [this.a[0], this.a[1], this.a[2]];
    let v2: V3;
    if (this.bone(id, LASER_BONE, [0, 0, LASER_REACH], this.b)) {
      v2 = [this.b[0], this.b[1], this.b[2]];
    } else {
      v2 = v1;
    }
    boss.laser.tick(v1, v2);
    this.counters.laserTicks++;
    this.lastLaser = { from: [v1[0], v1[1], v1[2]], to: [v2[0], v2[1], v2[2]] };
    // `(m_nCount & 1) == 0`: every second laser tick (the phase counts from the first one: documented deviation of the free-running counter)
    if (boss.laser.ticks % 2 === 1) {
      const hit = marchTerrain(v1, v2, (x, z) => this.d.world().heightAt?.(x, z), this.c);
      if (hit) {
        this.counters.sparks++;
        this.at(XI_NPCSP1RANSPARK, this.c, { scale: 2 });
      }
    }
  }

  private breathTick(boss: Boss): void {
    const id = boss.id;
    const w = this.d.world();
    if (!this.event(id, 0, this.a)) return;
    if (!this.event(id, 1, this.b)) return;
    const dx = this.b[0] - this.a[0];
    const dy = this.b[1] - this.a[1];
    const dz = this.b[2] - this.a[2];
    const len = Math.hypot(dx, dy, dz);
    const dir: V3 = len > 0 ? [dx / len, dy / len, dz / len] : [0, 0, 0];
    const scale = 0.3 * (w.scaleOf?.(id) ?? 1);
    for (let i = 0; i < BREATH_PER_TICK; i++) {
      const speed = (this.uniform() * 0.2 + 0.1) * 0.8;
      const vel: V3 = [
        dir[0] * speed,
        dir[1] * speed - (this.uniform() * 0.05 + 0.01),
        dir[2] * speed,
      ];
      if (boss.breath.create(this.a, scale, vel)) this.breathEmitted++;
    }
  }

  // ---- the frame -----------------------------------------------------------------------------------------------------------

  /** Draws the lasers and the breath pools into the effect list (once per frame; `textureId` is `SfxTextures.drawable`). */
  draw(list: DrawList, frame: SfxFrame, textureId: (path: string) => number): void {
    if (this.bosses.size === 0) return;
    const m = frame.invViewRot;
    const forward: V3 = [m[8] ?? 0, m[9] ?? 0, m[10] ?? 0];
    const eye: V3 = [frame.cameraPos[0], frame.cameraPos[1], frame.cameraPos[2]];
    let laserTex = -2;
    let fireTex = -2;
    for (const boss of this.bosses.values()) {
      if (boss.laser.active) {
        if (laserTex === -2) laserTex = textureId(LASER_TEXTURE);
        boss.laser.emit(list, laserTex, eye, forward);
      }
      if (boss.breath.live > 0) {
        if (fireTex === -2) fireTex = textureId(FIRE_DRAGON_TEXTURE);
        boss.breath.emit(list, fireTex, m);
      }
    }
  }

  // ---- probes --------------------------------------------------------------------------------------------------------------

  probe(): BossProbe {
    let live = 0;
    for (const b of this.bosses.values()) live += b.breath.live;
    return {
      charges: this.counters.charges,
      laserTicks: this.counters.laserTicks,
      sparks: this.counters.sparks,
      breathLive: live,
      breathEmitted: this.breathEmitted,
      poseMissing: this.counters.poseMissing,
      hits: this.counters.hits,
      effects: [...this.log],
      bosses: this.bosses.size,
      laser: this.lastLaser,
    };
  }

  /** Per-boss state for the tests. */
  stateOf(id: number): Readonly<Boss> | undefined {
    return this.bosses.get(id);
  }

  clear(): void {
    for (const b of this.bosses.values()) this.dropCharges(b);
    this.bosses.clear();
  }
}

// ---- the terrain march of the laser ---------------------------------------------------------------------------------------------

/** The step of the march along the beam. */
const MARCH_STEP = 0.5;

/**
 * `IntersectObjLine( .., bWithTerrain = TRUE, bWithObject = FALSE )` for the beam (visual only): the first point of the segment `a -> b` that lies at or below the terrain, refined by
 * bisection. `false` when the segment stays above the ground (or the ground is unknown).
 */
export function marchTerrain(
  a: readonly number[],
  b: readonly number[],
  heightAt: (x: number, z: number) => number | undefined,
  out: V3,
): boolean {
  const dx = (b[0] ?? 0) - (a[0] ?? 0);
  const dy = (b[1] ?? 0) - (a[1] ?? 0);
  const dz = (b[2] ?? 0) - (a[2] ?? 0);
  const len = Math.hypot(dx, dy, dz);
  if (len === 0) return false;
  const n = Math.max(1, Math.ceil(len / MARCH_STEP));
  const above = (t: number): boolean | undefined => {
    const x = (a[0] ?? 0) + dx * t;
    const z = (a[2] ?? 0) + dz * t;
    const h = heightAt(x, z);
    return h === undefined ? undefined : (a[1] ?? 0) + dy * t >= h;
  };
  let prev = 0;
  const first = above(0);
  if (first === undefined) return false;
  if (!first) {
    out[0] = a[0] ?? 0;
    out[1] = a[1] ?? 0;
    out[2] = a[2] ?? 0;
    return true;
  }
  for (let i = 1; i <= n; i++) {
    const t = i / n;
    const up = above(t);
    if (up === undefined) return false;
    if (!up) {
      let lo = prev;
      let hi = t;
      for (let k = 0; k < 12; k++) {
        const mid = (lo + hi) / 2;
        if (above(mid) === false) hi = mid;
        else lo = mid;
      }
      out[0] = (a[0] ?? 0) + dx * hi;
      out[1] = (a[1] ?? 0) + dy * hi;
      out[2] = (a[2] ?? 0) + dz * hi;
      return true;
    }
    prev = t;
  }
  return false;
}

export { XI_NAT_FIRE01_ADV, OBJSTA_ATK4 };
