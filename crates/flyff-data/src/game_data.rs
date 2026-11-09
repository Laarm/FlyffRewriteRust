//! `GameData`: the immutable data model every server subsystem reads (Plan/14 §6.2).
//!
//! `GameData` is built once by `load_all` and wrapped in `Arc<GameData>`; it is never mutated afterwards.
//! Fields are added by the work package that loads them (WP-DATA-04..16); the ones present now are the
//! texts, items, skills, movers, ctrls, emotes, jobs, the experience tables and the rules tables.
//! `GameData::default()` is an empty data set, convenient for fixtures in downstream tests.

use crate::character::NpcTable;
use crate::dialog_text::DialogText;
use crate::etc::EtcNames;
use crate::events::environment::EnvironmentTable;
use crate::events::generic_event::GenericEvent;
use crate::events::minigame::MiniGameTables;
use crate::events::rangda::RangdaDef;
use crate::events::spevent::XmasEventTable;
use crate::events::transform_item::TransformItemTable;
use crate::exp_table::ExpTables;
use crate::items_ext::accessory::AccessoryTable;
use crate::items_ext::collecting::CollectingTable;
use crate::items_ext::except::ItemExcept;
use crate::items_ext::exchange::ExchangeTable;
use crate::items_ext::giftbox::{GiftBoxTable, PackTable};
use crate::items_ext::item_etc::ItemEtcTables;
use crate::items_ext::pet::PetTables;
use crate::items_ext::random_option::RandomOptionTables;
use crate::items_ext::ticket::TicketTables;
use crate::items_ext::ultimate::UltimateTables;
use crate::job_item::JobItemTable;
use crate::lua_config::LuaConfigs;
use crate::models::ModelRegistry;
use crate::names::NameRules;
use crate::props::ctrl::CtrlProp;
use crate::props::item::{ItemProp, PartySkillProp, SkillProp};
use crate::props::item_index::ItemIndex;
use crate::props::job::JobTable;
use crate::props::motion::MotionProp;
use crate::props::mover::MoverProp;
use crate::props::skill::JobSkills;
use crate::props::skill_add::{AddSkillProp, lookup_add_skill};
use crate::quest::QuestTables;
use crate::rules::constant::ConstantTable;
use crate::rules::die_penalty::DiePenaltyTable;
use crate::rules::enchant::EnchantTable;
use crate::rules::pk_setting::PkSetting;
use crate::social::couple::CoupleTable;
use crate::social::guild::GuildTables;
use crate::social::honor::HonorTable;
use crate::social::lord::LordTables;
use crate::teleport16::Teleport16;
use crate::texts::TextTable;
use crate::types::{AddSkillId, CtrlId, IdTable, MoverId, SkillId};
use crate::worlds::WorldRegistry;
use flyff_common::det::DetMap;
use flyff_script::{Defines, DiagSummary, StringTable};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Bump when a loader changes what it produces for the same files (part of the data fingerprint).
pub const GAMEDATA_LOADER_VERSION: u32 = 2;

/// Where a source file came from (the serialisable form of `flyff_vfs::SourceRecord`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceInfo {
    /// `"resource"`, `"override"`, or `None` when the file does not exist.
    pub layer: Option<String>,
    /// Size in bytes (0 when missing).
    pub size: u64,
    /// Modification time in ns since the Unix epoch (0 when missing).
    pub mtime_ns: i64,
    /// `"utf16le"`, `"utf8"`, `"cp949"`, ... when the file was decoded as text.
    pub encoding: Option<String>,
}

