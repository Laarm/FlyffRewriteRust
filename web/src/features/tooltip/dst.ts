// The DST strings and the rate rule of the item tooltip (Plan/45 §10.3; `g_DstString`, `IsDst_Rate`, `FindDstString`,
// `PutDestParam`, `src:_Interface/WndManager.cpp:4427-4609, 7487-7550`). Both tables are C++ code, not Resource data (Open
// question 5 of doc 45): they are transcribed from the source by a script (the extraction is documented in `NOTES.md`) and pinned by
// `dst.test.ts` (every referenced TID exists in the real text bundle when `FLYFF_BUNDLES_DIR` is set). `#if` guards of the
// source are resolved for the shipped V15 build: `__NEWWPN1024` (`DST_MASTRY_ALL`) is not defined, everything `__VER >= 15` is in.

import {
  DST_ABILITY_MAX,
  DST_ABILITY_MIN,
  DST_ADDMAGIC,
  DST_ADJDEF,
  DST_ADJDEF_RATE,
  DST_ADJ_HITRATE,
  DST_ALL_DEC_RATE,
  DST_ALL_RECOVERY,
  DST_ALL_RECOVERY_RATE,
  DST_ATKPOWER,
  DST_ATKPOWER_RATE,
  DST_ATTACKSPEED,
  DST_ATTACKSPEED_RATE,
  DST_AXE_DMG,
  DST_BLOCK_MELEE,
  DST_BLOCK_RANGE,
  DST_BOWMASTER_DMG,
  DST_BOW_DMG,
  DST_CAST_CRITICAL_RATE,
  DST_CHRSTATE,
  DST_CHR_CHANCECRITICAL,
  DST_CHR_DMG,
  DST_CHR_RANGE,
  DST_CHR_STEALHP,
  DST_CHR_WEAEATKCHANGE,
  DST_CRITICAL_BONUS,
  DST_DEFHITRATE_DOWN,
  DST_DEX,
  DST_EXPERIENCE,
  DST_FP,
  DST_FP_DEC_RATE,
  DST_FP_MAX,
  DST_FP_MAX_RATE,
  DST_FP_RECOVERY,
  DST_FP_RECOVERY_RATE,
  DST_GIFTBOX,
  DST_HAWKEYE_RATE,
  DST_HEAL,
  DST_HP,
  DST_HPDMG_UP,
  DST_HP_MAX,
  DST_HP_MAX_RATE,
  DST_HP_RECOVERY,
  DST_HP_RECOVERY_RATE,
  DST_IMMUNITY,
  DST_INT,
  DST_JUMPING,
  DST_KILL_ALL,
  DST_KILL_ALL_RATE,
  DST_KILL_FP,
  DST_KILL_FP_RATE,
  DST_KILL_HP,
  DST_KILL_HP_RATE,
  DST_KILL_MP,
  DST_KILL_MP_RATE,
  DST_KNUCKLEMASTER_DMG,
  DST_KNUCKLE_DMG,
  DST_MASTRY_EARTH,
  DST_MASTRY_ELECTRICITY,
  DST_MASTRY_FIRE,
  DST_MASTRY_WATER,
  DST_MASTRY_WIND,
  DST_MELEE_STEALHP,
  DST_MONSTER_DMG,
  DST_MP,
  DST_MP_DEC_RATE,
  DST_MP_MAX,
  DST_MP_MAX_RATE,
  DST_MP_RECOVERY,
  DST_MP_RECOVERY_RATE,
  DST_ONEHANDMASTER_DMG,
  DST_PARRY,
  DST_PVP_DMG,
  DST_PVP_DMG_RATE,
  DST_REFLECT_DAMAGE,
  DST_RESIST_ALL,
  DST_RESIST_EARTH,
  DST_RESIST_ELECTRICITY,
  DST_RESIST_FIRE,
  DST_RESIST_MAGIC,
  DST_RESIST_MAGIC_RATE,
  DST_RESIST_WATER,
  DST_RESIST_WIND,
  DST_RESTPOINT_RATE,
  DST_SPEED,
  DST_SPELL_RATE,
  DST_STA,
  DST_STAT_ALLUP,
  DST_STOP_MOVEMENT,
  DST_STR,
  DST_SWD_DMG,
  DST_TWOHANDMASTER_DMG,
  DST_YOYOMASTER_DMG,
  DST_YOY_DMG,
} from '../../gen/defs/attribute';
import {
  TID_GAME_TOOLTIPALLSTAT,
  TID_GAME_TOOLTIPCONDITIONRATE,
  TID_GAME_TOOLTIP_ATTACKRANGE4,
  TID_GAME_TOOLTIP_BLOCK,
  TID_GAME_TOOLTIP_BLOCKRANGE,
  TID_GAME_TOOLTIP_IMMUNITY1,
  TID_GAME_TOOLTIP_MOVEMENT1,
  TID_TOOLTIP_ABILITYMAX,
  TID_TOOLTIP_ABILITYMIN,
  TID_TOOLTIP_ALL_DEC_RATE,
  TID_TOOLTIP_ALL_RECOVERY,
  TID_TOOLTIP_ALL_RECOVERY_RATE,
  TID_TOOLTIP_ATKPOWER,
  TID_TOOLTIP_ATKPOWER_VALUE,
  TID_TOOLTIP_ATKSPEED,
  TID_TOOLTIP_ATTACKSPEED_RATE,
  TID_TOOLTIP_CHRDMG,
  TID_TOOLTIP_CHRSTATE,
  TID_TOOLTIP_DEFALLELEMETAL,
  TID_TOOLTIP_DEFEARTH,
  TID_TOOLTIP_DEFELECTRICITY,
  TID_TOOLTIP_DEFENCE,
  TID_TOOLTIP_DEFFIFE,
  TID_TOOLTIP_DEFMAGIC,
  TID_TOOLTIP_DEFMAGIC_RATE,
  TID_TOOLTIP_DEFWATER,
  TID_TOOLTIP_DEFWIND,
  TID_TOOLTIP_DEX,
  TID_TOOLTIP_DST_ADJDEF_RATE,
  TID_TOOLTIP_DST_AXE_DMG,
  TID_TOOLTIP_DST_BOWMASTER_DMG,
  TID_TOOLTIP_DST_BOW_DMG,
  TID_TOOLTIP_DST_CAST_CRITICAL_RATE,
  TID_TOOLTIP_DST_CHR_WEAEATKCHANGE,
  TID_TOOLTIP_DST_CRITICAL_BONUS,
  TID_TOOLTIP_DST_CRITICAL_RATE,
  TID_TOOLTIP_DST_DEFHITRATE_DOWN,
  TID_TOOLTIP_DST_DMG_GET,
  TID_TOOLTIP_DST_EXPERIENCE,
  TID_TOOLTIP_DST_FP_DEC_RATE,
  TID_TOOLTIP_DST_FP_RATE,
  TID_TOOLTIP_DST_GIFTBOX,
  TID_TOOLTIP_DST_HAWKEYE,
  TID_TOOLTIP_DST_HEAL,
  TID_TOOLTIP_DST_HPDMG_UP,
  TID_TOOLTIP_DST_HP_MAX_RATE,
  TID_TOOLTIP_DST_KNUCKLEMASTER_DMG,
  TID_TOOLTIP_DST_KNUCKLE_DMG,
  TID_TOOLTIP_DST_MASTRY_EARTH,
  TID_TOOLTIP_DST_MASTRY_ELECTRICITY,
  TID_TOOLTIP_DST_MASTRY_FIRE,
  TID_TOOLTIP_DST_MASTRY_WATER,
  TID_TOOLTIP_DST_MASTRY_WIND,
  TID_TOOLTIP_DST_MELEE_STEALHP,
  TID_TOOLTIP_DST_MONSTER_DMG,
  TID_TOOLTIP_DST_MP_DEC_RATE,
  TID_TOOLTIP_DST_MP_MAX_RATE,
  TID_TOOLTIP_DST_ONEHANDMASTER_DMG,
  TID_TOOLTIP_DST_PVP_DMG,
  TID_TOOLTIP_DST_REFLECT_DAMAGE,
  TID_TOOLTIP_DST_SPELL_RATE,
  TID_TOOLTIP_DST_SWD_DMG,
  TID_TOOLTIP_DST_TWOHANDMASTER_DMG,
  TID_TOOLTIP_DST_YOYOMASTER_DMG,
  TID_TOOLTIP_DST_YOY_DMG,
  TID_TOOLTIP_FP,
  TID_TOOLTIP_FPRECOVERY,
  TID_TOOLTIP_FPRECOVERYRATE,
  TID_TOOLTIP_HITRATE,
  TID_TOOLTIP_HP,
  TID_TOOLTIP_HPRECOVERY,
  TID_TOOLTIP_HPRECOVERYRATE,
  TID_TOOLTIP_INT,
  TID_TOOLTIP_JUMPING,
  TID_TOOLTIP_KILL_ALL,
  TID_TOOLTIP_KILL_ALL_RATE,
  TID_TOOLTIP_KILL_FP,
  TID_TOOLTIP_KILL_FP_RATE,
  TID_TOOLTIP_KILL_HP,
  TID_TOOLTIP_KILL_HP_RATE,
  TID_TOOLTIP_KILL_MP,
  TID_TOOLTIP_KILL_MP_RATE,
  TID_TOOLTIP_MAXFP,
  TID_TOOLTIP_MAXHP,
  TID_TOOLTIP_MAXMP,
  TID_TOOLTIP_MP,
  TID_TOOLTIP_MPRECOVERY,
  TID_TOOLTIP_MPRECOVERYRATE,
  TID_TOOLTIP_PARRY,
  TID_TOOLTIP_RESTPOINT_RATE,
  TID_TOOLTIP_SPEED,
  TID_TOOLTIP_STA,
  TID_TOOLTIP_STR,
} from '../../gen/defs/text';
import type { RichRun } from '../../ui/text/markup';

