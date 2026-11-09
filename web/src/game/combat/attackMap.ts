// The attack notifications as `ACT` records (Plan/17 section 11.1, Plan/31 section 18; WP-COMBAT-10): what the observers'
// sim copies get for `MeleeAttackNtf`, `MagicAttackNtf`, `RangeAttackNtf` and `SpAttack`. The five parameters of an `ACT`
// are the `p1..p5` of `CActionMover::SendActMsg`; the message ids are `OBJMSG_*`. Pure functions, no sim access.

import type { ItemProp } from '../../gen/bundles';
import { DEFAULT_RANGE_MOTION, MTI_ATK1, isNullId, OBJMSG } from './constants';

/** The five parameters of an `ACT` record. */
export type ActParams = readonly [number, number, number, number, number];

/** The motion the server plays for a ranged or special attack: the weapon's `use_motion`, else the default. */
export function useMotionOf(
  weapon: Pick<ItemProp, 'use_motion'> | undefined,
  isPlayer: boolean,
): number {
  const m = weapon?.use_motion;
  if (m !== undefined && !isNullId(m) && m > 0) return m;
  return isPlayer ? DEFAULT_RANGE_MOTION : MTI_ATK1;
}

/** `MeleeAttackNtf{atk_msg, target, param2, param3}`: `SendActMsg(ATKn, target, param2, param3)`. */
export function meleeAct(target: number, param2: number, param3: number): ActParams {
  return [target | 0, param2 | 0, param3 | 0, 0, 0];
}

/**
 * `MagicAttackNtf{target, magic_power, projectile_id}`: `ATK_MAGIC1` starts the wand attack on `target`; the power and the
 * projectile id are what the queue keeps when the mover is busy (`QueuedMagic`).
 */
export function magicAct(target: number, power: number, projectileId: number): ActParams {
  return [target | 0, power | 0, projectileId | 0, 0, 0];
}

/**
 * `RangeAttackNtf{target, item_id, projectile_id}`: `ATK_RANGE1` with the attack motion as `p2`
 * (`do_attack_range` passes the weapon's `use_motion`, Plan/17 section 3.4).
 */
export function rangeAct(target: number, motion: number, projectileId: number): ActParams {
  return [target | 0, motion | 0, projectileId | 0, 0, 0];
}

/** `SpAttack{atk_msg, target, item_id}`: the special attack of a monster, the motion in `p3` (`atk_special`). */
export function specialAct(target: number, itemId: number, motion: number): ActParams {
  return [target | 0, itemId | 0, motion | 0, 0, 0];
}

/** `SendActMsg{obj_msg, p1, p2, p3}` (AI mode changes): the client adds `p4 = 1` (`OnSendActMsg` passes `(msg, p1, p2, p3, 1, 0)`). */
export function sendActAct(p1: number, p2: number, p3: number): ActParams {
  return [p1 | 0, p2 | 0, p3 | 0, 1, 0];
}

/** `true` for the ATK1..ATK4 messages a `MeleeAttackNtf` may carry (the server's VR-004 set). */
export function isComboMsg(msg: number): boolean {
  return msg >= OBJMSG.ATK1 && msg <= OBJMSG.ATK4;
}
