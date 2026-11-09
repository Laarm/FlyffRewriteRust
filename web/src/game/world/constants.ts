// Constants of the movement protocol the world handlers share (Plan/11 section 9, `_Common/MoverMsg.h`, ref/12 section 4).

/** `OBJSTAF_*` (`state_flags`). */
export const OBJSTAF = {
  COMBAT: 0x01,
  WALK: 0x02,
  SIT: 0x04,
  FLY: 0x08,
  ACC: 0x10,
  ETC: 0x20,
  ACCTURN: 0x40,
  TURBO: 0x80,
} as const;

/** `OBJSTA_*` groups of the packed `state` word. */
export const OBJSTA = {
  MOVE_MASK: 0x0000_00ff,
  STAND: 0x01,
  SIT: 0x03,
  ATTACK_MASK: 0x00ff_0000,
  /** `OBJSTA_DMG_FLY_ALL` bit test of 18 section 5.7. */
  DMG_FLY_BIT: 0x0200_0000,
  DEAD: 0x0800_0000,
} as const;

/** The world messages carry coordinates in the left-handed game space already (D3D, +Z north). */
export const NULL_ID = 0xffff_ffff;

/** `MoverMsg` ids used when the client acts on a mover (`ObjMsg`, Plan/31 4.4). */
export const OBJMSG = { STAND: 3, MOTION: 46, ACC_STOP: 28, STOP_TURN: 10 } as const;