/** `g_DstString`: the first entry of a DST wins (`DST_HP_MAX_RATE` is listed twice with the same TID). */
export const DST_STRING_ROWS: readonly (readonly [dst: number, tid: number])[] = [
  [DST_STR, TID_TOOLTIP_STR],
  [DST_DEX, TID_TOOLTIP_DEX],
  [DST_INT, TID_TOOLTIP_INT],
  [DST_STA, TID_TOOLTIP_STA],
  [DST_SPEED, TID_TOOLTIP_SPEED],
  [DST_ABILITY_MIN, TID_TOOLTIP_ABILITYMIN],
  [DST_ABILITY_MAX, TID_TOOLTIP_ABILITYMAX],
  [DST_ATTACKSPEED, TID_TOOLTIP_ATKSPEED],
  [DST_ADJDEF, TID_TOOLTIP_DEFENCE],
  [DST_RESIST_MAGIC, TID_TOOLTIP_DEFMAGIC],
  [DST_RESIST_ELECTRICITY, TID_TOOLTIP_DEFELECTRICITY],
  [DST_RESIST_ALL, TID_TOOLTIP_DEFALLELEMETAL],
  [DST_RESIST_FIRE, TID_TOOLTIP_DEFFIFE],
  [DST_RESIST_WIND, TID_TOOLTIP_DEFWIND],
  [DST_RESIST_WATER, TID_TOOLTIP_DEFWATER],
  [DST_RESIST_EARTH, TID_TOOLTIP_DEFEARTH],
  [DST_HP_MAX, TID_TOOLTIP_MAXHP],
  [DST_MP_MAX, TID_TOOLTIP_MAXMP],
  [DST_FP_MAX, TID_TOOLTIP_MAXFP],
  [DST_HP, TID_TOOLTIP_HP],
  [DST_MP, TID_TOOLTIP_MP],
  [DST_FP, TID_TOOLTIP_FP],
  [DST_HP_RECOVERY, TID_TOOLTIP_HPRECOVERY],
  [DST_MP_RECOVERY, TID_TOOLTIP_MPRECOVERY],
  [DST_FP_RECOVERY, TID_TOOLTIP_FPRECOVERY],
  [DST_HP_RECOVERY_RATE, TID_TOOLTIP_HPRECOVERYRATE],
  [DST_MP_RECOVERY_RATE, TID_TOOLTIP_MPRECOVERYRATE],
  [DST_FP_RECOVERY_RATE, TID_TOOLTIP_FPRECOVERYRATE],
  [DST_ALL_RECOVERY, TID_TOOLTIP_ALL_RECOVERY],
  [DST_ALL_RECOVERY_RATE, TID_TOOLTIP_ALL_RECOVERY_RATE],
  [DST_KILL_HP, TID_TOOLTIP_KILL_HP],
  [DST_KILL_MP, TID_TOOLTIP_KILL_MP],
  [DST_KILL_FP, TID_TOOLTIP_KILL_FP],
  [DST_KILL_ALL, TID_TOOLTIP_KILL_ALL],
  [DST_KILL_HP_RATE, TID_TOOLTIP_KILL_HP_RATE],
  [DST_KILL_MP_RATE, TID_TOOLTIP_KILL_MP_RATE],
  [DST_KILL_FP_RATE, TID_TOOLTIP_KILL_FP_RATE],
  [DST_KILL_ALL_RATE, TID_TOOLTIP_KILL_ALL_RATE],
  [DST_ALL_DEC_RATE, TID_TOOLTIP_ALL_DEC_RATE],
  [DST_ADJ_HITRATE, TID_TOOLTIP_HITRATE],
  [DST_CHR_DMG, TID_TOOLTIP_CHRDMG],
  [DST_CHRSTATE, TID_TOOLTIP_CHRSTATE],
  [DST_PARRY, TID_TOOLTIP_PARRY],
  [DST_ATKPOWER_RATE, TID_TOOLTIP_ATKPOWER],
  [DST_JUMPING, TID_TOOLTIP_JUMPING],
  [DST_BLOCK_MELEE, TID_GAME_TOOLTIP_BLOCK],
  [DST_BLOCK_RANGE, TID_GAME_TOOLTIP_BLOCKRANGE],
  [DST_STAT_ALLUP, TID_GAME_TOOLTIPALLSTAT],
  [DST_HP_MAX_RATE, TID_TOOLTIP_DST_HP_MAX_RATE],
  [DST_ADDMAGIC, TID_GAME_TOOLTIPCONDITIONRATE],
  [DST_ADJDEF_RATE, TID_TOOLTIP_DST_ADJDEF_RATE],
  [DST_HP_MAX_RATE, TID_TOOLTIP_DST_HP_MAX_RATE],
  [DST_MP_MAX_RATE, TID_TOOLTIP_DST_MP_MAX_RATE],
  [DST_FP_MAX_RATE, TID_TOOLTIP_DST_FP_RATE],
  [DST_CHR_CHANCECRITICAL, TID_TOOLTIP_DST_CRITICAL_RATE],
  [DST_CHR_WEAEATKCHANGE, TID_TOOLTIP_DST_CHR_WEAEATKCHANGE],
  [DST_MASTRY_EARTH, TID_TOOLTIP_DST_MASTRY_EARTH],
  [DST_MASTRY_FIRE, TID_TOOLTIP_DST_MASTRY_FIRE],
  [DST_MASTRY_WATER, TID_TOOLTIP_DST_MASTRY_WATER],
  [DST_MASTRY_ELECTRICITY, TID_TOOLTIP_DST_MASTRY_ELECTRICITY],
  [DST_MASTRY_WIND, TID_TOOLTIP_DST_MASTRY_WIND],
  [DST_REFLECT_DAMAGE, TID_TOOLTIP_DST_REFLECT_DAMAGE],
  [DST_MP_DEC_RATE, TID_TOOLTIP_DST_MP_DEC_RATE],
  [DST_FP_DEC_RATE, TID_TOOLTIP_DST_FP_DEC_RATE],
  [DST_SPELL_RATE, TID_TOOLTIP_DST_SPELL_RATE],
  [DST_CAST_CRITICAL_RATE, TID_TOOLTIP_DST_CAST_CRITICAL_RATE],
  [DST_CRITICAL_BONUS, TID_TOOLTIP_DST_CRITICAL_BONUS],
  [DST_YOY_DMG, TID_TOOLTIP_DST_YOY_DMG],
  [DST_BOW_DMG, TID_TOOLTIP_DST_BOW_DMG],
  [DST_KNUCKLE_DMG, TID_TOOLTIP_DST_KNUCKLE_DMG],
  [DST_SWD_DMG, TID_TOOLTIP_DST_SWD_DMG],
  [DST_AXE_DMG, TID_TOOLTIP_DST_AXE_DMG],
  [DST_ATTACKSPEED_RATE, TID_TOOLTIP_ATTACKSPEED_RATE],
  [DST_CHR_STEALHP, TID_TOOLTIP_DST_DMG_GET],
  [DST_PVP_DMG_RATE, TID_TOOLTIP_DST_DMG_GET],
  [DST_EXPERIENCE, TID_TOOLTIP_DST_EXPERIENCE],
  [DST_MELEE_STEALHP, TID_TOOLTIP_DST_MELEE_STEALHP],
  [DST_MONSTER_DMG, TID_TOOLTIP_DST_MONSTER_DMG],
  [DST_PVP_DMG, TID_TOOLTIP_DST_PVP_DMG],
  [DST_HEAL, TID_TOOLTIP_DST_HEAL],
  [DST_ATKPOWER, TID_TOOLTIP_ATKPOWER_VALUE],
  [DST_ONEHANDMASTER_DMG, TID_TOOLTIP_DST_ONEHANDMASTER_DMG],
  [DST_TWOHANDMASTER_DMG, TID_TOOLTIP_DST_TWOHANDMASTER_DMG],
  [DST_YOYOMASTER_DMG, TID_TOOLTIP_DST_YOYOMASTER_DMG],
  [DST_BOWMASTER_DMG, TID_TOOLTIP_DST_BOWMASTER_DMG],
  [DST_KNUCKLEMASTER_DMG, TID_TOOLTIP_DST_KNUCKLEMASTER_DMG],
  [DST_HAWKEYE_RATE, TID_TOOLTIP_DST_HAWKEYE],
  [DST_RESIST_MAGIC_RATE, TID_TOOLTIP_DEFMAGIC_RATE],
  [DST_GIFTBOX, TID_TOOLTIP_DST_GIFTBOX],
  [DST_HPDMG_UP, TID_TOOLTIP_DST_HPDMG_UP],
  [DST_DEFHITRATE_DOWN, TID_TOOLTIP_DST_DEFHITRATE_DOWN],
  [DST_RESTPOINT_RATE, TID_TOOLTIP_RESTPOINT_RATE],
  [DST_CHR_RANGE, TID_GAME_TOOLTIP_ATTACKRANGE4],
  [DST_STOP_MOVEMENT, TID_GAME_TOOLTIP_MOVEMENT1],
  [DST_IMMUNITY, TID_GAME_TOOLTIP_IMMUNITY1],
];

