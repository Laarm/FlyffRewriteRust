//! The bundle DTOs (Plan/14 §10.5, §10.7): the `data` section of every bundle, the envelope and the
//! manifest. Always compiled and wasm-safe (only `serde`, `indexmap` and the `GameData` value types):
//! `flyff-wasm` decodes the bundles with these types, the exporter (feature `bundle`) fills them, and the
//! JSON Schemas and TypeScript types are generated **from** them (feature `schema`), so none of the three
//! can drift.
//!
//! Conventions (14 §10.1):
//! * every path is a canonical VFS path (`normalize_resource_path`: lower case, `/`); fields documented as
//!   "bare file name" are joined to a fixed directory by the client and exported as written,
//! * ids are the define values, `NULL_ID` = 4294967295, `f32` values keep their exact bits (shortest
//!   round-trip text), strings are UTF-8 with the IDS keys already resolved,
//! * large homogeneous tables are [`RowTable`]s (columnar), everything is deterministic (sorted maps,
//!   file-order lists).

use super::rowtable::RowTable;
use crate::character::{NpcBuffSkill, NpcFigure};
use crate::client::{AtlasSprite, GuideStep, HelpEntry, HelpTreeNode};
use crate::exp_table::SetItemAvail;
use crate::items_ext::accessory::AccessoryDef;
use crate::items_ext::exchange::ExchangeMenu;
use crate::items_ext::item_etc::{PiercingDef, RandomOptItemDef, SetItemDef};
use crate::items_ext::random_option::RandomOptionGroup;
use crate::items_ext::ultimate::GemAbilityDef;
use crate::props::ctrl::CtrlProp;
use crate::props::item::ItemProp;
use crate::props::job::JobProp;
use crate::props::motion::MotionProp;
use crate::props::mover::MoverProp;
use crate::props::skill_add::AddSkillProp;
use crate::rules::pk_setting::PropensityPenalty;
use crate::social::guild::GuildBuffDef;
use crate::social::lord::LordSkill;
use crate::worlds::ContinentDef;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// `BUNDLE_FORMAT`: the version of the envelope and manifest shapes (14 §10.8).
pub const BUNDLE_FORMAT: u32 = 1;

/// When the client loads a bundle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum LoadClass {
    /// Fetched at boot (the 19 boot-class bundles, about 9.6 MB raw / 1.6 MB gzip).
    Boot,
    /// Fetched on first use (`help`, `quests`, `defines`).
    Lazy,
}

/// The envelope of every bundle file: `{ "bundle": "items", "schema": 1, "data": { ... } }`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Envelope<D> {
    /// The bundle name (one of [`super::BUNDLE_NAMES`]).
    pub bundle: String,
    /// The per-bundle schema version (bumped when a field is removed, renamed or changes meaning).
    pub schema: u32,
    /// The bundle-specific data.
    pub data: D,
}

/// The `data` type of one bundle: its name, schema version and load class.
pub trait BundleData {
    /// The bundle name.
    const NAME: &'static str;
    /// The schema version (14 §10.8).
    const SCHEMA: u32;
    /// Boot or lazy.
    const LOAD: LoadClass;
}

macro_rules! bundle_data {
    ($t:ty, $name:literal, $schema:literal, $load:ident) => {
        impl BundleData for $t {
            const NAME: &'static str = $name;
            const SCHEMA: u32 = $schema;
            const LOAD: LoadClass = LoadClass::$load;
        }
    };
}

// ---- manifest -------------------------------------------------------------------------------------------------

/// One bundle in `manifest.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ManifestEntry {
    /// `/data/<bundle>.<h16>.json`.
    pub url: String,
    /// The schema version of the bundle.
    pub schema: u32,
    /// The size of the uncompressed file.
    pub bytes: u64,
    /// The size of the gzip file (level 9).
    pub gz_bytes: u64,
    /// Boot or lazy.
    pub load: LoadClass,
}

/// `cache/data/manifest.json` (14 §10.3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct BundleManifest {
    /// [`BUNDLE_FORMAT`].
    pub format: u32,
    /// `xxh3_64` (16 lower-hex digits) over the sorted `name:h16` lines of the bundles; the client reloads
    /// when it differs from the one it loaded.
    pub data_version: String,
    /// `flyff <version> (<commit>)`.
    pub server_build: String,
    /// The bundles by name.
    pub bundles: BTreeMap<String, ManifestEntry>,
}

// ---- items, item-ext, skills, movers --------------------------------------------------------------------------

/// `items`: all `propItem` rows as seen by the client (Plan/14 §10.5). The `name_key` and
/// `description_key` columns are not exported (`ClientItem` = `Omit<ItemProp, "name_key" | "description_key">`);
/// the ultimate-weapon names carry the client transform of §10.6.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ItemsBundle {
    /// The rows in ascending id order.
    #[cfg_attr(feature = "schema", schemars(extend("x-flyff-omit" = ["name_key", "description_key"])))]
    pub items: RowTable<ItemProp>,
}
bundle_data!(ItemsBundle, "items", 1, Boot);

/// The columns of [`ItemsBundle::items`] and of the two skill tables that are not exported.
pub const CLIENT_ITEM_OMIT: [&str; 2] = ["name_key", "description_key"];
/// The columns of [`MoversBundle::movers`] that are not exported (`ClientMover`: no AI/drop block).
pub const CLIENT_MOVER_OMIT: [&str; 2] = ["name_key", "ex"];

