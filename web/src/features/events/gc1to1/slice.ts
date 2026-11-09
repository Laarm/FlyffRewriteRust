// `eventStore.gc1to1` (Plan/45 §28.0, §28.5; WP-CFEAT-45; ref/09b §3.16 "1:1 guild combat"; `src:_Common/GuildCombat1to1.cpp:703-767` the client manager): the 1:1 guild combat as the client keeps it.
// Every message is applied whether or not a window is open (PL-124); the slice is reset when the character leaves the world.

/** The manager states (`CGuildCombat1to1Mng::GC1TO1_*`): Close 0, Open 1 (tenders and line-ups), Entrance 2, War 3. */
export const GC1TO1_STATE = { Close: 0, Open: 1, Entrance: 2, War: 3 } as const;

/** The arena states (`GC1TO1_WARSTATE_*`, `Gc1to1NowState.state` 100 .. 104). */
export const GC1TO1_ARENA = {
  WarClose: 100,
  Wait: 101,
  War: 102,
  Finish: 103,
  CloseWait: 104,
} as const;

/** The player (round) states of `Gc1to1WarResult.state`: `GC1TO1_PLAYER_*` 200 .. 204. */
export const GC1TO1_PLAYER = { Ready: 200, War: 201, Win: 202, Lose: 203, Draw: 204 } as const;

/** The world ids of the four arenas (`WI_WORLD_GUILDWAR1TO1_0` .. `_3`, 241 .. 244). */
export const GC1TO1_WORLDS = [241, 242, 243, 244] as const;

/** `GetCurrentState`: the line-up limits of `Gc1to1MemberLineUpOpenWndNtf`. */
export interface Gc1to1Limits {
  readonly minLevel: number;
  readonly min: number;
  readonly max: number;
}

export interface Gc1to1Slice {
  /** `m_nState` of the manager / arena: any value of the last `Gc1to1NowState` (0 Close .. 3 War, 100 .. 104; the original stored EVERY value, so the invite rule also refuses in the arena states). */
  readonly state: number;
  /** The own guild's line-up (character ids, fight order): the `members` of the entrance message. */
  readonly lineup: readonly number[];
  /** The opposing guild (`enemy_guild`), 0 = not known. */
  readonly opponentGuild: number;
  /** The round counter of the arena (`prog_count`, 0-based). */
  readonly round: number;
  /** The WAIT state (101) runs: the HUD timer counts seconds (`waitSec`). */
  readonly waiting: boolean;
  /** The round states of the own line-up by character id (200 .. 204). */
  readonly results: ReadonlyMap<number, number>;
  /** The round wins `[ours, theirs]` (the server sends per receiver). */
  readonly wins: readonly [number, number];
  /** The limits of the last line-up window. */
  readonly limits: Gc1to1Limits;
}

export function initialGc1to1(): Gc1to1Slice {
  return {
    state: GC1TO1_STATE.Close,
    lineup: [],
    opponentGuild: 0,
    round: 0,
    waiting: false,
    results: new Map(),
    wins: [0, 0],
    limits: { minLevel: 0, min: 0, max: 0 },
  };
}
