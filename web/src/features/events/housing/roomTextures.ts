// The textures of the room the player stands in (Plan/45 §28.10, Plan/41 WP-REND-27; `CDeployManager::ChangeWallTex / ChangeTileTex`, `CGuildHouseBase::ApplyEFTexture`): for render-audio-m7.
//
//   world 209          the wallpaper and the carpet of `housing.papering` (the prop's `text_file` under `Model/Texture`)
//   world 212 - 214    the LAST installed row (list order) of the kind `IK2_GUILDHOUSE_FURNITURE` / `_PAPERING` with `IK3_WALLPAPER` / `IK3_CARPET`
//   elsewhere          `world 0`: the renderer restores the defaults (`OutProcessing`)
//
// `null` is the default texture (`Obj_MiniWall01.dds` for the wall, `World/Texture/Miniroom_floor01.dds` for the floor).

import type { ClientContext } from '../../../app/context';
import {
  IK2_GUILDHOUSE_FURNITURE,
  IK2_GUILDHOUSE_PAPERING,
  IK3_CARPET,
  IK3_WALLPAPER,
} from '../../../gen/defs/itemkind';
import { derivedView, type ViewSource } from '../../common/view';
import { isGuildHouseWorld } from '../../loading/images';
import { eventOf } from '../api';
import { WORLD_MINIROOM } from './slice';

export interface RoomTextures {
  /** The world the textures belong to (0 = none: restore). */
  readonly world: number;
  /** `prop.text_file` of the wall paper, `null` = the default. */
  readonly wall: string | null;
  readonly floor: string | null;
}

export const NO_ROOM_TEXTURES: RoomTextures = { world: 0, wall: null, floor: null };

const sameTextures = (a: RoomTextures, b: RoomTextures): boolean =>
  a.world === b.world && a.wall === b.wall && a.floor === b.floor;

/** Computes the textures from the stores (the pure core; the tests call it). */
export function computeRoomTextures(ctx: ClientContext, worldId: number): RoomTextures {
  const state = eventOf(ctx)?.store.getState();
  const file = (itemId: number | null): string | null => {
    if (itemId === null) return null;
    const t = ctx.data?.item(itemId)?.text_file ?? '';
    return t === '' ? null : t;
  };
  if (worldId === WORLD_MINIROOM) {
    const p = state?.housing.papering;
    return { world: worldId, wall: file(p?.wall ?? null), floor: file(p?.floor ?? null) };
  }
  if (isGuildHouseWorld(worldId)) {
    let wall: string | null = null;
    let floor: string | null = null;
    for (const row of state?.guildHouse.house?.rows ?? []) {
      if (!row.setup) continue;
      const prop = ctx.data?.item(row.itemId);
      if (
        prop?.item_kind2 !== IK2_GUILDHOUSE_FURNITURE &&
        prop?.item_kind2 !== IK2_GUILDHOUSE_PAPERING
      ) {
        continue;
      }
      if (prop.item_kind3 === IK3_WALLPAPER) wall = file(row.itemId);
      else if (prop.item_kind3 === IK3_CARPET) floor = file(row.itemId);
    }
    return { world: worldId, wall, floor };
  }
  return NO_ROOM_TEXTURES;
}

/** `roomTexturesOf( ctx )`: a view source over the events store and the world store. */
export function roomTexturesOf(ctx: ClientContext): ViewSource<RoomTextures> {
  const events = eventOf(ctx);
  const sources = [ctx.stores.worldStore, ...(events ? [events.store] : [])];
  return derivedView(
    () => computeRoomTextures(ctx, ctx.stores.worldStore.getState().worldId),
    sources,
    sameTextures,
  );
}
