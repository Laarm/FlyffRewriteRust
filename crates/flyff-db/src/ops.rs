//! Atomic batches and `DbOp` (Plan/13 §16.2, §16.3, §18): the types of every write the persistence actor
//! performs, and the dispatch that applies one op inside the writer's batch transaction.
//!
//! The enums are `#[non_exhaustive]`: variants that belong to a migration group (messaging, guilds, siege,
//! realm, houses) are added by the work packages that create those groups (WP-DB-10..15; the guild, relations,
//! siege, lord/tax/secret-room and house/event groups of `0003`-`0007` are all in). The ops of the core schema
//! (`0001`: accounts, characters, banks, counters, `server_kv`, `runtime_override`) live here and in
//! [`crate::repo`].

use crate::admin::model::{
    AdminBookmarkRecord, AdminNoteRecord, AdminTemplateRecord, IdempotencyDecision,
    IdempotencyState, IpBlockRecord, ScheduledNoticeRecord,
};
use crate::admin::repo as admin_repo;
use crate::error::DbError;
use crate::model::{
    AccountBanRecord, CampusMemberRecord, CharacterSnapshot, CoupleRecord, FriendRecord,
    Gc1to1TenderRecord, GuildBankDoc, GuildBankLogRecord, GuildHouseFurnitureRecord,
    GuildHouseRecord, GuildMemberRecord, GuildQuestRecord, GuildRecord, GuildSiegeBidRecord,
    GuildSiegePlayerRecord, GuildSiegeRecord, GuildWarRecord, HousingFurnitureRecord,
    LordCandidateRecord, LordElectionRecord, LordEventRecord, LordVoteRecord, MailRecord,
    MailStateRecord, NewAccountRecord, NewCharacterRecord, NoteRecord, PendingGrantRecord,
    PendingRemovalRecord, QuizQuestionRecord, QuizType, RainbowRaceEntryRecord,
    SecretRoomTenderRecord, TaxDetailRecord, TaxOwnerRecord,
};
use crate::repo::snapshot::DocHashes;
use crate::repo::{
    accounts, banks, characters, guilds, houses, messaging, meta, party_names, realm, relations,
    siege,
};
use flyff_common::rt::Reply;
use flyff_common::{AccountId, CharacterId, GuildId, SaveGen, UnixTime};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Correlates a game-thread request with its `DbReply` (allocated by the game thread).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct Ticket(pub u64);

/// What an atomic batch produced.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AtomicOutcome {
    /// Done, nothing to report.
    Done,
    /// A character was created (`CreateCharacter`).
    CharacterCreated(CharacterId),
    /// An account was created (`CreateAccount`).
    AccountCreated(AccountId),
    /// The first row the batch inserted with a database-allocated id: a note, a pending grant or a pending
    /// removal (`InsertNote`, `InsertPendingGrant`, `InsertPendingRemoval`).
    RowInserted(i64),
    /// How many rows a bulk op changed (`LiftAccountBansOf`).
    Lifted(u32),
    /// How many rows a conditional or keyed admin op changed (Plan/25 §11.6): `CancelPendingGrant` /
    /// `CancelPendingRemoval` report `0` when the row was delivered, applied, cancelled before or unknown; the
    /// update/delete ops of the admin tables report `0` for an unknown id.
    Changed(u32),
    /// The decision of `IdempotencyBegin`.
    Idempotency(IdempotencyDecision),
}

/// Who is told when a batch committed.
pub enum BatchReply {
    /// Nobody.
    None,
    /// The game thread (via `DbReply::Committed` / `Failed`).
    Game(Ticket),
    /// A lobby/admin caller waiting on a one-shot.
    Lobby(Reply<Result<AtomicOutcome, DbError>>),
}

impl BatchReply {
    /// `true` when a caller is blocked on the answer (the writer then ends its drain window early).
    pub fn is_waited_on(&self) -> bool {
        matches!(self, BatchReply::Lobby(_))
    }
}

macro_rules! atomic_kinds {
    ($($variant:ident),+ $(,)?) => {
        /// Kind of an atomic batch (metrics, logs, test assertions; Plan/13 §16.2). Every row of the §18 table
        /// has one variant; `Other` names ad-hoc batches (`PersistHandle::op` uses `Other("op")`).
        #[derive(Clone, Debug, PartialEq, Eq, Serialize)]
        #[non_exhaustive]
        pub enum AtomicKind {
            $(
                #[doc = concat!("`", stringify!($variant), "` (Plan/13 §18).")]
                $variant,
            )+
            /// Anything else.
            Other(&'static str),
        }

        impl AtomicKind {
            /// Every named kind, in the order of Plan/13 §16.2.
            pub const NAMED: &'static [AtomicKind] = &[$(AtomicKind::$variant),+];

            /// A stable label for logs and metrics (`Trade`, `MailSend`, ..., or the `Other` text).
            pub fn label(&self) -> &'static str {
                match self {
                    $(AtomicKind::$variant => stringify!($variant),)+
                    AtomicKind::Other(label) => label,
                }
            }
        }
    };
}

atomic_kinds!(
    Trade,
    VendorPurchase,
    MailSend,
    MailTakeItem,
    MailTakeGold,
    GuildBankPutItem,
    GuildBankTakeItem,
    GuildBankTakeGold,
    GuildContribution,
    GuildCloak,
    GuildCreate,
    GuildDisband,
    GuildSalary,
    GrantDelivery,
    RemovalApply,
    CharacterCreate,
    CharacterDelete,
    CharacterRename,
    CharacterSeal,
    CharacterUnseal,
    GuildRename,
    SiegeBid,
    SiegeCancel,
    SiegeResult,
    SiegeClaim,
    Gc1to1Tender,
    Gc1to1Cancel,
    Gc1to1Refund,
    LordDeposit,
    LordPledge,
    LordVote,
    LordEventCreate,
    LordElectionEnd,
    SecretRoomTender,
    SecretRoomCancel,
    SecretRoomResult,
    TaxPayout,
    RainbowApply,
    RainbowResult,
    GuildHouseBuy,
    GuildHouseUpkeep,
    GuildHouseFurnitureAdd,
    HousingFurnitureAdd,
    QuizPrize,
    AdminGrant,
    AdminItemRemoval,
    AdminGold,
    AdminGuildCreate,
    AdminGuildEdit,
    AdminOfflineEdit,
);

