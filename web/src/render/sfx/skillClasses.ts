// The behaviour classes of the skill effects (Plan/41 section 17.2, WP-REND-24; ref/20 section 7.2; CreateObj.cpp:33-375, Sfx.cpp):
// which class the `CreateSfx` factory builds for an effect id, who implements it, and the specs of the classes this work package adds.
//
//   {@link CLASS_ROWS}      the factory table, row by row (the classes of the plan's table: every XI id of the factory switch)
//   {@link classOf}         the class of an id (`CSfx` for everything the factory does not list)
//   {@link skillSpecOf}     the {@link ClassSpec} of the ids whose class is implemented here (`SfxSystem` consults it through `specOf`)
//   {@link UNIMPLEMENTED_XI} the ids whose class belongs to another work package (set-item auras REND-33, the item effects REND-23): they draw
//                          as the base `CSfx` until that package ships (the boss classes of REND-27 are `impl: 'boss'`, `bossClasses.ts`)
//
// The classes are table-driven: a row names the class and its ids, the behaviour of a class is a spec ({@link ClassSpec}) and, for
// the classes with a `Process` of their own, a {@link SfxBehavior} (`skillBehaviors.ts`). Adding a class is a row and a spec.

import {
  XI_BUFFPET_GRADE1,
  XI_BUFFPET_GRADE2,
  XI_BUFFPET_GRADE3,
  XI_CHR_CURSOR1,
  XI_EVE_EVENT_FAIRYLY,
  XI_FIR_RANGE01,
  XI_FIR_RANGE02,
  XI_FIR_WAND01,
  XI_FIR_WAND02,
  XI_GEN_CURE01,
  XI_GEN_ITEM_SETITEM03,
  XI_GEN_ITEM_SETITEM04,
  XI_GEN_ITEM_SETITEM05,
  XI_GEN_ITEM_SETITEM06,
  XI_GEN_ITEM_SETITEM07,
  XI_GEN_ITEM_SETITEM08,
  XI_GEN_ITEM_SETITEM09,
  XI_GEN_ITEM_SETITEM10,
  XI_GEN_LOGIN01,
  XI_GEN_MOVEMARK01,
  XI_GEN_PC_DIE01,
  XI_GEN_PVP_FLAG01,
  XI_GEN_RAINCIRCLE01,
  XI_GEN_REF01,
  XI_GEN_RESTORATION01,
  XI_GEN_WARP01,
  XI_GEN_WATERCIRCLE01,
  XI_GEN_WATERCROWN01,
  XI_HIT_BOW01,
  XI_HIT_CRITICAL01,
  XI_HIT_MISS01,
  XI_HIT_STICK01,
  XI_HIT_STICK02,
  XI_HIT_SWORD01,
  XI_HIT_SWORD02,
  XI_HIT_YOY01,
  XI_ITEM_COLLECT,
  XI_ITEM_RANGE_ATK1,
  XI_ITEM_RANGE_ATK2,
  XI_ITEM_RANGE_ATK3,
  XI_ITEM_RANGE_ATK4,
  XI_ITEM_WAND_ATK1,
  XI_ITEM_WAND_ATK2,
  XI_ITEM_WAND_ATK3,
  XI_ITEM_WAND_ATK4,
  XI_ITEM_YOYO_ATK1,
  XI_ITEM_YOYO_ATK2,
  XI_ITEM_YOYO_ATK3,
  XI_ITEM_YOYO_ATK4,
  XI_ITEM_YOYO_ATK5,
  XI_NAT_CUPITSTART01,
  XI_NAT_WASTART01,
  XI_NAT_WINGANGEL01,
  XI_NPCSP1DIRAMP,
  XI_NPCSP1DIRCANNON,
  XI_NPCSP1RANBALL,
  XI_NPC_DIR_STEAM,
  XI_NPC_RAN_CANNON,
  XI_NPC_RAN_CARD,
  XI_NPC_RAN_GAS,
  XI_NPC_RAN_MAGICBALL,
  XI_NPC_RAN_MAGICBLUE,
  XI_NPC_RAN_SPITTL,
  XI_SETIEM_EFFECTHAND,
  XI_SKILL_ACR_BOW_AIMEDSHOT01,
  XI_SKILL_ACR_BOW_AIMEDSHOT03,
  XI_SKILL_ACR_BOW_ARROWRAIN01,
  XI_SKILL_ACR_BOW_ARROWRAIN03,
  XI_SKILL_ACR_BOW_AUTOSHOT01,
  XI_SKILL_ACR_BOW_AUTOSHOT03,
  XI_SKILL_ACR_BOW_JUNKBOW01,
  XI_SKILL_ACR_BOW_JUNKBOW03,
  XI_SKILL_ACR_BOW_SILENTSHOT01,
  XI_SKILL_ACR_BOW_SILENTSHOT03,
  XI_SKILL_ACR_YOYO_SLOWSTEP01,
  XI_SKILL_ACR_YOYO_SLOWSTEP02,
  XI_SKILL_ACR_YOYO_SNITCH02,
  XI_SKILL_ASS_CHEER_CASTING01,
  XI_SKILL_ASS_CHEER_HASCASTING01,
  XI_SKILL_ASS_HEAL_CASTING01,
  XI_SKILL_ASS_KNU_BURSTCRACK01,
  XI_SKILL_ASS_KNU_SUPPORT01,
  XI_SKILL_ASS_KNU_SUPPORT02,
  XI_SKILL_ASS_KNU_SUPPORT03,
  XI_SKILL_ASS_KNU_TAMPINGHOLE01,
  XI_SKILL_ASS_RES_CASTING01,
  XI_SKILL_BIL_KNU_ASALRAALAIKUM02,
  XI_SKILL_BIL_KNU_BELIALSMESHING02,
  XI_SKILL_BIL_KNU_BLOODFIST02,
  XI_SKILL_BIL_KNU_PIERCINGSERPENT02,
  XI_SKILL_BIL_KNU_SONICHAND02,
  XI_SKILL_BLD_DOUBLEAX_HAWKATTACK03,
  XI_SKILL_BLD_DOUBLEAX_SPRINGATTACK01,
  XI_SKILL_BLD_DOUBLESW_BLADEDANCE01,
  XI_SKILL_BLD_DOUBLESW_BLADEDANCE02,
  XI_SKILL_BLD_DOUBLESW_BLADEDANCE03,
  XI_SKILL_BLD_DOUBLE_ARMORPENETRATE02,
  XI_SKILL_BLD_DOUBLE_CROSSSTRIKE02,
  XI_SKILL_BLD_DOUBLE_SONICBLADE01,
  XI_SKILL_BLD_DOUBLE_SONICBLADE03,
  XI_SKILL_DROP_DUST,
  XI_SKILL_DROP_DUST_RAIN,
  XI_SKILL_ELE_FIRE_BURINGFIELD02,
  XI_SKILL_ELE_FIRE_CASTING01,
  XI_SKILL_ELE_MULTI_CASTING01,
  XI_SKILL_JST_SUP_ABSORB01,
  XI_SKILL_JST_SUP_BLEEDING01,
  XI_SKILL_JST_SUP_CRITICALSWING01,
  XI_SKILL_JST_SUP_POISON01,
  XI_SKILL_JST_YOYO_BACKSTAB01,
  XI_SKILL_JST_YOYO_HITOFPENYA01,
  XI_SKILL_JST_YOYO_HITOFPENYA02,
  XI_SKILL_JST_YOYO_VATALSTAB01,
  XI_SKILL_JST_YOYO_VATALSTAB02,
  XI_SKILL_KNT_TWOSW_CHARGE01,
  XI_SKILL_KNT_TWOSW_EARTHDIVIDER02,
  XI_SKILL_KNT_TWO_POWERSWING01,
  XI_SKILL_KNT_TWO_POWERSWING02,
  XI_SKILL_MAG_EARTH_CASTING01,
  XI_SKILL_MAG_EARTH_SPIKESTONE01,
  XI_SKILL_MAG_ELECTRICITY_CASTING01,
  XI_SKILL_MAG_ELECTRICITY_LIGHTINGBALL01,
  XI_SKILL_MAG_FIRE_BOOMERANG01,
  XI_SKILL_MAG_FIRE_CASTING01,
  XI_SKILL_MAG_FIRE_FIRESTRIKE02,
  XI_SKILL_MAG_FIRE_HOTAIR01,
  XI_SKILL_MAG_MAG_CASTING01,
  XI_SKILL_MAG_WATER_CASTING01,
  XI_SKILL_MAG_WATER_ICEMISSILE01,
  XI_SKILL_MAG_WIND_CASTING01,
  XI_SKILL_MAG_WIND_STRONGWIND01,
  XI_SKILL_MAG_WIND_SWORDWIND01,
  XI_SKILL_MER_ONE_BLINDSIDE01,
  XI_SKILL_MER_ONE_BLOODYSTRIKE02,
  XI_SKILL_MER_ONE_GUILOTIN02,
  XI_SKILL_MER_ONE_KEENWHEEL01,
  XI_SKILL_MER_ONE_REFLEXHIT02,
  XI_SKILL_MER_ONE_SNEAKER02,
  XI_SKILL_MER_ONE_SPECIALHIT02,
  XI_SKILL_MER_ONE_SPLMASH01,
  XI_SKILL_MER_ONE_SPLMASH02,
  XI_SKILL_MER_ONE_SUPPORT01,
  XI_SKILL_MER_ONE_SUPPORT02,
  XI_SKILL_MER_ONE_SUPPORT03,
  XI_SKILL_MER_ONE_SUPPORT04,
  XI_SKILL_MUSHMOOT_02,
  XI_SKILL_MUSHMOOT_CHARGE,
  XI_SKILL_PSY_PSY_CASTING01,
  XI_SKILL_RAG_BOW_ARROWRAIN,
  XI_SKILL_RAG_BOW_FLAMEARROW01,
  XI_SKILL_RAG_BOW_FLAMEARROW03,
  XI_SKILL_RAG_BOW_ICEARROW01,
  XI_SKILL_RAG_BOW_PIRCINGARROW01,
  XI_SKILL_RAG_BOW_PIRCINGARROW03,
  XI_SKILL_RAG_BOW_POISONARROW01,
  XI_SKILL_RAG_BOW_POISONARROW03,
  XI_SKILL_RAG_BOW_SILENTARROW01,
  XI_SKILL_RAG_BOW_SILENTARROW03,
  XI_SKILL_RAG_BOW_TRIPLESHOT01,
  XI_SKILL_RAG_BOW_TRIPLESHOT03,
  XI_SKILL_RAG_SUP_FASTATTACK01,
  XI_SKILL_RAG_SUP_NATURE02,
  XI_SKILL_RIN_SUP_MERKABAHANZELRUSHA03,
  XI_SKILL_TRO_BLITZ,
  XI_SKILL_TRO_STRETCHING01,
  XI_SKILL_TRO_STRETCHING02,
  XI_SKILL_VAG_ONE_BRANDISH01,
  XI_SKILL_VAG_ONE_CLEANHIT01,
  XI_SKILL_VAG_ONE_OVERCUTTER01,
} from '../../gen/defs/obj';
import type { SfxOptions } from '../fx';
import { BASE_SPEC, type ClassSpec } from './classSpec';
import type { WorldQuery } from './SfxSystem';
import { bossSpecOf } from './bossClasses';
import { BEHAVIORS } from './skillBehaviors';

