// The client skill runtime (Plan/17 sections 6.3, 6.13, 6.14, 7.2, 11.1, 11.3, 11.5; WP-SKILL-12): the handlers of the skill
// messages and the client skill state they feed.
//
//   SpawnSelf, JobChanged, DoUseSkillPointNtf, InitSkillPoint, CmdSetSkillLevel   Core   the 45-slot job-skill table and the points
//   UseSkillNtf          Sim   the cast starts on the caster's copy: `SET_SKILL` + the cast animation (own mover: `local_act`,
//                              others: `ACT` at the server frame); the cast bar, the cast effects, the TID_GAME_SKILL_* lines
//   ClearUseSkill        Feature  the request was refused: `REQ_USESKILL` off, the action slot stops
//   EndSkillQueueNtf, SetActionPoint                                                the action-slot chain and its points
//   DoApplyUseSkill, CommonSkill                                                    the hit effect of a ctrl; the white flash
//   AttrHit (sim)        the local hit frame of a skill: the skill effects (`CreateSkillSfx`) for every mover, and for the own
//                        mover the cool-down (the first hit), the Blinkpool `TeleSkill` and nothing else
//
// What does NOT happen here, on purpose: the damage (the server's `Damage`), the projectile of a shot skill (`ProjectileSpawn`), the
// hit-stop of a skill hit (4 frames, inside the sim), the buff mirror (`features/buffs`), the buff loop effects (`buffs.ts`), the
// skill-tree editing and the skill points UI (45 WP-CFEAT-31), `SendToServerAp` (the V15 client never sent it, Plan/17 6.13).

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../app/context';
import { inputProvidersOf } from '../../app/inputProviders';
import type { ClientItem } from '../../data';
import { KT_MAGIC, KT_SKILL } from '../../gen/defs/attribute';
import { EXT_MAGICATKSHOT, EXT_MAGICSHOT } from '../../gen/defs/define';
import { JTYPE_HERO, JTYPE_MASTER } from '../../gen/defs/job';
import {
  SI_ASS_HEAL_HEALING,
  SI_GEN_ATK_COUNTER,
  SI_MAG_MAG_BLINKPOOL,
} from '../../gen/defs/skill';
import { TID_GAME_BLOCKTARGETING } from '../../gen/defs/text';
import { Vec3Schema } from '../../gen/proto/flyff/v1/common_pb';
import { TeleSkillSchema } from '../../gen/proto/flyff/v1/skill_pb';
import { n64 } from '../../net/int64';
import { RENDER_VIEW } from '../../gen/sim/wire';
import { Phase } from '../../net/registry';
import type { SkillAccess, SkillInfo } from '../../input/ports';
import type { SimEventRecord } from '../simEvents';
import { EntityKind } from '../entities';
import { worldRuntime } from '../world/runtime';
import { combatRuntime } from './runtime';
import { NULL_ID } from '../world/constants';
import type { DrainRow } from '../../rules';
import { OBJACT, OBJMSG } from './constants';
import { createSkillFx, type SkillFx } from './skillFx';
import { isNullNumber, SkillProps } from './skillProps';

const OWNER = 'skills';

/** `MAX_SKILL_JOB`. */
export const SKILL_SLOTS = 45;
/** `SKILLUSETYPE`. */
export const SUT = { NORMAL: 0, QUEUESTART: 1, QUEUEING: 2 } as const;
/** One server tick (15 Hz) in milliseconds: the unit of `UseSkillNtf.casting_time`. */
export const TICK_MS = 1000 / 15;
/** `HitKind` codes of the `AttrHit` payload (`flyff-sim client/events.rs hit_kind_code`). */
const HIT_MELEE_SKILL = 1;
const HIT_MAGIC_SKILL = 2;
/** The Blinkpool line test: static meshes and ctrl colliders, never the terrain (`IntersectObjLine` without terrain). */
const RAY_STATIC = 2;
const RAY_CTRL = 4;
/** Where the white flash of `CommonSkill` stands (Plan/17 11.1). */
const FLASH_HOLD_MIN_MS = 0;

export interface SkillSlot {
  /** `SI_*` id; `NULL_ID` for an empty slot. */
  readonly skillId: number;
  /** The stored level (`0` = not learnt). */
  readonly level: number;
}

export const EMPTY_SLOT: SkillSlot = { skillId: NULL_ID, level: 0 };

