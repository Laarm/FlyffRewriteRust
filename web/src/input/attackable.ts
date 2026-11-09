// `targeting.attackable(obj)` (Plan/43 §7.6; the client branch of `CMover::IsAttackAble`, `src:_Common/Mover.cpp:6577-6734`;
// WP-INPUT-09). Until the complete `client_attackable` export of the rules context exists (16's WP-MOVER-23, M5: players, duels,
// war, arena, guild combat) this is the NPC-only version of the same predicate with the same inputs and result: NPC targets
// follow the client branch of `is_attackable_npc` (Plan/17 §8.1), players are not attackable. WP-MOVER-23 replaces `attackableNpc`
// by the wasm call; nothing else here changes.
//
//   attackable(o):
//     the own character wears a text disguise                        -> false
//     the own character casts an item (base motion)                  -> cancel the cast (StateModeReq), false
//     a ground item or a ctrl                                        -> true
//     a player                                                       -> `env.player` (the rules' `client_attackable`: a duel partner ...),
//                                                                       false without it
//     an NPC: dead -> the focus is dropped, false; not killable -> false; peaceful -> false; a guard on the other side of the
//             attacker's alignment -> false; the flying state of the two differs -> false
//     a false result ends the auto-attack

import { RANK_GUARD } from '../gen/defs/attribute';
import type { ObjectInfo, SelfInfo } from './ports';

/** What the predicate decides on. */
export interface AttackerView {
  readonly flying: boolean;
  /** `pk_propensity > 0`. */
  readonly chaotic: boolean;
}

/** The NPC branch of `is_attackable_npc` for an attacker that is not disguised (the caller handled the disguise). */
export function attackableNpc(attacker: AttackerView, o: ObjectInfo): boolean {
  if (o.dead) return false;
  if (o.killable === false) return false;
  if (o.peaceful) return false;
  if (o.rank === RANK_GUARD) {
    const chaotic = o.chaotic ?? 0;
    // a guard on the good side (chaotic < 0) protects the chaotic player's victims: it fights the chaotic attacker only
    if (chaotic < 0 && attacker.chaotic) return false;
    if (chaotic > 0 && !attacker.chaotic) return false;
  }
  // a flying monster and a flying player meet only each other (the ground and the air do not mix)
  if ((o.flyingNpc ?? false) !== attacker.flying) return false;
  return true;
}

/** Everything {@link attackable} needs from the outside. */
export interface AttackableEnv {
  self(): SelfInfo | null;
  info(id: number): ObjectInfo | null;
  /** `IK3_TEXT_DISGUISE` buff on the own character. */
  textDisguised(): boolean;
  /** The own character casts an item; `cancel` sends the `StateModeReq` (doc 45/16). */
  baseMotion(): { active: boolean; cancel(): void };
  /** The player branch (`rules.clientAttackable`): `0` no, `1` yes, `2` yes and remember; absent = players are not attackable. */
  player?(id: number): number;
}

/** The result of one {@link attackable} call with the side effects the caller must apply. */
export interface AttackableResult {
  readonly ok: boolean;
  /** The NPC is dead: the focus must be dropped (`IsAttackAbleNPC` client branch). */
  readonly dropFocus: boolean;
}

/** `attackable(obj)` without the targeting side effects; see {@link applyAttackable} for those. */
export function evaluateAttackable(env: AttackableEnv, id: number): AttackableResult {
  const self = env.self();
  if (self === null) return { ok: false, dropFocus: false };
  if (env.textDisguised()) return { ok: false, dropFocus: false };
  const bm = env.baseMotion();
  if (bm.active) {
    bm.cancel();
    return { ok: false, dropFocus: false };
  }
  const o = env.info(id);
  if (o === null) return { ok: false, dropFocus: false };
  if (o.kind !== 'mover') return { ok: true, dropFocus: false };
  if (o.isPlayer) return { ok: (env.player?.(id) ?? 0) !== 0, dropFocus: false };
  const ok = attackableNpc({ flying: self.flying, chaotic: self.chaotic ?? false }, o);
  return { ok, dropFocus: !ok && o.dead };
}
