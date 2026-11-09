// How an entity is drawn (Plan/25 section 3.5): which sprite, how large, which colour and alpha, with which ring. Pure, so the rules are tested
// without a GL context. The sprites themselves are `spriteAtlas.ts`; colours are straight sRGB multipliers (the sprites are white or grey).
import { Flag, Kind, Rank, isBossRank } from './types';

/** The sprites of the atlas (cell index). */
export const Sprite = {
  ARROW: 0,
  DOT: 1,
  RING: 2,
  DASH_RING: 3,
  CROSS: 4,
  SQUARE: 5,
  DIAMOND: 6,
  STAR: 7,
  POS0: 8,
  POS1: 9,
  POS2: 10,
  POS3: 11,
  /** A soft round blob (heatmap, bins). */
  BLOB: 12,
} as const;
export type SpriteId = (typeof Sprite)[keyof typeof Sprite];

export type Rgb = readonly [number, number, number];

export const COLOR = {
  player: [1, 1, 1] as Rgb,
  friend: [0.35, 0.78, 0.4] as Rgb,
  guild: [0.38, 0.62, 1] as Rgb,
  pk: [1, 0.27, 0.27] as Rgb,
  gm: [1, 0.8, 0.2] as Rgb,
  monster: [1, 0.6, 0.15] as Rgb,
  aggressive: [1, 0.22, 0.22] as Rgb,
  captain: [1, 0.5, 0.1] as Rgb,
  boss: [1, 0.3, 0.1] as Rgb,
  dead: [0.55, 0.55, 0.58] as Rgb,
  npc: [0.1, 0.82, 0.76] as Rgb,
  pet: [0.76, 0.56, 1] as Rgb,
  item: [1, 0.9, 0.3] as Rgb,
  ctrl: [0.62, 0.72, 0.82] as Rgb,
  selected: [1, 1, 1] as Rgb,
  followed: [0.3, 0.9, 1] as Rgb,
  you: [1, 0.45, 0.8] as Rgb,
} as const;

/** One draw call's worth of an entity: the main sprite and the optional ring drawn around it. */
export interface Style {
  readonly sprite: SpriteId;
  /** Edge in CSS pixels at the nominal scale. */
  readonly size: number;
  readonly color: Rgb;
  readonly alpha: number;
  /** A sprite drawn under the main one (a ring: captain, GM, in combat), `null` for none. */
  readonly ring: SpriteId | null;
  readonly ringColor: Rgb;
  /** The main sprite turns with the yaw. */
  readonly rotates: boolean;
  /** Draw the ring pulsing (in combat). */
  readonly pulse: boolean;
}

/** What the style rules need to know beyond the entity itself. */
export interface StyleContext {
  /** Players of the guild of the followed or selected player (blue). */
  readonly guildOf?: (objId: number) => number;
  /** The guild id of the followed or selected player (`0`/absent: no highlight). */
  readonly focusGuild?: number;
  /** The party members of the followed player (green), when known. */
  readonly partyOf?: ReadonlySet<number>;
  /** The acting character's object id (the "You" badge). */
  readonly actingObjId?: number;
  /** Dim asleep monsters to 50 %. */
  readonly dimAsleep?: boolean;
}

export interface EntityLike {
  readonly id: number;
  readonly kind: number;
  readonly sub: number;
  readonly flags: number;
}

const RING_NONE = { ring: null, ringColor: COLOR.gm, pulse: false } as const;

/** The style of one entity (25 section 3.5). */
export function styleOf(e: EntityLike, ctx: StyleContext = {}): Style {
  const f = e.flags;
  switch (e.kind) {
    case Kind.PLAYER: {
      let color: Rgb = COLOR.player;
      if (ctx.partyOf?.has(e.id) === true) color = COLOR.friend;
      else if (
        ctx.focusGuild !== undefined &&
        ctx.focusGuild !== 0 &&
        ctx.guildOf?.(e.id) === ctx.focusGuild
      )
        color = COLOR.guild;
      if ((f & Flag.PK) !== 0) color = COLOR.pk;
      const gm = (f & Flag.GM) !== 0;
      const invisible = (f & Flag.INVISIBLE) !== 0;
      const you = ctx.actingObjId === e.id;
      return {
        sprite: Sprite.ARROW,
        size: 18,
        color,
        alpha: invisible ? 0.55 : 1,
        ring: invisible ? Sprite.DASH_RING : gm ? Sprite.RING : you ? Sprite.RING : null,
        ringColor: you && !gm && !invisible ? COLOR.you : COLOR.gm,
        rotates: true,
        pulse: false,
      };
    }
    case Kind.MONSTER: {
      const dead = (f & Flag.DEAD) !== 0;
      if (dead) {
        return {
          sprite: Sprite.CROSS,
          size: 9,
          color: COLOR.dead,
          alpha: 0.9,
          rotates: false,
          ...RING_NONE,
        };
      }
      const boss = isBossRank(e.sub) || (f & Flag.BOSS) !== 0;
      const captain = e.sub === Rank.CAPTAIN;
      const aggressive = (f & Flag.AGGRESSIVE) !== 0;
      const asleep = (f & Flag.ASLEEP) !== 0;
      const fighting = (f & Flag.FIGHTING) !== 0;
      return {
        sprite: boss ? Sprite.STAR : Sprite.DOT,
        size: boss ? 24 : captain ? 11 : 9,
        color: boss ? COLOR.boss : aggressive ? COLOR.aggressive : COLOR.monster,
        alpha: asleep && ctx.dimAsleep !== false ? 0.5 : 1,
        ring: fighting ? Sprite.RING : captain ? Sprite.RING : null,
        ringColor: fighting ? COLOR.aggressive : COLOR.captain,
        rotates: false,
        pulse: fighting,
      };
    }
    case Kind.NPC:
      return {
        sprite: Sprite.DOT,
        size: 10,
        color: COLOR.npc,
        alpha: 1,
        rotates: false,
        ...RING_NONE,
      };
    case Kind.PET:
      return {
        sprite: Sprite.DOT,
        size: 7,
        color: COLOR.pet,
        alpha: 1,
        rotates: false,
        ...RING_NONE,
      };
    case Kind.ITEM:
      return {
        sprite: Sprite.SQUARE,
        size: 7,
        color: COLOR.item,
        alpha: (f & Flag.OWNER_ONLY) !== 0 ? 0.65 : 1,
        rotates: false,
        ...RING_NONE,
      };
    default:
      return {
        sprite: Sprite.DIAMOND,
        size: 9,
        color: COLOR.ctrl,
        alpha: 1,
        rotates: false,
        ...RING_NONE,
      };
  }
}

/**
 * The scale of the sprites at a zoom: 1 up to 2 u/px, shrinking to 0.6 at 64 u/px and beyond, so that a tier-C overview with thousands of
 * players stays legible (the hit radius of the picking follows the nominal size).
 */
export function sizeScale(upp: number): number {
  if (upp <= 2) return 1;
  if (upp >= 64) return 0.6;
  return 1 - ((Math.log2(upp) - 1) / 5) * 0.4;
}

/** The size in pixels of a tier-C bin marker: grows with the count of monsters in the cell. */
export function binSize(count: number): number {
  return Math.min(44, 8 + 5 * Math.log2(1 + Math.max(0, count)));
}

/** The frame of the selection pulse (`ImgNavPos`, 4 frames, 120 ms each). */
export function pulseFrame(now: number): number {
  return Math.floor(now / 120) % 4;
}