/** `IsDst_Rate`'s table (the leading literal `0, 0, ..., 0` of the source makes DST 0 a rate too, KEEP). */
const DST_RATE_LIST: readonly number[] = [
  0,
  DST_ADJ_HITRATE,
  DST_ATKPOWER_RATE,
  DST_ADJDEF_RATE,
  DST_DEFHITRATE_DOWN,
  DST_HP_MAX_RATE,
  DST_MP_MAX_RATE,
  DST_FP_MAX_RATE,
  DST_HP_RECOVERY_RATE,
  DST_MP_RECOVERY_RATE,
  DST_FP_RECOVERY_RATE,
  DST_CHR_CHANCECRITICAL,
  DST_MASTRY_EARTH,
  DST_MASTRY_FIRE,
  DST_MASTRY_WATER,
  DST_MASTRY_ELECTRICITY,
  DST_MASTRY_WIND,
  DST_ATTACKSPEED,
  DST_MP_DEC_RATE,
  DST_FP_DEC_RATE,
  DST_SPELL_RATE,
  DST_CAST_CRITICAL_RATE,
  DST_CRITICAL_BONUS,
  DST_ALL_RECOVERY_RATE,
  DST_KILL_HP_RATE,
  DST_KILL_MP_RATE,
  DST_KILL_FP_RATE,
  DST_KILL_ALL_RATE,
  DST_ALL_DEC_RATE,
  DST_BLOCK_MELEE,
  DST_BLOCK_RANGE,
  DST_ATTACKSPEED_RATE,
  DST_CHR_STEALHP,
  DST_EXPERIENCE,
  DST_HAWKEYE_RATE,
  DST_RESIST_MAGIC_RATE,
  DST_SPEED,
  DST_REFLECT_DAMAGE,
  DST_RESTPOINT_RATE,
  DST_MONSTER_DMG,
  DST_PVP_DMG,
];