/// `skills`: skills, party skills, the add-skill levels and the skills of each job.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SkillsBundle {
    /// `propSkill` rows (`SI_*`).
    #[cfg_attr(feature = "schema", schemars(extend("x-flyff-omit" = ["name_key", "description_key"])))]
    pub skills: RowTable<ItemProp>,
    /// `propTroupeSkill` rows (`ST_*`).
    #[cfg_attr(feature = "schema", schemars(extend("x-flyff-omit" = ["name_key", "description_key"])))]
    pub party_skills: RowTable<ItemProp>,
    /// `propSkillAdd` rows (`SA_*`), the per-level numbers of every skill.
    pub levels: RowTable<AddSkillProp>,
    /// `job_skills[job]`: the skill ids of each job (32 jobs), ordered by the required level.
    pub job_skills: Vec<Vec<u32>>,
}
bundle_data!(SkillsBundle, "skills", 1, Boot);

/// `movers`: monsters, NPC models and pets (no AI, drops or respawn data) and the controls.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MoversBundle {
    /// `propMover` rows (`MI_*`) without the `ex` block and the name key.
    #[cfg_attr(feature = "schema", schemars(extend("x-flyff-omit" = ["name_key", "ex"])))]
    pub movers: RowTable<MoverProp>,
    /// `propCtrl` rows (`CI_*`).
    pub ctrls: RowTable<CtrlProp>,
}
bundle_data!(MoversBundle, "movers", 1, Boot);

/// One available pet option (the hatch probability stays server-side).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PetAvail {
    /// `DST_*`.
    pub dst: u32,
    /// The values per pet level (9).
    pub values: Vec<i32>,
    /// The item that grants it.
    pub item: u32,
    /// The mover ids that can have it.
    pub movers: Vec<u32>,
}

/// The pet display tables.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PetTablesDef {
    /// `AvailDestParam` rows.
    pub avail: Vec<PetAvail>,
    /// `MaxEnergy` per pet level.
    pub max_energy: Vec<i32>,
    /// `FeedEnergy N` blocks: rows of `(cost min, cost max, energy min, energy max)`.
    pub feed_energy: Vec<Vec<[i32; 4]>>,
}

/// The enchant effect table (`propEnchant.inc`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EnchantDef {
    /// `NORMAL`, `AL`, `NONE` limit levels.
    pub limit_level: Vec<i32>,
    /// `NORMAL_SCAL` and `ALL_SCAL` rows of ten scales.
    pub level_scale: Vec<Vec<f32>>,
}

/// `item-ext`: the item extension tables the client shows (14 §10.4; no gift box or pack contents, no
/// `s.txt`, no ultimate probabilities).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ItemExtBundle {
    /// Armor sets (`SetItem`).
    pub set_items: Vec<SetItemDef>,
    /// Random-option items (`RandomOptItem`).
    pub random_opt_items: Vec<RandomOptItemDef>,
    /// Piercing materials.
    pub piercing: Vec<PiercingDef>,
    /// Accessory bonuses.
    pub accessory: Vec<AccessoryDef>,
    /// Ultimate gem abilities.
    pub gem_ability: Vec<GemAbilityDef>,
    /// Awakening, blessing and pet random options.
    pub random_options: Vec<RandomOptionGroup>,
    /// Pet display tables.
    pub pet: PetTablesDef,
    /// Exchange menus.
    pub exchange: Vec<ExchangeMenu>,
    /// The enchant effect scales.
    pub enchant: EnchantDef,
    // ---- tickets (task ui-minigames, WP-CFEAT-45)
    /// `ticket.inc` with the expanded layers of `layer.inc`, ascending by item id (empty in an old bundle).
    #[serde(default)]
    pub tickets: Vec<TicketRow>,
}
bundle_data!(ItemExtBundle, "item-ext", 1, Boot);

/// One ticket of `ticket.inc` as the client's item dispatch needs it (`CTicketProperty` + `CLayerProperty`, R24 of Plan/45 §9.5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TicketRow {
    /// The ticket item (`II_*`).
    pub item: u32,
    /// The world the ticket opens (`WI_*`).
    pub world: u32,
    /// `GetExpanedLayer( world )` of `layer.inc`: the layers `-expand ..= 0` the holder may enter (0 without a line).
    pub expand: i32,
}

// ---- npcs ----------------------------------------------------------------------------------------------------------

/// One vendor stock entry.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct NpcStock {
    /// The item.
    pub item: u32,
    /// The stack count (`(short)pack_max`).
    pub count: i16,
}

/// One vendor tab: its title and the precomputed stock.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct NpcTab {
    /// The tab title (may be empty).
    pub title: String,
    /// The items for sale in tab order.
    pub stock: Vec<NpcStock>,
}

/// One NPC definition (`character.inc` family).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct NpcRow {
    /// The key as written (`MaFl_Mikyel`); lookups are case-insensitive.
    pub key: String,
    /// The display name.
    pub name: String,
    /// The portrait as a canonical VFS path under `char/`; empty when the NPC has none.
    pub portrait: String,
    /// The NPC has a dialog script.
    pub dialog: bool,
    /// The script key (`dialog_file` minus its extension, lower-cased), when there is a dialog.
    pub dialog_key: Option<String>,
    /// `SRT_*` (-1 = none).
    pub structure: i32,
    /// `BGM_*` played during the dialog (0 = none).
    pub music: u32,
    /// `SetFigure`.
    pub figure: Option<NpcFigure>,
    /// `SetEquip`.
    pub equip: Vec<u32>,
    /// The enabled `MMI_*` menu ids.
    pub menus: Vec<u32>,
    /// 0 = penya shop, 1 = chip shop.
    pub vendor_type: i32,
    /// Exactly four vendor tabs.
    pub tabs: Vec<NpcTab>,
    /// Buff skills the NPC casts.
    pub buff_skills: Vec<NpcBuffSkill>,
    /// Teleporter destinations `(x, z)`.
    pub teleports: Vec<[f32; 2]>,
}

