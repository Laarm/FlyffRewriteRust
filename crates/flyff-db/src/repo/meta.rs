//! `meta` counters, item-serial blocks, `server_kv` and `runtime_override` (Plan/13 §5.1, §15).

use crate::error::{DbError, DbErrorKind};
use flyff_common::UnixTime;
use rusqlite::{Connection, OptionalExtension, params};
use std::ops::Range;

/// Every `meta` counter key of Plan/13 §15.1 (`next_*` and `*_round`); `RaiseCounter` refuses other keys
/// (the `init.*` rows are recorded configuration values, not counters).
pub const COUNTER_KEYS: [&str; 15] = [
    counter::NEXT_CHARACTER_ID,
    counter::NEXT_GUILD_ID,
    counter::NEXT_MAIL_ID,
    counter::NEXT_GUILD_WAR_ID,
    counter::NEXT_ITEM_SERIAL,
    counter::NEXT_CAMPUS_ID,
    counter::NEXT_COUPLE_ID,
    counter::NEXT_SIEGE_ID,
    counter::NEXT_LORD_ELECTION_ID,
    counter::NEXT_GUILD_FURNITURE_SEQ,
    counter::NEXT_QUIZ_EVENT_ID,
    counter::TAX_ROUND,
    counter::SECRET_ROOM_ROUND,
    counter::RAINBOW_RACE_ROUND,
    counter::GC1TO1_ROUND,
];

/// The names of the §15.1 counters, for `DbOp::RaiseCounter { key, .. }`.
pub mod counter {
    /// `characters.id`.
    pub const NEXT_CHARACTER_ID: &str = "next_character_id";
    /// `guilds.id`.
    pub const NEXT_GUILD_ID: &str = "next_guild_id";
    /// `mails.id`.
    pub const NEXT_MAIL_ID: &str = "next_mail_id";
    /// `guild_wars.id`.
    pub const NEXT_GUILD_WAR_ID: &str = "next_guild_war_id";
    /// Item serial blocks.
    pub const NEXT_ITEM_SERIAL: &str = "next_item_serial";
    /// `campuses.id`.
    pub const NEXT_CAMPUS_ID: &str = "next_campus_id";
    /// `couples.id`.
    pub const NEXT_COUPLE_ID: &str = "next_couple_id";
    /// `guild_sieges.id`.
    pub const NEXT_SIEGE_ID: &str = "next_siege_id";
    /// `lord_elections.id`.
    pub const NEXT_LORD_ELECTION_ID: &str = "next_lord_election_id";
    /// `guild_house_furniture.seq`.
    pub const NEXT_GUILD_FURNITURE_SEQ: &str = "next_guild_furniture_seq";
    /// Quiz events.
    pub const NEXT_QUIZ_EVENT_ID: &str = "next_quiz_event_id";
    /// Current tax round.
    pub const TAX_ROUND: &str = "tax_round";
    /// Current secret room round.
    pub const SECRET_ROOM_ROUND: &str = "secret_room_round";
    /// Current rainbow race round.
    pub const RAINBOW_RACE_ROUND: &str = "rainbow_race_round";
    /// Current GC1to1 round.
    pub const GC1TO1_ROUND: &str = "gc1to1_round";
}

/// The `meta` key of the item serial counter.
pub const KEY_NEXT_ITEM_SERIAL: &str = counter::NEXT_ITEM_SERIAL;
/// The `meta` key of the character id counter.
pub const KEY_NEXT_CHARACTER_ID: &str = counter::NEXT_CHARACTER_ID;

/// `server_kv` keys owned by this crate (Plan/13 §15.2, §20).
pub mod kv {
    /// First start of the database (unix s).
    pub const FIRST_START_AT: &str = "server.first_start_at";
    /// Last start (unix s).
    pub const LAST_START_AT: &str = "server.last_start_at";
    /// Last clean shutdown (unix s).
    pub const LAST_CLEAN_SHUTDOWN_AT: &str = "server.last_clean_shutdown_at";
    /// The local date (`"YYYY-MM-DD"`, a JSON string) on which the guild salary was last paid; the pay-once guard
    /// of the 21:00 job (Plan/21 §5.10, PL-094). Written by the `GuildSalary` batches
    /// ([`crate::AtomicBatch::guild_salary`]).
    pub const GUILD_SALARY_LAST_PAID_ON: &str = "guild.salary_last_paid_on";
    /// The local date (`"YYYY-MM-DD"`, a JSON string, server zone: the format of `social/guild/salary.rs`) on which
    /// the tax payout last ran; the pay-once guard of the daily payout (Plan/22 §10.4, PL-100). Written by the
    /// `TaxPayout` batches ([`crate::AtomicBatch::tax_payout`]) in the SAME transaction as the payout mails, so a
    /// crash never pays a day twice and never loses the mails of a recorded day.
    pub const TAX_LAST_PAID_ON: &str = "tax.last_paid_on";
    /// The unix seconds (a JSON integer) of the last weekly tax change (`changed_at` of the current round); written
    /// by the `TaxPayout` batch of a change ([`crate::AtomicBatch::tax_payout`]).
    pub const TAX_LAST_CHANGED_AT: &str = "tax.last_changed_at";
    /// Whether the lord election runs (a JSON bool; a missing key means `true`): the `/ElectionProcess` switch of the
    /// GM, persisted with `DbOp::SetServerKv`.
    pub const LORD_ELECTION_ENABLED: &str = "lord.election_enabled";
}