/** The own cast in progress (the cast bar, Plan/17 11.1: `cast_time * 66.667 ms`). */
export interface CastState {
  readonly skillId: number;
  readonly level: number;
  readonly slot: number;
  readonly useType: number;
  readonly startMs: number;
  readonly endMs: number;
}

/** `CWndTaskBar::m_nExecute`, `m_nUsedSkillQueue` and `m_nActionPoint`. */
export interface ActionSlotState {
  /** 0 idle, 1 waiting for the first cast, 2 running. */
  executing: 0 | 1 | 2;
  /** The queue index being executed (0 at the start, +1 for every chained skill). */
  usedIndex: number;
  /** 0..100, the server's `SetActionPoint` (plus the local +1 of a wand shot). */
  actionPoint: number;
}

export interface CooldownView {
  readonly startMs: number;
  readonly endMs: number;
}

/** The white full-screen flash of `CommonSkill` (`g_Flash.SetTime(circleTime, skillTime)`). */
export interface FlashState {
  readonly startMs: number;
  readonly circleMs: number;
  readonly skillMs: number;
}

/** The late-bound answers of the features. */
export interface SkillHooks {
  /** `IsBullet(prop)`; default: `inputProviders.bulletEquipped` or true. */
  bullet?(prop: ClientItem): boolean;
  /** A system-chat line (`PutString`); the action-slot feature installs it. */
  chatLine?(text: string, tid: number): void;
}

export interface SkillStats {
  /** `UseSkillNtf` handled (own and others). */
  casts: number;
  castsOwn: number;
  /** Own starts the sim refused (`local_act` <= 0). */
  ownRefused: number;
  clears: number;
  hits: number;
  hitEffects: number;
  cooldownsSet: number;
  teleSent: number;
  teleBlocked: number;
  queueEnds: number;
  apUpdates: number;
  flashes: number;
  appliedByCtrl: number;
}

export interface SkillRuntime {
  readonly props: SkillProps;
  readonly stats: SkillStats;
  readonly hooks: SkillHooks;
  /** The 45 job-skill slots of the own character. */
  readonly table: readonly SkillSlot[];
  /** Bumps on every change of the table, the cool-downs, the cast and the action slot. */
  readonly version: number;
  readonly actionSlot: ActionSlotState;
  readonly cast: CastState | null;
  readonly flash: FlashState | null;
  /** The slot of skill `skillId`, `-1` when the character has none. */
  slotOf(skillId: number): number;
  /** `GetSkill(0, slot)`; `null` for an empty slot. */
  skillAt(slot: number): SkillSlot | null;
  /** The cool-down of a slot while it runs (`GetReuseDelay`), `null` when it is ready. */
  cooldown(slot: number, nowMs?: number): CooldownView | null;
  /** `[elapsed, total]` of the own cast for the head gauge, `null` when no cast is running. */
  castGauge(nowMs: number): [number, number] | null;
  /** Blinkpool: the ground point `cmdSetUseSkill` chose (`m_vTelePos`). */
  telePos: readonly [number, number, number] | null;
  /** The view of the input layer (`InputDeps.skills`). */
  readonly access: SkillAccess;
  /** One listener for every change; returns the unsubscribe function. */
  subscribe(fn: () => void): () => void;
  /** The action points shown grow by one (a wand shot with power >= 1, Plan/43 7.9). */
  actionPointUp(): void;
  /** `m_nExecute` (the input layer starts and cancels the queue). */
  setExecuting(n: 0 | 1 | 2): void;
  /** Forgets the cast, the cool-downs and the queue state (a new world, a reconnect). */
  reset(): void;
  /** Runs the waiting own starts and line tests (one per tick system call; also callable from tests). */
  pump(): void;
  /** Own casts waiting for a query slot, line tests waiting for one. */
  readonly pending: number;
}

const runtimes = new WeakMap<ClientContext, SkillRuntimeImpl>();

export function peekSkillsRuntime(ctx: ClientContext): SkillRuntime | undefined {
  return runtimes.get(ctx);
}

/** The skill runtime of a context (created on first use). */
export function skillsRuntime(ctx: ClientContext): SkillRuntime {
  const known = runtimes.get(ctx);
  if (known) return known;
  const rt = new SkillRuntimeImpl(ctx);
  runtimes.set(ctx, rt);
  return rt;
}

interface PendingStart {
  readonly msg: number;
  readonly p: readonly [number, number, number, number, number];
}

