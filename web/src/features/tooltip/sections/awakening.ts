// Sections 18-20 of the item tooltip: socketed cards, the awakening / blessing lines and the ultimate weapon's added options
// (Plan/45 §10.2; `PutPiercingOpt`, `PutAwakeningBlessing`, `PutAddedOpt`; `src:_Interface/WndManager.cpp:4793-4915, 5262-5360`).

import {
  DST_ADJDEF,
  DST_ATKPOWER,
  DST_DEX,
  DST_HP_MAX,
  DST_INT,
  DST_MELEE_STEALHP,
  DST_PVP_DMG,
  DST_STA,
  DST_STR,
} from '../../../gen/defs/attribute';
import { IK3_EGG, IK3_PET } from '../../../gen/defs/itemkind';
import {
  PARTS_CAP,
  PARTS_CLOAK,
  PARTS_CLOTH,
  PARTS_FOOT,
  PARTS_GLOVE,
  PARTS_HAND,
  PARTS_HAT,
  PARTS_RWEAPON,
  PARTS_SHIELD,
  PARTS_UPPER_BODY,
  PARTS_BOOTS,
} from '../../../gen/defs/neuz';
import {
  TID_GAME_AWAKENING,
  TID_GAME_AWAKENNIG_PET_00,
  TID_GAME_BLESSING_CAPTION,
  TID_GAME_BLESSING_WARNING,
} from '../../../gen/defs/text';
import { DEFAULT_TEXT_COLOR } from '../../../ui/text/markup';
import { MAX_AWAKE_COLOR } from '../colors';
import { dstAwakeLine, dstPlusLine, findDstString } from '../dst';
import type { ComposeCx } from '../model';
import { ESSTY } from '../rich';
import type { RandomOptionKind } from '../../../rules/types';

/** `PutPiercingOpt`: the merged parameters of the socketed cards, in the piercing colour. */
export function putPiercingOpt(cx: ComposeCx): void {
  const dsts = cx.numbers?.piercingDsts;
  if (!dsts) return;
  for (const [dst, adj] of dsts) {
    cx.b.add(dstPlusLine(dst, adj, cx.env.text), cx.colors.piercing);
  }
}

/**
 * `GetRandomOptionKind(item)` from the item's `parts`, `IK3` and (eggs) pet level: with `only_armor_awakes` only suit, gauntlet,
 * boots and helmet awaken (mirrors `flyff_rules::item::random_option::random_option_kind`).
 */
export function randomOptionKind(
  parts: number,
  ik3: number,
  eggLevelCOrBetter: boolean,
  onlyArmorAwakes: boolean,
): RandomOptionKind | null {
  if (
    parts === PARTS_UPPER_BODY ||
    parts === PARTS_HAND ||
    parts === PARTS_FOOT ||
    parts === PARTS_CAP
  ) {
    return 'awakening';
  }
  if (onlyArmorAwakes) return null;
  if (parts === PARTS_RWEAPON || parts === PARTS_SHIELD) return 'awakening';
  if (
    parts === PARTS_CLOTH ||
    parts === PARTS_CLOAK ||
    parts === PARTS_HAT ||
    parts === PARTS_GLOVE ||
    parts === PARTS_BOOTS
  ) {
    return 'blessing';
  }
  if (ik3 === IK3_EGG && eggLevelCOrBetter) return 'systemPet';
  if (ik3 === IK3_PET) return 'eatPet';
  return null;
}

/** The kind of the tooltip's item (`GetRandomOptionKind`). */
export function itemRandomKind(cx: ComposeCx): RandomOptionKind | null {
  const { prop, item, env } = cx;
  const parts = prop.parts === 0xffff_ffff ? 0xffff_ffff : prop.parts;
  const eggC = item.pet !== undefined && item.pet.level >= 2;
  // `PARTS_*` are small numbers; the prop's NULL_ID never matches one
  return randomOptionKind(parts, prop.item_kind3, eggC, env.features().only_armor_awakes);
}

/**
 * `PutAwakeningBlessing`: the caption of an awakenable item with no lines yet, the lines (`"\n%s %c%d"`) in the awakening /
 * blessing colour (`0xFFED4A62` for a line at the table maximum with `max_awake_color`; struck out while a protected choice is
 * pending), the blessing warning.
 */
export function putAwakeningBlessing(cx: ComposeCx): void {
  const { numbers, env, b, colors } = cx;
  const kind = itemRandomKind(cx);
  if (kind === null) return;
  const lines = numbers?.randomOptions ?? [];
  const size = lines.length;
  const t = env.text;
  let blessing = false;
  if (kind === 'awakening') {
    if (size === 0) b.add(`\n"${t(TID_GAME_AWAKENING)}"`, colors.awakening);
  } else if (kind === 'blessing') {
    if (size > 0) {
      b.add(`\n${t(TID_GAME_BLESSING_CAPTION)}`, colors.blessing);
      blessing = true;
    }
  } else if (size === 0) {
    b.add(`\n"${t(TID_GAME_AWAKENNIG_PET_00)}"`, colors.awakening);
  }
  const protectedChoice = numbers?.awakeningProtected === true;
  for (const line of lines) {
    const text = dstAwakeLine(line.dst, line.adj, t);
    const atMax = env.features().max_awake_color && line.maxAdj !== 0 && line.maxAdj <= line.adj;
    if (kind === 'awakening') {
      b.add(
        text,
        atMax ? MAX_AWAKE_COLOR : colors.awakening,
        protectedChoice ? ESSTY.STRIKETHROUGH : 0,
      );
    } else {
      b.add(text, atMax ? MAX_AWAKE_COLOR : colors.blessing);
    }
  }
  if (blessing) {
    b.add(`\n${t(TID_GAME_BLESSING_WARNING)}`, colors.blessingWarning);
  }
}

/** `nAddedOptDST[9]` of `PutAddedOpt`: the ultimate options the tooltip knows. */
const ADDED_OPT_DST: readonly number[] = [
  DST_HP_MAX,
  DST_ATKPOWER,
  DST_ADJDEF,
  DST_MELEE_STEALHP,
  DST_PVP_DMG,
  DST_STR,
  DST_STA,
  DST_DEX,
  DST_INT,
];

/**
 * `PutAddedOpt` (`__EXT_PIERCING`): the ultimate weapon's gem totals in DST order, `+N` (`+N%` for life steal and PvP damage) in
 * the red added-option colour. With `improved_ulti_gems` only the four main stats are listed.
 */
export function putAddedOpt(cx: ComposeCx): void {
  const gems = cx.numbers?.gemDsts;
  if (!gems) return;
  const improved = cx.env.features().improved_ulti_gems;
  for (const [dst, value] of [...gems].sort((a, c) => a[0] - c[0])) {
    const i = ADDED_OPT_DST.indexOf(dst);
    if (i < 0) continue;
    if (improved && i < 5) continue;
    const name = findDstString(dst, cx.env.text);
    const text = i === 3 || i === 4 ? `\n${name}+${String(value)}%` : `\n${name}+${String(value)}`;
    cx.b.add(text, cx.colors.addedOpt[6]);
  }
}

export { DEFAULT_TEXT_COLOR };
