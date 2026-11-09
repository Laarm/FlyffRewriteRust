//! Character records: the snapshot-owned and identity columns of `characters`, the document bundle, bank
//! snapshots and the enter-world load (Plan/13 §5.3-§5.6, §16.2, §16.10).
//!
//! Column ownership (13 §4): [`CharacterRow`] holds every **snapshot-owned** column (written by every
//! snapshot); [`CharacterIdentity`] holds the **identity/admin-owned** ones (written only by explicit ops).
//! A snapshot never writes identity columns and an op never writes snapshot columns.

use super::container::RepairNote;
use super::doc::JsonDoc;
use super::docs::{
    BankDoc, BuffsDoc, HonorDoc, InventoryDoc, PocketsDoc, QuestsDoc, SettingsDoc, SkillsDoc,
    SmTimersDoc, TaskbarDoc,
};
use super::houses::HousingLoad;
use super::messaging::{FriendRecord, NoteRecord, PendingGrantRecord, PendingRemovalRecord};
use crate::error::{DbError, DbErrorKind};
use flyff_common::{AccountId, CharacterId, SaveGen, UnixTime};
use serde::{Deserialize, Serialize};

/// Number of `PARTS_*` entries in [`CharacterRow::equipment_preview`] (`MAX_HUMAN_PARTS`).
pub const EQUIPMENT_PREVIEW_LEN: usize = 31;

/// Why a snapshot was taken (Plan/13 §16.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SaveReason {
    /// 90 s autosave.
    Periodic,
    /// Urgent (e.g. after a purchase).
    Urgent,
    /// World change.
    WorldChange,
    /// Logout.
    Logout,
    /// Return to the character select screen.
    ToCharSelect,
    /// Part of an atomic multi-entity batch.
    Atomic,
    /// Graceful shutdown.
    Shutdown,
}

/// Lifecycle of a `characters` row (`characters.state`, Plan/13 §14.2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CharacterState {
    /// 0: playable.
    #[default]
    Active = 0,
    /// 1: soft-deleted; name and slot are free, purged after `retention.deleted_character_days`.
    Deleted = 1,
    /// 2: sealed by an item; keeps its name, occupies no slot.
    Sealed = 2,
}

impl CharacterState {
    /// The `characters.state` value.
    pub const fn to_db(self) -> i64 {
        self as i64
    }

    /// Parses a `characters.state` value.
    pub const fn from_db(value: i64) -> Option<CharacterState> {
        match value {
            0 => Some(CharacterState::Active),
            1 => Some(CharacterState::Deleted),
            2 => Some(CharacterState::Sealed),
            _ => None,
        }
    }
}

