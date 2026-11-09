// Section 7d/7e and 8 of the item tooltip: vis, the seal-character scroll, the disguise DSTs, the eat pet's active skill and the vis
// pet's totals (Plan/45 §10.2; `PutNeededVis`, `PutSealChar`, `PutVisPetInfo`, the `IK2_GMTEXT` / `IsEatPet` blocks of
// `MakeToolTipText`; `src:_Interface/WndManager.cpp:5560-5640, 7552-7732, 6420-6520`).

import { MAX_MASTER, MAX_PROFESSIONAL } from '../../../gen/defs/job';
import {
  TID_GAME_BUFFPET_REQUIRE,
  TID_GAME_TOOLTIP_MARK_HERO,
  TID_GAME_TOOLTIP_MARK_MASTER,
  TID_GAME_TOOLTIP_TIMEOUT,
  TID_PK_LIMIT_DAY,
  TID_PK_LIMIT_HOUR,
  TID_PK_LIMIT_MINUTE,
  TID_PK_LIMIT_SECOND,
  TID_TOOLTIP_CHARDEX,
  TID_TOOLTIP_CHARINT,
  TID_TOOLTIP_CHARJOB,
  TID_TOOLTIP_CHARLEVEL,
  TID_TOOLTIP_CHARNAME,
  TID_TOOLTIP_CHARPOINT,
  TID_TOOLTIP_CHARSTA,
  TID_TOOLTIP_CHARSTAT,
  TID_TOOLTIP_CHARSTR,
} from '../../../gen/defs/text';
import { DEFAULT_TEXT_COLOR, argb } from '../../../ui/text/markup';
import { NULL_ID } from '../../inventory/info';
import { dstSignedLine, putDestParam } from '../dst';
import type { ComposeCx } from '../model';
import { sprintf } from '../printf';
import { timeSpan } from '../rich';

const RED = 0xffff0000;

/** `PutNeededVis`: the vis a vis item needs (red while the active vis pet lacks them). */
export function putNeededVis(cx: ComposeCx): void {
  const { prop, env, b } = cx;
  const needs = [prop.refer_target1, prop.refer_target2];
  // no active buff pet: both are missing (`FAILED_BOTH_NEEDVIS`)
  const state = env.needVisState?.(prop) ?? 'both';
  needs.forEach((need, i) => {
    if (need === NULL_ID || need === 0) return;
    const needProp = env.prop(need);
    const failed = state === 'both' || (i === 0 ? state === 'first' : state === 'second');
    b.add(
      `\n${env.text(TID_GAME_BUFFPET_REQUIRE)}: ${needProp?.name ?? ''}`,
      failed ? RED : DEFAULT_TEXT_COLOR,
    );
  });
}

/** `PutSealChar`: the sealed character's name, level, job and stats, all red. */
export function putSealChar(cx: ComposeCx): void {
  const { item, env, b } = cx;
  const t = env.text;
  let s = `\n${t(TID_TOOLTIP_CHARNAME)}\n${item.itemText}`;
  const job = item.repair;
  const level = item.repairNumber;
  let levelText: string;
  if (job >= MAX_PROFESSIONAL && job < MAX_MASTER) {
    levelText = `${String(level)}${t(TID_GAME_TOOLTIP_MARK_MASTER)}`;
  } else if (job >= MAX_MASTER) {
    levelText = `${String(level)}${t(TID_GAME_TOOLTIP_MARK_HERO)}`;
  } else {
    levelText = String(level);
  }
  s += `\n${t(TID_TOOLTIP_CHARLEVEL)}${levelText}`;
  s += `\n${t(TID_TOOLTIP_CHARJOB)}${env.jobName(job)}`;
  s += `\n${t(TID_TOOLTIP_CHARSTAT)}`;
  const stat = (i: number): number => item.cards[i] ?? 0;
  s += `\n${t(TID_TOOLTIP_CHARSTA)}${String(stat(0))}`;
  s += `\n${t(TID_TOOLTIP_CHARSTR)}${String(stat(1))}`;
  s += `\n${t(TID_TOOLTIP_CHARDEX)}${String(stat(2))}`;
  s += `\n${t(TID_TOOLTIP_CHARINT)}${String(stat(3))}`;
  s += `\n${t(TID_TOOLTIP_CHARPOINT)}${String(item.resistAbilityOption)}`;
  b.add(s, argb(255, 255, 20, 20));
}

