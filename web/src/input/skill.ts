// Skills from the taskbar and the action slot (Plan/43 §7.10, §7.12, Plan/17 §6.3, §6.13, §11.3; WP-INPUT-13, WP-INPUT-14): the client
// half of `CMover::CMD_SetUseSkill`, `CWndMgr::UseSkillShortCut`, the arrival rules of `CMover::ProcessMoveArrival` and the action
// slot control of `CWndTaskBar::UseSkillQueue` / `OnCancelSkill`.
//
//   useSkillShortcut(slot)     an F-key / taskbar skill, a reserved skill (`nextSkill`), the skill window double click
//   cmdSetUseSkill(...)        the preconditions, the target by `use_chance` / `exe_target`, the Blinkpool point, the command
//   onCmdArrived(target)       the sim reached the arrival range: skill timer gate, line of sight, Blinkpool line + move mark,
//                              `AS_BACK`, the PK Ctrl rule, then `UseSkillReq` and `REQ_USESKILL`
//   keyC / useSkillToFocus     the action slot: `nextSkill = ACTIONSLOT`, the queue start with `SUT_QUEUESTART`
//   endQueue / cancel          `EndSkillQueueReq` (a left click during the chain), the target-vanished cancel, take-off
//
// Every ray cast and `blink_point` / `target_check` is a query of the bridge (cap Q = 2 per loop iteration): an invocation that needs
// k slots runs only when k are left and otherwise waits whole in a FIFO that `pump()` runs first in the next iteration (as
// `AttackController` does).

import { create } from '@bufbuild/protobuf';
import { AS_BACK } from '../gen/defs/attribute';
import {
  EXT_ANOTHERWITH,
  EXT_AROUNDATK,
  EXT_SELFCHGPARAMET,
  WUI_NOW,
  WUI_TARGETCURSORPTZ,
  WUI_TARGETINGOBJ,
} from '../gen/defs/define';
import { IK3_CHEERSTICK, IK3_STAFF } from '../gen/defs/itemkind';
import { SI_ASS_HEAL_RESURRECTION, SI_MAG_MAG_BLINKPOOL } from '../gen/defs/skill';
import {
  TID_GAME_BLOCKTARGETING,
  TID_GAME_NEVERKILLSTOP,
  TID_GAME_NOMOVING,
} from '../gen/defs/text';
import { EndSkillQueueReqSchema, UseSkillReqSchema } from '../gen/proto/flyff/v1/skill_pb';
import { NULL_ID } from '../game/world/constants';
import { OBJMSG, STA } from '../game/combat/constants';
import { TARGET_CHECK } from '../gen/sim/wire';
import type { Vec3LH } from '../render/types';
import { LOS_LIFT, LOS_MASK } from './attack';
import { LOCAL_CLEAR } from './commands';
import type { SkillInfo } from './ports';
import { NEXTSKILL_ACTIONSLOT, NEXTSKILL_NONE } from './targeting';
import { REQ_USESKILL, type InputCore } from './worldTick';

/** `SKILLUSETYPE`. */
export const SUT_NORMAL = 0;
export const SUT_QUEUESTART = 1;
export const SUT_QUEUEING = 2;

/** `m_SkillTimer`: a held skill key casts at most once per second. */
export const SKILL_TIMER_MS = 1000;
/** `Q` calls per iteration (Plan/40 section 7.3). */
const Q_CAP = 2;

interface Op {
  step(): 'done' | 'retry';
}

export interface SkillStats {
  /** `UseSkillReq` sent. */
  sent: number;
  /** Arrivals dropped by the one-second skill timer. */
  timerGated: number;
  /** Arrivals refused by the line of sight / the Blinkpool line / `AS_BACK` / the Ctrl rule. */
  blocked: number;
  /** `EndSkillQueueReq` sent. */
  queueEnds: number;
  /** Queue starts (`SUT_QUEUESTART` commands). */
  queueStarts: number;
  deferred: number;
  retried: number;
}

export class SkillController {
  /** `m_SkillTimerStop`: set by any key-up, cleared by any key-down (`WndWorld.cpp:7947, 7969`). */
  skillTimerStop = true;
  /** `m_SkillTimer`: the time of the last skill sent while a key was held. */
  private skillTimerAt = Number.NEGATIVE_INFINITY;
  private readonly ops: Op[] = [];
  /** `CWndTaskBar::m_idTarget`: the target the queue was started on (`NULL_ID` = self / none). */
  queueTarget: number = NULL_ID;
  readonly stats: SkillStats = {
    sent: 0,
    timerGated: 0,
    blocked: 0,
    queueEnds: 0,
    queueStarts: 0,
    deferred: 0,
    retried: 0,
  };

