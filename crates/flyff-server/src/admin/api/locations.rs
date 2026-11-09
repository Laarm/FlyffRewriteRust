//! The quick locations of the live map and the shared bookmarks (Plan/25 §3.13, §7.4, §11.3, §11.6; WP-ADMIN-14).
//!
//! `GET /admin/api/locations` (`view.map`) answers the merged list of the panel. **The client holds no coordinates**:
//! every position comes from a data file or from a cited constant.
//!
//! | # | source | used for |
//! |---|---|---|
//! | 1 | the `teleport16` rows | the towns, fields and dungeons, with their thumbnails and level ranges |
//! | 2 | the revival points | the boss rooms |
//! | 3 | the dungeon entrances (trigger regions with a teleport target) | the entrances and the arrival points of the instances |
//! | 4 | the boss positions of `PartyDungeon.lua` and `AutoRaid.lua` | the bosses |
//! | 5 | the `.wld` default camera of every world | the group "All worlds" and the worlds without another source |
//! | 6 | `assets/admin/locations.default.toml` | the rows that no data file holds; each cites its constant (`src:<file>:<line>`) |
//! | 7 | the shared bookmarks (`admin_bookmarks`) | staff's own places |
//! | 8 | the per-browser favourites and recent jumps | the browser's `localStorage`, not this route |
//!
//! The default file lists the buttons of 25 §3.13 in order, each naming WHERE its position comes from (`from`); the
//! server resolves them against the loaded game data ([`Resolver`]). A row that does not resolve (a data set without
//! that file) is left out and named in `unresolved`.
//!
//! `POST /admin/api/locations`, `PUT /admin/api/locations/{id}` and `DELETE /admin/api/locations/{id}` (`act.world`)
//! are the shared bookmarks; `{id}` is the number in `bookmark_id`. A bookmark with the same group and name as a
//! default row replaces it in the list (that is how staff edit a default). Every change is audited (`admin.bookmark`,
//! `requested` then `done`).
//!
//! **Layer policy** (§3.13): `pick_live` for the worlds that the instance definitions, guild houses and the mini-room
//! use ([`meta::layer_policy`]), `fixed0` for the rest; a default row may override it.
//!
//! The DTOs derive `ts_rs::TS` with the feature `ts`; `declare_ts` lists them for `gen/dto.ts`.

use super::world::meta::{self, WorldEnv};
use super::{ApiState, Routes};
use crate::admin::auth::Who;
use crate::admin::error::{ApiError, ApiResult};
use crate::admin::guard::{AuditTrail, MutationJson};
use crate::admin::identity::Identity;
use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, State};
use flyff_data::GameData;
use flyff_data::types::{NULL_ID, WorldId};
use flyff_data::worlds::WorldRegistry;
use flyff_db::admin::{AdminBookmarkRecord, AdminReadQuery, LayerPolicy as DbPolicy};
use flyff_db::{AtomicBatch, AtomicKind, AtomicOutcome, DbErrorKind, DbOp};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, HashMap};

/// The default rows, embedded (`crates/flyff-server/assets/admin/locations.default.toml`).
pub const DEFAULTS_TOML: &str = include_str!("../../../assets/admin/locations.default.toml");

/// The label of the group with a button for every world (source 5: the `.wld` default camera).
pub const ALL_WORLDS: &str = "All worlds";
/// The group a bookmark without a group lands in.
pub const BOOKMARK_GROUP: &str = "Bookmarks";
/// The most shared bookmarks (a guard against a runaway client; the panel is a compact list).
pub const MAX_BOOKMARKS: usize = 500;
/// The longest name of a bookmark, in characters.
pub const NAME_MAX_CHARS: usize = 48;
/// The longest group label of a bookmark, in characters.
pub const GROUP_MAX_CHARS: usize = 32;
/// The smallest and largest zoom a bookmark may carry (world units per pixel).
pub const ZOOM_RANGE: (f32, f32) = (0.05, 64.0);

// ---- DTOs --------------------------------------------------------------------------------------------------------

/// Which layer a click on a location opens (25 §3.13): the same two values as the policy of a world.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum LocationPolicy {
    /// Layer 0.
    #[default]
    Fixed0,
    /// The staff member picks among the live layers.
    PickLive,
}

impl From<meta::LayerPolicy> for LocationPolicy {
    fn from(p: meta::LayerPolicy) -> Self {
        match p {
            meta::LayerPolicy::Fixed0 => LocationPolicy::Fixed0,
            meta::LayerPolicy::PickLive => LocationPolicy::PickLive,
        }
    }
}

impl From<LocationPolicy> for DbPolicy {
    fn from(p: LocationPolicy) -> Self {
        match p {
            LocationPolicy::Fixed0 => DbPolicy::Fixed0,
            LocationPolicy::PickLive => DbPolicy::PickLive,
        }
    }
}

impl From<DbPolicy> for LocationPolicy {
    fn from(p: DbPolicy) -> Self {
        match p {
            DbPolicy::Fixed0 => LocationPolicy::Fixed0,
            DbPolicy::PickLive => LocationPolicy::PickLive,
        }
    }
}

/// Where a row of the list comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum LocationKind {
    /// A default button of `locations.default.toml`, resolved from the data.
    Default,
    /// The `.wld` camera of a world (the group "All worlds").
    World,
    /// A shared bookmark.
    Bookmark,
}