const RATE_SET: ReadonlySet<number> = new Set(DST_RATE_LIST);
const STRING_MAP: ReadonlyMap<number, number> = (() => {
  const m = new Map<number, number>();
  for (const [dst, tid] of DST_STRING_ROWS) if (!m.has(dst)) m.set(dst, tid);
  return m;
})();

/** `IsDst_Rate(dst)`: the value is shown with a `%`. */
export function isDstRate(dst: number): boolean {
  return RATE_SET.has(dst);
}

/** The TID of `FindDstString(dst)`, `undefined` for a DST the table does not know. */
export function dstTid(dst: number): number | undefined {
  return STRING_MAP.get(dst);
}

/** The text lookup the DST helpers need (`prj.GetText`). */
export type TextOf = (tid: number) => string;

/** `FindDstString(dst)`: the display name of a DST, `""` for an unknown one. */
export function findDstString(dst: number, text: TextOf): string {
  const tid = STRING_MAP.get(dst);
  return tid === undefined ? '' : text(tid);
}

/** C `n / 2 / 10` on an `int`: truncation toward zero at each division (`DST_ATTACKSPEED` is shown as `adj / 2 / 10` %). */
export function attackSpeedPercent(adj: number): number {
  return Math.trunc(Math.trunc(adj / 2) / 10);
}