  constructor(private readonly c: InputCore) {}

  // ---- the query budget ---------------------------------------------------------------------------------------------------

  private budget(): number {
    return this.c.deps.sim.queryBudget?.() ?? Q_CAP;
  }

  private submit(op: Op): void {
    if (this.ops.length === 0) {
      if (op.step() === 'done') return;
      this.stats.retried += 1;
    } else {
      this.stats.deferred += 1;
    }
    this.ops.push(op);
  }

  /** The start of an iteration: the invocations that waited run first, in order. */
  pump(): void {
    while (this.ops.length > 0) {
      const op = this.ops[0];
      if (op === undefined || op.step() === 'retry') {
        this.stats.retried += 1;
        return;
      }
      this.ops.shift();
    }
  }

  get pending(): number {
    return this.ops.length;
  }

  /** World change, death: the waiting invocations are forgotten. */
  clear(): void {
    this.ops.length = 0;
  }

  // ---- key state of the timer ------------------------------------------------------------------------------------------------

  /** A key went down (`m_SkillTimerStop = FALSE`). */
  onKeyDown(): void {
    this.skillTimerStop = false;
  }

  /** A key went up (`m_SkillTimerStop = TRUE`). */
  onKeyUp(): void {
    this.skillTimerStop = true;
  }

  // ---- state helpers ----------------------------------------------------------------------------------------------------------

  private actAttack(state: number): boolean {
    return (state & STA.ATK_ALL) !== 0;
  }

  private ctrlHeld(): boolean {
    return (
      this.c.keys.ctrl || this.c.keys.isHeld('ControlLeft') || this.c.keys.isHeld('ControlRight')
    );
  }

  /** The hand item is a staff or a cheer stick (the only weapons that keep the reserved skill after a start). */
  private staffInHand(): boolean {
    const k = this.c.deps.hand?.()?.kind3;
    return k === IK3_STAFF || k === IK3_CHEERSTICK;
  }

  // ---- useSkillShortcut (§7.10) ------------------------------------------------------------------------------------------------

  /**
   * `CWndMgr::UseSkillShortCut(0, slot)`: a skill of the job-skill table from a taskbar slot, a reserved skill or the skill window.
   * The target is the focused mover (`NULL_ID` without a focus); a focused ground item or ctrl, a running action slot, a missing
   * bullet or an unknown skill do nothing.
   */
  useSkillShortcut(slot: number): void {
    const c = this.c;
    const skill = c.deps.skills?.skill(slot) ?? null;
    if (skill === null) return;
    const focus = c.targeting.focus;
    let target: number = NULL_ID;
    if (focus !== null) {
      const o = c.deps.entityInfo(focus);
      if (o === null) return;
      if (o.kind !== 'mover') return;
      target = focus;
    }
    if ((c.deps.actionSlot?.executing() ?? 0) !== 0) return;
    if (c.deps.skills?.isBullet(slot) === false) return;
    const ok = this.cmdSetUseSkill(target, slot, SUT_NORMAL);
    if (ok) {
      c.targeting.nextSkill = NEXTSKILL_NONE;
    } else if (this.actAttack(c.deps.self()?.state ?? 0)) {
      // reserved until the attack animation has ended
      c.targeting.nextSkill = slot;
    } else {
      c.targeting.nextSkill = NEXTSKILL_NONE;
    }
  }

