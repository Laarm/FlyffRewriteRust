// The mover dots of the navigator (`CWndNavigator::OnEraseBkgnd`, `src:_Interface/WndField.cpp:9662-9830`; ref/99 §4.4; Plan/45 §7.6;
// WP-CFEAT-28): which movers the minimap shows, with which frame of `Theme/navigator.inc`, at which offset.
//
//   frame 0 other player   1 peaceful NPC   2 monster   3 guild member   4 party member   5 the player who killed you / the focus
//   21 MIDBOSS, SUPER and BOSS monsters (`features.navigator_indicator`)   6 + structure: the NPC's `character.inc` structure icon
//   20 + emoticon: a quest NPC (hidden while the data has no quests)
//
// The decision order of the legacy `if` chain is kept: quest NPC, murderer, peaceful NPC, guild, monster, player (party or not). The
// filters (`OBJFILTER_*` of the place menu) apply to everything but the player who last killed you, which is always shown.

import { RANK_BOSS, RANK_MIDBOSS, RANK_SUPER } from '../../gen/defs/attribute';

/** The place menu (`m_bObjFilterPlayer/Party/NPC/Monster`): persisted with the window. */
export interface NavFilters {
  readonly player: boolean;
  readonly party: boolean;
  readonly npc: boolean;
  readonly monster: boolean;
}

export const DEFAULT_FILTERS: NavFilters = { player: true, party: true, npc: true, monster: true };

/** The `OBJFILTER_*` ids of the place menu items. */
export const OBJFILTER = { PLAYER: 0, PARTY: 1, NPC: 2, MONSTER: 3 } as const;

/** What the dot logic reads of one mover. */
export interface MoverSample {
  /** The object id (`GetId()`). */
  readonly id: number;
  /** `m_idPlayer` of a player, 0 for an NPC. */
  readonly idPlayer: number;
  readonly isPlayer: boolean;
  /** `IsPeaceful()` of an NPC. */
  readonly peaceful: boolean;
  /** `TRANSPARENT_MODE`. */
  readonly transparent: boolean;
  /** `IsAuthHigher( AUTH_GAMEMASTER )`. */
  readonly gm: boolean;
  /** `m_idGuild` (0 = none). */
  readonly guildId: number;
  /** `MoverProp.dwClass` of a monster (`RANK_*`), -1 for none. */
  readonly rank: number;
  /** `character.inc` `m_nStructure`, -1 for none. */
  readonly structure: number;
  /** `m_nQuestEmoticonIndex`, -1 for none. */
  readonly questEmoticon: number;
  /** World position (LH). */
  readonly x: number;
  readonly z: number;
  readonly name: string;
}

/** What the dot logic reads of the rest of the client. */
export interface NavEnv {
  readonly filters: NavFilters;
  readonly selfId: number;
  readonly selfGuildId: number;
  /** `m_idMurderer`: the player who killed the character last (0 = none). */
  readonly murdererId: number;
  /** `g_Party.IsMember( idPlayer )`. */
  isPartyMember(idPlayer: number): boolean;
  /** `features.navigator_indicator` (`__NAVIGATOR_INDICATOR`). */
  readonly indicator: boolean;
}

/** One dot (or structure icon) of the minimap. */
export interface Dot {
  readonly id: number;
  /** The frame of `navigator.inc`: the dot frame, or `6 + structure` for an icon, `20 + emoticon` for a quest NPC. */
  readonly frame: number;
  /** `dot`: the 5 x 5 frames (drawn at `-2`, the boss indicator 21 is a 16 x 16 frame at the same offset), `icon`: the structure icons (`-8`), `quest`: the quest marks (`-5`). */
  readonly kind: 'dot' | 'icon' | 'quest';
  readonly x: number;
  readonly z: number;
  readonly name: string;
}

export const DOT_SIZE = 5;
export const DOT_OFFSET = 2;
export const ICON_SIZE = 16;
export const ICON_OFFSET = 8;
export const QUEST_OFFSET = 5;

/** `IsRank( RANK_MIDBOSS ) || IsRank( RANK_SUPER ) || IsRank( RANK_BOSS )`. */
export function isBossRank(rank: number): boolean {
  return rank === RANK_MIDBOSS || rank === RANK_SUPER || rank === RANK_BOSS;
}

/** The place filters of the legacy loop for a mover that is not the murderer; `true` = the mover is skipped. */
export function filteredOut(m: MoverSample, env: NavEnv): boolean {
  const f = env.filters;
  if (!f.player && m.isPlayer) {
    // the player filter is off: only a party member survives, and only while the party filter is on
    if (!f.party) return true;
    if (!env.isPartyMember(m.idPlayer)) return true;
  }
  if (!f.monster && !m.isPlayer && !m.peaceful) return true;
  if (!f.npc && !m.isPlayer && m.peaceful) {
    // the NPC filter keeps the NPCs with a structure icon
    if (m.structure < 0) return true;
  }
  return m.transparent || m.gm;
}

/**
 * The frame of the dot of a mover, `null` when the minimap does not show it. The own character is never a dot (the arrow is drawn
 * in the centre).
 */
export function classify(m: MoverSample, env: NavEnv): Dot | null {
  if (m.id === env.selfId) return null;
  const murderer = env.murdererId !== 0 && m.isPlayer && m.idPlayer === env.murdererId;
  if (!murderer && filteredOut(m, env)) return null;
  const npc = !m.isPlayer;
  let frame: number;
  if (npc && m.questEmoticon >= 0) {
    // the quest NPCs are drawn after the others, 10 px larger than a dot (`20 + emoticon` at -5)
    return { id: m.id, frame: 20 + m.questEmoticon, kind: 'quest', x: m.x, z: m.z, name: m.name };
  } else if (murderer) {
    frame = 5;
  } else if (npc && m.peaceful) {
    frame = 1;
  } else if (env.selfGuildId !== 0 && env.selfGuildId === m.guildId) {
    frame = 3;
  } else if (npc && !m.peaceful) {
    frame = env.indicator && isBossRank(m.rank) ? 21 : 2;
  } else if (m.isPlayer) {
    frame = env.isPartyMember(m.idPlayer) ? 4 : 0;
  } else {
    frame = 0;
  }
  if (m.structure >= 0) {
    return { id: m.id, frame: 6 + m.structure, kind: 'icon', x: m.x, z: m.z, name: m.name };
  }
  return { id: m.id, frame, kind: 'dot', x: m.x, z: m.z, name: m.name };
}

/** The top-left offset of a dot relative to its centre point, and its size. */
export function dotBox(d: Dot): { dx: number; dy: number; size: number } {
  if (d.kind === 'icon') return { dx: -ICON_OFFSET, dy: -ICON_OFFSET, size: ICON_SIZE };
  if (d.kind === 'quest') return { dx: -QUEST_OFFSET, dy: -QUEST_OFFSET, size: ICON_SIZE };
  return { dx: -DOT_OFFSET, dy: -DOT_OFFSET, size: DOT_SIZE };
}

/** The 5 x 5 hit box of a dot at the centre point `(x, y)`: the drawn dot (the legacy click test uses `x - 2 .. x + 3`). */
export function dotHit(x: number, y: number, px: number, py: number): boolean {
  return (
    px >= x - DOT_OFFSET &&
    px < x - DOT_OFFSET + DOT_SIZE &&
    py >= y - DOT_OFFSET &&
    py < y - DOT_OFFSET + DOT_SIZE
  );
}