/// The snapshot-owned columns of `characters`, 1:1 and with the same names (Plan/13 §16.2, §5.4 owner S).
/// `max_level` is written as `MAX(max_level, ?)`; `gold` is an `i64` column the game clamps to `i32`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CharacterRow {
    /// `sex` 0 male / 1 female.
    pub sex: u8,
    /// `mover_index`: `MI_MALE` 11 / `MI_FEMALE` 12.
    pub mover_index: u32,
    /// `skin_set`.
    pub skin_set: u8,
    /// `hair_mesh`.
    pub hair_mesh: u8,
    /// `hair_color` (ARGB).
    pub hair_color: u32,
    /// `head_mesh`.
    pub head_mesh: u8,
    /// `hide_coat_mask`: bit z = `m_abHideCoat[z]`.
    pub hide_coat_mask: u8,
    /// `scale`.
    pub scale: f32,
    /// `world_id`.
    pub world_id: u32,
    /// `layer`.
    pub layer: i32,
    /// `pos_x/y/z`.
    pub pos: [f32; 3],
    /// `return_world_id`.
    pub return_world_id: u32,
    /// `return_x/y/z`.
    pub return_pos: [f32; 3],
    /// `marking_world_id`, 0 = no mark.
    pub marking_world_id: u32,
    /// `marking_x/y/z`.
    pub marking_pos: [f32; 3],
    /// `motion`.
    pub motion: u32,
    /// `act_state`.
    pub act_state: u32,
    /// `act_state_flags`.
    pub act_state_flags: u32,
    /// `hp`.
    pub hp: i32,
    /// `mp`.
    pub mp: i32,
    /// `fp`.
    pub fp: i32,
    /// `job`.
    pub job: u8,
    /// `level`.
    pub level: u16,
    /// `max_level`.
    pub max_level: u16,
    /// `exp`.
    pub exp: i64,
    /// `stat_str`.
    pub stat_str: i32,
    /// `stat_sta`.
    pub stat_sta: i32,
    /// `stat_dex`.
    pub stat_dex: i32,
    /// `stat_int`.
    pub stat_int: i32,
    /// `stat_points` (`m_nRemainGP`).
    pub stat_points: i32,
    /// `skill_points`.
    pub skill_points: i32,
    /// `skill_level_total`.
    pub skill_level_total: i32,
    /// `gold`.
    pub gold: i64,
    /// `flight_exp`.
    pub flight_exp: i32,
    /// `tutorial_state` (the legacy `m_nFlightLv` re-use).
    pub tutorial_state: i32,
    /// `fuel`, -1 = none.
    pub fuel: i32,
    /// `fuel_time_acc`.
    pub fuel_time_acc: u32,
    /// `mode_flags` (`FRESH_MODE` is cleared in the persisted value).
    pub mode_flags: u32,
    /// `fame`.
    pub fame: i32,
    /// `pk_value`.
    pub pk_value: i32,
    /// `pk_propensity`.
    pub pk_propensity: u32,
    /// `pk_exp`.
    pub pk_exp: u32,
    /// `death_exp`.
    pub death_exp: i64,
    /// `death_level`.
    pub death_level: i32,
    /// `angel_exp`.
    pub angel_exp: i64,
    /// `angel_level`.
    pub angel_level: i32,
    /// `mute_remaining_s` (the legacy `m_dwFlyTime` re-use).
    pub mute_remaining_s: u32,
    /// `messenger_state`.
    pub messenger_state: u32,
    /// `play_time_s`.
    pub play_time_s: i64,
    /// `attendance_bits`.
    pub attendance_bits: i64,
    /// `attendance_minutes`.
    pub attendance_minutes: u32,
    /// `attendance_elapsed`.
    pub attendance_elapsed: u32,
    /// `coupon_count`.
    pub coupon_count: i32,
    /// `honor_title_id`, `None` = no title (wire -1).
    pub honor_title_id: Option<u16>,
    /// `summoned_pet_obj_id`, `None` = none (wire `NULL_ID`).
    pub summoned_pet_obj_id: Option<u16>,
    /// `rest_points`.
    pub rest_points: i32,
    /// `last_logout_at`.
    pub last_logout_at: UnixTime,
    /// `equipment_preview`: the 31 `PARTS_*` item ids shown by the character list, 0 = empty.
    pub equipment_preview: [u32; EQUIPMENT_PREVIEW_LEN],
}

impl Default for CharacterRow {
    /// The SQL column defaults of Plan/13 §5.3 where they exist (level 1, stats 15, `fuel` -1, scale 1,
    /// `return_world_id` 1); columns without a SQL default are zero except `mover_index` (`MI_MALE` 11).
    fn default() -> Self {
        Self {
            sex: 0,
            mover_index: 11,
            skin_set: 0,
            hair_mesh: 0,
            hair_color: 0,
            head_mesh: 0,
            hide_coat_mask: 0,
            scale: 1.0,
            world_id: 0,
            layer: 0,
            pos: [0.0; 3],
            return_world_id: 1,
            return_pos: [0.0; 3],
            marking_world_id: 0,
            marking_pos: [0.0; 3],
            motion: 0,
            act_state: 0,
            act_state_flags: 0,
            hp: 0,
            mp: 0,
            fp: 0,
            job: 0,
            level: 1,
            max_level: 1,
            exp: 0,
            stat_str: 15,
            stat_sta: 15,
            stat_dex: 15,
            stat_int: 15,
            stat_points: 0,
            skill_points: 0,
            skill_level_total: 0,
            gold: 0,
            flight_exp: 0,
            tutorial_state: 0,
            fuel: -1,
            fuel_time_acc: 0,
            mode_flags: 0,
            fame: 0,
            pk_value: 0,
            pk_propensity: 0,
            pk_exp: 0,
            death_exp: 0,
            death_level: 0,
            angel_exp: 0,
            angel_level: 0,
            mute_remaining_s: 0,
            messenger_state: 0,
            play_time_s: 0,
            attendance_bits: 0,
            attendance_minutes: 0,
            attendance_elapsed: 0,
            coupon_count: 0,
            honor_title_id: None,
            summoned_pet_obj_id: None,
            rest_points: 0,
            last_logout_at: UnixTime(0),
            equipment_preview: [0; EQUIPMENT_PREVIEW_LEN],
        }
    }
}

