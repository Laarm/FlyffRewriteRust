//! The data side of `GET /admin/api/worlds` and `GET /admin/api/world/{id}/meta` (Plan/25 §3.3, §3.4, §3.6, §3.13,
//! §11.3; WP-ADMIN-08): pure functions over the loaded [`GameData`], the VFS and the client manifest, so that the
//! documents are testable without a server.
//!
//! * **Groups** (the world selector of §3.2): fields, dungeons, instances, PvP and events, housing, secret rooms, from
//!   the `World.inc` define name and the party-dungeon worlds ([`group_of`]).
//! * **Layer policy** (§3.13): `pick_live` for the worlds whose positive layers are made by doc 22 (the dungeons of
//!   `PartyDungeon.lua`: 121-125, 205, 206, 220 in the shipped data; the mini-room 209 and the guild houses 212-214),
//!   `fixed0` for every other world ([`layer_policy`]).
//! * **Geometry** is `lands`, `mpu`, `land_m = 128 * mpu`, the extent in metres, the indoor and fly flags and the
//!   default camera of the `.wld` (a key the data loader does not keep: it is read from the file here).
//! * **Tiles** say where the minimap is: the raw level-0 textures under `/res/<fp>/world/<folder>/` and the overview
//!   pyramid under `/gen/admin-map/<fp>/` (the `admin-map` kind builds it on the first request), the coverage rows and
//!   the level grids. Nothing here builds a tile.
//! * **Meta** lists what the map draws as overlays: the regions (towns, PK zones, attribute areas, titled places), the
//!   continents, the revival points, the portals with their targets, the spawn regions, the NPC placements of the
//!   `.dyo` and the `teleport16` rows of the world.

use flyff_assets::build::admin_map::{self, Coverage};
use flyff_assets::manifest::ClientManifest;
use flyff_assets::{AssetKind, AssetPrep};
use flyff_data::GameData;
use flyff_data::types::WorldId;
use flyff_data::world_list::WorldDef;
use flyff_data::worlds::{RegionDef, WorldFolderData, WorldRegistry};
use flyff_vfs::Vfs;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// What the documents are built from.
#[derive(Clone, Copy)]
pub struct WorldEnv<'a> {
    /// The resource index.
    pub vfs: &'a Vfs,
    /// The loaded game data.
    pub data: &'a GameData,
    /// The finalised client manifest (the `/res` fingerprints of the world folders).
    pub manifest: &'a ClientManifest,
    /// The derived-asset framework (the fingerprint of the `admin-map` partition of a folder).
    pub prep: &'a AssetPrep,
}

// ---- groups and the layer policy ---------------------------------------------------------------------------------

/// The groups of the world selector (25 §3.2), in selector order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Group {
    /// The open worlds: Madrigal, Azria, Coral Island and the unloaded ones.
    Fields,
    /// Dungeons that are not party instances.
    Dungeons,
    /// The party instance dungeons of `PartyDungeon.lua` (layer = dungeon id).
    Instances,
    /// Guild siege, 1 vs 1 zones, the arena, the quiz and event worlds.
    PvpEvents,
    /// Mini-rooms and guild houses.
    Housing,
    /// The secret rooms.
    SecretRooms,
}

impl Group {
    /// Every group in selector order.
    pub const ALL: [Group; 6] = [
        Group::Fields,
        Group::Dungeons,
        Group::Instances,
        Group::PvpEvents,
        Group::Housing,
        Group::SecretRooms,
    ];

    /// The id used in the JSON.
    pub fn id(self) -> &'static str {
        match self {
            Group::Fields => "fields",
            Group::Dungeons => "dungeons",
            Group::Instances => "instances",
            Group::PvpEvents => "pvp_events",
            Group::Housing => "housing",
            Group::SecretRooms => "secret_rooms",
        }
    }

    /// The label of the selector.
    pub fn label(self) -> &'static str {
        match self {
            Group::Fields => "Fields",
            Group::Dungeons => "Dungeons",
            Group::Instances => "Instances",
            Group::PvpEvents => "PvP & events",
            Group::Housing => "Housing",
            Group::SecretRooms => "Secret rooms",
        }
    }
}

