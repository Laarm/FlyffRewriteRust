//! The per-character JSON documents (Plan/13 §6.4-§6.12).
//!
//! Every document is an object with `schema_version` as its first field (current version 1 for all) and
//! implements [`JsonDoc`]. Nested fields are always written; only [`ItemRecord`](super::ItemRecord) omits
//! defaults. Field names are part of the contract: operators query them with `json_extract`.

use super::container::{ContainerRecord, EQUIP_SLOTS, RepairNote};
use super::doc::{JsonDoc, v1};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

macro_rules! impl_json_doc {
    ($ty:ty, $kind:literal) => {
        impl JsonDoc for $ty {
            const KIND: &'static str = $kind;
            const SCHEMA_VERSION: u32 = 1;
        }
    };
}

// ---- containers: inventory, bank, guild bank (§6.4) ----------------------------------------------------

/// The inventory: capacity 199 = 168 bag slots + 31 equipment slots (`features.inventory_168`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct InventoryDoc {
    /// Document version.
    #[serde(default = "v1")]
    pub schema_version: u32,
    /// The container.
    pub container: ContainerRecord,
}

impl Default for InventoryDoc {
    fn default() -> Self {
        Self {
            schema_version: Self::SCHEMA_VERSION,
            container: ContainerRecord::default(),
        }
    }
}

impl_json_doc!(InventoryDoc, "inventory");

impl InventoryDoc {
    /// Capacity with 168 bag slots (`features.inventory_168 = true`).
    pub const CAPACITY_168: u16 = 168 + EQUIP_SLOTS;

    /// An empty inventory of `capacity` entries; the bag is `capacity - 31` slots.
    pub fn empty(capacity: u16) -> Self {
        Self {
            schema_version: Self::SCHEMA_VERSION,
            container: ContainerRecord::empty(capacity, capacity.saturating_sub(EQUIP_SLOTS)),
        }
    }

    /// Number of bag slots of an inventory with `capacity` entries.
    pub fn bag_slots(capacity: u16) -> u16 {
        capacity.saturating_sub(EQUIP_SLOTS)
    }

    /// [`ContainerRecord::validate_and_repair_with`] with the inventory's bag size.
    pub fn validate_and_repair(&mut self, expected_capacity: u16) -> Vec<RepairNote> {
        self.container
            .validate_and_repair_with(expected_capacity, Self::bag_slots(expected_capacity))
    }
}

macro_rules! container_doc {
    ($(#[$meta:meta])* $name:ident, $kind:literal, $capacity:expr) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(default)]
        pub struct $name {
            /// Document version.
            #[serde(default = "v1")]
            pub schema_version: u32,
            /// The container.
            pub container: ContainerRecord,
        }

        impl Default for $name {
            fn default() -> Self {
                Self {
                    schema_version: Self::SCHEMA_VERSION,
                    container: ContainerRecord::default(),
                }
            }
        }

        impl_json_doc!($name, $kind);

        impl $name {
            /// The container size of this document kind.
            pub const CAPACITY: u16 = $capacity;

            /// An empty container of [`Self::CAPACITY`] bag slots.
            pub fn empty() -> Self {
                Self {
                    schema_version: Self::SCHEMA_VERSION,
                    container: ContainerRecord::empty(Self::CAPACITY, Self::CAPACITY),
                }
            }

            /// [`ContainerRecord::validate_and_repair`] for [`Self::CAPACITY`].
            pub fn validate_and_repair(&mut self) -> Vec<RepairNote> {
                self.container.validate_and_repair(Self::CAPACITY)
            }
        }
    };
}

container_doc!(
    /// One 42-slot bank (`banks.container`; gold and PIN are columns).
    BankDoc,
    "bank",
    42
);
container_doc!(
    /// The 42-slot guild bank (`guild_banks.container`; the penya pool is `guilds.gold`).
    GuildBankDoc,
    "guild_bank",
    42
);

// ---- pockets (§6.5) ------------------------------------------------------------------------------------

/// One of the three pockets.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PocketRecord {
    /// Pocket usable (legacy: loaded if `szItem != '$' || !bExpired`).
    pub available: bool,
    /// `bExpired`.
    pub expired: bool,
    /// `tExpirationDate` (absolute unix seconds), 0 = none.
    pub expires_at: i64,
    /// The pocket's container: capacity 18 / 42 / 42 (`__BIGGER_POCKET`).
    pub container: ContainerRecord,
}

