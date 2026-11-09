// The behaviour classes of the `CreateSfx` factory that the M3 renderer implements itself (Plan/41 section 17.2, ref/20 section
// 7.2; CreateObj.cpp:33-375, Sfx.cpp:28-640, SfxObj.cpp:77-260). Everything not listed is the base `CSfx`:
//
//   CSfx (default)         follows `idDest` (keeps the last position when it disappears), else `idSrc` (deleted when it disappears);
//                          drawn 0.2 above its position with the object yaw; `nSec`: 0 once, -1 forever, N > 0 loops and ends after N s
//   CSfxGenNormalDmg01     XI_HIT_SWORD01/02, XI_HIT_STICK01/02: plays once where it was created, no lift, yaw 0
//   CSfxGenRestoration01   XI_GEN_RESTORATION01, XI_GEN_LOGIN01, XI_GEN_WARP01, XI_GEN_PC_DIE01: plays once, follows `idSrc`
//                          while it exists (and stays when it does not), no lift, object yaw
//   CSfxGenMoveMark        XI_GEN_MOVEMARK01: the click-to-move mark, a singleton, drawn 0.05 above the point with the 3-axis path
//   CSfxGenWaterCircle...  XI_GEN_WATERCIRCLE01 / WATERCROWN01 (yaw-only, no lift), XI_GEN_RAINCIRCLE01 (3-axis path, no lift)
//   CSfxFixed              the melee / bow / yo-yo skill hit effects: placed at `posDest` while the source exists, no lift, object yaw;
//                          the factory sets the angle from the source's yaw (see `fixedAngle`)
//
// The skill classes (casting auras, hand links, specials, wings, troupe, duel flag, rain) are `skillClasses.ts` / `skillBehaviors.ts` (WP-REND-24); `specOf` asks
// this file's tables first.

import {
  XI_GEN_LOGIN01,
  XI_GEN_MOVEMARK01,
  XI_GEN_PC_DIE01,
  XI_GEN_RAINCIRCLE01,
  XI_GEN_RESTORATION01,
  XI_GEN_WARP01,
  XI_GEN_WATERCIRCLE01,
  XI_GEN_WATERCROWN01,
  XI_HIT_BOW01,
  XI_HIT_STICK01,
  XI_HIT_STICK02,
  XI_HIT_SWORD01,
  XI_HIT_SWORD02,
  XI_HIT_YOY01,
  XI_SKILL_ACR_BOW_AIMEDSHOT03,
  XI_SKILL_ACR_BOW_AUTOSHOT03,
  XI_SKILL_ACR_BOW_JUNKBOW03,
  XI_SKILL_ACR_BOW_SILENTSHOT03,
  XI_SKILL_ACR_YOYO_SLOWSTEP02,
  XI_SKILL_ACR_YOYO_SNITCH02,
  XI_SKILL_BIL_KNU_ASALRAALAIKUM02,
  XI_SKILL_BIL_KNU_BELIALSMESHING02,
  XI_SKILL_BIL_KNU_BLOODFIST02,
  XI_SKILL_BIL_KNU_PIERCINGSERPENT02,
  XI_SKILL_BIL_KNU_SONICHAND02,
  XI_SKILL_BLD_DOUBLEAX_HAWKATTACK03,
  XI_SKILL_BLD_DOUBLESW_BLADEDANCE02,
  XI_SKILL_BLD_DOUBLESW_BLADEDANCE03,
  XI_SKILL_BLD_DOUBLE_ARMORPENETRATE02,
  XI_SKILL_BLD_DOUBLE_CROSSSTRIKE02,
  XI_SKILL_BLD_DOUBLE_SONICBLADE03,
  XI_SKILL_ELE_FIRE_BURINGFIELD02,
  XI_SKILL_JST_YOYO_HITOFPENYA02,
  XI_SKILL_JST_YOYO_VATALSTAB02,
  XI_SKILL_KNT_TWOSW_EARTHDIVIDER02,
  XI_SKILL_KNT_TWO_POWERSWING02,
  XI_SKILL_MAG_FIRE_HOTAIR01,
  XI_SKILL_MER_ONE_REFLEXHIT02,
  XI_SKILL_MER_ONE_SPECIALHIT02,
  XI_SKILL_MER_SHIELD_PANBARRIER02,
  XI_SKILL_MER_SHIELD_PROTECTION02,
  XI_SKILL_PSY_HERO_STONE02,
  XI_SKILL_RAG_BOW_FLAMEARROW03,
  XI_SKILL_RAG_BOW_PIRCINGARROW03,
  XI_SKILL_RAG_BOW_POISONARROW03,
  XI_SKILL_RAG_BOW_SILENTARROW03,
  XI_SKILL_RAG_BOW_TRIPLESHOT03,
  XI_SKILL_RAG_SUP_NATURE02,
  XI_SKILL_RIN_SUP_MERKABAHANZELRUSHA03,
  XI_SKILL_VAG_ONE_BRANDISH01,
  XI_SKILL_VAG_ONE_CLEANHIT01,
  XI_SKILL_VAG_ONE_OVERCUTTER01,
} from '../../gen/defs/obj';
import { BASE_SPEC, type ClassSpec } from './classSpec';
import { skillSpecOf } from './skillClasses';

