//! Small types shared by the admin console's game side (Plan/25 §11.5, §11.10; WP-ADMIN-06): the connection id, the
//! failure an admin command or query answers with, and the container locator.

use flyff_common::GuildId;

/// One admin WebSocket (25 §11.5 `AdminConnId`): allocated by the server's gateway, monotonic from 1 per process. The
/// game thread keys its subscriptions by it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct AdminConnId(pub u64);

impl std::fmt::Display for AdminConnId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "admin-conn-{}", self.0)
    }
}

/// Why an admin command or query failed (the error side of `AdminOutput::Failed` and `AdminQueryResult::Failed`).
/// The server maps the code to the HTTP status and the wire code of `ErrorCode` (25 §11.10).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AdminErrorCode {
    /// The work package that owns the command has not landed (the scaffolding of WP-ADMIN-06).
    NotImplemented,
    /// The target does not exist (any more).
    NotFound,
    /// The request is malformed or out of range.
    Invalid,
    /// The target changed since the console looked at it (a serial mismatch, a moved or dead object): 409.
    StaleTarget,
    /// More targets than `admin.max_objects_per_action`: 413.
    TooManyTargets,
    /// The character is online and the operation needs it offline (or the reverse).
    CharacterOnline,
    /// The target's authority is not below the actor's letter (rank protection, 25 §1.6): 403.
    TargetOutranks,
    /// The bag has no room for what a direct grant brings (WP-ADMIN-20): 409.
    NoSpace,
    /// The mailbox would hold more than 50 mails (WP-ADMIN-20): 409.
    MailboxFull,
    /// A pending grant is being delivered (or cancelled) right now (WP-ADMIN-20): 409.
    DeliveryInProgress,
    /// The pending grant or removal was delivered, applied or cancelled already (WP-ADMIN-20): 409.
    AlreadyDelivered,
    /// Something went wrong inside the game (the cause is in the log).
    Internal,
}

impl AdminErrorCode {
    /// The snake_case wire name (equal to the `ErrorCode` of 25 §11.10 where one exists).
    pub const fn name(self) -> &'static str {
        match self {
            AdminErrorCode::NotImplemented => "not_implemented",
            AdminErrorCode::NotFound => "not_found",
            AdminErrorCode::Invalid => "invalid",
            AdminErrorCode::StaleTarget => "stale_target",
            AdminErrorCode::TooManyTargets => "too_many_targets",
            AdminErrorCode::CharacterOnline => "character_online",
            AdminErrorCode::TargetOutranks => "target_outranks",
            AdminErrorCode::NoSpace => "no_space",
            AdminErrorCode::MailboxFull => "mailbox_full",
            AdminErrorCode::DeliveryInProgress => "delivery_in_progress",
            AdminErrorCode::AlreadyDelivered => "already_delivered",
            AdminErrorCode::Internal => "internal",
        }
    }
}

/// A failed admin command or query: a code and an English message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdminFailure {
    /// What went wrong.
    pub code: AdminErrorCode,
    /// For the operator.
    pub message: String,
}

impl AdminFailure {
    /// A failure with a code and a message.
    pub fn new(code: AdminErrorCode, message: impl Into<String>) -> AdminFailure {
        AdminFailure {
            code,
            message: message.into(),
        }
    }

    /// The scaffolding answer (25 §11.5): `what` is the command or query, `wp` the work package that implements it.
    pub fn not_implemented(what: &str, wp: &str) -> AdminFailure {
        AdminFailure::new(
            AdminErrorCode::NotImplemented,
            format!("{what} is not implemented yet ({wp})"),
        )
    }
}

impl std::fmt::Display for AdminFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code.name(), self.message)
    }
}

/// Where a character's items live (25 §11.10 `ContainerLoc`): the locator of the container queries and of
/// `RemoveItem` / `AdjustGold`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContainerLoc {
    /// The worn equipment.
    Equipment,
    /// The inventory bag.
    Inventory,
    /// A pocket, `0..3`.
    Pocket(u8),
    /// A character bank slot, `0..3` (banks are account-scoped, 25 ADM-D8).
    Bank {
        /// The slot.
        slot: u8,
    },
    /// The bank of a guild.
    GuildBank(GuildId),
    /// The mailbox.
    Mailbox,
}