/// Non-snapshot writes (Plan/13 §16.3, canonical list). The core groups are implemented here; the groups of
/// later migrations are added by their work packages.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub enum DbOp {
    // ---- accounts (lobby/CLI) ----
    /// Creates an account -> `AccountCreated(id)` or `Err(AccountExists)`.
    CreateAccount(NewAccountRecord),
    /// Replaces the password hash (PHC string).
    SetAccountPassword {
        /// Account.
        account_id: AccountId,
        /// New argon2id PHC string.
        password_hash: String,
    },
    /// Sets `accounts.authority`.
    SetAccountAuthority {
        /// Account.
        account_id: AccountId,
        /// ASCII letter.
        authority: u8,
    },
    /// Records a successful login.
    RecordLogin {
        /// Account.
        account_id: AccountId,
        /// Time of the login.
        at: UnixTime,
        /// Peer address text.
        ip: String,
    },
    /// Inserts an account ban.
    InsertAccountBan(AccountBanRecord),
    /// Lifts one ban.
    LiftAccountBan {
        /// `account_bans.id`.
        ban_id: i64,
        /// When.
        at: UnixTime,
        /// By whom (`cli`, `gm:<account>`).
        by: String,
    },
    /// Lifts every ban of the account that is active at `at` (`/unban`, Plan/23 N09) -> `Lifted(n)`; no active
    /// ban is `Lifted(0)`, not an error.
    LiftAccountBansOf {
        /// The account.
        account_id: AccountId,
        /// When (also the instant the activity of a ban is judged at).
        at: UnixTime,
        /// By whom (`gm:<account>`).
        by: String,
    },
    // ---- characters (identity/admin-owned columns only) ----
    /// Creates a character with its documents and bank -> `CharacterCreated(id)` or
    /// `Err(NameTaken | SlotTaken)`; the id comes from `meta.next_character_id` in the same transaction.
    CreateCharacter(Box<NewCharacterRecord>),
    /// Soft delete (`state = 1`): frees the name and the slot at once (PL-057).
    SoftDeleteCharacter {
        /// Character.
        character_id: CharacterId,
        /// Deletion time.
        at: UnixTime,
    },
    /// Renames a character -> `Err(NameTaken)`.
    RenameCharacter {
        /// Character.
        character_id: CharacterId,
        /// New name.
        name: String,
    },
    /// Sets or clears `login_blocked_until`.
    SetCharacterLoginBlock {
        /// Character.
        character_id: CharacterId,
        /// Block end, `None` = unblocked.
        until: Option<UnixTime>,
    },
    /// Sets `characters.authority`.
    SetCharacterAuthority {
        /// Character.
        character_id: CharacterId,
        /// ASCII letter.
        authority: u8,
    },
    /// Seals a character (keeps the name, frees the slot).
    SealCharacter {
        /// Character.
        character_id: CharacterId,
        /// Seal time.
        at: UnixTime,
    },
    /// Unseals a character into a free slot of an account -> `Err(SlotTaken)`.
    UnsealCharacter {
        /// Character.
        character_id: CharacterId,
        /// Receiving account.
        account_id: AccountId,
        /// Receiving slot.
        slot: u8,
    },
    /// Hard-deletes soft-deleted characters (cascades) and the couples/campuses left without members.
    PurgeCharacters {
        /// The characters to purge.
        ids: Vec<CharacterId>,
    },
    /// Changes the bank PIN (four ASCII digits).
    SetBankPin {
        /// Character.
        character_id: CharacterId,
        /// New PIN.
        pin: [u8; 4],
    },
    // ---- messaging group (0002): the stored advanced-party names (Plan/21 §4.4) ----
    /// Stores the advanced-party name of a leader, replacing his previous one -> `Err(PartyNameTaken)`.
    SetPartyName {
        /// The leader.
        character_id: CharacterId,
        /// The name (unique case-insensitively).
        name: String,
    },
    /// Forgets the stored party name of a character.
    DeletePartyName {
        /// The character.
        character_id: CharacterId,
    },
    // ---- messaging group (0002): friends, notes, mails, grants, removals (Plan/13 §7, §16.3) ----
    /// Inserts or updates one `friends` row (a friendship is two rows: send both directions in one batch).
    /// `NotFound` when a character does not exist, `Conflict` for a self-friendship.
    UpsertFriend(FriendRecord),
    /// Deletes both rows of the friendship between `a` and `b`.
    DeleteFriendPair {
        /// One side.
        a: CharacterId,
        /// The other side.
        b: CharacterId,
    },
    /// Deletes every friend row where the character is the owner or the friend (delete / seal of a character).
    DeleteAllFriendsOf {
        /// The character.
        character_id: CharacterId,
    },
    /// Leaves an offline note -> `RowInserted(note id)`.
    InsertNote(NoteRecord),
    /// Marks the undelivered notes of `to_id` with `id <= up_to_id` delivered (after the join succeeded).
    MarkNotesDelivered {
        /// The receiver.
        to_id: CharacterId,
        /// Highest note id that was shown.
        up_to_id: i64,
        /// Delivery time.
        at: UnixTime,
    },
    /// Inserts a mail with the id the `MailService` allocated; pair it with `RaiseCounter(next_mail_id)`
    /// (`Conflict` for a used id, `NotFound` for an unknown receiver).
    InsertMail(MailRecord),
    /// Applies the state transitions of one mail: read, take gold, take item, delete (see [`MailStateRecord`];
    /// a take whose attachment is already gone is a `Conflict` that rolls the whole batch back).
    UpdateMail(MailStateRecord),
    /// Soft-deletes every undeleted mail sent before `created_before` (the expiry sweep of the mail service,
    /// attachments lost; idempotent).
    ExpireMails {
        /// Mails with `created_at` before this are expired.
        created_before: UnixTime,
        /// Deletion time.
        at: UnixTime,
    },
    /// Queues an item or gold grant for a character -> `RowInserted(grant id)`.
    InsertPendingGrant(PendingGrantRecord),
    /// Closes delivered grants. An unknown or already delivered id is logged as CRITICAL and does not roll the
    /// batch back (the grant was delivered in memory already).
    MarkGrantsDelivered {
        /// `pending_grants.id`s.
        ids: Vec<i64>,
        /// Delivery time.
        at: UnixTime,
    },
    /// Queues a GM removal of an item or penya -> `RowInserted(removal id)`.
    InsertPendingRemoval(PendingRemovalRecord),
    /// Records the applied removals `(id, removed_count)` (same leniency as `MarkGrantsDelivered`).
    MarkRemovalsDone {
        /// `(pending_removals.id, what was actually removed)`.
        done: Vec<(i64, u32)>,
        /// Application time.
        at: UnixTime,
    },
    // ---- admin group (0008, Plan/25 §11.6) ----
    /// Sets the position of an active character (offline unstuck; only from the game thread under an admin
    /// hold): world, layer 0, coordinates -> `Done` / `NotFound` / `Conflict` (sealed).
    SetCharacterPosition {
        /// The character.
        character_id: CharacterId,
        /// Target world.
        world_id: u32,
        /// Target position.
        pos: [f32; 3],
    },
    /// Restores a soft-deleted character into `slot` (the CLI's name and slot checks; only from the game thread
    /// under an admin hold) -> `NotFound` / `Conflict` / `NameTaken` / `SlotTaken`.
    UndeleteCharacter {
        /// The character.
        character_id: CharacterId,
        /// The slot it returns to.
        slot: u8,
    },
    /// Cancels a pending grant that is neither delivered nor cancelled (conditional update, stamped with the
    /// writer's clock) -> `Changed(1)`, or `Changed(0)` when it was delivered, cancelled or does not exist.
    CancelPendingGrant {
        /// `pending_grants.id`.
        id: i64,
        /// The audit actor.
        by: String,
    },
    /// Cancels a pending removal that is neither applied nor cancelled -> `Changed(0|1)` like
    /// [`DbOp::CancelPendingGrant`].
    CancelPendingRemoval {
        /// `pending_removals.id`.
        id: i64,
        /// The audit actor.
        by: String,
    },
    /// Inserts a shared bookmark -> `RowInserted(id)`.
    InsertAdminBookmark(AdminBookmarkRecord),
    /// Updates a shared bookmark by `id` -> `Changed(n)`.
    UpdateAdminBookmark(AdminBookmarkRecord),
    /// Deletes a shared bookmark -> `Changed(n)`.
    DeleteAdminBookmark {
        /// `admin_bookmarks.id`.
        id: i64,
    },
    /// Inserts a template or kit -> `RowInserted(id)` / `Conflict` when `(kind, name)` exists.
    InsertAdminTemplate(AdminTemplateRecord),
    /// Updates name and spec of a template -> `Changed(n)` / `Conflict`.
    UpdateAdminTemplate(AdminTemplateRecord),
    /// Deletes a template -> `Changed(n)`.
    DeleteAdminTemplate {
        /// `admin_templates.id`.
        id: i64,
    },
    /// Inserts a moderation note -> `RowInserted(id)`.
    InsertAdminNote(AdminNoteRecord),
    /// Pins or unpins a note -> `Changed(n)`.
    SetAdminNotePinned {
        /// `admin_notes.id`.
        id: i64,
        /// New state.
        pinned: bool,
    },
    /// Deletes a note -> `Changed(n)`.
    DeleteAdminNote {
        /// `admin_notes.id`.
        id: i64,
    },
    /// Inserts a scheduled notice -> `RowInserted(id)`.
    InsertScheduledNotice(ScheduledNoticeRecord),
    /// Updates a scheduled notice (not `last_fired_at`) -> `Changed(n)`.
    UpdateScheduledNotice(ScheduledNoticeRecord),
    /// Deletes a scheduled notice -> `Changed(n)`.
    DeleteScheduledNotice {
        /// `scheduled_notices.id`.
        id: i64,
    },
    /// Stamps `last_fired_at` of a scheduled notice (the notice job, one-minute granularity) -> `Changed(n)`.
    NoticeFired {
        /// `scheduled_notices.id`.
        id: i64,
        /// When it fired.
        at: UnixTime,
    },
    /// Inserts an IP block (the CIDR is normalised) -> `RowInserted(id)` / `Conflict` / `Invalid`.
    InsertIpBlock(IpBlockRecord),
    /// Deletes an IP block -> `Changed(n)`.
    DeleteIpBlock {
        /// `admin_ip_blocks.id`.
        id: i64,
    },
    /// Stores or replaces the TOTP secret of a staff account -> `Done` / `NotFound`.
    UpsertTotp {
        /// The account.
        account_id: AccountId,
        /// The raw secret (1..=128 bytes; scrubbed from logs).
        secret: Vec<u8>,
        /// Enrolment time.
        enabled_at: UnixTime,
    },
    /// Removes the TOTP secret of an account -> `Changed(n)`.
    DeleteTotp {
        /// The account.
        account_id: AccountId,
    },
    /// Sets or clears `accounts.must_change_password` -> `Done` / `NotFound`.
    SetMustChangePassword {
        /// The account.
        account_id: AccountId,
        /// New value.
        value: bool,
    },
    /// Registers an admin request before it is dispatched -> `Idempotency(decision)`
    /// ([`crate::admin::idempotency`]).
    IdempotencyBegin {
        /// The `Idempotency-Key` (1..=128 printable ASCII).
        key: String,
        /// The audit actor.
        actor: String,
        /// Method and path.
        route: String,
        /// SHA-256 of the request body.
        body_sha256: [u8; 32],
        /// Time of the request.
        at: UnixTime,
    },
    /// Closes an in-flight request as `Done` (with the response body to replay) or `Failed` -> `Changed(n)`.
    IdempotencyFinish {
        /// The key.
        key: String,
        /// `Done` or `Failed`.
        state: IdempotencyState,
        /// The response body of a `Done` request (at most 1 MiB).
        result: Option<String>,
    },
    // ---- guild group (0003, Plan/13 §8, §16.3; Plan/21 §5-§6) ----
    /// Inserts a guild or rewrites every column except `created_at` -> `GuildNameTaken` (case-insensitive, `''`
    /// exempt), `Invalid` (level outside 1..=50, name over 16 characters, notice over 127 bytes, logo over 999).
    UpsertGuild(GuildRecord),
    /// Deletes a guild; members, bank, log, quests and the house cascade, war and siege rows survive. A missing
    /// guild is not an error.
    DeleteGuild {
        /// The guild.
        guild_id: GuildId,
    },
    /// Inserts a member or rewrites rank, class, alias, contributions and war id -> `Conflict` for a character of
    /// another guild or a second rank-0 member, `NotFound`, `Invalid` (rank, class, alias over 12 bytes).
    UpsertGuildMember(GuildMemberRecord),
    /// Removes a character from its guild (idempotent).
    DeleteGuildMember {
        /// The member.
        character_id: CharacterId,
    },
    /// Replaces the 42-slot bank document of a guild -> `NotFound`, `Invalid` for an invalid container.
    SetGuildBank {
        /// The guild.
        guild_id: GuildId,
        /// The bank.
        doc: GuildBankDoc,
    },
    /// Appends one bank log row -> `RowInserted(id)`, `NotFound` for an unknown guild.
    InsertGuildBankLog(GuildBankLogRecord),
    /// Inserts or updates a guild war with the exact `WR_*` code -> `Conflict` when immutable columns change or a
    /// finished war is rewritten differently, `Invalid`.
    UpsertGuildWar(GuildWarRecord),
    /// Additive: sets `guild_members.war_id` of every member of the two guilds (war start sets the id, the end sets
    /// 0) -> `Changed(rows)`.
    SetGuildWarOfMembers {
        /// The declaring and the accepting guild.
        guilds: [GuildId; 2],
        /// The war id, 0 = none.
        war_id: u32,
    },
    /// Stores (`Some`) or deletes (`None`) the guild rejoin cooldown of a character -> `NotFound`.
    SetGuildRejoinCooldown {
        /// The character.
        character_id: CharacterId,
        /// End of the cooldown.
        until: Option<UnixTime>,
    },
    /// Inserts or updates a guild quest row (one per guild and quest).
    UpsertGuildQuest(GuildQuestRecord),
    /// Deletes a guild quest row.
    DeleteGuildQuest {
        /// The guild.
        guild_id: GuildId,
        /// The quest.
        quest_id: u16,
    },
    // ---- couples and campus (0004, Plan/13 §9; Plan/21 §11-§12) ----
    /// Inserts the couple and both member rows, or updates the experience of the stored couple -> `Conflict` for a
    /// character that is already coupled or other members, `Invalid` for the same character twice, `NotFound`.
    UpsertCouple(CoupleRecord),
    /// Deletes a couple with its member rows.
    DeleteCouple {
        /// `couples.id`.
        couple_id: u32,
    },
    /// Stores (`Some`) or deletes (`None`) the proposal cooldown of a proposer -> `NotFound`.
    SetCoupleProposal {
        /// The proposer.
        proposer_id: CharacterId,
        /// End of the cooldown.
        cooldown_until: Option<UnixTime>,
    },
    /// Creates a campus (insert or ignore).
    UpsertCampus {
        /// `campuses.id`.
        campus_id: u32,
        /// Creation time.
        created_at: UnixTime,
    },
    /// Deletes a campus with its member rows.
    DeleteCampus {
        /// `campuses.id`.
        campus_id: u32,
    },
    /// Adds a campus member or changes a role inside the same campus -> `Conflict` for another campus or a second
    /// master, `NotFound`.
    UpsertCampusMember(CampusMemberRecord),
    /// Removes a character from its campus (idempotent).
    DeleteCampusMember {
        /// The member.
        character_id: CharacterId,
    },
    /// Sets the campus point balance of a character (`campus_points` only; `characters` and `character_docs` are
    /// never touched, so an offline master can be rewarded) -> `NotFound`.
    SetCampusPoints {
        /// The character.
        character_id: CharacterId,
        /// New balance (may be negative).
        points: i32,
    },
    // ---- siege and GC 1:1 (0005, Plan/13 §10; Plan/21 §7-§8) ----
    /// Inserts or rewrites a siege row; the status only moves forward -> `Conflict`.
    UpsertGuildSiege(GuildSiegeRecord),
    /// Inserts or rewrites a bid under the transition guards of [`crate::repo::siege`] -> `Conflict` (a second
    /// cancel or claim, a bid that is not higher), `NotFound` for an unknown siege.
    UpsertGuildSiegeBid(GuildSiegeBidRecord),
    /// Inserts or rewrites a line-up row; rewards in status 31/32 are final -> `Conflict`.
    UpsertGuildSiegePlayer(GuildSiegePlayerRecord),
    /// Inserts or rewrites a GC 1:1 tender under the guards of 21 §8.3 (plus `C -> T`) -> `Conflict`.
    UpsertGc1to1Tender(Gc1to1TenderRecord),
    /// Replaces the GC 1:1 line-up of a guild in a round (`seq` 0..n) -> `Invalid` for a character twice.
    ReplaceGc1to1Lineup {
        /// The round.
        round: u32,
        /// The guild.
        guild_id: GuildId,
        /// The fighters in fight order.
        members: Vec<CharacterId>,
    },
    /// Additive (`uspExpireCombatReward`): bids and players in status 30 of sieges older than `before_siege_id`
    /// become 32 -> `Changed(rows)`.
    ExpireSiegeRewards {
        /// Sieges with a smaller id are expired.
        before_siege_id: u32,
        /// Expiry time (stored in `rewarded_at`).
        at: UnixTime,
    },
    // ---- lord, tax and secret room (0006, Plan/13 §11, §16.3; Plan/22 §9-§11) ----
    /// Inserts an election or rewrites its state, begin, requirement, winner and end. The state only moves forward
    /// (`0 -> 0..=3`, `1 -> 1..=3`, `2 -> 2/3`) and state 3 is final, not even an identical rewrite -> `Conflict`
    /// (so a second `LordElectionEnd` is impossible); `Invalid` for `lord_id` or `ended_at` outside state 3 and a
    /// negative `begin_at` or requirement.
    UpsertLordElection(LordElectionRecord),
    /// Inserts a candidate or rewrites deposit, votes, pledge and `left_out` -> `NotFound` (unknown election),
    /// `Conflict` (deposit or votes would decrease, `created_at` changes, the stored row is left out, the election
    /// has ended), `Invalid` (pledge over 255 bytes, negative deposit or votes).
    UpsertLordCandidate(LordCandidateRecord),
    /// One vote per (election, elector) -> `Conflict` for a second vote or an expired election, `NotFound`.
    InsertLordVote(LordVoteRecord),
    /// A new lord event of a creator, or the minute tick of the running one (`ticks_left` only, strictly lower)
    /// -> `Conflict` (a second create, a raise), `Invalid` (negative ticks, negative or non-finite factors).
    UpsertLordEvent(LordEventRecord),
    /// Removes the lord event of a creator (a row reaching 0 ticks); idempotent.
    DeleteLordEvent {
        /// The creator.
        character_id: CharacterId,
    },
    /// Removes every lord event (`/LEventInitialize`) -> `Changed(n)`.
    ClearLordEvents,
    /// Upserts the cooldown of a lord skill in minutes (`0` = ready) -> `Invalid` below 0.
    SetLordSkillCooldown {
        /// The skill.
        skill_id: u32,
        /// Minutes left.
        minutes: i32,
    },
    /// Whole-row upsert of a tax owner by (round, continent); no guard (the payout guard is the
    /// `tax.last_paid_on` date of the payout batch).
    UpsertTaxOwner(TaxOwnerRecord),
    /// Whole-row upsert of a tax detail by (round, continent, kind) -> `Invalid` (West + Admission, a rate outside
    /// 0..=100, a negative counter).
    UpsertTaxDetail(TaxDetailRecord),
    /// Inserts a secret-room tender or rewrites it under the guards of [`crate::repo::realm`] (a new row only as
    /// `T`, `T -> T` only higher, `C -> T`, `F -> T`, no second `C -> C` / `F -> F` refund) -> `Conflict`, `Invalid`
    /// (continent All, penya of 0 or less).
    UpsertSecretRoomTender(SecretRoomTenderRecord),
    /// Replaces the stored line-up of a guild in a round and side (an empty list deletes it) -> `Invalid` for a
    /// character twice or a continent other than 15 and 240 (nothing is deleted then).
    ReplaceSecretRoomLineup {
        /// The round.
        round: u32,
        /// The side: 15 (East) or 240 (West).
        continent: u8,
        /// The guild.
        guild_id: GuildId,
        /// The members (stored without an order).
        members: Vec<CharacterId>,
    },
    // ---- guild house, housing, rainbow race, quiz (0007, Plan/13 §12, §16.3; Plan/22 §12-§15) ----
    /// Inserts a guild house or rewrites world and `upkeep_until` (`created_at` is immutable: a buy over an
    /// existing house is `Conflict` and its charge rolls back) -> `NotFound` (unknown guild), `Invalid` (negative
    /// upkeep).
    UpsertGuildHouse(GuildHouseRecord),
    /// Inserts a piece of guild furniture or rewrites its placement and keep time (`guild_id`, `item_id`,
    /// `listed_at` are immutable per `seq`) -> `NotFound` (no house), `Conflict`, `Invalid` (seq 0, non-finite).
    UpsertGuildHouseFurniture(GuildHouseFurnitureRecord),
    /// Removes a piece of guild furniture; idempotent.
    DeleteGuildHouseFurniture {
        /// `guild_house_furniture.seq`.
        seq: u32,
    },
    /// Setup, remove and move of a housing piece (`listed_at` and `keep_until` are immutable) -> `NotFound`
    /// (unknown character), `Conflict`, `Invalid` (non-finite).
    UpsertHousingFurniture(HousingFurnitureRecord),
    /// Additive: the strict insert of the list-up batch -> `Conflict` when the owner already lists the item (the
    /// consumed item rolls back), `NotFound`, `Invalid`.
    InsertHousingFurniture(HousingFurnitureRecord),
    /// Removes a housing piece; idempotent.
    DeleteHousingFurniture {
        /// The owner.
        character_id: CharacterId,
        /// The item id.
        item_id: u32,
    },
    /// `allowed` inserts a visitor of a room, otherwise deletes it (both idempotent) -> `Invalid` (owner ==
    /// visitor), `NotFound` (unknown character).
    SetHousingVisitor {
        /// The room owner.
        owner_id: CharacterId,
        /// The visitor.
        visitor_id: CharacterId,
        /// Allowed or not.
        allowed: bool,
    },
    /// Additive: the character stops being a visitor of every room (character deletion) -> `Changed(n)`.
    DeleteHousingVisitsOf {
        /// The deleted character.
        character_id: CharacterId,
    },
    /// Inserts a rainbow-race entry or rewrites it under the guards of [`crate::repo::houses`] (a new row only as
    /// `N`, `N -> R/F/L`, `R -> W/L/N`, `W`/`L`/`F` final, no second application) -> `Conflict`, `Invalid`
    /// (ranking of 0 or less on `R`/`W`, a ranking on `N`/`L`/`F`).
    UpsertRainbowRaceEntry(RainbowRaceEntryRecord),
    /// Validates a quiz question and upserts it: `id == 0` is a natural-key upsert on `(quiz_type, question)`
    /// (`RowInserted(id)` for a new row, `Changed(1)` when an existing one was rewritten, `Changed(0)` when it
    /// already had the values), `id > 0` updates that row (`NotFound`, `Conflict`) -> `Invalid`.
    UpsertQuizQuestion(QuizQuestionRecord),
    /// Stamps `used_at` on the given questions (unknown ids ignored) -> `Changed(n)`.
    MarkQuizQuestionsUsed {
        /// `quiz_questions.id`s.
        ids: Vec<i64>,
        /// When they were asked.
        at: UnixTime,
    },
    /// Additive (`flyff quiz import --replace`): deletes the questions of one type that are not in `keep` ->
    /// `Changed(n)`.
    PruneQuizQuestions {
        /// The type to prune; the other type is untouched.
        quiz_type: QuizType,
        /// The question texts that stay.
        keep: Vec<String>,
    },
    // ---- misc ----
    /// Upserts a GM/admin runtime override.
    SetRuntimeOverride {
        /// Dotted key.
        key: String,
        /// Value.
        value: serde_json::Value,
        /// Who.
        by: String,
        /// When.
        at: UnixTime,
    },
    /// Deletes a runtime override.
    DeleteRuntimeOverride {
        /// Dotted key.
        key: String,
    },
    /// Upserts a `server_kv` value (`updated_at` = commit time).
    SetServerKv {
        /// Key.
        key: String,
        /// Value.
        value: serde_json::Value,
    },
    /// `UPDATE meta SET value = MAX(value, at_least)`: every op that creates a row with an allocated id also
    /// raises the counter above it (Plan/13 §15.1).
    RaiseCounter {
        /// One of [`crate::repo::meta::COUNTER_KEYS`].
        key: &'static str,
        /// The counter becomes at least this value.
        at_least: i64,
    },
}

