// The geometry of the skill tree window (`CWndSkillTreeEx::GetSkillPoint` :7333, `GetCalcImagePos` :7991, `SetActiveSlot` :6634;
// Plan/42 §19.4 D, WP-UI-38): where each skill icon sits, how the four sections (vagrant, expert, professional, hero) move up when a
// section is folded, how tall the window is. Pure functions of the layout rects and the four "section open" flags, so the numbers
// are table-tested without a window.
//
// Coordinates are the window's client coordinates (the layout's control rects). The original draws an icon at the skill's rect
// with `top - 2` (27 x 27) and adds the gap of the folded sections above it.

import {
  JTYPE_BASE,
  JTYPE_EXPERT,
  JTYPE_HERO,
  JTYPE_MASTER,
  JTYPE_PRO,
} from '../../../gen/defs/job';
import type { Point, Rect } from '../../layout/geometry';
import { LEGEND_POSITIONS, TIER_POSITIONS } from './skillPositions';

/** `nExpertGapX / nExpertGapY` and `nProGapX / nProGapY` of `GetSkillPoint`: the table coordinates are relative to these origins. */
export const EXPERT_GAP: Point = { x: 9, y: 90 };
export const PRO_GAP: Point = { x: 8, y: 228 };

/** The size an icon is drawn with (`Render(p2DRender, pt, CPoint(27, 27))`) and the size of the hit rect. */
export const ICON_SIZE = 27;
/** The preview icon of the selected skill (`WIDC_CUSTOM1`, natural size). */
export const PREVIEW_SIZE = 32;
/** How much a folded section shortens the window (`dwValue` of `SetActiveSlot`). */
export const FOLD_VAGRANT = 48;
export const FOLD_TIER = 144;
export const FOLD_HERO = 48;
/** The window height without the hero section (`rect.bottom -= 100` in `OnInitialUpdate`). */
export const HERO_SECTION_HEIGHT = 100;

/** The four "section is open" flags `m_bSlot[0..3]`: vagrant, expert, professional, hero. */
export type SectionFlags = readonly [boolean, boolean, boolean, boolean];

/** The layout rects `GetSkillPoint` reads (`GetWndCtrl`: the rects of the resource, never the moved ones). */
export interface TreeLayout {
  /** `WIDC_STATIC5` top-left: the origin of the expert table. */
  readonly expert: Point;
  /** `WIDC_STATIC7` top-left: the origin of the professional table. */
  readonly pro: Point;
  /** `WIDC_CUSTOM2..4`: the three vagrant skills. */
  readonly vagrant: readonly [Rect, Rect, Rect];
  /** `WIDC_CUSTOM5`: the master skill. */
  readonly master: Rect;
  /** `WIDC_CUSTOM6`: the hero skill. */
  readonly hero: Rect;
}

/** The three vagrant skills in the order of `WIDC_CUSTOM2..4` (`SI_VAG_ONE_CLEANHIT`, `_BRANDISH`, `_OVERCUTTER`). */
export const VAGRANT_SKILL_ORDER: readonly number[] = [1, 2, 3];

const rect = (l: number, t: number, w: number, h: number): Rect => ({
  l,
  t,
  r: l + w,
  b: t + h,
});

/**
 * `GetSkillPoint(dwSkillID, rect)`: the rect of the skill in the unfolded window (before the `top -= 2` of the drawing code and the
 * gap of the folded sections), or `null` for a skill that has no place in the window (the event skills, `return FALSE`).
 */
export function skillPoint(skillId: number, kind1: number, layout: TreeLayout): Rect | null {
  const v = VAGRANT_SKILL_ORDER.indexOf(skillId);
  if (v >= 0 && kind1 === JTYPE_BASE) {
    const base = layout.vagrant[v];
    return base ? { l: base.l, t: base.t + 1, r: base.r, b: base.b } : null;
  }
  const legend = LEGEND_POSITIONS.get(skillId);
  if (legend !== undefined) {
    const r = legend === 0 ? layout.master : layout.hero;
    return { ...r };
  }
  const tier = TIER_POSITIONS.get(skillId);
  if (tier) {
    const [x, y, k] = tier;
    const gap = k === 1 ? EXPERT_GAP : PRO_GAP;
    // `pt` is the top-left of the section's background static for the jobs types EXPERT and PRO of the skill's `dwItemKind1`
    const origin =
      kind1 === JTYPE_EXPERT ? layout.expert : kind1 === JTYPE_PRO ? layout.pro : { x: 0, y: 0 };
    return rect(x - gap.x + origin.x, y - gap.y + origin.y, 24, 24);
  }
  return null;
}

