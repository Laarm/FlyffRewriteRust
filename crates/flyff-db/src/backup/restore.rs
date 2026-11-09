//! `flyff db restore <FILE>` (Plan/13 §21.4) and the read-only inspection that `flyff db restore` prints before it
//! asks for confirmation.
//!
//! 1. the instance lock must be free ("stop the server first");
//! 2. the source is verified: a read-only open, `PRAGMA integrity_check` = `ok`, the flyff `application_id`, a
//!    schema version this binary understands;
//! 3. an existing `data/flyff.db` gets a `prerestore` backup first (after a `TRUNCATE` checkpoint). A database so
//!    damaged that it cannot be backed up is not deleted: it is moved aside as `flyff.db.before-restore-<stamp>`
//!    (with its `-wal`/`-shm`), because a restore is exactly what an operator does with a damaged database;
//! 4. the source is copied to `flyff.db.restore-tmp`, fsynced, the stale `-wal`/`-shm` are removed and the copy is
//!    renamed over `flyff.db`;
//! 5. a restored database with an older schema is migrated (with the pre-migration backup of §3);
//! 6. the summary of the restored database is returned. Everything after the backup time is lost; audit logs are
//!    not rewound.

use super::{online_backup, prune_backups, unique_backup_path};
use crate::calendar;
use crate::config::{CheckOnStart, PersistenceConfig};
use crate::conn::{APPLICATION_ID, check_integrity, ensure_private_dir, open_writer, pragma_i64};
use crate::error::{DbError, DbErrorKind};
use crate::lock::InstanceLock;
use crate::migrate::{latest_version, migrate_with_backup, table_exists};
use flyff_common::UnixTime;
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};

/// What a database holds, for the confirmation prompt and the restore summary.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DbSummary {
    /// `PRAGMA user_version`.
    pub user_version: i64,
    /// Rows of `accounts`.
    pub accounts: i64,
    /// Characters with `state = 0`.
    pub characters: i64,
    /// Characters with `state = 1` (soft-deleted, not purged yet).
    pub deleted_characters: i64,
    /// Rows of `guilds` (0 for a schema without the guild group).
    pub guilds: i64,
    /// Mails that are not deleted.
    pub open_mails: i64,
    /// `MAX(characters.saved_at)`: the newest snapshot (Unix seconds).
    pub newest_saved_at: Option<i64>,
    /// Size of the file in bytes.
    pub size_bytes: u64,
}

impl DbSummary {
    /// `YYYY-MM-DD HH:MM:SS UTC` of the newest save, or `-`.
    pub fn newest_save_text(&self) -> String {
        match self.newest_saved_at {
            Some(t) => format!(
                "{} {:02}:{:02}:{:02} UTC",
                calendar::date_string(t.div_euclid(calendar::DAY_S)),
                t.rem_euclid(calendar::DAY_S) / 3600,
                t.rem_euclid(calendar::DAY_S) % 3600 / 60,
                t.rem_euclid(calendar::DAY_S) % 60
            ),
            None => "-".to_owned(),
        }
    }
}

fn count(conn: &Connection, table: &str, filter: &str) -> Result<i64, DbError> {
    if !table_exists(conn, table)? {
        return Ok(0);
    }
    Ok(
        conn.query_row(&format!("SELECT COUNT(*) FROM {table} {filter}"), [], |r| {
            r.get(0)
        })?,
    )
}

/// Counts of `conn` (a migrated or older flyff database).
pub fn summarize(conn: &Connection) -> Result<DbSummary, DbError> {
    let newest_saved_at = if table_exists(conn, "characters")? {
        conn.query_row("SELECT MAX(saved_at) FROM characters", [], |r| {
            r.get::<_, Option<i64>>(0)
        })?
    } else {
        None
    };
    Ok(DbSummary {
        user_version: pragma_i64(conn, "user_version")?,
        accounts: count(conn, "accounts", "")?,
        characters: count(conn, "characters", "WHERE state = 0")?,
        deleted_characters: count(conn, "characters", "WHERE state = 1")?,
        guilds: count(conn, "guilds", "")?,
        open_mails: count(conn, "mails", "WHERE deleted_at IS NULL")?,
        newest_saved_at,
        size_bytes: 0,
    })
}

/// Opens `path` read-only and verifies it the way a restore must: `integrity_check` = `ok`, the flyff
/// `application_id`, `user_version` ≤ the latest schema of this binary. Returns what it holds.
pub fn inspect_database(path: &Path) -> Result<DbSummary, DbError> {
    if !path.is_file() {
        return Err(DbError::new(
            DbErrorKind::NotFound,
            format!("{} is not a file", path.display()),
        ));
    }
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| {
        let mut err = DbError::from(e);
        err.context = format!("{}: {}", path.display(), err.context);
        err
    })?;
    let app_id = pragma_i64(&conn, "application_id").map_err(|e| {
        DbError::new(
            DbErrorKind::Corrupt,
            format!(
                "{} is not a readable database: {}",
                path.display(),
                e.context
            ),
        )
    })?;
    if app_id != APPLICATION_ID {
        return Err(DbError::new(
            DbErrorKind::Corrupt,
            format!(
                "{} is not a flyff database (application_id {app_id:#x}, expected {APPLICATION_ID:#x})",
                path.display()
            ),
        ));
    }
    crate::migrate::guard_version(pragma_i64(&conn, "user_version")?)?;
    check_integrity(&conn, CheckOnStart::Full).map_err(|mut e| {
        e.context = format!("{}: {}", path.display(), e.context);
        e
    })?;
    let mut summary = summarize(&conn)?;
    summary.size_bytes = std::fs::metadata(path).map_or(0, |m| m.len());
    Ok(summary)
}