/// `npcs`: all 403 NPC definitions in file order.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct NpcsBundle {
    /// The NPCs.
    pub npcs: Vec<NpcRow>,
}
bundle_data!(NpcsBundle, "npcs", 1, Boot);

// ---- texts, help -----------------------------------------------------------------------------------------------

/// One `textClient.inc` entry.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TextRow {
    /// The text id (`TID_*`).
    pub id: u32,
    /// ARGB colour.
    pub color: u32,
    /// The format string (`%s`/`%d` placeholders), double quotes removed.
    pub text: String,
}

/// One chat emoticon command.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EmotionRow {
    /// The emoticon index.
    pub id: u32,
    /// The command (`laugh`, `sad`, ...).
    pub text: String,
}

/// `texts`: the text table, the emoticon commands and the NPC dialog lines.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TextsBundle {
    /// The non-empty texts in ascending id order.
    pub tids: RowTable<TextRow>,
    /// The `GETTEXT` bound: `getText(tid)` is `"error"` for `tid >= size`, `""` for a hole.
    pub size: u32,
    /// `textEmotion.txt`.
    pub emotions: Vec<EmotionRow>,
    /// `WorldDialog.txt` lines (line `n` is dialog string `n`).
    pub dialog: Vec<String>,
}
bundle_data!(TextsBundle, "texts", 1, Boot);

/// One help topic (`help.inc`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HelpTopic {
    /// The topic key.
    pub key: String,
    /// `"   " + join(strings, "\n\n   ")`.
    pub text: String,
}

/// `help` (lazy): the help family, the guide and the text panels.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HelpBundle {
    /// Help topics in file order.
    pub topics: Vec<HelpTopic>,
    /// The help tree.
    pub tree: Vec<HelpTreeNode>,
    /// The FAQ (answers carry four leading spaces).
    pub faq: Vec<HelpEntry>,
    /// Login tips (four leading spaces).
    pub tips: Vec<String>,
    /// Instant-help lines.
    pub instant: Vec<HelpEntry>,
    /// Tutorial pages.
    pub tutorial: Vec<HelpEntry>,
    /// The guide steps.
    pub guide: Vec<GuideStep>,
    /// The text panels of `Client/` by lower-case file stem (without the login notice).
    pub panels: BTreeMap<String, String>,
    /// The login notice.
    pub notice: String,
}
bundle_data!(HelpBundle, "help", 1, Lazy);

// ---- worlds ------------------------------------------------------------------------------------------------------

/// One `World.inc` entry.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct WorldRow {
    /// `WI_*`.
    pub id: u32,
    /// The define name.
    pub define: String,
    /// The world folder as written.
    pub folder: String,
    /// The display title (`""` for `WI_WORLD_QUIZ`).
    pub title: String,
    /// The folder exists and has a `.wld` (the 51 worlds the server loads).
    pub loadable: bool,
}

/// The `.wld` settings of one folder the client needs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct WorldSettingsDto {
    /// Lands in x and z.
    pub size: [u32; 2],
    /// Indoor world: no sky, no weather, fixed light.
    pub indoor: bool,
    /// Flying allowed.
    pub fly: bool,
    /// `BGM_*` of the world (0 = the default).
    pub bgm: u32,
    /// Default region attribute flags of the whole world.
    pub pk_mode: u32,
    /// Indoor ambient colour.
    pub ambient: u32,
    /// Indoor diffuse colour.
    pub diffuse: u32,
    /// Indoor light direction.
    pub light_dir: [f32; 3],
    /// Fog start, end and density.
    pub fog: [f32; 3],
    /// Metres per unit: the land edge is 128 x `mpu` world units.
    pub mpu: u32,
    /// Sky textures `[day, evening, night]` as canonical paths under `weather/`; `null` = the built-in one.
    pub sky: Vec<Option<String>>,
    /// Cloud textures, same order.
    pub cloud: Vec<Option<String>>,
    /// The sun texture (`weather/`), `null` = built-in.
    pub sun: Option<String>,
    /// The moon texture (`weather/`), `null` = built-in.
    pub moon: Option<String>,
    /// The 24 hourly light entries (`r1 g1 b1 r2 g2 b2`) when `use24light`.
    pub light24: Option<Vec<Vec<f32>>>,
}

/// One region of the client's `m_aRegion` (14 §10.5): every `.rgn` region that is not a begin, revival or
/// structure region, in file order.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ClientRegion {
    /// The region kind (`RI_*`).
    pub index: u32,
    /// `[left, top, right, bottom]` = `[xmin, zmin, xmax, zmax]`; containment is `left <= x < right && top <= z < bottom`.
    pub rect: [i32; 4],
    /// `RA_*` attribute flags.
    pub attribute: u32,
    /// `BGM_*` of the region.
    pub music: u32,
    /// The music plays directly (no fade).
    pub direct_music: bool,
    /// The teleport target world of a trigger region.
    pub tele_world: u32,
    /// The region key.
    pub key: String,
    /// The display title.
    pub title: Option<String>,
    /// The description.
    pub desc: Option<String>,
}

