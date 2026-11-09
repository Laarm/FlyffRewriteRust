// The tooltips of the buff icons (`CWndWorld::RenderBuffIcon / RenderExpBuffIcon / RenderSMBuff`, `CWndBuffStatus::RenderBuffIcon`,
// ref/22 §17 C, Plan/45 §12). Rich runs for `ui.tooltip.put`:
//
//   name (bold; green `0xff2fbe6d`, dark green for rare 102, violet for 103, blue for the feed pockets) [+ "   Lvl N" for skills]
//   the special line of the item (the smelting protectors "usable on demand", the party summon time and hint, the feed pocket's
//   period, count and description) or the time "DD:HH:MM:SS" / "HH:MM:SS" / "MM:SS" ... the description (`szCommand`) ... the
//   blue DST lines of the prop and (for skills) of the add-skill row of the level ... [the status window: the time LAST]

import type { ClientItem } from '../../data/clientData';
import {
  II_PET_EGG,
  II_SYS_SYS_SCR_AWAKESAFE,
  II_SYS_SYS_SCR_PARTYSUMMON,
  II_SYS_SYS_SCR_PET_FEED_POCKET,
  II_SYS_SYS_SCR_PET_FEED_POCKET02,
  II_SYS_SYS_SCR_RETURN,
  II_SYS_SYS_SCR_SMELPROT,
  II_SYS_SYS_SCR_SMELPROT2,
  II_SYS_SYS_SCR_SMELPROT3,
  II_SYS_SYS_SCR_SMELPROT4,
  II_SYS_SYS_SCR_SMELTING,
  II_SYS_SYS_SCR_SMELTING2,
  II_SYS_SYS_SCR_SUPERSMELTING,
} from '../../gen/defs/item';
import {
  SI_ASS_CHEER_STONEHAND,
  SI_ASS_HEAL_PREVENTION,
  SI_MAG_EARTH_LOOTING,
  SI_RIG_MASTER_BLESSING,
} from '../../gen/defs/skill';
import {
  IK2_BUFF2,
  IK3_ANGEL_BUFF,
  IK3_COUPLE_BUFF,
  IK3_EGG,
  IK3_GUILD_BUFF,
  IK3_PET,
  IK3_TS_BUFF,
} from '../../gen/defs/itemkind';
import {
  TID_GAME_COND_USE,
  TID_GAME_DEMOL_USE,
  TID_GAME_EXP_COUTMSG0,
  TID_GAME_EXP_COUTMSG1,
  TID_GAME_EXP_COUTMSG2,
  TID_GAME_EXP_COUTMSG3,
  TID_GAME_EXPITEM_TOOLTIP,
  TID_GAME_EXPITEM_TOOLTIP1,
  TID_GAME_PET_FEED_COUNT,
  TID_GAME_SUMMON_BUFF_ICON,
  TID_PK_LIMIT_DAY,
  TID_PK_LIMIT_HOUR,
  TID_PK_LIMIT_MINUTE,
  TID_PK_LIMIT_SECOND,
  TID_TOOLTIP_PERIOD,
} from '../../gen/defs/text';
import type { RichRun } from '../../ui/text/markup';
import { putDestParam } from '../tooltip/dst';
import { sprintf } from '../tooltip/printf';
import { ESSTY, RichBuilder, timeSpan } from '../tooltip/rich';
import { type Buff, remainOf } from './buffSet';
import type { DisplayEnv } from './display';
import { clockText, spanOfMs } from './format';
import { expFamilyOf } from './ids';
import { SM_ITEM_ID } from './sm';
import { BUFF_EQUIP, BUFF_SKILL } from './types';

export interface AddSkillDst {
  readonly dest_param: readonly [number, number];
  readonly adj_param_val: readonly [number, number];
}

export interface TooltipEnv extends DisplayEnv {
  addSkill(subDefine: number, level: number): AddSkillDst | undefined;
  /** `GetLevel()` of the player. */
  playerLevel(): number;
  /** `GetItemNumForClient(II_SYS_SYS_FEED_01)`. */
  feedCount(): number;
  /** The unix expiry of the active feed pocket in the bag (`m_dwKeepTime`), `null` when none is active. */
  feedPocketEnd(): number | null;
}

/** The skills whose add-skill row is not listed (`SI_RIG_MASTER_BLESSING` ..). */
const NO_ADD_SKILL = new Set<number>([
  SI_RIG_MASTER_BLESSING,
  SI_ASS_CHEER_STONEHAND,
  SI_MAG_EARTH_LOOTING,
  SI_ASS_HEAL_PREVENTION,
]);

const SMELT_PROTECT = new Set<number>([
  II_SYS_SYS_SCR_SMELPROT,
  II_SYS_SYS_SCR_SMELPROT2,
  II_SYS_SYS_SCR_SMELPROT3,
  II_SYS_SYS_SCR_SMELPROT4,
  II_SYS_SYS_SCR_SMELTING,
  II_SYS_SYS_SCR_SMELTING2,
]);

