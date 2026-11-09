//! SQLite connections: the writer and the read-only connections, with the pragmas of Plan/13 §2.2.
//!
//! | pragma | where |
//! |---|---|
//! | `page_size = 4096`, `auto_vacuum = INCREMENTAL`, `application_id = 0x464C5946` | once, on a new file |
//! | `application_id` verified | every open (foreign SQLite files are refused) |
//! | `journal_mode = WAL`, `wal_autocheckpoint = 1000`, `journal_size_limit = 64 MiB` | writer |
//! | `synchronous`, `foreign_keys = ON`, `busy_timeout`, `temp_store = MEMORY`, `trusted_schema = OFF` | every connection |
//! | `cache_size` | writer `-(cache_size_mib * 1024)` KiB, readers -16384 |
//! | `query_only = ON` | readers |
//! | `mmap_size = 0` | every connection (default; set explicitly to avoid SIGBUS on I/O errors) |

use crate::config::{CheckOnStart, DbConfig};
use crate::error::{DbError, DbErrorKind};
use rusqlite::{Connection, OpenFlags};
use std::path::Path;
use std::time::Duration;

/// `PRAGMA application_id` of every flyff database: "FLYF".
pub const APPLICATION_ID: i64 = 0x464C_5946;
/// `PRAGMA page_size` of a new database.
pub const PAGE_SIZE: i64 = 4096;
/// `PRAGMA auto_vacuum = INCREMENTAL` as reported by `PRAGMA auto_vacuum`.
pub const AUTO_VACUUM_INCREMENTAL: i64 = 2;
/// Reader page cache in KiB (`PRAGMA cache_size = -16384`, 16 MiB).
pub const READER_CACHE_KIB: i64 = 16 * 1024;
/// `PRAGMA journal_size_limit` of the writer (64 MiB).
pub const JOURNAL_SIZE_LIMIT: i64 = 64 * 1024 * 1024;
/// `PRAGMA wal_autocheckpoint` of the writer (pages).
pub const WAL_AUTOCHECKPOINT: i64 = 1000;

/// What [`open_writer`] found.
#[derive(Debug)]
pub struct WriterOpen {
    /// The read-write connection, all pragmas applied.
    pub conn: Connection,
    /// `true` when the file was new (or an empty skeleton database): `application_id`, `page_size` and
    /// `auto_vacuum` were set by this call.
    pub fresh: bool,
}

/// Opens (creating when missing) the database at `path` as the single read-write connection.
///
/// A new or empty file is stamped with `application_id`, `page_size`, `auto_vacuum = INCREMENTAL` and put
/// into WAL mode. An existing file must carry the flyff `application_id`; any other SQLite file is refused
/// (`Corrupt`, "not a flyff database").
pub fn open_writer(path: &Path, cfg: &DbConfig) -> Result<WriterOpen, DbError> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| with_path(e, path))?;
    conn.busy_timeout(Duration::from_millis(u64::from(cfg.busy_timeout_ms)))?;

    // Reading the application id forces SQLite to read the header: a non-database file fails here.
    let app_id = pragma_i64(&conn, "application_id").map_err(|e| with_path(e, path))?;
    let has_tables = has_user_tables(&conn).map_err(|e| with_path(e, path))?;
    let fresh = app_id == 0 && !has_tables;
    if !fresh && app_id != APPLICATION_ID {
        return Err(DbError::new(
            DbErrorKind::Corrupt,
            format!(
                "{} is not a flyff database (application_id {app_id:#x}, expected {APPLICATION_ID:#x})",
                path.display()
            ),
        ));
    }
    if fresh {
        // Must happen before the first table exists.
        conn.pragma_update(None, "page_size", PAGE_SIZE)?;
        conn.pragma_update(None, "auto_vacuum", AUTO_VACUUM_INCREMENTAL)?;
        conn.pragma_update(None, "application_id", APPLICATION_ID)?;
    }
    let mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(DbError::new(
            DbErrorKind::Bug,
            format!("journal_mode is {mode}, expected wal ({})", path.display()),
        ));
    }
    if fresh && pragma_i64(&conn, "auto_vacuum")? != AUTO_VACUUM_INCREMENTAL {
        // An empty database created earlier (the walking skeleton) was stamped with auto_vacuum = NONE; the
        // mode of an existing file only changes through VACUUM, which is trivial without tables.
        conn.execute_batch("VACUUM")?;
    }
    common_pragmas(&conn, cfg)?;
    conn.pragma_update(
        None,
        "cache_size",
        -(i64::from(cfg.cache_size_mib).saturating_mul(1024)),
    )?;
    conn.pragma_update(None, "wal_autocheckpoint", WAL_AUTOCHECKPOINT)?;
    conn.pragma_update(None, "journal_size_limit", JOURNAL_SIZE_LIMIT)?;
    Ok(WriterOpen { conn, fresh })
}

