// The lines of the guild bank log (`CDPClient::OnGuildBankLogList`, `src:Neuz/DPClient.cpp:17128-17290`, `__GUILD_BANK_LOG`; Plan/45 §23, ref/09b §3.7; WP-CFEAT-38). The server
// sends up to 100 entries per kind, newest first (`GuildLogViewNtf{kind, entries}`); the client turns each into one line:
//
//   date         "%.2d%.2d%.2d %.2d:%.2d   " of the entry's time in the LOCAL time of the browser (`CTime( time_t )`), the year with four digits: "20261024 11:30   "
//   kinds 1, 2   `TID_GAME_GUILDBANK_LOG_ITEM_ADD / _REMOVE( date, name, item, count )` ("Add %s\t%s\t%s\t%d"); the member name is cut at 9 characters + "...", the item name at 25
//                characters + "..."; an ultimate weapon (`refer_stat1 == WEAPON_ULTIMATE`) gets the prefix "(U)"; a guild cloak reads `TID_GUILD_CLOAK( own guild )` unless it is
//                one of the four dragon cloaks; " +N" follows for an upgraded item
//   kinds 3, 4   `TID_GAME_GUILDBANK_LOG_PENYA_ADD / _REMOVE( date, name, penya )` ("Deposit Penya %s\t%s\t%d"); the name is not cut
//
// FIX (the original dereferenced a NULL item prop): an item the data does not know prints its id.

import { IK3_CLOAK, WEAPON_ULTIMATE } from '../../gen/defs/itemkind';
import {
  II_ARM_S_CLO_CLO_DRAGON1,
  II_ARM_S_CLO_CLO_DRAGON2,
  II_ARM_S_CLO_CLO_DRAGON3,
  II_ARM_S_CLO_CLO_DRAGON4,
} from '../../gen/defs/item';
import {
  TID_GAME_GUILDBANK_LOG_ITEM_ADD,
  TID_GAME_GUILDBANK_LOG_ITEM_REMOVE,
  TID_GAME_GUILDBANK_LOG_PENYA_ADD,
  TID_GAME_GUILDBANK_LOG_PENYA_REMOVE,
  TID_GUILD_CLOAK,
} from '../../gen/defs/text';
import type { BankLogKind } from '../../ui/windows/guild/ports';
import { formatTemplate } from '../common/text';

/** One entry of `GuildLogViewNtf`. */
export interface LogEntry {
  readonly player: string;
  /** Unix seconds. */
  readonly loggedAt: number;
  /** Kinds 1 and 2: the item id; kinds 3 and 4: the penya amount. */
  readonly itemId: number;
  readonly abilityOption: number;
  readonly count: number;
}

/** What the lines read of the client. */
export interface LogEnv {
  text(tid: number): string;
  /** The prop of an item (`null` = unknown). */
  item(
    id: number,
  ): { readonly name: string; readonly kind3: number; readonly referStat1: number } | null;
  /** The name of the own guild (the guild cloak line), `null` when unknown. */
  guildName(): string | null;
}

const DRAGON_CLOAKS: readonly number[] = [
  II_ARM_S_CLO_CLO_DRAGON1,
  II_ARM_S_CLO_CLO_DRAGON2,
  II_ARM_S_CLO_CLO_DRAGON3,
  II_ARM_S_CLO_CLO_DRAGON4,
];

const two = (n: number): string => String(n).padStart(2, '0');

/** `"%.2d%.2d%.2d %.2d:%.2d   "` of a unix time in the local time of the browser. */
export function logDate(unix: number): string {
  const d = new Date(unix * 1000);
  return `${two(d.getFullYear())}${two(d.getMonth() + 1)}${two(d.getDate())} ${two(d.getHours())}:${two(d.getMinutes())}   `;
}

/** `Left( n )` plus "..." when the text is longer than `n` characters (`IsDBCSLeadByte` steps become whole characters). */
export function cutText(text: string, n: number): string {
  const chars = Array.from(text);
  return chars.length > n ? `${chars.slice(0, n).join('')}...` : text;
}

/** The item name of a log line (the cuts and the marks of the original). */
export function logItemName(entry: LogEntry, env: LogEnv): string {
  const prop = env.item(entry.itemId);
  // FIX: the original dereferenced the NULL prop of an unknown item
  if (!prop) return cutText(String(entry.itemId), 25);
  let name = prop.referStat1 === WEAPON_ULTIMATE ? `(U)${prop.name}` : prop.name;
  if (prop.kind3 === IK3_CLOAK) {
    const guild = env.guildName();
    name =
      guild !== null && !DRAGON_CLOAKS.includes(entry.itemId)
        ? formatTemplate(env.text(TID_GUILD_CLOAK), [guild])
        : prop.name;
  }
  name = cutText(name, 25);
  return entry.abilityOption > 0 ? `${name} +${String(entry.abilityOption)}` : name;
}

/** One line of the log of `kind`. */
export function formatLogLine(kind: BankLogKind, entry: LogEntry, env: LogEnv): string {
  const date = logDate(entry.loggedAt);
  if (kind === 1 || kind === 2) {
    const tid = kind === 1 ? TID_GAME_GUILDBANK_LOG_ITEM_ADD : TID_GAME_GUILDBANK_LOG_ITEM_REMOVE;
    return formatTemplate(env.text(tid), [
      date,
      cutText(entry.player, 9),
      logItemName(entry, env),
      entry.count,
    ]);
  }
  const tid = kind === 3 ? TID_GAME_GUILDBANK_LOG_PENYA_ADD : TID_GAME_GUILDBANK_LOG_PENYA_REMOVE;
  // the penya of kinds 3 and 4 rides in the item id slot of the entry
  return formatTemplate(env.text(tid), [date, entry.player, entry.itemId]);
}

/** The lines of a whole answer. */
export function formatLog(
  kind: BankLogKind,
  entries: readonly LogEntry[],
  env: LogEnv,
): readonly string[] {
  return entries.map((e) => formatLogLine(kind, e, env));
}
