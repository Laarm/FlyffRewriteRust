//! The new-character template and builder (Plan/13 §24, WP-DB-06; ledger PL-034, PL-050).
//!
//! The legacy `BASE_VALUE_TBL` seed (ref/04 §5.4) is replaced by a data file, `new_character.toml`: it is read
//! through the VFS (an operator drops an override into `overrides/`; the original `Resource/` has none) and
//! falls back to the embedded default `assets/new_character.toml`. [`NewCharacterTemplate::resolve`] parses and
//! checks it **at server start**: every define name must resolve, every item must exist and fit its slot, the
//! template world must have a `RI_BEGIN` region. A broken template refuses the start with a message that names
//! the key ([`TemplateError`]).
//!
//! [`build`] turns the template plus a lobby-validated [`CreateCharacterRequest`] into the
//! [`NewCharacterRecord`] that `DbOp::CreateCharacter` stores:
//!
//! 1. identity from the request; `mover_index` 11 / 12; **hair colour computed here** with
//!    `flyff_rules::appearance::default_hair_color` (the client's colour is never trusted); job, face and costume
//!    are not part of the request;
//! 2. position: a random `RI_BEGIN` region of the template world (the region's `vPos`, as the legacy
//!    `CreatePlayer` did);
//! 3. the inventory through the container port ([`super::container_port`]; doc 19's code replaces it): the bag
//!    items are added, then the equipment of the character's sex is added and equipped, with fresh serials from
//!    the [`super::serial::SerialAllocator`] and `endurance` from the item props (**FIX** PL-034: the food items
//!    are really delivered);
//! 4. skills, taskbar (`bag_ref` resolved to the created `obj_id`), pockets (18/42/42 or 6/24/24 as the init-only
//!    keys say), empty quests, buffs, honor, settings, SM timers;
//! 5. the row: template values, `max_level = level`, `equipment_preview` = the item ids of the equipped items at
//!    their `PARTS_*` index (13 §17.3 rule 6), so the new character is listed with its equipment.
//!
//! With an **empty** item table (a server started with `DataMode::Empty` in tests) the item checks are skipped
//! and items get [`ItemInfo::UNKNOWN`]; with an empty world registry the start position falls back to
//! [`MADRIGAL_BEGIN`]. With loaded data nothing falls back: a missing item or region is an error.

use super::container_port::{ContainerError, ContainerPort, ItemInfo};
use crate::input::CreateCharacterRequest;
use flyff_common::UnixTime;
use flyff_common::config::RulesConfig;
use flyff_common::gm::ModeFlags;
use flyff_common::rng::GameRng;
use flyff_data::GameData;
use flyff_db::model::{
    AppletSlot, BankDoc, BuffsDoc, CharacterDocs, CharacterRow, HonorDoc, InventoryDoc,
    ItemBarSlot, NewCharacterRecord, PocketsDoc, QuestsDoc, SettingsDoc, ShortcutRecord, SkillSlot,
    SkillsDoc, SmTimersDoc, TaskbarDoc,
};
use flyff_db::model::{EQUIPMENT_PREVIEW_LEN, MAX_SKILL_JOB};
use flyff_rules::appearance::{
    AppearanceError, default_hair_color, mover_index, validate_appearance,
};
use flyff_rules::pin::{PinError, validate_bank_pin};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The embedded default template (Plan/13 §24, verbatim).
pub const DEFAULT_TEMPLATE: &str = include_str!("../../assets/new_character.toml");

/// The VFS path of an operator's template (`overrides/new_character.toml`).
pub const TEMPLATE_VFS_PATH: &str = "new_character.toml";

/// The one `RI_BEGIN` region of `WdMadrigal.rgn` (ref/02 §7.3): the start position when no world data is
/// loaded at all (tests with an empty `GameData`).
pub const MADRIGAL_BEGIN: [f32; 3] = [6_971.984_4, 100.0, 3_336.884_3];

/// The only `schema_version` this code understands.
pub const TEMPLATE_SCHEMA_VERSION: u32 = 1;

// ---- the request ----------------------------------------------------------------------------------------

/// Why a [`CreateCharacterRequest`] is refused before anything is built.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RequestError {
    /// `slot > 2`.
    #[error("slot {0} is outside 0..=2")]
    Slot(u8),
    /// Sex, hair, head or skin set out of range.
    #[error("appearance: {0}")]
    Appearance(AppearanceError),
    /// The bank PIN is not four digits or is `0000`.
    #[error("bank PIN: {0}")]
    Pin(PinError),
}

