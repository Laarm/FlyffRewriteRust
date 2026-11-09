//! `StartupState` (Plan/13 §16.9): everything the services load at boot, read in one transaction by
//! [`crate::startup::load`] before the game thread starts.
//!
//! The core schema (`0001`) gives the `meta` counters, `server_kv`, the runtime overrides and the character
//! directory; the messaging group (`0002`) the party names and the mails; the social groups the guilds, wars and
//! rejoin cooldowns (`0003`), the couples and campuses (`0004`) and the sieges and GC 1:1 tenders (`0005`); the
//! content groups the lord, tax and secret room (`0006`) and the guild houses and the rainbow race (`0007`).
//! `Default` is the empty state of an absent group.

use super::character::CharacterState;
use super::guild::{GuildLoad, GuildWarRecord};
use super::houses::{GuildHouseLoad, RainbowRaceLoad};
use super::messaging::MailRecord;
use super::realm::{LordLoad, SecretRoomLoad, TaxLoad};
use super::relations::{CampusLoad, CoupleProposalRecord, CoupleRecord};
use super::siege::{Gc1to1Load, SiegeLoad};
use flyff_common::{AccountId, CharacterId, UnixTime};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The `meta` counters of Plan/13 §15.1 (`next_*` = next id to hand out, `*_round` = current round number).
/// Services allocate ids in memory from these values and persist `RaiseCounter` with every row they create.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetaCounters {
    /// `next_character_id` (allocated by the writer inside `CreateCharacter`).
    pub next_character_id: u32,
    /// `next_guild_id`.
    pub next_guild_id: u32,
    /// `next_mail_id`.
    pub next_mail_id: u32,
    /// `next_guild_war_id`.
    pub next_guild_war_id: u32,
    /// `next_item_serial` (handed out in blocks, `ReserveSerialBlock`).
    pub next_item_serial: u64,
    /// `next_campus_id`.
    pub next_campus_id: u32,
    /// `next_couple_id`.
    pub next_couple_id: u32,
    /// `next_siege_id`.
    pub next_siege_id: u32,
    /// `next_lord_election_id`.
    pub next_lord_election_id: u32,
    /// `next_guild_furniture_seq`.
    pub next_guild_furniture_seq: u32,
    /// `next_quiz_event_id`.
    pub next_quiz_event_id: u32,
    /// `tax_round`.
    pub tax_round: u32,
    /// `secret_room_round`.
    pub secret_room_round: u32,
    /// `rainbow_race_round`.
    pub rainbow_race_round: u32,
    /// `gc1to1_round`.
    pub gc1to1_round: u32,
}

impl Default for MetaCounters {
    /// The seeds of migration `0001` (`next_*` = 1, rounds = 0).
    fn default() -> Self {
        Self {
            next_character_id: 1,
            next_guild_id: 1,
            next_mail_id: 1,
            next_guild_war_id: 1,
            next_item_serial: 1,
            next_campus_id: 1,
            next_couple_id: 1,
            next_siege_id: 1,
            next_lord_election_id: 1,
            next_guild_furniture_seq: 1,
            next_quiz_event_id: 1,
            tax_round: 0,
            secret_room_round: 0,
            rainbow_race_round: 0,
            gc1to1_round: 0,
        }
    }
}

/// One row of `runtime_override` (Plan/13 §5.1): a GM/admin-changed configuration value that survives restarts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeOverrideRecord {
    /// Dotted `flyff.toml` key, e.g. `rates.monster_exp`.
    pub key: String,
    /// The value (any JSON).
    pub value: serde_json::Value,
    /// `gm:<account>`, `admin:<token-name>` or `cli`.
    pub set_by: String,
    /// When it was set.
    pub set_at: UnixTime,
}

