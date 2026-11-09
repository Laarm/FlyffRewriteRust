//! Database maintenance (Plan/13 §22, §23; WP-DB-08, WP-OPS-14): the WAL checkpoint policy of the writer, the
//! offline `flyff db vacuum` and `flyff character undelete`.
//!
//! The two commands change data files that a running server owns, so they take the instance lock first and refuse
//! with `Locked` (exit code 75) when it is held (Plan/13 §2.3).
//!
//! The daily retention job (WP-DB-16, Plan/13 §23) is [`run_daily`]: the writer thread runs it in its own
//! transaction when the game thread sends `MaintenanceJob::Daily` at `[retention] maintenance_hour`.

use crate::backup::{BackupClock, online_backup, require_free_space, unique_backup_path};
use crate::cmd::CheckpointMode;
use crate::config::{PersistenceConfig, RetentionConfig};
use crate::conn::{ensure_private_dir, open_writer};
use crate::error::{DbError, DbErrorKind};
use crate::lock::InstanceLock;
use flyff_common::{AccountId, CharacterId, UnixTime};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The periodic checkpoint restarts the WAL when it is still larger than this after the `PASSIVE` pass
/// (Plan/13 §22).
pub const WAL_RESTART_BYTES: u64 = 64 * 1024 * 1024;

/// The checkpoints the writer runs in its idle timer (Plan/13 §22): always a `PASSIVE` one; when the WAL is
/// still above [`WAL_RESTART_BYTES`] afterwards and no backup is running, a `RESTART` (which waits for readers up
/// to the busy timeout, so it is held back while the backup's read transaction is open).
pub fn periodic_checkpoints(
    wal_bytes_after_passive: u64,
    backup_running: bool,
) -> Vec<CheckpointMode> {
    let mut modes = vec![CheckpointMode::Passive];
    if wal_bytes_after_passive > WAL_RESTART_BYTES && !backup_running {
        modes.push(CheckpointMode::Restart);
    }
    modes
}

/// The size of the `-wal` file next to `db_path` (0 when there is none).
pub fn wal_size(db_path: &Path) -> u64 {
    let mut name = db_path.as_os_str().to_owned();
    name.push("-wal");
    std::fs::metadata(PathBuf::from(name)).map_or(0, |m| m.len())
}

// ---- the daily retention job (Plan/13 §23, WP-DB-16) -----------------------------------------------------------

/// Pages `PRAGMA incremental_vacuum` gives back to the file system per daily run (Plan/13 §23 "Space and stats").
pub const INCREMENTAL_VACUUM_PAGES: u32 = 2000;

/// What one daily retention run removed (`MaintenanceJob::Daily`, `PersistHandle::last_maintenance`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaintenanceReport {
    /// The `now` of the run.
    pub at: UnixTime,
    /// Soft-deleted characters purged (rows of `characters`; their documents, bank, friends, mails received,
    /// notes, grants, removals, couple / campus membership, party name, ... go with them by cascade).
    pub characters_purged: u64,
    /// Soft-deleted mails removed after `[retention] deleted_mail_days`.
    pub mails_purged: u64,
    /// Delivered notes removed after `delivered_note_days`.
    pub notes_purged: u64,
    /// Delivered grants removed after `finished_grant_days`.
    pub grants_purged: u64,
    /// Finished removals removed after `finished_grant_days`.
    pub removals_purged: u64,
    /// Cancelled grants and removals removed after `finished_grant_days` (counted apart from the delivered ones).
    pub cancelled_purged: u64,
    /// `admin_idempotency` rows older than 24 h (Plan/25 §11.6).
    pub idempotency_purged: u64,
    /// `guild_bank_log` rows beyond the newest `guild_bank_log_per_kind` of each guild and kind.
    pub guild_bank_log_trimmed: u64,
    /// Expired `couple_proposals` rows.
    pub couple_proposals_purged: u64,
    /// `guild_rejoin_cooldowns` rows whose `until` has passed (0003).
    pub rejoin_cooldowns_purged: u64,
    /// `incremental_vacuum` ran (skipped while a backup holds a read transaction open; the next run catches up).
    pub vacuumed: bool,
    /// Wall time of the run in milliseconds (informational).
    pub elapsed_ms: u64,
}

impl MaintenanceReport {
    /// Rows removed in total.
    pub fn rows_removed(&self) -> u64 {
        self.characters_purged
            + self.mails_purged
            + self.notes_purged
            + self.grants_purged
            + self.removals_purged
            + self.cancelled_purged
            + self.idempotency_purged
            + self.guild_bank_log_trimmed
            + self.couple_proposals_purged
            + self.rejoin_cooldowns_purged
    }
}

fn cutoff(now: UnixTime, days: u32) -> i64 {
    now.0.saturating_sub(i64::from(days) * 86_400)
}