/// The client data of one world folder.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct WorldFolderDto {
    /// The `.wld` settings.
    pub settings: WorldSettingsDto,
    /// The client regions.
    pub regions: Vec<ClientRegion>,
    /// The continents of `.wld.cnt`.
    pub continents: Vec<ContinentDef>,
}

/// One world-map texture and its world rectangle (`WORLD_MAPS`, a code table).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct WorldMapRow {
    /// The map number 0..=7.
    pub map: u32,
    /// The texture as a canonical VFS path (`theme/world_*.dds`).
    pub texture: String,
    /// The continent (`ContinentDef.id`) shown on this map; `null` for map 0.
    pub continent: Option<u32>,
    /// `[x1, z1, x2, z2]` in world coordinates; the screen z axis is inverted.
    pub rect: [i32; 4],
}

/// One monster information record of a continent map.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MapMonsterRow {
    /// The running record index = the sprite index in the atlas.
    pub frame: u32,
    /// 1..=5 mover ids, the last one is the giant.
    pub movers: Vec<u32>,
    /// Left/top in the 1280x960 reference space.
    pub pos: [i32; 2],
    /// Width and height of the hover rectangle.
    pub size: [i32; 2],
    /// The drop item (0 = none).
    pub drop_item: u32,
}

/// The monster records of one map.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MapMonsterDto {
    /// The map number 1..=7.
    pub map: u32,
    /// The key of its sprites in the `ui-atlases` bundle.
    pub atlas: String,
    /// The records.
    pub entries: Vec<MapMonsterRow>,
}

/// One rainbow race NPC marker.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RainbowMarkerDto {
    /// The running record index = the sprite index in `theme/texmaprainbow_npc.inc`.
    pub frame: u32,
    /// The continent map number the `map_pos` applies on.
    pub map: u32,
    /// Position on the whole-world map.
    pub total_pos: [i32; 2],
    /// Position on the continent map.
    pub map_pos: [i32; 2],
    /// Width and height.
    pub size: [i32; 2],
}

/// `worlds`: the world list, the client data of each folder, the world-map code table and the map records.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct WorldsBundle {
    /// `World.inc` in id order.
    pub worlds: Vec<WorldRow>,
    /// The client data per world folder (lower-case folder name).
    pub folders: BTreeMap<String, WorldFolderDto>,
    /// The 8 world-map textures with their rectangles.
    pub world_maps: Vec<WorldMapRow>,
    /// The monster records of the 7 continent maps.
    pub map_monsters: Vec<MapMonsterDto>,
    /// The rainbow race markers.
    pub rainbow_markers: Vec<RainbowMarkerDto>,
}
bundle_data!(WorldsBundle, "worlds", 1, Boot);

// ---- models, sfx-table, terrain -----------------------------------------------------------------------------------

/// One model registry entry with its resolved files.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ModelRow {
    /// The id within the object type (`MI_*`, `II_*`, `CI_*`, `XI_*`, the obj number).
    pub index: u32,
    /// The entry name.
    pub name: String,
    /// `MODELTYPE_*`.
    pub model_type: u32,
    /// The part name (`male/female` for items; the sound of an sfx).
    pub part: String,
    /// Flies.
    pub fly: bool,
    /// `MD_*` view distance class.
    pub distant: u32,
    /// Pickable.
    pub pick: bool,
    /// The default scale.
    pub scale: f32,
    /// Fades when it hides the player.
    pub trans: bool,
    /// Casts a shadow.
    pub shadow: bool,
    /// `ATEX_*` texture animation.
    pub texture_ex: u32,
    /// The render flag bit.
    pub render_flag: u32,
    /// The model file as a canonical VFS path, `null` when it does not exist.
    pub file: Option<String>,
    /// The skeleton file (`.chr`) of a mover, `null` when it does not exist.
    pub bone: Option<String>,
    /// INFORMATIVE ONLY (model viewer): the registry's motion list as written, each stem resolved to a VFS
    /// path or `null`. Clients MUST NOT resolve animations from it: playback uses the `ClipId`s of the
    /// motion table, which add the STAND fallback, the empty-slot quirk file and the wing-item slots.
    pub motions: Vec<(u32, Option<String>)>,
}

/// `models`: the model registry by object type.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ModelsBundle {
    /// The entries of each object type (`OT_*` as a decimal string).
    pub types: BTreeMap<String, RowTable<ModelRow>>,
}
bundle_data!(ModelsBundle, "models", 1, Boot);

/// One sfx entry (`mdlDyna.inc` section `sfx`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SfxRow {
    /// `XI_*`.
    pub id: u32,
    /// The entry name.
    pub name: String,
    /// The `.sfx` file as a canonical path under `sfx/`, `null` when it does not exist.
    pub file: Option<String>,
    /// The attached sound: a bare file name under `sound/` (44 §5.2), `null` when there is none.
    pub sound: Option<String>,
    /// The initial scale.
    pub scale: f32,
    /// `MD_*` view distance class.
    pub distant: u32,
    /// The render flag (skipped by the "SFX render off" option unless 1).
    pub render_flag: u32,
}

/// `sfx-table`: the effect models by `XI_` id.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SfxTableBundle {
    /// The sfx entries in ascending id order.
    pub sfx: RowTable<SfxRow>,
}
bundle_data!(SfxTableBundle, "sfx-table", 1, Boot);

/// One `Terrain.inc` row.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TerrainTexture {
    /// The innermost group name, `""` outside any group.
    pub folder: String,
    /// The texture id the `.lnd` layers use.
    pub id: u32,
    /// The second number of the row.
    pub frames: u32,
    /// The texture: a bare file name under `world/texture/`.
    pub file: String,
    /// The last string of the row (unused).
    pub sound: String,
    /// The enclosing `"name" N` group headers, outermost first; the water frame sets are rebuilt from them.
    pub group_path: Vec<(String, u32)>,
}

