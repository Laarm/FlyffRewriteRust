// The read models of the siege windows (Plan/45 §28.4; WP-CFEAT-41): the rows of the line-up editor from the guild roster and the player-data cache. Pure: the module gathers the facts.

import type { GuildMemberRow } from '../../../ui/windows/guild/ports';
import type { SelectionMember } from '../../../ui/windows/siege/ports';

/** The guild ranks that may be put in the line-up of the combat as the "master or kingpin" (`GUD_MASTER`, `GUD_KINGPIN`). */
export const RANK_MASTER = 0;
export const RANK_KINGPIN = 1;

/**
 * `UpDateGuildListBox`: the ONLINE members ordered by level, ascending (a `multimap< level, member >`: equal levels keep the order of the member map, i.e. the character id).
 * `rows` are in character id order already.
 */
export function onlineMembersByLevel(
  rows: readonly GuildMemberRow[],
  jobName: (job: number) => string,
): SelectionMember[] {
  return rows
    .filter((r) => r.online)
    .map((r, i) => ({ r, i }))
    .sort((a, b) => a.r.level - b.r.level || a.i - b.i)
    .map(({ r }) => ({ id: r.id, name: r.name, level: r.level, job: jobName(r.job) }));
}

/** A member (online or not) as a row of the editor, `null` for a character that is not in the roster. */
export function memberRowOf(
  rows: readonly GuildMemberRow[],
  id: number,
  jobName: (job: number) => string,
): SelectionMember | null {
  const r = rows.find((x) => x.id === id);
  return r ? { id: r.id, name: r.name, level: r.level, job: jobName(r.job) } : null;
}