/** C `%+d`: the sign is always printed. */
export function signed(n: number): string {
  const v = Math.trunc(n);
  return `${v < 0 ? '-' : '+'}${String(Math.abs(v))}`;
}

/** The value of a rate DST as the lines print it (`DST_ATTACKSPEED` through `adj / 2 / 10`). */
function rateValue(dst: number, adj: number): number {
  return dst === DST_ATTACKSPEED ? attackSpeedPercent(adj) : Math.trunc(adj);
}

/** `"\n%s%+d%%"` for a rate DST, `"\n%s%+d"` otherwise: `PutBaseItemOpt`, `PutVisPetInfo`, the accessory bonuses. */
export function dstSignedLine(dst: number, adj: number, text: TextOf): string {
  const name = findDstString(dst, text);
  return isDstRate(dst) ? `\n${name}${signed(rateValue(dst, adj))}%` : `\n${name}${signed(adj)}`;
}

/** `"\n%s%+d%%"` for a rate DST, `"\n%s+%d"` otherwise (a literal plus): `PutRandomOpt`, `PutPiercingOpt`. */
export function dstPlusLine(dst: number, adj: number, text: TextOf): string {
  const name = findDstString(dst, text);
  return isDstRate(dst)
    ? `\n${name}${signed(rateValue(dst, adj))}%`
    : `\n${name}+${String(Math.trunc(adj))}`;
}