/// `terrain`: the terrain texture table in file order.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TerrainBundle {
    /// The rows.
    pub textures: Vec<TerrainTexture>,
}
bundle_data!(TerrainBundle, "terrain", 1, Boot);

// ---- resdata-layouts, ui-atlases, sounds, music -------------------------------------------------------------------

/// One control of an applet (14 §10.5; `visible`, `disabled` and `tab_stop` are dropped).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ControlRow {
    /// `WTYPE_*`.
    #[serde(rename = "type")]
    pub kind: u32,
    /// `WIDC_*` name as written.
    pub name: String,
    /// The define value, -1 for the four unknown names.
    pub id: i32,
    /// A bare file name under `theme/` (42), may be empty.
    pub texture: String,
    /// 9-tile skin versus single image.
    pub tile: bool,
    /// `[left, top, right, bottom]` relative to the parent client area.
    pub rect: [i32; 4],
    /// Style bits.
    pub style: u32,
    /// Starts a radio group.
    pub group: bool,
    /// Caption.
    pub caption: String,
    /// Tooltip.
    pub tooltip: String,
}

/// One window layout.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AppletRow {
    /// `APP_*` name.
    pub name: String,
    /// The define value.
    pub id: i32,
    /// A bare file name under `theme/`.
    pub texture: String,
    /// 12-tile skin versus single image.
    pub tile: bool,
    /// Initial width including the frame.
    pub w: i32,
    /// Initial height including the frame.
    pub h: i32,
    /// `WBS_*` bits.
    pub style: u32,
    /// The window title.
    pub title: String,
    /// The help key (non-empty adds the `?` button).
    pub help: String,
    /// The controls in paint order.
    pub controls: Vec<ControlRow>,
}

/// `resdata-layouts`: 365 applets with 2,809 controls.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ResdataBundle {
    /// The applets in file order.
    pub applets: Vec<AppletRow>,
}
bundle_data!(ResdataBundle, "resdata-layouts", 1, Boot);

/// One texture atlas.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AtlasDto {
    /// The texture as a canonical VFS path; empty for the two non-atlas files.
    pub texture: String,
    /// The colour key (0 = none).
    pub color_key: u32,
    /// The path was a file name under `theme/`.
    pub multi_language: bool,
    /// The texture size (0, 0 when unknown).
    pub size: [u32; 2],
    /// The sub-images in file order.
    pub sprites: Vec<AtlasSprite>,
}

/// `ui-atlases`: all 39 atlas descriptors by the canonical path of their `.inc`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UiAtlasesBundle {
    /// The atlases.
    pub atlases: BTreeMap<String, AtlasDto>,
}
bundle_data!(UiAtlasesBundle, "ui-atlases", 1, Boot);

/// `sounds`: `SND_`/`VOC_` id to `sound/<file>` (a canonical path), `null` = the file is missing, `""` =
/// silent. An id that the file does not define is absent.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SoundsBundle {
    /// id to file.
    pub sounds: BTreeMap<u32, Option<String>>,
}
bundle_data!(SoundsBundle, "sounds", 1, Boot);

/// `music`: `BGM_` id to `music/<file>` (a canonical path), `null` = the file is missing; ids absent from
/// `default.bgm` are absent.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MusicBundle {
    /// id to file.
    pub music: BTreeMap<u32, Option<String>>,
}
bundle_data!(MusicBundle, "music", 1, Boot);

// ---- honor, tables, names ------------------------------------------------------------------------------------------

/// One honor title.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HonorTitleRow {
    /// The first column of the row.
    pub id: u32,
    /// `HI_*` large group.
    pub large_group: u32,
    /// `HS_*`/`JOB_*`/`MI_*` small group.
    pub small_group: u32,
    /// The count needed.
    pub need: i32,
    /// The title text.
    pub title: String,
}

/// `honor`: the 58 honor titles.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HonorBundle {
    /// The titles in file order.
    pub titles: Vec<HonorTitleRow>,
}
bundle_data!(HonorBundle, "honor", 1, Boot);

/// One job (`etc.inc` + `propJob.inc`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct JobRow {
    /// `JOB_*`.
    pub id: u32,
    /// The job name.
    pub name: String,
    /// The job this one advances from.
    pub base: u32,
    /// `JTYPE_*`.
    #[serde(rename = "type")]
    pub kind: u32,
    /// The stat factors of `propJob.inc`, `null` when the job has no row.
    pub prop: Option<JobProp>,
}

/// The starting equipment of one job: `equip[slot] = [male, female]`, `NULL_ID` = empty.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct JobItemsRow {
    /// `JOB_*`.
    pub job: u32,
    /// At most 10 `[male, female]` pairs by position.
    pub equip: Vec<[u32; 2]>,
}

/// The skill-point costs of `Constant.inc` per skill kind.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SkillPointCosts {
    /// `JTYPE_BASE` (default 1).
    pub vag_sp: u32,
    /// `JTYPE_EXPERT` (default 2).
    pub expert_sp: u32,
    /// Every other job type (default 3).
    pub pro_sp: u32,
}