/// Which layer a click on a location of the world should open (25 §3.13).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum LayerPolicy {
    /// Layer 0.
    Fixed0,
    /// The player picks one of the live layers (a popover from the `LayerList`); none live: the empty template layer 0.
    PickLive,
}

/// The worlds whose positive layers are made by doc 22: the dungeons of `PartyDungeon.lua`, the mini-room and the
/// guild houses.
pub fn pick_live_worlds(data: &GameData) -> BTreeSet<WorldId> {
    use flyff_common::defs::world::{
        WI_GUILDHOUSE_LARGE, WI_GUILDHOUSE_MIDDLE, WI_GUILDHOUSE_SMALL, WI_WORLD_MINIROOM,
    };
    let mut set: BTreeSet<WorldId> = [
        WI_WORLD_MINIROOM,
        WI_GUILDHOUSE_SMALL,
        WI_GUILDHOUSE_MIDDLE,
        WI_GUILDHOUSE_LARGE,
    ]
    .into_iter()
    .collect();
    if let Some(pd) = &data.lua.party_dungeon {
        set.extend(
            pd.dungeons
                .iter()
                .map(|d| d.world)
                .filter(|w| *w != flyff_data::types::NULL_ID),
        );
    }
    set
}

/// The layer policy of `world`.
pub fn layer_policy(world: WorldId, pick_live: &BTreeSet<WorldId>) -> LayerPolicy {
    if pick_live.contains(&world) {
        LayerPolicy::PickLive
    } else {
        LayerPolicy::Fixed0
    }
}

/// The group of a world: the housing and secret-room defines first, the party-dungeon worlds are instances, the
/// guild-war, arena, quiz and event worlds are PvP and events, the other dungeons (`WI_DUNGEON_*`, the abyss floors) are
/// dungeons, the rest fields.
pub fn group_of(def: &WorldDef, instances: &BTreeSet<WorldId>) -> Group {
    let n = def.define_name.as_str();
    if n.starts_with("WI_GUILDHOUSE") || n == "WI_WORLD_MINIROOM" {
        Group::Housing
    } else if n.starts_with("WI_DUNGEON_SECRET") {
        Group::SecretRooms
    } else if instances.contains(&def.id) || n.starts_with("WI_INSTANCE_") {
        Group::Instances
    } else if ["GUILDWAR", "ARENA", "QUIZ", "EVENT"]
        .iter()
        .any(|k| n.starts_with("WI_WORLD_") && n.contains(k))
    {
        Group::PvpEvents
    } else if n.starts_with("WI_DUNGEON_") || n.starts_with("WI_WORLD_HEAVEN") {
        Group::Dungeons
    } else {
        Group::Fields
    }
}

/// The party-dungeon worlds (instances), without the housing worlds of [`pick_live_worlds`].
fn instance_worlds(data: &GameData) -> BTreeSet<WorldId> {
    data.lua
        .party_dungeon
        .as_ref()
        .map(|pd| {
            pd.dungeons
                .iter()
                .map(|d| d.world)
                .filter(|w| *w != flyff_data::types::NULL_ID)
                .collect()
        })
        .unwrap_or_default()
}

// ---- GET /admin/api/worlds -----------------------------------------------------------------------------------------

/// One entry of [`WorldsDoc::groups`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct GroupRow {
    /// The id (`fields`, ...).
    pub id: Group,
    /// The label.
    pub label: String,
}

/// The minimap of a world in the world list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TilesSummary {
    /// Landscapes with a tile.
    pub covered: u32,
    /// Landscapes in the world.
    pub lands: u32,
    /// The highest overview level.
    pub max_level: u32,
}

