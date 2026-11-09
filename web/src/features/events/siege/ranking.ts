// The rankings of the siege (Plan/45 §28.4; `src:Neuz/DPClient.cpp:7340-7430`, `_Interface/WndWorld.cpp:1190-1330`, `WndField.cpp:18340-18480`): the multimaps of the original, the shared tie
// ranks, the name cut and the person ranking of `GC_PLAYERPOINT`. All pure.
//
//   * `m_mmapGuildCombat_GuildPrecedence` and `...PlayerPrecedence` are `multimap< int, ... >` read from `rbegin()`: points descending, equal points in REVERSE order of arrival;
//   * a tie shares the rank (`nRate++` only when the points differ from the row above: 1, 2, 2, 3);
//   * the names are cut at ten characters (the result window) or five (the HUD boards) plus dots; a name of exactly that length gets the dots too (KEEP).

import {
  JOB_ACROBAT,
  JOB_ASSIST,
  JOB_BILLPOSTER,
  JOB_BILLPOSTER_HERO,
  JOB_BILLPOSTER_MASTER,
  JOB_BLADE,
  JOB_BLADE_HERO,
  JOB_BLADE_MASTER,
  JOB_ELEMENTOR,
  JOB_ELEMENTOR_HERO,
  JOB_ELEMENTOR_MASTER,
  JOB_JESTER,
  JOB_JESTER_HERO,
  JOB_JESTER_MASTER,
  JOB_KNIGHT,
  JOB_KNIGHT_HERO,
  JOB_KNIGHT_MASTER,
  JOB_MAGICIAN,
  JOB_MERCENARY,
  JOB_PSYCHIKEEPER,
  JOB_PSYCHIKEEPER_HERO,
  JOB_PSYCHIKEEPER_MASTER,
  JOB_RANGER,
  JOB_RANGER_HERO,
  JOB_RANGER_MASTER,
  JOB_RINGMASTER,
  JOB_RINGMASTER_HERO,
  JOB_RINGMASTER_MASTER,
} from '../../../gen/defs/job';
import { cutName, padLeft } from '../../../ui/windows/siege/format';
import type { RankRow, RankView } from '../../../ui/windows/siege/ports';
import type { GuildRankEntry } from '../store';

/** The multimap read from `rbegin()`: ascending by key with equal keys in arrival order, then reversed. */
export function multimapReverse<T>(items: readonly T[], key: (t: T) => number): T[] {
  return items
    .map((t, i) => ({ t, i }))
    .sort((a, b) => key(a.t) - key(b.t) || a.i - b.i)
    .reverse()
    .map((x) => x.t);
}

export interface Ranked<T> {
  readonly item: T;
  /** The dense rank (1, 2, 2, 3). */
  readonly rank: number;
  /** The first row of its group of equal points. */
  readonly first: boolean;
}

/** Dense ranks over rows already ordered by points descending. */
export function denseRanks<T>(ordered: readonly T[], points: (t: T) => number): Ranked<T>[] {
  let rank = 0;
  let old: number | null = null;
  return ordered.map((item) => {
    const p = points(item);
    const first = old !== p;
    if (first) rank += 1;
    old = p;
    return { item, rank, first };
  });
}

/** The guilds of `GC_GUILDPRECEDENCE` that were sent (`bRecive`), in board order. */
export function guildBoard(entries: readonly GuildRankEntry[]): Ranked<GuildRankEntry>[] {
  return denseRanks(
    multimapReverse(
      entries.filter((e) => e.recv),
      (e) => e.points,
    ),
    (e) => e.points,
  );
}

export interface PlayerPoints {
  readonly id: number;
  readonly points: number;
}

/** The players of `GC_PLAYERPRECEDENCE` of every guild as one list, in board order. */
export function personBoard(lists: readonly (readonly PlayerPoints[])[]): Ranked<PlayerPoints>[] {
  return denseRanks(
    multimapReverse(lists.flat(), (p) => p.points),
    (p) => p.points,
  );
}

/** The rows of the guild ranking of the result window: `"%2d   <name>\t(<pts>) WINNER"`. */
export function resultGuildRows(entries: readonly GuildRankEntry[]): string[] {
  return guildBoard(entries).map(({ item, rank, first }) => {
    const name = cutName(item.name, 10);
    if (!first) return `     ${name}\t(${String(item.points)})`;
    const winner = rank === 1 ? ' WINNER' : '';
    return `${padLeft(rank, 2)}   ${name}\t(${String(item.points)})${winner}`;
  });
}

