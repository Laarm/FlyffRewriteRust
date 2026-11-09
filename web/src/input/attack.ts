// Attacks of the own character (Plan/43 §7.4, §7.5, §7.7, §7.9; Plan/17 §3.2-§3.4, §11.3; WP-INPUT-10, WP-INPUT-11): the client side
// of the melee, wand and bow attack. The world tick decides WHEN (`useFocusObj`, the charge release and auto-fire); this module
// walks the rest of the way:
//
//   useFocusObj(obj)      the command for the focused object: melee / wand / bow, stand up from a seat, pick up an item
//   CmdArrived{cmd}       the sim reached the attack range: the swing starts locally (`local_act`) and the request is sent
//   LocalAttack{msg,...}  the sim chained the next combo step at the end frame of ATKn: `MeleeAttackReq{ATKn+1}`
//   CmdAborted            a ranger-roller shot from beyond the item range: the command ends
//
// Every `local_act` and LOS `ray_cast` is a query of the bridge, capped at 2 per loop iteration (Plan/40 §7.3). An invocation that
// needs k calls runs only when k slots are left; otherwise it waits whole in a FIFO that `pump()` (the start of the next
// iteration) runs before anything new. A call that answers `undefined` is asked again the next iteration. The `local_act` calls of
// one invocation are never split across iterations, except when a slot was taken by a caller outside this module (then the rest
// of the invocation follows; both `STOP` and the attack message are idempotent).

import { create } from '@bufbuild/protobuf';
import { IK3_BOW, IK3_WAND, IK3_YOYO } from '../gen/defs/itemkind';
import { TID_GAME_BLOCKTARGETING } from '../gen/defs/text';
import {
  MagicAttackReqSchema,
  MeleeAttackReqSchema,
  RangeAttackReqSchema,
} from '../gen/proto/flyff/v1/combat_pb';
import { MotionReqSchema } from '../gen/proto/flyff/v1/movement_pb';
import { OBJACT, OBJMSG, STA } from '../game/combat/constants';
import { useMotionOf as weaponMotion } from '../game/combat/attackMap';
import type { Vec3LH } from '../render/types';
import { evaluateAttackable } from './attackable';
import { OBJACT_NONE } from './commands';
import { RAY_CTRL, RAY_STATIC } from './picking';
import type { HandItem } from './ports';
import { MODE_DONMOVE, REQ_USESKILL, type InputCore } from './worldTick';

/** The LOS ray of the client: static meshes and ctrl colliders, never the terrain (`IntersectObjLine` without terrain). */
export const LOS_MASK = RAY_STATIC | RAY_CTRL;
/** The test ray runs at +0.5 m above both positions. */
export const LOS_LIFT = 0.5;
/** `Q` calls per iteration. */
export const Q_CAP = 2;

/** One deferrable invocation: `step()` returns `done` when it finished (or gave up) and `retry` to run again next iteration. */
interface Op {
  readonly name: string;
  step(): 'done' | 'retry';
}

export interface AttackStats {
  meleeSent: number;
  magicSent: number;
  rangeSent: number;
  comboSent: number;
  losBlocked: number;
  deferred: number;
  retried: number;
  standUps: number;
}

export class AttackController {
  private readonly ops: Op[] = [];
  readonly stats: AttackStats = {
    meleeSent: 0,
    magicSent: 0,
    rangeSent: 0,
    comboSent: 0,
    losBlocked: 0,
    deferred: 0,
    retried: 0,
    standUps: 0,
  };

  constructor(private readonly c: InputCore) {}

  // ---- the query budget ------------------------------------------------------------------------------------------------------

  private budget(): number {
    return this.c.deps.sim.queryBudget?.() ?? Q_CAP;
  }