/// One world of the list.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct WorldRow {
    /// The world id.
    pub id: u32,
    /// The display name (`World.inc` `SetTitle`; the folder when it has none).
    pub name: String,
    /// The folder as written in `World.inc`.
    pub folder: String,
    /// The `WI_*` define.
    pub define: String,
    /// The group of the selector.
    pub group: Group,
    /// Landscapes in x and z.
    pub lands: [u32; 2],
    /// Metres per heightmap unit.
    pub mpu: u32,
    /// The side of a landscape in metres.
    pub land_m: u32,
    /// The world's size in metres in x and z.
    pub extent_m: [u32; 2],
    /// Indoor (`.wld` `indoor`).
    pub indoor: bool,
    /// Flying is allowed (`.wld` `fly`).
    pub fly: bool,
    /// The raw `.wld` `pkmode` word.
    pub pk_mode: u32,
    /// The minimap, `null` without tiles.
    pub tiles: Option<TilesSummary>,
    /// The layer policy of the world.
    pub layer_policy: LayerPolicy,
    /// Players in the world, in all layers.
    pub online: u32,
}

/// `GET /admin/api/worlds`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct WorldsDoc {
    /// Every world the server loads, ascending by id.
    pub worlds: Vec<WorldRow>,
    /// The groups in selector order.
    pub groups: Vec<GroupRow>,
}

fn name_of(def: &WorldDef) -> String {
    let t = def.title.trim();
    if t.is_empty() {
        def.folder.clone()
    } else {
        t.to_owned()
    }
}

/// The world list. `online` maps a world id to its players.
pub fn worlds_doc(env: &WorldEnv<'_>, online: &BTreeMap<u32, u32>) -> WorldsDoc {
    let instances = instance_worlds(env.data);
    let pick_live = pick_live_worlds(env.data);
    let mut worlds = Vec::new();
    for id in env.data.worlds.loadable_ids() {
        let (Some(def), Some(folder)) =
            (env.data.worlds.world(id), env.data.worlds.folder_data(id))
        else {
            continue;
        };
        let s = &folder.settings;
        let mpu = if s.mpu == 0 { 4 } else { s.mpu };
        let land_m = 128 * mpu;
        let tiles = admin_map::coverage(env.vfs, env.data, id).map(|c: Coverage| TilesSummary {
            covered: u32::try_from(c.tiles.len()).unwrap_or(u32::MAX),
            lands: c.lands.0 * c.lands.1,
            max_level: c.max_level(),
        });
        worlds.push(WorldRow {
            id,
            name: name_of(def),
            folder: def.folder.clone(),
            define: def.define_name.clone(),
            group: group_of(def, &instances),
            lands: [s.size.0, s.size.1],
            mpu,
            land_m,
            extent_m: [s.size.0 * land_m, s.size.1 * land_m],
            indoor: s.indoor,
            fly: s.fly,
            pk_mode: s.pk_mode,
            tiles,
            layer_policy: layer_policy(id, &pick_live),
            online: online.get(&id).copied().unwrap_or(0),
        });
    }
    worlds.sort_by_key(|w| (w.group, w.id));
    WorldsDoc {
        worlds,
        groups: Group::ALL
            .iter()
            .map(|g| GroupRow {
                id: *g,
                label: g.label().to_owned(),
            })
            .collect(),
    }
}

// ---- GET /admin/api/world/{id}/meta -------------------------------------------------------------------------------

/// The size of the world.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Geometry {
    /// Landscapes in x and z.
    pub lands: [u32; 2],
    /// Metres per heightmap unit.
    pub mpu: u32,
    /// The side of a landscape in metres (`128 * mpu`).
    pub land_m: u32,
    /// The size in metres in x and z.
    pub extent_m: [u32; 2],
    /// Indoor.
    pub indoor: bool,
    /// Flying is allowed.
    pub fly: bool,
    /// The raw `.wld` `pkmode` word.
    pub pk_mode: u32,
}

/// The default camera of the `.wld` (`camera x y z yaw pitch`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Camera {
    /// Position x.
    pub x: f32,
    /// Position y (height).
    pub y: f32,
    /// Position z.
    pub z: f32,
    /// Yaw in radians as the file has it.
    pub yaw: f32,
    /// Pitch as the file has it.
    pub pitch: f32,
}