impl DbOp {
    /// The variant name (fault probes, logs, the failed-command dump).
    pub fn name(&self) -> &'static str {
        match self {
            DbOp::CreateAccount(_) => "CreateAccount",
            DbOp::SetAccountPassword { .. } => "SetAccountPassword",
            DbOp::SetAccountAuthority { .. } => "SetAccountAuthority",
            DbOp::RecordLogin { .. } => "RecordLogin",
            DbOp::InsertAccountBan(_) => "InsertAccountBan",
            DbOp::LiftAccountBan { .. } => "LiftAccountBan",
            DbOp::LiftAccountBansOf { .. } => "LiftAccountBansOf",
            DbOp::CreateCharacter(_) => "CreateCharacter",
            DbOp::SoftDeleteCharacter { .. } => "SoftDeleteCharacter",
            DbOp::RenameCharacter { .. } => "RenameCharacter",
            DbOp::SetCharacterLoginBlock { .. } => "SetCharacterLoginBlock",
            DbOp::SetCharacterAuthority { .. } => "SetCharacterAuthority",
            DbOp::SealCharacter { .. } => "SealCharacter",
            DbOp::UnsealCharacter { .. } => "UnsealCharacter",
            DbOp::PurgeCharacters { .. } => "PurgeCharacters",
            DbOp::SetBankPin { .. } => "SetBankPin",
            DbOp::SetPartyName { .. } => "SetPartyName",
            DbOp::DeletePartyName { .. } => "DeletePartyName",
            DbOp::UpsertFriend(_) => "UpsertFriend",
            DbOp::DeleteFriendPair { .. } => "DeleteFriendPair",
            DbOp::DeleteAllFriendsOf { .. } => "DeleteAllFriendsOf",
            DbOp::InsertNote(_) => "InsertNote",
            DbOp::MarkNotesDelivered { .. } => "MarkNotesDelivered",
            DbOp::InsertMail(_) => "InsertMail",
            DbOp::UpdateMail(_) => "UpdateMail",
            DbOp::ExpireMails { .. } => "ExpireMails",
            DbOp::InsertPendingGrant(_) => "InsertPendingGrant",
            DbOp::MarkGrantsDelivered { .. } => "MarkGrantsDelivered",
            DbOp::InsertPendingRemoval(_) => "InsertPendingRemoval",
            DbOp::MarkRemovalsDone { .. } => "MarkRemovalsDone",
            DbOp::SetCharacterPosition { .. } => "SetCharacterPosition",
            DbOp::UndeleteCharacter { .. } => "UndeleteCharacter",
            DbOp::CancelPendingGrant { .. } => "CancelPendingGrant",
            DbOp::CancelPendingRemoval { .. } => "CancelPendingRemoval",
            DbOp::InsertAdminBookmark(_) => "InsertAdminBookmark",
            DbOp::UpdateAdminBookmark(_) => "UpdateAdminBookmark",
            DbOp::DeleteAdminBookmark { .. } => "DeleteAdminBookmark",
            DbOp::InsertAdminTemplate(_) => "InsertAdminTemplate",
            DbOp::UpdateAdminTemplate(_) => "UpdateAdminTemplate",
            DbOp::DeleteAdminTemplate { .. } => "DeleteAdminTemplate",
            DbOp::InsertAdminNote(_) => "InsertAdminNote",
            DbOp::SetAdminNotePinned { .. } => "SetAdminNotePinned",
            DbOp::DeleteAdminNote { .. } => "DeleteAdminNote",
            DbOp::InsertScheduledNotice(_) => "InsertScheduledNotice",
            DbOp::UpdateScheduledNotice(_) => "UpdateScheduledNotice",
            DbOp::DeleteScheduledNotice { .. } => "DeleteScheduledNotice",
            DbOp::NoticeFired { .. } => "NoticeFired",
            DbOp::InsertIpBlock(_) => "InsertIpBlock",
            DbOp::DeleteIpBlock { .. } => "DeleteIpBlock",
            DbOp::UpsertTotp { .. } => "UpsertTotp",
            DbOp::DeleteTotp { .. } => "DeleteTotp",
            DbOp::SetMustChangePassword { .. } => "SetMustChangePassword",
            DbOp::IdempotencyBegin { .. } => "IdempotencyBegin",
            DbOp::IdempotencyFinish { .. } => "IdempotencyFinish",
            DbOp::UpsertGuild(_) => "UpsertGuild",
            DbOp::DeleteGuild { .. } => "DeleteGuild",
            DbOp::UpsertGuildMember(_) => "UpsertGuildMember",
            DbOp::DeleteGuildMember { .. } => "DeleteGuildMember",
            DbOp::SetGuildBank { .. } => "SetGuildBank",
            DbOp::InsertGuildBankLog(_) => "InsertGuildBankLog",
            DbOp::UpsertGuildWar(_) => "UpsertGuildWar",
            DbOp::SetGuildWarOfMembers { .. } => "SetGuildWarOfMembers",
            DbOp::SetGuildRejoinCooldown { .. } => "SetGuildRejoinCooldown",
            DbOp::UpsertGuildQuest(_) => "UpsertGuildQuest",
            DbOp::DeleteGuildQuest { .. } => "DeleteGuildQuest",
            DbOp::UpsertCouple(_) => "UpsertCouple",
            DbOp::DeleteCouple { .. } => "DeleteCouple",
            DbOp::SetCoupleProposal { .. } => "SetCoupleProposal",
            DbOp::UpsertCampus { .. } => "UpsertCampus",
            DbOp::DeleteCampus { .. } => "DeleteCampus",
            DbOp::UpsertCampusMember(_) => "UpsertCampusMember",
            DbOp::DeleteCampusMember { .. } => "DeleteCampusMember",
            DbOp::SetCampusPoints { .. } => "SetCampusPoints",
            DbOp::UpsertGuildSiege(_) => "UpsertGuildSiege",
            DbOp::UpsertGuildSiegeBid(_) => "UpsertGuildSiegeBid",
            DbOp::UpsertGuildSiegePlayer(_) => "UpsertGuildSiegePlayer",
            DbOp::UpsertGc1to1Tender(_) => "UpsertGc1to1Tender",
            DbOp::ReplaceGc1to1Lineup { .. } => "ReplaceGc1to1Lineup",
            DbOp::ExpireSiegeRewards { .. } => "ExpireSiegeRewards",
            DbOp::UpsertLordElection(_) => "UpsertLordElection",
            DbOp::UpsertLordCandidate(_) => "UpsertLordCandidate",
            DbOp::InsertLordVote(_) => "InsertLordVote",
            DbOp::UpsertLordEvent(_) => "UpsertLordEvent",
            DbOp::DeleteLordEvent { .. } => "DeleteLordEvent",
            DbOp::ClearLordEvents => "ClearLordEvents",
            DbOp::SetLordSkillCooldown { .. } => "SetLordSkillCooldown",
            DbOp::UpsertTaxOwner(_) => "UpsertTaxOwner",
            DbOp::UpsertTaxDetail(_) => "UpsertTaxDetail",
            DbOp::UpsertSecretRoomTender(_) => "UpsertSecretRoomTender",
            DbOp::ReplaceSecretRoomLineup { .. } => "ReplaceSecretRoomLineup",
            DbOp::UpsertGuildHouse(_) => "UpsertGuildHouse",
            DbOp::UpsertGuildHouseFurniture(_) => "UpsertGuildHouseFurniture",
            DbOp::DeleteGuildHouseFurniture { .. } => "DeleteGuildHouseFurniture",
            DbOp::UpsertHousingFurniture(_) => "UpsertHousingFurniture",
            DbOp::InsertHousingFurniture(_) => "InsertHousingFurniture",
            DbOp::DeleteHousingFurniture { .. } => "DeleteHousingFurniture",
            DbOp::SetHousingVisitor { .. } => "SetHousingVisitor",
            DbOp::DeleteHousingVisitsOf { .. } => "DeleteHousingVisitsOf",
            DbOp::UpsertRainbowRaceEntry(_) => "UpsertRainbowRaceEntry",
            DbOp::UpsertQuizQuestion(_) => "UpsertQuizQuestion",
            DbOp::MarkQuizQuestionsUsed { .. } => "MarkQuizQuestionsUsed",
            DbOp::PruneQuizQuestions { .. } => "PruneQuizQuestions",
            DbOp::SetRuntimeOverride { .. } => "SetRuntimeOverride",
            DbOp::DeleteRuntimeOverride { .. } => "DeleteRuntimeOverride",
            DbOp::SetServerKv { .. } => "SetServerKv",
            DbOp::RaiseCounter { .. } => "RaiseCounter",
        }
    }
}