  /**
   * `CMover::CMD_SetUseSkill(idTarget, slot, sut)` (client): `false` = nothing was started. Writes the `USESKILL` command when
   * every precondition holds.
   */
  cmdSetUseSkill(target: number, slot: number, sut: number): boolean {
    const c = this.c;
    c.commands.localCmd = 0; // `m_oaCmd = OBJACT_NONE`
    const self = c.deps.self();
    if (self === null) return false;
    const st = self.state;
    if (
      self.flying ||
      this.actAttack(st) ||
      (st & STA.JUMP_ALL) !== 0 ||
      (st & STA.DMG_FLY_ALL) !== 0 ||
      self.dead
    ) {
      return false;
    }
    const skill = c.deps.skills?.skill(slot) ?? null;
    if (skill === null) return false;
    const hooks = c.deps.hooks;
    if (hooks?.baseMotionActive?.() === true) {
      hooks.cancelBaseMotion?.();
      return false;
    }
    let to = target;
    switch (skill.useChance) {
      case WUI_NOW:
        to = self.id;
        break;
      case WUI_TARGETINGOBJ: {
        const f = c.targeting.focus;
        if (f !== null && c.deps.entityInfo(f)?.kind === 'mover') to = f;
        break;
      }
      case WUI_TARGETCURSORPTZ:
        to = self.id;
        if (!this.blinkTarget(self.pos, skill)) {
          c.targeting.nextSkill = NEXTSKILL_NONE;
          return false;
        }
        break;
      default:
        break;
    }
    if (to !== self.id && to !== NULL_ID) {
      const o = c.deps.entityInfo(to);
      if (o !== null && o.kind === 'mover') {
        if (skill.evildoing < 0 && !c.attack.attackable(to)) return false;
        if (skill.skillId === SI_ASS_HEAL_RESURRECTION) {
          if (!o.isPlayer || !o.dead) return false;
        } else if (o.dead) {
          return false;
        }
      }
    }
    // the way to the target by the execution kind
    switch (skill.exeTarget) {
      case EXT_SELFCHGPARAMET:
      case EXT_AROUNDATK:
        to = self.id;
        break;
      case EXT_ANOTHERWITH:
        if (to === NULL_ID) to = self.id;
        break;
      default:
        // EXT_MAGICATKSHOT / MAGICATK / MAGICSHOT / OBJCHGPARAMET and the rest need a target
        if (to === NULL_ID) return false;
        break;
    }
    return c.commands.useSkill(to, slot, sut, skill.attackRange);
  }

  /**
   * The ground point of a Blinkpool (`WUI_TARGETCURSORPTZ`): the cursor's ground point clamped to the skill range and walked back
   * over blocked cells (`blink_point`, 31 section 10.2). `false` = no point (`TID_GAME_NOMOVING`, the reservation ends).
   */
  private blinkTarget(self: Vec3LH, skill: SkillInfo): boolean {
    const c = this.c;
    const pick = c.pickGround?.();
    if (pick === undefined || pick === null) {
      // no ground under the cursor (or no query slot: the original has no retry either)
      c.notify(TID_GAME_NOMOVING);
      return false;
    }
    const p = pick.pos;
    // `SetAngle(GetDegree(vPos, GetPos()))`: face the point (LOCAL_FACE angle)
    const dx = p[0] - self[0];
    const dz = p[2] - self[2];
    c.deps.sim.cmd.localFace(headingDegrees(dx, dz));
    const tele = c.deps.sim.blinkPoint?.(self, p, skill.skillRange);
    c.deps.skills?.setTelePos(tele ? [tele.pos[0], tele.pos[1], tele.pos[2]] : [p[0], p[1], p[2]]);
    return true;
  }

  // ---- the arrival (§7.10 "on CmdArrived{USESKILL}") ------------------------------------------------------------------------------

  /** `losBlocked(from, to)`: `undefined` = no query slot. */
  private lineBlocked(from: Vec3LH, to: Vec3LH): boolean | undefined {
    const dx = to[0] - from[0];
    const dy = to[1] - from[1];
    const dz = to[2] - from[2];
    const len = Math.hypot(dx, dy, dz);
    if (len < 1e-6) return false;
    const hit = this.c.deps.sim.rayCast(from, [dx / len, dy / len, dz / len], len, LOS_MASK);
    if (hit === undefined) return undefined;
    return hit !== null;
  }

  /** `CmdArrived{USESKILL, target}`: the sim stopped in the range of the skill. */
  onCmdArrived(target: number): void {
    const c = this.c;
    const cm = c.commands;
    const slot = cm.cmdSlot;
    const sut = cm.cmdSut;
    cm.onDestCleared();
    const o = c.deps.entityInfo(target);
    if (o?.kind !== 'mover') return;
    const now = c.deps.now();
    if (!(this.skillTimerStop || now - this.skillTimerAt >= SKILL_TIMER_MS)) {
      this.stats.timerGated += 1; // a gated arrival drops the command (KEEP)
      return;
    }
    this.submit(this.arrivalOp(target, slot, sut));
  }