/** The C++ class names of the factory (CreateObj.cpp). */
export type SfxClassName = string;

/** Who draws the class: `general` / `projectile` (REND-18, -20, -21: `classes.ts`, `Projectiles.ts`), `skill` (this package), `boss` (REND-27: `bossClasses.ts`), `other`. */
export type ClassImpl = 'general' | 'projectile' | 'skill' | 'boss' | 'other';

export interface ClassRow {
  readonly cls: SfxClassName;
  readonly impl: ClassImpl;
  /** The class that really runs when it differs from the one the factory names (the Jester ids fall through to the blade class). */
  readonly effective?: SfxClassName;
  readonly ids: readonly number[];
}

/** CreateObj.cpp:40-340 as a table (the ids of one `case` group share the class of the `new` that ends it). */
export const CLASS_ROWS: readonly ClassRow[] = [
  {
    cls: 'CSfxPartsLinkShoulder',
    impl: 'other',
    ids: [
      XI_SETIEM_EFFECTHAND,
      XI_GEN_ITEM_SETITEM03,
      XI_GEN_ITEM_SETITEM04,
      XI_GEN_ITEM_SETITEM05,
      XI_GEN_ITEM_SETITEM06,
      XI_GEN_ITEM_SETITEM07,
      XI_GEN_ITEM_SETITEM08,
      XI_GEN_ITEM_SETITEM09,
      XI_GEN_ITEM_SETITEM10,
    ],
  },
  {
    cls: 'CSfxGenNormalDmg01',
    impl: 'general',
    ids: [XI_HIT_SWORD01, XI_HIT_SWORD02, XI_HIT_STICK01, XI_HIT_STICK02],
  },
  {
    cls: 'CSfxItemWandAtk1',
    impl: 'projectile',
    ids: [
      XI_FIR_WAND01,
      XI_ITEM_WAND_ATK1,
      XI_NPC_RAN_SPITTL,
      XI_NPC_RAN_GAS,
      XI_NPC_RAN_MAGICBALL,
      XI_NPC_RAN_MAGICBLUE,
      XI_NPC_RAN_CARD,
      XI_NPC_RAN_CANNON,
    ],
  },
  { cls: 'CSfxItemWandAtkAir', impl: 'projectile', ids: [XI_FIR_WAND02] },
  {
    cls: 'CSfx',
    impl: 'general',
    ids: [
      XI_FIR_RANGE01,
      XI_FIR_RANGE02,
      XI_GEN_CURE01,
      XI_GEN_REF01,
      XI_HIT_CRITICAL01,
      XI_HIT_MISS01,
      XI_SKILL_MER_ONE_BLOODYSTRIKE02,
      XI_NAT_WASTART01,
      XI_NAT_CUPITSTART01,
      XI_SKILL_KNT_TWO_POWERSWING01,
    ],
  },
  { cls: 'CSfxGenRestoration01', impl: 'general', ids: [XI_GEN_RESTORATION01] },
  { cls: 'CSfxGenLogin', impl: 'general', ids: [XI_GEN_LOGIN01] },
  { cls: 'CSfxGenWarp', impl: 'general', ids: [XI_GEN_WARP01] },
  { cls: 'CSfxGenPcDie', impl: 'general', ids: [XI_GEN_PC_DIE01] },
  { cls: 'CSfxGenMoveMark', impl: 'general', ids: [XI_GEN_MOVEMARK01] },
  { cls: 'CSfxGenRainCircle', impl: 'general', ids: [XI_GEN_RAINCIRCLE01] },
  { cls: 'CSfxGenWaterCircle', impl: 'general', ids: [XI_GEN_WATERCIRCLE01] },
  { cls: 'CSfxGenWaterCrown', impl: 'general', ids: [XI_GEN_WATERCROWN01] },
  { cls: 'CSfxItemWandAtk2', impl: 'projectile', ids: [XI_ITEM_WAND_ATK2] },
  { cls: 'CSfxItemWandAtk3', impl: 'projectile', ids: [XI_ITEM_WAND_ATK3] },
  { cls: 'CSfxItemWandAtk4', impl: 'projectile', ids: [XI_ITEM_WAND_ATK4] },
  {
    cls: 'CSfxItemRangeAtk1',
    impl: 'projectile',
    ids: [XI_ITEM_RANGE_ATK1, XI_ITEM_RANGE_ATK2, XI_ITEM_RANGE_ATK3, XI_ITEM_RANGE_ATK4],
  },
  {
    cls: 'CSfxFixed',
    impl: 'general',
    ids: [
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
      XI_SKILL_BIL_KNU_SONICHAND02,
      XI_SKILL_BIL_KNU_ASALRAALAIKUM02,
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
      XI_EVE_EVENT_FAIRYLY,
    ],
  },
  { cls: 'CSfxSkillMerKeenWheel', impl: 'skill', ids: [XI_SKILL_MER_ONE_KEENWHEEL01] },
  { cls: 'CSfxSkillMerSplmash', impl: 'skill', ids: [XI_SKILL_MER_ONE_SPLMASH01] },
  { cls: 'CSfxSkillMerBlindSide', impl: 'skill', ids: [XI_SKILL_MER_ONE_BLINDSIDE01] },
  { cls: 'CSfxSkillMagSwordWind', impl: 'projectile', ids: [XI_SKILL_MAG_WIND_SWORDWIND01] },
  { cls: 'CSfxSkillMagStrongWind', impl: 'projectile', ids: [XI_SKILL_MAG_WIND_STRONGWIND01] },
  { cls: 'CSfxSkillMagFireBoomerang', impl: 'projectile', ids: [XI_SKILL_MAG_FIRE_BOOMERANG01] },
  { cls: 'CSfxSkillMagHotAir', impl: 'skill', ids: [XI_SKILL_MAG_FIRE_HOTAIR01] },
  {
    cls: 'CSfxSkillMagFireCasting',
    impl: 'skill',
    ids: [
      XI_SKILL_MAG_EARTH_CASTING01,
      XI_SKILL_MAG_ELECTRICITY_CASTING01,
      XI_SKILL_MAG_WATER_CASTING01,
      XI_SKILL_MAG_MAG_CASTING01,
      XI_SKILL_ELE_FIRE_CASTING01,
      XI_SKILL_ASS_CHEER_HASCASTING01,
      XI_SKILL_ELE_MULTI_CASTING01,
      XI_SKILL_PSY_PSY_CASTING01,
      XI_SKILL_MAG_FIRE_CASTING01,
      XI_SKILL_ASS_HEAL_CASTING01,
      XI_SKILL_ASS_RES_CASTING01,
    ],
  },
  {
    cls: 'CSfxSkillMagWindCasting',
    impl: 'skill',
    ids: [XI_SKILL_MAG_WIND_CASTING01, XI_SKILL_ASS_CHEER_CASTING01],
  },
  { cls: 'CSfxMagicMiAtk1', impl: 'boss', ids: [XI_SKILL_MUSHMOOT_02] },
  { cls: 'CSfxTroStretching01', impl: 'skill', ids: [XI_SKILL_TRO_STRETCHING01] },
  { cls: 'CSfxTroStretching02', impl: 'skill', ids: [XI_SKILL_TRO_STRETCHING02] },
  { cls: 'CSfxTroBlitz', impl: 'skill', ids: [XI_SKILL_TRO_BLITZ] },
  { cls: 'CSfxSkillAssBurstcrack', impl: 'skill', ids: [XI_SKILL_ASS_KNU_BURSTCRACK01] },
  { cls: 'CSfxSkillAssTampinghole', impl: 'skill', ids: [XI_SKILL_ASS_KNU_TAMPINGHOLE01] },
  { cls: 'CSfxNpcDirSteam', impl: 'boss', ids: [XI_NPC_DIR_STEAM] },
  { cls: 'CSfxDuelParty', impl: 'skill', ids: [XI_GEN_PVP_FLAG01] },
  { cls: 'CSfxCollect', impl: 'other', ids: [XI_ITEM_COLLECT] },
  { cls: 'CSfxClockWorksCharge', impl: 'boss', ids: [XI_NPCSP1DIRAMP] },
  { cls: 'CSfxClockWorksCannon', impl: 'boss', ids: [XI_NPCSP1DIRCANNON] },
  { cls: 'CSfxAtkStraight', impl: 'boss', ids: [XI_NPCSP1RANBALL] },
  { cls: 'CSfxMushmootCharge', impl: 'boss', ids: [XI_SKILL_MUSHMOOT_CHARGE] },
  { cls: 'CSfxSkillMagIceMissile', impl: 'projectile', ids: [XI_SKILL_MAG_WATER_ICEMISSILE01] },
  {
    cls: 'CSfxSkillMagLightningBall',
    impl: 'projectile',
    ids: [XI_SKILL_MAG_ELECTRICITY_LIGHTINGBALL01],
  },
  { cls: 'CSfxShoot', impl: 'projectile', ids: [XI_SKILL_MAG_FIRE_FIRESTRIKE02] },
  { cls: 'CSfxSkillMagSpikeStone', impl: 'projectile', ids: [XI_SKILL_MAG_EARTH_SPIKESTONE01] },
  { cls: 'CSfxRotate', impl: 'skill', ids: [XI_NAT_WINGANGEL01] },
  { cls: 'CSfxShootWave', impl: 'projectile', ids: [XI_SKILL_MER_ONE_GUILOTIN02] },
  { cls: 'CSfxItemRangeAtk1_StoneRain', impl: 'skill', ids: [XI_SKILL_DROP_DUST_RAIN] },
  { cls: 'CSfxItemRangeAtk1_Stone', impl: 'skill', ids: [XI_SKILL_DROP_DUST] },
  { cls: 'CSfxItemRangeAtk1_AllowRain', impl: 'skill', ids: [XI_SKILL_ACR_BOW_ARROWRAIN03] },
  { cls: 'CSfxItemRangeAtk1_Allow', impl: 'skill', ids: [XI_SKILL_RAG_BOW_ARROWRAIN] },
  {
    cls: 'CSfxAllowPartsLink',
    impl: 'skill',
    ids: [
      XI_SKILL_RAG_BOW_ICEARROW01,
      XI_SKILL_RAG_SUP_FASTATTACK01,
      XI_SKILL_RAG_BOW_FLAMEARROW01,
      XI_SKILL_RAG_BOW_PIRCINGARROW01,
      XI_SKILL_RAG_BOW_POISONARROW01,
      XI_SKILL_RAG_BOW_SILENTARROW01,
      XI_SKILL_RAG_BOW_TRIPLESHOT01,
      XI_SKILL_ACR_BOW_JUNKBOW01,
      XI_SKILL_ACR_BOW_AIMEDSHOT01,
      XI_SKILL_ACR_BOW_SILENTSHOT01,
      XI_SKILL_ACR_BOW_ARROWRAIN01,
      XI_SKILL_ACR_BOW_AUTOSHOT01,
      XI_SKILL_MER_ONE_SUPPORT01,
      XI_SKILL_MER_ONE_SUPPORT02,
      XI_SKILL_MER_ONE_SUPPORT03,
      XI_SKILL_MER_ONE_SUPPORT04,
      XI_SKILL_ASS_KNU_SUPPORT01,
      XI_SKILL_ASS_KNU_SUPPORT02,
      XI_SKILL_ASS_KNU_SUPPORT03,
    ],
  },
  {
    cls: 'CSfxPartsLinkJst',
    impl: 'skill',
    effective: 'CSfxPartsLinkBlade',
    ids: [
      XI_SKILL_JST_YOYO_VATALSTAB01,
      XI_SKILL_JST_YOYO_HITOFPENYA01,
      XI_SKILL_ACR_YOYO_SLOWSTEP01,
      XI_SKILL_JST_SUP_POISON01,
      XI_SKILL_JST_SUP_BLEEDING01,
      XI_SKILL_JST_YOYO_BACKSTAB01,
      XI_SKILL_JST_SUP_ABSORB01,
      XI_SKILL_JST_SUP_CRITICALSWING01,
    ],
  },
  {
    cls: 'CSfxPartsLinkBlade',
    impl: 'skill',
    ids: [
      XI_SKILL_BLD_DOUBLEAX_SPRINGATTACK01,
      XI_SKILL_BLD_DOUBLE_SONICBLADE01,
      XI_SKILL_KNT_TWOSW_CHARGE01,
      XI_SKILL_BLD_DOUBLESW_BLADEDANCE01,
    ],
  },
  { cls: 'CSfxCursor', impl: 'other', ids: [XI_CHR_CURSOR1] },
  {
    cls: 'CSfxLinkMover',
    impl: 'other',
    ids: [XI_BUFFPET_GRADE1, XI_BUFFPET_GRADE2, XI_BUFFPET_GRADE3],
  },
];