impl CreateCharacterRequest {
    /// The structural rules of 12 §9.1 steps 2, 4 and 5 (slot range, appearance, bank PIN through
    /// `flyff_rules::pin::validate_bank_pin`; no own copy of the rules). The name rules and the uniqueness
    /// belong to the lobby and the directory.
    pub fn validate(&self) -> Result<(), RequestError> {
        if self.slot > 2 {
            return Err(RequestError::Slot(self.slot));
        }
        validate_appearance(self.sex, self.hair_mesh, self.head_mesh, self.skin_set)
            .map_err(RequestError::Appearance)?;
        let pin =
            std::str::from_utf8(&self.bank_pin).map_err(|_| RequestError::Pin(PinError::Format))?;
        validate_bank_pin(pin).map_err(RequestError::Pin)?;
        Ok(())
    }
}

// ---- the file --------------------------------------------------------------------------------------------

/// A number or a define name (`WI_WORLD_MADRIGAL`, `APP_SKILL3`, `25`).
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
enum IdRef {
    Num(i64),
    Name(String),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawStats {
    #[serde(rename = "str")]
    strength: i32,
    sta: i32,
    dex: i32,
    int: i32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSkill {
    position: u8,
    skill: IdRef,
    level: u32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBag {
    item: IdRef,
    count: u16,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawInventory {
    #[serde(default)]
    bag: Vec<RawBag>,
    #[serde(default)]
    equip_male: BTreeMap<String, IdRef>,
    #[serde(default)]
    equip_female: BTreeMap<String, IdRef>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawApplet {
    slot: u8,
    kind: String,
    id: IdRef,
    #[serde(default)]
    index: u32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBarItem {
    page: u8,
    slot: u8,
    kind: String,
    #[serde(default)]
    id: Option<IdRef>,
    #[serde(default)]
    bag_ref: Option<usize>,
    #[serde(default)]
    index: u32,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTaskbar {
    #[serde(default)]
    applets: Vec<RawApplet>,
    #[serde(default)]
    items: Vec<RawBarItem>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPockets {
    #[serde(default = "yes")]
    pocket0_available: bool,
}

impl Default for RawPockets {
    fn default() -> Self {
        Self {
            pocket0_available: true,
        }
    }
}

fn yes() -> bool {
    true
}

/// The file as written (Plan/13 §24).
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTemplate {
    schema_version: u32,
    world: IdRef,
    start_region: IdRef,
    layer: i32,
    return_world: IdRef,
    return_pos: [f32; 3],
    level: u16,
    job: IdRef,
    stats: RawStats,
    #[serde(default)]
    stat_points: i32,
    hp: i32,
    mp: i32,
    fp: i32,
    #[serde(default)]
    gold: i64,
    #[serde(default)]
    bank_gold: i64,
    #[serde(default = "minus_one")]
    fuel: i32,
    #[serde(default = "one")]
    scale: f32,
    #[serde(default)]
    skin_set: u8,
    #[serde(default)]
    mode_flags: Vec<IdRef>,
    #[serde(default)]
    skills: Vec<RawSkill>,
    #[serde(default)]
    inventory: RawInventory,
    #[serde(default)]
    taskbar: RawTaskbar,
    #[serde(default)]
    pockets: RawPockets,
}

fn minus_one() -> i32 {
    -1
}

fn one() -> f32 {
    1.0
}

// ---- the resolved template -------------------------------------------------------------------------------

/// One skill the character starts with (`SkillsDoc.slots[position]`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct StartSkill {
    /// `SkillPosition` 0..44.
    pub position: u8,
    /// `ST_*`/`SI_*` skill id.
    pub skill: u32,
    /// Level.
    pub level: u32,
}

/// One bag item of the kit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct StartItem {
    /// `II_*` item id.
    pub item: u32,
    /// Number of pieces (split over stacks above the prop's `pack_max`).
    pub count: u16,
}

/// One equipped item of the kit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct StartEquip {
    /// `PARTS_*` index 0..30.
    pub part: u8,
    /// `II_*` item id.
    pub item: u32,
}

/// A taskbar applet slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct StartApplet {
    /// Applet slot 0..17.
    pub slot: u8,
    /// `SHORTCUT_*` kind.
    pub kind: u32,
    /// Applet or motion id.
    pub id: u32,
    /// `m_dwIndex`.
    pub index: u32,
}

/// What an item-bar shortcut points at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum BarTarget {
    /// The `obj_id` of the bag item `inventory.bag[n]` as created.
    Bag(usize),
    /// A fixed id (a motion, an applet).
    Id(u32),
}

/// An item-bar slot (`page` 0..7, `slot` 0..8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct StartBarItem {
    /// Page.
    pub page: u8,
    /// Slot.
    pub slot: u8,
    /// `SHORTCUT_*` kind.
    pub kind: u32,
    /// The target.
    pub target: BarTarget,
    /// `m_dwIndex`.
    pub index: u32,
}

/// The validated template (Plan/13 §24). Everything is a number: names were resolved by
/// [`NewCharacterTemplate::resolve`].
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NewCharacterTemplate {
    /// Start world (`WI_WORLD_MADRIGAL`).
    pub world: u32,
    /// The region index that marks start positions (`RI_BEGIN`).
    pub start_region: u32,
    /// Layer.
    pub layer: i32,
    /// Return world (KEEP-QUIRK: Madrigal at `(0,0,0)`).
    pub return_world: u32,
    /// Return position.
    pub return_pos: [f32; 3],
    /// Start level.
    pub level: u16,
    /// Start job (`JOB_VAGRANT`).
    pub job: u8,
    /// `[str, sta, dex, int]`.
    pub stats: [i32; 4],
    /// Unspent stat points.
    pub stat_points: i32,
    /// Start HP.
    pub hp: i32,
    /// Start MP.
    pub mp: i32,
    /// Start FP.
    pub fp: i32,
    /// Start penya.
    pub gold: i64,
    /// `fuel` (-1 = none).
    pub fuel: i32,
    /// Model scale.
    pub scale: f32,
    /// `SKINSET_01`.
    pub skin_set: u8,
    /// Persisted `m_dwMode` (`FRESH_MODE | EQUIP_DENIAL_MODE` = 0x10020000).
    pub mode_flags: u32,
    /// Skills.
    pub skills: Vec<StartSkill>,
    /// Bag items, added in this order.
    pub bag: Vec<StartItem>,
    /// Equipment of a male character, ascending part index.
    pub equip_male: Vec<StartEquip>,
    /// Equipment of a female character, ascending part index.
    pub equip_female: Vec<StartEquip>,
    /// Applets.
    pub applets: Vec<StartApplet>,
    /// Item-bar slots.
    pub bar: Vec<StartBarItem>,
    /// Pocket 0 available (pockets 1-2 follow `features.perm_extrabags`).
    pub pocket0_available: bool,
}

/// Why a template cannot be used. Every message names the key; the server refuses to start.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TemplateError {
    /// The TOML does not parse or does not fit the schema (the text carries the line).
    #[error("new_character.toml: {0}")]
    Syntax(String),
    /// `schema_version` is not 1.
    #[error("new_character.toml: schema_version {0} is not supported (expected 1)")]
    Version(u32),
    /// A define name that neither the loaded define table nor the legacy headers know.
    #[error("new_character.toml: {key}: unknown define '{name}'")]
    UnknownDefine {
        /// Where.
        key: String,
        /// The name.
        name: String,
    },
    /// A value outside its range or inconsistent with the game data.
    #[error("new_character.toml: {key}: {why}")]
    Invalid {
        /// Where.
        key: String,
        /// What is wrong.
        why: String,
    },
}

fn invalid(key: impl Into<String>, why: impl Into<String>) -> TemplateError {
    TemplateError::Invalid {
        key: key.into(),
        why: why.into(),
    }
}

/// The `m_dwMode` bits by their legacy names (`src:_Common/authorization.h:20-60`).
pub fn mode_flag_by_name(name: &str) -> Option<ModeFlags> {
    Some(match name {
        "MATCHLESS_MODE" => ModeFlags::MATCHLESS,
        "TRANSPARENT_MODE" => ModeFlags::TRANSPARENT,
        "ONEKILL_MODE" => ModeFlags::ONEKILL,
        "DONMOVE_MODE" => ModeFlags::DONMOVE,
        "SAYTALK_MODE" => ModeFlags::SAYTALK,
        "MATCHLESS2_MODE" => ModeFlags::MATCHLESS2,
        "NO_ATTACK_MODE" => ModeFlags::NO_ATTACK,
        "ITEM_MODE" => ModeFlags::ITEM,
        "COMMUNITY_MODE" => ModeFlags::COMMUNITY,
        "TALK_MODE" => ModeFlags::TALK,
        "SHOUTTALK_MODE" => ModeFlags::SHOUTTALK,
        "RECOVERCHAO_MODE" => ModeFlags::RECOVERCHAO,
        "FREEPK_MODE" => ModeFlags::FREEPK,
        "PVPCONFIRM_MODE" => ModeFlags::PVPCONFIRM,
        "QUERYSETPLAYERNAME_MODE" => ModeFlags::QUERYSETPLAYERNAME,
        "MODE_MAILBOX" => ModeFlags::MAILBOX,
        "EVENT_OLDBOY_MODE" => ModeFlags::EVENT_OLDBOY,
        "EQUIP_DENIAL_MODE" => ModeFlags::EQUIP_DENIAL,
        "MODE_EXPUP_STOP" => ModeFlags::EXPUP_STOP,
        "FRESH_MODE" => ModeFlags::FRESH,
        "NOTFRESH_MODE" => ModeFlags::NOTFRESH,
        "NOTFRESH_MODE2" => ModeFlags::NOTFRESH2,
        "DONTALK_MODE" => ModeFlags::DONTALK,
        _ => return None,
    })
}

/// The `SHORTCUT_*` kinds of the taskbar (`ref/22 §11`; the numbers of [`ShortcutRecord::kind`]).
pub fn shortcut_kind_by_name(name: &str) -> Option<u32> {
    Some(match name {
        "NONE" => 0,
        "ETC" => 1,
        "APPLET" => 2,
        "MOTION" => 3,
        "SCRIPT" => 4,
        "ITEM" => 5,
        "SKILL" => 6,
        "OBJECT" => 7,
        "CHAT" => 8,
        "SKILLFUN" => 9,
        "EMOTICON" => 10,
        "LORDSKILL" => 11,
        _ => return None,
    })
}

const SHORTCUT_ITEM: u32 = 5;
const MAX_APPLET_SLOT: u8 = 17;
const MAX_BAR_PAGE: u8 = 7;
const MAX_BAR_SLOT: u8 = 8;

/// Name resolution: the loaded define table first, the compiled-in legacy headers second.
struct Defs<'a> {
    gd: &'a GameData,
}

