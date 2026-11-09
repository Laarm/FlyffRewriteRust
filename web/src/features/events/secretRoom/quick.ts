// The member list of the war (`CWndSecretRoomQuick`, `src:_Interface/WndSecretRoom.cpp:1838-2264`, `WndManager.cpp:3541-3570`; Plan/45 §28.6; WP-CFEAT-52): the pure parts - when the window is
// opened or closed, which members it lists in which order, and the text, colour and gauge of each row. The module gathers the facts (the roster, the movers in view) and hands the rows to the
// window of `ui/windows/secretroom`.

import {
  TID_GAME_QUICK_MARK_HERO,
  TID_GAME_QUICK_MARK_MASTER,
  TID_GAME_TOOLTIP_MARK_HERO,
  TID_GAME_TOOLTIP_MARK_MASTER,
} from '../../../gen/defs/text';
import type { QuickRowView } from '../../../ui/windows/secretroom/ports';
import { MAX_SECRETROOM_MEMBER, QUICK_COLOR } from '../../../ui/windows/secretroom/rows';
import type { GuildMemberRow } from '../../../ui/windows/guild/ports';
import { formatTemplate } from '../../common/text';
import { isHeroJob, isMasterJob } from '../../hud/targetFrame/model';

export { SR_WAR_STATE } from './constants';
import { SR_WAR_STATE } from './constants';

/** What the check of `WndManager.cpp:3541-3570` decides: open the window, close it, or leave it. */
export function quickAction(f: {
  readonly state: number;
  readonly inRoom: boolean;
  readonly hasOwnEntry: boolean;
  readonly open: boolean;
}): 'open' | 'close' | 'none' {
  if (f.state >= SR_WAR_STATE && f.inRoom && f.hasOwnEntry && !f.open) return 'open';
  if (!f.inRoom && f.open) return 'close';
  return 'none';
}

/**
 * The members the window lists, fixed when it opens (KEEP): the guild master first (whether or not he is in the line-up), then the other line-up members that are guild members, ascending by level,
 * equal levels with the higher job id first (`prMemberLevelDesc`, KEEP despite its name), equal level and job in line-up order (stable: FIX of the unspecified `std::sort`), at most 11 in all.
 */
export function quickMembers(
  masterId: number,
  lineup: readonly number[],
  roster: readonly GuildMemberRow[],
): number[] {
  const byId = new Map(roster.map((r) => [r.id, r]));
  const others = lineup
    .map((id, order) => ({ id, order, row: byId.get(id) }))
    .filter((x) => x.id !== masterId && x.row !== undefined)
    .sort(
      (a, b) =>
        (a.row?.level ?? 0) - (b.row?.level ?? 0) ||
        (b.row?.job ?? 0) - (a.row?.job ?? 0) ||
        a.order - b.order,
    )
    .map((x) => x.id);
  const out: number[] = [];
  if (masterId !== 0 && byId.has(masterId)) out.push(masterId);
  for (const id of others) out.push(id);
  return out.slice(0, MAX_SECRETROOM_MEMBER);
}

/** What the row of one member reads. */
export interface QuickFacts {
  readonly id: number;
  readonly name: string;
  readonly level: number;
  readonly job: number;
  readonly online: boolean;
  /** The row is the first one (the guild master). */
  readonly first: boolean;
  readonly self: boolean;
  /** The mover when he is in view (the own character always is), else `null`. */
  readonly mover: { readonly hp: number; readonly maxHp: number } | null;
}

/** One row: the text, the colour and the HP gauge (`SetGuildMember` / `OnDraw`). */
export function quickRow(f: QuickFacts, text: (tid: number) => string): QuickRowView {
  if (f.mover === null) {
    // out of view: "<level>[-M|-H]. <name>", grey, black when offline
    const mark = isMasterJob(f.job)
      ? text(TID_GAME_TOOLTIP_MARK_MASTER)
      : isHeroJob(f.job)
        ? text(TID_GAME_TOOLTIP_MARK_HERO)
        : '';
    return {
      id: f.id,
      text: `${String(f.level)}${mark}. ${f.name}`,
      color: f.online ? QUICK_COLOR.away : QUICK_COLOR.offline,
      hp: 0,
      inView: false,
      self: f.self,
    };
  }
  const { hp, maxHp } = f.mover;
  let label: string;
  if (isMasterJob(f.job))
    label = formatTemplate(text(TID_GAME_QUICK_MARK_MASTER), [f.level, f.name]);
  else if (isHeroJob(f.job))
    label = formatTemplate(text(TID_GAME_QUICK_MARK_HERO), [f.level, f.name]);
  else label = `${String(f.level)}. ${f.name}`;
  let color: number = QUICK_COLOR.ok;
  if (hp <= 0) color = QUICK_COLOR.dead;
  else if (maxHp > 0 && hp / maxHp < 0.1) color = QUICK_COLOR.low;
  // the guild master (row 0) in view is always green, also when dead (KEEP)
  if (f.first) color = QUICK_COLOR.master;
  return {
    id: f.id,
    text: label,
    color,
    hp: maxHp > 0 ? Math.max(0, Math.min(1, hp / maxHp)) : hp > 0 ? 1 : 0,
    inView: true,
    self: f.self,
  };
}
