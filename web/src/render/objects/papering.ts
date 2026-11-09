// The wallpaper and the carpet of a room (Plan/41 section 19.1 "housing", Plan/45 section 28.10, WP-REND-27; `CDeployManager::ChangeWallTex / ChangeTileTex`, `Housing.cpp:685-930, 1240-1290`,
// `CGuildHouseBase::ApplyEFTexture`, `GuildHouse.cpp:245-288`, `CLandscape::ForceTexture`, `lod.cpp:785-802`):
//
//   wall    every instance of the room's wall model (`obj_miniwall01.o3d` in the mini room 209 and the small guild house 212) shows `Model/Texture/<file>` on EVERY material
//           (`GetObject3D()->SetTexture`); `null` = the default `Model/Texture/Obj_MiniWall01.dds` (`m_pOriginWall`)
//   floor   every VISIBLE terrain layer of the current world shows `Model/Texture/<file>` (DIR_MODELTEX also for carpets, KEEP; `ForceTexture`); `null` = the default
//           `World/Texture/Miniroom_floor01.dds` (`m_pOriginTile`, DIR_WORLDTEX)
//   scope   FIX: the original overwrote the SHARED terrain texture entry, so the floor stayed changed after leaving; here the override belongs to the current world and is cleared at every world
//           change (`OutProcessing`), and `world 0` (the player left the room) restores everything
//   file    `items[item].text_file`: the real items are `II_HOU_PAP_WAL01/02`, `II_HOU_PAP_CAR01/02` (`IK2_PAPERING`, mini room) and `II_GHOU_PAP_WAL01`, `II_GHOU_PAP_CAR01`
//           (`IK2_GUILDHOUSE_PAPERING`)
//
// The middle house (213: `obj_miniwall02.o3d`, `Obj_MiniWall02.dds`, `Miniroom_floor02.dds`, `Housing.cpp:1240-1290`) is a table entry only (F-292: the middle and the large guild house are DROP).

/** The wall model and the default textures of a room world. */
export interface PaperingRoom {
  /** The wall model file (lower case). */
  readonly wallModel: string;
  /** The default wall texture under `Model/Texture` (`m_pOriginWall`). */
  readonly wallDefault: string;
  /** The default floor texture under `World/Texture` (`m_pOriginTile`). */
  readonly floorDefault: string;
}

/** `WI_WORLD_MINIROOM` and the guild house worlds. */
export const PAPERING_ROOMS: Readonly<Record<number, PaperingRoom>> = {
  209: {
    wallModel: 'obj_miniwall01.o3d',
    wallDefault: 'Obj_MiniWall01.dds',
    floorDefault: 'Miniroom_floor01.dds',
  },
  212: {
    wallModel: 'obj_miniwall01.o3d',
    wallDefault: 'Obj_MiniWall01.dds',
    floorDefault: 'Miniroom_floor01.dds',
  },
  // DROP (F-292), untested
  213: {
    wallModel: 'obj_miniwall02.o3d',
    wallDefault: 'Obj_MiniWall02.dds',
    floorDefault: 'Miniroom_floor02.dds',
  },
};

/** What `roomPapering` is told. */
export interface PaperingRequest {
  readonly world: number;
  readonly wall: string | null;
  readonly floor: string | null;
}

/** The decision for a request: what the wall model and the floor show. */
export interface PaperingPlan {
  readonly wallModel: string;
  /** The wall texture file under `Model/Texture`. */
  readonly wallTexture: string;
  /** The floor texture as a VFS path (`model/texture/<file>` for an item, `world/texture/<default>` for the default). */
  readonly floorPath: string;
}

/** The plan of a request; `null` for a world that is no papered room (or `world 0`): everything is restored. */
export function planPapering(req: PaperingRequest): PaperingPlan | null {
  const room = PAPERING_ROOMS[req.world];
  if (!room) return null;
  return {
    wallModel: room.wallModel,
    wallTexture: req.wall ?? room.wallDefault,
    floorPath:
      req.floor !== null
        ? `model/texture/${req.floor.toLowerCase()}`
        : `world/texture/${room.floorDefault.toLowerCase()}`,
  };
}

/** The two consumers of a plan: the static objects (the wall) and the terrain (the floor). */
export interface PaperingTargets {
  setTextureOverride(file: string, textureFile: string | null): number;
  clearTextureOverrides(): void;
  forceTexture(path: string | null): void;
}

/** The probe row `render.papering()`. */
export interface PaperingProbe {
  readonly world: number;
  readonly wallModel: string | null;
  readonly wall: string | null;
  readonly floor: string | null;
  readonly groups: number;
  readonly applied: number;
}

export class Papering {
  private plan: PaperingPlan | null = null;
  private world = 0;
  private groups = 0;
  private applied = 0;

  constructor(private readonly targets: () => PaperingTargets | null) {}

  /** `fx.roomPapering`: shows the wall paper and the carpet of the room, or restores everything for `world 0` / a world that has no room. */
  apply(req: PaperingRequest): void {
    const t = this.targets();
    const plan = planPapering(req);
    this.world = req.world;
    this.applied++;
    if (!t) {
      this.plan = plan;
      return;
    }
    if (this.plan && plan?.wallModel !== this.plan.wallModel) {
      t.setTextureOverride(this.plan.wallModel, null);
    }
    if (plan === null) {
      t.clearTextureOverrides();
      t.forceTexture(null);
      this.plan = null;
      this.groups = 0;
      return;
    }
    this.groups = t.setTextureOverride(plan.wallModel, plan.wallTexture);
    t.forceTexture(plan.floorPath);
    this.plan = plan;
  }

  /** The renderer rebuilt its world (new statics, new terrain): shows the current plan again when the world is the same room. */
  reapply(worldId: number, req: PaperingRequest | null): void {
    if (req?.world !== worldId) {
      this.plan = null;
      this.world = 0;
      return;
    }
    this.plan = null;
    this.apply(req);
  }

  /** Forgets the plan (a world change) without touching the targets (they are new). */
  forget(): void {
    this.plan = null;
    this.world = 0;
    this.groups = 0;
  }

  probe(): PaperingProbe {
    return {
      world: this.world,
      wallModel: this.plan?.wallModel ?? null,
      wall: this.plan?.wallTexture ?? null,
      floor: this.plan?.floorPath ?? null,
      groups: this.groups,
      applied: this.applied,
    };
  }
}