impl Defs<'_> {
    fn get(&self, name: &str) -> Option<i64> {
        self.gd
            .defines
            .value_u32(name)
            .map(i64::from)
            .or_else(|| flyff_common::defs::lookup(name))
    }

    fn id(&self, key: &str, r: &IdRef) -> Result<i64, TemplateError> {
        match r {
            IdRef::Num(n) => Ok(*n),
            IdRef::Name(name) => self.get(name).ok_or_else(|| TemplateError::UnknownDefine {
                key: key.to_owned(),
                name: name.clone(),
            }),
        }
    }

    fn id_u32(&self, key: &str, r: &IdRef) -> Result<u32, TemplateError> {
        let v = self.id(key, r)?;
        u32::try_from(v).map_err(|_| invalid(key, format!("{v} is not an unsigned 32-bit id")))
    }
}

/// The item facts the builder and the checks need.
fn item_info(gd: &GameData, item: u32) -> ItemInfo {
    match gd.items.get(item) {
        Some(p) => ItemInfo {
            pack_max: u16::try_from(p.pack_max.clamp(1, u32::from(u16::MAX))).unwrap_or(1),
            // `=` in the data (0 or NULL_ID) means "no endurance": -1, as in 13 §6.13
            endurance: if p.endurance == 0 || p.endurance == u32::MAX {
                -1
            } else {
                i32::try_from(p.endurance).unwrap_or(i32::MAX)
            },
        },
        None => ItemInfo::UNKNOWN,
    }
}