/**
 * The awakening / blessing line (`PutAwakeningBlessing` with `__EXT_PIERCING`): `"\n%s %c%d%%"` (a trailing space for
 * `DST_ATTACKSPEED`) or `"\n%s %c%d"`, the sign from `adj > 0 ? '+' : '-'` and the absolute value.
 */
export function dstAwakeLine(dst: number, adj: number, text: TextOf): string {
  const name = findDstString(dst, text);
  const sign = adj > 0 ? '+' : '-';
  if (isDstRate(dst)) {
    if (dst === DST_ATTACKSPEED)
      return `\n${name} ${sign}${String(Math.abs(attackSpeedPercent(adj)))}% `;
    return `\n${name} ${sign}${String(Math.abs(Math.trunc(adj)))}%`;
  }
  return `\n${name} ${sign}${String(Math.abs(Math.trunc(adj)))}`;
}

/**
 * One effect of a worn named set (`PutSetItemOpt` step 4): `"\n%s: %s% +d%%"` (the `% +d` conversion prints the sign, so the
 * number follows the name directly) for a rate DST, `"\n%s: %s +%d"` otherwise.
 */
export function dstSetLine(dst: number, adj: number, label: string, text: TextOf): string {
  const name = findDstString(dst, text);
  return isDstRate(dst)
    ? `\n${label}: ${name}${signed(rateValue(dst, adj))}%`
    : `\n${label}: ${name} +${String(Math.trunc(adj))}`;
}