export { BASE_SPEC, type ClassSpec, type FollowKind } from './classSpec';

const NORMAL_DMG: ClassSpec = {
  follow: 'none',
  lift: 0,
  yaw: false,
  render2: false,
  once: true,
  singleton: false,
  removable: false,
};
const GEN_SRC: ClassSpec = {
  follow: 'src',
  lift: 0,
  yaw: true,
  render2: false,
  once: true,
  singleton: false,
  removable: false,
};
const MOVE_MARK: ClassSpec = {
  follow: 'none',
  lift: 0.05,
  yaw: false,
  render2: true,
  once: true,
  singleton: true,
  removable: false,
};
const WATER: ClassSpec = {
  follow: 'none',
  lift: 0,
  yaw: false,
  render2: false,
  once: true,
  singleton: false,
  removable: false,
};
const RAIN_CIRCLE: ClassSpec = { ...WATER, render2: true };
const FIXED: ClassSpec = {
  follow: 'fixed',
  lift: 0,
  yaw: true,
  render2: false,
  once: false,
  singleton: false,
  removable: false,
};

/** Angle rules of `CSfxFixed` ids: `SetAngle(-srcAngle)`, `SetAngle(-srcAngle + 180)` or `SetAngle(180 - srcAngle)`. */
export type FixedAngle = 'neg' | 'negPlus180' | 'minus180';