/// `true` when the item table has content (item checks apply).
fn items_loaded(gd: &GameData) -> bool {
    !gd.items.is_empty()
}

/// The `RI_BEGIN` positions of `world` in file order.
fn begin_positions(gd: &GameData, world: u32, region: u32) -> Vec<[f32; 3]> {
    gd.worlds
        .folder_data(world)
        .map(|f| {
            f.regions
                .iter()
                .filter(|r| r.index == region)
                .map(|r| r.pos)
                .collect()
        })
        .unwrap_or_default()
}

/// `true` when world data is loaded at all.
fn worlds_loaded(gd: &GameData) -> bool {
    !gd.worlds.worlds.is_empty()
}

impl NewCharacterTemplate {
    /// Reads the template: the VFS file `new_character.toml` when it exists (an operator override), else the
    /// embedded default; parses and validates it against `gd`.
    pub fn load(vfs: Option<&flyff_vfs::Vfs>, gd: &GameData) -> Result<Self, TemplateError> {
        if let Some(vfs) = vfs
            && vfs.exists(TEMPLATE_VFS_PATH)
        {
            let bytes = vfs
                .read(TEMPLATE_VFS_PATH)
                .map_err(|e| TemplateError::Syntax(format!("cannot read the override: {e}")))?;
            let text = String::from_utf8(bytes)
                .map_err(|_| TemplateError::Syntax("the override is not valid UTF-8".to_owned()))?;
            tracing::info!(
                "new-character template: using the override of overrides/new_character.toml"
            );
            return Self::resolve(&text, gd);
        }
        Self::resolve(DEFAULT_TEMPLATE, gd)
    }