impl CharacterRow {
    /// The `characters` columns a snapshot writes (owner S, Plan/13 §5.4) in table order, with the vectors
    /// expanded (`pos` = `pos_x, pos_y, pos_z`). `saved_at` (commit time) is stamped by the writer and is
    /// not part of the row.
    pub const SNAPSHOT_COLUMNS: &'static [&'static str] = &[
        "sex",
        "mover_index",
        "skin_set",
        "hair_mesh",
        "hair_color",
        "head_mesh",
        "hide_coat_mask",
        "scale",
        "world_id",
        "layer",
        "pos_x",
        "pos_y",
        "pos_z",
        "return_world_id",
        "return_x",
        "return_y",
        "return_z",
        "marking_world_id",
        "marking_x",
        "marking_y",
        "marking_z",
        "motion",
        "act_state",
        "act_state_flags",
        "hp",
        "mp",
        "fp",
        "job",
        "level",
        "max_level",
        "exp",
        "stat_str",
        "stat_sta",
        "stat_dex",
        "stat_int",
        "stat_points",
        "skill_points",
        "skill_level_total",
        "gold",
        "flight_exp",
        "tutorial_state",
        "fuel",
        "fuel_time_acc",
        "mode_flags",
        "fame",
        "pk_value",
        "pk_propensity",
        "pk_exp",
        "death_exp",
        "death_level",
        "angel_exp",
        "angel_level",
        "mute_remaining_s",
        "messenger_state",
        "play_time_s",
        "attendance_bits",
        "attendance_minutes",
        "attendance_elapsed",
        "coupon_count",
        "honor_title_id",
        "summoned_pet_obj_id",
        "rest_points",
        "last_logout_at",
        "equipment_preview",
    ];
}

/// The identity/admin-owned columns of `characters` (read-only for the game; Plan/13 §16.2).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CharacterIdentity {
    /// `id` (`m_idPlayer`), from `meta.next_character_id`.
    pub id: CharacterId,
    /// `account_id`.
    pub account_id: AccountId,
    /// `slot` 0..2.
    pub slot: u8,
    /// `name`, unique among non-deleted characters (case-insensitive).
    pub name: String,
    /// `state`.
    pub state: CharacterState,
    /// `created_at`.
    pub created_at: UnixTime,
    /// `authority` (`m_chAuthority`, an ASCII letter such as `b'F'`); 0 when unset.
    pub authority: u8,
    /// `login_blocked_until`, `None` = not blocked.
    pub login_blocked_until: Option<UnixTime>,
}

impl CharacterIdentity {
    /// The identity/admin-owned columns of `characters`, in table order (the SQL-side superset of the
    /// fields above: `deleted_at` and `sealed_at` are written by the soft-delete / seal ops only).
    pub const COLUMNS: &'static [&'static str] = &[
        "id",
        "account_id",
        "slot",
        "name",
        "state",
        "created_at",
        "deleted_at",
        "sealed_at",
        "login_blocked_until",
        "authority",
    ];
}

/// The nine `character_docs` documents (Plan/13 §5.5, §16.2); every column is snapshot-owned.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CharacterDocs {
    /// `inventory`.
    pub inventory: InventoryDoc,
    /// `pockets`.
    pub pockets: PocketsDoc,
    /// `skills`.
    pub skills: SkillsDoc,
    /// `quests`.
    pub quests: QuestsDoc,
    /// `taskbar`.
    pub taskbar: TaskbarDoc,
    /// `buffs`.
    pub buffs: BuffsDoc,
    /// `sm_timers`.
    pub sm_timers: SmTimersDoc,
    /// `honor`.
    pub honor: HonorDoc,
    /// `settings`.
    pub settings: SettingsDoc,
}