/// Where the minimap of the world is.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TilesMeta {
    /// The file-name prefix of the level-0 tiles (`wdmadrigal`): `<prefix>XX-ZZ.dds`, two digits at least.
    pub prefix: String,
    /// The URL prefix of the level-0 tiles (`/res/<fp>/world/<folder>/`), `null` when the folder is not client
    /// visible.
    #[serde(rename = "res")]
    #[cfg_attr(feature = "ts", ts(rename = "res"))]
    pub res_url: Option<String>,
    /// The URL prefix of the overview tiles and the coverage document (`/gen/admin-map/<fp>/`): `<id>/<k>/<tx>_<tz>.png`
    /// and `world-<id>.tiles.json`.
    #[serde(rename = "gen")]
    #[cfg_attr(feature = "ts", ts(rename = "gen"))]
    pub gen_url: Option<String>,
    /// Pixels per tile side.
    pub tile_px: u32,
    /// The highest overview level.
    pub max_level: u32,
    /// Landscapes with a tile.
    pub covered_count: u32,
    /// One string per landscape row (row 0 = z 0 first), `1` for a landscape with a tile.
    pub covered: Vec<String>,
    /// The overview levels: columns and rows of the tile grid and how many tiles exist.
    pub levels: Vec<LevelMeta>,
}

/// One overview level.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LevelMeta {
    /// The level (1..).
    pub k: u32,
    /// Tile columns.
    pub cols: u32,
    /// Tile rows.
    pub rows: u32,
    /// Tiles that exist.
    pub tiles: u32,
}

/// A region of the `.rgn` (named areas, PK zones and other attribute areas, revival and portal areas).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RegionDoc {
    /// `dwIndex`: 10 trigger, 11 attribute, 12 begin, 13 revival, 14 structure, 15 place.
    pub index: u32,
    /// The kind as text.
    pub kind: String,
    /// `m_szKey` (revival and place keys).
    pub key: String,
    /// The title.
    pub title: Option<String>,
    /// The description.
    pub desc: Option<String>,
    /// `[x0, z0, x1, z1]` in metres.
    pub rect: [i32; 4],
    /// The `RA_*` attribute flags.
    pub attribute: u32,
    /// The portal target world (`null` = not a portal).
    pub to_world: Option<u32>,
}

/// A continent polygon (`.wld.cnt`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ContinentDoc {
    /// `C_id`.
    pub id: u32,
    /// `C_name`.
    pub name: String,
    /// A town.
    pub town: bool,
    /// The server uses it as a gameplay continent.
    pub gameplay: bool,
    /// The polygon `[x, z]`, not closed.
    pub polygon: Vec<[f32; 2]>,
}

/// A revival point.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RevivalDoc {
    /// The revival key.
    pub key: String,
    /// The title when it has one.
    pub title: Option<String>,
    /// Where a revived player stands `[x, y, z]`.
    pub pos: [f32; 3],
    /// The area of the point `[x0, z0, x1, z1]`.
    pub rect: [i32; 4],
}

/// A portal: a trigger region with a teleport target.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PortalDoc {
    /// The trigger area `[x0, z0, x1, z1]`.
    pub rect: [i32; 4],
    /// The title of the area.
    pub title: Option<String>,
    /// The destination world.
    pub to_world: u32,
    /// The name of the destination world.
    pub to_name: String,
    /// The destination position `[x, y, z]`.
    pub to_pos: [f32; 3],
}

/// A spawn region (`respawn` rows of the `.rgn`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SpawnDoc {
    /// The row in the file (`gm_index` of the respawner counts the accepted rows).
    pub row: u32,
    /// `monster`, `ctrl` or `item`.
    pub kind: String,
    /// The prop id.
    pub prop: u32,
    /// The mover's or ctrl's name.
    pub name: String,
    /// The monster level.
    pub level: Option<u32>,
    /// The area `[x0, z0, x1, z1]`.
    pub rect: [i32; 4],
    /// The target count.
    pub count: i32,
    /// The respawn time in seconds as written.
    pub time_s: u32,
}

/// An NPC placed by the `.dyo`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct NpcDoc {
    /// The `character.inc` key.
    pub key: String,
    /// The display name.
    pub name: String,
    /// The mover prop id.
    pub prop: u32,
    /// Position x.
    pub x: f32,
    /// Position z.
    pub z: f32,
    /// Yaw in degrees.
    pub angle: f32,
}