/// One SQLite transaction scope (Plan/13 §16.2): a savepoint inside the batch's `BEGIN IMMEDIATE` transaction;
/// all-or-nothing.
pub struct AtomicBatch {
    /// Metrics/log label.
    pub kind: AtomicKind,
    /// Snapshots applied first, each generation-guarded (Plan/13 §16.5).
    pub snapshots: Vec<CharacterSnapshot>,
    /// Ops applied afterwards, in order.
    pub ops: Vec<DbOp>,
    /// Who is told about the result.
    pub reply: BatchReply,
}

impl AtomicBatch {
    /// A batch of ops without snapshots and without a reply.
    pub fn of_ops(kind: AtomicKind, ops: Vec<DbOp>) -> Self {
        Self {
            kind,
            snapshots: Vec::new(),
            ops,
            reply: BatchReply::None,
        }
    }
}

/// The effects of the commands applied so far in one transaction attempt, kept apart from the writer's
/// committed state so that a rolled back command or batch leaves no trace in memory (Plan/13 §16.4).
#[derive(Default)]
pub(crate) struct Overlay {
    /// Highest generation written (or covered) per account.
    pub gens: BTreeMap<AccountId, SaveGen>,
    /// Document hashes written per character.
    pub hashes: BTreeMap<CharacterId, DocHashes>,
    /// Snapshots written.
    pub snapshots_written: u64,
    /// Snapshots skipped by the generation guard.
    pub snapshots_stale: u64,
    /// Ops that found their row already closed (`MarkGrantsDelivered`, `MarkRemovalsDone`): logged as CRITICAL
    /// and counted in `DbStatsSnapshot::anomalies`.
    pub anomalies: u64,
}