impl CharacterDocs {
    /// The `character_docs` document columns, in the order of [`CharacterDocs::to_json_columns`].
    pub const COLUMNS: [&'static str; 9] = [
        "inventory",
        "pockets",
        "skills",
        "quests",
        "taskbar",
        "buffs",
        "sm_timers",
        "honor",
        "settings",
    ];

    /// Serialises every document, in [`CharacterDocs::COLUMNS`] order (writer thread).
    pub fn to_json_columns(&self) -> Result<[String; 9], DbError> {
        Ok([
            self.inventory.to_json()?,
            self.pockets.to_json()?,
            self.skills.to_json()?,
            self.quests.to_json()?,
            self.taskbar.to_json()?,
            self.buffs.to_json()?,
            self.sm_timers.to_json()?,
            self.honor.to_json()?,
            self.settings.to_json()?,
        ])
    }

    /// Parses the nine columns, in [`CharacterDocs::COLUMNS`] order (reader threads). Old document
    /// versions are upgraded; the first malformed document fails the whole load.
    pub fn from_json_columns(columns: &[String; 9]) -> Result<CharacterDocs, DbError> {
        let [
            inventory,
            pockets,
            skills,
            quests,
            taskbar,
            buffs,
            sm_timers,
            honor,
            settings,
        ] = columns;
        Ok(CharacterDocs {
            inventory: InventoryDoc::from_json(inventory)?,
            pockets: PocketsDoc::from_json(pockets)?,
            skills: SkillsDoc::from_json(skills)?,
            quests: QuestsDoc::from_json(quests)?,
            taskbar: TaskbarDoc::from_json(taskbar)?,
            buffs: BuffsDoc::from_json(buffs)?,
            sm_timers: SmTimersDoc::from_json(sm_timers)?,
            honor: HonorDoc::from_json(honor)?,
            settings: SettingsDoc::from_json(settings)?,
        })
    }
}

/// One bank written by a snapshot (`pin` is never part of a snapshot).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BankSnapshot {
    /// The bank's owner (own or another active character of the account).
    pub character_id: CharacterId,
    /// `banks.gold`.
    pub gold: i64,
    /// `banks.container`.
    pub doc: BankDoc,
}

/// One bank loaded at enter-world: every ACTIVE character of the account has one (Plan/13 §16.10).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BankLoad {
    /// The bank's owner.
    pub character_id: CharacterId,
    /// The owner's slot (the legacy `m_idPlayerBank[slot]` order).
    pub slot: u8,
    /// `banks.pin`: four ASCII digits.
    pub pin: [u8; 4],
    /// `banks.gold`.
    pub gold: i64,
    /// `banks.container`.
    pub doc: BankDoc,
}

/// Owned deep copy of one character, built on the game thread and written by the persistence thread
/// (Plan/13 §16.2).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CharacterSnapshot {
    /// Owning account.
    pub account_id: AccountId,
    /// Character.
    pub character_id: CharacterId,
    /// Global save generation (higher wins when coalescing). `gen` is a reserved word in edition
    /// 2024, hence the raw identifier (Plan/13 §16.2 names the field `gen`).
    #[serde(rename = "gen")]
    pub r#gen: SaveGen,
    /// Why the snapshot was taken.
    pub reason: SaveReason,
    /// Wall-clock time of the snapshot.
    pub taken_at: UnixTime,
    /// Snapshot-owned columns.
    pub row: CharacterRow,
    /// The nine documents.
    pub docs: CharacterDocs,
    /// Every bank loaded at enter-world (own + the account's other active characters), at most 3.
    pub banks: Vec<BankSnapshot>,
}

/// Payload of `DbOp::CreateCharacter`; the id is assigned by the writer (Plan/13 §16.2).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NewCharacterRecord {
    /// Owning account.
    pub account_id: AccountId,
    /// Slot 0..2.
    pub slot: u8,
    /// Name.
    pub name: String,
    /// `created_at`.
    pub created_at: UnixTime,
    /// Initial snapshot-owned columns.
    pub row: CharacterRow,
    /// Initial documents.
    pub docs: CharacterDocs,
    /// Initial bank PIN: four ASCII digits.
    pub bank_pin: [u8; 4],
    /// Initial bank container.
    pub bank: BankDoc,
}