const CLASS_BY_ID = new Map<number, ClassRow>();
for (const row of CLASS_ROWS) for (const id of row.ids) CLASS_BY_ID.set(id, row);

/** The class `CreateSfx` builds for `xi` (`CSfx` for an id the factory does not list). */
export function classOf(xi: number): SfxClassName {
  const row = CLASS_BY_ID.get(xi);
  return row ? (row.effective ?? row.cls) : 'CSfx';
}

/** Who draws the class of `xi` (the default class is the generic one). */
export function implOf(xi: number): ClassImpl {
  return CLASS_BY_ID.get(xi)?.impl ?? 'general';
}

/** The row of an id, `undefined` for the default class. */
export function rowOf(xi: number): ClassRow | undefined {
  return CLASS_BY_ID.get(xi);
}

/** The ids whose class is another work package's. */
export const UNIMPLEMENTED_XI: readonly number[] = CLASS_ROWS.filter(
  (r) => r.impl === 'other',
).flatMap((r) => r.ids);

/** The `.sfx` ids of the flying yo-yo (`CreateSfxYoYo` always builds `CSfxItemYoyoAtk` whatever the id: the weapon's `sfx_obj[1]`). */
export const YOYO_ATK_XI: readonly number[] = [
  XI_ITEM_YOYO_ATK1,
  XI_ITEM_YOYO_ATK2,
  XI_ITEM_YOYO_ATK3,
  XI_ITEM_YOYO_ATK4,
  XI_ITEM_YOYO_ATK5,
];