    /// Parses `text` and checks every name, range and item against `gd`.
    pub fn resolve(text: &str, gd: &GameData) -> Result<Self, TemplateError> {
        let raw: RawTemplate =
            toml::from_str(text).map_err(|e| TemplateError::Syntax(e.to_string()))?;
        if raw.schema_version != TEMPLATE_SCHEMA_VERSION {
            return Err(TemplateError::Version(raw.schema_version));
        }
        let defs = Defs { gd };
        let world = defs.id_u32("world", &raw.world)?;
        let start_region = defs.id_u32("start_region", &raw.start_region)?;
        let return_world = defs.id_u32("return_world", &raw.return_world)?;
        let job = u8::try_from(defs.id("job", &raw.job)?)
            .map_err(|_| invalid("job", "does not fit a job id"))?;
        if raw.layer < 0 {
            return Err(invalid("layer", "must not be negative"));
        }
        if raw.level == 0 {
            return Err(invalid("level", "must be at least 1"));
        }
        if raw.hp <= 0 || raw.mp < 0 || raw.fp < 0 {
            return Err(invalid(
                "hp/mp/fp",
                "hp must be positive, mp and fp not negative",
            ));
        }
        if raw.bank_gold != 0 {
            return Err(invalid(
                "bank_gold",
                "a new bank starts empty (CreateCharacter writes banks.gold = 0)",
            ));
        }
        if raw.gold < 0 || raw.gold > i64::from(i32::MAX) {
            return Err(invalid("gold", "must be 0..=2147483647"));
        }
        let mut mode = ModeFlags::NONE;
        for (i, m) in raw.mode_flags.iter().enumerate() {
            let key = format!("mode_flags[{i}]");
            match m {
                IdRef::Num(n) => {
                    mode |= ModeFlags::from_bits(
                        u32::try_from(*n).map_err(|_| invalid(&key, "not a 32-bit mask"))?,
                    );
                }
                IdRef::Name(name) => {
                    mode |=
                        mode_flag_by_name(name).ok_or_else(|| TemplateError::UnknownDefine {
                            key,
                            name: name.clone(),
                        })?;
                }
            }
        }

        // the world must offer a start position
        if worlds_loaded(gd) && begin_positions(gd, world, start_region).is_empty() {
            return Err(invalid(
                "start_region",
                format!(
                    "world {world} has no region with index {start_region} (RI_BEGIN) in its .rgn"
                ),
            ));
        }

        let mut skills = Vec::new();
        for (i, s) in raw.skills.iter().enumerate() {
            let key = format!("skills[{i}]");
            if usize::from(s.position) >= MAX_SKILL_JOB {
                return Err(invalid(
                    &key,
                    format!("position {} is not below {MAX_SKILL_JOB}", s.position),
                ));
            }
            if skills.iter().any(|k: &StartSkill| k.position == s.position) {
                return Err(invalid(
                    &key,
                    format!("position {} is used twice", s.position),
                ));
            }
            let skill = defs.id_u32(&format!("{key}.skill"), &s.skill)?;
            if skill == 0 {
                return Err(invalid(&key, "skill id 0 is not a skill"));
            }
            skills.push(StartSkill {
                position: s.position,
                skill,
                level: s.level,
            });
        }

        let check_items = items_loaded(gd);
        let mut bag = Vec::new();
        for (i, b) in raw.inventory.bag.iter().enumerate() {
            let key = format!("inventory.bag[{i}]");
            let item = defs.id_u32(&format!("{key}.item"), &b.item)?;
            if b.count == 0 || b.count > flyff_db::model::MAX_ITEM_COUNT {
                return Err(invalid(
                    &key,
                    format!(
                        "count {} is not 1..={}",
                        b.count,
                        flyff_db::model::MAX_ITEM_COUNT
                    ),
                ));
            }
            if check_items && !gd.items.contains(item) {
                return Err(invalid(&key, format!("item {item} is not in propItem.txt")));
            }
            bag.push(StartItem {
                item,
                count: b.count,
            });
        }
        let equip_male = resolve_equip(
            &defs,
            gd,
            "inventory.equip_male",
            &raw.inventory.equip_male,
            0,
        )?;
        let equip_female = resolve_equip(
            &defs,
            gd,
            "inventory.equip_female",
            &raw.inventory.equip_female,
            1,
        )?;

        let mut applets = Vec::new();
        for (i, a) in raw.taskbar.applets.iter().enumerate() {
            let key = format!("taskbar.applets[{i}]");
            if a.slot > MAX_APPLET_SLOT {
                return Err(invalid(
                    &key,
                    format!("slot {} is above {MAX_APPLET_SLOT}", a.slot),
                ));
            }
            if applets.iter().any(|k: &StartApplet| k.slot == a.slot) {
                return Err(invalid(&key, format!("slot {} is used twice", a.slot)));
            }
            let kind = shortcut_kind_by_name(&a.kind).ok_or_else(|| {
                invalid(
                    format!("{key}.kind"),
                    format!("'{}' is not a shortcut kind", a.kind),
                )
            })?;
            applets.push(StartApplet {
                slot: a.slot,
                kind,
                id: defs.id_u32(&format!("{key}.id"), &a.id)?,
                index: a.index,
            });
        }
        let mut bar: Vec<StartBarItem> = Vec::new();
        for (i, b) in raw.taskbar.items.iter().enumerate() {
            let key = format!("taskbar.items[{i}]");
            if b.page > MAX_BAR_PAGE || b.slot > MAX_BAR_SLOT {
                return Err(invalid(&key, "page is 0..=7 and slot 0..=8"));
            }
            if bar.iter().any(|k| k.page == b.page && k.slot == b.slot) {
                return Err(invalid(&key, "page and slot are used twice"));
            }
            let kind = shortcut_kind_by_name(&b.kind).ok_or_else(|| {
                invalid(
                    format!("{key}.kind"),
                    format!("'{}' is not a shortcut kind", b.kind),
                )
            })?;
            let target = match (&b.bag_ref, &b.id) {
                (Some(n), None) => {
                    if kind != SHORTCUT_ITEM {
                        return Err(invalid(&key, "bag_ref is only for kind ITEM"));
                    }
                    if *n >= bag.len() {
                        return Err(invalid(
                            &key,
                            format!(
                                "bag_ref {n} is outside inventory.bag ({} entries)",
                                bag.len()
                            ),
                        ));
                    }
                    BarTarget::Bag(*n)
                }
                (None, Some(id)) => BarTarget::Id(defs.id_u32(&format!("{key}.id"), id)?),
                _ => return Err(invalid(&key, "give exactly one of id and bag_ref")),
            };
            bar.push(StartBarItem {
                page: b.page,
                slot: b.slot,
                kind,
                target,
                index: b.index,
            });
        }

        Ok(NewCharacterTemplate {
            world,
            start_region,
            layer: raw.layer,
            return_world,
            return_pos: raw.return_pos,
            level: raw.level,
            job,
            stats: [
                raw.stats.strength,
                raw.stats.sta,
                raw.stats.dex,
                raw.stats.int,
            ],
            stat_points: raw.stat_points,
            hp: raw.hp,
            mp: raw.mp,
            fp: raw.fp,
            gold: raw.gold,
            fuel: raw.fuel,
            scale: raw.scale,
            skin_set: raw.skin_set,
            mode_flags: mode.bits(),
            skills,
            bag,
            equip_male,
            equip_female,
            applets,
            bar,
            pocket0_available: raw.pockets.pocket0_available,
        })
    }
}