/** The four lines `DST_STAT_ALLUP` expands to in `PutBaseItemOpt` (STR, DEX, INT, STA), each `"\n%s%+d"`. */
export function statAllUpLines(adj: number, text: TextOf): string[] {
  return [DST_STR, DST_DEX, DST_INT, DST_STA].map(
    (d) => `\n${findDstString(d, text)}${signed(adj)}`,
  );
}

/** The blue `PutDestParam` colour (`D3DCOLOR_XRGB(0, 0, 255)`). */
export const DEST_PARAM_COLOR = 0xff0000ff;

/**
 * `CWndMgr::PutDestParam(dst1, dst2, adj1, adj2, str)`: up to two lines in blue, skipping `NULL_ID`, 0 and `DST_CHRSTATE`. The
 * sign is `+` unless the value is negative (then none is added; the minus is part of `%d`); `DST_ATTACKSPEED` through
 * `adj / 2 / 10`. Returns the runs to append.
 */
export function putDestParam(
  dst1: number,
  dst2: number,
  adj1: number,
  adj2: number,
  text: TextOf,
): RichRun[] {
  const runs: RichRun[] = [];
  const one = (dst: number, adj: number): void => {
    if (dst === 0xffff_ffff || dst === 0 || dst === DST_CHRSTATE) return;
    const sign = adj < 0 ? '' : '+';
    const name = findDstString(dst, text);
    const body = isDstRate(dst)
      ? `${sign}${String(dst === DST_ATTACKSPEED ? attackSpeedPercent(adj) : Math.trunc(adj))}%`
      : `${sign}${String(Math.trunc(adj))}`;
    runs.push({
      text: `\n${name}${body}`,
      color: DEST_PARAM_COLOR,
      bold: false,
      underline: false,
      strike: false,
    });
  };
  one(dst1, adj1);
  one(dst2, adj2);
  return runs;
}