export { CHILD_XI } from './skillBehaviors';

/** `XI_SKILL_MER_ONE_SNEAKER02` (scaled to the target's radius by the factory). */
export const XI_SNEAKER02 = XI_SKILL_MER_ONE_SNEAKER02;

// ---- the specs -----------------------------------------------------------------------------------------------------------------

const SKILL_BASE: ClassSpec = { ...BASE_SPEC, follow: 'none', lift: 0, once: true };
const SPEC_BY_CLASS = new Map<SfxClassName, ClassSpec>();

function define(
  cls: SfxClassName,
  spec: Partial<ClassSpec> & { behavior?: ClassSpec['behavior'] },
): void {
  SPEC_BY_CLASS.set(cls, { ...SKILL_BASE, ...spec });
}

// casting auras: static where the cast began; the fire one stands a metre up and turns against the caster, the wind one a hand up
define('CSfxSkillMagFireCasting', { lift: 1, behavior: BEHAVIORS.castFire });
define('CSfxSkillMagWindCasting', { lift: 0.2, yaw: false });
// Mercenary specials 1 and 3: static, object yaw (0)
define('CSfxSkillMerKeenWheel', {});
define('CSfxSkillMerBlindSide', {});
define('CSfxSkillMerSplmash', { once: false, behavior: BEHAVIORS.splmash });
// Assist knuckle specials
define('CSfxSkillAssBurstcrack', { lift: 0.1, behavior: BEHAVIORS.burstcrack });
define('CSfxSkillAssTampinghole', { behavior: BEHAVIORS.tampinghole });
// hand links
define('CSfxAllowPartsLink', { behavior: BEHAVIORS.handLink });
define('CSfxPartsLinkBlade', { once: false, behavior: BEHAVIORS.bladeLink });
// wings
define('CSfxRotate', { once: false, behavior: BEHAVIORS.wingAngel });
// troupe
define('CSfxTroStretching01', { behavior: BEHAVIORS.stretching01 });
define('CSfxTroStretching02', { once: false, behavior: BEHAVIORS.stretching02 });
define('CSfxTroBlitz', { once: false, behavior: BEHAVIORS.blitz });
// the flag over a duelist: a base effect that ends with the duel
define('CSfxDuelParty', { ...BASE_SPEC, behavior: BEHAVIORS.duelParty });
// rain
define('CSfxItemRangeAtk1_AllowRain', { behavior: BEHAVIORS.arrowRain });
define('CSfxItemRangeAtk1_StoneRain', { behavior: BEHAVIORS.stoneRain });
define('CSfxItemRangeAtk1_Allow', { once: false, render2: true, behavior: BEHAVIORS.arrowDrop });
define('CSfxItemRangeAtk1_Stone', { once: false, render2: true, behavior: BEHAVIORS.stoneDrop });

