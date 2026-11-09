//! Account records (Plan/13 §5.2, §16.3, §16.7): the rows of `accounts` and `account_bans` as the lobby,
//! the admin tooling and the writer see them.
//!
//! The password column holds an argon2id PHC string over the md5 hex of the legacy password (D3/D5, doc 12
//! §5.2); hashing and verification live in [`crate::password`].

use flyff_common::{AccountId, UnixTime};
use serde::{Deserialize, Serialize};

/// The authority letter stored for accounts and characters that have no grant (`'F'`, Plan/13 §5.2).
pub const AUTHORITY_DEFAULT: u8 = b'F';

/// How an account came to exist (`accounts.created_via`, doc 12 §5.3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CreatedVia {
    /// `flyff account create` or the admin API.
    #[default]
    Cli,
    /// `POST /api/register`.
    Web,
    /// The first-start administrator.
    Bootstrap,
}

impl CreatedVia {
    /// The value stored in `accounts.created_via`.
    pub const fn as_str(self) -> &'static str {
        match self {
            CreatedVia::Cli => "cli",
            CreatedVia::Web => "web",
            CreatedVia::Bootstrap => "bootstrap",
        }
    }

    /// Parses a stored `accounts.created_via` value.
    pub fn parse(text: &str) -> Option<CreatedVia> {
        match text {
            "cli" => Some(CreatedVia::Cli),
            "web" => Some(CreatedVia::Web),
            "bootstrap" => Some(CreatedVia::Bootstrap),
            _ => None,
        }
    }
}

/// Payload of `DbOp::CreateAccount` (the id is assigned by SQLite, `AUTOINCREMENT`, never reused).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewAccountRecord {
    /// Normalised name, `[a-z][a-z0-9]{2,15}` (validated by doc 12; the column CHECK enforces 3..=16
    /// characters). Unique case-insensitively.
    pub name: String,
    /// argon2id PHC string over the md5 hex of the password (see [`crate::password`]).
    pub password_hash: String,
    /// Authority letter (`b'F'` ... `b'P'`).
    pub authority: u8,
    /// `created_at`.
    pub created_at: UnixTime,
    /// `created_via`.
    pub created_via: CreatedVia,
    /// `registration_ip` (web registrations).
    pub registration_ip: Option<String>,
    /// `email`.
    pub email: Option<String>,
    /// `must_change_password` (Plan/25 §1.3): the first-start administrator and accounts whose password an
    /// administrator reset start with the flag set.
    pub must_change_password: bool,
}

impl NewAccountRecord {
    /// A record with authority `F`, no address and no e-mail.
    pub fn new(
        name: impl Into<String>,
        password_hash: impl Into<String>,
        created_at: UnixTime,
        created_via: CreatedVia,
    ) -> Self {
        Self {
            name: name.into(),
            password_hash: password_hash.into(),
            authority: AUTHORITY_DEFAULT,
            created_at,
            created_via,
            registration_ip: None,
            email: None,
            must_change_password: false,
        }
    }

    /// The same record with `must_change_password` set (Plan/25 §1.3).
    #[must_use]
    pub fn with_must_change_password(mut self) -> Self {
        self.must_change_password = true;
        self
    }
}

/// One row of `accounts` (`queries::find_account`, `queries::account_by_id`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountRecord {
    /// `id`.
    pub id: AccountId,
    /// `name` (as stored, i.e. as normalised at creation).
    pub name: String,
    /// `password_hash` (PHC string).
    pub password_hash: String,
    /// `authority`: an ASCII letter.
    pub authority: u8,
    /// `created_at`.
    pub created_at: UnixTime,
    /// `created_via`.
    pub created_via: CreatedVia,
    /// `registration_ip`.
    pub registration_ip: Option<String>,
    /// `last_login_at`.
    pub last_login_at: Option<UnixTime>,
    /// `last_login_ip`.
    pub last_login_ip: Option<String>,
    /// `email`.
    pub email: Option<String>,
    /// `must_change_password` (migration 0008): while set, the admin console answers 428 on every route except
    /// the session and password routes (Plan/25 §1.3). The game login is unaffected.
    pub must_change_password: bool,
}

/// One row of `account_bans` (payload of `DbOp::InsertAccountBan`, result of `queries::active_ban`).
///
/// A ban is active iff `lifted_at IS NULL AND (until IS NULL OR until > now)` (Plan/13 §5.2).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountBanRecord {
    /// `id`; ignored by `InsertAccountBan` (SQLite assigns it).
    pub id: i64,
    /// `account_id`.
    pub account_id: AccountId,
    /// `created_at`.
    pub created_at: UnixTime,
    /// `until`, `None` = permanent.
    pub until: Option<UnixTime>,
    /// `reason`.
    pub reason: String,
    /// `created_by` (`cli` or `gm:<account>`).
    pub created_by: String,
    /// `lifted_at`.
    pub lifted_at: Option<UnixTime>,
    /// `lifted_by`.
    pub lifted_by: Option<String>,
}

impl AccountBanRecord {
    /// `true` while the ban applies at `now` (Plan/13 §5.2).
    pub fn is_active(&self, now: UnixTime) -> bool {
        self.lifted_at.is_none() && self.until.is_none_or(|u| u > now)
    }
}

/// One entry of the account list of the admin tooling (`queries::list_accounts`, doc 12 §5.6): the account
/// row without the password hash, plus the active ban and the number of active characters.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountListEntry {
    /// `id`.
    pub id: AccountId,
    /// `name`.
    pub name: String,
    /// `authority`.
    pub authority: u8,
    /// `created_at`.
    pub created_at: UnixTime,
    /// `created_via`.
    pub created_via: CreatedVia,
    /// `last_login_at`.
    pub last_login_at: Option<UnixTime>,
    /// `last_login_ip`.
    pub last_login_ip: Option<String>,
    /// The currently active ban: `Some(None)` = permanent, `Some(Some(t))` = until `t`.
    pub banned_until: Option<Option<UnixTime>>,
    /// Characters with `state = 0`.
    pub characters: u32,
}

/// Filter of `queries::list_accounts`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountListFilter {
    /// Case-insensitive substring of the name.
    pub name_substr: Option<String>,
    /// Only accounts whose stored authority letter is `L` or higher (GM accounts, doc 12 §5.5 `--gm`).
    pub gm_only: bool,
    /// Maximum number of rows, ordered by id.
    pub limit: u32,
}

impl Default for AccountListFilter {
    fn default() -> Self {
        Self {
            name_substr: None,
            gm_only: false,
            limit: 1000,
        }
    }
}