const FIXED_ANGLE = new Map<number, FixedAngle>();
for (const id of [
  XI_SKILL_VAG_ONE_OVERCUTTER01,
  XI_SKILL_VAG_ONE_CLEANHIT01,
  XI_SKILL_VAG_ONE_BRANDISH01,
  XI_SKILL_MER_ONE_SPECIALHIT02,
  XI_SKILL_MER_ONE_REFLEXHIT02,
  XI_SKILL_BIL_KNU_BELIALSMESHING02,
  XI_SKILL_BLD_DOUBLE_CROSSSTRIKE02,
  XI_SKILL_BLD_DOUBLE_ARMORPENETRATE02,
  XI_SKILL_BIL_KNU_BLOODFIST02,
  XI_SKILL_BLD_DOUBLE_SONICBLADE03,
  XI_SKILL_BIL_KNU_PIERCINGSERPENT02,
  XI_SKILL_RIN_SUP_MERKABAHANZELRUSHA03,
  XI_SKILL_ELE_FIRE_BURINGFIELD02,
  XI_SKILL_KNT_TWOSW_EARTHDIVIDER02,
]) {
  FIXED_ANGLE.set(id, 'neg');
}
for (const id of [XI_SKILL_BIL_KNU_SONICHAND02, XI_SKILL_BIL_KNU_ASALRAALAIKUM02]) {
  FIXED_ANGLE.set(id, 'negPlus180');
}
for (const id of [
  XI_HIT_YOY01,
  XI_HIT_BOW01,
  XI_SKILL_JST_YOYO_HITOFPENYA02,
  XI_SKILL_JST_YOYO_VATALSTAB02,
  XI_SKILL_ACR_YOYO_SLOWSTEP02,
  XI_SKILL_RAG_BOW_FLAMEARROW03,
  XI_SKILL_RAG_BOW_PIRCINGARROW03,
  XI_SKILL_RAG_BOW_POISONARROW03,
  XI_SKILL_RAG_BOW_SILENTARROW03,
  XI_SKILL_RAG_SUP_NATURE02,
  XI_SKILL_RAG_BOW_TRIPLESHOT03,
  XI_SKILL_ACR_YOYO_SNITCH02,
  XI_SKILL_ACR_BOW_AIMEDSHOT03,
  XI_SKILL_ACR_BOW_JUNKBOW03,
  XI_SKILL_ACR_BOW_SILENTSHOT03,
  XI_SKILL_ACR_BOW_AUTOSHOT03,
  XI_SKILL_BLD_DOUBLEAX_HAWKATTACK03,
  XI_SKILL_BLD_DOUBLESW_BLADEDANCE02,
  XI_SKILL_BLD_DOUBLESW_BLADEDANCE03,
  XI_SKILL_KNT_TWO_POWERSWING02,
]) {
  FIXED_ANGLE.set(id, 'minus180');
}

/** The effects the server's `RemoveSfxObj` marks consume (the three of SfxObj.cpp:191-217 and the hot-air aura of Sfx.cpp:2772). */
export const REMOVABLE_XI: ReadonlySet<number> = new Set([
  XI_SKILL_PSY_HERO_STONE02,
  XI_SKILL_MER_SHIELD_PANBARRIER02,
  XI_SKILL_MER_SHIELD_PROTECTION02,
  XI_SKILL_MAG_FIRE_HOTAIR01,
]);

const SPECS = new Map<number, ClassSpec>();
for (const id of [XI_HIT_SWORD01, XI_HIT_SWORD02, XI_HIT_STICK01, XI_HIT_STICK02]) {
  SPECS.set(id, NORMAL_DMG);
}
for (const id of [XI_GEN_RESTORATION01, XI_GEN_LOGIN01, XI_GEN_WARP01, XI_GEN_PC_DIE01]) {
  SPECS.set(id, GEN_SRC);
}
SPECS.set(XI_GEN_MOVEMARK01, MOVE_MARK);
SPECS.set(XI_GEN_WATERCIRCLE01, WATER);
SPECS.set(XI_GEN_WATERCROWN01, WATER);
SPECS.set(XI_GEN_RAINCIRCLE01, RAIN_CIRCLE);
for (const id of FIXED_ANGLE.keys()) SPECS.set(id, FIXED);
for (const id of REMOVABLE_XI) SPECS.set(id, { ...BASE_SPEC, removable: true });

/** The behaviour class of an effect id. */
export function specOf(xi: number): ClassSpec {
  return SPECS.get(xi) ?? skillSpecOf(xi) ?? BASE_SPEC;
}

/** The angle the factory gives a `CSfxFixed` effect from its source's yaw, `null` for ids that keep their own angle. */
export function fixedAngle(xi: number, srcAngle: number): number | null {
  switch (FIXED_ANGLE.get(xi)) {
    case 'neg':
      return -srcAngle;
    case 'negPlus180':
      return -srcAngle + 180;
    case 'minus180':
      return 180 - srcAngle;
    case undefined:
      return null;
  }
}

/** `true` for an effect id handled by the `CSfxFixed` angle table. */
export const isFixedHit = (xi: number): boolean => FIXED_ANGLE.has(xi);