// effects that are not in the factory table but need a spec: the second model of the Splmash plays where it stands, unlifted
const SPEC_BY_ID = new Map<number, ClassSpec>([[XI_SKILL_MER_ONE_SPLMASH02, SKILL_BASE]]);

/** The spec of the class of `xi` when this package implements it. */
export function skillSpecOf(xi: number): ClassSpec | undefined {
  const row = CLASS_BY_ID.get(xi);
  if (row?.impl === 'boss') return bossSpecOf(row.cls);
  if (row?.impl !== 'skill') return SPEC_BY_ID.get(xi);
  return SPEC_BY_CLASS.get(row.effective ?? row.cls);
}

// ---- the factory's post-creation fixes (CreateObj.cpp:263-340) -----------------------------------------------------------------

export interface PreCreate {
  /** The options the effect is made with. */
  readonly o: SfxOptions;
  /** Effects the factory also makes (`XI_NAT_WASTART01` starts the wings). */
  readonly also: readonly (readonly [number, SfxOptions])[];
}

/** `GetDegree(vDestPos, vSrcPos)` of Obj.h: the angle in degrees of `dest - src` in the XZ plane, 0 along -Z, increasing toward +X. */
export function getDegree(dest: readonly number[], src: readonly number[]): number {
  let dx = (dest[0] ?? 0) - (src[0] ?? 0);
  let dz = (dest[2] ?? 0) - (src[2] ?? 0);
  const len = Math.hypot(dx, dz);
  if (len > 0) {
    dx /= len;
    dz /= len;
  }
  // `dot(normalize((0, 0, -1)), dir)` = -dz
  const degree = (Math.acos(Math.max(-1, Math.min(1, -dz))) * 180) / Math.PI;
  return dx < 0 ? 360 - degree : degree;
}