/** The name colour of a buff: dark green for rare 102, violet for 103, the pocket blue, else green. */
export function nameColor(prop: ClientItem): number {
  if (prop.item_rare === 102) return 0xff005d00;
  if (prop.item_rare === 103) return 0xffb600ff;
  if (prop.id === II_SYS_SYS_SCR_PET_FEED_POCKET || prop.id === II_SYS_SYS_SCR_PET_FEED_POCKET02)
    return 0xff2e70a9;
  return 0xff2fbe6d;
}

function dstRuns(prop: ClientItem, env: DisplayEnv): RichRun[] {
  return [
    ...putDestParam(
      prop.dest_param[0],
      prop.dest_param[1],
      prop.adj_param_val[0],
      prop.adj_param_val[1],
      env.text,
    ),
    ...putDestParam(prop.dest_param[2], 0, prop.adj_param_val[2], 0, env.text),
  ];
}

/** `str.Format(TID_PK_LIMIT_*)` of the feed pocket's period: days + 1, hours, minutes over one, else seconds. */
function pocketPeriod(env: DisplayEnv, endUnix: number): string {
  const t = endUnix - env.nowUnix();
  if (t <= 0) return '';
  const ct = timeSpan(t);
  if (ct.days) return sprintf(env.text(TID_PK_LIMIT_DAY), [ct.days + 1]);
  if (ct.hours) return sprintf(env.text(TID_PK_LIMIT_HOUR), [ct.hours]);
  if (ct.minutes > 1) return sprintf(env.text(TID_PK_LIMIT_MINUTE), [ct.minutes]);
  return sprintf(env.text(TID_PK_LIMIT_SECOND), [ct.seconds]);
}

/** The tooltip of a skill buff of the status window (`CWndBuffStatus::RenderBuffIcon`): the time comes last. */
export function statusTooltip(env: TooltipEnv, b: Buff, party = false): RichRun[] | null {
  const prop = party ? env.partySkill(b.id) : env.skill(b.id);
  if (!prop) return null;
  const rb = new RichBuilder();
  const color = nameColor(prop);
  rb.add(prop.name, color, ESSTY.BOLD);
  rb.add(`   Lvl ${String(b.level)}`, color, ESSTY.BOLD);
  let time = '';
  if (b.total > 0) time = `\n${clockText(spanOfMs(remainOf(b, env.now())))}`;
  rb.add(`\n${prop.description}`, 0xff000000);
  rb.addRuns(
    putDestParam(
      prop.dest_param[0],
      prop.dest_param[1],
      prop.adj_param_val[0],
      prop.adj_param_val[1],
      env.text,
    ),
  );
  if (b.type === BUFF_SKILL && !NO_ADD_SKILL.has(b.id)) {
    const row = env.addSkill(prop.sub_define, b.level);
    if (row)
      rb.addRuns(
        putDestParam(
          row.dest_param[0],
          row.dest_param[1],
          row.adj_param_val[0],
          row.adj_param_val[1],
          env.text,
        ),
      );
  }
  rb.add(time, 0xff000000);
  return rb.runs;
}