type Op = () => 'done' | 'retry';

class SkillRuntimeImpl implements SkillRuntime {
  readonly props: SkillProps;
  readonly stats: SkillStats = {
    casts: 0,
    castsOwn: 0,
    ownRefused: 0,
    clears: 0,
    hits: 0,
    hitEffects: 0,
    cooldownsSet: 0,
    teleSent: 0,
    teleBlocked: 0,
    queueEnds: 0,
    apUpdates: 0,
    flashes: 0,
    appliedByCtrl: 0,
  };
  readonly hooks: SkillHooks = {};
  table: SkillSlot[] = Array.from({ length: SKILL_SLOTS }, () => EMPTY_SLOT);
  version = 0;
  readonly actionSlot: ActionSlotState = { executing: 0, usedIndex: 0, actionPoint: 0 };
  cast: CastState | null = null;
  flash: FlashState | null = null;
  telePos: readonly [number, number, number] | null = null;
  /** The skill a mover is casting (set by `UseSkillNtf`, read by the hit frame). */
  readonly casting = new Map<
    number,
    { skillId: number; level: number; target: number; slot: number }
  >();
  private readonly cdStart = new Float64Array(SKILL_SLOTS);
  private readonly cdEnd = new Float64Array(SKILL_SLOTS);
  private readonly listeners = new Set<() => void>();
  private readonly starts: PendingStart[] = [];
  private readonly ops: Op[] = [];
  readonly fx: SkillFx;

  constructor(readonly ctx: ClientContext) {
    this.props = new SkillProps(ctx);
    this.fx = createSkillFx(ctx, this);
  }

  // ---- table ---------------------------------------------------------------------------------------------------------

  setTable(skills: readonly { skillId: number; level: number }[]): void {
    const next: SkillSlot[] = [];
    for (let i = 0; i < SKILL_SLOTS; i += 1) {
      const s = skills[i];
      next.push(s ? { skillId: s.skillId >>> 0, level: s.level >>> 0 } : EMPTY_SLOT);
    }
    this.table = next;
    this.changed();
  }

  setLevel(skillId: number, level: number): void {
    const i = this.slotOf(skillId);
    const cur = this.table[i];
    if (i < 0 || !cur || cur.level === level) return;
    const next = [...this.table];
    next[i] = { skillId: cur.skillId, level };
    this.table = next;
    this.changed();
  }

  /** `InitSkillPoint`: every non master / hero skill goes back to level 0. */
  resetLevels(): void {
    this.table = this.table.map((s) => {
      if (s.skillId === NULL_ID || s.level === 0) return s;
      const kind = this.props.prop(s.skillId)?.item_kind1;
      return kind === JTYPE_MASTER || kind === JTYPE_HERO ? s : { skillId: s.skillId, level: 0 };
    });
    this.changed();
  }

  slotOf(skillId: number): number {
    return this.table.findIndex((s) => s.skillId === skillId);
  }

  skillAt(slot: number): SkillSlot | null {
    const s = this.table[slot];
    return s && s.skillId !== NULL_ID ? s : null;
  }

  // ---- cool-downs -----------------------------------------------------------------------------------------------------

  cooldown(slot: number, nowMs = this.ctx.now()): CooldownView | null {
    const end = this.cdEnd[slot];
    if (end === undefined || end <= nowMs) return null;
    return { startMs: this.cdStart[slot] ?? end, endMs: end };
  }

  /** `SetCoolTime`: the slot of `skillId` is busy for `ms`. */
  setCooldown(skillId: number, ms: number): void {
    const slot = this.slotOf(skillId);
    if (slot < 0) return;
    const now = this.ctx.now();
    this.cdStart[slot] = now;
    this.cdEnd[slot] = now + ms;
    this.stats.cooldownsSet += 1;
    this.changed();
  }

  // ---- cast bar --------------------------------------------------------------------------------------------------------

  castGauge(nowMs: number): [number, number] | null {
    const c = this.cast;
    if (!c) return null;
    if (nowMs >= c.endMs) {
      this.cast = null;
      return null;
    }
    return [nowMs - c.startMs, c.endMs - c.startMs];
  }

  // ---- the views of the input layer -----------------------------------------------------------------------------------------