fn removed(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

/// The retention steps of Plan/13 §23 on `conn`, **inside the caller's transaction** (the writer opens one
/// `BEGIN IMMEDIATE` for the run):
///
/// 1. purge the characters in `purge` - only those that are soft-deleted (`state = 1`) for at least
///    `deleted_character_days` at `now` (the game selects them from its directory; this re-check is the
///    safety net) - with the cascades and the couples / campuses they leave empty ([`crate::repo::characters::purge`]),
/// 2. delete soft-deleted mails older than `deleted_mail_days`,
/// 3. delete delivered notes older than `delivered_note_days`,
/// 4. delete delivered or cancelled grants and finished or cancelled removals older than `finished_grant_days`
///    and the `admin_idempotency` rows older than 24 h,
/// 5. keep the newest `guild_bank_log_per_kind` `guild_bank_log` rows of each (guild, kind),
/// 6. delete expired `couple_proposals`,
/// 7. delete `guild_rejoin_cooldowns` whose `until` has passed.
///
/// Every step is idempotent: a second run with the same `now` removes nothing. Mail expiry (soft delete after
/// 15 days) is the `MailService`'s (`DbOp::ExpireMails` / `UpdateMail`), audit files and backups rotate in their
/// own threads. The space step (`incremental_vacuum`, `PRAGMA optimize`) cannot run inside a transaction and is
/// done by the writer afterwards.
pub fn run_daily(
    conn: &Connection,
    cfg: &RetentionConfig,
    purge: &[CharacterId],
    now: UnixTime,
) -> Result<MaintenanceReport, DbError> {
    let mut report = MaintenanceReport {
        at: now,
        ..MaintenanceReport::default()
    };

    // 1. characters: only soft-deleted ones that are old enough, whatever the caller asked for
    let character_cutoff = cutoff(now, cfg.deleted_character_days);
    let mut due = Vec::new();
    {
        let mut stmt = conn.prepare_cached(
            "SELECT id FROM characters WHERE id = ?1 AND state = 1 AND deleted_at IS NOT NULL AND deleted_at < ?2",
        )?;
        for id in purge {
            if stmt
                .query_row(params![id.0, character_cutoff], |r| r.get::<_, u32>(0))
                .optional()?
                .is_some()
            {
                due.push(*id);
            } else {
                tracing::warn!(
                    character = id.0,
                    "maintenance: a character in the purge list is not due (or not deleted) and is kept"
                );
            }
        }
    }
    report.characters_purged = removed(crate::repo::characters::purge(conn, &due)?);

    // 2.-4. rows that were finished a while ago
    report.mails_purged = removed(
        conn.prepare_cached("DELETE FROM mails WHERE deleted_at IS NOT NULL AND deleted_at < ?1")?
            .execute([cutoff(now, cfg.deleted_mail_days)])?,
    );
    report.notes_purged = removed(
        conn.prepare_cached(
            "DELETE FROM notes WHERE delivered_at IS NOT NULL AND delivered_at < ?1",
        )?
        .execute([cutoff(now, cfg.delivered_note_days)])?,
    );
    let finished = cutoff(now, cfg.finished_grant_days);
    report.grants_purged = removed(
        conn.prepare_cached(
            "DELETE FROM pending_grants WHERE delivered_at IS NOT NULL AND delivered_at < ?1",
        )?
        .execute([finished])?,
    );
    report.removals_purged = removed(
        conn.prepare_cached(
            "DELETE FROM pending_removals WHERE done_at IS NOT NULL AND done_at < ?1",
        )?
        .execute([finished])?,
    );
    // cancelled rows (migration 0008) are finished too; a row is never both cancelled and delivered
    report.cancelled_purged = removed(
        conn.prepare_cached(
            "DELETE FROM pending_grants WHERE cancelled_at IS NOT NULL AND cancelled_at < ?1",
        )?
        .execute([finished])?,
    ) + removed(
        conn.prepare_cached(
            "DELETE FROM pending_removals WHERE cancelled_at IS NOT NULL AND cancelled_at < ?1",
        )?
        .execute([finished])?,
    );
    // idempotency keys live for 24 h (Plan/25 §11.6)
    report.idempotency_purged = removed(crate::admin::idempotency::purge(conn, now)?);

    // 5. the guild bank log keeps the newest N rows per guild and kind
    report.guild_bank_log_trimmed = removed(
        conn.prepare_cached(
            "DELETE FROM guild_bank_log WHERE id IN (
               SELECT id FROM (
                 SELECT id, ROW_NUMBER() OVER (PARTITION BY guild_id, kind ORDER BY id DESC) AS rn
                 FROM guild_bank_log
               ) WHERE rn > ?1)",
        )?
        .execute([i64::from(cfg.guild_bank_log_per_kind)])?,
    );

    // 6. couple proposals whose cooldown is over
    report.couple_proposals_purged = removed(
        conn.prepare_cached("DELETE FROM couple_proposals WHERE cooldown_until < ?1")?
            .execute([now.0])?,
    );

    // 7. guild rejoin cooldowns that are over (the game compares `until` with its clock; an old row is clutter)
    report.rejoin_cooldowns_purged = removed(
        conn.prepare_cached("DELETE FROM guild_rejoin_cooldowns WHERE until < ?1")?
            .execute([now.0])?,
    );
    Ok(report)
}