/// One button of the quick-locations panel.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LocationRow {
    /// Stable id: `d:<group>:<name>` (default), `w:<world>`, `b:<bookmark id>`. Favourites are kept by id.
    pub id: String,
    /// The group label.
    pub group: String,
    /// The button text.
    pub name: String,
    /// The world.
    pub world: u32,
    /// Which layer a click opens.
    pub layer_policy: LocationPolicy,
    /// Position x in metres (whole metres for the default rows).
    pub x: f32,
    /// Position z in metres.
    pub z: f32,
    /// The scale a click flies to, world units per pixel (2 = 1:2); `null` fits the world or keeps the scale.
    pub zoom: Option<f32>,
    /// Where the position comes from: `teleport16`, `region title`, `trigger region`, `region target`, `.wld camera`,
    /// `PartyDungeon.lua`, `AutoRaid.lua`, `revival <key>`, `.dyo NPC <key>`, `src:<file>:<line>` or `bookmark`.
    pub source: String,
    /// The thumbnail file of a `teleport16` row (`tp_flaris.png`, under `Theme/`).
    pub thumb: Option<String>,
    /// The level range of a `teleport16` row, when it has one.
    pub min_level: Option<u32>,
    /// See `min_level`.
    pub max_level: Option<u32>,
    /// Default, world camera or bookmark.
    pub kind: LocationKind,
    /// `kind == bookmark`: the staff may edit it (the `act.world` capability decides who).
    pub editable: bool,
    /// The number the bookmark routes take.
    pub bookmark_id: Option<i64>,
    /// A bookmark's own zoom (`zoom` falls back to the preset of its group).
    pub own_zoom: Option<f32>,
    /// A bookmark's order inside its group.
    pub sort: i32,
    /// Who made the bookmark (`staff:<account>`, `owner:local`).
    pub created_by: Option<String>,
    /// Unix seconds of the last change of a bookmark.
    pub updated_at: Option<i64>,
    /// The id of the default row that this bookmark replaces.
    pub overrides: Option<String>,
}

/// One group of the panel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct GroupDoc {
    /// A slug of the label.
    pub id: String,
    /// The label.
    pub label: String,
    /// Rows in the group.
    pub count: u32,
}

/// `GET /admin/api/locations`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LocationsDoc {
    /// The groups in panel order (the default groups, the groups of bookmarks, "All worlds").
    pub groups: Vec<GroupDoc>,
    /// The rows: the default rows in file order, then the bookmarks by `sort` and name, per group.
    pub locations: Vec<LocationRow>,
    /// The names of default rows that did not resolve against this data set.
    pub unresolved: Vec<String>,
}

/// The body of `POST /admin/api/locations` and `PUT /admin/api/locations/{id}` (inside the `Mutation` envelope).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct BookmarkBody {
    /// The group (default `Bookmarks`).
    #[serde(default)]
    pub group: String,
    /// The button text.
    pub name: String,
    /// The world (a world the server loads).
    pub world: u32,
    /// Which layer a click opens; left out = by the world.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub layer_policy: Option<LocationPolicy>,
    /// Position x in metres.
    pub x: f32,
    /// Position z in metres.
    pub z: f32,
    /// The scale in world units per pixel; left out = the preset of the group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub zoom: Option<f32>,
    /// The order inside the group (default 0).
    #[serde(default)]
    pub sort: i32,
}

// ---- the default file ----------------------------------------------------------------------------------------------

/// Where the position of a default row comes from (the `from` of a row of `locations.default.toml`).
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Source {
    /// The `teleport16.inc` row of that name (a leading `[Tag] ` is ignored).
    Teleport16 {
        /// The row's name.
        name: String,
    },
    /// The `vPos` of the titled region of that world.
    RegionPos {
        /// The world.
        world: WorldId,
        /// The title.
        title: String,
    },
    /// The `vPos` of the region of `world` that teleports to world `to` (a dungeon entrance).
    PortalPos {
        /// The world that has the region.
        world: WorldId,
        /// The destination world.
        to: WorldId,
    },
    /// The destination of that region (where a player arrives).
    PortalTarget {
        /// The world that has the region.
        world: WorldId,
        /// The destination world.
        to: WorldId,
    },
    /// The `camera` line of the world's `.wld`.
    WldCamera {
        /// The world.
        world: WorldId,
    },
    /// The `SetMonster` position of a monster in `PartyDungeon.lua`.
    PartyDungeon {
        /// The dungeon's world.
        world: WorldId,
        /// The `MI_*` name.
        monster: String,
    },
    /// The `SetPosition` of `AddRaid(name)` in `AutoRaid.lua`.
    AutoRaid {
        /// The raid's name.
        name: String,
    },
    /// The revival point of that key.
    Revival {
        /// The world.
        world: WorldId,
        /// The revival key.
        key: String,
    },
    /// The placement of an NPC of the world's `.dyo`.
    DyoNpc {
        /// The world.
        world: WorldId,
        /// The `character.inc` key.
        key: String,
    },
    /// A constant of the original code that no data file holds; `source` cites it.
    Const {
        /// The world.
        world: WorldId,
        /// Position x.
        x: f32,
        /// Position z.
        z: f32,
        /// `src:<file under Source/>:<line>`.
        source: String,
    },
}

/// One `[[location]]` row of the default file.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DefaultRow {
    /// The group label.
    pub group: String,
    /// The button text.
    pub name: String,
    /// Where the position comes from.
    pub from: Source,
    /// Overrides the group's zoom.
    #[serde(default)]
    pub zoom: Option<f32>,
    /// Overrides the layer policy of the world.
    #[serde(default)]
    pub policy: Option<LocationPolicy>,
}