  readonly access: SkillAccess = {
    skill: (slot) => this.info(slot),
    isBullet: (slot) => this.isBullet(slot),
    setTelePos: (p) => {
      this.telePos = p === null ? null : [p[0], p[1], p[2]];
    },
    telePos: () =>
      this.telePos === null ? null : [this.telePos[0], this.telePos[1], this.telePos[2]],
  };

  private info(slot: number): SkillInfo | null {
    const s = this.skillAt(slot);
    if (!s) return null;
    const prop = this.props.prop(s.skillId);
    if (!prop) return null;
    const add = this.props.add(prop, s.level);
    return {
      slot,
      skillId: s.skillId,
      level: s.level,
      useChance: prop.use_chance,
      exeTarget: prop.exe_target,
      evildoing: prop.evildoing,
      attackRange: prop.attack_range,
      atkStyle: prop.atk_style,
      skillRange: add?.skill_range ?? 0,
      skillType: prop.skill_type,
    };
  }

  private isBullet(slot: number): boolean {
    const s = this.skillAt(slot);
    const prop = s ? this.props.prop(s.skillId) : undefined;
    if (!prop || isNullNumber(prop.link_kind_bullet)) return true;
    if (this.hooks.bullet) return this.hooks.bullet(prop);
    if (this.ctx.features.features.no_arrows_poster) return true;
    return inputProvidersOf(this.ctx).bulletEquipped?.() ?? true;
  }

  // ---- changes -----------------------------------------------------------------------------------------------------------

  subscribe(fn: () => void): () => void {
    this.listeners.add(fn);
    return () => {
      this.listeners.delete(fn);
    };
  }

  changed(): void {
    this.version += 1;
    for (const fn of [...this.listeners]) {
      try {
        fn();
      } catch (error) {
        this.ctx.log.error('skills', `listener: ${String(error)}`);
      }
    }
  }

  setActionPoint(ap: number): void {
    const v = Math.max(0, Math.min(100, Math.trunc(ap)));
    if (v === this.actionSlot.actionPoint) return;
    this.actionSlot.actionPoint = v;
    this.changed();
  }

  actionPointUp(): void {
    if (this.actionSlot.actionPoint < 100) this.setActionPoint(this.actionSlot.actionPoint + 1);
  }

  setExecuting(n: 0 | 1 | 2): void {
    if (this.actionSlot.executing === n) return;
    this.actionSlot.executing = n;
    if (n === 0) this.actionSlot.usedIndex = 0;
    this.changed();
  }

  reset(): void {
    this.cast = null;
    this.flash = null;
    this.telePos = null;
    this.casting.clear();
    this.starts.length = 0;
    this.ops.length = 0;
    this.cdStart.fill(0);
    this.cdEnd.fill(0);
    // the action points are the character's, not the world's: they stay (the join batch and `SetActionPoint` set them)
    this.actionSlot.executing = 0;
    this.actionSlot.usedIndex = 0;
    this.changed();
  }

  // ---- own starts and line tests (cap Q) -------------------------------------------------------------------------------------

  /** An own cast waits for the sim's query slot (`local_act`). */
  queueStart(msg: number, p: readonly [number, number, number, number, number]): void {
    this.starts.push({ msg, p });
  }

  queueOp(op: Op): void {
    this.ops.push(op);
  }

  get pending(): number {
    return this.starts.length + this.ops.length;
  }

  pump(): void {
    const sim = this.ctx.sim;
    while (this.starts.length > 0) {
      if (sim.queryBudget() < 1) return;
      const s = this.starts[0];
      if (!s) break;
      const r = sim.localAct(s.msg, s.p);
      if (r === undefined) return;
      this.starts.shift();
      if (r <= 0) this.stats.ownRefused += 1;
    }
    while (this.ops.length > 0) {
      const op = this.ops[0];
      if (!op || op() === 'retry') return;
      this.ops.shift();
    }
  }
}

// ----------------------------------------------------------------------------------------------------------------------------

