// The couple arithmetic of the client (Plan/45 §24.2, Plan/21 §11; WP-CFEAT-40): `CCoupleProperty::GetLevel`, `GetExperienceRate`, `GetSkill` and the minimum fill of the EXP gauge
// of `CWndCoupleTabInfo::OnDraw` (`src:_Common/couple.cpp:340-390`, `src:_Interface/WndField.cpp:23880-23960`). Pure, no store: the Rust twin is `flyff_rules::social::couple_level`
// (the goldens 13 -> 1, 14 -> 2, 647 -> 20, 648 -> 21 are shared).

/** `CCouple::eMaxLevel`: a couple stops gaining EXP at this level. */
export const COUPLE_MAX_LEVEL = 21;

/**
 * `CCoupleProperty::GetLevel(exp)`: the index of the FIRST threshold of `level_exp` with `exp < threshold`, or 1 when there is none. `level_exp[i]` is the EXP that reaches level `i + 1`;
 * KEEP: the first threshold wins, so the shipped file (its rows after level 31 drop to 500) is read in file order exactly as the original does. The index 0 (threshold 0) can never match a
 * non-negative EXP, so the level is at least 1.
 */
export function coupleLevel(levelExp: readonly number[], exp: number): number {
  const i = levelExp.findIndex((threshold) => exp < threshold);
  return i < 0 ? 1 : Math.max(1, i);
}

/** `CCoupleProperty::GetTotalExperience(level)` = `m_vExp[level - 1]`; `undefined` outside the table (the original read out of range in a release build). */
export function coupleTotal(levelExp: readonly number[], level: number): number | undefined {
  return levelExp[level - 1];
}

/**
 * `CCoupleProperty::GetExperienceRate(level, exp)`: `(exp - total(level)) / (total(level + 1) - total(level))` in single precision, `0` at `eMaxLevel` (and for a level the table does not
 * reach, where the original read garbage).
 */
export function coupleRate(levelExp: readonly number[], level: number, exp: number): number {
  if (level === COUPLE_MAX_LEVEL) return 0;
  const from = coupleTotal(levelExp, level);
  const to = coupleTotal(levelExp, level + 1);
  if (from === undefined || to === undefined) return 0;
  const required = to - from;
  if (required === 0) return 0;
  return Math.fround(Math.fround(exp - from) / Math.fround(required));
}

const MIN_LOW = Math.fround(0.04);
const MIN_HIGH = Math.fround(0.075);

/**
 * The fill the gauge shows for a rate: a couple that has barely started still shows a sliver (`if( fRate > 0.00f && fRate < 0.04f ) fRate = 0.05f; else if( fRate >= 0.04f && fRate <= 0.075f )
 * fRate = 0.075f;`).
 */
export function gaugeRate(rate: number): number {
  const r = Math.fround(rate);
  if (r > 0 && r < MIN_LOW) return Math.fround(0.05);
  if (r >= MIN_LOW && r <= MIN_HIGH) return MIN_HIGH;
  return r;
}

/** The pixel width of the fill in a bar `width` px wide: `(int)( rect.Width() * fRate )` in single precision, never wider than the bar. */
export function gaugeFill(width: number, rate: number): number {
  const w = Math.trunc(Math.fround(Math.fround(width) * Math.fround(rate)));
  return Math.max(0, Math.min(w, width));
}

/** A `skill_levels` row: `[couple level, level of each skill kind]`. */
export type SkillLevelRow = readonly [number, readonly number[]];

/**
 * `CCoupleProperty::GetSkill(level)`: the levels of the skill kinds at a couple level - the row with the greatest `level` that is at most the couple level ("a row holds from its level on").
 * Empty for a level below the first row. FIX: the original indexed `m_vSkills[ -1 ]` for an uncoupled player (level 0); the caller does not ask then.
 */
export function skillRowAt(rows: readonly SkillLevelRow[], level: number): readonly number[] {
  let best: SkillLevelRow | null = null;
  for (const row of rows) {
    if (row[0] <= level && (best === null || row[0] >= best[0])) best = row;
  }
  return best ? best[1] : [];
}