/// Opens a read-only (`query_only`) connection on an existing database.
pub fn open_reader(path: &Path, cfg: &DbConfig) -> Result<Connection, DbError> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| with_path(e, path))?;
    conn.busy_timeout(Duration::from_millis(u64::from(cfg.busy_timeout_ms)))?;
    let app_id = pragma_i64(&conn, "application_id").map_err(|e| with_path(e, path))?;
    if app_id != APPLICATION_ID {
        return Err(DbError::new(
            DbErrorKind::Corrupt,
            format!(
                "{} is not a flyff database (application_id {app_id:#x})",
                path.display()
            ),
        ));
    }
    common_pragmas(&conn, cfg)?;
    conn.pragma_update(None, "cache_size", -READER_CACHE_KIB)?;
    conn.pragma_update(None, "query_only", "ON")?;
    Ok(conn)
}

/// Pragmas shared by every connection (`synchronous` is only meaningful for the writer but harmless for
/// readers and keeps the rule "every connection" of Plan/13 §2.2).
fn common_pragmas(conn: &Connection, cfg: &DbConfig) -> Result<(), DbError> {
    conn.pragma_update(None, "synchronous", cfg.synchronous.pragma_value())?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    conn.pragma_update(None, "trusted_schema", "OFF")?;
    conn.pragma_update(None, "mmap_size", 0_i64)?;
    Ok(())
}

fn with_path(e: impl Into<DbError>, path: &Path) -> DbError {
    let mut err: DbError = e.into();
    err.context = format!("{}: {}", path.display(), err.context);
    err
}

/// Creates `dir` (and its parents) when missing; a directory created here gets mode 0700 on Unix
/// (`data/` and `backups/` hold the database and its backups, Plan/13 §2.1). Existing directories are left
/// alone.
pub fn ensure_private_dir(dir: &Path) -> Result<(), DbError> {
    if dir.is_dir() {
        return Ok(());
    }
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Reads an integer pragma.
pub fn pragma_i64(conn: &Connection, name: &str) -> Result<i64, DbError> {
    Ok(conn.query_row(&format!("PRAGMA {name}"), [], |r| r.get::<_, i64>(0))?)
}

/// Reads a text pragma.
pub fn pragma_text(conn: &Connection, name: &str) -> Result<String, DbError> {
    Ok(conn.query_row(&format!("PRAGMA {name}"), [], |r| r.get::<_, String>(0))?)
}

/// `true` when the database has at least one table.
pub(crate) fn has_user_tables(conn: &Connection) -> Result<bool, DbError> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table')",
        [],
        |r| r.get::<_, i64>(0),
    )? != 0)
}

/// Runs `PRAGMA quick_check` / `PRAGMA integrity_check` (`[database] check_on_start`, Plan/13 §20).
///
/// `Off` does nothing. A failed check is `Corrupt` and carries the first lines SQLite reports.
pub fn check_integrity(conn: &Connection, mode: CheckOnStart) -> Result<(), DbError> {
    let pragma = match mode {
        CheckOnStart::Off => return Ok(()),
        CheckOnStart::Quick => "quick_check",
        CheckOnStart::Full => "integrity_check",
    };
    let mut stmt = conn.prepare(&format!("PRAGMA {pragma}"))?;
    let lines: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<_, _>>()?;
    if lines.len() == 1 && lines.first().is_some_and(|l| l == "ok") {
        Ok(())
    } else {
        let shown: Vec<&str> = lines.iter().take(5).map(String::as_str).collect();
        Err(DbError::new(
            DbErrorKind::Corrupt,
            format!("{pragma} failed: {}", shown.join("; ")),
        ))
    }
}
