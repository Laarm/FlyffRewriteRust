//! `accounts` and `account_bans` writes (Plan/13 §5.2, §16.3; doc 12 §5): `CreateAccount`,
//! `SetAccountPassword`, `SetAccountAuthority`, `RecordLogin`, `InsertAccountBan`, `LiftAccountBan`.
//!
//! Every function runs inside the writer's batch transaction (inside a command savepoint) and maps the
//! constraint failures it expects to the *Expected* error kinds of Plan/13 §16.6.

use crate::error::{DbError, DbErrorKind, is_constraint};
use crate::model::{AccountBanRecord, NewAccountRecord};
use flyff_common::{AccountId, UnixTime};
use rusqlite::{Connection, params};

/// Validates an authority letter and returns its column text.
pub(crate) fn authority_text(authority: u8) -> Result<String, DbError> {
    if authority.is_ascii_alphabetic() {
        Ok(char::from(authority).to_string())
    } else {
        Err(DbError::new(
            DbErrorKind::Bug,
            format!("authority {authority:#04x} is not an ASCII letter"),
        ))
    }
}

/// `CreateAccount` -> the new id, or `AccountExists` when the name is taken (case-insensitively).
pub fn create_account(conn: &Connection, rec: &NewAccountRecord) -> Result<AccountId, DbError> {
    let authority = authority_text(rec.authority)?;
    let taken: bool = conn
        .prepare_cached("SELECT EXISTS (SELECT 1 FROM accounts WHERE name = ?1)")?
        .query_row([&rec.name], |r| r.get(0))?;
    if taken {
        return Err(DbError::new(
            DbErrorKind::AccountExists,
            format!("account '{}' exists", rec.name),
        ));
    }
    let inserted = conn
        .prepare_cached(
            "INSERT INTO accounts (name, password_hash, authority, created_at, created_via,
                                   registration_ip, email, must_change_password)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )?
        .execute(params![
            rec.name,
            rec.password_hash,
            authority,
            rec.created_at.0,
            rec.created_via.as_str(),
            rec.registration_ip,
            rec.email,
            i64::from(rec.must_change_password),
        ]);
    match inserted {
        Ok(_) => {
            let id = conn.last_insert_rowid();
            u32::try_from(id)
                .map(AccountId)
                .map_err(|_| DbError::new(DbErrorKind::Bug, format!("account id {id} exceeds u32")))
        }
        Err(e) if is_constraint(&e, "UNIQUE", "accounts.name") => Err(DbError::new(
            DbErrorKind::AccountExists,
            format!("account '{}' exists", rec.name),
        )),
        Err(e) => Err(e.into()),
    }
}

fn require_row(changed: usize, what: &str) -> Result<(), DbError> {
    if changed == 0 {
        Err(DbError::new(DbErrorKind::NotFound, what.to_owned()))
    } else {
        Ok(())
    }
}

/// `SetAccountPassword`.
pub fn set_password(
    conn: &Connection,
    account: AccountId,
    password_hash: &str,
) -> Result<(), DbError> {
    let n = conn
        .prepare_cached("UPDATE accounts SET password_hash = ?2 WHERE id = ?1")?
        .execute(params![account.0, password_hash])?;
    require_row(n, &format!("account {account} does not exist"))
}

/// `SetMustChangePassword` (Plan/25 §1.3): sets or clears `accounts.must_change_password`; `NotFound` for an
/// unknown account.
pub fn set_must_change_password(
    conn: &Connection,
    account: AccountId,
    value: bool,
) -> Result<(), DbError> {
    let n = conn
        .prepare_cached("UPDATE accounts SET must_change_password = ?2 WHERE id = ?1")?
        .execute(params![account.0, i64::from(value)])?;
    require_row(n, &format!("account {account} does not exist"))
}

/// `SetAccountAuthority`.
pub fn set_authority(conn: &Connection, account: AccountId, authority: u8) -> Result<(), DbError> {
    let letter = authority_text(authority)?;
    let n = conn
        .prepare_cached("UPDATE accounts SET authority = ?2 WHERE id = ?1")?
        .execute(params![account.0, letter])?;
    require_row(n, &format!("account {account} does not exist"))
}

/// `RecordLogin`.
pub fn record_login(
    conn: &Connection,
    account: AccountId,
    at: UnixTime,
    ip: &str,
) -> Result<(), DbError> {
    let n = conn
        .prepare_cached("UPDATE accounts SET last_login_at = ?2, last_login_ip = ?3 WHERE id = ?1")?
        .execute(params![account.0, at.0, ip])?;
    require_row(n, &format!("account {account} does not exist"))
}

/// `InsertAccountBan`: a new, active ban. `NotFound` when the account does not exist.
pub fn insert_ban(conn: &Connection, ban: &AccountBanRecord) -> Result<(), DbError> {
    let inserted = conn
        .prepare_cached(
            "INSERT INTO account_bans (account_id, created_at, until, reason, created_by)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?
        .execute(params![
            ban.account_id.0,
            ban.created_at.0,
            ban.until.map(|t| t.0),
            ban.reason,
            ban.created_by,
        ]);
    match inserted {
        Ok(_) => Ok(()),
        Err(e) if is_constraint(&e, "FOREIGN KEY", "") => Err(DbError::new(
            DbErrorKind::NotFound,
            format!("account {} does not exist", ban.account_id),
        )),
        Err(e) => Err(e.into()),
    }
}

/// `LiftAccountBansOf`: marks every ban of the account that is active at `at` (not lifted, permanent or ending
/// after `at`) as lifted; returns how many.
pub fn lift_bans_of(
    conn: &Connection,
    account: AccountId,
    at: UnixTime,
    by: &str,
) -> Result<u32, DbError> {
    let n = conn
        .prepare_cached(
            "UPDATE account_bans SET lifted_at = ?2, lifted_by = ?3
             WHERE account_id = ?1 AND lifted_at IS NULL AND (until IS NULL OR until > ?2)",
        )?
        .execute(params![account.0, at.0, by])?;
    Ok(u32::try_from(n).unwrap_or(u32::MAX))
}

/// `LiftAccountBan`: marks one still-open ban as lifted; `NotFound` when it does not exist or was lifted.
pub fn lift_ban(conn: &Connection, ban_id: i64, at: UnixTime, by: &str) -> Result<(), DbError> {
    let n = conn
        .prepare_cached(
            "UPDATE account_bans SET lifted_at = ?2, lifted_by = ?3
             WHERE id = ?1 AND lifted_at IS NULL",
        )?
        .execute(params![ban_id, at.0, by])?;
    require_row(
        n,
        &format!("ban {ban_id} does not exist or is already lifted"),
    )
}