/// The reference counts of the loaded data (Plan/14 §9.1, `flyff check`). A field belongs to the work
/// package that loads the table; fields of tables that are not loaded yet stay 0.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataCounts {
    /// C01: distinct defines.
    pub defines: u32,
    /// C04: string keys.
    pub string_keys: u32,
    /// C05: distinct TIDs defined by `textClient.inc` (2,975: the plan's figure counts entries).
    pub text_entries: u32,
    /// C05: the non-empty texts that are stored (2,965; 10 entries have an empty text).
    pub text_tids: u32,
    /// C06: `propItem` rows stored (`II_CHEERUP` counts twice).
    pub item_rows: u32,
    /// C06: unique item ids.
    pub item_unique_ids: u32,
    /// C06: the highest item id.
    pub item_max_id: u32,
    /// C07: `propSkill` rows.
    pub skill_rows: u32,
    /// C07: `propTroupeSkill` rows.
    pub party_skill_rows: u32,
    /// C08: `propSkillAdd` rows.
    pub add_skill_rows: u32,
    /// C08: the highest `SA_` id.
    pub add_skill_max_id: u32,
    /// C08: empty CSV fields read as 0.
    pub add_skill_empty_fields: u32,
    /// C09: `propMover` rows.
    pub mover_rows: u32,
    /// C09: the highest `MI_` id.
    pub mover_max_id: u32,
    /// C09: `movers.size()` (`m_nMoverPropSize`).
    pub mover_size: u32,
    /// C10: `propCtrl` rows.
    pub ctrl_rows: u32,
    /// C10: `propMotion` rows.
    pub motion_rows: u32,
    /// C10: `propJob` rows.
    pub job_rows: u32,
    /// C19: `item_except.world_drop_excluded`.
    pub except_world_drop_excluded: u32,
    /// C19: `except.txt` patches applied.
    pub except_patches: u32,
    /// Rows read from `expTable.inc` blocks (`expCharacter`).
    pub exp_character_rows: u32,
    /// `expCompanyTest` guild levels.
    pub guild_level_rows: u32,
    /// `jobItem.inc` jobs.
    pub job_item_jobs: u32,
    /// C34: `NumberOverflow` diagnostics.
    pub number_overflow: u32,
    /// C34: `IndexOutOfRange` diagnostics.
    pub index_out_of_range: u32,
    /// C34: `CapacityExceeded` diagnostics.
    pub capacity_exceeded: u32,
    /// C33: client-keyword divergences.
    pub client_variant_divergence: u32,
    /// C11: `propMoverEx.inc` blocks (609).
    pub mover_ex_blocks: u32,
    /// §9.3 check 1: `propMoverEx.inc` blocks of a mover that does not exist.
    pub mover_ex_unknown: u32,
    /// C18: quests / guild quests / party quests after L21, L37 and L38 (0 / 0 / 0 on the shipped data).
    pub quests: [u32; 3],
    /// C29: `MonsterBuffs.inc` pairs (127).
    pub monster_buff_pairs: u32,
    /// C16: `propDropEvent.inc` `DropItem` lines (200).
    pub drop_event_rows: u32,
    /// C13: gift-box blocks by keyword (`GiftBox`, `GiftBox2` .. `GiftBox6`: 142 / 0 / 21 / 7 / 1 / 26).
    pub giftbox_blocks: [u32; 6],
    /// C14: `PackItem` blocks (460).
    pub pack_blocks: u32,
    /// C12: NPC definitions read per character file (`character.inc`, `-etc`, `-school`; 366 / 7 / 30).
    pub npc_rows: [u32; 3],
    /// Distinct NPC keys (`npcs.len()`).
    pub npc_unique: u32,
    /// NPCs that are vendors (`NpcDef::is_vendor`).
    pub npc_vendors: u32,
    /// C24: model entries (`mdlObj.inc` + `mdlDyna.inc`, all types).
    pub model_elems: u32,
    /// C24: motion references (the lines of the motion lists, 17,696).
    pub model_motion_refs: u32,
    /// C17: `World.inc` entries (57).
    pub world_defs: u32,
    /// Worlds with a folder and a `.wld` (the 51 the world server loads).
    pub worlds_loadable: u32,
    /// Distinct world folders with a `.wld` (33).
    pub world_folders: u32,
    /// C22: regions over all folders (`region3`: 190).
    pub regions: u32,
    /// C23: respawns over all folders (`respawn7`: 1,579).
    pub respawns: u32,
    /// C21: `resdata.inc` applets (365).
    pub resdata_applets: u32,
    /// C21: `resdata.inc` controls (2,809).
    pub resdata_controls: u32,
    /// C21: applets with an empty control block (20).
    pub resdata_empty_blocks: u32,
    /// C21: applet or control names that are not defines (4).
    pub resdata_unknown_names: u32,
    /// C26: `sound.inc` entries (1,667).
    pub sound_entries: u32,
    /// C26: distinct sound ids (1,665).
    pub sound_ids: u32,
    /// C26: the highest sound id (3,608).
    pub sound_max_id: u32,
    /// C26: entries with an empty file name (179, the overridden `BGM_NONE` included; 178 silent ids).
    pub sound_empty: u32,
    /// C26: ids whose file does not exist.
    pub sound_missing: u32,
    /// C27: `default.bgm` entries (42).
    pub music_entries: u32,
    /// C27: music entries whose file does not exist (3).
    pub music_missing: u32,
    /// C32: texture atlases of `Theme/`, `Icon/` and `SFX/` (27, 9, 3).
    pub atlases: [u32; 3],
    /// `Terrain.inc` rows.
    pub terrain_rows: u32,
    /// C36: world-map monster records over the maps 1..=7 (78).
    pub map_monster_records: u32,
    /// C36: rainbow race markers (7).
    pub rainbow_markers: u32,
    /// `help.inc` topics.
    pub help_topics: u32,
    /// The text panels of `Client/` (incl. the notice).
    pub panels: u32,
    /// `filter_ENG.inc` word pairs (330).
    pub filter_pairs: u32,
    /// `textEmotion.txt` entries (29).
    pub text_emotions: u32,
    /// `InvalidName_ENG.inc` substrings (195).
    pub invalid_names: u32,
    /// `Letter_ENG.inc` allowed bytes.
    pub allowed_letters: u32,
    /// C29: `honorList.txt` titles (58).
    pub honor_titles: u32,
    /// C29: `lordskill.txt` rows (8).
    pub lord_skills: u32,
    /// C29: `teleport16.inc` rows (17).
    pub teleport16_rows: u32,
    /// C29: `guildbuff.inc` rows (4).
    pub guild_buffs: u32,
    /// C15: `propItemEtc.inc` piercing materials, sets and random-option items (40 / 134 / 48).
    pub item_etc: [u32; 3],
    /// C30: active `propEvent.inc` events (501, 502, 531).
    pub generic_events: u32,
    /// C28: `WorldDialog.txt` lines (1,250).
    pub dialog_lines: u32,
    /// `Exchange_Script.txt` menus.
    pub exchange_menus: u32,
    /// `accessory.inc` accessories.
    pub accessories: u32,
    /// `pet.inc` `AvailDestParam` rows.
    pub pet_kinds: u32,
    /// `randomoption.inc` blocks.
    pub random_option_groups: u32,
    /// `Ultimate_GemAbility.txt` `ABILITY` blocks.
    pub gem_abilities: u32,
    /// `couple.inc` levels.
    pub couple_levels: u32,
    /// C31: `ticket.inc` tickets (5).
    pub tickets: u32,
    /// C31: `layer.inc` lines (2).
    pub ticket_layers: u32,
    /// `MiniGame_Alphabet.txt` words.
    pub minigame_words: u32,
    /// Entries `s.txt` wrote (F10).
    pub s_txt_entries: u32,
    /// L90: the static Lua configuration scripts that ran and were projected (of 7).
    pub lua_static_configs: u32,
    /// L90: the live Lua scripts that compiled and were registered (of 5).
    pub lua_live_scripts: u32,
}

