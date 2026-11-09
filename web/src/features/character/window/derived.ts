// The numbers of the character window's "Detail" block from the rules' `DerivedStats` (Plan/45 §20.1; `CWndCharInfo::OnDraw`,
// `RenderATK`, `src:_Interface/WndField.cpp:3159-3530`, `:4408-4449`; WP-CFEAT-11). Pure functions: the module feeds them the
// flat derived record of `selfStore` (or a `stat_preview` result) and the DST parameters of the own replica.

import { DST_ATKPOWER, DST_ATKPOWER_RATE } from '../../../gen/defs/attribute';
import type { DerivedStatsJs } from '../../../rules';
import type { DerivedStats } from '../../../ui/windows/char/ports';

/** `SM_ATTACK_UP` and `SM_ATTACK_UP1` (`CmnHdr.h`): the attack-up item-mall scrolls multiply the shown attack by 1.2. */
export const SM_ATTACK_UP = 15;
export const SM_ATTACK_UP1 = 23;

/** What `RenderATK` adds to `(min + max) / 2`. */
export interface AtkExtras {
  /** `IsSMMode(SM_ATTACK_UP1) || IsSMMode(SM_ATTACK_UP)`. */
  readonly attackUp: boolean;
  /** `GetParam(DST_ATKPOWER_RATE, 0)`. */
  readonly rate: number;
  /** `GetParam(DST_ATKPOWER, 0)`. */
  readonly flat: number;
}

/**
 * `RenderATK`: `nATK = (min + max) / 2`; x 1.2 (`float`, truncated) under an attack-up scroll; plus `rate` percent when the rate is
 * positive; plus the weapon's plus damage (the original's `GetWeaponPlusDamage` returns 0) and `DST_ATKPOWER`; never below 0.
 */
export function shownAttack(min: number, max: number, x: AtkExtras): number {
  let atk = Math.trunc((min + max) / 2);
  if (x.attackUp) atk = Math.trunc(Math.fround(atk * Math.fround(1.2)));
  if (x.rate > 0) atk += Math.trunc((atk * x.rate) / 100);
  atk += x.flat;
  return atk < 0 ? 0 : atk;
}

/** `int(GetAttackSpeed() * 100) / 2` with the product in single precision. */
export function shownAttackSpeed(speed: number): number {
  return Math.trunc(Math.trunc(Math.fround(speed * Math.fround(100))) / 2);
}

/** The DST parameters `RenderATK` reads from the replica's `adj[95] ++ chg[95]` (`dst_view`), `0` without a replica. */
export function atkExtrasOf(adj: ArrayLike<number> | undefined, attackUp: boolean): AtkExtras {
  return {
    attackUp,
    rate: adj?.[DST_ATKPOWER_RATE] ?? 0,
    flat: adj?.[DST_ATKPOWER] ?? 0,
  };
}

/** The rules' derived numbers as the window prints them. `statistics` adds the eight extra lines (capped cast speed). */
export function derivedForWindow(
  d: DerivedStatsJs,
  extras: AtkExtras,
  statistics: boolean,
  maxCast: number,
): DerivedStats {
  const base: DerivedStats = {
    atk: shownAttack(d.attackMin, d.attackMax, extras),
    def: d.defenseShown,
    critical: d.criticalProb,
    attackSpeed: shownAttackSpeed(d.attackSpeed),
  };
  if (!statistics) return base;
  const s = d.statistics;
  return {
    ...base,
    speedRun: s.speedRun,
    criticalRate: s.criticalRate,
    // `GetSpellCastSpeed` is displayed capped at `gameplay.set_max_cast`
    spellCastSpeed: Math.min(s.spellCastSpeed, maxCast),
    pvmDmg: s.pvmDmg,
    hitRate: s.hitRate,
    blockMelee: s.blockMelee,
    blockRange: s.blockRange,
    pvpDmg: s.pvpDmg,
  };
}

/**
 * The flat derived record of `selfStore` (`statistics.speedRun` ...) back into the nested form; `null` before the first
 * `derived_stats` call of the session.
 */
export function unflattenDerived(
  flat: Readonly<Record<string, number>> | null,
): DerivedStatsJs | null {
  if (flat?.['attackMin'] === undefined) return null;
  const n = (k: string): number => flat[k] ?? 0;
  return {
    str: n('str'),
    sta: n('sta'),
    dex: n('dex'),
    int: n('int'),
    maxHp: n('maxHp'),
    maxMp: n('maxMp'),
    maxFp: n('maxFp'),
    hpRecovery: n('hpRecovery'),
    mpRecovery: n('mpRecovery'),
    fpRecovery: n('fpRecovery'),
    attackMin: n('attackMin'),
    attackMax: n('attackMax'),
    defenseShown: n('defenseShown'),
    hitRating: n('hitRating'),
    parry: n('parry'),
    criticalProb: n('criticalProb'),
    attackSpeed: n('attackSpeed'),
    castingAniSpeed: n('castingAniSpeed'),
    moveSpeed: n('moveSpeed'),
    statistics: {
      speedRun: n('statistics.speedRun'),
      criticalRate: n('statistics.criticalRate'),
      spellCastSpeed: n('statistics.spellCastSpeed'),
      hitRate: n('statistics.hitRate'),
      blockMelee: n('statistics.blockMelee'),
      blockRange: n('statistics.blockRange'),
      pvmDmg: n('statistics.pvmDmg'),
      pvpDmg: n('statistics.pvpDmg'),
    },
  };
}