/// The experience tables.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ExpSection {
    /// `expCharacter` rows `[exp1, exp2, lp, limit]` (row index = file row).
    pub character: Vec<[i64; 4]>,
    /// `expParty` rows `[exp, point]`.
    pub party: Vec<[u32; 2]>,
    /// `expPartySkill` rows `[exp, level]`.
    pub party_skill: Vec<[u32; 2]>,
    /// `expUpitem` by column (weapon, helmet, suit, gauntlet, boots, shield) then row `+0..=+10`.
    pub upitem: Vec<Vec<i32>>,
    /// The set-item bonus per number of equipped pieces.
    pub set_item_avail: Vec<SetItemAvail>,
}

/// The guild tables.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GuildSection {
    /// `[pxp, penya, max_member]` per guild level.
    pub levels: Vec<[u32; 3]>,
    /// The guild buffs (`guildbuff.inc`).
    pub buffs: Vec<GuildBuffDef>,
}

/// The couple tables.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CoupleSection {
    /// EXP per couple level (divided by the couple rate).
    pub level_exp: Vec<i32>,
    /// The couple skill items.
    pub skill_kinds: Vec<u32>,
    /// `(level, level of each kind)` rows.
    pub skill_levels: Vec<(i32, Vec<i32>)>,
}

/// The PK colours and penalty steps.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PkSection {
    /// `GENERAL_COLOR`.
    pub general_color: u32,
    /// `READY_COLOR`.
    pub ready_color: u32,
    /// `CHAO_COLOR`.
    pub chao_color: u32,
    /// `LIMITE_LEVEL`.
    pub limit_level: i32,
    /// The `ADDPENALTY` steps.
    pub penalties: Vec<PropensityPenalty>,
}

/// One entry of `teleport16.inc` (the 16-level teleporter window).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Teleport16Row {
    /// The destination world.
    pub world: u32,
    /// The destination position.
    pub pos: [f32; 3],
    /// The name.
    pub name: String,
    /// The description.
    pub desc: String,
    /// The preview image (a bare file name).
    pub image: String,
    /// The boss mover (0 = none).
    pub boss: u32,
    /// Minimum level.
    pub min_level: u32,
    /// Maximum level (`max < min` is `MAX_LEGEND_LEVEL` = 129 on the client).
    pub max_level: u32,
    /// The class column.
    pub class: u32,
    /// The cooldown.
    pub cooldown: u32,
    /// The boss drops the window shows (empty until `propMoverEx` is loaded).
    pub boss_drops: Vec<u32>,
}

/// One generic event (`propEvent.inc`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GenericEventRow {
    /// The event id (500+).
    pub id: u32,
    /// The English title.
    pub title: String,
    /// `[year, month, day, hour]` of the begin.
    pub begin: Vec<i32>,
    /// `[year, month, day, hour]` of the end.
    pub end: Vec<i32>,
}

// ---- lord events (doc 45 §28.2, task ui-lord) ---------------------------------------------------------------------------

/// One row of `lordevent.inc` (`m_vEFactor` / `m_vIFactor`): the bonus factor of a lord event and its price.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct LordEventRate {
    /// The bonus (0.05 = 5 %); row 0 is 0.
    pub factor: f32,
    /// The price in penya of this row alone (`ILordEvent::GetCost` adds the exp and the item price); row 0 is 0.
    pub cost: i64,
}

/// `lordevent.inc`: the two rate lists the lord's event window steps through (the original client read the file itself and
/// the `Lord` message carries no table).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct LordEventTables {
    /// The experience rows (`exp { ... }`, 11 here).
    pub exp: Vec<LordEventRate>,
    /// The item drop rows (`item { ... }`, 11 here).
    pub item: Vec<LordEventRate>,
}

/// `tables`: the small rule tables the client needs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TablesBundle {
    /// The jobs.
    pub jobs: Vec<JobRow>,
    /// The starting equipment (`jobItem.inc`; the begin items stay server-only).
    pub job_items: Vec<JobItemsRow>,
    /// The skill-point costs.
    pub constants: SkillPointCosts,
    /// The experience tables.
    pub exp: ExpSection,
    /// The guild tables.
    pub guild: GuildSection,
    /// The couple tables.
    pub couple: CoupleSection,
    /// The PK settings.
    pub pk: PkSection,
    /// The emote icons (`propMotion.txt`).
    pub emotes: Vec<MotionProp>,
    /// `SRT_*` to the structure name.
    pub structures: BTreeMap<u32, String>,
    /// `GUD_*` to the guild rank name.
    pub guild_appellations: BTreeMap<u32, String>,
    /// `teleport16.inc`.
    pub teleports16: Vec<Teleport16Row>,
    /// The lord skills.
    pub lord_skills: Vec<LordSkill>,
    /// The generic event titles and times.
    pub generic_events: Vec<GenericEventRow>,
    // ---- lord (ui-lord, M7) ----
    /// `lordevent.inc`: the rate lists of the lord's event window (older fixtures lack the field).
    #[serde(default)]
    pub lord_events: LordEventTables,
}
bundle_data!(TablesBundle, "tables", 1, Boot);

/// `names`: the chat word filter and the name rules.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct NamesBundle {
    /// `filter_ENG.inc` as written: `[word, replacement]` (the client lower-cases the alphabetic words).
    pub chat_filter: Vec<[String; 2]>,
    /// Banned name substrings, lower-cased.
    pub invalid_substrings: Vec<String>,
    /// The characters a name may use, ascending; `null` = no restriction.
    pub allowed_letters: Option<String>,
    /// The same for vendor titles.
    pub vendor_allowed_letters: Option<String>,
}
bundle_data!(NamesBundle, "names", 1, Boot);

// ---- quests (lazy; empty in this data set) --------------------------------------------------------------------------

