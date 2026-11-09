// The local command layer (Plan/43 §7.8; WP-INPUT-07 destination commands, WP-INPUT-10 the attack and item commands): the client
// half of `CMover::CMD_*`. Every command writes a record of the sim command buffer (ops 0x19-0x1C, Plan/31 §17.4); the sim
// applies it at the start of the next stepped frame, walks to the target and reports `CmdArrived{cmd, target}` when the attack
// range is reached (`input/attack.ts` then starts the swing). The destination commands report `DEST_POS` / `DEST_OBJ`, which
// doc 40 turns into `DestPosReq` / `SetDestObjReq` (nothing is sent from here, `__TRAFIC_1222`: an unchanged destination
// reports nothing).
//
//   meleeAttack(id)      LOCAL_CMD{MELEE_ATTACK, id, range 0}; a yo-yo derives its range from the item (`range -1`, `ar`)
//   rangeAttack(id, p)   LOCAL_CMD{RANGE_ATTACK, id, derive, ar = bow range, p = [id, power]}
//   magicAttack(id, p)   LOCAL_CMD{MAGIC_ATTACK, id, derive, p = [id, power]}
//   useItem(obj)         LOCAL_CMD{USEITEM, obj, derive}: walks to a ground item or a chest; the server picks up on arrival
//
//   useSkill(t, s, sut)  LOCAL_CMD{USESKILL, t, derive, ar = the skill's attack range, p = [slot, t, sut]}: the sim walks into the
//                        range of the skill and reports `CmdArrived{USESKILL}`; `input/skill.ts` decides the rest (WP-INPUT-13)

import { IK2_TOOLS, IK3_BOW, IK3_WAND, IK3_YOYO } from '../gen/defs/itemkind';
import { LOCAL_CLEAR as WIRE_CLEAR } from '../gen/sim/wire';
import type { Vec3 } from '../game/simCmd';
import type { HandItem, SimPort } from './ports';

/** `OBJACT_*` used here (`flyff_sim::types`). */
export const OBJACT_NONE = 0;
export const OBJACT_MELEE_ATTACK = 1;
export const OBJACT_MAGIC_ATTACK = 2;
export const OBJACT_RANGE_ATTACK = 3;
export const OBJACT_USESKILL = 5;
export const OBJACT_USEITEM = 6;

/** `LOCAL_CMD.flags`: report `BEHAVIOR` before the destination (`__BS_ADJUST_SYNC`). */
export const LOCAL_CMD_BEHAVIOR_FIRST = 1;

/** `LOCAL_CLEAR` bits. */
export const LOCAL_CLEAR = WIRE_CLEAR;

/** What the commands read about the own character. */
export interface CommandSelf {
  readonly dead: boolean;
  /** The ATK group of the state word is not empty. */
  readonly actAttack: boolean;
}

export interface CommandContext {
  readonly sim: Pick<SimPort, 'cmd'>;
  /** `true` when the object is another player (the destination switch reports `BEHAVIOR` first). */
  isPlayer(id: number): boolean;
  /** The item in the active hand (`null`: bare hands). Absent = bare hands. */
  hand?(): HandItem | null;
  /** The own character's combat state. Absent = ready. */
  self?(): CommandSelf | null;
  /** An arrow is in the bullet slot, or `features.no_arrows_poster` (the check of `IsBullet`). */
  bullet?(): boolean;
}

export class LocalCommands {
  /** The destination object the sim holds (`0` = none), mirrored for the `SetDestObj` rule. */
  destObj = 0;
  /** `localCmd`: the TS mirror of the sim's active command (`OBJACT_*`). */
  localCmd = OBJACT_NONE;
  /** The charge power of the pending bow / wand command (`CmdArrived` does not carry it). */
  cmdPower = 0;
  /** The target of the pending command. */
  cmdTarget = 0;
  /** The job-skill slot and the use type (`SKILLUSETYPE`) of the pending skill command (`GetCmdParam(0)`, `GetCmdParam(2)`). */
  cmdSlot = -1;
  cmdSut = 0;

  constructor(private readonly c: CommandContext) {}

  /**
   * `SetDestObj(id, range)` (`CMD_SetDestObj`, bSend TRUE): walk to an object. When the previous destination was a PLAYER the
   * sim reports `BEHAVIOR` first (`__BS_ADJUST_SYNC`, `MoverMsg.cpp:49-57`). `range` 0 = arrive next to it. The
   * destination the sim holds is mirrored in `destObj` (cleared by `onDestCleared`); an unchanged target writes nothing.
   */
  setDestObj(id: number, range: number): void {
    if (id === this.destObj) return;
    const flags =
      this.destObj !== 0 && this.c.isPlayer(this.destObj) ? LOCAL_CMD_BEHAVIOR_FIRST : 0;
    this.c.sim.cmd.localCmd(OBJACT_NONE, id, range, 0, [0, 0, 0], flags);
    this.destObj = id;
    this.localCmd = OBJACT_NONE;
  }