/// Everything the game needs to place a character into the world (Plan/13 §16.10): the identity, the snapshot
/// columns, the nine documents, the banks and the groups of the later migrations (friends, notes, grants, removals,
/// party name, guild rejoin cooldown, housing); a group without rows is its `Default`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CharacterLoad {
    /// Identity columns.
    pub identity: CharacterIdentity,
    /// Snapshot-owned columns.
    pub row: CharacterRow,
    /// Documents, upgraded to the current schema version with containers validated and repaired.
    pub docs: CharacterDocs,
    /// What the container validation had to repair.
    pub repairs: Vec<RepairNote>,
    /// A bank for every ACTIVE character of the account.
    pub banks: Vec<BankLoad>,
    /// The character's own friend rows (0002), oldest friendship first.
    pub friends: Vec<FriendRecord>,
    /// Undelivered offline notes, oldest first (0002; at most 20 by game rule). Deliver them after the join
    /// succeeded and then send `DbOp::MarkNotesDelivered`.
    pub notes: Vec<NoteRecord>,
    /// Open grants, oldest first (0002): delivered by one `GrantDelivery` batch at enter-world.
    pub grants: Vec<PendingGrantRecord>,
    /// Open character removals, oldest first (0002): applied by one `RemovalApply` batch at enter-world.
    pub removals: Vec<PendingRemovalRecord>,
    /// The stored advanced-party name of this character as a leader (0002), if any.
    pub party_name: Option<String>,
    /// The raw end of the guild rejoin cooldown (0003, `guild_rejoin_cooldowns.until`), if a row exists. The game
    /// compares it with its own clock; the row is deleted by the daily job once it is over.
    pub guild_rejoin_until: Option<UnixTime>,
    /// The character's mini room (0007): the raw furniture rows, expired ones included (the game removes them at
    /// load, 13 §17.4 step 7), and the characters allowed to visit.
    pub housing: HousingLoad,
}

/// One entry of the character-select list (`queries::character_list`, Plan/13 §16.7): at most three active
/// characters of an account, ordered by slot. The equipment shown comes from the precomputed
/// `equipment_preview`, so the list never parses an inventory.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CharacterListEntry {
    /// Character id.
    pub id: CharacterId,
    /// Slot 0..2.
    pub slot: u8,
    /// Name.
    pub name: String,
    /// `sex`.
    pub sex: u8,
    /// `mover_index`.
    pub mover_index: u32,
    /// `skin_set`.
    pub skin_set: u8,
    /// `hair_mesh`.
    pub hair_mesh: u8,
    /// `hair_color` (ARGB).
    pub hair_color: u32,
    /// `head_mesh`.
    pub head_mesh: u8,
    /// `hide_coat_mask`.
    pub hide_coat_mask: u8,
    /// `job`.
    pub job: u8,
    /// `level`.
    pub level: u16,
    /// `[stat_str, stat_sta, stat_dex, stat_int]`.
    pub stats: [i32; 4],
    /// `mode_flags`.
    pub mode_flags: u32,
    /// `world_id`.
    pub world_id: u32,
    /// `pos_x/y/z`.
    pub pos: [f32; 3],
    /// `equipment_preview`, decoded: the 31 `PARTS_*` item ids, 0 = empty.
    pub equipment_preview: [u32; EQUIPMENT_PREVIEW_LEN],
    /// `login_blocked_until`, `None` = not blocked (the list shows the slot as blocked while it lies in the future).
    pub login_blocked_until: Option<UnixTime>,
    /// The guild of the character (`guild_members`), `None` when it has none.
    pub guild_id: Option<flyff_common::GuildId>,
}

/// Converts the `characters.state` column, naming the bad value.
pub fn character_state_from_db(value: i64) -> Result<CharacterState, DbError> {
    CharacterState::from_db(value).ok_or_else(|| {
        DbError::new(
            DbErrorKind::Bug,
            format!("characters.state holds {value}, expected 0, 1 or 2"),
        )
    })
}
