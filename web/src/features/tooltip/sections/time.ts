// Sections 9 and 11 of the item tooltip: the cooldown and the keep time (Plan/45 §10.2; `PutCoolTime`, `PutKeepTime`;
// `src:_Interface/WndManager.cpp:5950-6075`).

import { IK3_TICKET } from '../../../gen/defs/itemkind';
import { IK2_FOOD, IK2_SKILL, IK3_PILL, IK3_VIS } from '../../../gen/defs/itemkind';
import {
  TID_GAME_COND_USE,
  TID_PK_LIMIT_DAY,
  TID_PK_LIMIT_HOUR,
  TID_PK_LIMIT_MINUTE,
  TID_PK_LIMIT_SECOND,
  TID_TOOLTIP_COOLTIME,
  TID_TOOLTIP_DATE,
  TID_TOOLTIP_ITEMTIME,
  TID_TOOLTIP_PERIOD,
} from '../../../gen/defs/text';
import { DEFAULT_TEXT_COLOR } from '../../../ui/text/markup';
import { NULL_ID } from '../../inventory/info';
import { ITEM_FLAG } from '../../inventory/items';
import { cooldownGroup } from '../../inventory/state';
import type { ComposeCx } from '../model';
import { sprintf } from '../printf';
import { timeSpan } from '../rich';

/** `PutCoolTime`: `mm:ss` left of the item's cooldown group, rounded `(end - now + 500) / 1000`. */
export function putCoolTime(cx: ComposeCx): void {
  const { prop, env, b, colors } = cx;
  const group = cooldownGroup(
    prop.item_kind2,
    prop.item_kind3,
    prop.skill_ready,
    IK2_FOOD,
    IK2_SKILL,
    IK3_PILL,
  );
  if (group <= 0) return;
  const end = env.cooldownEnd(group);
  if (end === null || end <= cx.now) return;
  cx.volatile = true;
  const ct = timeSpan(Math.trunc((end - cx.now + 500) / 1000));
  b.add('\n', DEFAULT_TEXT_COLOR);
  b.add(sprintf(env.text(TID_TOOLTIP_COOLTIME), [ct.minutes, ct.seconds]), colors.general);
}

const isVisProp = (kind3: number): boolean => kind3 === IK3_VIS;

/**
 * `PutKeepTime`: the prop's duration (`circle_time`: 1 = "applies on death", else `Duration: d h m s`), then the remaining
 * keep time of the instance. Tickets print `d/h/m/s`; vis `d h m` / `h m` / `m s` / `s`; the others the largest unit only, KEEP-QUIRK:
 * days shown as days + 1.
 */
export function putKeepTime(cx: ComposeCx): void {
  const { prop, item, env, b, colors } = cx;
  const text = env.text;
  if (prop.circle_time !== NULL_ID) {
    b.add('\n', DEFAULT_TEXT_COLOR);
    if (prop.circle_time === 1) {
      b.add(text(TID_GAME_COND_USE), colors.time);
    } else {
      const ct = timeSpan(prop.circle_time);
      b.add(`${text(TID_TOOLTIP_ITEMTIME)} : `, colors.time);
      b.add(
        sprintf(text(TID_TOOLTIP_DATE), [ct.days, ct.hours, ct.minutes, ct.seconds]),
        colors.time,
      );
    }
  }
  let keep = item.keepTimeAt;
  let t = keep - env.nowSec();
  // a vis that is not worn has no server data: its duration is the prop's `ability_min` minutes
  if (t <= 0 && isVisProp(prop.item_kind3)) {
    keep = prop.ability_min === NULL_ID ? 0 : prop.ability_min;
    t = Math.trunc(keep * 60);
  }
  if (keep === 0 || (item.flag & ITEM_FLAG.EXPIRED) !== 0) return;
  let str = '';
  if (t > 0) {
    cx.volatile = true;
    const ct = timeSpan(t);
    const unit = (tid: number, n: number): string => sprintf(text(tid), [n]);
    if (prop.item_kind3 === IK3_TICKET) {
      str = sprintf(text(TID_TOOLTIP_DATE), [ct.days, ct.hours, ct.minutes, ct.seconds]);
    } else if (isVisProp(prop.item_kind3)) {
      if (ct.days !== 0) {
        str =
          unit(TID_PK_LIMIT_DAY, ct.days) +
          unit(TID_PK_LIMIT_HOUR, ct.hours) +
          unit(TID_PK_LIMIT_MINUTE, ct.minutes);
      } else if (ct.hours !== 0) {
        str = unit(TID_PK_LIMIT_HOUR, ct.hours) + unit(TID_PK_LIMIT_MINUTE, ct.minutes);
      } else if (ct.minutes !== 0) {
        str = unit(TID_PK_LIMIT_MINUTE, ct.minutes) + unit(TID_PK_LIMIT_SECOND, ct.seconds);
      } else {
        str = unit(TID_PK_LIMIT_SECOND, ct.seconds);
      }
    } else if (ct.days !== 0) {
      str = unit(TID_PK_LIMIT_DAY, ct.days + 1);
    } else if (ct.hours !== 0) {
      str = unit(TID_PK_LIMIT_HOUR, ct.hours);
    } else if (ct.minutes !== 0) {
      str = unit(TID_PK_LIMIT_MINUTE, ct.minutes);
    } else {
      str = unit(TID_PK_LIMIT_SECOND, ct.seconds);
    }
  }
  b.add('\n', DEFAULT_TEXT_COLOR);
  b.add(str + text(TID_TOOLTIP_PERIOD), colors.time);
}