  /** `SetDestPos(pos, forward)`: click-to-move (`LOCAL_DEST_POS`; `forward` is forced by the sim, `__BS_ADJUST_SYNC`). */
  setDestPos(pos: Vec3, forward = true): void {
    this.c.sim.cmd.localDestPos(pos, forward);
    this.destObj = 0;
    this.localCmd = OBJACT_NONE;
  }

  /** `ClearDest()`. */
  clearDest(): void {
    this.c.sim.cmd.localClear(LOCAL_CLEAR.DEST | LOCAL_CLEAR.DEST_OBJ);
    this.destObj = 0;
  }

  /** `ClearCmd()`: the pending command (a movement key or a click on the ground ends it). */
  clearCmd(): void {
    if (this.localCmd === OBJACT_NONE && this.destObj === 0) return;
    this.c.sim.cmd.localClear(LOCAL_CLEAR.CMD);
    this.localCmd = OBJACT_NONE;
  }

  /** `mvrf &= !MVRF_SKILL` (a left click cancels a reserved skill flag). */
  clearSkillFlag(): void {
    this.c.sim.cmd.localClear(LOCAL_CLEAR.SKILL_FLAG);
  }

  /** `SET_TRACKING 0` (the Esc chain stops the flight auto-track). */
  stopTracking(): void {
    this.c.sim.cmd.setTracking(0);
  }

  /** The sim cleared the destination (arrival, `CmdArrived`, a movement key): forget the mirror. */
  onDestCleared(): void {
    this.destObj = 0;
    this.localCmd = OBJACT_NONE;
  }

  // ---- the attack and item commands (WP-INPUT-10 / -11) ----------------------------------------------------------------------

  private blocked(): boolean {
    const s = this.c.self?.();
    return s !== null && s !== undefined && (s.dead || s.actAttack);
  }

  private set(act: number, target: number): void {
    this.localCmd = act;
    this.cmdTarget = target;
    this.destObj = target;
  }

  /**
   * `CMD_SetMeleeAttack(id, range)`: no tool in the hand (`IK2_TOOLS`), not while attacking or dead. A yo-yo has a range of its
   * own (`range -1`: the sim derives `get_attack_range(ar)` from the item); every other weapon walks next to the target.
   */
  meleeAttack(id: number, range = 0): boolean {
    const hand = this.c.hand?.() ?? null;
    if ((hand !== null && hand.kind2 === IK2_TOOLS) || this.blocked()) return false;
    if (hand !== null && hand.kind3 === IK3_YOYO) {
      this.c.sim.cmd.localCmd(OBJACT_MELEE_ATTACK, id, -1, hand.attackRange, [0, 0, 0], 0);
    } else {
      this.c.sim.cmd.localCmd(OBJACT_MELEE_ATTACK, id, range, 0, [0, 0, 0], 0);
    }
    this.set(OBJACT_MELEE_ATTACK, id);
    return true;
  }

  /** `CMD_SetRangeAttack(id, power)`: needs a bow in the hand and arrows (or `no_arrows_poster`). */
  rangeAttack(id: number, power: number): boolean {
    const hand = this.c.hand?.();
    if (hand?.kind3 !== IK3_BOW || this.blocked()) return false;
    if (this.c.bullet?.() === false) return false;
    this.c.sim.cmd.localCmd(OBJACT_RANGE_ATTACK, id, -1, hand.attackRange, [id | 0, power, 0], 0);
    this.set(OBJACT_RANGE_ATTACK, id);
    this.cmdPower = power;
    return true;
  }

  /** `CMD_SetMagicAttack(id, power)`: needs a wand in the hand. */
  magicAttack(id: number, power: number): boolean {
    const hand = this.c.hand?.();
    if (hand?.kind3 !== IK3_WAND || this.blocked()) return false;
    this.c.sim.cmd.localCmd(OBJACT_MAGIC_ATTACK, id, -1, 0, [id | 0, power, 0], 0);
    this.set(OBJACT_MAGIC_ATTACK, id);
    this.cmdPower = power;
    return true;
  }

  /**
   * `CMD_SetUseSkill` (its last step): `LOCAL_CMD{OBJACT_USESKILL}` with the arrival range of the skill (`ar` = its `AR_*` code, the
   * sim derives `get_attack_range(ar)`, `range -1`). The preconditions are `SkillController.cmdSetUseSkill`'s.
   */
  useSkill(target: number, slot: number, sut: number, ar: number): boolean {
    this.c.sim.cmd.localCmd(OBJACT_USESKILL, target, -1, ar, [slot, target | 0, sut], 0);
    this.set(OBJACT_USESKILL, target);
    this.cmdSlot = slot;
    this.cmdSut = sut;
    return true;
  }

  /** `CMD_SetUseItem(obj)`: walk to a ground item or a chest (the server picks up on arrival, 18 §4.5). */
  useItem(id: number): boolean {
    this.c.sim.cmd.localCmd(OBJACT_USEITEM, id, -1, 0, [0, 0, 0], 0);
    this.set(OBJACT_USEITEM, id);
    return true;
  }
}
