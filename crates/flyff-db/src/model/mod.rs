//! Pure data records of `flyff-db` (Plan/13 §6, §16.2): no rusqlite, no SQL. The game converts runtime
//! structs to and from these records in `flyff_game::persist`; the persistence thread serialises them to
//! the JSON documents of `character_docs` and friends.

pub mod account;
pub mod character;
pub mod container;
pub mod doc;
pub mod docs;
pub mod guild;
pub mod houses;
pub mod item;
pub mod messaging;
pub mod realm;
pub mod relations;
pub mod siege;
pub mod startup;

pub use account::{
    AUTHORITY_DEFAULT, AccountBanRecord, AccountListEntry, AccountListFilter, AccountRecord,
    CreatedVia, NewAccountRecord,
};

pub use character::{
    BankLoad, BankSnapshot, CharacterDocs, CharacterIdentity, CharacterListEntry, CharacterLoad,
    CharacterRow, CharacterSnapshot, CharacterState, EQUIPMENT_PREVIEW_LEN, NewCharacterRecord,
    SaveReason, character_state_from_db,
};
pub use container::{ContainerRecord, EQUIP_SLOTS, RepairNote};
pub use doc::JsonDoc;
pub use docs::{
    AppletSlot, BUFF_ITEM, BUFF_ITEM2, BUFF_PET, BUFF_SKILL, BankDoc, BuffRecord, BuffsDoc,
    GuildBankDoc, HonorDoc, InventoryDoc, ItemBarSlot, MAX_CHAT_IGNORE, MAX_CLIENT_UI_BYTES,
    MAX_SETTINGS_BYTES, MAX_SHORTCUT_STRING, MAX_SKILL_JOB, PocketRecord, PocketsDoc, QuestRecord,
    QuestsDoc, QueueSlot, REGISTERED_SETTINGS, SETTING_CHAT_IGNORE, SETTING_CLIENT_UI,
    SETTING_PRIVACY_MESSENGER, SETTING_PRIVACY_MESSENGER_JOIN, SETTING_PRIVACY_PARTY,
    SETTING_PRIVACY_SHOUT, SETTING_PRIVACY_TRADE, SETTING_PRIVACY_WHISPER, SHORTCUT_KIND_CHAT,
    SM_TIMER_COUNT, SettingsDoc, ShortcutRecord, SkillSlot, SkillsDoc, SmTimersDoc, TaskbarDoc,
};
pub use item::{
    ITEM_FLAG_BINDS, ITEM_FLAG_EXPIRED, ITEM_FLAG_ISUSING, ItemRecord, MAX_ITEM_COUNT,
    MAX_ITEM_TEXT_BYTES, PetRecord, StoredItem,
};

/// The id newtypes of `flyff_common::ids` that persistence records use (Plan/13 §1.3 `model::ids`).
pub mod ids {
    pub use flyff_common::{
        AccountId, CharacterId, GuildId, GuildWarId, ItemSerial, MailId, SaveGen, UnixTime,
    };
}

pub use messaging::{
    FriendRecord, GrantKind, MAIL_KEEP_DAYS, MAX_MAIL_BODY_BYTES, MAX_MAIL_TITLE_BYTES,
    MAX_NOTE_BYTES, MailRecord, MailStateRecord, NoteRecord, PendingGrantRecord,
    PendingRemovalRecord, RemovalLocation, SYSTEM_SENDER,
};
pub use startup::{
    DirectoryEntry, MetaCounters, PartyNameRecord, RuntimeOverrideRecord, StartupState,
};

// ---- the social groups (0003 guilds, 0004 relations, 0005 siege; Plan/13 §8-§10) ----
pub use guild::{
    DEFAULT_RANK_POWERS, GUILD_ALIAS_MAX_BYTES, GUILD_LOG_TAIL, GUILD_LOGO_MAX, GUILD_MAX_LEVEL,
    GUILD_NAME_MAX_CHARS, GUILD_NOTICE_MAX_BYTES, GUILD_RANKS, GuildBankLogKind,
    GuildBankLogRecord, GuildBankLogTail, GuildLoad, GuildMemberRecord, GuildQuestRecord,
    GuildRecord, GuildWarRecord, RANK_MASTER, RANK_ROOKIE, WAR_RESULT_MAX, WarSideRecord,
};
pub use relations::{
    CampusLoad, CampusMemberRecord, CampusRole, CoupleProposalRecord, CoupleRecord,
};
pub use siege::{
    BidStatus, Gc1to1Load, Gc1to1TenderRecord, GuildSiegeBidRecord, GuildSiegePlayerRecord,
    GuildSiegeRecord, PlayerRewardStatus, SiegeLoad, SiegeRankRow, SiegeStatus, TenderState,
};

// ---- the content groups (0006 lord, tax and secret room; 0007 houses, rainbow race and quiz; Plan/13 §11-§12) ----
pub use houses::{
    GuildHouseFurnitureRecord, GuildHouseLoad, GuildHouseRecord, HousingFurnitureRecord,
    HousingLoad, QUIZ_TEXT_MAX_BYTES, QuizQuestionRecord, QuizType, RainbowEntryState,
    RainbowRaceEntryRecord, RainbowRaceLoad,
};
pub use realm::{
    ElectionState, LORD_PLEDGE_MAX_BYTES, LordCandidateRecord, LordElectionRecord, LordEventRecord,
    LordLoad, LordVoteRecord, SecretRoomLineup, SecretRoomLoad, SecretRoomTenderRecord,
    SecretRoomTenderState, TaxContinent, TaxDetailRecord, TaxKind, TaxLoad, TaxOwnerRecord,
};
