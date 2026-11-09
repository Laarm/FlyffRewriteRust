//! Records of the messaging & economy group (migration `0002_messaging.sql`, Plan/13 §7, §16.3, §16.10, §18):
//! friends, offline notes, mails, pending grants and pending removals.
//!
//! The records are pure data (serde, no SQL). Mail attachments and grant templates are [`StoredItem`]s, so a
//! mailed pet keeps its name, its vis keep times and the vis-transform flag (PL-054). Times are Unix seconds
//! ([`UnixTime`]); `None` means "never".

use super::item::{ItemRecord, StoredItem};
use flyff_common::{CharacterId, GuildId, MailId, UnixTime};
use serde::{Deserialize, Serialize};

/// Longest mail title in bytes (`MAX_MAILTITLE`); the game enforces it, the schema does not.
pub const MAX_MAIL_TITLE_BYTES: usize = 32;
/// Longest mail text in bytes (`MAX_MAILTEXT`).
pub const MAX_MAIL_BODY_BYTES: usize = 256;
/// Longest offline note in bytes.
pub const MAX_NOTE_BYTES: usize = 255;
/// A mail expires this many days after it was sent (`MAX_KEEP_MAX_DAY`, Plan/19 §11.1).
pub const MAIL_KEEP_DAYS: i64 = 15;
/// `mails.sender_id` of a system mail (rewards, refunds): no character has id 0.
pub const SYSTEM_SENDER: CharacterId = CharacterId(0);

/// One row of `friends` (`tblMessenger`): a friendship is **two** rows, one per direction; `blocked` belongs to
/// the owner's row only (Plan/13 §7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FriendRecord {
    /// The owner of this row.
    pub character_id: CharacterId,
    /// The friend.
    pub friend_id: CharacterId,
    /// The owner blocked the friend (`bBlock`).
    pub blocked: bool,
    /// When the friendship was made.
    pub created_at: UnixTime,
}

impl FriendRecord {
    /// The two rows of a new friendship, owner `a` first.
    pub fn pair(a: CharacterId, b: CharacterId, created_at: UnixTime) -> [FriendRecord; 2] {
        let row = |character_id, friend_id| FriendRecord {
            character_id,
            friend_id,
            blocked: false,
            created_at,
        };
        [row(a, b), row(b, a)]
    }
}

/// One row of `notes` (`TAG_TBL`): a message left for an offline character, delivered at its enter-world.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteRecord {
    /// `notes.id`; `0` in a record that is about to be inserted (the row id is allocated by the database).
    pub id: i64,
    /// The receiver.
    pub to_id: CharacterId,
    /// The sender (no foreign key: the sender may be purged).
    pub from_id: CharacterId,
    /// The text, at most [`MAX_NOTE_BYTES`] bytes.
    pub text: String,
    /// When it was left.
    pub created_at: UnixTime,
    /// When the receiver got it, `None` while undelivered.
    pub delivered_at: Option<UnixTime>,
}

impl NoteRecord {
    /// A new, undelivered note (`id` 0).
    pub fn new(
        to_id: CharacterId,
        from_id: CharacterId,
        text: impl Into<String>,
        created_at: UnixTime,
    ) -> Self {
        Self {
            id: 0,
            to_id,
            from_id,
            text: text.into(),
            created_at,
            delivered_at: None,
        }
    }
}