/// Resolves one `equip_*` table: part name -> item; checks the item against its slot and sex; ascending part
/// order (TOML tables are unordered, the plan's order is ascending part index).
fn resolve_equip(
    defs: &Defs<'_>,
    gd: &GameData,
    table: &str,
    raw: &BTreeMap<String, IdRef>,
    sex: u32,
) -> Result<Vec<StartEquip>, TemplateError> {
    let mut out: Vec<StartEquip> = Vec::new();
    for (part_name, item_ref) in raw {
        let key = format!("{table}.{part_name}");
        let part = defs
            .get(part_name)
            .and_then(|v| u8::try_from(v).ok())
            .filter(|p| usize::from(*p) < EQUIPMENT_PREVIEW_LEN)
            .ok_or_else(|| {
                if defs.get(part_name).is_none() {
                    TemplateError::UnknownDefine {
                        key: key.clone(),
                        name: part_name.clone(),
                    }
                } else {
                    invalid(&key, "not a PARTS_* index (0..=30)")
                }
            })?;
        let item = defs.id_u32(&key, item_ref)?;
        if items_loaded(gd) {
            let Some(p) = gd.items.get(item) else {
                return Err(invalid(&key, format!("item {item} is not in propItem.txt")));
            };
            if p.parts != u32::from(part) && p.partsub != u32::from(part) {
                return Err(invalid(
                    &key,
                    format!(
                        "item {item} ({}) is worn on part {} (sub {}), not on part {part}",
                        p.name, p.parts, p.partsub
                    ),
                ));
            }
            if p.item_sex <= 1 && p.item_sex != sex {
                return Err(invalid(
                    &key,
                    format!("item {item} ({}) is for the other sex", p.name),
                ));
            }
        }
        out.push(StartEquip { part, item });
    }
    out.sort_by_key(|e| e.part);
    if let Some(w) = out.windows(2).find(|w| w[0].part == w[1].part) {
        return Err(invalid(table, format!("part {} appears twice", w[0].part)));
    }
    Ok(out)
}

