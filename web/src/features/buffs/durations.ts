// The rows of the item duration list (`CWndCommItemCtrl::DrawSM / DrawSkill / OnMouseWndSurface`, `src:_Interface/WndCommItem.cpp`,
// ref/22 §17 D, Plan/45 §12): the premium items in effect - the state-mode items (SM) and the charged item buffs - with the name in
// blue, the remaining time under it and the "usage" tooltip. The experience scrolls of a family merge into one row that lists
// every scroll's time ("1:02:03 / 0:10:00").

import type { ClientItem } from '../../data/clientData';
import {
  II_SYS_SYS_SCR_BLESSING,
  II_SYS_SYS_SCR_PET_FEED_POCKET,
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
  IK1_HOUSING,
  IK2_BUFF2,
  IK3_ANGEL_BUFF,
  IK3_EGG,
  IK3_TICKET,
} from '../../gen/defs/itemkind';
import {
  TID_GAME_COND_USE,
  TID_GAME_DEMOL_USE,
  TID_GAME_TIP_ANGEL_PLAIN,
  TID_GAME_TOOLTIP_PERMANENTTIME_1,
  TID_TOOLTIP_ITEMTIME,
  TID_TOOLTIP_PERIOD,
  TID_TOOLTIP_RETURN_USE,
  TID_TOOLTIP_USE,
} from '../../gen/defs/text';
import type { CommItemRow } from '../../ui/windows/itemuse/ports';
import { sprintf } from '../tooltip/printf';
import { ESSTY, RichBuilder, timeSpan } from '../tooltip/rich';
import { type Buff, remainOf } from './buffSet';
import { dateText, spanOfMs } from './format';
import { expFamilyOf } from './ids';
import { PERMANENT_TIME, smRemaining, type DisplayEnv } from './display';
import { SM_ITEM_ID, SM_MAX } from './sm';
import { BUFF_EQUIP, BUFF_SKILL } from './types';

export interface DurationEnv extends DisplayEnv {
  /** The unix expiry of the ticket in the bag (`GetTicket()->m_dwKeepTime`), `null` when none. */
  ticketEnd(): number | null;
  /** The unix expiry of the active feed pocket in the bag, `null` when none. */
  feedPocketEnd(): number | null;
}

const SMELT_ITEMS = new Set<number>([
  II_SYS_SYS_SCR_SMELPROT,
  II_SYS_SYS_SCR_SMELPROT2,
  II_SYS_SYS_SCR_SMELPROT3,
  II_SYS_SYS_SCR_SMELPROT4,
  II_SYS_SYS_SCR_SMELTING,
  II_SYS_SYS_SCR_SMELTING2,
  II_SYS_SYS_SCR_SUPERSMELTING,
]);

const NAME_COLOR = 0xff0000ff;

/** `%.1d:%.2d:%.2d ` of an experience scroll's remaining time. */
function expClock(ms: number): string {
  const ct = spanOfMs(ms);
  return `${String(ct.hours)}:${String(ct.minutes).padStart(2, '0')}:${String(ct.seconds).padStart(2, '0')} `;
}

function usageTooltip(env: DurationEnv, prop: ClientItem, body: string): CommItemRow['tooltip'] {
  const rb = new RichBuilder();
  rb.add(prop.name, NAME_COLOR, ESSTY.BOLD);
  rb.add('\n', 0xff000000);
  rb.add(body, 0xff000000);
  rb.add('\n', 0xff000000);
  rb.add(sprintf(env.text(TID_TOOLTIP_USE), [prop.description]), 0xff000000);
  return rb.runs;
}

/** The "time left" line of the default tooltip. */
function timeLeftLine(env: DurationEnv, prop: ClientItem): string {
  // `circle_time == -1` takes the skill time in seconds; else the circle time
  const secs =
    prop.circle_time === 0xffff_ffff || prop.circle_time === -1
      ? Math.trunc(prop.skill_time / 1000)
      : prop.circle_time;
  const label = `${env.text(TID_TOOLTIP_ITEMTIME)} : `;
  if (
    prop.skill_time >= 999_999_999 &&
    (prop.circle_time === 0xffff_ffff || prop.circle_time === -1)
  ) {
    return label + env.text(TID_GAME_TOOLTIP_PERMANENTTIME_1);
  }
  return label + dateText(timeSpan(secs), env.text);
}