/// The goal marker of a quest condition (`QuestGoalData`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct QuestGoalData {
    /// The global running goal counter.
    pub index: u32,
    /// World x.
    pub x: i32,
    /// World z.
    pub z: i32,
    /// `atoi` of the last four characters of `GOAL_TEXT_ID_nnnn`.
    pub text_id: u32,
}

/// An item condition (`QuestItemCond`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct QuestItemCond {
    /// -1 any, 0 male, 1 female.
    pub sex: i32,
    /// 0: `job_or_item` is a job (-1 any); 1: the item the player must own (-1 any).
    #[serde(rename = "type")]
    pub kind: i32,
    /// A job or an item, see `type`.
    pub job_or_item: i32,
    /// The item.
    pub item: u32,
    /// The count.
    pub num: i32,
    /// The ability option.
    pub ability_option: i32,
    /// Flags.
    pub flag: i32,
    /// The goal marker.
    pub goal: Option<QuestGoalData>,
}

/// A party or guild condition (raw directive values).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct QuestPartyCond {
    /// The mode.
    pub mode: i32,
    /// 0 `==`, 1 `>=`, -1 `<=`.
    pub comp: i32,
    /// The number.
    pub num: i32,
    /// Leader only.
    pub leader: i32,
}

/// The previous-quest condition (B1).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct QuestPrevious {
    /// The type.
    #[serde(rename = "type")]
    pub kind: i32,
    /// 6 slots, 0 = unused.
    pub quests: Vec<u32>,
}

/// The begin conditions B1-B15.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct QuestBegin {
    /// B1.
    pub previous: QuestPrevious,
    /// B2: 6 quests.
    pub exclusive: Vec<u32>,
    /// B3: empty = any.
    pub jobs: Vec<u32>,
    /// B4.
    pub level: [i32; 2],
    /// B5.
    pub party: QuestPartyCond,
    /// B6.
    pub guild: QuestPartyCond,
    /// B7: -1 any.
    pub sex: i32,
    /// B8: `[skill, level]`.
    pub skill: [u32; 2],
    /// B9.
    pub pk_value: i32,
    /// B10: items that must not be owned.
    pub not_items: Vec<QuestItemCond>,
    /// B14: items that must be owned.
    pub items: Vec<QuestItemCond>,
    /// B11: 0 none, -1 must not be disguised.
    pub disguise: i32,
    /// B12.
    pub pet_exp: i32,
    /// B12.
    pub pet_level: i32,
    /// B13.
    pub tutorial_state: i32,
    /// B15.
    pub tsp: i32,
    /// `[item, num]` slots of `SetBeginSetAddItem`.
    pub add_items: Vec<[u32; 2]>,
}

/// The patrol end condition (E2).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct QuestPatrol {
    /// The world.
    pub world: u32,
    /// The normalised rectangle.
    pub rect: [i32; 4],
    /// `SetPatrolZoneName`.
    pub name: String,
    /// `PATROL_DESTINATION_ID_nnnn` key into `patrol_destinations`.
    pub destination: Option<u32>,
    /// The goal marker.
    pub goal: Option<QuestGoalData>,
}

/// A kill condition (E3).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct QuestKill {
    /// The mover (0 = unused slot).
    pub mover: u32,
    /// The count.
    pub num: i32,
    /// The goal marker.
    pub goal: Option<QuestGoalData>,
}

/// The "complete these quests" condition (E11).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct QuestCompleteQuests {
    /// 0 none, 1 or, 2 and.
    pub oper: i32,
    /// 6 quests.
    pub quests: Vec<u32>,
}

/// The end character (the NPC that completes the quest).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct QuestEndCharacter {
    /// The NPC key, `""` = never completable through dialogs.
    pub npc: String,
    /// The goal marker.
    pub goal: Option<QuestGoalData>,
}

/// The end dialog condition.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct QuestEndDialog {
    /// The NPC key.
    pub npc: String,
    /// The dialog key (case-sensitive).
    pub key: String,
    /// The goal marker.
    pub goal: Option<QuestGoalData>,
}

/// The end conditions E1-E16.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct QuestEnd {
    /// E1.
    pub limit_time_min: i32,
    /// E2.
    pub patrol: Option<QuestPatrol>,
    /// E3: 2 slots.
    pub kill: Vec<QuestKill>,
    /// E4: `[skill, level]`.
    pub skill: [u32; 2],
    /// E5.
    pub gold: i32,
    /// E6.
    pub level: [i32; 2],
    /// E7: units of 1/100 percent.
    pub exp_percent: [i32; 2],
    /// E8.
    pub pet_exp: i32,
    /// E8.
    pub pet_level: i32,
    /// E9.
    pub disguise: i32,
    /// E10: 0 = the last defined state.
    pub state: i32,
    /// E11.
    pub complete_quest: QuestCompleteQuests,
    /// E12.
    pub party: QuestPartyCond,
    /// E13.
    pub guild: QuestPartyCond,
    /// E14.
    pub one_items: Vec<QuestItemCond>,
    /// E15.
    pub items: Vec<QuestItemCond>,
    /// E16.
    pub tsp: i32,
    /// The completing NPC.
    pub character: QuestEndCharacter,
    /// `SetEndCondMultiCharacter` (display only): `[npc, item]`.
    pub multi_characters: Vec<(String, u32)>,
    /// The end dialog.
    pub dialog: Option<QuestEndDialog>,
}

