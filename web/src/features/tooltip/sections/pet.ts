// Section 17 of the item tooltip: the pet an egg item holds and the feed pocket (Plan/45 §10.2; `PutPetInfo`, `PutPetFeedPocket`;
// `src:_Interface/WndManager.cpp:5380-5710`). The pet numbers come from the item record (`PetData`) and the pet tables of the
// `item-ext` bundle (`CPetProperty`: `MaxEnergy`, the avail-parameter rows).

import {
  DST_ADJDEF,
  DST_ATKPOWER,
  DST_DEX,
  DST_HP_MAX,
  DST_INT,
  DST_STA,
  DST_STR,
} from '../../../gen/defs/attribute';
import {
  TID_GAME_ABILITY,
  TID_GAME_CHARACTER_02,
  TID_GAME_PET_FEED_COUNT,
  TID_GAME_PET_FEEDPOCKET,
  TID_GAME_PET_FEEDPOCKET_USE,
  TID_GAME_PET_TOOLTIP_EXP,
  TID_GAME_PET_TOOLTIP_EXP_MAX,
  TID_GAME_PET_TOOLTIP_HP,
  TID_GAME_PET_TOOLTIP_LEVEL,
  TID_GAME_PET_TOOLTIP_LIFE,
  TID_GAME_PETGRADE_A,
  TID_GAME_PETGRADE_B,
  TID_GAME_PETGRADE_C,
  TID_GAME_PETGRADE_D,
  TID_GAME_PETGRADE_E,
  TID_GAME_PETGRADE_S,
  TID_GAME_PETINFO_DEAD,
  TID_GAME_POCKET_NOTUSING,
  TID_GAME_POCKETUSING,
  TID_PK_LIMIT_DAY,
  TID_PK_LIMIT_HOUR,
  TID_PK_LIMIT_MINUTE,
  TID_PK_LIMIT_SECOND,
  TID_TOOLTIP_ATKPOWER_VALUE,
  TID_TOOLTIP_DEFENCE,
  TID_TOOLTIP_DEX,
  TID_TOOLTIP_DST_HP_MAX,
  TID_TOOLTIP_INT,
  TID_TOOLTIP_PERIOD,
  TID_TOOLTIP_STA,
  TID_TOOLTIP_STR,
} from '../../../gen/defs/text';
import { DEFAULT_TEXT_COLOR, argb } from '../../../ui/text/markup';
import { ITEM_FLAG } from '../../inventory/items';
import { NULL_ID } from '../../inventory/info';
import type { ComposeCx } from '../model';
import { sprintf } from '../printf';
import { ESSTY, timeSpan } from '../rich';
import { itemName, putPetKind } from './head';

const PET_GRADE_TID: readonly number[] = [
  TID_GAME_PETGRADE_E,
  TID_GAME_PETGRADE_D,
  TID_GAME_PETGRADE_C,
  TID_GAME_PETGRADE_B,
  TID_GAME_PETGRADE_A,
  TID_GAME_PETGRADE_S,
];

/** `MAX_PET_EGG_EXP` / `MAX_PET_EXP`. */
const maxExp = (level: number): number => (level === 0 ? 50_000 : 100_000);

/** `PutPetInfo`: the egg's pet (grade, ability, level history, experience, life and energy). */
export function putPetInfo(cx: ComposeCx): void {
  const { item, prop, env, b, colors } = cx;
  const pet = item.pet;
  if (!pet) return;
  const t = env.text;
  // `pEdit->Empty()`: the whole tooltip is rebuilt as the pet card
  b.runs.length = 0;
  b.add(itemName(cx), colors.name0, ESSTY.BOLD);
  putPetKind(cx);
  if ((item.flag & ITEM_FLAG.EXPIRED) !== 0) {
    b.add(` ${t(TID_GAME_PETINFO_DEAD)}`, colors.resistSM1, ESSTY.BOLD);
  }
  const level = pet.level;
  b.add('\n', DEFAULT_TEXT_COLOR);
  b.add(
    `${t(TID_GAME_CHARACTER_02)} : ${t(PET_GRADE_TID[level] ?? TID_GAME_PETGRADE_E)}`,
    colors.resistSM,
  );
  const tables = env.petTables();
  if (level !== 0) {
    const avail = tables?.avail[pet.kind];
    // `GetAvailDestParam`: the sum of the per-level parameter of the avail level chosen at each level-up
    let param = 0;
    for (let l = 1; l <= level; l += 1) {
      const chosen = pet.availLevel[l - 1] ?? 0;
      param += avail?.values[chosen - 1] ?? 0;
    }
    const dst = avail?.dst ?? 0;
    const tid = abilityTid(dst);
    b.add('\n', DEFAULT_TEXT_COLOR);
    b.add(
      `${t(TID_GAME_ABILITY)} : ${tid === undefined ? '' : t(tid)} +${String(param)}`,
      colors.resistSM1,
    );
    b.add('\n', DEFAULT_TEXT_COLOR);
    b.add('(', argb(255, 0, 200, 255));
    for (let l = 1; l <= level; l += 1) {
      b.add(
        sprintf(t(TID_GAME_PET_TOOLTIP_LEVEL), [pet.availLevel[l - 1] ?? 0]),
        argb(255, 0, 127, 255),
      );
      if (l !== level) b.add('/', argb(255, 0, 200, 255));
    }
    b.add(')', argb(255, 0, 200, 255));
    petExpLine(cx, pet.exp, level);
    if (!cx.env.features().no_pet_life) {
      const maxEnergy = tables?.max_energy[level - 1] ?? 1;
      b.add('\n', DEFAULT_TEXT_COLOR);
      b.add(sprintf(t(TID_GAME_PET_TOOLTIP_LIFE), [pet.life]), argb(255, 255, 100, 100));
      b.add('\n', DEFAULT_TEXT_COLOR);
      b.add(sprintf(t(TID_GAME_PET_TOOLTIP_HP), [pet.energy, maxEnergy]), argb(255, 255, 10, 10));
    }
  } else {
    petExpLine(cx, pet.exp, level);
  }
  b.add('\n', DEFAULT_TEXT_COLOR);
  b.add(sprintf('%s', [prop.description]), colors.piercing);
}