impl Overlay {
    /// Folds the overlay of a successfully released command into this one.
    pub(crate) fn merge(&mut self, other: Overlay) {
        for (account, gen_) in other.gens {
            let slot = self.gens.entry(account).or_insert(SaveGen::ZERO);
            if gen_ > *slot {
                *slot = gen_;
            }
        }
        self.hashes.extend(other.hashes);
        self.snapshots_written += other.snapshots_written;
        self.snapshots_stale += other.snapshots_stale;
        self.anomalies += other.anomalies;
    }
}

/// Everything an op or snapshot needs besides the connection.
pub(crate) struct ApplyCtx<'a> {
    /// Wall clock of this attempt (stamps `saved_at`, `server_kv.updated_at`).
    pub now: UnixTime,
    /// Generations committed so far.
    pub committed_gens: &'a BTreeMap<AccountId, SaveGen>,
    /// Document hashes committed so far.
    pub committed_hashes: &'a BTreeMap<CharacterId, DocHashes>,
    /// Earlier, released commands of this transaction attempt.
    pub batch: &'a Overlay,
    /// The effects of the command being applied.
    pub own: Overlay,
}

impl<'a> ApplyCtx<'a> {
    pub(crate) fn new(
        now: UnixTime,
        committed_gens: &'a BTreeMap<AccountId, SaveGen>,
        committed_hashes: &'a BTreeMap<CharacterId, DocHashes>,
        batch: &'a Overlay,
    ) -> Self {
        Self {
            now,
            committed_gens,
            committed_hashes,
            batch,
            own: Overlay::default(),
        }
    }

