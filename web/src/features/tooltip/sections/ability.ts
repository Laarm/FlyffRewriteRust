// Section 7 of the item tooltip: the ability lines of gear, medicine and jewelry (Plan/45 §10.2 rows 7a-7c; `PutItemMinMax`,
// `PutItemSpeed`, `PutItemResist`, `PutBaseResist`, `PutBaseItemOpt`, `PutRandomOpt`, `PutEnchantOpt`, `PutMedicine`;
// `src:_Interface/WndManager.cpp:4731-4790, 4958-5075, 5127-5260, 5740-5795`).

import {
  DST_ADJ_HITRATE,
  DST_ATKPOWER_RATE,
  DST_FP,
  DST_HP,
  DST_HP_MAX_RATE,
  DST_MP,
  DST_STAT_ALLUP,
} from '../../../gen/defs/attribute';
import {
  IK1_ARMOR,
  IK1_WEAPON,
  IK2_WEAPON_DIRECT,
  IK2_WEAPON_MAGIC,
} from '../../../gen/defs/itemkind';
import {
  TID_GAME_TOOLTIP_ATTACKRANGE2,
  TID_GAME_TOOLTIP_ATTACKSPEED,
  TID_GAME_TOOLTIP_DEFENSE_B,
  TID_GAME_TOOLTIP_EARTH_RES,
  TID_GAME_TOOLTIP_ELEC_RES,
  TID_GAME_TOOLTIP_FIRE_RES,
  TID_GAME_TOOLTIP_RECOVFP,
  TID_GAME_TOOLTIP_RECOVHP,
  TID_GAME_TOOLTIP_RECOVMP,
  TID_GAME_TOOLTIP_WATER_RES,
  TID_GAME_TOOLTIP_WIND_RES,
  TID_GAME_TOOLTIPBLOCKRATE,
  TID_GAME_FAST,
  TID_GAME_FASTEST,
  TID_GAME_NORMALS,
  TID_GAME_SLOW,
  TID_GAME_VERYFAST,
  TID_GAME_VERYSLOW,
  TID_GAME_TOOLTIP_MAXRECOVER,
  TID_UPGRADE_EARTH,
  TID_UPGRADE_ELECTRICITY,
  TID_UPGRADE_FIRE,
  TID_UPGRADE_WATER,
  TID_UPGRADE_WIND,
} from '../../../gen/defs/text';
import { DEFAULT_TEXT_COLOR } from '../../../ui/text/markup';
import { NULL_ID } from '../../inventory/info';
import { elementColor } from '../colors';
import { dstPlusLine, dstSignedLine, findDstString, signed, statAllUpLines } from '../dst';
import type { ComposeCx } from '../model';
import { sprintf } from '../printf';
import { ESSTY } from '../rich';

/** `GetATKSpeedString(fSpeed)` (`WndManager.cpp:110-125`): the thresholds are `double` literals compared with a `float`. */
export function attackSpeedText(cx: Pick<ComposeCx, 'env'>, speed: number): string {
  const f = Math.fround(speed);
  const t = (tid: number): string => cx.env.text(tid);
  if (f < 0.035) return t(TID_GAME_VERYSLOW);
  if (f < 0.05) return t(TID_GAME_SLOW);
  if (f < 0.07) return t(TID_GAME_NORMALS);
  if (f < 0.08) return t(TID_GAME_FAST);
  if (f < 0.17) return t(TID_GAME_VERYFAST);
  return t(TID_GAME_FASTEST);
}

/**
 * `PutItemMinMax`: the newline always, then (when the prop has an ability range and endurance) the label in the general colour and
 * ` min ~ max` in the effectiveness colour of `f = numbers.multiplier`.
 */