/// One row of `mails` (`MAIL_TBL`). The attachment is the full [`StoredItem`] (PL-054); taking the item or the
/// gold clears the column and stamps `item_taken_at` / `gold_taken_at` (so a serial scan never sees an item twice).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailRecord {
    /// `mails.id`, allocated by the `MailService` from `meta.next_mail_id`.
    pub id: MailId,
    /// The receiver.
    pub receiver_id: CharacterId,
    /// The sender; [`SYSTEM_SENDER`] for a system mail (no foreign key).
    pub sender_id: CharacterId,
    /// Title, at most [`MAX_MAIL_TITLE_BYTES`] bytes.
    pub title: String,
    /// Text, at most [`MAX_MAIL_BODY_BYTES`] bytes.
    pub body: String,
    /// Attached penya; 0 once taken.
    pub gold: i64,
    /// Attached item; `None` when there was none or once taken.
    pub item: Option<StoredItem>,
    /// When it was sent; it expires [`MAIL_KEEP_DAYS`] days later.
    pub created_at: UnixTime,
    /// When the receiver first read it.
    pub read_at: Option<UnixTime>,
    /// When the gold was taken.
    pub gold_taken_at: Option<UnixTime>,
    /// When the item was taken.
    pub item_taken_at: Option<UnixTime>,
    /// Soft delete (also expiry): the row stays until the retention job purges it.
    pub deleted_at: Option<UnixTime>,
}

impl MailRecord {
    /// A new unread mail without attachments; add them with [`MailRecord::with_gold`] and
    /// [`MailRecord::with_item`].
    pub fn new(
        id: MailId,
        receiver_id: CharacterId,
        sender_id: CharacterId,
        title: impl Into<String>,
        body: impl Into<String>,
        created_at: UnixTime,
    ) -> Self {
        Self {
            id,
            receiver_id,
            sender_id,
            title: title.into(),
            body: body.into(),
            gold: 0,
            item: None,
            created_at,
            read_at: None,
            gold_taken_at: None,
            item_taken_at: None,
            deleted_at: None,
        }
    }

    /// Attaches `gold` penya.
    #[must_use]
    pub fn with_gold(mut self, gold: i64) -> Self {
        self.gold = gold;
        self
    }

    /// Attaches an item (its `obj_id` is reset: a storage index means nothing in a mail).
    #[must_use]
    pub fn with_item(mut self, item: ItemRecord) -> Self {
        self.item = Some(StoredItem::new(item));
        self
    }

    /// `true` for a system mail.
    pub fn is_system(&self) -> bool {
        self.sender_id == SYSTEM_SENDER
    }

    /// When the mail expires ([`MAIL_KEEP_DAYS`] after it was sent).
    pub fn expires_at(&self) -> UnixTime {
        UnixTime(self.created_at.0.saturating_add(MAIL_KEEP_DAYS * 86_400))
    }

    /// `true` when the mail was soft-deleted.
    pub fn is_deleted(&self) -> bool {
        self.deleted_at.is_some()
    }
}

/// The payload of `DbOp::UpdateMail`: the state transitions of one mail (read, take gold, take item, delete).
/// Every `Some` field is applied, `None` leaves the column alone, so several transitions can share one op.
///
/// * `take_gold_at` sets `gold = 0, gold_taken_at = at` and `take_item_at` sets `item = NULL,
///   item_taken_at = at`. Both are **guarded**: the mail must exist, be undeleted and still hold the attachment,
///   otherwise the op fails with `Conflict` and the whole atomic batch (including the receiver's snapshot that
///   already contains the item) rolls back, so a stale game state can never duplicate an attachment.
/// * `read_at` keeps the first read time, `deleted_at` the first deletion time (both idempotent).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MailStateRecord {
    /// The mail.
    pub id: MailId,
    /// The receiver read it.
    pub read_at: Option<UnixTime>,
    /// The receiver took the gold.
    pub take_gold_at: Option<UnixTime>,
    /// The receiver took the item.
    pub take_item_at: Option<UnixTime>,
    /// Soft delete (the attachments are lost, KEEP).
    pub deleted_at: Option<UnixTime>,
}

impl MailStateRecord {
    /// Marks `id` read.
    pub fn read(id: MailId, at: UnixTime) -> Self {
        Self {
            id,
            read_at: Some(at),
            ..Self::default()
        }
    }

    /// Takes the gold of `id` (`gold = 0`, `gold_taken_at = at`).
    pub fn take_gold(id: MailId, at: UnixTime) -> Self {
        Self {
            id,
            take_gold_at: Some(at),
            ..Self::default()
        }
    }