/// `character_docs.pockets`: the three pockets (`PocketRecord` order = pocket number 0..2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PocketsDoc {
    /// Document version.
    #[serde(default = "v1")]
    pub schema_version: u32,
    /// Pocket 0..=2.
    pub pockets: [PocketRecord; 3],
}

impl Default for PocketsDoc {
    fn default() -> Self {
        Self {
            schema_version: Self::SCHEMA_VERSION,
            pockets: Default::default(),
        }
    }
}

impl_json_doc!(PocketsDoc, "pockets");

impl PocketsDoc {
    /// Pocket capacities with `features.bigger_pocket = true`.
    pub const CAPACITIES_BIG: [u16; 3] = [18, 42, 42];

    /// Three empty, available pockets with the given capacities (`perm_extrabags` repack default).
    pub fn empty(capacities: [u16; 3]) -> Self {
        Self {
            schema_version: Self::SCHEMA_VERSION,
            pockets: capacities.map(|c| PocketRecord {
                available: true,
                expired: false,
                expires_at: 0,
                container: ContainerRecord::empty(c, c),
            }),
        }
    }

    /// Validates every pocket against its expected capacity; the notes of all pockets are concatenated
    /// in pocket order.
    pub fn validate_and_repair(&mut self, capacities: [u16; 3]) -> Vec<RepairNote> {
        let mut notes = Vec::new();
        for (pocket, capacity) in self.pockets.iter_mut().zip(capacities) {
            notes.extend(pocket.container.validate_and_repair(capacity));
        }
        notes
    }
}

// ---- skills (§6.6) -------------------------------------------------------------------------------------

/// `MAX_SKILL_JOB`: 3 vagrant + 20 + 20 + 1 + 1 skill positions.
pub const MAX_SKILL_JOB: usize = 45;

/// One learned skill.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillSlot {
    /// Skill id.
    pub skill_id: u32,
    /// Skill level.
    pub level: u32,
}

/// `character_docs.skills`: `slots[SkillPosition]` = `m_aJobSkill[position]`, `null` = unlearned.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SkillsDoc {
    /// Document version.
    #[serde(default = "v1")]
    pub schema_version: u32,
    /// Length [`MAX_SKILL_JOB`] after [`SkillsDoc::normalize`].
    pub slots: Vec<Option<SkillSlot>>,
}

impl Default for SkillsDoc {
    fn default() -> Self {
        Self {
            schema_version: Self::SCHEMA_VERSION,
            slots: vec![None; MAX_SKILL_JOB],
        }
    }
}

impl_json_doc!(SkillsDoc, "skills");

impl SkillsDoc {
    /// Pads `slots` with `None` to [`MAX_SKILL_JOB`] entries. Surplus entries are kept (a newer data set
    /// may have more positions; dropping them would lose skills) and reported through the return value.
    pub fn normalize(&mut self) -> usize {
        if self.slots.len() < MAX_SKILL_JOB {
            self.slots.resize(MAX_SKILL_JOB, None);
        }
        self.slots.len() - MAX_SKILL_JOB
    }
}

// ---- quests (§6.7) -------------------------------------------------------------------------------------

/// One active quest (`m_lpQuestCntArray` record).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct QuestRecord {
    /// `wId`.
    pub quest_id: u16,
    /// `nState`: 0 `QS_BEGIN` ... 14 `QS_END`.
    pub state: u8,
    /// `wTime` bits 0..14: minutes left.
    pub time_left_min: u16,
    /// `wTime` bit 15: the timer is frozen.
    pub timer_frozen: bool,
    /// `kill0`, `kill1`.
    pub kills: [u16; 2],
    /// Bit 0 (`bPatrol`).
    pub patrol_done: bool,
    /// Bit 1 (`r2` / `m_bDialog`).
    pub dialog_done: bool,
}

/// `character_docs.quests`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct QuestsDoc {
    /// Document version.
    #[serde(default = "v1")]
    pub schema_version: u32,
    /// Active quests in insertion order (gameplay cap 100 is the quest engine's rule).
    pub active: Vec<QuestRecord>,
    /// Completed quest ids (legacy cap 300 is doc 20's decision, no DB limit).
    pub completed: Vec<u16>,
    /// Quest-tracker ids, at most 5.
    pub checked: Vec<u16>,
}