export function putItemMinMax(cx: ComposeCx): void {
  const { prop, env, b, colors, numbers } = cx;
  // the batched numbers have not arrived (the rules context is still loading): nothing numeric is invented
  if (!numbers) return;
  b.add('\n', DEFAULT_TEXT_COLOR);
  if (prop.ability_min === NULL_ID || prop.ability_max === NULL_ID || prop.endurance === NULL_ID)
    return;
  const f = Math.fround(numbers.multiplier);
  let color = colors.effective3;
  if (Math.fround(1.0) <= f) color = colors.effective0;
  else if (Math.fround(0.8) <= f) color = colors.effective1;
  else if (Math.fround(0.6) <= f) color = colors.effective2;
  const min = numbers.ability.min;
  const max = numbers.ability.max;
  const weapon = prop.item_kind2 === IK2_WEAPON_DIRECT || prop.item_kind2 === IK2_WEAPON_MAGIC;
  b.add(
    env.text(weapon ? TID_GAME_TOOLTIP_ATTACKRANGE2 : TID_GAME_TOOLTIP_DEFENSE_B),
    colors.general,
  );
  b.add(` ${String(min)} ~ ${String(max)}`, color);
}

/** `PutItemSpeed`: the attack-speed word of a weapon. */
export function putItemSpeed(cx: ComposeCx): void {
  const { prop, env, b, colors } = cx;
  if (prop.item_kind2 !== IK2_WEAPON_DIRECT && prop.item_kind2 !== IK2_WEAPON_MAGIC) return;
  // the loader keeps `=` as -1.0 in this column; the original compares with 0xffffffff
  if (prop.attack_speed < 0 || prop.attack_speed >= 4_294_967_295) return;
  b.add('\n', DEFAULT_TEXT_COLOR);
  b.add(
    sprintf(env.text(TID_GAME_TOOLTIP_ATTACKSPEED), [attackSpeedText(cx, prop.attack_speed)]),
    colors.general,
  );
}

const ELEMENT_TID: Readonly<Record<number, number>> = {
  1: TID_UPGRADE_FIRE,
  2: TID_UPGRADE_WATER,
  3: TID_UPGRADE_ELECTRICITY,
  4: TID_UPGRADE_WIND,
  5: TID_UPGRADE_EARTH,
};

/**
 * `PutItemResist`: ` +N` per element on gear (bold when a cash element stone is applied); for a non-gear item `"\n%s Lv%d"`.
 * KEEP: the element comes from the instance (`m_bItemResist`), so a card (always 0) would print a bare ` LvN`; the shipped cards are
 * `IK2_MATERIAL`, a kind the section switch never reaches, so no card tooltip shows the line.
 */
export function putItemResist(cx: ComposeCx): void {
  const { prop, item, env, b, colors } = cx;
  const element = item.itemResist;
  const tid = ELEMENT_TID[element];
  const name = tid === undefined ? '' : env.text(tid);
  const color = elementColor(colors, element);
  if (prop.item_kind1 === IK1_WEAPON || prop.item_kind1 === IK1_ARMOR) {
    if (name === '') return;
    const line = `\n${name}${signed(item.resistAbilityOption)}`;
    b.add(line, color, item.resistSmItemId !== 0 ? ESSTY.BOLD : 0);
  } else {
    b.add(`\n${name} Lv${String(prop.item_lv)}`, color);
  }
}

/** `PutBaseResist`: the element resistances of the prop, in the element colours. */
export function putBaseResist(cx: ComposeCx): void {
  const { prop, env, b, colors } = cx;
  const rows: readonly (readonly [number, number, number])[] = [
    [prop.item_resist_electricity, TID_GAME_TOOLTIP_ELEC_RES, colors.resistElectricity],
    [prop.item_resist_fire, TID_GAME_TOOLTIP_FIRE_RES, colors.resistFire],
    [prop.item_resist_water, TID_GAME_TOOLTIP_WATER_RES, colors.resistWater],
    [prop.item_resist_wind, TID_GAME_TOOLTIP_WIND_RES, colors.resistWind],
    [prop.item_resist_earth, TID_GAME_TOOLTIP_EARTH_RES, colors.resistEarth],
  ];
  for (const [value, tid, color] of rows) {
    if (value === 0) continue;
    b.add('\n', DEFAULT_TEXT_COLOR);
    b.add(sprintf(env.text(tid), [value]), color);
  }
}