    /// Takes the item of `id` (`item = NULL`, `item_taken_at = at`).
    pub fn take_item(id: MailId, at: UnixTime) -> Self {
        Self {
            id,
            take_item_at: Some(at),
            ..Self::default()
        }
    }

    /// Soft-deletes `id` (delete or expiry).
    pub fn delete(id: MailId, at: UnixTime) -> Self {
        Self {
            id,
            deleted_at: Some(at),
            ..Self::default()
        }
    }
}

/// What a pending grant delivers (`pending_grants.kind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GrantKind {
    /// An item template (`item` is set).
    Item,
    /// Penya (`gold > 0`).
    Gold,
}

impl GrantKind {
    /// The `kind` column text.
    pub const fn as_str(self) -> &'static str {
        match self {
            GrantKind::Item => "item",
            GrantKind::Gold => "gold",
        }
    }

    /// Parses the `kind` column.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "item" => Some(GrantKind::Item),
            "gold" => Some(GrantKind::Gold),
            _ => None,
        }
    }
}

/// One row of `pending_grants` (F-020; replaces `ITEM_SEND_TBL`, `MAIL_STR_REALTIME` and guild salaries for
/// offline members). Delivered at enter-world and by the `grant_poll` job; a fresh serial is assigned when an item
/// grant is delivered, so the stored template carries serial 0.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingGrantRecord {
    /// `pending_grants.id`; `0` in a record that is about to be inserted.
    pub id: i64,
    /// The receiver.
    pub character_id: CharacterId,
    /// Item or gold.
    pub kind: GrantKind,
    /// The item template of an item grant.
    pub item: Option<StoredItem>,
    /// The penya of a gold grant (`> 0`); 0 for an item grant.
    pub gold: i64,
    /// Where the grant comes from: `cli`, `gm:<account>`, `admin_api`, `guild_salary:<guild_id>`, `event:<name>`.
    pub source: String,
    /// Free text for the operator.
    pub note: String,
    /// When it was created.
    pub created_at: UnixTime,
    /// When it was delivered, `None` while open.
    pub delivered_at: Option<UnixTime>,
    /// When an administrator cancelled it (`CancelPendingGrant`, Plan/25 §5.6); a cancelled grant is never delivered.
    #[serde(default)]
    pub cancelled_at: Option<UnixTime>,
    /// Who cancelled it (`staff:<account>`, `owner:token`, ...).
    #[serde(default)]
    pub cancelled_by: Option<String>,
}

impl PendingGrantRecord {
    /// An item grant (`serial` of the template is set to 0: the delivery assigns a fresh one).
    pub fn item(
        character_id: CharacterId,
        mut item: ItemRecord,
        source: impl Into<String>,
        note: impl Into<String>,
        created_at: UnixTime,
    ) -> Self {
        item.serial = 0;
        Self {
            id: 0,
            character_id,
            kind: GrantKind::Item,
            item: Some(StoredItem::new(item)),
            gold: 0,
            source: source.into(),
            note: note.into(),
            created_at,
            delivered_at: None,
            cancelled_at: None,
            cancelled_by: None,
        }
    }

    /// A gold grant of `gold` penya (must be positive).
    pub fn gold(
        character_id: CharacterId,
        gold: i64,
        source: impl Into<String>,
        note: impl Into<String>,
        created_at: UnixTime,
    ) -> Self {
        Self {
            id: 0,
            character_id,
            kind: GrantKind::Gold,
            item: None,
            gold,
            source: source.into(),
            note: note.into(),
            created_at,
            delivered_at: None,
            cancelled_at: None,
            cancelled_by: None,
        }
    }

    /// `true` while the grant can still be delivered (neither delivered nor cancelled).
    pub fn is_open(&self) -> bool {
        self.delivered_at.is_none() && self.cancelled_at.is_none()
    }
}

/// Where a pending removal looks for the item (`pending_removals.location`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemovalLocation {
    /// The character's inventory.
    Inventory,
    /// The character's bank.
    Bank,
    /// One of the character's pockets.
    Pocket,
    /// The guild bank (the row carries `guild_id` instead of `character_id`).
    GuildBank,
}