impl Default for QuestsDoc {
    fn default() -> Self {
        Self {
            schema_version: Self::SCHEMA_VERSION,
            active: Vec::new(),
            completed: Vec::new(),
            checked: Vec::new(),
        }
    }
}

impl_json_doc!(QuestsDoc, "quests");

// ---- taskbar (§6.8) ------------------------------------------------------------------------------------

/// `SHORTCUT` kind of a chat macro, the only kind that carries `text`.
pub const SHORTCUT_KIND_CHAT: u32 = 8;
/// `MAX_SHORTCUT_STRING`: the longest chat macro in bytes.
pub const MAX_SHORTCUT_STRING: usize = 128;

/// One taskbar shortcut (`SHORTCUT`, ref/22 §11).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShortcutRecord {
    /// `m_dwShortcut`: 0 NONE 1 ETC 2 APPLET 3 MOTION 4 SCRIPT 5 ITEM 6 SKILL 7 OBJECT 8 CHAT 9 SKILLFUN
    /// 10 EMOTICON 11 LORDSKILL.
    pub kind: u32,
    /// Applet id / item `obj_id` / skill index / motion id / emoticon index.
    pub id: u32,
    /// Skill: 0 normal, 2 party; item: container type.
    #[serde(rename = "type")]
    pub type_: u32,
    /// `m_dwIndex`.
    pub index: u32,
    /// `m_dwUserId`.
    pub user_id: u32,
    /// `m_dwData`.
    pub data: u32,
    /// Chat macro text (only `kind == 8`), at most 128 bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// An applet slot (`slot` 0..17).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppletSlot {
    /// Slot 0..17.
    pub slot: u8,
    /// The shortcut.
    pub shortcut: ShortcutRecord,
}

/// An item-bar slot (`page` 0..7, `slot` 0..8).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ItemBarSlot {
    /// Page 0..7.
    pub page: u8,
    /// Slot 0..8.
    pub slot: u8,
    /// The shortcut.
    pub shortcut: ShortcutRecord,
}

/// A skill-queue slot (`slot` 0..4).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct QueueSlot {
    /// Slot 0..4.
    pub slot: u8,
    /// The shortcut.
    pub shortcut: ShortcutRecord,
}

/// `character_docs.taskbar` (persistence half of F-337).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TaskbarDoc {
    /// Document version.
    #[serde(default = "v1")]
    pub schema_version: u32,
    /// Non-empty applet slots.
    pub applets: Vec<AppletSlot>,
    /// Non-empty item-bar slots.
    pub items: Vec<ItemBarSlot>,
    /// Non-empty queue slots.
    pub queue: Vec<QueueSlot>,
    /// `m_nActionPoint` (legacy column `m_SkillBar`).
    pub action_points: i32,
}

impl Default for TaskbarDoc {
    fn default() -> Self {
        Self {
            schema_version: Self::SCHEMA_VERSION,
            applets: Vec::new(),
            items: Vec::new(),
            queue: Vec::new(),
            action_points: 0,
        }
    }
}

impl_json_doc!(TaskbarDoc, "taskbar");

// ---- buffs (§6.9) --------------------------------------------------------------------------------------

/// `BUFF_ITEM`.
pub const BUFF_ITEM: u8 = 0;
/// `BUFF_SKILL`.
pub const BUFF_SKILL: u8 = 1;
/// `BUFF_PET`.
pub const BUFF_PET: u8 = 2;
/// `BUFF_ITEM2` (the only kind with an absolute expiry).
pub const BUFF_ITEM2: u8 = 3;

/// One persisted buff. `BUFF_EQUIP` (4) and housing buffs are never stored.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BuffRecord {
    /// [`BUFF_ITEM`] 0, [`BUFF_SKILL`] 1, [`BUFF_PET`] 2, [`BUFF_ITEM2`] 3.
    pub kind: u8,
    /// Item / skill id.
    pub id: u16,
    /// Skill level; pet: `MAKELONG(adj, dst)`; item2: 0 (see `expires_at`).
    pub level: u32,
    /// Remaining duration at snapshot time in milliseconds (legacy `tmTotal` as saved).
    pub remaining_ms: u32,
    /// `BUFF_ITEM2` only: absolute expiry (the legacy server stored it in `level`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
}

/// `character_docs.buffs`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BuffsDoc {
    /// Document version.
    #[serde(default = "v1")]
    pub schema_version: u32,
    /// The buffs.
    pub buffs: Vec<BuffRecord>,
}