/// One reward item.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct QuestRewardItem {
    /// -1 any, 0 male, 1 female.
    pub sex: i32,
    /// The type.
    #[serde(rename = "type")]
    pub kind: i32,
    /// A job or an item.
    pub job_or_item: i32,
    /// The item.
    pub item: u32,
    /// The count.
    pub num: i32,
    /// The ability option.
    pub ability_option: i32,
    /// The bind flag.
    pub bind: i32,
}

/// A teleport reward.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct QuestTeleport {
    /// The world.
    pub world: u32,
    /// The position.
    pub pos: [f32; 3],
}

/// The rewards (`null` when hidden by `SetEndRewardHide(1)`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct QuestRewards {
    /// The items.
    pub items: Vec<QuestRewardItem>,
    /// `[min, max]` gold.
    pub gold: [i32; 2],
    /// `[min, max]` exp.
    pub exp: [i64; 2],
    /// Skill points.
    pub skill_point: i32,
    /// `[min, max]` PK value.
    pub pk_value: [i32; 2],
    /// The teleport.
    pub teleport: Option<QuestTeleport>,
    /// Raises the pet level.
    pub pet_levelup: bool,
    /// TSP.
    pub tsp: i32,
}

/// One described state.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct QuestStateDesc {
    /// 0..=15.
    pub state: u32,
    /// The `SetDesc` text.
    pub desc: String,
}

/// The client definition of one quest (14 §10.5.1; empty in this data set).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct QuestClientDef {
    /// The quest id.
    pub id: u32,
    /// `SetTitle`, at most 63 bytes.
    pub title: String,
    /// `SetHeadQuest` after the remap: the quest-log tree.
    pub head_quest: u32,
    /// `SetQuestType` (`QT_*`).
    pub quest_type: i32,
    /// `SetRepeat`.
    pub repeat: bool,
    /// `SetRemove(0)`: the player cannot cancel.
    pub no_remove: bool,
    /// `[npc, item]` givers in file order.
    pub givers: Vec<(String, u32)>,
    /// The begin conditions.
    pub begin: QuestBegin,
    /// The end conditions.
    pub end: QuestEnd,
    /// The rewards, `null` when hidden.
    pub rewards: Option<QuestRewards>,
    /// The described states in ascending order.
    pub states: Vec<QuestStateDesc>,
}

/// `quests` (lazy): the quest definitions, the NPC links and the destination names.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct QuestsBundle {
    /// The quests in ascending id order (empty in this data set).
    pub quests: Vec<QuestClientDef>,
    /// NPC key (lower case) to the quests it gives, file order.
    pub quest_src: BTreeMap<String, Vec<u32>>,
    /// NPC key (lower case) to the quests it completes.
    pub quest_dst: BTreeMap<String, Vec<u32>>,
    /// `QuestDestination.txt.txt`.
    pub quest_destinations: BTreeMap<u32, String>,
    /// `PatrolDestination.txt.txt`.
    pub patrol_destinations: BTreeMap<u32, String>,
}
bundle_data!(QuestsBundle, "quests", 1, Lazy);

// ---- defines (lazy) ------------------------------------------------------------------------------------------------

/// `defines` (lazy): every define in file order, for the dev surfaces and the GM UI.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DefinesBundle {
    /// `[name, value]` pairs.
    pub defines: Vec<(String, i64)>,
}
bundle_data!(DefinesBundle, "defines", 1, Lazy);

// ---- the two projections of binary derived assets (schema owner: doc 24 §10.5-10.6) ------------------------------------

/// One clip row of the `motions` projection.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MotionClipRow {
    /// The `.ani` (source 0) or `.o3d` (source 1) VFS path.
    pub file: String,
    /// 0 `.ani`, 1 built-in `.o3d` animation.
    pub source: u32,
    /// The frame count.
    pub frames: u32,
    /// The raw `perSlerp` f32 of the file.
    pub per_slerp_raw: f32,
    /// The bone count of an `.ani`.
    pub bones: u32,
    /// `[index, frame, flags, sound]` of every attribute with flags.
    pub attrs: Vec<[f64; 4]>,
}

/// One model row of the `motions` projection.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MotionModelRow {
    /// The object type.
    pub ot: u32,
    /// The registry index.
    pub index: u32,
    /// `m_nMax`.
    pub max: u32,
    /// The clip used for an empty slot (-1 = none).
    pub empty: i32,
    /// The clip of the model's own `.o3d` animation (-1 = none).
    pub builtin: i32,
    /// `[mti, clip]` of every non-empty slot (clip -1 = missing).
    pub slots: Vec<[i32; 2]>,
}

/// `motions`: the JSON projection of the binary motion table (built by `flyff-assets`, doc 24 §10.5).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MotionsBundle {
    /// Source-0 rows first (sorted by file), then source-1 rows: row index = `ClipId`.
    pub clips: RowTable<MotionClipRow>,
    /// One row per registry model that can animate, sorted by `(ot, index)`.
    pub models: RowTable<MotionModelRow>,
}
bundle_data!(MotionsBundle, "motions", 1, Boot);

/// One file row of the `model-bounds` projection.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ModelBoundsRow {
    /// The `.o3d` VFS path.
    pub file: String,
    /// The header `bbMin`.
    pub min: [f32; 3],
    /// The header `bbMax`.
    pub max: [f32; 3],
}

/// `model-bounds`: the JSON projection of the binary model bounds (doc 24 §10.6).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ModelBoundsBundle {
    /// One row per parsable `.o3d`.
    pub files: RowTable<ModelBoundsRow>,
}
bundle_data!(ModelBoundsBundle, "model-bounds", 1, Boot);
