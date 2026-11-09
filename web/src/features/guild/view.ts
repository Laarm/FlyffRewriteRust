// The read models of the guild windows (Plan/45 §23, §2.8; WP-CFEAT-37, -38): the guild store, the player-data cache and the presence overlay become the `GuildView` of the guild window,
// the `PowerView` of the two power pages and the `GuildBankView` of the bank. Pure: the module gathers the facts, so the rows (names and levels from the cache, the online state, the
// "%.2d/%.2d" head counts) are tested without a client.

import {
  TID_GAME_GUILD_WAR_DATA_LOSE,
  TID_GAME_GUILD_WAR_DATA_SURRENDER,
  TID_GAME_GUILD_WAR_DATA_WIN,
} from '../../gen/defs/text';
import {
  EMPTY_GUILD_VIEW,
  EMPTY_GUILD_WAR,
  GUILD_RANKS,
  type GuildBankView,
  type GuildMemberRow,
  type GuildView,
  type GuildWarView,
  type PowerView,
} from '../../ui/windows/guild/ports';
import { formatTemplate } from '../common/text';
import { ItemViewCache } from '../common/itemView';
import type { PowerDraft } from './power';
import { GUILD_BANK_SLOTS, type GuildState, isMasterOf, ownBrief, rankCount } from './store';

/** `GUILD_RANK_CAPS`: the head cap of the master, kingpin, captain and supporter; the rookie cap is the guild's member limit. */
export const RANK_CAPS: readonly number[] = [1, 5, 10, 20];

/** What a member row reads of the cache. */
export interface MemberFacts {
  readonly name: string;
  readonly level: number;
  readonly job: number;
  /** `uLogin > 0`. */
  readonly online: boolean;
}

export interface ViewFacts {
  readonly state: GuildState;
  readonly selfId: number;
  /** The cache entry of a character (`undefined` = never heard of). */
  member(id: number): MemberFacts | undefined;
  /** The channel number of the presence overlay (> 0 = online). */
  presence(id: number): number | undefined;
  /** `prj.m_aJob[job].dwJobType`. */
  jobType(job: number): number;
  /** `tables.guild.levels`: `[pxp, penya, max_member]` per level. */
  readonly levels: readonly (readonly number[])[];
  text(tid: number): string;
}

/** `GetMaxMemeber( level )`: the member limit of a guild level (row `level - 1`), 0 outside the table. */
export function maxMembers(levels: readonly (readonly number[])[], level: number): number {
  return levels[level - 1]?.[2] ?? 0;
}

const two = (n: number): string => String(n).padStart(2, '0');

/** The "%.2d/%.2d" count of a member level. */
export function rankCountText(count: number, cap: number): string {
  return `${two(count)}/${two(cap)}`;
}

/** The members in character id order (`m_mapPMember`) as the rows of the member page. */
export function memberRows(f: ViewFacts): GuildMemberRow[] {
  const own = f.state.own;
  if (!own) return [];
  const out: GuildMemberRow[] = [];
  for (const [id, m] of own.members) {
    const c = f.member(id);
    const job = c?.job ?? 0;
    out.push({
      id,
      name: c?.name ?? '',
      alias: m.alias,
      rank: m.rank,
      cls: m.cls,
      level: c?.level ?? 1,
      job,
      jobType: f.jobType(job),
      online: (f.presence(id) ?? 0) > 0 || c?.online === true,
      givePxp: m.givePxp,
      giveGold: m.giveGold,
      surrender: m.surrender,
    });
  }
  return out;
}

/** The war tab of the own guild. */
export function warView(f: ViewFacts): GuildWarView {
  const s = f.state;
  const own = s.own;
  const b = ownBrief(s);
  if (!own || !b) return EMPTY_GUILD_WAR;
  const enemy = own.enemy === 0 ? undefined : s.briefs.get(own.enemy);
  let startAt: number | null = null;
  if (own.enemy !== 0) {
    for (const w of s.wars.values()) {
      if (w.decl.guild === own.id || w.acpt.guild === own.id) startAt = w.startAt;
    }
  }
  return {
    enemyName: enemy?.name ?? '',
    enemyLogo: enemy?.logo ?? 0,
    win: b.win,
    lose: b.lose,
    surrender: b.surrender,
    winText: formatTemplate(f.text(TID_GAME_GUILD_WAR_DATA_WIN), [b.win]),
    loseText: formatTemplate(f.text(TID_GAME_GUILD_WAR_DATA_LOSE), [b.lose]),
    surrenderText: formatTemplate(f.text(TID_GAME_GUILD_WAR_DATA_SURRENDER), [b.surrender]),
    startAt,
  };
}

/** The view of the guild window; `version` is the caller's counter (it bumps whenever the store or a source changed). */
export function buildGuildView(f: ViewFacts, version: number): GuildView {
  const s = f.state;
  const own = s.own;
  const b = ownBrief(s);
  if (!own || !b) return { ...EMPTY_GUILD_VIEW, version, selfWar: s.selfWar !== 0 };
  const cap = maxMembers(f.levels, b.level);
  const counts = Array.from({ length: GUILD_RANKS }, (_, rank) =>
    rankCountText(rankCount(own, rank), rank < 4 ? (RANK_CAPS[rank] ?? 0) : cap),
  );
  return {
    version,
    hasGuild: true,
    name: b.name,
    level: b.level,
    masterName: f.member(b.idMaster)?.name ?? '',
    memberCount: own.members.size,
    maxMembers: cap,
    pxp: own.pxp,
    penya: b.guildGold,
    notice: own.notice,
    logo: b.logo,
    isMaster: isMasterOf(s, f.selfId),
    salaries: own.salaries,
    rankCounts: counts,
    members: memberRows(f),
    selfWar: s.selfWar !== 0,
    war: warView(f),
  };
}

/** The view of the two power pages: the draft, editable for the master only. */
export function buildPowerView(draft: PowerDraft, state: GuildState, selfId: number): PowerView {
  return {
    words: draft.words,
    editable: state.own !== null && isMasterOf(state, selfId),
    dirty: draft.dirty,
  };
}

/** Builds the views of the bank grid; the item views keep their identity while the item and the item bundle do not change. */
export class BankViewBuilder {
  readonly #items: ItemViewCache;
  #last: GuildBankView | null = null;
  #key: readonly unknown[] = [];

  constructor(data: ConstructorParameters<typeof ItemViewCache>[0]) {
    this.#items = new ItemViewCache(data);
  }

  build(
    state: GuildState,
    selfId: number,
    newFontColor: boolean,
    dataVersion: number,
    version: number,
  ): GuildBankView {
    const b = ownBrief(state);
    const key = [
      state.bank.container,
      b?.guildGold ?? 0,
      isMasterOf(state, selfId),
      newFontColor,
      dataVersion,
    ];
    if (
      this.#last &&
      key.length === this.#key.length &&
      key.every((v, i) => Object.is(v, this.#key[i]))
    ) {
      return this.#last;
    }
    const c = state.bank.container;
    const items = Array.from({ length: GUILD_BANK_SLOTS }, (_, slot) => {
      const it = c?.getAtSlot(slot) ?? null;
      return it ? this.#items.view(it) : null;
    });
    const view: GuildBankView = {
      version,
      items,
      gold: b?.guildGold ?? 0,
      isMaster: isMasterOf(state, selfId),
      newFontColor,
    };
    this.#last = view;
    this.#key = key;
    return view;
  }
}