  private arrivalOp(target: number, slot: number, sut: number): Op {
    const c = this.c;
    type Stage = 'los' | 'send' | 'back' | 'stop';
    let stage: Stage = 'los';
    let tele: Vec3LH | null = null;
    let teleChecked = false;
    return {
      step: () => {
        const self = c.deps.self();
        const o = c.deps.entityInfo(target);
        if (self === null || o?.pos == null) return 'done';
        if (stage === 'los') {
          if (this.budget() < 1) return 'retry';
          const a: Vec3LH = [self.pos[0], self.pos[1] + LOS_LIFT, self.pos[2]];
          const b: Vec3LH = [o.pos[0], o.pos[1] + LOS_LIFT, o.pos[2]];
          const blocked = target === self.id ? false : this.lineBlocked(a, b);
          if (blocked === undefined) return 'retry';
          if (blocked) {
            this.blockNotice(TID_GAME_BLOCKTARGETING);
            c.targeting.nextSkill = NEXTSKILL_NONE;
            return 'done';
          }
          stage = (c.reqFlags & REQ_USESKILL) === 0 ? 'send' : 'stop';
        }
        const skill = c.deps.skills?.skill(slot) ?? null;
        if (stage === 'send') {
          if (skill === null) return 'done';
          if (skill.skillId === SI_MAG_MAG_BLINKPOOL && !teleChecked) {
            tele = c.deps.skills?.telePos() ?? null;
            if (tele !== null) {
              if (this.budget() < 1) return 'retry';
              const from: Vec3LH = [self.pos[0], self.pos[1] + 1, self.pos[2]];
              const blocked = this.lineBlocked(from, tele);
              if (blocked === undefined) return 'retry';
              if (blocked) {
                c.targeting.nextSkill = NEXTSKILL_NONE;
                this.blockNotice(TID_GAME_BLOCKTARGETING);
                return 'done';
              }
              c.deps.render()?.moveMark?.(tele, null);
            }
            teleChecked = true;
          }
          if (skill.atkStyle === AS_BACK) {
            if (this.budget() < 1) return 'retry';
            const bits = c.deps.sim.targetCheck?.(target, 0, 0);
            if (bits === undefined) return 'retry';
            if ((bits & TARGET_CHECK.AS_BACK_OK) === 0) {
              this.blockNotice(TID_GAME_NEVERKILLSTOP);
              return 'done';
            }
          }
          // a good skill on another chaotic player needs Ctrl while the PK event is on
          if (
            skill.evildoing > 0 &&
            c.deps.hooks?.chaoticPlayer?.(target) === true &&
            !this.ctrlHeld()
          ) {
            this.stats.blocked += 1;
            return 'done';
          }
          // legacy-send: SendUseSkill
          c.deps.send?.({
            case: 'useSkillReq',
            value: create(UseSkillReqSchema, {
              type: 0,
              slot,
              target,
              useType: sut,
              control: this.ctrlHeld(),
            }),
          });
          this.stats.sent += 1;
          c.reqFlags |= REQ_USESKILL;
          stage = 'stop';
        }
        // `ClearDestObj(); SendActMsg(OBJMSG_STOP); if (!m_SkillTimerStop) m_SkillTimer.Reset()`
        if (this.budget() < 1) return 'retry';
        c.deps.sim.cmd.localClear(LOCAL_CLEAR.DEST | LOCAL_CLEAR.DEST_OBJ);
        c.commands.destObj = 0;
        if (c.deps.sim.localAct?.(OBJMSG.STOP, [0, 0, 0, 0, 0]) === undefined) return 'retry';
        if (!this.skillTimerStop) this.skillTimerAt = c.deps.now();
        return 'done';
      },
    };
  }

  private blockNotice(tid: number): void {
    this.stats.blocked += 1;
    this.c.notify(tid);
  }

  // ---- the answers of the server (Plan/17 §11.1) -------------------------------------------------------------------------------

  /** `UseSkillNtf` / `ClearUseSkill` of the own mover: the request is answered (`commands.clearReqUseSkill`). */
  clearReqUseSkill(): void {
    this.c.reqFlags &= ~REQ_USESKILL;
  }

  // ---- the action slot (§7.12) -----------------------------------------------------------------------------------------------------

  /** `KeyC` key-up on the ground: the queue starts on the focused mover, or on nothing when nothing is focused. */
  onActionSlotKey(): void {
    const c = this.c;
    const self = c.deps.self();
    if (self === null || self.flying) return;
    const f = c.targeting.focus;
    if (f === null || c.deps.entityInfo(f)?.kind === 'mover') {
      c.targeting.nextSkill = NEXTSKILL_ACTIONSLOT;
    }
  }

