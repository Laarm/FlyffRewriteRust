//! Grants of the console (Plan/25 §5.2, §5.3, §5.6, §11.5, §11.10; **WP-ADMIN-20**): `GrantBatch`,
//! `CancelPendingGrant`, `CancelPendingRemoval`, and the item validation behind `POST /admin/api/items/validate`.
//!
//! | file | content |
//! |---|---|
//! | [`validate()`] | [`ItemSpec`] -> the normalised `ItemRecord` (every field and every item kind, the golden table of the rules), [`ItemFields`] (which builder fields an item shows) |
//! | `deliver` | the body of [`GrantBatch`]: per recipient Direct, Mail, Pending hand-off and Auto, one `AdminGrant` batch per delivery, the 10 minute `request_id` ledger |
//! | `cancel` | the conditional cancels of pending grants and removals (they answer when the database committed) |
//!
//! **Who does what.** The HTTP route validates and resolves the recipients, then sends `GrantBatch` in chunks of at
//! most 100 recipients. The game thread decides per recipient (it alone knows who is online with room), delivers
//! into the bag or the mailbox in the same commit block as its persistence batch, and answers one
//! [`RecipientResult`] each. A recipient that must wait for a delivery that is not installed yet (`Pending`, or
//! `Auto` for an offline character once WP-ECON-09 exists) answers [`GrantOutcome::NeedsPending`]: the route inserts
//! the `pending_grants` rows (it needs their database ids and no game state).

mod cancel;
mod deliver;
pub mod validate;

pub use cancel::GrantLedger;
pub(crate) use cancel::{cancel_pending_grant, cancel_pending_removal};
pub(super) use deliver::grant_batch;
pub use validate::{FieldError, ItemFields, Normalised, ValidateEnv, Warning, fields_of, validate};

use crate::admin::types::AdminFailure;
use flyff_common::CharacterId;

/// A random option line of an item (`dst` 1..=127, `adj` -511..=511).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RandomOptLine {
    /// The `DST_*` id.
    pub dst: u8,
    /// The adjustment.
    pub adj: i16,
}

/// How long a granted item lives (`KeepUntil`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KeepUntil {
    /// As the item's own rule says (no expiry is set at creation).
    #[default]
    Item,
    /// Never expires.
    None,
    /// For this many seconds from delivery.
    Duration {
        /// Seconds.
        secs: u64,
    },
    /// Until this Unix time.
    At {
        /// Unix seconds.
        unix: i64,
    },
}

/// One vis of a vis pet with its keep-until rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisSpec {
    /// The vis item id (`IK3_VIS`).
    pub item_id: u32,
    /// How long it lives.
    pub keep_until: KeepUntil,
}

/// What to create (25 §11.10 `ItemSpec`; [`validate()`] turns it into a stored item).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ItemSpec {
    /// The item id (`II_*`).
    pub item_id: u32,
    /// The stack size.
    pub count: u16,
    /// Upgrade level `+0..+10` (accessory 20, collector 5).
    pub upgrade: u8,
    /// The element (`0` = none, `1` fire .. `5` earth).
    pub element: u8,
    /// The element level.
    pub element_level: u8,
    /// Up to three random option lines.
    pub random_options: Vec<RandomOptLine>,
    /// Safe-upgraded (the protected-awakening flag; the lines stay inactive until the player re-awakens).
    pub safe: bool,
    /// Pierced socket cards (`0` = an empty socket).
    pub piercing: Vec<u32>,
    /// Ultimate gems: the ability item of each socket (`0` = empty).
    pub ultimate_gems: Vec<u32>,
    /// Vis of a vis pet.
    pub vis: Vec<VisSpec>,
    /// System pet data of an egg.
    pub pet: Option<flyff_db::PetRecord>,
    /// Charged (a cash item).
    pub charged: bool,
    /// Expiry.
    pub keep_until: KeepUntil,
    /// Soulbound or not; `None` = the item's default.
    pub bound: Option<bool>,
    /// Text of a note or letter.
    pub text: Option<String>,
    /// The guild of a guild cloak.
    pub guild_id: Option<u32>,
}

/// How the game delivers a batch (25 §5.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GrantMode {
    /// Online with room: the bag. Online without room: a mail. Offline: pending when the delivery exists, else a
    /// mail.
    Auto,
    /// The bag, online only.
    Direct,
    /// A system mail per item (the penya rides on the first), online or offline.
    Mail {
        /// The subject (cut to 31 bytes).
        subject: String,
        /// The text (cut to 255 bytes).
        text: String,
    },
    /// Hand the grant back to the route as pending rows.
    Pending,
}

/// One recipient of a batch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrantNow {
    /// The character (online or offline: the mode decides what is possible).
    pub character: CharacterId,
    /// The items.
    pub items: Vec<ItemSpec>,
    /// Penya.
    pub gold: u64,
}

/// Deliver items and penya, atomically per recipient (`GrantBatch`; `request_id` de-duplicates a retry for ten
/// minutes). Answers [`AdminOutput::Granted`](crate::input::AdminOutput::Granted).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrantBatch {
    /// The idempotency id of the request (a UUID).
    pub request_id: u128,
    /// How to deliver.
    pub mode: GrantMode,
    /// The recipients (at most [`MAX_RECIPIENTS`]).
    pub grants: Vec<GrantNow>,
}

/// The most recipients of one `GrantBatch` (the route chunks larger requests).
pub const MAX_RECIPIENTS: usize = 100;
/// The most item lines per recipient (25 §5.3).
pub const MAX_LINES: usize = 50;

/// What happened for one recipient.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GrantOutcome {
    /// Delivered into the bag; the serial of every created item line (the first touched stack for a merged one)
    /// and the penya added.
    Delivered {
        /// Serials, one per item line in request order.
        serials: Vec<u64>,
        /// Penya added to the wallet.
        gold: u64,
    },
    /// Sent as mails.
    Mailed {
        /// The mails.
        mail_ids: Vec<u64>,
    },
    /// The route must insert pending grants (the game cannot deliver them yet or the mode asked for them).
    NeedsPending,
    /// Refused or failed; nothing was given.
    Failed(AdminFailure),
}

/// One recipient's result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecipientResult {
    /// The recipient.
    pub character: CharacterId,
    /// What happened.
    pub outcome: GrantOutcome,
}

/// The answer of a [`GrantBatch`] (`AdminOutput::Granted`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GrantedReport {
    /// The request.
    pub request_id: u128,
    /// One result per recipient, in request order.
    pub per_recipient: Vec<RecipientResult>,
    /// `true` when this is the stored answer of a request the game had already carried out (a retry).
    pub replayed: bool,
}

/// Cancel a pending grant (`CancelPendingGrant`; a conditional update, never both delivered and cancelled).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CancelPendingGrant {
    /// The grant row.
    pub id: i64,
}

/// Cancel a pending removal (`CancelPendingRemoval`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CancelPendingRemoval {
    /// The removal row.
    pub id: i64,
}