/// What a restore did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestoreReport {
    /// The restored database.
    pub summary: DbSummary,
    /// The `prerestore` backup of the database that was replaced.
    pub prerestore_backup: Option<PathBuf>,
    /// Where a database that could not be backed up was moved instead.
    pub moved_aside: Option<PathBuf>,
    /// The pre-migration backup, when the restored schema was older.
    pub premigrate_backup: Option<PathBuf>,
    /// Schema versions applied after the restore.
    pub migrated: Vec<u32>,
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Restores `file` over the database of `base_dir` (module documentation lists the steps). `Locked` when a
/// server (or another CLI) holds the instance lock. `now` names the backups.
pub fn restore(
    cfg: &PersistenceConfig,
    base_dir: &Path,
    file: &Path,
    now: UnixTime,
) -> Result<RestoreReport, DbError> {
    cfg.validate()?;
    let db_path = base_dir.join(&cfg.database.path);
    if let Some(parent) = db_path.parent() {
        ensure_private_dir(parent)?;
    }
    // 1. the server must be stopped
    let _lock = InstanceLock::acquire(&db_path).map_err(|mut e| {
        if e.kind == DbErrorKind::Locked {
            e.context = format!("stop the server first: {}", e.context);
        }
        e
    })?;
    // 2. verify the source
    if let (Ok(a), Ok(b)) = (file.canonicalize(), db_path.canonicalize())
        && a == b
    {
        return Err(DbError::new(
            DbErrorKind::Conflict,
            "the file to restore is the live database itself",
        ));
    }
    inspect_database(file)?;

    // 3. keep what is there
    let backups = base_dir.join(&cfg.backup.dir);
    let mut prerestore_backup = None;
    let mut moved_aside = None;
    if db_path.exists() {
        match prerestore(&db_path, &backups, now) {
            Ok(dest) => {
                if let Err(e) = prune_backups(
                    &backups,
                    "prerestore",
                    usize::try_from(cfg.backup.keep_prerestore)
                        .unwrap_or(usize::MAX)
                        .max(1),
                ) {
                    tracing::warn!(error = %e, "could not prune old prerestore backups");
                }
                prerestore_backup = Some(dest);
            }
            Err(e) => {
                let stamp = super::utc_compact(now);
                let aside = with_suffix(&db_path, &format!(".before-restore-{stamp}"));
                tracing::warn!(
                    error = %e,
                    moved_to = %aside.display(),
                    "the current database cannot be backed up: moving it aside"
                );
                std::fs::rename(&db_path, &aside)?;
                for companion in ["-wal", "-shm"] {
                    let from = with_suffix(&db_path, companion);
                    if from.exists() {
                        let _ = std::fs::rename(
                            &from,
                            with_suffix(&db_path, &format!(".before-restore-{stamp}{companion}")),
                        );
                    }
                }
                moved_aside = Some(aside);
            }
        }
    }

    // 4. copy, drop the stale companions, rename over
    let tmp = with_suffix(&db_path, ".restore-tmp");
    let _ = std::fs::remove_file(&tmp);
    let copied = std::fs::copy(file, &tmp)
        .and_then(|_| std::fs::File::open(&tmp)?.sync_all())
        .map_err(DbError::from);
    if let Err(e) = copied {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    let _ = std::fs::remove_file(with_suffix(&db_path, "-wal"));
    let _ = std::fs::remove_file(with_suffix(&db_path, "-shm"));
    std::fs::rename(&tmp, &db_path)?;
    if let Some(dir) = db_path.parent()
        && let Ok(d) = std::fs::File::open(dir)
    {
        let _ = d.sync_all();
    }

    // 5. an older schema is migrated (with the pre-migration backup of §3)
    let mut writer = open_writer(&db_path, &cfg.database)?.conn;
    let migration = migrate_with_backup(&mut writer, &backups, cfg.backup.keep_premigrate, now)?;
    // leave a self-contained file (no WAL) behind
    let _ = writer.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
    let mut summary = summarize(&writer)?;
    drop(writer);
    summary.size_bytes = std::fs::metadata(&db_path).map_or(0, |m| m.len());
    debug_assert!(summary.user_version <= i64::from(latest_version()));
    Ok(RestoreReport {
        summary,
        prerestore_backup,
        moved_aside,
        premigrate_backup: migration.premigrate_backup,
        migrated: migration.applied,
    })
}

/// A `prerestore` backup of the database at `db_path`: open it, `TRUNCATE` checkpoint, online backup.
fn prerestore(db_path: &Path, backups: &Path, now: UnixTime) -> Result<PathBuf, DbError> {
    let conn = Connection::open(db_path)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
    ensure_private_dir(backups)?;
    let dest = unique_backup_path(backups, "prerestore", now);
    online_backup(&conn, &dest)?;
    Ok(dest)
}
