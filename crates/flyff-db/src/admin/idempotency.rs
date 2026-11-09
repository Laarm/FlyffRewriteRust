//! The idempotency rows of repeatable admin mutations (`admin_idempotency`, Plan/25 §11.6, §12 rule 9).
//!
//! The key row is written **before** dispatch ([`begin`], one writer transaction, so two concurrent requests with
//! one key cannot both start). The route then decides from the [`IdempotencyDecision`]:
//!
//! | decision | answer |
//! |---|---|
//! | `Started` | dispatch, then [`finish`] with `Done` and the response body (or `Failed` when nothing changed) |
//! | `InFlight` | `409 in_flight` |
//! | `Done` | the stored result (never run twice) |
//! | `Mismatch` | `422 idempotency_mismatch` (another body, route or actor under the same key) |
//!
//! A `failed` row of the same request is re-armed (`Started` again): a request that failed before it changed
//! anything may be retried with its key. A row stuck `in_flight` (the process died mid-request) stays `InFlight`
//! until the daily job purges it 24 h after creation; the operator uses a new key.

use super::model::{
    IDEMPOTENCY_KEY_MAX_BYTES, IDEMPOTENCY_RESULT_MAX_BYTES, IDEMPOTENCY_RETENTION_S,
    IdempotencyDecision, IdempotencyRecord, IdempotencyState, invalid,
};
use crate::error::{DbError, DbErrorKind};
use flyff_common::UnixTime;
use rusqlite::{Connection, OptionalExtension, params};

fn check_key(key: &str) -> Result<(), DbError> {
    if key.is_empty()
        || key.len() > IDEMPOTENCY_KEY_MAX_BYTES
        || !key.bytes().all(|b| (0x21..=0x7e).contains(&b))
    {
        return Err(invalid(format!(
            "an Idempotency-Key is 1..={IDEMPOTENCY_KEY_MAX_BYTES} printable ASCII characters"
        )));
    }
    Ok(())
}

/// Reads one row (`None` when the key is unknown). Used by the writer ops and by the admin read side.
pub fn get(conn: &Connection, key: &str) -> Result<Option<IdempotencyRecord>, DbError> {
    let row = conn
        .prepare_cached(
            "SELECT key, actor, route, body_sha256, state, result, created_at
             FROM admin_idempotency WHERE key = ?1",
        )?
        .query_row([key], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Vec<u8>>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, i64>(6)?,
            ))
        })
        .optional()?;
    let Some((key, actor, route, hash, state, result, created_at)) = row else {
        return Ok(None);
    };
    let body_sha256: [u8; 32] = hash.as_slice().try_into().map_err(|_| {
        DbError::new(
            DbErrorKind::Bug,
            format!(
                "idempotency key '{key}': body_sha256 holds {} bytes",
                hash.len()
            ),
        )
    })?;
    let state = IdempotencyState::parse(&state).ok_or_else(|| {
        DbError::new(
            DbErrorKind::Bug,
            format!("idempotency key '{key}': unknown state '{state}'"),
        )
    })?;
    Ok(Some(IdempotencyRecord {
        key,
        actor,
        route,
        body_sha256,
        state,
        result,
        created_at: UnixTime(created_at),
    }))
}

/// `IdempotencyBegin`: registers the request `(key, actor, route, body_sha256)` or reports what the key stands
/// for already (see the module table).
pub fn begin(
    conn: &Connection,
    key: &str,
    actor: &str,
    route: &str,
    body_sha256: &[u8; 32],
    at: UnixTime,
) -> Result<IdempotencyDecision, DbError> {
    check_key(key)?;
    let Some(existing) = get(conn, key)? else {
        conn.prepare_cached(
            "INSERT INTO admin_idempotency (key, actor, route, body_sha256, state, result, created_at)
             VALUES (?1, ?2, ?3, ?4, 'in_flight', NULL, ?5)",
        )?
        .execute(params![key, actor, route, body_sha256.as_slice(), at.0])?;
        return Ok(IdempotencyDecision::Started);
    };
    if existing.actor != actor || existing.route != route || &existing.body_sha256 != body_sha256 {
        return Ok(IdempotencyDecision::Mismatch);
    }
    match existing.state {
        IdempotencyState::Done => Ok(IdempotencyDecision::Done {
            result: existing.result,
        }),
        IdempotencyState::InFlight => Ok(IdempotencyDecision::InFlight {
            since: existing.created_at,
        }),
        IdempotencyState::Failed => {
            conn.prepare_cached(
                "UPDATE admin_idempotency SET state = 'in_flight', result = NULL, created_at = ?2
                 WHERE key = ?1 AND state = 'failed'",
            )?
            .execute(params![key, at.0])?;
            Ok(IdempotencyDecision::Started)
        }
    }
}

/// `IdempotencyFinish`: closes an `in_flight` row with `Done` (and the response body) or `Failed`; returns the
/// rows changed (`0` when the row is unknown or not in flight). `InFlight` is not a final state (`Invalid`).
pub fn finish(
    conn: &Connection,
    key: &str,
    state: IdempotencyState,
    result: Option<&str>,
) -> Result<u32, DbError> {
    if state == IdempotencyState::InFlight {
        return Err(invalid("an idempotency row finishes as done or failed"));
    }
    if result.is_some_and(|r| r.len() > IDEMPOTENCY_RESULT_MAX_BYTES) {
        return Err(invalid(format!(
            "an idempotency result is at most {IDEMPOTENCY_RESULT_MAX_BYTES} bytes"
        )));
    }
    let n = conn
        .prepare_cached(
            "UPDATE admin_idempotency SET state = ?2, result = ?3 WHERE key = ?1 AND state = 'in_flight'",
        )?
        .execute(params![key, state.as_str(), result])?;
    Ok(u32::try_from(n).unwrap_or(u32::MAX))
}

/// The daily purge (Plan/25 §11.6): rows created more than 24 h before `now`. Returns how many.
pub fn purge(conn: &Connection, now: UnixTime) -> Result<usize, DbError> {
    Ok(conn
        .prepare_cached("DELETE FROM admin_idempotency WHERE created_at < ?1")?
        .execute([now.0.saturating_sub(IDEMPOTENCY_RETENTION_S)])?)
}
