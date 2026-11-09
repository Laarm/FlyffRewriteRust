// The documents of the world routes (Plan/25 section 3.3, 3.4, 3.13, 3.9, 11.3): `GET /admin/api/worlds`,
// `GET /admin/api/world/{id}/meta` and `GET /admin/api/world/find`. Transcribed from `crates/flyff-server/src/admin/api/world/meta.rs`
// (the structs derive `ts_rs::TS` and `meta::declare_ts` / `find::declare_ts` write the same declarations into `gen/dto.ts`;
// `metaTypes.test.ts` makes `tsc -b` fail when the two drift apart). Types only: importing this file costs nothing.

/** The groups of the world selector, in selector order. */
export type WorldGroup =
  'fields' | 'dungeons' | 'instances' | 'pvp_events' | 'housing' | 'secret_rooms';

/** Which layer a click on a location of the world should open (Plan/25 section 3.13). */
export type LayerPolicy = 'fixed0' | 'pick_live';

export interface GroupRow {
  readonly id: WorldGroup;
  readonly label: string;
}

/** The minimap of a world in the world list. */
export interface TilesSummary {
  /** Landscapes with a tile. */
  readonly covered: number;
  readonly lands: number;
  /** The highest overview level. */
  readonly max_level: number;
}

export interface WorldRow {
  readonly id: number;
  readonly name: string;
  readonly folder: string;
  readonly define: string;
  readonly group: WorldGroup;
  readonly lands: readonly [number, number];
  readonly mpu: number;
  readonly land_m: number;
  readonly extent_m: readonly [number, number];
  readonly indoor: boolean;
  readonly fly: boolean;
  readonly pk_mode: number;
  readonly tiles: TilesSummary | null;
  readonly layer_policy: LayerPolicy;
  /** Players in the world, in all layers. */
  readonly online: number;
}

export interface WorldsDoc {
  readonly worlds: readonly WorldRow[];
  readonly groups: readonly GroupRow[];
}

export interface Geometry {
  readonly lands: readonly [number, number];
  readonly mpu: number;
  /** The side of a landscape in metres (`128 * mpu`). */
  readonly land_m: number;
  readonly extent_m: readonly [number, number];
  readonly indoor: boolean;
  readonly fly: boolean;
  readonly pk_mode: number;
}

/** The default camera of the `.wld`. */
export interface Camera {
  readonly x: number;
  readonly y: number;
  readonly z: number;
  readonly yaw: number;
  readonly pitch: number;
}

export interface LevelMeta {
  /** The level (1..). */
  readonly k: number;
  readonly cols: number;
  readonly rows: number;
  /** Tiles that exist. */
  readonly tiles: number;
}

/** Where the minimap of the world is. */
export interface TilesMeta {
  /** The file-name prefix of the level-0 tiles (`wdmadrigal`): `<prefix>XX-ZZ.dds`, two digits at least. */
  readonly prefix: string;
  /** `/res/<fp>/world/<folder>/`, `null` when the folder is not client visible. */
  readonly res: string | null;
  /** `/gen/admin-map/<fp>/`: `<id>/<k>/<tx>_<tz>.png` and `world-<id>.tiles.json`. */
  readonly gen: string | null;
  readonly tile_px: number;
  readonly max_level: number;
  readonly covered_count: number;
  /** One string per landscape row (row 0 = z 0 first), `1` for a landscape with a tile. */
  readonly covered: readonly string[];
  readonly levels: readonly LevelMeta[];
}

export interface RegionDoc {
  /** `dwIndex`: 10 trigger, 11 attribute, 12 begin, 13 revival, 14 structure, 15 place. */
  readonly index: number;
  readonly kind: string;
  readonly key: string;
  readonly title: string | null;
  readonly desc: string | null;
  /** `[x0, z0, x1, z1]` in metres. */
  readonly rect: readonly [number, number, number, number];
  /** The `RA_*` attribute flags. */
  readonly attribute: number;
  readonly to_world: number | null;
}

export interface ContinentDoc {
  readonly id: number;
  readonly name: string;
  readonly town: boolean;
  readonly gameplay: boolean;
  /** `[x, z]`, not closed. */
  readonly polygon: readonly (readonly [number, number])[];
}

export interface RevivalDoc {
  readonly key: string;
  readonly title: string | null;
  readonly pos: readonly [number, number, number];
  readonly rect: readonly [number, number, number, number];
}

export interface PortalDoc {
  readonly rect: readonly [number, number, number, number];
  readonly title: string | null;
  readonly to_world: number;
  readonly to_name: string;
  readonly to_pos: readonly [number, number, number];
}

export interface SpawnDoc {
  readonly row: number;
  /** `monster`, `ctrl` or `item`. */
  readonly kind: string;
  readonly prop: number;
  readonly name: string;
  readonly level: number | null;
  readonly rect: readonly [number, number, number, number];
  readonly count: number;
  readonly time_s: number;
}

export interface NpcDoc {
  readonly key: string;
  readonly name: string;
  readonly prop: number;
  readonly x: number;
  readonly z: number;
  readonly angle: number;
}

export interface TeleportDoc {
  readonly name: string;
  readonly pos: readonly [number, number, number];
  readonly image: string;
  readonly min_level: number;
  readonly max_level: number;
}

export interface MetaDoc {
  readonly id: number;
  readonly name: string;
  readonly folder: string;
  readonly define: string;
  readonly group: WorldGroup;
  readonly layer_policy: LayerPolicy;
  readonly geometry: Geometry;
  readonly camera: Camera | null;
  readonly tiles: TilesMeta | null;
  readonly regions: readonly RegionDoc[];
  readonly continents: readonly ContinentDoc[];
  readonly revival: readonly RevivalDoc[];
  readonly portals: readonly PortalDoc[];
  readonly spawns: readonly SpawnDoc[];
  readonly npcs: readonly NpcDoc[];
  readonly teleports: readonly TeleportDoc[];
}

// ---- GET /admin/api/world/find ----------------------------------------------------------------------------------------------------

/** What a hit of the find route is. */
export type FindKind = 'player' | 'monster' | 'npc' | 'pet' | 'region' | 'location';

export interface FindHit {
  readonly kind: FindKind;
  /** The object id of a live hit, `0` for a region or a location. */
  readonly obj_id: number;
  /** The character id of a player, `0` otherwise. */
  readonly character_id: number;
  readonly prop: number;
  readonly name: string;
  /** A second line: the description of a region, the kind of a location. */
  readonly detail: string;
  readonly world: number;
  readonly layer: number;
  readonly x: number;
  readonly z: number;
  readonly level: number;
}

export interface FindDoc {
  readonly hits: readonly FindHit[];
}