/// The default file.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DefaultsFile {
    /// The zoom of each group.
    #[serde(default)]
    pub zoom: BTreeMap<String, f32>,
    /// The rows, in order.
    #[serde(default)]
    pub location: Vec<DefaultRow>,
}

/// Parses the embedded default file; an error names the problem (the unit test keeps the shipped file valid).
pub fn parse_defaults(text: &str) -> Result<DefaultsFile, String> {
    let file: DefaultsFile = toml::from_str(text).map_err(|e| e.to_string())?;
    let mut seen = std::collections::BTreeSet::new();
    for r in &file.location {
        if r.group.trim().is_empty() || r.name.trim().is_empty() {
            return Err("a row needs a group and a name".to_owned());
        }
        if !seen.insert((r.group.to_lowercase(), r.name.to_lowercase())) {
            return Err(format!("{}/{} is listed twice", r.group, r.name));
        }
        if let Source::Const { source, .. } = &r.from
            && !valid_citation(source)
        {
            return Err(format!(
                "{}/{}: a const row cites its original constant as src:<file>:<line>",
                r.group, r.name
            ));
        }
    }
    Ok(file)
}

/// `src:<file>:<line>` or `src:<file>:<line>-<line>`.
pub fn valid_citation(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("src:") else {
        return false;
    };
    let Some((file, lines)) = rest.rsplit_once(':') else {
        return false;
    };
    !file.trim().is_empty()
        && !lines.is_empty()
        && lines
            .split('-')
            .all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

// ---- resolving the data sources --------------------------------------------------------------------------------------

/// A resolved position.
#[derive(Clone, Debug, PartialEq)]
pub struct Spot {
    /// The world.
    pub world: WorldId,
    /// Position x (whole metres).
    pub x: f32,
    /// Position z (whole metres).
    pub z: f32,
    /// The source text of the row.
    pub source: String,
    /// The thumbnail of a `teleport16` row.
    pub thumb: Option<String>,
    /// The level range of a `teleport16` row (`min`, `max`; the "no limit" 129 is `None`).
    pub levels: Option<(u32, u32)>,
}

fn metres(v: f32) -> f32 {
    v.round()
}

fn spot(world: WorldId, x: f32, z: f32, source: impl Into<String>) -> Spot {
    Spot {
        world,
        x: metres(x),
        z: metres(z),
        source: source.into(),
        thumb: None,
        levels: None,
    }
}

/// A raid of `AutoRaid.lua`.
#[derive(Clone, Debug, PartialEq)]
pub struct RaidSpot {
    /// `AddRaid("name")`.
    pub name: String,
    /// The world of the first `SetPosition`.
    pub world: WorldId,
    /// x.
    pub x: f32,
    /// z.
    pub z: f32,
}

/// Reads `AutoRaid.lua`: the raids with their first position. Comments (`--`) are dropped first, so a raid or a
/// position that the file keeps as a comment does not exist; a `SetPosition` belongs to the last `AddRaid` before it, as
/// in Lua.
pub fn parse_auto_raid(text: &str) -> Vec<RaidSpot> {
    let mut out: Vec<RaidSpot> = Vec::new();
    let mut current: Option<String> = None;
    let mut positioned = false;
    for raw in text.lines() {
        let line = raw.split("--").next().unwrap_or("").trim();
        if let Some(arg) = call_args(line, "AddRaid") {
            current = arg
                .split(',')
                .next()
                .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_owned())
                .filter(|s| !s.is_empty());
            positioned = false;
        } else if let Some(arg) = call_args(line, "SetPosition")
            && let Some(name) = &current
            && !positioned
        {
            let n: Vec<f32> = arg
                .split(',')
                .map(|t| t.trim().parse::<f32>())
                .take_while(Result::is_ok)
                .flatten()
                .collect();
            if let [world, x, _y, z, ..] = n[..]
                && world >= 0.0
                && world.fract() == 0.0
            {
                out.push(RaidSpot {
                    name: name.clone(),
                    world: world as WorldId,
                    x,
                    z,
                });
                positioned = true;
            }
        }
    }
    out
}

/// The text between the parentheses of `name( ... )` when the line starts with that call.
fn call_args<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(name)?.trim_start().strip_prefix('(')?;
    Some(rest.rsplit_once(')').map_or(rest, |(inside, _)| inside))
}

/// `[Town] Flaris` -> `Flaris`.
fn untagged(name: &str) -> &str {
    let t = name.trim();
    match t.strip_prefix('[').and_then(|r| r.split_once(']')) {
        Some((_, rest)) => rest.trim(),
        None => t,
    }
}