/// Provenance of a load (Plan/14 §6.2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataMeta {
    /// [`GAMEDATA_LOADER_VERSION`].
    pub loader_version: u32,
    /// `xxh3_128` over the sorted source records and the loader version (hex digits via
    /// `format!("{:032x}")`).
    pub fingerprint: u128,
    /// Every file that was looked up, by normalised path.
    pub sources: BTreeMap<String, SourceInfo>,
    /// The counts.
    pub counts: DataCounts,
    /// The diagnostics digest.
    pub diag_summary: DiagSummary,
    /// Load time in ms. Not serialised: it differs between runs, and exports must be byte-identical.
    #[serde(skip)]
    pub load_ms: u32,
}

impl Default for DataMeta {
    fn default() -> Self {
        Self {
            loader_version: GAMEDATA_LOADER_VERSION,
            fingerprint: 0,
            sources: BTreeMap::new(),
            counts: DataCounts::default(),
            diag_summary: DiagSummary {
                counts: BTreeMap::new(),
                first: Vec::new(),
            },
            load_ms: 0,
        }
    }
}

/// The immutable game data (see the module documentation).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GameData {
    /// Provenance, counts and the diagnostics digest.
    pub meta: DataMeta,
    /// L01: the define table. Not serialised.
    #[serde(skip)]
    pub defines: Arc<Defines>,
    /// L02: the `IDS_` string table. Not serialised.
    #[serde(skip)]
    pub strings: Arc<StringTable>,
    /// L03: `textClient.inc`.
    pub texts: TextTable,
    /// L13 (+L32/L39): `propMover.txt`, `movers.size()` = max id + 1.
    pub movers: IdTable<MoverProp>,
    /// `m_mapMVI`: mover display name to id, first wins (GM commands).
    pub mover_names: DetMap<String, MoverId>,
    /// L14: `propItem.txt` (F2, F3, F6).
    pub items: IdTable<ItemProp>,
    /// F3: the per-kind lists, the min/max table and the name map.
    pub item_index: ItemIndex,
    /// L14b: `except.txt`.
    pub item_except: ItemExcept,
    /// L15: `propSkill.txt`.
    pub skills: IdTable<SkillProp>,
    /// F4: the per-job skill lists.
    pub job_skills: JobSkills,
    /// `propSkill` display names, first wins (F2; separate from `item_index.by_name`).
    pub skill_names: DetMap<String, SkillId>,
    /// L16: `propTroupeSkill.txt`.
    pub party_skills: IdTable<PartySkillProp>,
    /// `propTroupeSkill` display names, first wins (kept apart from `skill_names`: `ST_` and `SI_` ids
    /// overlap numerically).
    pub party_skill_names: DetMap<String, SkillId>,
    /// L17 `propJob.inc` + L34 `etc.inc` job block.
    pub jobs: JobTable,
    /// L18: `propCtrl.txt`.
    pub ctrls: IdTable<CtrlProp>,
    /// `m_mapCtrl`: ctrl display name to id, first wins.
    pub ctrl_names: DetMap<String, CtrlId>,
    /// L19: `propSkillAdd.csv`.
    pub add_skills: IdTable<AddSkillProp>,
    /// L22: `expTable.inc` (guild levels are in `guild`).
    pub exp: ExpTables,
    /// L31: `propMotion.txt` (emote/action icons).
    pub emotes: IdTable<MotionProp>,
    /// L33: `jobItem.inc`.
    pub job_items: JobItemTable,
    /// L34: `etc.inc` structure and guild appellation names.
    pub etc: EtcNames,
    /// L35: `propEnchant.inc`.
    pub enchant: EnchantTable,
    /// L36: `Constant.inc`.
    pub constant: ConstantTable,
    /// L42: `DiePenalty.inc`.
    pub die_penalty: DiePenaltyTable,
    /// L43: `PKSetting.inc`.
    pub pk: PkSetting,
    /// L22 `expCompanyTest`, L66 `guildbuff.inc`, L73 `GuildCombat.txt`.
    pub guild: GuildTables,
    /// L10-L11: `mdlObj.inc` and `mdlDyna.inc`.
    pub models: ModelRegistry,
    /// L12 + L80: `World.inc` and the per-folder text data.
    pub worlds: WorldRegistry,
    /// L20: `character.inc`, `character-etc.inc`, `character-school.inc` with the vendor stock.
    pub npcs: NpcTable,
    /// L21, L37, L38: `propQuest*.inc`, `propGuildQuest.inc`, `propPartyQuest.inc` (WP-DATA-15; empty here).
    #[serde(default)]
    pub quests: QuestTables,
    /// L46: `InvalidName_ENG.inc`, `Letter_ENG.inc`, `Letter1_ENG.inc`.
    pub name_rules: NameRules,
    /// L30: `teleport16.inc`.
    pub teleports16: Vec<Teleport16>,
    /// L44: `propItemEtc.inc` (piercing, sets, random-option items).
    pub item_etc: ItemEtcTables,
    /// L40: `propGiftbox.inc` (server-only gift-box contents, cumulative out of 1 000 000).
    pub giftboxes: GiftBoxTable,
    /// L41: `propPackItem.inc` (server-only pack contents).
    pub pack_items: PackTable,
    /// L45: `propEvent.inc` (ids, flags, English titles, times, drop items, factors and item spawns).
    pub generic_events: Vec<GenericEvent>,
    /// L67: `MonsterBuffs.inc` (`__MONSTER_BUFFS`: the item buff a kill gives the killer's party).
    pub monster_buffs: crate::events::monster_buffs::MonsterBuffTable,
    /// L68: `DPSProp.txt` (`__DPS_METER`: the monsters that get a damage meter besides bosses; empty here).
    pub dps_monsters: Vec<u32>,
    /// L48: `pet.inc` (+ the `Pet_*` blocks of `s.txt`, F10).
    pub pet: PetTables,
    /// L49: `collecting.inc` (+ `Collecting_*` of `s.txt`).
    pub collecting: CollectingTable,
    /// L50: `accessory.inc` (+ `Accessory_Probability` of `s.txt`).
    pub accessory: AccessoryTable,
    /// L52: `ticket.inc` and `layer.inc`.
    pub tickets: TicketTables,
    /// L53: the four mini-game scripts.
    pub minigames: MiniGameTables,
    /// L59: `EnvironmentEffect.txt`.
    pub environment: EnvironmentTable,
    /// L61: `randomeventmonster.inc`.
    pub rangda: Vec<RangdaDef>,
    /// L61: `transformitem.inc`.
    pub transform_items: TransformItemTable,
    /// L62: the highest `LEVEL` sequence of `Client/Guide.inc`.
    pub guide_max_seq: u32,
    /// L72: `spevent.txt`.
    pub xmas_event: XmasEventTable,
    /// L51: `randomoption.inc`.
    pub random_option: RandomOptionTables,
    /// L54: `Ultimate_GemAbility.txt` and `Ultimate_UltimateWeapon.txt`.
    pub ultimate: UltimateTables,
    /// L55: `Exchange_Script.txt`.
    pub exchange: ExchangeTable,
    /// L60 `lordevent.inc` / `lordskill.txt`, L75 `election.inc`.
    pub lord: LordTables,
    /// L64: `couple.inc` (F12).
    pub couple: CoupleTable,
    /// L65: `honorList.txt`.
    pub honor: HonorTable,
    /// L70: `WorldDialog.txt`.
    pub dialog_text: DialogText,
    /// L90: the Lua configuration bridge: the typed static configs and the live script sources (WP-DATA-14).
    pub lua: LuaConfigs,
}