impl Default for BuffsDoc {
    fn default() -> Self {
        Self {
            schema_version: Self::SCHEMA_VERSION,
            buffs: Vec::new(),
        }
    }
}

impl_json_doc!(BuffsDoc, "buffs");

// ---- SM timers (§6.10) ---------------------------------------------------------------------------------

/// Number of `SM_*` timers.
pub const SM_TIMER_COUNT: usize = 26;

/// `character_docs.sm_timers`: `timers[SM_*]` in SM index order (0 BANK15 ... 25 BANK001, ref/04 §7.10).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SmTimersDoc {
    /// Document version.
    #[serde(default = "v1")]
    pub schema_version: u32,
    /// `m_dwSMTime[SM_*]`.
    pub timers: [u32; SM_TIMER_COUNT],
    /// `m_nPlusMaxHitPoint` (stored with `SM_MAX_HP50` = 9).
    pub max_hp_bonus: i32,
    /// `m_nAttackResistLeft` (`SM_RESIST_ATTACK_LEFT` = 16).
    pub attack_resist_left: i32,
    /// `m_nAttackResistRight` (`SM_RESIST_ATTACK_RIGHT` = 17).
    pub attack_resist_right: i32,
    /// `m_nDefenseResist` (`SM_RESIST_DEFENSE` = 18).
    pub defense_resist: i32,
}

impl Default for SmTimersDoc {
    /// All timers and bonuses zero ("row deleted when all zero" of the legacy schema becomes an all-zero
    /// document).
    fn default() -> Self {
        Self {
            schema_version: Self::SCHEMA_VERSION,
            timers: [0; SM_TIMER_COUNT],
            max_hp_bonus: 0,
            attack_resist_left: 0,
            attack_resist_right: 0,
            defense_resist: 0,
        }
    }
}

impl_json_doc!(SmTimersDoc, "sm_timers");

// ---- honor (§6.11) -------------------------------------------------------------------------------------

/// `character_docs.honor`: honor title id -> counter. Zero counters are omitted when written and dropped
/// when read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HonorDoc {
    /// Document version.
    #[serde(default = "v1")]
    pub schema_version: u32,
    /// Title id (`honorList.txt` id) -> counter.
    #[serde(
        serialize_with = "serialize_nonzero_counters",
        deserialize_with = "deserialize_nonzero_counters"
    )]
    pub counters: BTreeMap<u16, i32>,
}

impl Default for HonorDoc {
    fn default() -> Self {
        Self {
            schema_version: Self::SCHEMA_VERSION,
            counters: BTreeMap::new(),
        }
    }
}

impl_json_doc!(HonorDoc, "honor");

impl HonorDoc {
    /// Sets a counter; zero removes the entry.
    pub fn set(&mut self, title_id: u16, value: i32) {
        if value == 0 {
            self.counters.remove(&title_id);
        } else {
            self.counters.insert(title_id, value);
        }
    }

    /// The counter of `title_id` (0 when absent).
    pub fn get(&self, title_id: u16) -> i32 {
        self.counters.get(&title_id).copied().unwrap_or(0)
    }
}

fn serialize_nonzero_counters<S: serde::Serializer>(
    counters: &BTreeMap<u16, i32>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.collect_map(counters.iter().filter(|(_, v)| **v != 0))
}

fn deserialize_nonzero_counters<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<u16, i32>, D::Error> {
    let mut counters = BTreeMap::<u16, i32>::deserialize(deserializer)?;
    counters.retain(|_, v| *v != 0);
    Ok(counters)
}

// ---- settings (§6.12) ----------------------------------------------------------------------------------

