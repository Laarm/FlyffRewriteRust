//! `flyff-db` — tier N (Plan/13, D5): SQLite persistence. The **only** crate that links `rusqlite`
//! (rule X5); no tokio (rule X1), channels are `flume` (X7).
//!
//! Layout (Plan/13 §1.3):
//!
//! * [`config`] — `[database]`/`[save]`/`[backup]`/`[audit]`/`[retention]` ([`PersistenceConfig`], [`config::KEY_DOCS`]),
//! * [`conn`] — writer/reader connections and the pragmas of §2.2, [`lock`] — the instance lock
//!   `data/flyff.db.lock`, [`reader`] — the `flyff-persist-ro-*` threads,
//! * [`migrate`] — embedded migrations `0001_core.sql` ... `0008_admin.sql`, version guard,
//!   pre-migration backup ([`backup`]), counter repair, init-only values,
//! * [`model`] — pure data records and the JSON documents (inventory, bank, pockets, skills, quests, taskbar,
//!   buffs, SM timers, honor, settings, items),
//! * [`cmd`], [`ops`], [`handle`], [`persistence`] — the command/reply contract of Plan/01 §4.4 (`DbCmd`, `DbOp`,
//!   `AtomicBatch`, `DbReply`, `PersistHandle`, `open`),
//! * the persistence actor — the writer thread `flyff-persist` (batches, savepoints, generations, retry mode,
//!   poisoning, shutdown; `Plan/13 §16.4-§16.8`), [`flush`] (`wait_flushed`), [`fault`] (test hooks),
//! * [`repo`] (SQL per domain), [`queries`] (lobby/admin reads), [`startup`] ([`StartupState`]) and [`password`]
//!   (argon2id over the md5 hex),
//! * the social groups `0003`-`0005` (guilds, couples and campus, sieges): records in [`model`], SQL in
//!   [`repo::guilds`], [`repo::relations`] and [`repo::siege`], the batch constructors in [`batches_social`], the audit
//!   builders in [`audit::social`],
//! * the content groups `0006` (lord, tax, secret room) and `0007` (guild house, housing, rainbow race, quiz): records
//!   in [`model`], SQL in [`repo::realm`] and [`repo::houses`], the batch constructors in [`batches_content`], the audit
//!   builders in [`audit::content`] and the library behind `flyff quiz import` in [`quiz_import`],
//! * `testing` (feature `testing`) — `TempDb` and record fixtures.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

mod actor;
pub mod admin;
pub mod audit;
pub mod backup;
pub mod batches;
pub mod batches_content;
pub mod batches_social;
pub mod calendar;
pub mod check;
mod clock;
pub mod cmd;
pub mod config;
pub mod conn;
pub mod error;
pub mod fault;
pub mod flush;
pub mod handle;
pub mod lock;
pub mod maintenance;
pub mod migrate;
pub mod model;
pub mod ops;
pub mod password;
pub mod persistence;
pub mod queries;
pub mod quiz_import;
pub mod reader;
pub mod repo;
pub mod startup;

pub use admin::{
    AdminBookmarkRecord, AdminGrantRow, AdminNoteRecord, AdminOfflineEdit, AdminReadQuery,
    AdminReadResult, AdminRemovalRow, AdminScan, AdminTemplateRecord, ChunkLimits,
    IdempotencyDecision, IdempotencyRecord, IdempotencyState, IpBlockRecord, KeysetScan,
    LayerPolicy, NoteSubject, NoteSubjectKind, Page, PendingFilter, PendingState, ScanEvent,
    ScanStream, ScheduledNoticeRecord, StaffAccountRecord, TemplateKind, TotpRecord,
};
pub use audit::{AuditActor, AuditError, AuditEvent, AuditLog, AuditStats, AuditTap};
pub use cmd::{
    AtomicBatch, AtomicKind, AtomicOutcome, BackupKind, BatchReply, CheckpointMode, DbCmd, DbOp,
    DbReply, GameReadQuery, GameReadResult, MaintenanceJob, MaintenanceReport, ReadCmd,
    ShutdownMode, ShutdownReport, Ticket,
};
pub use config::{
    AuditConfig, BackupConfig, CheckOnStart, DbConfig, InitOnlyValues, KEY_DOCS, KeyDoc,
    PersistenceConfig, RetentionConfig, SaveConfig, Synchronous,
};
pub use error::{DbError, DbErrorKind};
pub use flush::FlushTracker;
pub use handle::{DbStatsSnapshot, PersistHandle};
pub use lock::InstanceLock;
pub use model::{
    AccountBanRecord, AccountListEntry, AccountListFilter, AccountRecord, BankDoc, BankLoad,
    BankSnapshot, BidStatus, BuffsDoc, CampusLoad, CampusMemberRecord, CampusRole, CharacterDocs,
    CharacterIdentity, CharacterListEntry, CharacterLoad, CharacterRow, CharacterSnapshot,
    CharacterState, ContainerRecord, CoupleProposalRecord, CoupleRecord, CreatedVia,
    DirectoryEntry, ElectionState, FriendRecord, Gc1to1Load, Gc1to1TenderRecord, GrantKind,
    GuildBankDoc, GuildBankLogKind, GuildBankLogRecord, GuildBankLogTail,
    GuildHouseFurnitureRecord, GuildHouseLoad, GuildHouseRecord, GuildLoad, GuildMemberRecord,
    GuildQuestRecord, GuildRecord, GuildSiegeBidRecord, GuildSiegePlayerRecord, GuildSiegeRecord,
    GuildWarRecord, HonorDoc, HousingFurnitureRecord, HousingLoad, InventoryDoc, ItemRecord,
    JsonDoc, LordCandidateRecord, LordElectionRecord, LordEventRecord, LordLoad, LordVoteRecord,
    MailRecord, MailStateRecord, MetaCounters, NewAccountRecord, NewCharacterRecord, NoteRecord,
    PartyNameRecord, PendingGrantRecord, PendingRemovalRecord, PetRecord, PlayerRewardStatus,
    PocketsDoc, QuestsDoc, QuizQuestionRecord, QuizType, RainbowEntryState, RainbowRaceEntryRecord,
    RainbowRaceLoad, RemovalLocation, RepairNote, RuntimeOverrideRecord, SYSTEM_SENDER, SaveReason,
    SecretRoomLineup, SecretRoomLoad, SecretRoomTenderRecord, SecretRoomTenderState, SettingsDoc,
    SiegeLoad, SiegeRankRow, SiegeStatus, SkillsDoc, SmTimersDoc, StartupState, StoredItem,
    TaskbarDoc, TaxContinent, TaxDetailRecord, TaxKind, TaxLoad, TaxOwnerRecord, TenderState,
    WarSideRecord,
};
pub use persistence::{OpenOptions, OpenOutput, Persistence, open, open_with};