/** The rows of the person ranking of the result window; `MVP` marks the best player on the first row of his point group only (KEEP). */
export function resultPersonRows(
  lists: readonly (readonly PlayerPoints[])[],
  bestPlayer: number | undefined,
  nameOf: (id: number) => string,
): string[] {
  return personBoard(lists).map(({ item, rank, first }) => {
    const name = cutName(nameOf(item.id), 10);
    if (!first) return `     ${name}\t(${String(item.points)})`;
    const mvp = bestPlayer !== undefined && item.id === bestPlayer ? ' MVP' : '';
    return `${padLeft(rank, 2)}   ${name}\t(${String(item.points)})${mvp}`;
  });
}

// ---- the person ranking (`GC_PLAYERPOINT`, `CWndGuildCombatRank_Person::DivisionList`) ----------------------------------------------------------------

/** One entry of `GC_PLAYERPOINT`. */
export interface PlayerPointEntry {
  readonly id: number;
  readonly job: number;
  readonly points: number;
}

/** The page of the job line of a job: 1 Mercenary, 2 Magician, 3 Acrobat, 4 Assist, `-1` for a job of no line (the vagrant: it appears nowhere, KEEP). */
export function jobPage(job: number): number {
  switch (job) {
    case JOB_MERCENARY:
    case JOB_KNIGHT:
    case JOB_BLADE:
    case JOB_KNIGHT_MASTER:
    case JOB_BLADE_MASTER:
    case JOB_KNIGHT_HERO:
    case JOB_BLADE_HERO:
      return 1;
    case JOB_MAGICIAN:
    case JOB_PSYCHIKEEPER:
    case JOB_ELEMENTOR:
    case JOB_PSYCHIKEEPER_MASTER:
    case JOB_ELEMENTOR_MASTER:
    case JOB_PSYCHIKEEPER_HERO:
    case JOB_ELEMENTOR_HERO:
      return 2;
    case JOB_ACROBAT:
    case JOB_JESTER:
    case JOB_RANGER:
    case JOB_JESTER_MASTER:
    case JOB_RANGER_MASTER:
    case JOB_JESTER_HERO:
    case JOB_RANGER_HERO:
      return 3;
    case JOB_ASSIST:
    case JOB_RINGMASTER:
    case JOB_BILLPOSTER:
    case JOB_RINGMASTER_MASTER:
    case JOB_BILLPOSTER_MASTER:
    case JOB_RINGMASTER_HERO:
    case JOB_BILLPOSTER_HERO:
      return 4;
    default:
      return -1;
  }
}

/** `MAX_GUILDCOMBAT_RANK`: a page keeps at most this many rows. */
export const RANK_PAGE_MAX = 100;

export interface RankEnv {
  readonly selfId: number;
  /** The name now known to the player-data cache, `''` for none. */
  nameOf(id: number): string;
  /** `prj.m_aJob[ job ].szName`. */
  jobName(job: number): string;
}

/**
 * The five pages of the person ranking and the own row of each: `DivisionList` puts a player (in the order of `rbegin()`) on the page of his job line and on the first page; a page
 * keeps 100 rows; the own row is found anywhere in the list (FIX).
 */
export function rankView(entries: readonly PlayerPointEntry[], env: RankEnv): RankView {
  const ordered = multimapReverse(entries, (e) => e.points);
  const pages: PlayerPointEntry[][] = [[], [], [], [], []];
  for (const e of ordered) {
    const page = jobPage(e.job);
    if (page < 0) continue;
    const mine = pages[page];
    const all = pages[0];
    if (mine && mine.length < RANK_PAGE_MAX) mine.push(e);
    if (all && all.length < RANK_PAGE_MAX) all.push(e);
  }
  const own: (RankRow | null)[] = [];
  const out = pages.map((list, p) => {
    const ranked = denseRanks(list, (e) => e.points);
    const rows: RankRow[] = ranked.map(({ item, rank, first }) => ({
      rank,
      tie: !first,
      id: item.id,
      name: env.nameOf(item.id),
      job: env.jobName(item.job),
      points: item.points,
    }));
    own[p] = rows.find((r) => r.id === env.selfId) ?? null;
    return rows;
  });
  return { pages: out, own };
}