impl RemovalLocation {
    /// The `location` column text.
    pub const fn as_str(self) -> &'static str {
        match self {
            RemovalLocation::Inventory => "inventory",
            RemovalLocation::Bank => "bank",
            RemovalLocation::Pocket => "pocket",
            RemovalLocation::GuildBank => "guild_bank",
        }
    }

    /// Parses the `location` column.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "inventory" => Some(RemovalLocation::Inventory),
            "bank" => Some(RemovalLocation::Bank),
            "pocket" => Some(RemovalLocation::Pocket),
            "guild_bank" => Some(RemovalLocation::GuildBank),
            _ => None,
        }
    }
}

/// One row of `pending_removals` (replaces `ITEM_REMOVE_TBL` and `tblRemoveItemFromGuildBank`, the GM
/// confiscation). Applied at enter-world (character rows) or by the 30 s poll (guild-bank rows); matches by `item_id`
/// plus the optional `serial` / `upgrade_level` / `element` / `element_level` (`None` = any).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingRemovalRecord {
    /// `pending_removals.id`; `0` in a record that is about to be inserted.
    pub id: i64,
    /// The character the item is taken from; `None` for a guild-bank removal.
    pub character_id: Option<CharacterId>,
    /// The guild of a guild-bank removal; `None` for a character removal.
    pub guild_id: Option<GuildId>,
    /// Where to look.
    pub location: RemovalLocation,
    /// The item id, `0` = penya.
    pub item_id: u32,
    /// Exact serial to match.
    pub serial: Option<u64>,
    /// Required "+N".
    pub upgrade_level: Option<i32>,
    /// Required element.
    pub element: Option<u8>,
    /// Required element level.
    pub element_level: Option<i32>,
    /// Item count, or the penya amount when `item_id` is 0 (`> 0`).
    pub count: u32,
    /// Who asked (`cli`, `gm:<account>`, ...).
    pub requested_by: String,
    /// Free text.
    pub reason: String,
    /// When it was requested.
    pub created_at: UnixTime,
    /// When it was applied, `None` while open.
    pub done_at: Option<UnixTime>,
    /// What was actually removed.
    pub removed_count: Option<u32>,
    /// When an administrator cancelled it (`CancelPendingRemoval`, Plan/25 §5.6); a cancelled removal is never applied.
    #[serde(default)]
    pub cancelled_at: Option<UnixTime>,
    /// Who cancelled it.
    #[serde(default)]
    pub cancelled_by: Option<String>,
}

impl PendingRemovalRecord {
    /// A removal of `count` pieces of `item_id` from `location` of `character_id` (any serial / upgrade / element).
    pub fn from_character(
        character_id: CharacterId,
        location: RemovalLocation,
        item_id: u32,
        count: u32,
        requested_by: impl Into<String>,
        created_at: UnixTime,
    ) -> Self {
        Self {
            id: 0,
            character_id: Some(character_id),
            guild_id: None,
            location,
            item_id,
            serial: None,
            upgrade_level: None,
            element: None,
            element_level: None,
            count,
            requested_by: requested_by.into(),
            reason: String::new(),
            created_at,
            done_at: None,
            removed_count: None,
            cancelled_at: None,
            cancelled_by: None,
        }
    }

    /// `true` while the removal can still be applied (neither applied nor cancelled).
    pub fn is_open(&self) -> bool {
        self.done_at.is_none() && self.cancelled_at.is_none()
    }

    /// A removal from the guild bank of `guild_id`.
    pub fn from_guild_bank(
        guild_id: GuildId,
        item_id: u32,
        count: u32,
        requested_by: impl Into<String>,
        created_at: UnixTime,
    ) -> Self {
        Self {
            character_id: None,
            guild_id: Some(guild_id),
            location: RemovalLocation::GuildBank,
            ..Self::from_character(
                CharacterId(0),
                RemovalLocation::GuildBank,
                item_id,
                count,
                requested_by,
                created_at,
            )
        }
    }
}