/** The ability name of a pet's avail parameter (`GetAvailDestParam` switch). */
function abilityTid(dst: number): number | undefined {
  switch (dst) {
    case DST_STR:
      return TID_TOOLTIP_STR;
    case DST_DEX:
      return TID_TOOLTIP_DEX;
    case DST_INT:
      return TID_TOOLTIP_INT;
    case DST_STA:
      return TID_TOOLTIP_STA;
    case DST_ATKPOWER:
      return TID_TOOLTIP_ATKPOWER_VALUE;
    case DST_ADJDEF:
      return TID_TOOLTIP_DEFENCE;
    case DST_HP_MAX:
      return TID_TOOLTIP_DST_HP_MAX;
    default:
      return undefined;
  }
}

function petExpLine(cx: ComposeCx, exp: number, level: number): void {
  const { env, b } = cx;
  // `exp * 10000 / max` in 64-bit integers, then `/ 100.0f`
  const result = Math.trunc((exp * 10000) / maxExp(level));
  const percent = Math.fround(result / 100);
  const line =
    percent >= Math.fround(99.99)
      ? env.text(TID_GAME_PET_TOOLTIP_EXP_MAX)
      : sprintf(env.text(TID_GAME_PET_TOOLTIP_EXP), [percent]);
  b.add('\n', DEFAULT_TEXT_COLOR);
  b.add(line, argb(255, 120, 120, 220));
}

/** The remaining time of a feed pocket: the largest unit only (`days + 1` for days). */
function pocketRemaining(cx: ComposeCx, t: number): string {
  const { env } = cx;
  if (t <= 0) return '';
  const ct = timeSpan(t);
  const unit = (tid: number, n: number): string => sprintf(env.text(tid), [n]);
  if (ct.days !== 0) return unit(TID_PK_LIMIT_DAY, ct.days + 1);
  if (ct.hours !== 0) return unit(TID_PK_LIMIT_HOUR, ct.hours);
  if (ct.minutes !== 0) return unit(TID_PK_LIMIT_MINUTE, ct.minutes);
  return unit(TID_PK_LIMIT_SECOND, ct.seconds);
}

/** `PutPetFeedPocket`: the pocket card (`Empty()` first): name, state, remaining time, the feed count and the description. */
export function putPetFeedPocket(cx: ComposeCx): void {
  const { item, prop, env, b, colors } = cx;
  const t = env.text;
  b.runs.length = 0;
  if (item.keepTimeAt > 0) {
    b.add(prop.name, colors.name0, ESSTY.BOLD);
    const t0 = item.keepTimeAt - env.nowSec();
    const expired = (item.flag & ITEM_FLAG.EXPIRED) !== 0;
    if (env.feedPocketActive()) {
      b.add(` ${t(TID_GAME_POCKETUSING)}`, argb(255, 255, 0, 0));
      if (!expired) {
        cx.volatile = true;
        b.add('\n', DEFAULT_TEXT_COLOR);
        b.add(pocketRemaining(cx, t0) + t(TID_TOOLTIP_PERIOD), argb(255, 255, 20, 20));
      }
      b.add('\n', DEFAULT_TEXT_COLOR);
      b.add(`${t(TID_GAME_PET_FEED_COUNT)} ${String(env.feedCount())}`, argb(255, 50, 50, 205));
    } else {
      b.add(` ${t(TID_GAME_POCKET_NOTUSING)}`, argb(255, 255, 0, 0));
      if (!expired) {
        cx.volatile = true;
        b.add('\n', DEFAULT_TEXT_COLOR);
        b.add(pocketRemaining(cx, t0) + t(TID_TOOLTIP_PERIOD), colors.time);
      }
    }
    b.add('\n', DEFAULT_TEXT_COLOR);
    b.add(t(TID_GAME_PET_FEEDPOCKET), colors.piercing);
  } else {
    b.add(prop.name, colors.name0, ESSTY.BOLD);
    b.add('\n', DEFAULT_TEXT_COLOR);
    b.add(t(TID_GAME_PET_FEEDPOCKET_USE), colors.piercing);
  }
}

export { NULL_ID };