    /// The highest generation of `account` written or covered so far (`SaveGen::ZERO` = none).
    pub(crate) fn gen_of(&self, account: AccountId) -> SaveGen {
        [
            self.committed_gens.get(&account),
            self.batch.gens.get(&account),
            self.own.gens.get(&account),
        ]
        .into_iter()
        .flatten()
        .copied()
        .max()
        .unwrap_or(SaveGen::ZERO)
    }

    /// The document hashes last written for `character`.
    pub(crate) fn hashes_of(&self, character: CharacterId) -> Option<DocHashes> {
        self.own
            .hashes
            .get(&character)
            .or_else(|| self.batch.hashes.get(&character))
            .or_else(|| self.committed_hashes.get(&character))
            .copied()
    }
}

/// Applies one op inside the caller's savepoint (Plan/13 §16.3).
pub(crate) fn apply_op(
    conn: &rusqlite::Connection,
    ctx: &mut ApplyCtx<'_>,
    op: &DbOp,
) -> Result<AtomicOutcome, DbError> {
    match op {
        DbOp::CreateAccount(rec) => {
            accounts::create_account(conn, rec).map(AtomicOutcome::AccountCreated)
        }
        DbOp::SetAccountPassword {
            account_id,
            password_hash,
        } => accounts::set_password(conn, *account_id, password_hash).map(|()| AtomicOutcome::Done),
        DbOp::SetAccountAuthority {
            account_id,
            authority,
        } => accounts::set_authority(conn, *account_id, *authority).map(|()| AtomicOutcome::Done),
        DbOp::RecordLogin { account_id, at, ip } => {
            accounts::record_login(conn, *account_id, *at, ip).map(|()| AtomicOutcome::Done)
        }
        DbOp::InsertAccountBan(ban) => {
            accounts::insert_ban(conn, ban).map(|()| AtomicOutcome::Done)
        }
        DbOp::LiftAccountBan { ban_id, at, by } => {
            accounts::lift_ban(conn, *ban_id, *at, by).map(|()| AtomicOutcome::Done)
        }
        DbOp::LiftAccountBansOf { account_id, at, by } => {
            accounts::lift_bans_of(conn, *account_id, *at, by).map(AtomicOutcome::Lifted)
        }
        DbOp::CreateCharacter(rec) => {
            characters::create_character(conn, rec).map(AtomicOutcome::CharacterCreated)
        }
        DbOp::SoftDeleteCharacter { character_id, at } => {
            characters::soft_delete(conn, *character_id, *at).map(|()| AtomicOutcome::Done)
        }
        DbOp::RenameCharacter { character_id, name } => {
            characters::rename(conn, *character_id, name).map(|()| AtomicOutcome::Done)
        }
        DbOp::SetCharacterLoginBlock {
            character_id,
            until,
        } => characters::set_login_block(conn, *character_id, *until).map(|()| AtomicOutcome::Done),
        DbOp::SetCharacterAuthority {
            character_id,
            authority,
        } => {
            characters::set_authority(conn, *character_id, *authority).map(|()| AtomicOutcome::Done)
        }
        DbOp::SealCharacter { character_id, at } => {
            characters::seal(conn, *character_id, *at).map(|()| AtomicOutcome::Done)
        }
        DbOp::UnsealCharacter {
            character_id,
            account_id,
            slot,
        } => characters::unseal(conn, *character_id, *account_id, *slot)
            .map(|()| AtomicOutcome::Done),
        DbOp::PurgeCharacters { ids } => {
            let purged = characters::purge(conn, ids)?;
            tracing::info!(requested = ids.len(), purged, "characters purged");
            Ok(AtomicOutcome::Done)
        }
        DbOp::SetBankPin { character_id, pin } => {
            banks::set_pin(conn, *character_id, *pin).map(|()| AtomicOutcome::Done)
        }
        DbOp::SetPartyName { character_id, name } => {
            party_names::set(conn, *character_id, name).map(|()| AtomicOutcome::Done)
        }
        DbOp::DeletePartyName { character_id } => {
            party_names::delete(conn, *character_id).map(|()| AtomicOutcome::Done)
        }
        DbOp::UpsertFriend(friend) => {
            messaging::upsert_friend(conn, friend).map(|()| AtomicOutcome::Done)
        }
        DbOp::DeleteFriendPair { a, b } => {
            messaging::delete_friend_pair(conn, *a, *b).map(|_| AtomicOutcome::Done)
        }
        DbOp::DeleteAllFriendsOf { character_id } => {
            messaging::delete_all_friends_of(conn, *character_id).map(|_| AtomicOutcome::Done)
        }
        DbOp::InsertNote(note) => {
            messaging::insert_note(conn, note).map(AtomicOutcome::RowInserted)
        }
        DbOp::MarkNotesDelivered {
            to_id,
            up_to_id,
            at,
        } => messaging::mark_notes_delivered(conn, *to_id, *up_to_id, *at)
            .map(|_| AtomicOutcome::Done),
        DbOp::InsertMail(mail) => messaging::insert_mail(conn, mail).map(|()| AtomicOutcome::Done),
        DbOp::UpdateMail(update) => {
            messaging::update_mail(conn, update).map(|()| AtomicOutcome::Done)
        }
        DbOp::ExpireMails { created_before, at } => {
            let expired = messaging::expire_mails(conn, *created_before, *at)?;
            if expired > 0 {
                tracing::info!(expired, "mails expired");
            }
            Ok(AtomicOutcome::Done)
        }
        DbOp::InsertPendingGrant(grant) => {
            messaging::insert_pending_grant(conn, grant).map(AtomicOutcome::RowInserted)
        }
        DbOp::MarkGrantsDelivered { ids, at } => {
            let (_, anomalies) = messaging::mark_grants_delivered(conn, ids, *at)?;
            ctx.own.anomalies += anomalies as u64;
            Ok(AtomicOutcome::Done)
        }
        DbOp::InsertPendingRemoval(removal) => {
            messaging::insert_pending_removal(conn, removal).map(AtomicOutcome::RowInserted)
        }
        DbOp::MarkRemovalsDone { done, at } => {
            let (_, anomalies) = messaging::mark_removals_done(conn, done, *at)?;
            ctx.own.anomalies += anomalies as u64;
            Ok(AtomicOutcome::Done)
        }
        DbOp::SetCharacterPosition {
            character_id,
            world_id,
            pos,
        } => characters::set_position(conn, *character_id, *world_id, *pos)
            .map(|()| AtomicOutcome::Done),
        DbOp::UndeleteCharacter { character_id, slot } => {
            characters::undelete(conn, *character_id, *slot).map(|()| AtomicOutcome::Done)
        }
        DbOp::CancelPendingGrant { id, by } => {
            messaging::cancel_pending_grant(conn, *id, by, ctx.now).map(AtomicOutcome::Changed)
        }
        DbOp::CancelPendingRemoval { id, by } => {
            messaging::cancel_pending_removal(conn, *id, by, ctx.now).map(AtomicOutcome::Changed)
        }
        DbOp::InsertAdminBookmark(b) => {
            admin_repo::insert_bookmark(conn, b).map(AtomicOutcome::RowInserted)
        }
        DbOp::UpdateAdminBookmark(b) => {
            admin_repo::update_bookmark(conn, b).map(AtomicOutcome::Changed)
        }
        DbOp::DeleteAdminBookmark { id } => {
            admin_repo::delete_bookmark(conn, *id).map(AtomicOutcome::Changed)
        }
        DbOp::InsertAdminTemplate(t) => {
            admin_repo::insert_template(conn, t).map(AtomicOutcome::RowInserted)
        }
        DbOp::UpdateAdminTemplate(t) => {
            admin_repo::update_template(conn, t).map(AtomicOutcome::Changed)
        }
        DbOp::DeleteAdminTemplate { id } => {
            admin_repo::delete_template(conn, *id).map(AtomicOutcome::Changed)
        }
        DbOp::InsertAdminNote(n) => {
            admin_repo::insert_note(conn, n).map(AtomicOutcome::RowInserted)
        }
        DbOp::SetAdminNotePinned { id, pinned } => {
            admin_repo::set_note_pinned(conn, *id, *pinned).map(AtomicOutcome::Changed)
        }
        DbOp::DeleteAdminNote { id } => {
            admin_repo::delete_note(conn, *id).map(AtomicOutcome::Changed)
        }
        DbOp::InsertScheduledNotice(n) => {
            admin_repo::insert_notice(conn, n).map(AtomicOutcome::RowInserted)
        }
        DbOp::UpdateScheduledNotice(n) => {
            admin_repo::update_notice(conn, n).map(AtomicOutcome::Changed)
        }
        DbOp::DeleteScheduledNotice { id } => {
            admin_repo::delete_notice(conn, *id).map(AtomicOutcome::Changed)
        }
        DbOp::NoticeFired { id, at } => {
            admin_repo::notice_fired(conn, *id, *at).map(AtomicOutcome::Changed)
        }
        DbOp::InsertIpBlock(b) => {
            admin_repo::insert_ip_block(conn, b).map(AtomicOutcome::RowInserted)
        }
        DbOp::DeleteIpBlock { id } => {
            admin_repo::delete_ip_block(conn, *id).map(AtomicOutcome::Changed)
        }
        DbOp::UpsertTotp {
            account_id,
            secret,
            enabled_at,
        } => admin_repo::upsert_totp(conn, *account_id, secret, *enabled_at)
            .map(|()| AtomicOutcome::Done),
        DbOp::DeleteTotp { account_id } => {
            admin_repo::delete_totp(conn, *account_id).map(AtomicOutcome::Changed)
        }
        DbOp::SetMustChangePassword { account_id, value } => {
            accounts::set_must_change_password(conn, *account_id, *value)
                .map(|()| AtomicOutcome::Done)
        }
        DbOp::IdempotencyBegin {
            key,
            actor,
            route,
            body_sha256,
            at,
        } => admin_repo::idempotency_begin(conn, key, actor, route, body_sha256, *at)
            .map(AtomicOutcome::Idempotency),
        DbOp::IdempotencyFinish { key, state, result } => {
            admin_repo::idempotency_finish(conn, key, *state, result.as_deref())
                .map(AtomicOutcome::Changed)
        }
        DbOp::UpsertGuild(g) => guilds::upsert_guild(conn, g).map(|()| AtomicOutcome::Done),
        DbOp::DeleteGuild { guild_id } => {
            guilds::delete_guild(conn, *guild_id).map(|_| AtomicOutcome::Done)
        }
        DbOp::UpsertGuildMember(m) => guilds::upsert_member(conn, m).map(|()| AtomicOutcome::Done),
        DbOp::DeleteGuildMember { character_id } => {
            guilds::delete_member(conn, *character_id).map(|_| AtomicOutcome::Done)
        }
        DbOp::SetGuildBank { guild_id, doc } => {
            guilds::set_bank(conn, *guild_id, doc).map(|()| AtomicOutcome::Done)
        }
        DbOp::InsertGuildBankLog(r) => {
            guilds::insert_bank_log(conn, r).map(AtomicOutcome::RowInserted)
        }
        DbOp::UpsertGuildWar(w) => guilds::upsert_war(conn, w).map(|()| AtomicOutcome::Done),
        DbOp::SetGuildWarOfMembers { guilds: g, war_id } => {
            guilds::set_war_of_members(conn, *g, *war_id).map(AtomicOutcome::Changed)
        }
        DbOp::SetGuildRejoinCooldown {
            character_id,
            until,
        } => guilds::set_rejoin_cooldown(conn, *character_id, *until).map(|()| AtomicOutcome::Done),
        DbOp::UpsertGuildQuest(q) => guilds::upsert_quest(conn, q).map(|()| AtomicOutcome::Done),
        DbOp::DeleteGuildQuest { guild_id, quest_id } => {
            guilds::delete_quest(conn, *guild_id, *quest_id).map(|()| AtomicOutcome::Done)
        }
        DbOp::UpsertCouple(c) => relations::upsert_couple(conn, c).map(|()| AtomicOutcome::Done),
        DbOp::DeleteCouple { couple_id } => {
            relations::delete_couple(conn, *couple_id).map(|_| AtomicOutcome::Done)
        }
        DbOp::SetCoupleProposal {
            proposer_id,
            cooldown_until,
        } => relations::set_couple_proposal(conn, *proposer_id, *cooldown_until)
            .map(|()| AtomicOutcome::Done),
        DbOp::UpsertCampus {
            campus_id,
            created_at,
        } => relations::upsert_campus(conn, *campus_id, *created_at).map(|()| AtomicOutcome::Done),
        DbOp::DeleteCampus { campus_id } => {
            relations::delete_campus(conn, *campus_id).map(|_| AtomicOutcome::Done)
        }
        DbOp::UpsertCampusMember(m) => {
            relations::upsert_campus_member(conn, m).map(|()| AtomicOutcome::Done)
        }
        DbOp::DeleteCampusMember { character_id } => {
            relations::delete_campus_member(conn, *character_id).map(|_| AtomicOutcome::Done)
        }
        DbOp::SetCampusPoints {
            character_id,
            points,
        } => {
            relations::set_campus_points(conn, *character_id, *points).map(|()| AtomicOutcome::Done)
        }
        DbOp::UpsertGuildSiege(s) => siege::upsert_siege(conn, s).map(|()| AtomicOutcome::Done),
        DbOp::UpsertGuildSiegeBid(b) => siege::upsert_bid(conn, b).map(|()| AtomicOutcome::Done),
        DbOp::UpsertGuildSiegePlayer(p) => {
            siege::upsert_player(conn, p).map(|()| AtomicOutcome::Done)
        }
        DbOp::UpsertGc1to1Tender(t) => siege::upsert_tender(conn, t).map(|()| AtomicOutcome::Done),
        DbOp::ReplaceGc1to1Lineup {
            round,
            guild_id,
            members,
        } => siege::replace_lineup(conn, *round, *guild_id, members).map(|()| AtomicOutcome::Done),
        DbOp::ExpireSiegeRewards {
            before_siege_id,
            at,
        } => siege::expire_rewards(conn, *before_siege_id, *at).map(AtomicOutcome::Changed),
        DbOp::UpsertLordElection(e) => {
            realm::upsert_election(conn, e).map(|()| AtomicOutcome::Done)
        }
        DbOp::UpsertLordCandidate(c) => {
            realm::upsert_candidate(conn, c).map(|()| AtomicOutcome::Done)
        }
        DbOp::InsertLordVote(v) => realm::insert_vote(conn, v).map(|()| AtomicOutcome::Done),
        DbOp::UpsertLordEvent(e) => realm::upsert_event(conn, e).map(|()| AtomicOutcome::Done),
        DbOp::DeleteLordEvent { character_id } => {
            realm::delete_event(conn, *character_id).map(|_| AtomicOutcome::Done)
        }
        DbOp::ClearLordEvents => realm::clear_events(conn).map(AtomicOutcome::Changed),
        DbOp::SetLordSkillCooldown { skill_id, minutes } => {
            realm::set_skill_cooldown(conn, *skill_id, *minutes).map(|()| AtomicOutcome::Done)
        }
        DbOp::UpsertTaxOwner(o) => realm::upsert_tax_owner(conn, o).map(|()| AtomicOutcome::Done),
        DbOp::UpsertTaxDetail(d) => realm::upsert_tax_detail(conn, d).map(|()| AtomicOutcome::Done),
        DbOp::UpsertSecretRoomTender(t) => {
            realm::upsert_secret_room_tender(conn, t).map(|()| AtomicOutcome::Done)
        }
        DbOp::ReplaceSecretRoomLineup {
            round,
            continent,
            guild_id,
            members,
        } => realm::replace_secret_room_lineup(conn, *round, *continent, *guild_id, members)
            .map(|()| AtomicOutcome::Done),
        DbOp::UpsertGuildHouse(h) => {
            houses::upsert_guild_house(conn, h).map(|()| AtomicOutcome::Done)
        }
        DbOp::UpsertGuildHouseFurniture(f) => {
            houses::upsert_guild_furniture(conn, f).map(|()| AtomicOutcome::Done)
        }
        DbOp::DeleteGuildHouseFurniture { seq } => {
            houses::delete_guild_furniture(conn, *seq).map(|_| AtomicOutcome::Done)
        }
        DbOp::UpsertHousingFurniture(f) => {
            houses::upsert_housing(conn, f).map(|()| AtomicOutcome::Done)
        }
        DbOp::InsertHousingFurniture(f) => {
            houses::insert_housing(conn, f).map(|()| AtomicOutcome::Done)
        }
        DbOp::DeleteHousingFurniture {
            character_id,
            item_id,
        } => houses::delete_housing(conn, *character_id, *item_id).map(|_| AtomicOutcome::Done),
        DbOp::SetHousingVisitor {
            owner_id,
            visitor_id,
            allowed,
        } => houses::set_housing_visitor(conn, *owner_id, *visitor_id, *allowed)
            .map(|()| AtomicOutcome::Done),
        DbOp::DeleteHousingVisitsOf { character_id } => {
            houses::delete_housing_visits_of(conn, *character_id).map(AtomicOutcome::Changed)
        }
        DbOp::UpsertRainbowRaceEntry(e) => {
            houses::upsert_rainbow_entry(conn, e).map(|()| AtomicOutcome::Done)
        }
        DbOp::UpsertQuizQuestion(q) => {
            houses::upsert_quiz_question(conn, q).map(|done| match done {
                houses::QuizUpsert::Inserted(id) => AtomicOutcome::RowInserted(id),
                houses::QuizUpsert::Updated => AtomicOutcome::Changed(1),
                houses::QuizUpsert::Unchanged => AtomicOutcome::Changed(0),
            })
        }
        DbOp::MarkQuizQuestionsUsed { ids, at } => {
            houses::mark_quiz_used(conn, ids, *at).map(AtomicOutcome::Changed)
        }
        DbOp::PruneQuizQuestions { quiz_type, keep } => {
            houses::prune_quiz_questions(conn, *quiz_type, keep).map(AtomicOutcome::Changed)
        }
        DbOp::SetRuntimeOverride { key, value, by, at } => {
            meta::set_override(conn, key, value, by, *at).map(|()| AtomicOutcome::Done)
        }
        DbOp::DeleteRuntimeOverride { key } => {
            meta::delete_override(conn, key).map(|()| AtomicOutcome::Done)
        }
        DbOp::SetServerKv { key, value } => {
            meta::set_kv(conn, key, value, ctx.now).map(|()| AtomicOutcome::Done)
        }
        DbOp::RaiseCounter { key, at_least } => {
            meta::raise_counter(conn, key, *at_least).map(|()| AtomicOutcome::Done)
        }
    }
}
