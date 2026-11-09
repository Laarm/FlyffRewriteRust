// Constants of the map protocol that the engine needs by name (Plan/25 section 3.15 "Encoding of MapFlags and sub", 3.16). The numbers equal the
// `EntityKind` / `Tier` enums of `gen/proto/admin` (checked by `types.test.ts`); they are repeated here as plain constants so that the pure
// modules of the engine (store, picking, labels, filters) do not import the protobuf runtime.

/** `EntityKind`. */
export const Kind = {
  PLAYER: 1,
  MONSTER: 2,
  NPC: 3,
  PET: 4,
  ITEM: 5,
  CTRL: 6,
} as const;
export type KindId = (typeof Kind)[keyof typeof Kind];

/** `Tier`. */
export const TierId = { A: 1, B: 2, C: 3 } as const;

/** `MapFlags` bits. */
export const Flag = {
  ASLEEP: 1 << 0,
  DEAD: 1 << 1,
  FIGHTING: 1 << 2,
  AGGRESSIVE: 1 << 3,
  PK: 1 << 4,
  GM: 1 << 5,
  INVISIBLE: 1 << 6,
  OWNER_ONLY: 1 << 7,
  BOSS: 1 << 8,
  FLYING: 1 << 9,
} as const;

/**
 * The rank codes of a monster's `sub` (0 is normal; the zero is not on the wire). 3 boss is `RANK_SUPER` (Clockworks, Rangda, Asmodan, the
 * instance bosses), 4 giant is `RANK_BOSS` (the "Giant X" monsters, Great White Bolo, the dungeon giants and leaders).
 */
export const Rank = { NORMAL: 0, CAPTAIN: 1, MIDBOSS: 2, BOSS: 3, GIANT: 4, GUARD: 5 } as const;

/** Mid-boss, boss and giant: individual markers at every tier. */
export function isBossRank(rank: number): boolean {
  return rank === Rank.MIDBOSS || rank === Rank.BOSS || rank === Rank.GIANT;
}

/** `1 << kind` for each kind in the list: the `kinds` bitset of a subscription. */
export function kindsMask(kinds: readonly number[]): number {
  let bits = 0;
  for (const k of kinds) bits |= 1 << k;
  return bits >>> 0;
}

/** The names of the kinds as the Layers panel and the lists show them. */
export const KIND_LABEL: Readonly<Record<number, string>> = {
  [Kind.PLAYER]: 'Players',
  [Kind.MONSTER]: 'Monsters',
  [Kind.NPC]: 'NPCs',
  [Kind.PET]: 'Pets',
  [Kind.ITEM]: 'Items on ground',
  [Kind.CTRL]: 'Ctrls',
};

/** The name of a monster rank. */
export function rankName(rank: number): string {
  switch (rank) {
    case Rank.CAPTAIN:
      return 'captain';
    case Rank.MIDBOSS:
      return 'mid-boss';
    case Rank.BOSS:
      return 'boss';
    case Rank.GIANT:
      return 'giant';
    case Rank.GUARD:
      return 'guard';
    default:
      return 'normal';
  }
}

/** The nominal update interval of a tier in milliseconds (3, 8 and 30 ticks of 15 Hz). */
export function nominalIntervalMs(tier: number): number {
  switch (tier) {
    case TierId.B:
      return Math.round((8 / 15) * 1000);
    case TierId.C:
      return 2000;
    default:
      return 200;
  }
}

/** The authority letters from which a player counts as a game master ("gold ring", Plan/25 section 3.5). */
export const GM_FLAG = Flag.GM;
