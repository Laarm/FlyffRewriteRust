// The buff constants of the client (`src:_Common/SkillInfluence.h:4-10`): the type of an entry of the buff set and the sizes.

/** `BUFF_ITEM`: an item effect. */
export const BUFF_ITEM = 0;
/** `BUFF_SKILL`: a skill effect. */
export const BUFF_SKILL = 1;
/** `BUFF_PET`: the system pet's effect. */
export const BUFF_PET = 2;
/** `BUFF_ITEM2`: an item effect whose `level` is the absolute expiry (unix seconds); the wire sends seconds remaining. */
export const BUFF_ITEM2 = 3;
/** `BUFF_EQUIP`: an icon for an equipped item with `DST_GIFTBOX` (no timer). */
export const BUFF_EQUIP = 4;

/** `MAX_SKILLBUFF_COUNT`: the slots of the skill-buff strip (`APP_BUFF_STATUS`). */
export const MAX_SKILLBUFF_COUNT = 28;
/** `MAX_SKILLINFLUENCE`. */
export const MAX_SKILLINFLUENCE = 64;