/** The duration list: the state-mode rows, then the charged item buffs in the order of the buff map. */
export function buildDurationRows(
  env: DurationEnv,
  own: readonly Buff[],
  smTimers: readonly { readonly kind: number; readonly endUnix: number }[],
): CommItemRow[] {
  const rows: CommItemRow[] = [];
  const nowUnix = env.nowUnix();
  const now = env.now();
  for (const t of smRemaining(smTimers, nowUnix)) {
    const itemId = SM_ITEM_ID[t.kind] ?? 0;
    const prop = itemId !== 0 ? env.item(itemId) : undefined;
    if (!prop || prop.icon === '' || t.kind >= SM_MAX) continue;
    const detail =
      prop.id === II_SYS_SYS_SCR_BLESSING
        ? env.text(TID_GAME_COND_USE)
        : dateText(timeSpan(t.remainSec), env.text);
    rows.push({
      key: `sm:${String(t.kind)}`,
      icon: `item/${prop.icon}`,
      name: prop.name,
      detail,
      tooltip: usageTooltip(
        env,
        prop,
        prop.id === II_SYS_SYS_SCR_BLESSING ? env.text(TID_GAME_COND_USE) : timeLeftLine(env, prop),
      ),
    });
  }
  const done = new Set<number>();
  for (const b of own) {
    if (b.type === BUFF_SKILL || b.type === BUFF_EQUIP) continue;
    const family = expFamilyOf(b.id);
    if (family) {
      if (done.has(family.kind)) continue;
    }
    const id = family ? family.base : b.id;
    const prop = env.item(id);
    if (!prop || prop.charged === 0) continue;
    if (prop.item_kind3 === IK3_EGG) continue;
    if (prop.item_kind1 === IK1_HOUSING) continue;
    if (prop.icon === '') continue;
    let detail: string;
    let tipBody: string;
    if (family) {
      done.add(family.kind);
      const scrolls = family.overlap
        ? own.filter((x) => family.ids.includes(x.id))
        : own.filter((x) => x.id === family.base).slice(0, 1);
      detail = scrolls
        .map((s, i) => `${expClock(remainOf(s, now))}${i + 1 < scrolls.length ? '/ ' : ''}`)
        .join('');
      tipBody = timeLeftLine(env, prop);
    } else if (SMELT_ITEMS.has(prop.id)) {
      detail = env.text(TID_GAME_DEMOL_USE);
      tipBody = detail;
    } else if (prop.id === II_SYS_SYS_SCR_RETURN) {
      detail = env.text(TID_TOOLTIP_RETURN_USE);
      tipBody = timeLeftLine(env, prop);
    } else if (prop.item_kind3 === IK3_ANGEL_BUFF) {
      detail = env.text(TID_GAME_TIP_ANGEL_PLAIN);
      tipBody = detail;
    } else if (prop.item_kind2 === IK2_BUFF2) {
      const left = timeSpan(Math.max(0, b.level - nowUnix));
      detail = `${env.text(TID_TOOLTIP_ITEMTIME)} : ${dateText(left, env.text)}`;
      tipBody = detail;
    } else if (prop.item_kind3 === IK3_TICKET) {
      const end = env.ticketEnd();
      detail = '';
      if (end !== null) {
        const t = end - nowUnix;
        detail = (t > 0 ? dateText(timeSpan(t), env.text) : '') + env.text(TID_TOOLTIP_PERIOD);
      }
      tipBody = detail;
    } else if (prop.id === II_SYS_SYS_SCR_PET_FEED_POCKET) {
      const end = env.feedPocketEnd();
      detail = '';
      if (end !== null && end - nowUnix > 0) detail = dateText(timeSpan(end - nowUnix), env.text);
      tipBody = detail === '' ? '' : `${env.text(TID_TOOLTIP_ITEMTIME)} : ${detail}`;
    } else if (prop.skill_time >= PERMANENT_TIME) {
      detail = env.text(TID_GAME_TOOLTIP_PERMANENTTIME_1);
      tipBody = timeLeftLine(env, prop);
    } else {
      detail = dateText(spanOfMs(remainOf(b, now)), env.text);
      tipBody = timeLeftLine(env, prop);
    }
    rows.push({
      key: family ? `exp:${String(family.kind)}` : `${String(b.type)}:${String(b.id)}`,
      icon: `item/${prop.icon}`,
      name: prop.name,
      detail,
      tooltip: usageTooltip(env, prop, tipBody),
    });
  }
  return rows;
}