/// The space step after the daily transaction: `PRAGMA incremental_vacuum(N)` returns free pages to the file
/// system, then `PRAGMA optimize` refreshes the planner statistics. `vacuum = false` (a backup is running: its
/// read transaction pins the pages) skips the vacuum; the next run catches up. Returns whether it vacuumed.
pub fn space_and_stats(conn: &Connection, vacuum: bool) -> Result<bool, DbError> {
    if vacuum {
        conn.execute_batch(&format!(
            "PRAGMA incremental_vacuum({INCREMENTAL_VACUUM_PAGES})"
        ))?;
    }
    conn.execute_batch("PRAGMA optimize")?;
    Ok(vacuum)
}

// ---- vacuum ----------------------------------------------------------------------------------------------------

/// What `flyff db vacuum` did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VacuumReport {
    /// The manual backup written first.
    pub backup: PathBuf,
    /// Database size (file plus WAL) before.
    pub bytes_before: u64,
    /// Database size after `VACUUM` and the `TRUNCATE` checkpoint.
    pub bytes_after: u64,
}

fn footprint(db_path: &Path) -> u64 {
    std::fs::metadata(db_path)
        .map_or(0, |m| m.len())
        .saturating_add(wal_size(db_path))
}

/// `flyff db vacuum` (Plan/10 §9.2, R13): with the instance lock held, write a `manual` backup, then `VACUUM`
/// and `wal_checkpoint(TRUNCATE)`. Refuses with `Locked` while a server runs.
pub fn vacuum(
    cfg: &PersistenceConfig,
    base_dir: &Path,
    clock: &Arc<dyn BackupClock>,
) -> Result<VacuumReport, DbError> {
    cfg.validate()?;
    let db_path = base_dir.join(&cfg.database.path);
    if !db_path.is_file() {
        return Err(DbError::new(
            DbErrorKind::NotFound,
            format!("there is no database at {}", db_path.display()),
        ));
    }
    let _lock = InstanceLock::acquire(&db_path).map_err(|mut e| {
        if e.kind == DbErrorKind::Locked {
            e.context = format!("stop the server first: {}", e.context);
        }
        e
    })?;
    let conn = open_writer(&db_path, &cfg.database)?.conn;
    let backups = base_dir.join(&cfg.backup.dir);
    ensure_private_dir(&backups)?;
    // the backup and VACUUM's temporary copy both need room
    require_free_space(&db_path, &backups, cfg.backup.min_free_space_factor)?;
    let backup = unique_backup_path(&backups, "manual", clock.now());
    online_backup(&conn, &backup)?;
    let bytes_before = footprint(&db_path);
    conn.execute_batch("VACUUM")?;
    conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
    drop(conn);
    Ok(VacuumReport {
        backup,
        bytes_before,
        bytes_after: footprint(&db_path),
    })
}

// ---- character undelete ----------------------------------------------------------------------------------------

/// What `flyff character undelete` restored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UndeleteReport {
    /// The character.
    pub character: CharacterId,
    /// Its name.
    pub name: String,
    /// Its account.
    pub account: AccountId,
    /// The slot it now occupies.
    pub slot: u8,
    /// The slot it had when it was deleted.
    pub previous_slot: u8,
    /// When it had been deleted.
    pub deleted_at: Option<UnixTime>,
}

/// `flyff character undelete <NAME|ID> [--slot N]` (Plan/13 §14.2, R13): restores a soft-deleted character when
/// its name and a slot are free. The slot is `slot` when given, else the slot it had, else the lowest free one.
///
/// Errors (all carry a message for the operator): `NotFound` (no such deleted character, or it was purged),
/// `Conflict` (the character is not deleted), `NameTaken` (a live character uses the name now), `SlotTaken` (the
/// requested slot is occupied, or no slot is free). `Locked` while a server runs.
pub fn undelete_character(
    cfg: &PersistenceConfig,
    base_dir: &Path,
    target: &str,
    slot: Option<u8>,
) -> Result<UndeleteReport, DbError> {
    cfg.validate()?;
    let db_path = base_dir.join(&cfg.database.path);
    if !db_path.is_file() {
        return Err(DbError::new(
            DbErrorKind::NotFound,
            format!("there is no database at {}", db_path.display()),
        ));
    }
    if let Some(s) = slot
        && s > 2
    {
        return Err(DbError::new(
            DbErrorKind::SlotTaken,
            format!("slot {s} does not exist (slots are 0, 1 and 2)"),
        ));
    }
    let _lock = InstanceLock::acquire(&db_path).map_err(|mut e| {
        if e.kind == DbErrorKind::Locked {
            e.context = format!("stop the server first: {}", e.context);
        }
        e
    })?;
    let mut conn = open_writer(&db_path, &cfg.database)?.conn;
    let report = undelete_in(&mut conn, target, slot)?;
    conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
    Ok(report)
}