/// `chat.ignore`: array of character names (at most 1000; doc 21/23).
pub const SETTING_CHAT_IGNORE: &str = "chat.ignore";
/// `privacy.whisper` (boolean, absent = `true`).
pub const SETTING_PRIVACY_WHISPER: &str = "privacy.whisper";
/// `privacy.trade`.
pub const SETTING_PRIVACY_TRADE: &str = "privacy.trade";
/// `privacy.party`.
pub const SETTING_PRIVACY_PARTY: &str = "privacy.party";
/// `privacy.shout`.
pub const SETTING_PRIVACY_SHOUT: &str = "privacy.shout";
/// `privacy.messenger`.
pub const SETTING_PRIVACY_MESSENGER: &str = "privacy.messenger";
/// `privacy.messenger_join` (friend login/logout notices).
pub const SETTING_PRIVACY_MESSENGER_JOIN: &str = "privacy.messenger_join";
/// `client.ui`: the web client's layout JSON as a string (`ClientUiSave.layout_json`, at most 8000 bytes),
/// stored verbatim, never interpreted by the server.
pub const SETTING_CLIENT_UI: &str = "client.ui";
/// Every registered key of §6.12 (the list is frozen with the WP-DB-03 golden files).
pub const REGISTERED_SETTINGS: [&str; 8] = [
    SETTING_CHAT_IGNORE,
    SETTING_PRIVACY_WHISPER,
    SETTING_PRIVACY_TRADE,
    SETTING_PRIVACY_PARTY,
    SETTING_PRIVACY_SHOUT,
    SETTING_PRIVACY_MESSENGER,
    SETTING_PRIVACY_MESSENGER_JOIN,
    SETTING_CLIENT_UI,
];
/// Total serialised size limit of one `SettingsDoc`.
pub const MAX_SETTINGS_BYTES: usize = 32 * 1024;
/// Size limit of the `client.ui` value in bytes.
pub const MAX_CLIENT_UI_BYTES: usize = 8000;
/// Maximum number of names in `chat.ignore`.
pub const MAX_CHAT_IGNORE: usize = 1000;

/// `character_docs.settings`: small per-character server-side settings that are not worth a column.
/// Unknown keys are preserved; the game enforces the size limits before building the snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SettingsDoc {
    /// Document version.
    #[serde(default = "v1")]
    pub schema_version: u32,
    /// Key -> value; keys are sorted, so the JSON is deterministic.
    pub values: BTreeMap<String, Value>,
}

impl Default for SettingsDoc {
    fn default() -> Self {
        Self {
            schema_version: Self::SCHEMA_VERSION,
            values: BTreeMap::new(),
        }
    }
}

impl_json_doc!(SettingsDoc, "settings");

impl SettingsDoc {
    /// A `privacy.*` flag: `true` unless the key holds `false`.
    pub fn privacy(&self, key: &str) -> bool {
        self.values
            .get(key)
            .and_then(Value::as_bool)
            .unwrap_or(true)
    }

    /// The `client.ui` string, if set.
    pub fn client_ui(&self) -> Option<&str> {
        self.values.get(SETTING_CLIENT_UI).and_then(Value::as_str)
    }

    /// The `chat.ignore` names (non-string entries are skipped).
    pub fn chat_ignore(&self) -> Vec<&str> {
        self.values
            .get(SETTING_CHAT_IGNORE)
            .and_then(Value::as_array)
            .map(|names| names.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default()
    }

    /// Serialised size in bytes of the whole document.
    pub fn serialized_len(&self) -> usize {
        serde_json::to_string(self).map_or(usize::MAX, |s| s.len())
    }

    /// Checks the §6.12 limits: the document is at most [`MAX_SETTINGS_BYTES`], `client.ui` is a string of
    /// at most [`MAX_CLIENT_UI_BYTES`] bytes and `chat.ignore` an array of at most [`MAX_CHAT_IGNORE`]
    /// strings. Returns the first violation.
    pub fn check_limits(&self) -> Result<(), String> {
        if let Some(ui) = self.values.get(SETTING_CLIENT_UI) {
            match ui.as_str() {
                Some(s) if s.len() <= MAX_CLIENT_UI_BYTES => {}
                Some(s) => {
                    return Err(format!(
                        "client.ui is {} bytes, the limit is {MAX_CLIENT_UI_BYTES}",
                        s.len()
                    ));
                }
                None => return Err("client.ui must be a string".to_owned()),
            }
        }
        if let Some(ignore) = self.values.get(SETTING_CHAT_IGNORE) {
            match ignore.as_array() {
                Some(names)
                    if names.len() <= MAX_CHAT_IGNORE && names.iter().all(Value::is_string) => {}
                Some(_) => {
                    return Err(format!(
                        "chat.ignore must hold at most {MAX_CHAT_IGNORE} strings"
                    ));
                }
                None => return Err("chat.ignore must be an array".to_owned()),
            }
        }
        let len = self.serialized_len();
        if len > MAX_SETTINGS_BYTES {
            return Err(format!(
                "settings are {len} bytes, the limit is {MAX_SETTINGS_BYTES}"
            ));
        }
        Ok(())
    }
}