/** Registers the skill handlers and the tick system of a context (called by the combat module). */
export function registerSkillHandlers(ctx: ClientContext): SkillRuntime {
  const rt = skillsRuntime(ctx) as SkillRuntimeImpl;
  const { registry } = ctx.net;
  const { selfStore } = ctx.stores;
  const world = worldRuntime(ctx);
  const isSelf = (id: number): boolean => id === 0 || ctx.entities.get(id)?.isSelf === true;
  const selfId = (): number => ctx.entities.self()?.objId ?? 0;

  // the Hot Air projectile needs the pain / skill times of the level the caster cast (`m_nMagicPower` of the effect)
  combatRuntime(ctx).projectiles.hotAirTimes = (attacker, skillId) => {
    const cast = rt.casting.get(attacker);
    const slot = rt.slotOf(skillId);
    const level =
      cast?.skillId === skillId
        ? cast.level
        : isSelf(attacker) && slot >= 0
          ? (rt.table[slot]?.level ?? 1)
          : 1;
    const add = rt.props.addOf(skillId, Math.max(level, 1));
    return add ? { painMs: add.pain_time, skillMs: add.skill_time } : undefined;
  };

  // ---- the table and the points ------------------------------------------------------------------------------------------------
  registry.on('spawnSelf', Phase.Core, OWNER, (_id, m) => {
    rt.reset();
    rt.setTable(m.self?.jobSkills ?? []);
  });
  registry.on('jobChanged', Phase.Core, OWNER, (id, m) => {
    // the others' lists are 20 or 21 entries long and only matter to the sim's stance (not to the table)
    if (isSelf(id)) rt.setTable(m.skills);
  });
  registry.on('doUseSkillPointNtf', Phase.Core, OWNER, (_id, m) => {
    rt.setTable(m.skills);
    selfStore.setState({ skillPoints: m.skillPoint });
  });
  registry.on('initSkillPoint', Phase.Core, OWNER, (_id, m) => {
    rt.resetLevels();
    selfStore.setState({ skillPoints: m.skillPoint });
  });
  registry.on('cmdSetSkillLevel', Phase.Core, OWNER, (_id, m) => {
    rt.setLevel(m.skillId, m.level);
  });
  // `SetSkillLevel` (another player's visible level) is read and ignored (KEEP, ref/09a 5.6)

  // ---- animation speed of the caster ------------------------------------------------------------------------------------------
  const NEUTRAL: DrainRow = {
    speedAdjPct: 0,
    speedChgSet: false,
    chrState: 0,
    hawkeyeRate: 0,
    jumping: 0,
    attackSpeed: 1,
    castingAniSpeed: 1,
  };
  /** The caster's animation speed of the notification replaces the derived one (remote PLAYERS only, 03 PL-087). */
  const applyAniSpeed = (id: number, speed: number, magic: boolean): void => {
    const rec = ctx.entities.get(id);
    if (rec?.kind !== EntityKind.Player || rec.isSelf) return;
    if (!(speed > 0) || !Number.isFinite(speed)) return;
    const cur = world.params.derivedOf(id) ?? NEUTRAL;
    const old = magic ? cur.castingAniSpeed : cur.attackSpeed;
    if (Math.abs(old - speed) < 1e-4) return;
    world.params.setDerived(
      id,
      magic ? { ...cur, castingAniSpeed: speed } : { ...cur, attackSpeed: speed },
    );
    world.params.flushOne(id);
  };

  // ---- UseSkillNtf ---------------------------------------------------------------------------------------------------------------
  registry.on('useSkillNtf', Phase.Sim, OWNER, (casterId, m, mctx) => {
    const id = casterId === 0 ? selfId() : casterId;
    const prop = rt.props.prop(m.skillId);
    if (!prop) return;
    rt.stats.casts += 1;
    const own = isSelf(id);
    const add = rt.props.add(prop, m.level);
    const slot = own ? rt.slotOf(m.skillId) : -1;
    rt.casting.set(id, { skillId: m.skillId, level: m.level, target: m.target, slot });
    // `DoUseSkill`: the message and the motion of the notification (Plan/17 6.3 steps 17-19, as the client runs them)
    const castTicks = Math.max(0, m.castingTime);
    let motion = prop.use_motion;
    let msg: number;
    if (prop.skill_type === KT_MAGIC) {
      if (m.useType === SUT.QUEUEING) {
        motion += 2;
        msg = OBJMSG.MAGICSKILL;
      } else {
        msg = OBJMSG.MAGICCASTING;
      }
    } else if (prop.skill_type === KT_SKILL) {
      msg = OBJMSG.MELEESKILL;
    } else {
      return;
    }
    if (m.useType !== SUT.QUEUEING && castTicks === 0 && m.skillId !== SI_GEN_ATK_COUNTER) {
      motion += 2; // no cast animation
    }
    const p: [number, number, number, number, number] = [
      motion | 0,
      m.target | 0,
      castTicks,
      m.skillId | 0,
      0,
    ];
    // `SetActParam(OBJACT_USESKILL, skill, target)`, level, `nSkillCount`: the context the sim's hit handlers read
    ctx.sim.cmd.setSkill({
      id,
      skill: m.skillId,
      level: m.level,
      target: m.target,
      skillCount: add ? add.skill_count | 0 : 0,
      act: OBJACT.USESKILL,
    });
    if (own) {
      rt.stats.castsOwn += 1;
      // the animation starts after `SET_SKILL` was applied (the sim reads the skill id when the message arrives)
      rt.queueStart(msg, p);
      const now = ctx.now();
      if (castTicks > 0) {
        rt.cast = {
          skillId: m.skillId,
          level: m.level,
          slot,
          useType: m.useType,
          startMs: now,
          endMs: now + castTicks * TICK_MS,
        };
      } else {
        rt.cast = null;
      }
      // the action slot: `m_nExecute` 1 -> 2 at the first cast, the queue index follows the chain
      const a = rt.actionSlot;
      if (m.useType === SUT.QUEUESTART) {
        a.usedIndex = 0;
        a.executing = 2;
      } else if (m.useType === SUT.QUEUEING) {
        a.usedIndex += 1;
        a.executing = 2;
      } else if (a.executing === 1) {
        a.executing = 2;
      }
      rt.changed();
      ctx.input.skills?.clearReqUseSkill();
      ctx.input.skills?.onOwnSkillStarted(m.skillId, prop.exe_target);
    } else {
      applyAniSpeed(id, m.aniSpeed, prop.skill_type === KT_MAGIC);
      const frame = m.serverFrame !== 0n ? n64(m.serverFrame) : mctx.serverFrame;
      ctx.sim.cmd.act(id, frame, msg, p, false);
    }
    rt.fx.onCast(id, own, prop, m.skillId, m.level, m.target);
  });

  registry.on('clearUseSkill', Phase.Feature, OWNER, () => {
    rt.stats.clears += 1;
    rt.cast = null;
    ctx.input.skills?.onClearUseSkill();
    rt.setExecuting(0);
  });
  registry.on('endSkillQueueNtf', Phase.Feature, OWNER, () => {
    rt.stats.queueEnds += 1;
    // `OnCancelSkill` does nothing while a `UseSkillReq` is in flight; the input layer knows (it owns `REQ_USESKILL`)
    ctx.input.skills?.onEndSkillQueue();
    rt.setExecuting(0);
  });
  registry.on('setActionPoint', Phase.Feature, OWNER, (_id, m) => {
    rt.stats.apUpdates += 1;
    rt.setActionPoint(m.actionPoint);
  });

  // ---- effects of the others -----------------------------------------------------------------------------------------------------
  registry.on('doApplyUseSkill', Phase.Fx, OWNER, (src, m) => {
    rt.stats.appliedByCtrl += 1;
    rt.fx.createSkillSfx(src, m.target, m.skillId, m.level);
  });
  registry.on('commonSkill', Phase.Fx, OWNER, (_id, m) => {
    const add = rt.props.addOf(m.skillId, m.level);
    if (!add) return;
    rt.stats.flashes += 1;
    rt.flash = {
      startMs: ctx.now(),
      circleMs: Math.max(FLASH_HOLD_MIN_MS, add.circle_time | 0),
      skillMs: Math.max(0, add.skill_time | 0),
    };
    rt.changed();
  });
  // a new world: the casts and the cool-downs of the old one are gone
  registry.on('replace', Phase.Core, OWNER, () => {
    rt.casting.clear();
    rt.cast = null;
  });
  registry.on('delObj', Phase.Core, OWNER, (id) => {
    rt.casting.delete(id);
  });

  // ---- the hit frame (sim event AttrHit) ------------------------------------------------------------------------------------------
  ctx.sim.events.on(['AttrHit'], (e: SimEventRecord) => {
    const kind = e.words[0];
    if (kind !== HIT_MELEE_SKILL && kind !== HIT_MAGIC_SKILL) return;
    onSkillHit(ctx, rt, e, kind === HIT_MAGIC_SKILL);
  });

  // the waiting own starts / line tests run after the iteration's sim frame (the budget of that iteration is free again)
  ctx.tickSystems.register('skills.pump', () => {
    rt.pump();
  });
  return rt;
}