/**
 * What `CreateSfx` does to an id besides picking its class:
 *   `XI_SKILL_KNT_TWO_POWERSWING01`   the destination is cleared (the effect follows its source)
 *   `XI_SKILL_MER_ONE_BLOODYSTRIKE02` the angle is `-GetDegree(src, dest)` (note the argument order of the original)
 *   `XI_SKILL_MER_ONE_SNEAKER02`      the scale is the target's radius
 *   `XI_NAT_WASTART01`, `XI_NAT_CUPITSTART01`   the wings (`XI_NAT_WINGANGEL01`) start with the same parameters
 */
export function preCreate(xi: number, o: SfxOptions, world: WorldQuery): PreCreate {
  const tmpA: [number, number, number] = [0, 0, 0];
  const tmpB: [number, number, number] = [0, 0, 0];
  switch (xi) {
    case XI_SKILL_KNT_TWO_POWERSWING01:
      return { o: { ...o, dest: 0 }, also: [] };
    case XI_SKILL_MER_ONE_BLOODYSTRIKE02: {
      const src = o.src ?? 0;
      const dest = o.dest ?? 0;
      if (src !== 0 && dest !== 0 && world.posOf(src, tmpA) && world.posOf(dest, tmpB)) {
        return { o: { ...o, angle: -getDegree(tmpA, tmpB) }, also: [] };
      }
      return { o, also: [] };
    }
    case XI_SKILL_MER_ONE_SNEAKER02: {
      const dest = o.dest ?? 0;
      const radius = dest !== 0 ? world.radiusOf?.(dest) : undefined;
      if (radius !== undefined && radius > 0 && world.posOf(dest, tmpA)) {
        return { o: { ...o, scale: radius }, also: [] };
      }
      return { o, also: [] };
    }
    case XI_NAT_WASTART01:
    case XI_NAT_CUPITSTART01:
      return { o, also: [[XI_NAT_WINGANGEL01, o]] };
    default:
      return { o, also: [] };
  }
}