  /** `CWndWorld::UseSkillToFocusObj`: the reserved queue start of the world tick. */
  useSkillToFocus(focus: number | null): boolean {
    const c = this.c;
    const ok = this.useSkillQueue(focus);
    if (!ok) return false;
    const hand = c.deps.hand?.() ?? null;
    if (hand !== null) {
      const first = c.deps.actionSlot?.queue()[0];
      const skill = first === undefined ? null : (c.deps.skills?.skill(first) ?? null);
      if (
        hand.kind3 !== IK3_STAFF &&
        hand.kind3 !== IK3_CHEERSTICK &&
        skill !== null &&
        skill.exeTarget !== EXT_SELFCHGPARAMET &&
        c.deps.options().autoAttack
      ) {
        c.targeting.autoAttack = true;
      }
    }
    return true;
  }

  /** `CWndTaskBar::UseSkillQueue(target)`: `SUT_QUEUESTART` of the first queue entry. */
  useSkillQueue(t: number | null): boolean {
    const c = this.c;
    const slotPort = c.deps.actionSlot;
    const self = c.deps.self();
    if (slotPort === undefined || self === null) return false;
    if (slotPort.executing() === 2) return false;
    if (this.actAttack(self.state) || (self.state & STA.JUMP_ALL) !== 0 || self.flying) {
      return false;
    }
    if (t !== null && c.deps.entityInfo(t)?.kind !== 'mover') return false;
    const idTarget = t === null || c.deps.entityInfo(t) === null ? NULL_ID : t;
    this.queueTarget = idTarget;
    const first = slotPort.queue()[0];
    if (first === undefined) {
      c.targeting.nextSkill = NEXTSKILL_NONE; // an empty action slot cancels the reservation
      return false;
    }
    const skill = c.deps.skills?.skill(first) ?? null;
    if (skill === null || c.deps.skills?.isBullet(first) === false) return false;
    slotPort.setExecuting(1);
    if (!this.cmdSetUseSkill(idTarget, first, SUT_QUEUESTART)) {
      this.cancel(); // the first skill failed: the queue is not used
    } else {
      this.stats.queueStarts += 1;
      // a staff / cheer stick keeps `ACTIONSLOT`: the queue restarts by itself (KEEP)
      if (!this.staffInHand()) c.targeting.nextSkill = NEXTSKILL_NONE;
    }
    return true;
  }

  /**
   * `CWndTaskBar::OnCancelSkill`: the action slot stops being executed (not while a `UseSkillReq` waits for its answer).
   * `LOCAL_CLEAR{DEST_OBJ}` ends the walk to the queue target.
   */
  cancel(): void {
    const c = this.c;
    if ((c.reqFlags & REQ_USESKILL) !== 0) return;
    c.deps.actionSlot?.setExecuting(0);
    this.queueTarget = NULL_ID;
    c.commands.clearDest();
    c.targeting.nextSkill = NEXTSKILL_NONE;
  }

  /** A left click during the chain (§4.4): `EndSkillQueueReq`. */
  onLeftClick(): void {
    const c = this.c;
    if ((c.deps.actionSlot?.executing() ?? 0) === 0) return;
    this.stats.queueEnds += 1;
    // legacy-send: SendEndSkillQueue
    c.deps.send?.({ case: 'endSkillQueueReq', value: create(EndSkillQueueReqSchema, {}) });
  }

  /** Every world tick: the queue target vanished -> `cancel()` (`CWndTaskBar::Process`). */
  checkQueueTarget(): void {
    const c = this.c;
    if ((c.deps.actionSlot?.executing() ?? 0) === 0) return;
    if (this.queueTarget === NULL_ID) return;
    if (c.deps.entityInfo(this.queueTarget) === null) this.cancel();
  }

  /** Take-off cancels the queue (§9.6). */
  onTakeOff(): void {
    this.cancel();
  }

  /** The world is entered or left. */
  reset(): void {
    this.ops.length = 0;
    this.queueTarget = NULL_ID;
    this.skillTimerAt = Number.NEGATIVE_INFINITY;
    this.skillTimerStop = true;
  }
}

/** `GetDegree(vPos, GetPos())`: the XZ heading from the caster to the point in degrees `[0, 360]` (0 = -Z, clockwise toward +X). */
export function headingDegrees(dx: number, dz: number): number {
  const len = Math.hypot(dx, dz);
  if (len < 1e-9) return 0;
  const dot = Math.max(-1, Math.min(1, -dz / len));
  const deg = (Math.acos(dot) * 180) / Math.PI;
  return dx < 0 ? 360 - deg : deg;
}