/// A title with its line breaks as spaces, for comparing.
fn flat(title: &str) -> String {
    title
        .split(['\r', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The display names of the loaded worlds by id (`World.inc` titles).
pub fn world_names(data: &GameData) -> HashMap<WorldId, String> {
    data.worlds
        .loadable_ids()
        .map(|id| (id, world_name(data, id)))
        .collect()
}

fn world_name(data: &GameData, id: WorldId) -> String {
    data.worlds
        .world(id)
        .map(|d| {
            let t = d.title.trim();
            if t.is_empty() {
                d.folder.clone()
            } else {
                t.to_owned()
            }
        })
        .unwrap_or_else(|| format!("World {id}"))
}

/// The `.wld` camera of a world as `(x, z)`, else the middle of the world.
fn camera_of(env: &WorldEnv<'_>, id: WorldId) -> Option<(f32, f32)> {
    let def = env.data.worlds.world(id)?;
    let key = WorldRegistry::folder_key(&def.folder);
    let text = env
        .vfs
        .read(&format!("world/{key}/{key}.wld"))
        .ok()
        .map(|b| String::from_utf8_lossy(&b).into_owned());
    if let Some(c) = text.as_deref().and_then(meta::parse_camera) {
        return Some((c.x, c.z));
    }
    let s = &env.data.worlds.folder_data(id)?.settings;
    let mpu = if s.mpu == 0 { 4 } else { s.mpu };
    let land = 128 * mpu;
    Some((
        (s.size.0 * land) as f32 / 2.0,
        (s.size.1 * land) as f32 / 2.0,
    ))
}

/// What resolving needs besides the data: the files read once.
pub struct Resolver<'a> {
    env: WorldEnv<'a>,
    raids: Option<Vec<RaidSpot>>,
    npcs: HashMap<WorldId, Vec<meta::NpcDoc>>,
}

impl<'a> Resolver<'a> {
    /// A resolver over `env`.
    pub fn new(env: WorldEnv<'a>) -> Resolver<'a> {
        Resolver {
            env,
            raids: None,
            npcs: HashMap::new(),
        }
    }

    /// The position a `from` names, or why it cannot be found.
    pub fn resolve(&mut self, from: &Source) -> Result<Spot, String> {
        let data = self.env.data;
        match from {
            Source::Teleport16 { name } => {
                let want = untagged(name).to_lowercase();
                let t = data
                    .teleports16
                    .iter()
                    .find(|t| untagged(&t.name).to_lowercase() == want)
                    .ok_or_else(|| format!("no teleport16 row named {name}"))?;
                let max = t.effective_max_level();
                let mut s = spot(t.world, t.pos[0], t.pos[2], "teleport16");
                if !t.image.is_empty() {
                    s.thumb = Some(t.image.clone());
                }
                if t.min_level > 0 || max < flyff_data::teleport16::MAX_LEGEND_LEVEL {
                    s.levels = Some((t.min_level, max));
                }
                Ok(s)
            }
            Source::RegionPos { world, title } => {
                let folder = data
                    .worlds
                    .folder_data(*world)
                    .ok_or_else(|| format!("world {world} is not loaded"))?;
                let want = flat(title).to_lowercase();
                let r = folder
                    .regions
                    .iter()
                    .find(|r| {
                        r.title
                            .as_deref()
                            .is_some_and(|t| flat(t).to_lowercase() == want)
                    })
                    .ok_or_else(|| format!("world {world} has no region titled {title}"))?;
                Ok(spot(*world, r.pos[0], r.pos[2], "region title"))
            }
            Source::PortalPos { world, to } | Source::PortalTarget { world, to } => {
                let folder = data
                    .worlds
                    .folder_data(*world)
                    .ok_or_else(|| format!("world {world} is not loaded"))?;
                let r = folder
                    .regions
                    .iter()
                    .find(|r| r.tele_world == *to && r.tele_world != 0 && r.tele_world != NULL_ID)
                    .ok_or_else(|| format!("world {world} has no region that teleports to {to}"))?;
                Ok(match from {
                    Source::PortalPos { .. } => spot(*world, r.pos[0], r.pos[2], "trigger region"),
                    _ => spot(*to, r.tele_pos[0], r.tele_pos[2], "region target"),
                })
            }
            Source::WldCamera { world } => {
                if data.worlds.world(*world).is_none() {
                    return Err(format!("world {world} is not loaded"));
                }
                let (x, z) = camera_of(&self.env, *world)
                    .ok_or_else(|| format!("world {world} has no camera"))?;
                Ok(spot(*world, x, z, ".wld camera"))
            }
            Source::PartyDungeon { world, monster } => {
                let pd = data
                    .lua
                    .party_dungeon
                    .as_ref()
                    .ok_or("PartyDungeon.lua is not loaded")?;
                let m = pd
                    .dungeons
                    .iter()
                    .filter(|d| d.world == *world)
                    .flat_map(|d| d.monsters.iter())
                    .find(|m| m.mover_name.eq_ignore_ascii_case(monster))
                    .ok_or_else(|| format!("PartyDungeon.lua has no {monster} in world {world}"))?;
                Ok(spot(*world, m.pos[0], m.pos[2], "PartyDungeon.lua"))
            }
            Source::AutoRaid { name } => {
                if self.raids.is_none() {
                    self.raids = Some(
                        self.env
                            .vfs
                            .read("AutoRaid.lua")
                            .map(|b| parse_auto_raid(&String::from_utf8_lossy(&b)))
                            .unwrap_or_default(),
                    );
                }
                let r = self
                    .raids
                    .iter()
                    .flatten()
                    .find(|r| r.name.eq_ignore_ascii_case(name))
                    .ok_or_else(|| format!("AutoRaid.lua has no raid {name}"))?;
                Ok(spot(r.world, r.x, r.z, "AutoRaid.lua"))
            }
            Source::Revival { world, key } => {
                let (_, r) = data
                    .worlds
                    .revival
                    .points
                    .iter()
                    .find(|(w, r)| *w == *world && r.key.eq_ignore_ascii_case(key))
                    .ok_or_else(|| format!("world {world} has no revival point {key}"))?;
                Ok(spot(*world, r.pos[0], r.pos[2], format!("revival {key}")))
            }
            Source::DyoNpc { world, key } => {
                if !self.npcs.contains_key(world) {
                    let list = meta::meta_doc(&self.env, *world)
                        .map(|m| m.npcs)
                        .unwrap_or_default();
                    self.npcs.insert(*world, list);
                }
                let n = self
                    .npcs
                    .get(world)
                    .into_iter()
                    .flatten()
                    .find(|n| n.key.eq_ignore_ascii_case(key))
                    .ok_or_else(|| format!("world {world} has no NPC {key}"))?;
                Ok(spot(*world, n.x, n.z, format!(".dyo NPC {}", n.key)))
            }
            Source::Const {
                world,
                x,
                z,
                source,
            } => {
                if data.worlds.world(*world).is_none() {
                    return Err(format!("world {world} is not loaded"));
                }
                Ok(spot(*world, *x, *z, source.clone()))
            }
        }
    }
}

// ---- building the list -----------------------------------------------------------------------------------------------

/// A lower-case slug: letters and digits, other runs become `-`.
pub fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// The default rows resolved against the data.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Defaults {
    /// The resolved rows in file order (`kind` default).
    pub rows: Vec<LocationRow>,
    /// One row per world: its `.wld` camera (`kind` world, group [`ALL_WORLDS`]).
    pub worlds: Vec<LocationRow>,
    /// `group/name` of the rows that did not resolve, with the reason.
    pub unresolved: Vec<String>,
    /// The zoom of each group.
    pub zoom: BTreeMap<String, f32>,
}

fn row(id: String, group: &str, name: &str, spot: Spot, kind: LocationKind) -> LocationRow {
    LocationRow {
        id,
        group: group.to_owned(),
        name: name.to_owned(),
        world: spot.world,
        layer_policy: LocationPolicy::Fixed0,
        x: spot.x,
        z: spot.z,
        zoom: None,
        source: spot.source,
        thumb: spot.thumb,
        min_level: spot.levels.map(|l| l.0),
        max_level: spot.levels.map(|l| l.1),
        kind,
        editable: false,
        bookmark_id: None,
        own_zoom: None,
        sort: 0,
        created_by: None,
        updated_at: None,
        overrides: None,
    }
}

/// Resolves the rows of `file` and the camera of every world against the data.
pub fn build_defaults(env: &WorldEnv<'_>, file: &DefaultsFile) -> Defaults {
    let pick_live = meta::pick_live_worlds(env.data);
    let policy = |world: WorldId| LocationPolicy::from(meta::layer_policy(world, &pick_live));
    let mut resolver = Resolver::new(*env);
    let mut out = Defaults {
        zoom: file.zoom.clone(),
        ..Defaults::default()
    };
    for r in &file.location {
        match resolver.resolve(&r.from) {
            Ok(spot) => {
                let world = spot.world;
                let mut l = row(
                    format!("d:{}:{}", slug(&r.group), slug(&r.name)),
                    r.group.trim(),
                    r.name.trim(),
                    spot,
                    LocationKind::Default,
                );
                l.layer_policy = r.policy.unwrap_or_else(|| policy(world));
                l.zoom = r.zoom.or_else(|| file.zoom.get(r.group.trim()).copied());
                out.rows.push(l);
            }
            Err(why) => out
                .unresolved
                .push(format!("{}/{}: {why}", r.group, r.name)),
        }
    }
    for id in env.data.worlds.loadable_ids() {
        let Some((x, z)) = camera_of(env, id) else {
            continue;
        };
        let name = world_name(env.data, id);
        let mut l = row(
            format!("w:{id}"),
            ALL_WORLDS,
            &name,
            spot(id, x, z, ".wld camera"),
            LocationKind::World,
        );
        l.layer_policy = policy(id);
        out.worlds.push(l);
    }
    out
}

/// The default rows of the embedded file against the data (an invalid embedded file is a programming error that the
/// unit test catches; at run time it yields no default rows and says so).
pub fn embedded_defaults(env: &WorldEnv<'_>) -> Defaults {
    match parse_defaults(DEFAULTS_TOML) {
        Ok(file) => build_defaults(env, &file),
        Err(e) => {
            tracing::error!("locations.default.toml: {e}");
            Defaults {
                unresolved: vec![format!("locations.default.toml: {e}")],
                ..Defaults::default()
            }
        }
    }
}

fn bookmark_row(
    b: &AdminBookmarkRecord,
    preset: Option<f32>,
    overrides: Option<String>,
) -> LocationRow {
    LocationRow {
        id: format!("b:{}", b.id),
        group: b.group.clone(),
        name: b.name.clone(),
        world: b.world_id,
        layer_policy: b.layer_policy.into(),
        x: b.x,
        z: b.z,
        zoom: b.zoom.or(preset),
        source: "bookmark".to_owned(),
        thumb: None,
        min_level: None,
        max_level: None,
        kind: LocationKind::Bookmark,
        editable: true,
        bookmark_id: Some(b.id),
        own_zoom: b.zoom,
        sort: b.sort,
        created_by: Some(b.created_by.clone()),
        updated_at: Some(b.updated_at.0),
        overrides,
    }
}

/// Merges the shared bookmarks into the defaults: a bookmark with the group and name of a default row replaces it,
/// the groups come in the order of the file, then the groups of bookmarks (by label), then "All worlds".
pub fn merge(defaults: &Defaults, bookmarks: &[AdminBookmarkRecord]) -> LocationsDoc {
    let key = |g: &str, n: &str| (g.trim().to_lowercase(), n.trim().to_lowercase());
    let mut replaced: HashMap<(String, String), String> = HashMap::new();
    for b in bookmarks {
        let k = key(&b.group, &b.name);
        if let Some(d) = defaults
            .rows
            .iter()
            .chain(defaults.worlds.iter())
            .find(|d| key(&d.group, &d.name) == k)
        {
            replaced.insert(k, d.id.clone());
        }
    }
    let mut marks: Vec<LocationRow> = bookmarks
        .iter()
        .map(|b| {
            let preset = defaults.zoom.get(b.group.trim()).copied();
            bookmark_row(b, preset, replaced.get(&key(&b.group, &b.name)).cloned())
        })
        .collect();
    marks.sort_by(|a, b| {
        (a.sort, a.name.to_lowercase(), a.bookmark_id).cmp(&(
            b.sort,
            b.name.to_lowercase(),
            b.bookmark_id,
        ))
    });

    let kept = |d: &&LocationRow| !replaced.contains_key(&key(&d.group, &d.name));
    let mut order: Vec<String> = Vec::new();
    let mut note = |label: &str| {
        if !order.iter().any(|g| g.eq_ignore_ascii_case(label)) {
            order.push(label.to_owned());
        }
    };
    for d in &defaults.rows {
        note(&d.group);
    }
    let mut extra: Vec<&str> = marks
        .iter()
        .map(|m| m.group.as_str())
        .filter(|g| !g.eq_ignore_ascii_case(ALL_WORLDS))
        .collect();
    extra.sort_by_key(|g| g.to_lowercase());
    for g in extra {
        note(g);
    }
    note(ALL_WORLDS);
    if defaults.worlds.is_empty()
        && !marks
            .iter()
            .any(|m| m.group.eq_ignore_ascii_case(ALL_WORLDS))
    {
        order.retain(|g| !g.eq_ignore_ascii_case(ALL_WORLDS));
    }

    let mut locations: Vec<LocationRow> = Vec::new();
    let mut groups: Vec<GroupDoc> = Vec::new();
    for label in &order {
        let same = |g: &str| g.eq_ignore_ascii_case(label);
        let start = locations.len();
        locations.extend(
            defaults
                .rows
                .iter()
                .filter(kept)
                .filter(|d| same(&d.group))
                .cloned(),
        );
        if same(ALL_WORLDS) {
            locations.extend(defaults.worlds.iter().filter(kept).cloned());
        }
        locations.extend(marks.iter().filter(|m| same(&m.group)).cloned());
        let count = locations.len() - start;
        if count > 0 {
            groups.push(GroupDoc {
                id: slug(label),
                label: label.clone(),
                count: u32::try_from(count).unwrap_or(u32::MAX),
            });
        }
    }
    LocationsDoc {
        groups,
        locations,
        unresolved: defaults.unresolved.clone(),
    }
}

// ---- routes ---------------------------------------------------------------------------------------------------------------

/// Mounts the routes of this area.
pub fn mount(r: Routes<ApiState>) -> Routes<ApiState> {
    r.get("/admin/api/locations", list)
        .post("/admin/api/locations", create)
        .put("/admin/api/locations/{id}", update)
        .delete("/admin/api/locations/{id}", delete)
}

fn env(st: &ApiState) -> WorldEnv<'_> {
    let a = &st.ops.http.assets;
    WorldEnv {
        vfs: &a.vfs,
        data: &a.data,
        manifest: &a.manifest,
        prep: &a.prep,
    }
}

async fn stored(st: &ApiState) -> ApiResult<Vec<AdminBookmarkRecord>> {
    st.ops
        .persist
        .admin_read(AdminReadQuery::Bookmarks)
        .await
        .map_err(|e| ApiError::db(&e))?
        .bookmarks()
        .map_err(|e| ApiError::db(&e))
}

/// The defaults of this process: resolved once per request off the async threads (a few milliseconds: 40 lookups and one
/// `.dyo`), the bookmarks are read fresh.
async fn defaults(st: &ApiState) -> ApiResult<Defaults> {
    let st2 = st.clone();
    tokio::task::spawn_blocking(move || embedded_defaults(&env(&st2)))
        .await
        .map_err(|_| ApiError::unavailable("the locations could not be resolved"))
}

/// `GET /admin/api/locations`.
async fn list(State(st): State<ApiState>, Who(_who): Who) -> ApiResult<Json<LocationsDoc>> {
    let d = defaults(&st).await?;
    let marks = stored(&st).await?;
    Ok(Json(merge(&d, &marks)))
}

/// The rows whose name or group contains `needle` (lower case), the best match first: what the command palette searches (Plan/25 §2.3). A
/// default row or a bookmark comes before the camera of a world; at most `limit` rows.
pub async fn search(st: &ApiState, needle: &str, limit: usize) -> ApiResult<Vec<LocationRow>> {
    let d = defaults(st).await?;
    let marks = stored(st).await?;
    Ok(rank_matches(merge(&d, &marks).locations, needle, limit))
}

/// The ranking of [`search`]: exact name, name prefix, name contains, group contains.
pub fn rank_matches(rows: Vec<LocationRow>, needle: &str, limit: usize) -> Vec<LocationRow> {
    let mut hits: Vec<(u8, bool, usize, LocationRow)> = rows
        .into_iter()
        .filter_map(|r| {
            let name = r.name.to_lowercase();
            let rank = if name == needle {
                0
            } else if name.starts_with(needle) {
                1
            } else if name.contains(needle) {
                2
            } else if r.group.to_lowercase().contains(needle) {
                3
            } else {
                return None;
            };
            Some((rank, r.kind == LocationKind::World, name.len(), r))
        })
        .collect();
    hits.sort_by(|a, b| {
        (a.0, a.1, a.2, a.3.name.to_lowercase(), &a.3.id).cmp(&(
            b.0,
            b.1,
            b.2,
            b.3.name.to_lowercase(),
            &b.3.id,
        ))
    });
    hits.into_iter().take(limit).map(|h| h.3).collect()
}

/// Checks the fields of a bookmark body that need no data: the name, the group, the zoom and finite coordinates. Returns the
/// body with trimmed texts and the default group filled in.
pub fn check_fields(b: &BookmarkBody) -> ApiResult<BookmarkBody> {
    let name = b.name.trim().to_owned();
    if name.is_empty()
        || name.chars().count() > NAME_MAX_CHARS
        || name.chars().any(char::is_control)
    {
        return Err(ApiError::invalid(
            "name",
            format!("the name is 1..={NAME_MAX_CHARS} characters"),
        ));
    }
    let group = if b.group.trim().is_empty() {
        BOOKMARK_GROUP.to_owned()
    } else {
        b.group.trim().to_owned()
    };
    if group.chars().count() > GROUP_MAX_CHARS || group.chars().any(char::is_control) {
        return Err(ApiError::invalid(
            "group",
            format!("the group is at most {GROUP_MAX_CHARS} characters"),
        ));
    }
    if !b.x.is_finite() {
        return Err(ApiError::invalid("x", "x is not a number"));
    }
    if !b.z.is_finite() {
        return Err(ApiError::invalid("z", "z is not a number"));
    }
    if let Some(z) = b.zoom
        && !(z.is_finite() && (ZOOM_RANGE.0..=ZOOM_RANGE.1).contains(&z))
    {
        return Err(ApiError::invalid(
            "zoom",
            format!(
                "the zoom is {}..={} world units per pixel",
                ZOOM_RANGE.0, ZOOM_RANGE.1
            ),
        ));
    }
    Ok(BookmarkBody {
        group,
        name,
        ..b.clone()
    })
}

/// The size of a loaded world in metres.
fn extent_of(data: &GameData, world: WorldId) -> Option<(f32, f32)> {
    let s = &data.worlds.folder_data(world)?.settings;
    let land = 128 * if s.mpu == 0 { 4 } else { s.mpu };
    #[allow(clippy::cast_precision_loss)]
    Some(((s.size.0 * land) as f32, (s.size.1 * land) as f32))
}

/// Checks a bookmark body against the data: [`check_fields`], a world the server loads and a position inside it.
pub fn check_body(data: &GameData, b: &BookmarkBody) -> ApiResult<BookmarkBody> {
    let body = check_fields(b)?;
    let (ex, ez) = if data.worlds.loadable_ids().any(|w| w == body.world) {
        extent_of(data, body.world).unwrap_or((0.0, 0.0))
    } else {
        return Err(ApiError::invalid(
            "world",
            "the server does not load this world",
        ));
    };
    if !(0.0..=ex).contains(&body.x) {
        return Err(ApiError::invalid("x", "x is outside the world"));
    }
    if !(0.0..=ez).contains(&body.z) {
        return Err(ApiError::invalid("z", "z is outside the world"));
    }
    Ok(body)
}

fn record(
    data: &GameData,
    b: &BookmarkBody,
    who: &Identity,
    now: flyff_common::UnixTime,
    id: i64,
) -> AdminBookmarkRecord {
    let pick_live = meta::pick_live_worlds(data);
    let policy = b
        .layer_policy
        .unwrap_or_else(|| meta::layer_policy(b.world, &pick_live).into());
    AdminBookmarkRecord {
        id,
        group: b.group.clone(),
        name: b.name.clone(),
        world_id: b.world,
        layer_policy: policy.into(),
        x: b.x,
        z: b.z,
        zoom: b.zoom,
        sort: b.sort,
        created_by: who.label(),
        created_at: now,
        updated_at: now,
    }
}

fn db_error(e: &flyff_db::DbError) -> ApiError {
    match e.kind {
        DbErrorKind::Invalid => ApiError::invalid("bookmark", e.to_string()),
        DbErrorKind::NotFound => ApiError::not_found("no such bookmark"),
        _ => ApiError::db(e),
    }
}

fn same_name(a: &AdminBookmarkRecord, b: &BookmarkBody) -> bool {
    a.group.eq_ignore_ascii_case(&b.group) && a.name.eq_ignore_ascii_case(&b.name)
}

async fn row_of(st: &ApiState, rec: &AdminBookmarkRecord) -> ApiResult<LocationRow> {
    let d = defaults(st).await?;
    let doc = merge(&d, std::slice::from_ref(rec));
    doc.locations
        .into_iter()
        .find(|l| l.bookmark_id == Some(rec.id))
        .ok_or_else(|| ApiError::internal("the stored bookmark does not read back"))
}

/// `POST /admin/api/locations`.
async fn create(
    State(st): State<ApiState>,
    Who(who): Who,
    MutationJson(m): MutationJson<BookmarkBody>,
) -> ApiResult<Json<LocationRow>> {
    let reason = st.policy().reason_optional(m.reason.as_ref())?;
    let body = check_body(&st.ops.http.assets.data, &m.body)?;
    let existing = stored(&st).await?;
    if existing.len() >= MAX_BOOKMARKS {
        return Err(ApiError::invalid(
            "bookmark",
            format!("at most {MAX_BOOKMARKS} shared bookmarks"),
        ));
    }
    if existing.iter().any(|e| same_name(e, &body)) {
        return Err(ApiError::invalid(
            "name",
            "a bookmark of that name exists in this group",
        ));
    }
    let now = st.ops.admin.clock.unix();
    let rec = record(&st.ops.http.assets.data, &body, &who, now, 0);
    let trail = AuditTrail::begin(
        &st.ops.audit,
        &who,
        "admin.bookmark",
        reason,
        json!({"action": "create", "name": rec.name, "group": rec.group, "world": rec.world_id}),
    )?;
    let out = st
        .ops
        .persist
        .call(AtomicBatch::of_ops(
            AtomicKind::Other("admin_bookmark"),
            vec![DbOp::InsertAdminBookmark(rec.clone())],
        ))
        .await;
    let id = match out {
        Ok(AtomicOutcome::RowInserted(id)) => id,
        Ok(other) => {
            trail.failed("unexpected outcome");
            return Err(ApiError::internal(&format!(
                "a bookmark insert answered {other:?}"
            )));
        }
        Err(e) => {
            trail.failed(&e.to_string());
            return Err(db_error(&e));
        }
    };
    trail.done(json!({"id": id}));
    Ok(Json(row_of(&st, &AdminBookmarkRecord { id, ..rec }).await?))
}

/// `PUT /admin/api/locations/{id}`: every field but the author and the creation time is replaced.
async fn update(
    State(st): State<ApiState>,
    Who(who): Who,
    Path(id): Path<i64>,
    MutationJson(m): MutationJson<BookmarkBody>,
) -> ApiResult<Json<LocationRow>> {
    let reason = st.policy().reason_optional(m.reason.as_ref())?;
    let body = check_body(&st.ops.http.assets.data, &m.body)?;
    let all = stored(&st).await?;
    let old = all
        .iter()
        .find(|b| b.id == id)
        .cloned()
        .ok_or_else(|| ApiError::not_found("no such bookmark"))?;
    if all.iter().any(|e| e.id != id && same_name(e, &body)) {
        return Err(ApiError::invalid(
            "name",
            "a bookmark of that name exists in this group",
        ));
    }
    let now = st.ops.admin.clock.unix();
    let mut rec = record(&st.ops.http.assets.data, &body, &who, now, id);
    rec.created_by = old.created_by.clone();
    rec.created_at = old.created_at;
    let trail = AuditTrail::begin(
        &st.ops.audit,
        &who,
        "admin.bookmark",
        reason,
        json!({"action": "update", "id": id, "name": rec.name, "group": rec.group, "world": rec.world_id}),
    )?;
    match st
        .ops
        .persist
        .call(AtomicBatch::of_ops(
            AtomicKind::Other("admin_bookmark"),
            vec![DbOp::UpdateAdminBookmark(rec.clone())],
        ))
        .await
    {
        Ok(AtomicOutcome::Changed(n)) if n > 0 => {}
        Ok(_) => {
            trail.failed("no such bookmark");
            return Err(ApiError::not_found("no such bookmark"));
        }
        Err(e) => {
            trail.failed(&e.to_string());
            return Err(db_error(&e));
        }
    }
    trail.done(json!({}));
    Ok(Json(row_of(&st, &rec).await?))
}

/// `DELETE /admin/api/locations/{id}` (the body, an optional `Mutation` envelope, may carry a reason).
async fn delete(
    State(st): State<ApiState>,
    Who(who): Who,
    Path(id): Path<i64>,
    body: Bytes,
) -> ApiResult<Json<serde_json::Value>> {
    let reason = if body.iter().all(u8::is_ascii_whitespace) {
        None
    } else {
        let m: crate::admin::dto::Mutation<serde_json::Value> = serde_json::from_slice(&body)
            .map_err(|e| ApiError::bad_request(format!("malformed body: {e}")))?;
        st.policy().reason_optional(m.reason.as_ref())?
    };
    let name = stored(&st)
        .await?
        .into_iter()
        .find(|b| b.id == id)
        .map(|b| b.name)
        .ok_or_else(|| ApiError::not_found("no such bookmark"))?;
    let trail = AuditTrail::begin(
        &st.ops.audit,
        &who,
        "admin.bookmark",
        reason,
        json!({"action": "delete", "id": id, "name": name}),
    )?;
    match st
        .ops
        .persist
        .call(AtomicBatch::of_ops(
            AtomicKind::Other("admin_bookmark"),
            vec![DbOp::DeleteAdminBookmark { id }],
        ))
        .await
    {
        Ok(AtomicOutcome::Changed(n)) if n > 0 => {}
        Ok(_) => {
            trail.failed("no such bookmark");
            return Err(ApiError::not_found("no such bookmark"));
        }
        Err(e) => {
            trail.failed(&e.to_string());
            return Err(db_error(&e));
        }
    }
    trail.done(json!({}));
    Ok(Json(json!({"id": id})))
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
    push::<LocationPolicy>(out, cfg);
    push::<LocationKind>(out, cfg);
    push::<LocationRow>(out, cfg);
    push::<GroupDoc>(out, cfg);
    push::<LocationsDoc>(out, cfg);
    push::<BookmarkBody>(out, cfg);
}