struct DeletedRow {
    id: u32,
    account: u32,
    name: String,
    slot: u8,
    state: i64,
    deleted_at: Option<i64>,
}

fn find_character(conn: &Connection, target: &str) -> Result<Option<DeletedRow>, DbError> {
    let read = |r: &rusqlite::Row<'_>| -> rusqlite::Result<DeletedRow> {
        Ok(DeletedRow {
            id: r.get(0)?,
            account: r.get(1)?,
            name: r.get(2)?,
            slot: r.get(3)?,
            state: r.get(4)?,
            deleted_at: r.get(5)?,
        })
    };
    const COLUMNS: &str = "id, account_id, name, slot, state, deleted_at";
    // a number is an id first (a character may also be named "123"), then a name
    if let Ok(id) = target.trim().parse::<u32>() {
        let row = conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM characters WHERE id = ?1"),
                [id],
                read,
            )
            .optional()?;
        if row.is_some() {
            return Ok(row);
        }
    }
    // the most recently deleted character of that name; a live one is reported as "not deleted"
    Ok(conn
        .query_row(
            &format!(
                "SELECT {COLUMNS} FROM characters WHERE name = ?1 COLLATE NOCASE
                 ORDER BY (state = 1) DESC, deleted_at DESC, id DESC LIMIT 1"
            ),
            [target.trim()],
            read,
        )
        .optional()?)
}

fn undelete_in(
    conn: &mut Connection,
    target: &str,
    slot: Option<u8>,
) -> Result<UndeleteReport, DbError> {
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let Some(row) = find_character(&tx, target)? else {
        return Err(DbError::new(
            DbErrorKind::NotFound,
            format!(
                "no character '{target}' (a deleted character is purged after [retention] deleted_character_days)"
            ),
        ));
    };
    if row.state != 1 {
        return Err(DbError::new(
            DbErrorKind::Conflict,
            format!(
                "character '{}' (#{}) is {}, not deleted",
                row.name,
                row.id,
                if row.state == 2 { "sealed" } else { "active" }
            ),
        ));
    }
    let live_owner: Option<u32> = tx
        .query_row(
            "SELECT id FROM characters WHERE name = ?1 COLLATE NOCASE AND state <> 1",
            [&row.name],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(other) = live_owner {
        return Err(DbError::new(
            DbErrorKind::NameTaken,
            format!(
                "the name '{}' is used by character #{other} now: rename that character first",
                row.name
            ),
        ));
    }
    let taken: Vec<u8> = {
        let mut stmt =
            tx.prepare("SELECT slot FROM characters WHERE account_id = ?1 AND state = 0")?;
        stmt.query_map([row.account], |r| r.get(0))?
            .collect::<Result<_, _>>()?
    };
    let chosen = match slot {
        Some(s) if taken.contains(&s) => {
            return Err(DbError::new(
                DbErrorKind::SlotTaken,
                format!("slot {s} of account #{} is occupied", row.account),
            ));
        }
        Some(s) => s,
        None if !taken.contains(&row.slot) => row.slot,
        None => (0..=2_u8).find(|s| !taken.contains(s)).ok_or_else(|| {
            DbError::new(
                DbErrorKind::SlotTaken,
                format!(
                    "account #{} has no free character slot: delete a character or pass --slot",
                    row.account
                ),
            )
        })?,
    };
    let changed = tx.execute(
        "UPDATE characters SET state = 0, deleted_at = NULL, slot = ?2 WHERE id = ?1 AND state = 1",
        params![row.id, chosen],
    )?;
    if changed != 1 {
        return Err(DbError::new(
            DbErrorKind::Bug,
            format!("character #{} changed while it was restored", row.id),
        ));
    }
    tx.commit()?;
    Ok(UndeleteReport {
        character: CharacterId(row.id),
        name: row.name,
        account: AccountId(row.account),
        slot: chosen,
        previous_slot: row.slot,
        deleted_at: row.deleted_at.map(UnixTime),
    })
}