/** The hit frame of a skill (`CMover::OnMeleeSkill` / `OnMagicSkill`, client branch). */
function onSkillHit(
  ctx: ClientContext,
  rt: SkillRuntimeImpl,
  e: SimEventRecord,
  magic: boolean,
): void {
  const id = e.objId;
  const cast = rt.casting.get(id);
  if (!cast) return;
  const prop = rt.props.prop(cast.skillId);
  if (!prop) return;
  const add = rt.props.add(prop, cast.level);
  const target = e.words[2];
  const hitIndex = e.words[3];
  const own = ctx.entities.get(id)?.isSelf === true;
  rt.stats.hits += 1;
  // the target must still exist (`IsInvalidObj(pFocusObj)` -> nothing happens, no cool-down)
  if (!ctx.entities.has(target)) return;
  if (magic && cast.skillId === SI_ASS_HEAL_HEALING && deadMover(ctx, target)) return;
  const shoot = prop.exe_target === EXT_MAGICATKSHOT || prop.exe_target === EXT_MAGICSHOT;
  if (!shoot) {
    rt.stats.hitEffects += 1;
    rt.fx.createSkillSfx(id, target, cast.skillId, cast.level);
  }
  if (!own) return;
  const finish = (): void => {
    // the cool-down of the first hit (`nCount == 0`) when the skill has one
    if (hitIndex === 0 && add && !isNullNumber(add.cooldown)) {
      rt.setCooldown(cast.skillId, add.cooldown);
    }
  };
  if (magic && cast.skillId === SI_MAG_MAG_BLINKPOOL) {
    rt.queueOp(blinkpoolOp(ctx, rt, finish));
    rt.pump();
    return;
  }
  finish();
}