/// One entry of the character directory: every non-purged character (Plan/13 §16.9). The game thread's
/// `PlayerDirectory` is built from it (name lookups, the purge selection `state = 1 AND deleted_at < ...`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectoryEntry {
    /// Character id.
    pub id: CharacterId,
    /// Owning account.
    pub account_id: AccountId,
    /// Name.
    pub name: String,
    /// `slot` 0..2 (the player directory needs it for the per-account slot view; Plan/21 §3.1, open question
    /// 25, added by WP-SOCIAL-01).
    pub slot: u8,
    /// `sex`.
    pub sex: u8,
    /// `job`.
    pub job: u8,
    /// `level`.
    pub level: u16,
    /// `state`.
    pub state: CharacterState,
    /// `deleted_at` (soft-deleted characters only).
    pub deleted_at: Option<UnixTime>,
}

/// One row of `party_names` (Plan/13 §7, 21 §4.4): the stored advanced-party name of a leader.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartyNameRecord {
    /// The leader who owns the name.
    pub character_id: CharacterId,
    /// The name as stored (unique case-insensitively).
    pub name: String,
}

/// Everything services load at boot in one read transaction (Plan/13 §16.9).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StartupState {
    /// All `meta` counters.
    pub counters: MetaCounters,
    /// `server_kv`: key -> JSON value.
    pub server_kv: BTreeMap<String, serde_json::Value>,
    /// `runtime_override` rows, ordered by key.
    pub runtime_overrides: Vec<RuntimeOverrideRecord>,
    /// Every non-purged character, ordered by id.
    pub directory: Vec<DirectoryEntry>,
    /// `party_names` rows (the messaging group, 0002), ordered by character id.
    pub party_names: Vec<PartyNameRecord>,
    /// Every undeleted mail (`deleted_at IS NULL`, 0002), ordered by id: the `MailService` loads them all at
    /// start (legacy `LoadPost`).
    pub mails: Vec<MailRecord>,
    /// Every guild with its roster, bank, log tail and quests, ordered by id (0003).
    pub guilds: Vec<GuildLoad>,
    /// The wars without a result, ordered by id (0003); the 2 h timeout continues from `started_at`.
    pub running_wars: Vec<GuildWarRecord>,
    /// The rejoin cooldowns that have not ended yet (`until > now`), ordered by character (0003).
    pub rejoin_cooldowns: Vec<(CharacterId, UnixTime)>,
    /// Every couple with two members, ordered by id (0004).
    pub couples: Vec<CoupleRecord>,
    /// Every stored proposal cooldown, ordered by proposer (0004); the `CoupleService` expires them.
    pub couple_proposals: Vec<CoupleProposalRecord>,
    /// Every campus with its members, ordered by id (0004).
    pub campuses: Vec<CampusLoad>,
    /// Every stored campus point balance, ordered by character (0004); the `CampusService` is the only writer.
    pub campus_points: Vec<(CharacterId, i32)>,
    /// The siege state (0005).
    pub sieges: SiegeLoad,
    /// The GC 1:1 tenders and line-ups (0005).
    pub gc1to1: Gc1to1Load,
    /// The lord election, candidates, electors, current lord, lord events and skill cooldowns (0006).
    pub lord: LordLoad,
    /// The tax owners and details of the current round (0006).
    pub tax: TaxLoad,
    /// The tendered (`T`) secret-room tenders of the current round with their line-ups (0006).
    pub secret_room: SecretRoomLoad,
    /// Every guild house with its furniture, ordered by guild (0007).
    pub guild_houses: Vec<GuildHouseLoad>,
    /// The rainbow-race entries of the current round and the previous ranking (0007).
    pub rainbow_race: RainbowRaceLoad,
    /// The undelivered offline notes per receiver, ordered by character (`SELECT to_id, COUNT(*) FROM notes WHERE
    /// delivered_at IS NULL GROUP BY to_id`): the game's `NoteBox` enforces the cap of 20 per receiver in memory
    /// because the writer is asynchronous (Plan/21 §9.6, WP-SOCIAL-20).
    pub note_counts: Vec<(CharacterId, u32)>,
}
