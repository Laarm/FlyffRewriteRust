//! Writes of the admin group (migration `0008_admin`, Plan/25 §11.6): the SQL behind the admin `DbOp`s.
//!
//! Every function runs on the writer connection inside a command savepoint ([`crate::ops`] dispatch). The
//! conditional updates return the number of rows they changed, which the batch reports as
//! `AtomicOutcome::Changed(n)`: a cancel that finds its grant delivered changes `0` rows and is not an error.
//! Values that violate a documented limit are refused with `Invalid` (the route validates first); a clash of a
//! unique key is `Conflict`; a missing referenced account is `NotFound`.

use super::idempotency;
use super::model::{
    AdminBookmarkRecord, AdminNoteRecord, AdminTemplateRecord, IdempotencyDecision,
    IdempotencyState, IpBlockRecord, NOTE_TEXT_MAX_CHARS, ScheduledNoticeRecord,
    TEMPLATE_SPEC_MAX_BYTES, invalid, normalize_cidr,
};
use crate::error::{DbError, DbErrorKind, is_constraint};
use flyff_common::{AccountId, UnixTime};
use rusqlite::{Connection, params};

fn changed(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn finite(value: f32, what: &str) -> Result<f64, DbError> {
    if value.is_finite() {
        Ok(f64::from(value))
    } else {
        Err(invalid(format!("{what} is not a finite number")))
    }
}

// ---- bookmarks -------------------------------------------------------------------------------------------------

/// `InsertAdminBookmark` -> the new id.
pub fn insert_bookmark(conn: &Connection, b: &AdminBookmarkRecord) -> Result<i64, DbError> {
    conn.prepare_cached(
        "INSERT INTO admin_bookmarks (grp, name, world, layer_policy, x, z, zoom, sort, created_by, created_at,
                                      updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
    )?
    .execute(params![
        b.group,
        b.name,
        b.world_id,
        b.layer_policy.as_str(),
        finite(b.x, "x")?,
        finite(b.z, "z")?,
        b.zoom.map(|z| finite(z, "zoom")).transpose()?,
        b.sort,
        b.created_by,
        b.created_at.0,
        b.updated_at.0,
    ])?;
    Ok(conn.last_insert_rowid())
}

/// `UpdateAdminBookmark`: replaces every field but `created_by` / `created_at`; returns rows changed (`0` = no
/// such bookmark).
pub fn update_bookmark(conn: &Connection, b: &AdminBookmarkRecord) -> Result<u32, DbError> {
    let n = conn
        .prepare_cached(
            "UPDATE admin_bookmarks SET grp = ?2, name = ?3, world = ?4, layer_policy = ?5, x = ?6, z = ?7,
                                        zoom = ?8, sort = ?9, updated_at = ?10
             WHERE id = ?1",
        )?
        .execute(params![
            b.id,
            b.group,
            b.name,
            b.world_id,
            b.layer_policy.as_str(),
            finite(b.x, "x")?,
            finite(b.z, "z")?,
            b.zoom.map(|z| finite(z, "zoom")).transpose()?,
            b.sort,
            b.updated_at.0,
        ])?;
    Ok(changed(n))
}

/// `DeleteAdminBookmark`: rows deleted.
pub fn delete_bookmark(conn: &Connection, id: i64) -> Result<u32, DbError> {
    Ok(changed(
        conn.prepare_cached("DELETE FROM admin_bookmarks WHERE id = ?1")?
            .execute([id])?,
    ))
}

// ---- templates -------------------------------------------------------------------------------------------------

fn spec_text(t: &AdminTemplateRecord) -> Result<String, DbError> {
    let text = serde_json::to_string(&t.spec)?;
    if text.len() > TEMPLATE_SPEC_MAX_BYTES {
        return Err(invalid(format!(
            "template spec is {} bytes, the limit is {TEMPLATE_SPEC_MAX_BYTES}",
            text.len()
        )));
    }
    if t.name.trim().is_empty() {
        return Err(invalid("template name is empty"));
    }
    Ok(text)
}

fn template_name_clash(t: &AdminTemplateRecord, e: rusqlite::Error) -> DbError {
    if is_constraint(&e, "UNIQUE", "admin_templates") {
        DbError::new(
            DbErrorKind::Conflict,
            format!(
                "a {} template named '{}' exists already",
                t.kind.as_str(),
                t.name
            ),
        )
    } else {
        e.into()
    }
}

/// `InsertAdminTemplate` -> the new id; `Conflict` when `(kind, name)` exists (case-insensitively).
pub fn insert_template(conn: &Connection, t: &AdminTemplateRecord) -> Result<i64, DbError> {
    let spec = spec_text(t)?;
    conn.prepare_cached(
        "INSERT INTO admin_templates (kind, name, spec, created_by, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?
    .execute(params![
        t.kind.as_str(),
        t.name,
        spec,
        t.created_by,
        t.created_at.0,
        t.updated_at.0
    ])
    .map_err(|e| template_name_clash(t, e))?;
    Ok(conn.last_insert_rowid())
}

/// `UpdateAdminTemplate`: replaces name, spec and `updated_at` (the kind and the author stay); rows changed.
/// `Conflict` when the new name collides with another template of the kind.
pub fn update_template(conn: &Connection, t: &AdminTemplateRecord) -> Result<u32, DbError> {
    let spec = spec_text(t)?;
    let n = conn
        .prepare_cached(
            "UPDATE admin_templates SET name = ?2, spec = ?3, updated_at = ?4 WHERE id = ?1",
        )?
        .execute(params![t.id, t.name, spec, t.updated_at.0])
        .map_err(|e| template_name_clash(t, e))?;
    Ok(changed(n))
}

/// `DeleteAdminTemplate`: rows deleted.
pub fn delete_template(conn: &Connection, id: i64) -> Result<u32, DbError> {
    Ok(changed(
        conn.prepare_cached("DELETE FROM admin_templates WHERE id = ?1")?
            .execute([id])?,
    ))
}

// ---- notes -----------------------------------------------------------------------------------------------------

/// `InsertAdminNote` -> the new id.
pub fn insert_note(conn: &Connection, n: &AdminNoteRecord) -> Result<i64, DbError> {
    if n.text.trim().is_empty() || n.text.chars().count() > NOTE_TEXT_MAX_CHARS {
        return Err(invalid(format!(
            "a note holds 1..={NOTE_TEXT_MAX_CHARS} characters"
        )));
    }
    conn.prepare_cached(
        "INSERT INTO admin_notes (subject_kind, subject_id, text, pinned, author, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?
    .execute(params![
        n.subject.kind.as_str(),
        n.subject.id,
        n.text,
        i64::from(n.pinned),
        n.author,
        n.created_at.0
    ])?;
    Ok(conn.last_insert_rowid())
}

/// `SetAdminNotePinned`: rows changed.
pub fn set_note_pinned(conn: &Connection, id: i64, pinned: bool) -> Result<u32, DbError> {
    Ok(changed(
        conn.prepare_cached("UPDATE admin_notes SET pinned = ?2 WHERE id = ?1")?
            .execute(params![id, i64::from(pinned)])?,
    ))
}

/// `DeleteAdminNote`: rows deleted.
pub fn delete_note(conn: &Connection, id: i64) -> Result<u32, DbError> {
    Ok(changed(
        conn.prepare_cached("DELETE FROM admin_notes WHERE id = ?1")?
            .execute([id])?,
    ))
}

// ---- scheduled notices -----------------------------------------------------------------------------------------

/// `InsertScheduledNotice` -> the new id.
pub fn insert_notice(conn: &Connection, n: &ScheduledNoticeRecord) -> Result<i64, DbError> {
    n.validate()?;
    conn.prepare_cached(
        "INSERT INTO scheduled_notices (text, style, target, start_at, repeat_min, daily_at, end_at, paused,
                                        created_by, created_at, last_fired_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
    )?
    .execute(params![
        n.text,
        n.style,
        n.target,
        n.start_at.0,
        n.repeat_min,
        n.daily_at,
        n.end_at.map(|t| t.0),
        i64::from(n.paused),
        n.created_by,
        n.created_at.0,
        n.last_fired_at.map(|t| t.0),
    ])?;
    Ok(conn.last_insert_rowid())
}

/// `UpdateScheduledNotice`: replaces text, style, target, the schedule and `paused` (not the author and not
/// `last_fired_at`); rows changed.
pub fn update_notice(conn: &Connection, n: &ScheduledNoticeRecord) -> Result<u32, DbError> {
    n.validate()?;
    let rows = conn
        .prepare_cached(
            "UPDATE scheduled_notices SET text = ?2, style = ?3, target = ?4, start_at = ?5, repeat_min = ?6,
                                          daily_at = ?7, end_at = ?8, paused = ?9
             WHERE id = ?1",
        )?
        .execute(params![
            n.id,
            n.text,
            n.style,
            n.target,
            n.start_at.0,
            n.repeat_min,
            n.daily_at,
            n.end_at.map(|t| t.0),
            i64::from(n.paused),
        ])?;
    Ok(changed(rows))
}

/// `DeleteScheduledNotice`: rows deleted.
pub fn delete_notice(conn: &Connection, id: i64) -> Result<u32, DbError> {
    Ok(changed(
        conn.prepare_cached("DELETE FROM scheduled_notices WHERE id = ?1")?
            .execute([id])?,
    ))
}

/// `NoticeFired`: stamps `last_fired_at` (the granularity of the notice job is one minute); rows changed.
pub fn notice_fired(conn: &Connection, id: i64, at: UnixTime) -> Result<u32, DbError> {
    Ok(changed(
        conn.prepare_cached("UPDATE scheduled_notices SET last_fired_at = ?2 WHERE id = ?1")?
            .execute(params![id, at.0])?,
    ))
}

// ---- IP blocks -------------------------------------------------------------------------------------------------

/// `InsertIpBlock` -> the new id. The CIDR is normalised ([`normalize_cidr`]); `Conflict` when the same network
/// is blocked already, `Invalid` for a malformed CIDR.
pub fn insert_ip_block(conn: &Connection, b: &IpBlockRecord) -> Result<i64, DbError> {
    let cidr = normalize_cidr(&b.cidr)?;
    conn.prepare_cached(
        "INSERT INTO admin_ip_blocks (cidr, reason, until, created_by, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?
    .execute(params![
        cidr,
        b.reason,
        b.until.map(|t| t.0),
        b.created_by,
        b.created_at.0
    ])
    .map_err(|e| {
        if is_constraint(&e, "UNIQUE", "admin_ip_blocks.cidr") {
            DbError::new(DbErrorKind::Conflict, format!("{cidr} is blocked already"))
        } else {
            e.into()
        }
    })?;
    Ok(conn.last_insert_rowid())
}

/// `DeleteIpBlock`: rows deleted.
pub fn delete_ip_block(conn: &Connection, id: i64) -> Result<u32, DbError> {
    Ok(changed(
        conn.prepare_cached("DELETE FROM admin_ip_blocks WHERE id = ?1")?
            .execute([id])?,
    ))
}

// ---- TOTP ------------------------------------------------------------------------------------------------------

/// `UpsertTotp`: stores (or replaces) the secret of a staff account; `NotFound` for an unknown account.
pub fn upsert_totp(
    conn: &Connection,
    account: AccountId,
    secret: &[u8],
    enabled_at: UnixTime,
) -> Result<(), DbError> {
    if secret.is_empty() || secret.len() > 128 {
        return Err(invalid("a TOTP secret is 1..=128 bytes"));
    }
    conn.prepare_cached(
        "INSERT INTO admin_totp (account_id, secret, enabled_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(account_id) DO UPDATE SET secret = excluded.secret, enabled_at = excluded.enabled_at",
    )?
    .execute(params![account.0, secret, enabled_at.0])
    .map_err(|e| {
        if is_constraint(&e, "FOREIGN KEY", "") {
            DbError::new(
                DbErrorKind::NotFound,
                format!("account {account} does not exist"),
            )
        } else {
            e.into()
        }
    })?;
    Ok(())
}

/// `DeleteTotp`: disables TOTP of the account; rows deleted.
pub fn delete_totp(conn: &Connection, account: AccountId) -> Result<u32, DbError> {
    Ok(changed(
        conn.prepare_cached("DELETE FROM admin_totp WHERE account_id = ?1")?
            .execute([account.0])?,
    ))
}

// ---- idempotency -----------------------------------------------------------------------------------------------

/// `IdempotencyBegin`: see [`idempotency::begin`].
pub fn idempotency_begin(
    conn: &Connection,
    key: &str,
    actor: &str,
    route: &str,
    body_sha256: &[u8; 32],
    at: UnixTime,
) -> Result<IdempotencyDecision, DbError> {
    idempotency::begin(conn, key, actor, route, body_sha256, at)
}

/// `IdempotencyFinish`: see [`idempotency::finish`].
pub fn idempotency_finish(
    conn: &Connection,
    key: &str,
    state: IdempotencyState,
    result: Option<&str>,
) -> Result<u32, DbError> {
    idempotency::finish(conn, key, state, result)
}