/// A `teleport16` destination in this world.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TeleportDoc {
    /// The name.
    pub name: String,
    /// `[x, y, z]`.
    pub pos: [f32; 3],
    /// The thumbnail file (`tp_flaris.png`).
    pub image: String,
    /// Minimum level.
    pub min_level: u32,
    /// Maximum level (129 = none).
    pub max_level: u32,
}

/// `GET /admin/api/world/{id}/meta`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct MetaDoc {
    /// The world id.
    pub id: u32,
    /// The display name.
    pub name: String,
    /// The folder as written in `World.inc`.
    pub folder: String,
    /// The `WI_*` define.
    pub define: String,
    /// The group.
    pub group: Group,
    /// The layer policy.
    pub layer_policy: LayerPolicy,
    /// The size.
    pub geometry: Geometry,
    /// The default camera.
    pub camera: Option<Camera>,
    /// The minimap.
    pub tiles: Option<TilesMeta>,
    /// Every region of the `.rgn` in file order (triggers, attribute areas, the begin point, revival areas, structures):
    /// those with a title are the named areas, the others carry attribute flags (PK zones, ...).
    pub regions: Vec<RegionDoc>,
    /// Continent polygons.
    pub continents: Vec<ContinentDoc>,
    /// Revival points.
    pub revival: Vec<RevivalDoc>,
    /// Portals.
    pub portals: Vec<PortalDoc>,
    /// Spawn regions.
    pub spawns: Vec<SpawnDoc>,
    /// NPC placements.
    pub npcs: Vec<NpcDoc>,
    /// `teleport16` rows of this world.
    pub teleports: Vec<TeleportDoc>,
}

fn region_kind(index: u32) -> &'static str {
    use flyff_common::defs::obj::{
        RI_ATTRIBUTE, RI_BEGIN, RI_PLACE, RI_REVIVAL, RI_STRUCTURE, RI_TRIGGER,
    };
    match index {
        RI_TRIGGER => "trigger",
        RI_ATTRIBUTE => "attribute",
        RI_BEGIN => "begin",
        RI_REVIVAL => "revival",
        RI_STRUCTURE => "structure",
        RI_PLACE => "place",
        _ => "other",
    }
}

fn region_doc(r: &RegionDef) -> RegionDoc {
    RegionDoc {
        index: r.index,
        kind: region_kind(r.index).to_owned(),
        key: r.key.clone(),
        title: r.title.clone().filter(|t| !t.trim().is_empty()),
        desc: r.desc.clone().filter(|t| !t.trim().is_empty()),
        rect: r.rect,
        attribute: r.attribute,
        to_world: (r.tele_world != 0 && r.tele_world != flyff_data::types::NULL_ID)
            .then_some(r.tele_world),
    }
}

/// The `camera x y z yaw pitch` line of a `.wld` text (the loader ignores the key, so it is read here).
pub fn parse_camera(text: &str) -> Option<Camera> {
    for line in text.lines() {
        let mut it = line.split_whitespace();
        if !it.next().is_some_and(|k| k.eq_ignore_ascii_case("camera")) {
            continue;
        }
        let v: Vec<f32> = it.filter_map(|t| t.parse().ok()).collect();
        if let [x, y, z, yaw, pitch, ..] = v[..] {
            return Some(Camera {
                x,
                y,
                z,
                yaw,
                pitch,
            });
        }
        return None;
    }
    None
}

fn camera_of(vfs: &Vfs, folder_key: &str) -> Option<Camera> {
    let bytes = vfs
        .read(&format!("world/{folder_key}/{folder_key}.wld"))
        .ok()?;
    parse_camera(&String::from_utf8_lossy(&bytes))
}

fn tiles_meta(env: &WorldEnv<'_>, cov: &Coverage) -> TilesMeta {
    let res_dir = format!("world/{}", cov.folder);
    let res = env
        .manifest
        .res
        .groups
        .get(&res_dir)
        .map(|fp| format!("{}/{fp}/{res_dir}/", env.manifest.res.prefix));
    let gen_fp = env.prep.partition_fp(AssetKind::AdminMap, &cov.folder);
    let max = cov.max_level();
    TilesMeta {
        prefix: cov.prefix.clone(),
        res_url: res,
        gen_url: gen_fp.map(|fp| format!("{}/admin-map/{fp}/", env.manifest.gen_urls.prefix)),
        tile_px: admin_map::TILE_PX,
        max_level: max,
        covered_count: u32::try_from(cov.tiles.len()).unwrap_or(u32::MAX),
        covered: cov.rows(),
        levels: (1..=max)
            .map(|k| {
                let (cols, rows) = cov.grid(k);
                LevelMeta {
                    k,
                    cols,
                    rows,
                    tiles: u32::try_from(cov.level_tiles(k).len()).unwrap_or(u32::MAX),
                }
            })
            .collect(),
    }
}

