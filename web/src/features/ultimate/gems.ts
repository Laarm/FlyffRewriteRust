// The tables the ultimate windows read besides the item table (Plan/19 §12.7, Plan/45 §18.6; WP-CFEAT-26): which jewel a weapon level needs, the
// roles of the items, and the attribute a socketed jewel gives (the labels of the jewel removal window).

import {
  II_GEN_MAT_DIAMOND,
  II_GEN_MAT_DIAMOND01,
  II_GEN_MAT_EMERALD,
  II_GEN_MAT_MOONSTONE,
  II_GEN_MAT_MOONSTONE_1,
  II_GEN_MAT_ORICHALCUM01,
  II_GEN_MAT_ORICHALCUM01_1,
  II_GEN_MAT_ORICHALCUM02,
  II_GEN_MAT_RUBY,
  II_GEN_MAT_SAPPHIRE,
  II_GEN_MAT_TOPAZ,
  II_GEN_MAT_TOPAZ09,
} from '../../gen/defs/item';
import {
  TID_TOOLTIP_DST_ADJDEF,
  TID_TOOLTIP_DST_ATKPOWER,
  TID_TOOLTIP_DST_HP_MAX,
  TID_TOOLTIP_DST_MELEE_STEALHP,
  TID_TOOLTIP_DST_PVP_DMG,
  TID_TOOLTIP_DEX,
  TID_TOOLTIP_INT,
  TID_TOOLTIP_STA,
  TID_TOOLTIP_STR,
} from '../../gen/defs/text';
import type { UltimateRole } from '../../ui/windows/ultimate/ports';
import type { ItemTextColors } from '../tooltip/colors';

/** One row of `LEVEL_GEM` (`Ultimate_GemAbility.txt`): the jewel of a weapon level range. */
export interface LevelGemRow {
  readonly min: number;
  readonly max: number;
  readonly item: number;
}

/**
 * `LEVEL_GEM` of the shipped `Ultimate_GemAbility.txt`. The table is not part of the client bundles (Plan/19 open question 8: the server tables stay on
 * the server), so the client carries the five shipped rows; `gems.real.test.ts` compares them with the file when the Resource is at hand.
 */
export const LEVEL_GEM: readonly LevelGemRow[] = [
  { min: 60, max: 70, item: II_GEN_MAT_TOPAZ },
  { min: 71, max: 85, item: II_GEN_MAT_RUBY },
  { min: 86, max: 100, item: II_GEN_MAT_SAPPHIRE },
  { min: 101, max: 119, item: II_GEN_MAT_EMERALD },
  { min: 120, max: 120, item: II_GEN_MAT_DIAMOND },
];

/** `CUltimateWeapon::GetGemKind( limit_level )`: the jewel of the first row whose range holds the level, `null` when there is none. */
export function gemKind(level: number, table: readonly LevelGemRow[] = LEVEL_GEM): number | null {
  const row = table.find((r) => level <= r.max && level >= r.min);
  return row ? row.item : null;
}

/** The role of an item id in the ultimate windows. */
export function ultimateRole(itemId: number): UltimateRole | null {
  switch (itemId) {
    case II_GEN_MAT_ORICHALCUM01:
    case II_GEN_MAT_ORICHALCUM01_1:
      return 'orichalcum';
    case II_GEN_MAT_MOONSTONE:
    case II_GEN_MAT_MOONSTONE_1:
      return 'moonstone';
    case II_GEN_MAT_ORICHALCUM02:
      return 'shining';
    case II_GEN_MAT_DIAMOND:
    case II_GEN_MAT_EMERALD:
    case II_GEN_MAT_SAPPHIRE:
    case II_GEN_MAT_RUBY:
    case II_GEN_MAT_TOPAZ:
      return 'jewel';
    default:
      return null;
  }
}

/** The five jewel kinds per ability: `II_GEN_MAT_DIAMOND01` ... `II_GEN_MAT_TOPAZ09` are 45 items, five (one per jewel) for each of nine abilities. */
export const ABILITY_GROUP_SIZE = 5;
export const ABILITY_GROUPS = 9;

/** The ability group 1..9 of an ability item (`DIAMOND01` ... `TOPAZ09`), `0` for any other id. */
export function abilityGroup(itemId: number): number {
  if (itemId < II_GEN_MAT_DIAMOND01 || itemId > II_GEN_MAT_TOPAZ09) return 0;
  return Math.floor((itemId - II_GEN_MAT_DIAMOND01) / ABILITY_GROUP_SIZE) + 1;
}

/** The text of each ability group (`CWndRemoveJewel::OnDraw`): hit points, attack, defence, life steal, PvP damage, then STR, STA, DEX, INT. */
const GROUP_TEXT: readonly number[] = [
  TID_TOOLTIP_DST_HP_MAX,
  TID_TOOLTIP_DST_ATKPOWER,
  TID_TOOLTIP_DST_ADJDEF,
  TID_TOOLTIP_DST_MELEE_STEALHP,
  TID_TOOLTIP_DST_PVP_DMG,
  TID_TOOLTIP_STR,
  TID_TOOLTIP_STA,
  TID_TOOLTIP_DEX,
  TID_TOOLTIP_INT,
];

/** The first group `features.improved_ulti_gems` still shows: the four stats. */
export const FIRST_IMPROVED_GROUP = 6;

/** What the removal window prints for a jewel: the attribute and the colour of the added-option line, `null` for no text. */
export interface JewelText {
  readonly text: string;
  readonly color: number;
}

/**
 * The line of a socketed jewel. `improved` (`features.improved_ulti_gems` = `__IMPROVED_ULTI_GEMS`) drops the first five attributes: a jewel that
 * gives one has no text (the picture stays). The colour is `dwAddedOpt<group>` of the tooltip colour set.
 */
export function jewelText(
  abilityItemId: number,
  improved: boolean,
  colors: ItemTextColors,
  text: (tid: number) => string,
): JewelText | null {
  const group = abilityGroup(abilityItemId);
  if (group === 0) return null;
  if (improved && group < FIRST_IMPROVED_GROUP) return null;
  const tid = GROUP_TEXT[group - 1];
  const color = colors.addedOpt[group - 1];
  if (tid === undefined || color === undefined) return null;
  return { text: text(tid), color };
}