// ---- the builder -----------------------------------------------------------------------------------------

/// Capacities and switches the document layout depends on (the init-only `features.*` keys, 13 §6.3/§6.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildOptions {
    /// Inventory capacity: 199 (`inventory_168`) or 73 (vanilla 42 + 31).
    pub inventory_capacity: u16,
    /// Pocket capacities: 18/42/42 (`bigger_pocket`) or 6/24/24.
    pub pocket_capacities: [u16; 3],
    /// `features.perm_extrabags`: pockets 1 and 2 are available.
    pub perm_extrabags: bool,
}

impl BuildOptions {
    /// From the rules configuration.
    pub fn from_rules(rules: &RulesConfig) -> Self {
        let f = &rules.features;
        Self {
            inventory_capacity: if f.inventory_168 {
                InventoryDoc::CAPACITY_168
            } else {
                42 + flyff_db::model::EQUIP_SLOTS
            },
            pocket_capacities: if f.bigger_pocket {
                PocketsDoc::CAPACITIES_BIG
            } else {
                [6, 24, 24]
            },
            perm_extrabags: f.perm_extrabags,
        }
    }
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self::from_rules(&RulesConfig::default())
    }
}

/// Why a character could not be built.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum BuildError {
    /// The request is malformed (the lobby should have refused it).
    #[error("invalid request: {0}")]
    Request(#[from] RequestError),
    /// An item of the kit is not in the game data.
    #[error("item {0} of the starter kit is not in the game data")]
    UnknownItem(u32),
    /// The template world has no start region.
    #[error("the start world has no RI_BEGIN region")]
    NoStartRegion,
    /// The container port refused (bag too small for the kit, ...).
    #[error("starter kit: {0}")]
    Container(#[from] ContainerError),
}

/// What `build` needs besides the template, the data and the request.
#[derive(Clone, Copy, Debug)]
pub struct BuildContext {
    /// The owning account (the request carries no account: it comes from `GameInput::CreateCharacter`).
    pub account: flyff_common::AccountId,
    /// The wall time of the tick (`created_at`).
    pub now: UnixTime,
    /// Document capacities and pocket availability.
    pub options: BuildOptions,
}

/// `NewCharacterRecord` for `req` (Plan/13 §24 steps 1-5). `serial` yields fresh item serials (the game passes
/// the [`super::serial::SerialAllocator`]), `rng` is the world stream of the start world.
pub fn build(
    t: &NewCharacterTemplate,
    gd: &GameData,
    req: &CreateCharacterRequest,
    cx: &BuildContext,
    serial: &mut dyn FnMut() -> Result<u64, String>,
    rng: &mut dyn GameRng,
) -> Result<NewCharacterRecord, BuildError> {
    let (opts, account, now) = (&cx.options, cx.account, cx.now);
    req.validate()?;
    // identity and appearance (the client's colour and job are never used)
    let hair_color = default_hair_color(req.sex, req.hair_mesh)
        .ok_or(RequestError::Appearance(AppearanceError::HairMesh))?;
    let mover = mover_index(req.sex).ok_or(RequestError::Appearance(AppearanceError::Sex))?;

    // position
    let starts = begin_positions(gd, t.world, t.start_region);
    let pos = if starts.is_empty() {
        if worlds_loaded(gd) {
            return Err(BuildError::NoStartRegion);
        }
        MADRIGAL_BEGIN
    } else {
        let pick =
            usize::try_from(rng.random_below(u32::try_from(starts.len()).unwrap_or(u32::MAX)))
                .unwrap_or(0);
        starts.get(pick).copied().unwrap_or(MADRIGAL_BEGIN)
    };

    // inventory: bag first, then the equipment of this sex
    let mut inventory = InventoryDoc::empty(opts.inventory_capacity);
    let bag_slots = usize::from(InventoryDoc::bag_slots(opts.inventory_capacity));
    let mut bag_obj_ids: Vec<u16> = Vec::new();
    let mut equipment_preview = [0_u32; EQUIPMENT_PREVIEW_LEN];
    {
        let mut port = ContainerPort::new(&mut inventory.container, bag_slots);
        for b in &t.bag {
            let info = known_info(gd, b.item)?;
            let touched = port.add(b.item, b.count, info, serial)?;
            bag_obj_ids.push(touched.first().copied().unwrap_or(0));
        }
        let equip = if req.sex == 0 {
            &t.equip_male
        } else {
            &t.equip_female
        };
        for e in equip {
            let info = known_info(gd, e.item)?;
            let touched = port.add(e.item, 1, info, serial)?;
            let obj_id = touched.first().copied().unwrap_or(0);
            port.equip(obj_id, usize::from(e.part))?;
            if let Some(slot) = equipment_preview.get_mut(usize::from(e.part)) {
                *slot = e.item;
            }
        }
    }

    // pockets: pocket 0 per template, 1 and 2 per perm_extrabags (an unavailable pocket is "expired", as the
    // legacy creation left them)
    let mut pockets = PocketsDoc::empty(opts.pocket_capacities);
    for (i, p) in pockets.pockets.iter_mut().enumerate() {
        let available = if i == 0 {
            t.pocket0_available
        } else {
            opts.perm_extrabags
        };
        p.available = available;
        p.expired = !available;
    }

    // skills
    let mut skills = SkillsDoc::default();
    for s in &t.skills {
        if let Some(slot) = skills.slots.get_mut(usize::from(s.position)) {
            *slot = Some(SkillSlot {
                skill_id: s.skill,
                level: s.level,
            });
        }
    }

    // taskbar
    let mut taskbar = TaskbarDoc::default();
    for a in &t.applets {
        taskbar.applets.push(AppletSlot {
            slot: a.slot,
            shortcut: ShortcutRecord {
                kind: a.kind,
                id: a.id,
                index: a.index,
                ..ShortcutRecord::default()
            },
        });
    }
    for b in &t.bar {
        let id = match b.target {
            BarTarget::Id(id) => id,
            BarTarget::Bag(n) => u32::from(bag_obj_ids.get(n).copied().unwrap_or(0)),
        };
        taskbar.items.push(ItemBarSlot {
            page: b.page,
            slot: b.slot,
            shortcut: ShortcutRecord {
                kind: b.kind,
                id,
                index: b.index,
                // item-bar rows carry data = 1 (ref/04 §5.4 seed `0,0,5,45,0,0,0,1`)
                data: 1,
                ..ShortcutRecord::default()
            },
        });
    }

    let row = CharacterRow {
        sex: req.sex,
        mover_index: mover,
        skin_set: req.skin_set,
        hair_mesh: req.hair_mesh,
        hair_color,
        head_mesh: req.head_mesh,
        scale: t.scale,
        world_id: t.world,
        layer: t.layer,
        pos,
        return_world_id: t.return_world,
        return_pos: t.return_pos,
        hp: t.hp,
        mp: t.mp,
        fp: t.fp,
        job: t.job,
        level: t.level,
        max_level: t.level,
        stat_str: t.stats[0],
        stat_sta: t.stats[1],
        stat_dex: t.stats[2],
        stat_int: t.stats[3],
        stat_points: t.stat_points,
        gold: t.gold,
        fuel: t.fuel,
        mode_flags: t.mode_flags,
        honor_title_id: None,
        summoned_pet_obj_id: None,
        equipment_preview,
        ..CharacterRow::default()
    };

    Ok(NewCharacterRecord {
        account_id: account,
        slot: req.slot,
        name: req.name.clone(),
        created_at: now,
        row,
        docs: CharacterDocs {
            inventory,
            pockets,
            skills,
            quests: QuestsDoc::default(),
            taskbar,
            buffs: BuffsDoc::default(),
            sm_timers: SmTimersDoc::default(),
            honor: HonorDoc::default(),
            settings: SettingsDoc::default(),
        },
        bank_pin: validate_bank_pin(std::str::from_utf8(&req.bank_pin).unwrap_or(""))
            .map_err(RequestError::Pin)?,
        bank: BankDoc::empty(),
    })
}

fn known_info(gd: &GameData, item: u32) -> Result<ItemInfo, BuildError> {
    if items_loaded(gd) && !gd.items.contains(item) {
        return Err(BuildError::UnknownItem(item));
    }
    Ok(item_info(gd, item))
}
