// The boss movers of the M7 client effects and sounds (Plan/41 section 17.1, 44 section 9.1 T11; `MoverActEvent.cpp:1647-1695`): which mover index is which boss handler, and the
// attack states the handlers branch on. Pure constants, shared by the router (`boss.ts`), the renderer (`render/sfx/bossFx.ts`) and the audio cue (`audio/cues.ts`) - none of them may import
// another of the three (layering), so the table lives here.

import {
  MI_BIGMUSCLE,
  MI_CLOCKWORK1,
  MI_DU_METEONYKER,
  MI_DU_METEONYKER2,
  MI_DU_METEONYKER3,
  MI_DU_METEONYKER4,
  MI_KRRR,
  MI_LOADCLOCKWORK,
  MI_MUSHMOOT,
} from '../../gen/defs/obj';

/** `OBJSTA_ATK1..4` (the sub-state of a non-player melee swing, flyff-sim `types.rs`). */
export const OBJSTA_ATK1 = 0x0001_0000;
export const OBJSTA_ATK2 = 0x0002_0000;
export const OBJSTA_ATK3 = 0x0003_0000;
export const OBJSTA_ATK4 = 0x0004_0000;
/** `OBJSTA_ATK_ALL`. */
export const OBJSTA_ATK_ALL = 0x00ff_0000;

/** The boss handler a mover index belongs to (`CMover::OnAttackMelee` dispatch). */
export type BossKind = 'clockworks' | 'bigmuscle' | 'krrr' | 'mushmoot' | 'meteonyker';

/** The Meteonyker ids (`__Y_DRAGON_FIRE`): red, blue, green, yellow. */
export const METEONYKER_IDS: readonly number[] = [
  MI_DU_METEONYKER,
  MI_DU_METEONYKER2,
  MI_DU_METEONYKER3,
  MI_DU_METEONYKER4,
];

/**
 * The boss handler of a mover index, `undefined` for every other mover. `MI_LOADCLOCKWORK` is a ClockWorks only with `__BETTER_CLOCKWORK_ALPHA` (`features.better_clockwork_alpha`).
 */
export function bossKindOf(propId: number, betterClockworkAlpha: boolean): BossKind | undefined {
  switch (propId) {
    case MI_CLOCKWORK1:
      return 'clockworks';
    case MI_LOADCLOCKWORK:
      return betterClockworkAlpha ? 'clockworks' : undefined;
    case MI_BIGMUSCLE:
      return 'bigmuscle';
    case MI_KRRR:
      return 'krrr';
    case MI_MUSHMOOT:
      return 'mushmoot';
    case MI_DU_METEONYKER:
    case MI_DU_METEONYKER2:
    case MI_DU_METEONYKER3:
    case MI_DU_METEONYKER4:
      return 'meteonyker';
    default:
      return undefined;
  }
}

/** The models whose records the renderer must pose with event points (`want_attach`): every boss of the handlers above, `MI_LOADCLOCKWORK` included (the pose costs nothing when it never fights). */
export const BOSS_MODEL_IDS: readonly number[] = [
  MI_CLOCKWORK1,
  MI_LOADCLOCKWORK,
  MI_BIGMUSCLE,
  MI_KRRR,
  MI_MUSHMOOT,
  ...METEONYKER_IDS,
];
