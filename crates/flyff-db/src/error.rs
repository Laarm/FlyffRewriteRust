//! `DbError` / `DbErrorKind` (Plan/13 §16.6).

use serde::{Deserialize, Serialize};

/// Classification of a persistence failure (Plan/13 §16.6).
///
/// The variants are grouped like the plan's table: *Expected* (mapped by an op, rolled back to a
/// savepoint, reported to the caller), *caller-visible states*, and *infrastructure* failures. The enum is
/// `#[non_exhaustive]`: names are frozen, additions are additive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum DbErrorKind {
    // ---- Expected ----
    /// A character name is already taken (`characters_name_live`).
    NameTaken,
    /// An account already has an active character in that slot (`characters_slot_active`).
    SlotTaken,
    /// An account with that name exists.
    AccountExists,
    /// The party name is taken (`party_names.name`).
    PartyNameTaken,
    /// The guild name is taken (`guilds_name`).
    GuildNameTaken,
    /// The addressed row does not exist (or is not visible to the caller).
    NotFound,
    /// Another expected conflict (a violated uniqueness rule that has no dedicated kind).
    Conflict,
    /// An op refused a value that violates its documented limits (a malformed CIDR, a note over 2000
    /// characters, ...): the route that built the op should have validated it first (Plan/25 §11.6).
    Invalid,
    // ---- caller-visible states ----
    /// `wait_flushed` ran into `[database] flush_wait_timeout_ms`.
    FlushTimeout,
    /// The last save of the account failed permanently (the account is poisoned until released).
    SaveFailed,
    /// The persistence actor is shutting down (or the handle is detached).
    ShuttingDown,
    /// The queue or a reader is saturated; retry later.
    Busy,
    // ---- infrastructure ----
    /// A transient SQLite failure (`BUSY`, `LOCKED`, `FULL`, `IOERR`, `CANTOPEN`, `NOMEM`): retry mode.
    Transient,
    /// A programming error or an unmapped constraint failure.
    Bug,
    /// The database file is corrupt or not a database (exit code 74).
    Corrupt,
    /// File-system error.
    Io,
    /// The database schema is newer than this binary (version guard, Plan/13 §3).
    NewerSchema,
    /// Another `flyff` instance holds `data/flyff.db.lock` (exit code 75).
    Locked,
    // ---- additions of flyff-db (not in the plan's list) ----
    /// An init-only configuration value differs from the one recorded in `meta.init.*` (exit code 78).
    InitOnlyMismatch,
    /// The configuration violates a range of Plan/13 §2.4.
    InvalidConfig,
    /// The operation is not implemented by the walking skeleton (replaced by WP-DB-04).
    Unsupported,
    /// A request did not complete in time (shutdown wait).
    Timeout,
}

impl DbErrorKind {
    /// `true` for the *Expected* class of Plan/13 §16.6: failures an op maps explicitly. The writer rolls
    /// back the command's savepoint and reports the error to the caller; other commands are unaffected.
    pub const fn is_expected(self) -> bool {
        matches!(
            self,
            DbErrorKind::NameTaken
                | DbErrorKind::SlotTaken
                | DbErrorKind::AccountExists
                | DbErrorKind::PartyNameTaken
                | DbErrorKind::GuildNameTaken
                | DbErrorKind::NotFound
                | DbErrorKind::Conflict
                | DbErrorKind::Invalid
        )
    }
}

/// A persistence error with context.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[error("{kind:?}: {context}")]
pub struct DbError {
    /// What went wrong.
    pub kind: DbErrorKind,
    /// Human readable context (path, SQL error text, ...).
    pub context: String,
}

impl DbError {
    /// Builds an error.
    pub fn new(kind: DbErrorKind, context: impl Into<String>) -> Self {
        Self {
            kind,
            context: context.into(),
        }
    }

    /// The `ShuttingDown` error returned by stub paths.
    pub fn shutting_down() -> Self {
        Self::new(DbErrorKind::ShuttingDown, "persistence is shutting down")
    }

    /// `true` when the failure class is *transient*: the writer rolls the batch back and retries.
    pub fn is_transient(&self) -> bool {
        self.kind == DbErrorKind::Transient
    }
}

/// Classifies a SQLite failure into the plan's classes (Plan/13 §16.6 table).
///
/// `BUSY`/`LOCKED`/`FULL`/`IOERR*`/`CANTOPEN`/`NOMEM` are transient, `CORRUPT`/`NOTADB` are corrupt,
/// "no rows" is `NotFound`, and everything else (including constraint failures, which ops map to
/// *Expected* kinds themselves before they reach this conversion) is a `Bug`.
pub fn classify_sqlite(e: &rusqlite::Error) -> DbErrorKind {
    use rusqlite::ErrorCode;
    if matches!(e, rusqlite::Error::QueryReturnedNoRows) {
        return DbErrorKind::NotFound;
    }
    match e.sqlite_error_code() {
        Some(ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase) => DbErrorKind::Corrupt,
        Some(
            ErrorCode::DatabaseBusy
            | ErrorCode::DatabaseLocked
            | ErrorCode::DiskFull
            | ErrorCode::SystemIoFailure
            | ErrorCode::CannotOpen
            | ErrorCode::OutOfMemory,
        ) => DbErrorKind::Transient,
        _ => DbErrorKind::Bug,
    }
}

/// `true` when `e` is a constraint violation of SQLite class `class` (`"UNIQUE"`, `"CHECK"`,
/// `"FOREIGN KEY"`, `"NOT NULL"`) whose message mentions `needle` (for `UNIQUE`: the failing `table.column`,
/// e.g. `"characters.name"`; an empty needle matches any). Ops use it to map the failures they expect to the
/// *Expected* kinds of Plan/13 §16.6.
pub fn is_constraint(e: &rusqlite::Error, class: &str, needle: &str) -> bool {
    match e {
        rusqlite::Error::SqliteFailure(code, Some(message))
            if code.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            message.contains(class) && message.contains(needle)
        }
        _ => false,
    }
}

impl From<rusqlite::Error> for DbError {
    fn from(e: rusqlite::Error) -> Self {
        DbError::new(classify_sqlite(&e), e.to_string())
    }
}

impl From<std::io::Error> for DbError {
    fn from(e: std::io::Error) -> Self {
        DbError::new(DbErrorKind::Io, e.to_string())
    }
}

impl From<serde_json::Error> for DbError {
    fn from(e: serde_json::Error) -> Self {
        DbError::new(DbErrorKind::Bug, format!("json: {e}"))
    }
}
