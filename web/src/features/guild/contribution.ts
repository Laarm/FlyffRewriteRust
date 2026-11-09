// `CGuild::CanContribute` and the texts of `CGuild::MeritResultMsg` (`src:_Common/guild.cpp:557-610`, `_Interface/WndGuildMerit.cpp:60-100`; Plan/45 §23, 21 §5.3; WP-CFEAT-37): the
// pre-check the contribution window runs before it sends `GuildContributionReq`. The numbers are the original's: the guild's points and penya and the member's counters wrap
// like `DWORD` (the member's penya like `int`); the server checks again with checked arithmetic (`add_to`).

import {
  TID_GAME_GUILDMAXBANKGOLD,
  TID_GAME_GUILDMERITMAXGOLD,
  TID_GAME_GUILDMERITMAXLEVEL,
  TID_GAME_GUILDMERITSTUFF,
  TID_GAME_GUILDMERITSYSERROR,
} from '../../gen/defs/text';

/** `CONTRIBUTION_RESULT`. */
export type ContributionResult =
  | 'ok'
  /** `CONTRIBUTION_FAIL_MAXLEVEL`: points at the top level. */
  | 'maxLevel'
  | 'guildOverflowPxp'
  | 'guildOverflowPenya'
  /** `CONTRIBUTION_FAIL_INVALID_CONDITION`: the player is no member. */
  | 'invalid'
  | 'overflowPxp'
  | 'overflowPenya';

export interface ContributionGuild {
  readonly level: number;
  /** `m_dwContributionPxp`. */
  readonly pxp: number;
  /** `m_nGoldGuild`. */
  readonly penya: number;
  /** `CGuildTable::GetMaxLevel()`: 50 (the length of the level table). */
  readonly maxLevel: number;
}

export interface ContributionMember {
  /** `m_dwGivePxpCount`. */
  readonly givePxp: number;
  /** `m_nGiveGold`. */
  readonly giveGold: number;
}

const u32 = (n: number): number => n >>> 0;
const i32 = (n: number): number => n | 0;

/** `CanContribute( dwPxp, dwPenya, idPlayer )` with the member's record (`null` = the player is no member). */
export function canContribute(
  guild: ContributionGuild,
  member: ContributionMember | null,
  pxp: number,
  penya: number,
): ContributionResult {
  if (pxp > 0 && guild.level >= guild.maxLevel) return 'maxLevel';
  if (u32(guild.pxp + pxp) < u32(guild.pxp)) return 'guildOverflowPxp';
  if (u32(guild.penya + penya) < u32(guild.penya)) return 'guildOverflowPenya';
  if (member === null) return 'invalid';
  if (u32(member.givePxp + pxp) < u32(member.givePxp)) return 'overflowPxp';
  if (i32(member.giveGold + i32(penya)) < i32(member.giveGold)) return 'overflowPenya';
  return 'ok';
}

/**
 * The line `MeritResultMsg` prints for a result: `TID_GAME_GUILDMERITSTUFF` for `ok` (FIX: this client prints it when the server's `GuildContributionNtf` arrives, not when the
 * request is sent), `0` for the two PXP overflows (the original's switch has no case for them).
 */
export function meritResultTid(result: ContributionResult): number {
  switch (result) {
    case 'ok':
      return TID_GAME_GUILDMERITSTUFF;
    case 'maxLevel':
      return TID_GAME_GUILDMERITMAXLEVEL;
    case 'guildOverflowPenya':
      return TID_GAME_GUILDMAXBANKGOLD;
    case 'invalid':
      return TID_GAME_GUILDMERITSYSERROR;
    case 'overflowPenya':
      return TID_GAME_GUILDMERITMAXGOLD;
    case 'guildOverflowPxp':
    case 'overflowPxp':
      return 0;
  }
}

/** A gem stack of the bag: its item level (`ItemProp::item_lv`) and the pieces. */
export interface GemStack {
  readonly itemLv: number;
  readonly count: number;
}

/** The contribution points the server gives for the gem stacks: the sum of `((item_lv + 1) / 2) * count` (integer division). */
export function gemPoints(stacks: readonly GemStack[]): number {
  let total = 0;
  for (const s of stacks) total += Math.floor((s.itemLv + 1) / 2) * s.count;
  return total;
}