/** The render view flags a mover as dead (`MOVER_FLAGS.DEAD`). */
function deadMover(ctx: ClientContext, id: number): boolean {
  const view = ctx.sim.view;
  if (view?.valid !== true) return false;
  return (view.flagsOf(id) & RENDER_VIEW.moverFlags.DEAD) !== 0;
}

/**
 * Blinkpool at the own local hit frame (Plan/17 6.14): the object-only line from the own position + 1 y to the chosen point; blocked
 * -> `nextSkill` is cleared, `TID_GAME_BLOCKTARGETING` is shown and neither `TeleSkill` nor the cool-down follows; else exactly one
 * `TeleSkill{pos}` is sent. A call without a query slot waits for the next iteration.
 */
function blinkpoolOp(ctx: ClientContext, rt: SkillRuntimeImpl, done: () => void): Op {
  const tele = rt.telePos;
  let finished = false;
  return () => {
    if (finished) return 'done';
    const self = ctx.entities.self();
    const view = ctx.sim.view;
    if (!self || tele === null) {
      finished = true;
      done();
      return 'done';
    }
    // the render view knows the exact position; the store's copy (4 Hz) is the fallback before the first frame
    const at: [number, number, number] = [0, 0, 0];
    if (view?.valid !== true || !view.posOf(self.objId, at)) {
      const p = ctx.stores.selfStore.getState().pos;
      at[0] = p[0];
      at[1] = p[1];
      at[2] = p[2];
    }
    if (ctx.sim.queryBudget() < 1) return 'retry';
    const from: [number, number, number] = [at[0], at[1] + 1, at[2]];
    const dx = tele[0] - from[0];
    const dy = tele[1] - from[1];
    const dz = tele[2] - from[2];
    const len = Math.hypot(dx, dy, dz);
    let blocked = false;
    if (len > 1e-6) {
      const hit = ctx.sim.rayCast(from, [dx / len, dy / len, dz / len], len, RAY_STATIC | RAY_CTRL);
      if (hit === undefined) return 'retry';
      blocked = hit !== null;
    }
    finished = true;
    if (blocked) {
      rt.stats.teleBlocked += 1;
      ctx.input.skills?.clearNextSkill();
      ctx.input.skills?.notify(TID_GAME_BLOCKTARGETING);
      return 'done';
    }
    // legacy-send: SendTeleSkill
    ctx.net.sender.send({
      case: 'teleSkill',
      value: create(TeleSkillSchema, {
        pos: create(Vec3Schema, { x: tele[0], y: tele[1], z: tele[2] }),
      }),
    });
    rt.stats.teleSent += 1;
    done();
    return 'done';
  };
}

export type { SkillFx } from './skillFx';
