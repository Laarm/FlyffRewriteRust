// `guildStore` (Plan/45 §2.3, §23; WP-CFEAT-37, -38): the mirror of `g_GuildMng` (`CGuildMng`, `_Common/guild.h`) with the pieces of the client that hang on it: every guild of the server
// in brief (`AllGuilds` and the notifications), the full record of the own guild (powers, salaries, notice, members with their contribution), the guild wars the client knows
// (`g_GuildWarMng`), the own war id (`g_pPlayer->m_idWar`), the guild bank and the four bank logs. Scope `world`: the join batch of every world sends `AllGuilds`, `Guild` and `War`
// again. Names, levels and jobs of members live in the player-data cache (`features/common/playerData.ts`), the online state in the messenger's presence overlay.
//
// The briefs are the wire records themselves, REPLACED and never mutated, so that `MoverVisualState.guild` can share them (the renderer reads name, level and logo from it).

import type { StoreApi } from 'zustand/vanilla';
import type { GuildBrief } from '../../gen/proto/flyff/v1/social_types_pb';
import type { StoreRegistry } from '../../stores/registry';
import type { ItemContainer } from '../inventory/container';
import type { InvItem } from '../inventory/items';

import { GUD, GUILD_BANK_SLOTS } from '../../ui/windows/guild/ports';

export { GUD, GUILD_BANK_SLOTS };

/** `CGuildMember` without the name (that is the cache's). */
export interface MemberRecord {
  /** `m_nMemberLv`, 0 master .. 4 rookie. */
  readonly rank: number;
  /** `m_nClass`, 0..2. */
  readonly cls: number;
  readonly alias: string;
  /** `m_nGiveGold` (may be negative after withdrawals). */
  readonly giveGold: number;
  /** `m_dwGivePxpCount`. */
  readonly givePxp: number;
  /** `m_nSurrender`. */
  readonly surrender: number;
  readonly win: number;
  readonly lose: number;
}

/** The full record of the own guild (`CGuild` after `OnGuild`). */
export interface OwnGuild {
  readonly id: number;
  /** `m_adwPower[5]`. */
  readonly powers: readonly number[];
  /** `m_adwPenya[5]`: the salary of each member level. */
  readonly salaries: readonly number[];
  readonly notice: string;
  /** `m_dwContributionPxp`. */
  readonly pxp: number;
  /** `m_idEnemyGuild`, 0 = none. */
  readonly enemy: number;
  /** The members by character id (ascending: `m_mapPMember`). */
  readonly members: ReadonlyMap<number, MemberRecord>;
}

/** One side of a war (`WAR_ENTRY`). */
export interface WarSide {
  readonly guild: number;
  readonly size: number;
  readonly surrender: number;
  readonly dead: number;
  readonly absent: number;
}

/** `CGuildWar`. */
export interface WarState {
  readonly id: number;
  readonly decl: WarSide;
  readonly acpt: WarSide;
  /** The `WF_*` flag as its ASCII value ('0' = `WF_WARTIME`). */
  readonly flag: number;
  /** Unix seconds. */
  readonly startAt: number;
}

export interface GuildBankState {
  /** The 42 slots (`m_GuildBank`), `null` before the first `GuildBankWndNtf`. */
  readonly container: ItemContainer<InvItem> | null;
  /** The bank window is open (`GuildBankWndNtf{mode 0}` until it closes). */
  readonly open: boolean;
}

/** Index 0..3 = the lists of kinds 1..4 (the formatted lines, newest first). */
export type BankLogs = readonly (readonly string[])[];

export interface GuildState {
  /** Every known guild in brief, by guild id. */
  readonly briefs: ReadonlyMap<number, GuildBrief>;
  /** The own guild; `null` without one. */
  readonly own: OwnGuild | null;
  readonly wars: ReadonlyMap<number, WarState>;
  /** `g_pPlayer->m_idWar`, 0 = none. */
  readonly selfWar: number;
  readonly bank: GuildBankState;
  readonly log: BankLogs;
  /** `AllGuilds.next_id`. */
  readonly nextId: number;
}

export const EMPTY_LOGS: BankLogs = [[], [], [], []];

export function initialGuildState(): GuildState {
  return {
    briefs: new Map(),
    own: null,
    wars: new Map(),
    selfWar: 0,
    bank: { container: null, open: false },
    log: EMPTY_LOGS,
    nextId: 0,
  };
}

export type GuildStore = StoreApi<GuildState>;

/** The store of a registry (created on first use). */
export function defineGuildStore(registry: StoreRegistry): GuildStore {
  const existing = registry.get('guild');
  if (existing) return existing as unknown as GuildStore;
  return registry.defineStore<GuildState>({
    name: 'guild',
    scope: 'world',
    owner: '45',
    initial: initialGuildState,
  });
}

// ---- reads ------------------------------------------------------------------------------------------------------------------------------

/** The brief of the own guild. */
export function ownBrief(s: GuildState): GuildBrief | null {
  return s.own ? (s.briefs.get(s.own.id) ?? null) : null;
}

/** `CGuild::IsMaster( id )`: the master's rank is 0. */
export function isMasterOf(s: GuildState, charId: number): boolean {
  return s.own?.members.get(charId)?.rank === GUD.MASTER;
}

/** The member level of a character in the own guild, `-1` for none. */
export function rankOf(s: GuildState, charId: number): number {
  return s.own?.members.get(charId)?.rank ?? -1;
}

/** `CGuild::GetMemberLvSize( rank )`: the head count of a member level. */
export function rankCount(own: OwnGuild, rank: number): number {
  let n = 0;
  for (const m of own.members.values()) if (m.rank === rank) n += 1;
  return n;
}
