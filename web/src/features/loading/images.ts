// The loading image of a destination world (Plan/45 §6.1, ref/09a §6.3; `CDPClient::OnReplace`, `DPClient.cpp:2339-2368`).

import {
  WI_GUILDHOUSE_LARGE,
  WI_GUILDHOUSE_MIDDLE,
  WI_GUILDHOUSE_SMALL,
  WI_INSTANCE_DREADFULCAVE,
  WI_INSTANCE_OMINOUS,
  WI_INSTANCE_OMINOUS_1,
  WI_WORLD_GUILDWAR,
  WI_WORLD_GUILDWAR1TO1_0,
  WI_WORLD_GUILDWAR1TO1_L,
} from '../../gen/defs/world';

/** The three images of an ordinary map change (`szMapFile`), picked uniformly at random. */
export const RANDOM_LOADING_IMAGES: readonly string[] = [
  'MapLoading2.tga',
  'MapLoading3.tga',
  'MapLoading4.tga',
];

export type LoadingImageKind =
  'guildWar' | 'ominous' | 'ominous1' | 'dreadfulCave' | 'guildHouse' | 'random';

export interface LoadingImage {
  readonly kind: LoadingImageKind;
  /** The file in `Theme/` (a TGA; the server serves a converted PNG). */
  readonly file: string;
}

export interface LoadingImageOptions {
  /** `GuildHouse->IsMyGuildHouse(worldId)`. */
  readonly ownGuildHouse?: boolean;
  /** Uniform in `[0, 1)` (default `Math.random`). */
  readonly random?: () => number;
}

/** `true` for the worlds of a guild-house id (the own guild's house is one of them). */
export function isGuildHouseWorld(worldId: number): boolean {
  return (
    worldId === WI_GUILDHOUSE_SMALL ||
    worldId === WI_GUILDHOUSE_MIDDLE ||
    worldId === WI_GUILDHOUSE_LARGE
  );
}

export function loadingImageFor(worldId: number, options: LoadingImageOptions = {}): LoadingImage {
  if (
    worldId === WI_WORLD_GUILDWAR ||
    (worldId >= WI_WORLD_GUILDWAR1TO1_0 && worldId <= WI_WORLD_GUILDWAR1TO1_L)
  ) {
    return { kind: 'guildWar', file: 'MapLoading_GuildWar.tga' };
  }
  if (worldId === WI_INSTANCE_OMINOUS) return { kind: 'ominous', file: 'MapLoading_Ominous.tga' };
  if (worldId === WI_INSTANCE_OMINOUS_1) {
    return { kind: 'ominous1', file: 'MapLoading_Ominous_1.tga' };
  }
  if (worldId === WI_INSTANCE_DREADFULCAVE) {
    return { kind: 'dreadfulCave', file: 'MapLoading_DreadfulCave.tga' };
  }
  if (options.ownGuildHouse === true) {
    return { kind: 'guildHouse', file: 'MapLoading_Guildhouse.tga' };
  }
  const random = options.random ?? Math.random;
  const i = Math.min(
    RANDOM_LOADING_IMAGES.length - 1,
    Math.floor(random() * RANDOM_LOADING_IMAGES.length),
  );
  return { kind: 'random', file: RANDOM_LOADING_IMAGES[i] ?? 'MapLoading2.tga' };
}

/** The legacy fade: `m_nAlpha -= 6` per rendered frame, from 255 until it reaches 0. */
export const FADE_STEP = 6;
export function fadeStep(alpha: number): number {
  return Math.max(0, alpha - FADE_STEP);
}