/** The DST lines of an add-skill level (`GetSkillProp(..., skill, 1)` + `PutDestParam`): the disguise item and the eat pet. */
export function putActiveSkillDsts(cx: ComposeCx): void {
  const { prop, env, b } = cx;
  if (prop.active_skill === NULL_ID) return;
  const skill = env.skillProp(prop.active_skill);
  if (!skill) return;
  const add = env.addSkill(skill.sub_define, 1);
  if (!add) return;
  b.addRuns(
    putDestParam(
      add.dest_param[0],
      add.dest_param[1],
      add.adj_param_val[0],
      add.adj_param_val[1],
      env.text,
    ),
  );
}

/** The `IK2_GMTEXT` / `IK3_TEXT_DISGUISE` block: the item's own DSTs, then its active skill's. */
export function putDisguise(cx: ComposeCx): void {
  const { prop, env, b } = cx;
  b.addRuns(
    putDestParam(
      prop.dest_param[0],
      prop.dest_param[1],
      prop.adj_param_val[0],
      prop.adj_param_val[1],
      env.text,
    ),
  );
  putActiveSkillDsts(cx);
}

/** The remaining time of one vis: `d h m` / `h m` / `m s` / `s` from the day/hour/minute texts, the time-out text when past. */
function visRemaining(cx: ComposeCx, t: number): string {
  const { env } = cx;
  const ct = timeSpan(t);
  const day = (n: number): string => sprintf(env.text(TID_PK_LIMIT_DAY), [n]);
  const hour = (n: number): string => sprintf(env.text(TID_PK_LIMIT_HOUR), [n]);
  const minute = (n: number): string => sprintf(env.text(TID_PK_LIMIT_MINUTE), [n]);
  const second = (n: number): string => sprintf(env.text(TID_PK_LIMIT_SECOND), [n]);
  if (ct.days > 0) return day(ct.days) + hour(ct.hours) + minute(ct.minutes);
  if (ct.hours > 0) return hour(ct.hours) + minute(ct.minutes);
  if (ct.minutes > 0) return minute(ct.minutes) + second(ct.seconds);
  if (ct.seconds >= 0) return second(ct.seconds);
  return env.text(TID_GAME_TOOLTIP_TIMEOUT);
}

export { visRemaining };

/** `PutVisPetInfo`: the summed effects of the active vis, then every socketed vis with its remaining time. */
export function putVisPetInfo(cx: ComposeCx): void {
  const { item, env, b, colors } = cx;
  const total = new Map<number, number>();
  for (let i = 0; i < item.cards.length; i += 1) {
    const visId = item.cards[i] ?? 0;
    const vis = env.prop(visId);
    if (!vis) continue;
    const expires = item.visExpireAt[i] ?? 0;
    if (cx.env.nowSec() >= expires) continue;
    if (env.visActive && !env.visActive(item, visId)) continue;
    for (let k = 0; k < 3; k += 1) {
      const dst = vis.dest_param[k];
      if (dst === undefined || dst === NULL_ID) continue;
      total.set(dst, (total.get(dst) ?? 0) + (vis.adj_param_val[k] ?? 0));
    }
  }
  for (const dst of [...total.keys()].sort((a, c) => a - c)) {
    b.add(dstSignedLine(dst, total.get(dst) ?? 0, env.text), colors.piercing);
  }
  cx.volatile = cx.volatile || item.cards.some((c) => c !== 0 && c !== NULL_ID);
  for (let i = 0; i < 9; i += 1) {
    const visId = item.cards[i] ?? 0;
    if (visId === 0 || visId === NULL_ID) continue;
    const vis = env.prop(visId);
    if (!vis) continue;
    const t = (item.visExpireAt[i] ?? 0) - env.nowSec();
    b.add('\n', DEFAULT_TEXT_COLOR);
    b.add(`${vis.name} (${visRemaining(cx, t)})`, t > 0 ? colors.time : RED);
  }
}