/**
 * `GetCalcImagePos(kind)`: the vertical gap the folded sections above a skill's section leave (a negative number of pixels). The
 * vagrant section never moves, the expert one by the vagrant fold, the professional one by both folds, the master and hero skills by
 * all three.
 */
export function sectionGap(kind1: number, flags: SectionFlags): number {
  const [vagrant, expert, pro] = flags;
  switch (kind1) {
    case JTYPE_EXPERT:
      return vagrant ? 0 : -FOLD_VAGRANT;
    case JTYPE_PRO:
      return (vagrant ? 0 : -FOLD_VAGRANT) + (expert ? 0 : -FOLD_TIER);
    case JTYPE_MASTER:
    case JTYPE_HERO:
      return (vagrant ? 0 : -FOLD_VAGRANT) + (expert ? 0 : -FOLD_TIER) + (pro ? 0 : -FOLD_TIER);
    default:
      return 0;
  }
}

/** Is a skill of this tier drawn and hit (`OnDraw`: a folded section skips its skills; `m_bSlot[3]` hides master AND hero skills)? */
export function tierVisible(kind1: number, flags: SectionFlags): boolean {
  const [vagrant, expert, pro, hero] = flags;
  switch (kind1) {
    case JTYPE_BASE:
      return vagrant;
    case JTYPE_EXPERT:
      return expert;
    case JTYPE_PRO:
      return pro;
    case JTYPE_MASTER:
    case JTYPE_HERO:
      return hero;
    default:
      return true;
  }
}

/** The rect an icon is drawn and hit in: the skill's rect, `top - 2`, 27 x 27, moved by the gap. `null` = not drawn. */
export function iconRect(
  skillId: number,
  kind1: number,
  layout: TreeLayout,
  flags: SectionFlags,
): Rect | null {
  if (!tierVisible(kind1, flags)) return null;
  const r = skillPoint(skillId, kind1, layout);
  if (!r) return null;
  const gap = sectionGap(kind1, flags);
  return rect(r.l, r.t - 2 + gap, ICON_SIZE, ICON_SIZE);
}

/** The vertical offsets the controls below a folded section move by (`SetActiveSlot`). */
export interface ControlOffsets {
  /** `WIDC_STATIC3` (the expert header) and `WIDC_STATIC5` (the expert background). */
  readonly expertHeader: number;
  /** `WIDC_STATIC6` (the professional header) and `WIDC_STATIC7` (the professional background). */
  readonly proHeader: number;
  /** `WIDC_STATIC1` (the hero header) and `WIDC_STATIC11` (the hero background). */
  readonly heroHeader: number;
}

export function controlOffsets(flags: SectionFlags): ControlOffsets {
  const [vagrant, expert, pro] = flags;
  const v = vagrant ? 0 : -FOLD_VAGRANT;
  const e = expert ? 0 : -FOLD_TIER;
  const p = pro ? 0 : -FOLD_TIER;
  return { expertHeader: v, proHeader: v + e, heroHeader: v + e + p };
}

/**
 * The window height: the layout's height, minus the hero section for a character that cannot have one, minus every folded section
 * (the original adds and subtracts these step by step, with 90 instead of 100 when the hero section appears: FIX, the height is a
 * function of the flags here).
 */
export function windowHeight(layoutHeight: number, legend: boolean, flags: SectionFlags): number {
  const [vagrant, expert, pro, hero] = flags;
  let h = layoutHeight;
  if (!legend) h -= HERO_SECTION_HEIGHT;
  if (!vagrant) h -= FOLD_VAGRANT;
  if (!expert) h -= FOLD_TIER;
  if (!pro) h -= FOLD_TIER;
  if (legend && !hero) h -= FOLD_HERO;
  return h;
}

/** The `nLevel - 1 (+ 20)` frame of `icon_IconSkillLevel.inc` for a level: 0..19 plain, 20..39 for a level changed in the window. */
export function levelFrame(level: number, maxLevel: number, edited: boolean): number {
  const add = edited ? 20 : 0;
  return level < maxLevel ? level - 1 + add : 19 + add;
}