/// `UPDATE meta SET value = MAX(value, at_least) WHERE key = ?` (inserts a missing counter row).
/// Only the §15.1 counters are accepted.
pub fn raise_counter(conn: &Connection, key: &str, at_least: i64) -> Result<(), DbError> {
    if !COUNTER_KEYS.contains(&key) {
        return Err(DbError::new(
            DbErrorKind::Bug,
            format!("RaiseCounter: '{key}' is not a meta counter"),
        ));
    }
    conn.prepare_cached(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = MAX(value, excluded.value)",
    )?
    .execute(params![key, at_least])?;
    Ok(())
}

/// Reads a counter (`None` when the row is missing).
pub fn counter(conn: &Connection, key: &str) -> Result<Option<i64>, DbError> {
    Ok(conn
        .prepare_cached("SELECT value FROM meta WHERE key = ?1")?
        .query_row([key], |r| r.get(0))
        .optional()?)
}

/// Allocates and returns the current value of a counter, incrementing it by one (`value - 1` is the id
/// handed out). Used for ids the writer assigns inside the transaction (`next_character_id`).
pub fn take_id(conn: &Connection, key: &str) -> Result<i64, DbError> {
    let id: Option<i64> = conn
        .prepare_cached("UPDATE meta SET value = value + 1 WHERE key = ?1 RETURNING value - 1")?
        .query_row([key], |r| r.get(0))
        .optional()?;
    id.ok_or_else(|| DbError::new(DbErrorKind::Bug, format!("meta counter '{key}' is missing")))
}

/// Reserves `size` item serials: `UPDATE meta SET value = value + size RETURNING value - size`
/// (Plan/13 §15.1). The block is `start..start + size`; serial 0 is never inside (the counter starts at 1).
pub fn reserve_serial_block(conn: &Connection, size: u32) -> Result<Range<u64>, DbError> {
    let size = i64::from(size.max(1));
    let start: Option<i64> = conn
        .prepare_cached("UPDATE meta SET value = value + ?1 WHERE key = ?2 RETURNING value - ?1")?
        .query_row(params![size, KEY_NEXT_ITEM_SERIAL], |r| r.get(0))
        .optional()?;
    let start = start.ok_or_else(|| {
        DbError::new(
            DbErrorKind::Bug,
            "meta counter 'next_item_serial' is missing",
        )
    })?;
    let start = u64::try_from(start.max(1)).unwrap_or(1);
    let end = start.saturating_add(u64::try_from(size).unwrap_or(1));
    Ok(start..end)
}

/// `server_kv` upsert (`updated_at` = `now`).
pub fn set_kv(
    conn: &Connection,
    key: &str,
    value: &serde_json::Value,
    now: UnixTime,
) -> Result<(), DbError> {
    let text = serde_json::to_string(value)?;
    conn.prepare_cached(
        "INSERT INTO server_kv (key, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
    )?
    .execute(params![key, text, now.0])?;
    Ok(())
}

/// Reads one `server_kv` value.
pub fn get_kv(conn: &Connection, key: &str) -> Result<Option<serde_json::Value>, DbError> {
    let text: Option<String> = conn
        .prepare_cached("SELECT value FROM server_kv WHERE key = ?1")?
        .query_row([key], |r| r.get(0))
        .optional()?;
    text.map(|t| serde_json::from_str(&t).map_err(DbError::from))
        .transpose()
}

/// Reads a `server_kv` integer (unix seconds) tolerant of a missing table (a database that has not been
/// migrated yet) and of non-integer values.
pub fn get_kv_i64(conn: &Connection, key: &str) -> Option<i64> {
    get_kv(conn, key).ok().flatten().and_then(|v| v.as_i64())
}

/// `runtime_override` upsert.
pub fn set_override(
    conn: &Connection,
    key: &str,
    value: &serde_json::Value,
    by: &str,
    at: UnixTime,
) -> Result<(), DbError> {
    let text = serde_json::to_string(value)?;
    conn.prepare_cached(
        "INSERT INTO runtime_override (key, value, set_by, set_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, set_by = excluded.set_by,
                                        set_at = excluded.set_at",
    )?
    .execute(params![key, text, by, at.0])?;
    Ok(())
}

/// `runtime_override` delete; a missing key is not an error (the operation is idempotent).
pub fn delete_override(conn: &Connection, key: &str) -> Result<(), DbError> {
    conn.prepare_cached("DELETE FROM runtime_override WHERE key = ?1")?
        .execute([key])?;
    Ok(())
}