fn spawns_of(env: &WorldEnv<'_>, folder: &WorldFolderData) -> Vec<SpawnDoc> {
    use flyff_common::defs::define::{OT_CTRL, OT_ITEM, OT_MOVER};
    use flyff_game::world::RespawnConfig;
    use flyff_game::world::respawn::effective_mover_count;
    // the count the game spawns: a giant line has one monster (owner rule, bugs #005 / #016), a normal line the file
    // count after `__INCREASED_SPAWN` and the cap of 80. `WorldEnv` carries no game config, so this is the shipped
    // default of `gameplay.increased_spawn` (1.0): giants show x1 at any value, the others as the default game.
    let increased_spawn = RespawnConfig::default().increased_spawn;
    folder
        .respawns
        .iter()
        .enumerate()
        .map(|(row, r)| {
            let mut count = r.max_count;
            let (kind, name, level) = match r.obj_type {
                OT_MOVER => {
                    let p = env.data.mover(r.index);
                    if let Some(p) = p {
                        count = effective_mover_count(r.max_count, p, increased_spawn);
                    }
                    (
                        "monster",
                        p.map(|p| p.name.clone()).unwrap_or_default(),
                        p.map(|p| p.level),
                    )
                }
                OT_CTRL => (
                    "ctrl",
                    env.data
                        .ctrls
                        .get(r.index)
                        .map(|p| p.name.clone())
                        .unwrap_or_default(),
                    None,
                ),
                OT_ITEM => (
                    "item",
                    env.data
                        .item(r.index)
                        .map(|p| p.name.clone())
                        .unwrap_or_default(),
                    None,
                ),
                _ => ("monster", String::new(), None),
            };
            SpawnDoc {
                row: u32::try_from(row).unwrap_or(u32::MAX),
                kind: kind.to_owned(),
                prop: r.index,
                name,
                level,
                rect: r.rect,
                count,
                time_s: r.time_s,
            }
        })
        .collect()
}

fn npcs_of(env: &WorldEnv<'_>, folder_key: &str) -> Vec<NpcDoc> {
    use flyff_common::defs::define::OT_MOVER;
    let Ok(bytes) = env
        .vfs
        .read(&format!("world/{folder_key}/{folder_key}.dyo"))
    else {
        return Vec::new();
    };
    let Ok(parsed) = flyff_formats::dyo::parse(&bytes) else {
        return Vec::new();
    };
    parsed
        .value
        .records
        .iter()
        .filter(|r| r.obj_type == OT_MOVER)
        .filter_map(|r| {
            let m = r.mover()?;
            let key = m.character_key.to_string_lossy_ascii();
            if key.is_empty() {
                return None;
            }
            let [x, _, z] = r.base.absolute_position(None);
            let name = env
                .data
                .npcs
                .get(&key)
                .map(|n| n.name.trim().to_owned())
                .filter(|n| !n.is_empty())
                .or_else(|| env.data.mover(r.base.index).map(|p| p.name.clone()))
                .unwrap_or_else(|| key.clone());
            Some(NpcDoc {
                key,
                name,
                prop: r.base.index,
                x,
                z,
                angle: r.base.angle,
            })
        })
        .collect()
}