impl GameData {
    /// `GetAddSkillProp(sub_define, level)`: the add-skill row of a skill level.
    ///
    /// `idx = sub_define + max(level, 1) - 1`; **KEEP-QUIRK:** when `idx < 5` the index is bumped by 5, so
    /// `add_skill(5, 1)` is row 5 and `add_skill(1, 1)` is row 6 (ref/17 §7.8).
    pub fn add_skill(&self, sub_define: u32, level: u32) -> Option<&AddSkillProp> {
        lookup_add_skill(&self.add_skills, sub_define, level)
    }

    /// The add-skill row with the given `SA_` id.
    pub fn add_skill_by_id(&self, id: AddSkillId) -> Option<&AddSkillProp> {
        self.add_skills.get(id)
    }

    /// `GetItemProp(id)`.
    pub fn item(&self, id: u32) -> Option<&ItemProp> {
        self.items.get(id)
    }

    /// `GetSkillProp(id)`.
    pub fn skill(&self, id: SkillId) -> Option<&SkillProp> {
        self.skills.get(id)
    }

    /// `GetMoverProp(id)`.
    pub fn mover(&self, id: MoverId) -> Option<&MoverProp> {
        self.movers.get(id)
    }

    /// `GetCharacter(key)`: the NPC definition of `key`, case-insensitive.
    pub fn npc(&self, key: &str) -> Option<&crate::character::NpcDef> {
        self.npcs.get(key)
    }
}