  /** Runs `op` at once when nothing waits ahead of it, else (or when it is not finished) queues it. */
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
        this.stats.retried += op === undefined ? 0 : 1;
        return;
      }
      this.ops.shift();
    }
  }

  /** Invocations waiting for a query slot (the probe and the tests read it). */
  get pending(): number {
    return this.ops.length;
  }

  /** Forgets every waiting invocation (world change, death). */
  clear(): void {
    this.ops.length = 0;
  }

  // ---- attackability (Plan/43 §7.6) ---------------------------------------------------------------------------------------------

  /** `targeting.attackable(obj)` with its side effects: a refusal ends the auto-attack, a dead NPC drops the focus. */
  attackable(id: number): boolean {
    const c = this.c;
    const hooks = c.deps.hooks;
    const r = evaluateAttackable(
      {
        self: () => c.deps.self(),
        info: (i) => c.deps.entityInfo(i),
        textDisguised: () => hooks?.textDisguised?.() ?? false,
        player: (i) => hooks?.playerAttackable?.(i) ?? 0,
        baseMotion: () => ({
          active: hooks?.baseMotionActive?.() ?? false,
          cancel: () => {
            hooks?.cancelBaseMotion?.();
          },
        }),
      },
      id,
    );
    if (r.dropFocus) c.targeting.setFocus(null);
    if (!r.ok) c.targeting.autoAttack = false;
    return r.ok;
  }

  /**
   * `DoAttackMagic(target, power)` without the walk (the flying wand, §9.5): the line of sight, then `local_act(ATK_MAGIC1)` and
   * `MagicAttackReq` as the arrival of a ground wand command would.
   */
  doAttackMagic(target: number, power = 0): void {
    this.submit(this.shotOp('magic', target, power));
  }

  // ---- useFocusObj (§7.4) ----------------------------------------------------------------------------------------------------

  /**
   * `useFocusObj(obj)`: the command that follows a held click or the auto-attack. Returns whether the object was handled
   * (a refusal of the state returns `false`).
   */
  useFocusObj(id: number): boolean {
    const c = this.c;
    const self = c.deps.self();
    const o = c.deps.entityInfo(id);
    if (self === null || o === null) return false;
    const state = self.state;
    const actJump = (state & STA.JUMP_ALL) !== 0;
    const actAttack = (state & STA.ATK_ALL) !== 0;
    if (
      self.flying ||
      actJump ||
      actAttack ||
      self.dead ||
      (self.modeFlags & MODE_DONMOVE) !== 0 ||
      self.settingStone
    ) {
      return false;
    }
    const m = c.mouse;
    if (o.kind === 'item' || o.kind === 'ctrl') {
      if (m.L.down && !m.L.downed) c.commands.useItem(id);
      m.L.downed = m.L.down;
      return true;
    }
    const hand = c.deps.hand?.() ?? null;
    const wandAuto = c.deps.features()?.wand_autoattacks !== false;
    if (hand?.kind3 === IK3_WAND && wandAuto) {
      c.commands.magicAttack(id, 0);
    } else if (self.sitting === true) {
      const actDamage = (state & STA.DMG_ALL) !== 0;
      const action = (state & STA.ACTION_ALL) !== 0;
      // (the jump and attack states returned above)
      if (!actDamage && !action && (c.reqFlags & REQ_USESKILL) === 0) this.standUp();
    } else if (hand?.kind3 === IK3_BOW) {
      if (!m.L.down) c.commands.rangeAttack(id, 0); // a held click charges instead (§7.7)
    } else {
      c.commands.meleeAttack(id);
    }
    return true;
  }

  /** `MotionReq{OBJMSG_STANDUP}`: a sitting attacker stands first. */
  private standUp(): void {
    this.stats.standUps += 1;
    this.c.deps.send?.({
      case: 'motionReq',
      value: create(MotionReqSchema, { objMsg: OBJMSG.STANDUP }),
    });
  }

  // ---- sim events (§7.9) -----------------------------------------------------------------------------------------------------

  /** `CmdArrived{cmd, target}`: the sim stopped in front of the target (31 §11.7). */
  onCmdArrived(cmd: number, target: number): void {
    const cm = this.c.commands;
    const power = cm.cmdPower;
    cm.onDestCleared();
    cm.cmdPower = 0;
    switch (cmd) {
      case OBJACT.MELEE_ATTACK:
        this.submit(this.meleeOp(target));
        break;
      case OBJACT.MAGIC_ATTACK:
        this.submit(this.shotOp('magic', target, power));
        break;
      case OBJACT.RANGE_ATTACK:
        this.submit(this.shotOp('range', target, power));
        break;
      default:
        break; // USESKILL (WP-INPUT-13), USEITEM: the sim already stood and faced the target
    }
  }

  /** `CmdAborted`: the shot was out of the item's range (ranger roller): the command is over. */
  onCmdAborted(): void {
    this.c.commands.localCmd = OBJACT_NONE;
    this.c.commands.cmdPower = 0;
  }

  /** `LocalAttack{msg, target, flags}`: the next combo step started locally; the request follows in the same frame. */
  onLocalAttack(msg: number, target: number, flags: number): void {
    this.stats.comboSent += 1;
    // legacy-send: SendMeleeAttack, SendMeleeAttack2
    this.c.deps.send?.({
      case: 'meleeAttackReq',
      value: create(MeleeAttackReqSchema, {
        atkMsg: msg,
        target,
        param2: 0,
        param3: (flags << 16) | 0,
      }),
    });
  }

  // ---- the invocations ---------------------------------------------------------------------------------------------------------

  private alive(target: number): boolean {
    const self = this.c.deps.self();
    const o = this.c.deps.entityInfo(target);
    return self !== null && !self.dead && o !== null && o.kind === 'mover' && !o.dead;
  }

  /** `losBlocked(self + 0.5y, target + 0.5y)`: `undefined` = no query slot. */
  private los(target: number): boolean | undefined {
    const c = this.c;
    const self = c.deps.self();
    const o = c.deps.entityInfo(target);
    const to = o?.pos;
    if (self === null || !to) return false;
    const a: Vec3LH = [self.pos[0], self.pos[1] + LOS_LIFT, self.pos[2]];
    const b: Vec3LH = [to[0], to[1] + LOS_LIFT, to[2]];
    const dx = b[0] - a[0];
    const dy = b[1] - a[1];
    const dz = b[2] - a[2];
    const len = Math.hypot(dx, dy, dz);
    if (len < 1e-6) return false;
    const hit = c.deps.sim.rayCast(a, [dx / len, dy / len, dz / len], len, LOS_MASK);
    if (hit === undefined) return undefined;
    return hit !== null;
  }

  private blockedNotice(): void {
    this.stats.losBlocked += 1;
    this.c.notify(TID_GAME_BLOCKTARGETING);
  }

  /** `doAttackMelee(target)`: a yo-yo checks the line of sight first; `STOP`, then `ATK1`; `r > 0` sends the request. */
  private meleeOp(target: number): Op {
    const c = this.c;
    const hand = (): HandItem | null => c.deps.hand?.() ?? null;
    let stage: 'los' | 'act' = hand()?.kind3 === IK3_YOYO ? 'los' : 'act';
    let stopped = false;
    return {
      name: 'melee',
      step: () => {
        if (!this.alive(target) || c.deps.sim.localAct === undefined) return 'done';
        if (stage === 'los') {
          if (this.budget() < 1) return 'retry';
          const blocked = this.los(target);
          if (blocked === undefined) return 'retry';
          if (blocked) {
            this.blockedNotice();
            return 'done';
          }
          stage = 'act';
        }
        if (this.budget() < (stopped ? 1 : 2)) return 'retry';
        if (!stopped) {
          if (c.deps.sim.localAct(OBJMSG.STOP, [0, 0, 0, 0, 0]) === undefined) return 'retry';
          stopped = true;
        }
        const r = c.deps.sim.localAct(OBJMSG.ATK1, [target | 0, 0, 0, 0, 0]);
        if (r === undefined) return 'retry';
        if (r > 0) {
          this.stats.meleeSent += 1;
          // legacy-send: SendMeleeAttack, SendMeleeAttack2
          c.deps.send?.({
            case: 'meleeAttackReq',
            value: create(MeleeAttackReqSchema, {
              atkMsg: OBJMSG.ATK1,
              target,
              param2: 0,
              param3: (r << 16) | 0,
            }),
          });
          c.deps.hooks?.localAttackStarted?.();
        }
        return 'done';
      },
    };
  }

  /**
   * `doAttackMagic` / `doAttackRange` (§7.9): the line of sight, then `local_act(ATK_MAGIC1 / ATK_RANGE1)`; a started attack
   * (`r == 1`) sends the request with the charge power as computed (0..4).
   */
  private shotOp(kind: 'magic' | 'range', target: number, power: number): Op {
    const c = this.c;
    let stage: 'los' | 'act' = 'los';
    return {
      name: kind,
      step: () => {
        if (!this.alive(target) || c.deps.sim.localAct === undefined) return 'done';
        if (stage === 'los') {
          if (this.budget() < 1) return 'retry';
          const blocked = this.los(target);
          if (blocked === undefined) return 'retry';
          if (blocked) {
            this.blockedNotice();
            return 'done';
          }
          stage = 'act';
        }
        if (this.budget() < 1) return 'retry';
        let r: number | undefined;
        if (kind === 'magic') {
          r = c.deps.sim.localAct(OBJMSG.ATK_MAGIC1, [target | 0, 0, 0, 0, 0]);
        } else {
          const hand = c.deps.hand?.() ?? null;
          const motion = weaponMotion({ use_motion: hand?.useMotion ?? -1 }, true);
          r = c.deps.sim.localAct(OBJMSG.ATK_RANGE1, [target | 0, motion, 0, 0, 0]);
        }
        if (r === undefined) return 'retry';
        if (r === 1) {
          if (kind === 'magic') {
            this.stats.magicSent += 1;
            // legacy-send: SendMagicAttack
            c.deps.send?.({
              case: 'magicAttackReq',
              value: create(MagicAttackReqSchema, {
                atkMsg: OBJMSG.ATK_MAGIC1,
                target,
                param2: 0,
                param3: 0,
                magicPower: power,
              }),
            });
            if (power >= 1) c.deps.hooks?.actionPointUp?.();
          } else {
            this.stats.rangeSent += 1;
            // legacy-send: SendRangeAttack
            c.deps.send?.({
              case: 'rangeAttackReq',
              value: create(RangeAttackReqSchema, {
                atkMsg: OBJMSG.ATK_RANGE1,
                target,
                itemId: power,
              }),
            });
          }
        }
        return 'done';
      },
    };
  }
}