/** The tooltip of an item, pet, equip or item2 buff (or a party skill) of the world strip. */
export function hudTooltip(env: TooltipEnv, b: Buff, party = false): RichRun[] | null {
  const prop =
    b.type === BUFF_SKILL ? (party ? env.partySkill(b.id) : env.skill(b.id)) : env.item(b.id);
  if (!prop) return null;
  const rb = new RichBuilder();
  rb.add(prop.name, nameColor(prop), ESSTY.BOLD);
  const remain = remainOf(b, env.now());
  let special = false;
  if (prop.item_kind3 === IK3_ANGEL_BUFF) {
    rb.add('   0%', 0xff6464ff, ESSTY.BOLD);
    special = true;
  } else if (
    prop.item_kind3 === IK3_EGG ||
    prop.item_kind3 === IK3_COUPLE_BUFF ||
    prop.item_kind3 === IK3_TS_BUFF ||
    prop.item_kind3 === IK3_GUILD_BUFF
  ) {
    special = true;
  }
  if (prop.item_kind3 === IK3_PET) special = true;
  let str = '';
  const id = prop.id;
  if (SMELT_PROTECT.has(id) || id === II_SYS_SYS_SCR_SUPERSMELTING) {
    str = `\n${env.text(TID_GAME_DEMOL_USE)}`;
  } else if (id === II_SYS_SYS_SCR_RETURN || id === II_SYS_SYS_SCR_AWAKESAFE) {
    // the return scroll (a double click sends it back) and the awakening safe scroll have no time line
  } else if (id === II_SYS_SYS_SCR_PARTYSUMMON) {
    const ct = spanOfMs(remain);
    str = `\n${clockText(ct)}\n${env.text(TID_GAME_SUMMON_BUFF_ICON)}`;
  } else if (id === II_SYS_SYS_SCR_PET_FEED_POCKET) {
    const pocket = env.feedPocketEnd();
    if (pocket !== null) {
      rb.add('\n', 0xff000000);
      rb.add(`${pocketPeriod(env, pocket)}${env.text(TID_TOOLTIP_PERIOD)}`, 0xffff1414);
      rb.add('\n', 0xff000000);
      rb.add(`${env.text(TID_GAME_PET_FEED_COUNT)} ${String(env.feedCount())}`, 0xff3232cd);
      rb.add('\n', 0xff000000);
      rb.add(prop.description, 0xffb200ff);
    }
  } else if (!special && b.type !== BUFF_EQUIP) {
    if (b.total > 0) str = `\n${clockText(spanOfMs(remain))}`;
    else if (prop.item_kind2 === IK2_BUFF2) {
      str = `\n${clockText(timeSpan(Math.max(0, b.level - env.nowUnix())))}`;
    }
  }
  if (id !== II_SYS_SYS_SCR_PET_FEED_POCKET && id !== II_PET_EGG)
    rb.add(`\n${prop.description}`, 0xff000000);
  if (id === II_SYS_SYS_SCR_PET_FEED_POCKET02) {
    rb.add('\n', 0xff000000);
    rb.add(`${env.text(TID_GAME_PET_FEED_COUNT)} ${String(env.feedCount())}`, 0xff3232cd);
  }
  rb.addRuns(dstRuns(prop, env));
  if (b.type === BUFF_SKILL) {
    const row = env.addSkill(prop.sub_define, b.level);
    if (row)
      rb.addRuns(
        putDestParam(
          row.dest_param[0],
          row.dest_param[1],
          row.adj_param_val[0],
          row.adj_param_val[1],
          env.text,
        ),
      );
  }
  if (id !== II_SYS_SYS_SCR_PET_FEED_POCKET && id !== II_PET_EGG) rb.add(str, 0xff000000);
  return rb.runs;
}

/** The tooltip of an experience-scroll family icon (`RenderExpBuffIcon`): the bonus and the time of each scroll. */
export function expTooltip(env: TooltipEnv, own: readonly Buff[], kind: number): RichRun[] | null {
  const family = own.map((b) => expFamilyOf(b.id)).find((f) => f?.kind === kind);
  if (!family) return null;
  const prop = env.item(family.base);
  if (!prop) return null;
  const scrolls = family.overlap
    ? own.filter((b) => family.ids.includes(b.id))
    : own.filter((b) => b.id === family.base).slice(0, 1);
  const count = scrolls.length;
  const level = env.playerLevel();
  let factor = 1;
  if (count === 1) factor *= 1.5;
  else if (count === 2) factor *= family.special || level < 61 ? 2.0 : 1.5;
  else if (count === 3) factor *= family.special || level < 41 ? 2.5 : level < 61 ? 2.0 : 1.5;
  const rb = new RichBuilder();
  rb.add(prop.name, 0xff0000ff);
  rb.add('\n', 0xff000000);
  rb.add(env.text(TID_GAME_EXPITEM_TOOLTIP), 0xff000000);
  rb.add(` ${String(Math.trunc((factor - 1) * 100))}% `, 0xffff5a50);
  rb.add(env.text(TID_GAME_EXPITEM_TOOLTIP1), 0xff000000);
  rb.add('\n', 0xff000000);
  rb.add(env.text(TID_GAME_EXP_COUTMSG0), 0xff000000);
  scrolls.forEach((b, i) => {
    const tid =
      i === 0 ? TID_GAME_EXP_COUTMSG1 : i === 1 ? TID_GAME_EXP_COUTMSG2 : TID_GAME_EXP_COUTMSG3;
    const ct = spanOfMs(remainOf(b, env.now()));
    const clock = ct.hours >= 1 ? clockText(ct) : clockText(ct);
    rb.add(`\n  ${env.text(tid)} ${clock} `, 0xff000000);
  });
  return rb.runs;
}

/** The tooltip of a state-mode item icon (`RenderSMBuff`). */
export function smTooltip(env: TooltipEnv, kind: number, remainSec: number): RichRun[] | null {
  const itemId = SM_ITEM_ID[kind] ?? 0;
  const prop = itemId !== 0 ? env.item(itemId) : undefined;
  if (!prop) return null;
  const rb = new RichBuilder();
  rb.add(prop.name, nameColor(prop), ESSTY.BOLD);
  const ct = spanOfMs(remainSec * 1000);
  rb.add(`\n${prop.circle_time === 1 ? env.text(TID_GAME_COND_USE) : clockText(ct)}`, 0xff000000);
  return rb.runs;
}