/** `PutBaseItemOpt`: the three DST slots of the prop (`__PROP_0827`), then the accessory bonuses. */
export function putBaseItemOpt(cx: ComposeCx): void {
  const { prop, env, b, colors } = cx;
  const text = env.text;
  for (let i = 0; i < 3; i += 1) {
    const dst = prop.dest_param[i];
    if (dst === undefined || dst === NULL_ID) continue;
    const adj = prop.adj_param_val[i] ?? 0;
    if (dst === DST_STAT_ALLUP) {
      b.add(statAllUpLines(adj, text).join(''), colors.general);
    } else {
      b.add(dstSignedLine(dst, adj, text), colors.general);
    }
  }
  // `CAccessoryProperty::GetDst`: the upgrade-level bonuses of an accessory (empty for any other item)
  for (const [dst, adj] of cx.env.accessoryDsts(cx.item.itemId, cx.item.abilityOption)) {
    if (dst === DST_STAT_ALLUP) b.add(statAllUpLines(adj, text).join(''), colors.general);
    else b.add(dstSignedLine(dst, adj, text), colors.general);
  }
}

/** `PutRandomOpt`: the lines of the stat-bonus item ("of the Tiger") of a wearable. */
export function putRandomOpt(cx: ComposeCx): void {
  const { prop, env, b, colors, numbers } = cx;
  if (prop.parts === NULL_ID) return;
  const stat = numbers?.randomStat;
  if (!stat) return;
  for (const [dst, adj] of stat.dsts) {
    b.add(dstPlusLine(dst, adj, env.text), colors.randomOption);
  }
}

/** `PutEnchantOpt`: the armour +N set bonus on a worn set piece. */
export function putEnchantOpt(cx: ComposeCx): void {
  const row = cx.extra.armorSetBonus;
  if (!row) return;
  const { env, b, colors } = cx;
  const c = colors.enchantOption;
  if (row.hit_rate > 0) {
    b.add(`\n${findDstString(DST_ADJ_HITRATE, env.text)}+${String(row.hit_rate)}%`, c);
  }
  if (row.block > 0) {
    b.add(`\n${env.text(TID_GAME_TOOLTIPBLOCKRATE)}+${String(row.block)}%`, c);
  }
  if (row.max_hit_point_rate > 0) {
    b.add(`\n${findDstString(DST_HP_MAX_RATE, env.text)}+${String(row.max_hit_point_rate)}%`, c);
  }
  // without `better_armor_bonus` the original prints `nAddMagic`; the shipped `Setitem` table has no such column, so that
  // (non-default) branch prints nothing
  if (env.features().better_armor_bonus && row.attack_power_rate > 0) {
    b.add(`\n${findDstString(DST_ATKPOWER_RATE, env.text)}+${String(row.attack_power_rate)}%`, c);
  }
  if (row.added > 0) {
    b.add(`\n${findDstString(DST_STAT_ALLUP, env.text)}+${String(row.added)}`, c);
  }
}

/** `PutMedicine`: "recovers N HP/MP/FP" (and, without `remove_max_recover_rate`, the maximum recovery). */
export function putMedicine(cx: ComposeCx, dst: number, value: number): void {
  const { env, b, colors, prop } = cx;
  if (dst === NULL_ID) return;
  if (value !== -1) {
    const tid =
      dst === DST_MP
        ? TID_GAME_TOOLTIP_RECOVMP
        : dst === DST_HP
          ? TID_GAME_TOOLTIP_RECOVHP
          : dst === DST_FP
            ? TID_GAME_TOOLTIP_RECOVFP
            : 0;
    if (tid !== 0) {
      b.add('\n', DEFAULT_TEXT_COLOR);
      b.add(sprintf(env.text(tid), [value]), colors.general);
    }
  }
  if (!env.features().remove_max_recover_rate && prop.ability_min !== NULL_ID) {
    b.add('\n', DEFAULT_TEXT_COLOR);
    b.add(sprintf(env.text(TID_GAME_TOOLTIP_MAXRECOVER), [prop.ability_min]), colors.general);
  }
}
