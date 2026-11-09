// Numbers of the combat protocol that the client runtime and the input layer share (Plan/17 section 2, `flyff_sim::types`,
// `_AIInterface/ActionMover.h`). They are not in the generated define tables (the `OBJMSG_*` and `AF_*` come from C++ headers
// outside `Resource/`), so `constants.test.ts` pins them against `crates/flyff-sim/src/types.rs` when the repository is there.

import { NULL_ID } from '../world/constants';

/** `OBJMSG_*` (`MoverMsg.h:105-166`; `flyff_sim::types::obj_msgs`). */
export const OBJMSG = {
  NONE: 0,
  FORWARD: 1,
  BACKWARD: 2,
  STAND: 3,
  SITDOWN: 4,
  STANDUP: 5,
  STOP: 6,
  ASTOP: 7,
  LTURN: 8,
  RTURN: 9,
  STOP_TURN: 10,
  PICKUP: 11,
  RESURRECTION: 12,
  COLLECT: 13,
  APPEAR: 14,
  APPEAR2: 15,
  STUN: 16,
  JUMP: 17,
  MODE_COMBAT: 18,
  MODE_PEACE: 19,
  MODE_WALK: 20,
  MODE_RUN: 21,
  MODE_FLY: 22,
  MODE_GROUND: 23,
  MODE_TURBO_ON: 24,
  MODE_TURBO_OFF: 25,
  MODE_TURN: 26,
  ACC_START: 27,
  ACC_STOP: 28,
  ATK1: 29,
  ATK2: 30,
  ATK3: 31,
  ATK4: 32,
  SP_ATK1: 33,
  SP_ATK2: 34,
  ATK_RANGE1: 35,
  ATK_MAGIC1: 36,
  MELEESKILL: 37,
  MAGICSKILL: 38,
  MAGICCASTING: 39,
  DIE: 40,
  DAMAGE: 41,
  DAMAGE_FORCE: 42,
  MOTION: 46,
} as const;

/** `BUFF_*` types of the buff set (`SkillInfluence.h`): the wire's `buff_type`. */
export const BUFF = { ITEM: 0, SKILL: 1, PET: 2, ITEM2: 3, EQUIP: 4 } as const;

/** `OBJACT_*`: the command that runs when a destination object is reached (`flyff_sim::types`). */
export const OBJACT = {
  NONE: 0,
  MELEE_ATTACK: 1,
  MAGIC_ATTACK: 2,
  RANGE_ATTACK: 3,
  SP_ATTACK: 4,
  USESKILL: 5,
  USEITEM: 6,
  COLLECT: 7,
} as const;

/** `AF_*` attack flags (`_AIInterface/ActionMover.h:27-44`). */
export const AF = {
  GENERIC: 0x1,
  MISS: 0x2,
  RESERVED: 0x4,
  MAGIC: 0x8,
  MELEESKILL: 0x10,
  MAGICSKILL: 0x20,
  CRITICAL1: 0x40,
  CRITICAL2: 0x80,
  CRITICAL: 0xc0,
  PUSH: 0x100,
  PARRY: 0x200,
  RESIST: 0x400,
  STUN: 0x800,
  BLOCKING: 0x1000,
  FORCE: 0x2000,
  RANGE: 0x4000,
  MONSTER_SP_CLIENT: 0x8000,
  FLYING: 0x1000_0000,
} as const;

/** `OBJSTA_*` groups of the state word (`flyff_sim::types`), the bit tests of `CAction` (`IsActAttack` and friends). */
export const STA = {
  MOVE_ALL: 0x0000_00ff,
  STAND: 0x1,
  SIT: 0x3,
  JUMP_ALL: 0x0000_f000,
  ATK_ALL: 0x00ff_0000,
  DMG_ALL: 0x0f00_0000,
  DMG_FLY_ALL: 0x0200_0000,
  DIE_ALL: 0x0800_0000,
  ACTION_ALL: 0xf000_0000,
} as const;

/** `OBJSTAF_*` state flags (`flyff_sim::types`). */
export const STAF = { COMBAT: 0x01, WALK: 0x02, SIT: 0x04, FLY: 0x08 } as const;

/** `MTI_STAND_13` and `MTI_ATK_13` (the bow stance block): the default motion of a bow attack is their difference. */
export const MTI_STAND_13 = 2328;
export const MTI_ATK_13 = 2351;
/** `MTI_ATK1`: the motion of a monster's attack and the player default of `do_attack_range` (`MTI_ATK_13 - MTI_STAND_13`). */
export const MTI_ATK1 = 23;
/** The default motion of a bow attack when the weapon has no `use_motion` (Plan/17 section 3.4). */
export const DEFAULT_RANGE_MOTION = MTI_ATK_13 - MTI_STAND_13;

export { NULL_ID };

/** `true` for the `NULL_ID` of an unsigned column or its signed twin. */
export function isNullId(v: number): boolean {
  return v === NULL_ID || v === -1;
}