/// The meta document of world `id`; `None` when the world is not loaded.
pub fn meta_doc(env: &WorldEnv<'_>, id: WorldId) -> Option<MetaDoc> {
    let def = env.data.worlds.world(id)?;
    let folder = env.data.worlds.folder_data(id)?;
    if !def.loadable {
        return None;
    }
    let key = WorldRegistry::folder_key(&def.folder);
    let s = &folder.settings;
    let mpu = if s.mpu == 0 { 4 } else { s.mpu };
    let land_m = 128 * mpu;
    let cov = admin_map::coverage(env.vfs, env.data, id);
    let regions: Vec<RegionDoc> = folder.regions.iter().map(region_doc).collect();
    let revival: Vec<RevivalDoc> = env
        .data
        .worlds
        .revival
        .points
        .iter()
        .filter(|(w, _)| *w == id)
        .map(|(_, r)| RevivalDoc {
            key: r.key.clone(),
            title: r.title.clone().filter(|t| !t.trim().is_empty()),
            pos: r.pos,
            rect: r.rect,
        })
        .collect();
    let portals: Vec<PortalDoc> = folder
        .regions
        .iter()
        .filter(|r| {
            r.tele_world != 0
                && r.tele_world != flyff_data::types::NULL_ID
                && r.index == flyff_common::defs::obj::RI_TRIGGER
        })
        .map(|r| PortalDoc {
            rect: r.rect,
            title: r.title.clone().filter(|t| !t.trim().is_empty()),
            to_world: r.tele_world,
            to_name: env
                .data
                .worlds
                .world(r.tele_world)
                .map(name_of)
                .unwrap_or_default(),
            to_pos: r.tele_pos,
        })
        .collect();
    let continents: Vec<ContinentDoc> = folder
        .continents
        .iter()
        .map(|c| ContinentDoc {
            id: c.id,
            name: c.name.clone(),
            town: c.town,
            gameplay: c.use_real_data,
            polygon: c.vertices.iter().map(|v| [v[0], v[2]]).collect(),
        })
        .collect();
    let teleports: Vec<TeleportDoc> = env
        .data
        .teleports16
        .iter()
        .filter(|t| t.world == id)
        .map(|t| TeleportDoc {
            name: t.name.clone(),
            pos: t.pos,
            image: t.image.clone(),
            min_level: t.min_level,
            max_level: t.effective_max_level(),
        })
        .collect();
    Some(MetaDoc {
        id,
        name: name_of(def),
        folder: def.folder.clone(),
        define: def.define_name.clone(),
        group: group_of(def, &instance_worlds(env.data)),
        layer_policy: layer_policy(id, &pick_live_worlds(env.data)),
        geometry: Geometry {
            lands: [s.size.0, s.size.1],
            mpu,
            land_m,
            extent_m: [s.size.0 * land_m, s.size.1 * land_m],
            indoor: s.indoor,
            fly: s.fly,
            pk_mode: s.pk_mode,
        },
        camera: camera_of(env.vfs, &key),
        tiles: cov.as_ref().map(|c| tiles_meta(env, c)),
        regions,
        continents,
        revival,
        portals,
        spawns: spawns_of(env, folder),
        npcs: npcs_of(env, &key),
        teleports,
    })
}

/// Declares every type of this module for the generated TypeScript of the DTO catalogue (feature `ts`).
#[cfg(feature = "ts")]
pub fn declare_ts(out: &mut String, cfg: &ts_rs::Config) {
    use ts_rs::TS;
    fn push<T: TS + 'static>(out: &mut String, cfg: &ts_rs::Config) {
        out.push_str("export ");
        out.push_str(&T::decl(cfg));
        out.push('\n');
    }
    push::<Group>(out, cfg);
    push::<LayerPolicy>(out, cfg);
    push::<GroupRow>(out, cfg);
    push::<TilesSummary>(out, cfg);
    push::<WorldRow>(out, cfg);
    push::<WorldsDoc>(out, cfg);
    push::<Geometry>(out, cfg);
    push::<Camera>(out, cfg);
    push::<LevelMeta>(out, cfg);
    push::<TilesMeta>(out, cfg);
    push::<RegionDoc>(out, cfg);
    push::<ContinentDoc>(out, cfg);
    push::<RevivalDoc>(out, cfg);
    push::<PortalDoc>(out, cfg);
    push::<SpawnDoc>(out, cfg);
    push::<NpcDoc>(out, cfg);
    push::<TeleportDoc>(out, cfg);
    push::<MetaDoc>(out, cfg);
}
