//! Embedded migrations, the version guard, the pre-migration backup and counter repair (Plan/13 §3, §15).
//!
//! * The schema version is `PRAGMA user_version` = the number of applied migrations; there is no
//!   `schema_version` table. `flyff db check` and `flyff version` print it.
//! * `migrations/NNNN_name.sql` files are embedded with `include_str!` and run in order, each in one
//!   `BEGIN IMMEDIATE` transaction together with the `user_version` bump and a `PRAGMA foreign_key_check`
//!   (a violation rolls the migration back and aborts the start). **Append-only** after the first tagged
//!   release (`docs/contracts.toml` `db-migrations`; `migrations/CHECKSUMS` guards accidental edits).
//! * Version guard: a database newer than this binary is refused ([`DbErrorKind::NewerSchema`]).
//! * Pre-migration backup: when a non-empty database is about to be migrated, `backups/flyff-<ts>-premigrate.db`
//!   is written through the online backup API and verified with `quick_check` first; any failure aborts.
//! * After the migrations [`repair_counters`] raises every `meta` counter above the ids already in use and
//!   [`apply_init_only`] records / verifies the init-only configuration values.
//!
//! Deviation from the plan text: the plan names `rusqlite_migration`; the runner here is a small in-house
//! one with identical semantics (`user_version`, one transaction per file) because the pre-migration backup,
//! the `foreign_key_check` step and the dry-run listing need control the crate does not offer, and one
//! dependency less has to match the pinned `rusqlite`.

use crate::backup::{backup_file_name, online_backup, prune_backups};
use crate::config::InitOnlyValues;
use crate::conn::has_user_tables;
use crate::error::{DbError, DbErrorKind};
use flyff_common::UnixTime;
use rusqlite::{Connection, TransactionBehavior};
use std::path::{Path, PathBuf};

/// One embedded migration file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Migration {
    /// 1-based position; equals `user_version` once applied.
    pub version: u32,
    /// File stem without the number, e.g. `core` for `0001_core.sql`.
    pub name: &'static str,
    /// The SQL text.
    pub sql: &'static str,
}

/// Every migration, in order (Plan/13 §1.3).
pub static MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "core",
        sql: include_str!("../migrations/0001_core.sql"),
    },
    Migration {
        version: 2,
        name: "messaging",
        sql: include_str!("../migrations/0002_messaging.sql"),
    },
    Migration {
        version: 3,
        name: "guilds",
        sql: include_str!("../migrations/0003_guilds.sql"),
    },
    Migration {
        version: 4,
        name: "relations",
        sql: include_str!("../migrations/0004_relations.sql"),
    },
    Migration {
        version: 5,
        name: "siege",
        sql: include_str!("../migrations/0005_siege.sql"),
    },
    Migration {
        version: 6,
        name: "realm",
        sql: include_str!("../migrations/0006_realm.sql"),
    },
    Migration {
        version: 7,
        name: "houses_events",
        sql: include_str!("../migrations/0007_houses_events.sql"),
    },
    Migration {
        version: 8,
        name: "admin",
        sql: include_str!("../migrations/0008_admin.sql"),
    },
];

/// `migrations/CHECKSUMS`: `<sha256 hex>  <file name>` per migration (guards accidental edits; verified by a
/// unit test).
pub const CHECKSUMS: &str = include_str!("../migrations/CHECKSUMS");

/// The schema version this binary produces.
pub fn latest_version() -> u32 {
    u32::try_from(MIGRATIONS.len()).unwrap_or(u32::MAX)
}

/// `PRAGMA user_version`.
pub fn user_version(conn: &Connection) -> Result<i64, DbError> {
    crate::conn::pragma_i64(conn, "user_version")
}

/// Refuses a database whose schema is newer than this binary.
pub fn guard_version(current: i64) -> Result<(), DbError> {
    if current > i64::from(latest_version()) {
        return Err(DbError::new(
            DbErrorKind::NewerSchema,
            format!(
                "database schema v{current} is newer than this binary (v{}); restore a backup or upgrade flyff",
                latest_version()
            ),
        ));
    }
    Ok(())
}

/// Where a database stands relative to this binary (`flyff db migrate --dry-run`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationStatus {
    /// `PRAGMA user_version`.
    pub current: i64,
    /// [`latest_version`].
    pub latest: u32,
    /// Migrations that would run, in order.
    pub pending: Vec<Migration>,
}

/// Lists the pending migrations without applying them. Applies the version guard.
pub fn status(conn: &Connection) -> Result<MigrationStatus, DbError> {
    let current = user_version(conn)?;
    guard_version(current)?;
    let pending = MIGRATIONS
        .iter()
        .filter(|m| i64::from(m.version) > current)
        .copied()
        .collect();
    Ok(MigrationStatus {
        current,
        latest: latest_version(),
        pending,
    })
}

/// What [`migrate_with_backup`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrateReport {
    /// `user_version` before.
    pub from: i64,
    /// `user_version` after.
    pub to: i64,
    /// Versions applied by this call.
    pub applied: Vec<u32>,
    /// `true` when the database had no tables before (a new database).
    pub fresh: bool,
    /// The verified pre-migration backup, when one was taken.
    pub premigrate_backup: Option<PathBuf>,
}

/// Applies every pending migration (no backup). Each file runs in one `BEGIN IMMEDIATE` transaction with
/// the `user_version` bump and a foreign-key check; the first failure rolls its own file back and stops.
/// Returns the applied versions.
pub fn migrate(conn: &mut Connection) -> Result<Vec<u32>, DbError> {
    migrate_to(conn, latest_version())
}

/// Like [`migrate`], but stops after version `target`: how the "previous schema" databases of the migration
/// tests and of the per-release fixtures (`tests/fixtures/db/v000N.db`) are built. A `target` at or below the
/// current version applies nothing.
pub fn migrate_to(conn: &mut Connection, target: u32) -> Result<Vec<u32>, DbError> {
    let plan = status(conn)?;
    let mut applied = Vec::new();
    for m in plan.pending.iter().filter(|m| m.version <= target) {
        apply_one(conn, m)?;
        applied.push(m.version);
    }
    Ok(applied)
}

fn apply_one(conn: &mut Connection, m: &Migration) -> Result<(), DbError> {
    let ctx = |e: rusqlite::Error| {
        let mut err = DbError::from(e);
        err.context = format!("migration {:04}_{}: {}", m.version, m.name, err.context);
        err
    };
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(ctx)?;
    tx.execute_batch(m.sql).map_err(ctx)?;
    tx.pragma_update(None, "user_version", i64::from(m.version))
        .map_err(ctx)?;
    let violations = foreign_key_violations(&tx).map_err(ctx)?;
    if violations > 0 {
        // Dropping `tx` rolls the migration back.
        return Err(DbError::new(
            DbErrorKind::Bug,
            format!(
                "migration {:04}_{}: PRAGMA foreign_key_check reports {violations} violation(s)",
                m.version, m.name
            ),
        ));
    }
    tx.commit().map_err(ctx)?;
    tracing::info!(version = m.version, name = m.name, "applied migration");
    Ok(())
}

fn foreign_key_violations(conn: &Connection) -> Result<usize, rusqlite::Error> {
    let mut stmt = conn.prepare("PRAGMA foreign_key_check")?;
    let mut rows = stmt.query([])?;
    let mut n = 0;
    while rows.next()?.is_some() {
        n += 1;
    }
    Ok(n)
}

/// Brings the database to [`latest_version`]: version guard, then (for a non-empty database that is behind)
/// a verified `<backup_dir>/flyff-<ts>-premigrate.db`, then the migrations. `keep_premigrate` bounds the
/// number of `premigrate` files kept (at least the new one).
pub fn migrate_with_backup(
    conn: &mut Connection,
    backup_dir: &Path,
    keep_premigrate: u32,
    now: UnixTime,
) -> Result<MigrateReport, DbError> {
    let plan = status(conn)?;
    let fresh = plan.current == 0 && !has_user_tables(conn)?;
    let mut premigrate_backup = None;
    if !plan.pending.is_empty() && !fresh {
        let mut at = now;
        let dest = loop {
            let candidate = backup_dir.join(backup_file_name("premigrate", at));
            if !candidate.exists() {
                break candidate;
            }
            at = UnixTime(at.0.saturating_add(1));
        };
        online_backup(conn, &dest).map_err(|mut e| {
            e.context = format!("pre-migration backup {}: {}", dest.display(), e.context);
            e
        })?;
        tracing::info!(path = %dest.display(), from = plan.current, "pre-migration backup written");
        let keep = usize::try_from(keep_premigrate)
            .unwrap_or(usize::MAX)
            .max(1);
        if let Err(e) = prune_backups(backup_dir, "premigrate", keep) {
            tracing::warn!(error = %e, "could not prune old premigrate backups");
        }
        premigrate_backup = Some(dest);
    }
    let applied = migrate(conn)?;
    Ok(MigrateReport {
        from: plan.current,
        to: user_version(conn)?,
        applied,
        fresh,
        premigrate_backup,
    })
}

// ---- `flyff db migrate [--dry-run]` ------------------------------------------------------------------------

/// Result of [`cli_migrate`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CliMigrate {
    /// Where the database stood before (for a missing database: version 0, everything pending).
    pub before: MigrationStatus,
    /// What was applied; `None` for a dry run.
    pub report: Option<MigrateReport>,
}

/// The function behind `flyff db migrate [--dry-run]` (Plan/13 §3; CLI wiring is doc 10's WP-OPS-08).
///
/// Refuses with [`DbErrorKind::Locked`] while a server holds `data/flyff.db.lock`. A dry run only reads: it
/// lists the pending migrations and never creates or modifies a file other than the lock. Otherwise the
/// database is created when missing, checked (`check_on_start`), migrated with the pre-migration backup,
/// and the init-only values and counters are verified like [`crate::open`] does.
pub fn cli_migrate(
    cfg: &crate::config::PersistenceConfig,
    base_dir: &Path,
    dry_run: bool,
    now: UnixTime,
) -> Result<CliMigrate, DbError> {
    cfg.validate()?;
    let db_path = base_dir.join(&cfg.database.path);
    let exists = db_path.exists();
    let _lock = if exists || !dry_run {
        if let Some(parent) = db_path.parent() {
            crate::conn::ensure_private_dir(parent)?;
        }
        Some(crate::lock::InstanceLock::acquire(&db_path)?)
    } else {
        None
    };
    if dry_run {
        if !exists {
            return Ok(CliMigrate {
                before: MigrationStatus {
                    current: 0,
                    latest: latest_version(),
                    pending: MIGRATIONS.to_vec(),
                },
                report: None,
            });
        }
        let conn = Connection::open_with_flags(
            &db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let app_id = crate::conn::pragma_i64(&conn, "application_id")?;
        if app_id != 0 && app_id != crate::conn::APPLICATION_ID {
            return Err(DbError::new(
                DbErrorKind::Corrupt,
                format!("{} is not a flyff database", db_path.display()),
            ));
        }
        return Ok(CliMigrate {
            before: status(&conn)?,
            report: None,
        });
    }
    let mut writer = crate::conn::open_writer(&db_path, &cfg.database)?.conn;
    crate::conn::check_integrity(&writer, cfg.database.check_on_start)?;
    let before = status(&writer)?;
    let backup_dir = base_dir.join(&cfg.backup.dir);
    let report = migrate_with_backup(&mut writer, &backup_dir, cfg.backup.keep_premigrate, now)?;
    apply_init_only(&writer, &cfg.init_only)?;
    repair_counters(&writer)?;
    Ok(CliMigrate {
        before,
        report: Some(report),
    })
}

// ---- counters (Plan/13 §15.1) ---------------------------------------------------------------------------

/// `meta` counters whose value is "the next id to hand out" and the table column holding the ids in use.
pub(crate) const NEXT_ID_COUNTERS: &[(&str, &str, &str)] = &[
    ("next_character_id", "characters", "id"),
    ("next_guild_id", "guilds", "id"),
    ("next_mail_id", "mails", "id"),
    ("next_guild_war_id", "guild_wars", "id"),
    ("next_campus_id", "campuses", "id"),
    ("next_couple_id", "couples", "id"),
    ("next_siege_id", "guild_sieges", "id"),
    ("next_lord_election_id", "lord_elections", "id"),
    ("next_guild_furniture_seq", "guild_house_furniture", "seq"),
];

/// `meta` counters holding the *current round number* (not "next") and the table that stores rounds.
pub(crate) const ROUND_COUNTERS: &[(&str, &str, &str)] = &[
    ("tax_round", "tax_owners", "round"),
    ("secret_room_round", "secret_room_tenders", "round"),
    ("rainbow_race_round", "rainbow_race_entries", "round"),
    ("gc1to1_round", "gc1to1_tenders", "round"),
];

/// A counter that [`repair_counters`] had to raise.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CounterRepair {
    /// `meta.key`.
    pub key: &'static str,
    /// Value found.
    pub from: i64,
    /// Value written.
    pub to: i64,
}

pub(crate) fn table_exists(conn: &Connection, table: &str) -> Result<bool, DbError> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [table],
        |r| r.get::<_, i64>(0),
    )? != 0)
}

/// Value of the `meta` counter `key`, `None` when the row does not exist.
pub fn meta_get(conn: &Connection, key: &str) -> Result<Option<i64>, DbError> {
    match conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0)) {
        Ok(v) => Ok(Some(v)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Defensive counter repair (Plan/13 §3, §15.1): raises every `next_*` counter to `MAX(id) + 1` of the table
/// it allocates for and every `*_round` counter to the highest round stored. Counters are only ever raised.
/// Returns the counters that changed (each is logged as a warning: a healthy database never needs this).
pub fn repair_counters(conn: &Connection) -> Result<Vec<CounterRepair>, DbError> {
    let mut repaired = Vec::new();
    let all = NEXT_ID_COUNTERS
        .iter()
        .map(|c| (c, 1_i64))
        .chain(ROUND_COUNTERS.iter().map(|c| (c, 0_i64)));
    for (&(key, table, column), plus) in all {
        if !table_exists(conn, table)? {
            continue;
        }
        let max: i64 = conn.query_row(
            &format!("SELECT COALESCE(MAX({column}), 0) FROM {table}"),
            [],
            |r| r.get(0),
        )?;
        let wanted = max.saturating_add(plus);
        let Some(have) = meta_get(conn, key)? else {
            conn.execute(
                "INSERT INTO meta (key, value) VALUES (?1, ?2)",
                rusqlite::params![key, wanted],
            )?;
            continue;
        };
        if have < wanted {
            conn.execute(
                "UPDATE meta SET value = ?2 WHERE key = ?1",
                rusqlite::params![key, wanted],
            )?;
            tracing::warn!(
                key,
                from = have,
                to = wanted,
                "meta counter raised above the ids in use"
            );
            repaired.push(CounterRepair {
                key,
                from: have,
                to: wanted,
            });
        }
    }
    Ok(repaired)
}

// ---- init-only values (Plan/13 §15.1) ----------------------------------------------------------------------

const INIT_INVENTORY_168: &str = "init.features.inventory_168";
const INIT_BIGGER_POCKET: &str = "init.features.bigger_pocket";
const INIT_MAX_PACK_ITEM: &str = "init.gameplay.max_pack_item";

/// Records the init-only configuration values in `meta.init.*` when they are missing (a new database) and
/// verifies them otherwise (`check_init_only`, Plan/03 §4.1 class I). A mismatch refuses to start with
/// [`DbErrorKind::InitOnlyMismatch`] naming every differing key. Returns `true` when values were written.
pub fn apply_init_only(conn: &Connection, wanted: &InitOnlyValues) -> Result<bool, DbError> {
    let expected = [
        (
            INIT_INVENTORY_168,
            "features.inventory_168",
            i64::from(wanted.inventory_168),
        ),
        (
            INIT_BIGGER_POCKET,
            "features.bigger_pocket",
            i64::from(wanted.bigger_pocket),
        ),
        (
            INIT_MAX_PACK_ITEM,
            "gameplay.max_pack_item",
            i64::from(wanted.max_pack_item),
        ),
    ];
    let mut wrote = false;
    let mut mismatches = Vec::new();
    for (meta_key, config_key, value) in expected {
        match meta_get(conn, meta_key)? {
            None => {
                conn.execute(
                    "INSERT INTO meta (key, value) VALUES (?1, ?2)",
                    rusqlite::params![meta_key, value],
                )?;
                wrote = true;
            }
            Some(stored) if stored != value => mismatches.push(format!(
                "{config_key} is {value} in the configuration but the database was created with {stored}"
            )),
            Some(_) => {}
        }
    }
    if mismatches.is_empty() {
        Ok(wrote)
    } else {
        Err(DbError::new(
            DbErrorKind::InitOnlyMismatch,
            format!(
                "init-only configuration changed after the database was created: {}",
                mismatches.join("; ")
            ),
        ))
    }
}

/// The values recorded by [`apply_init_only`], `None` when the database predates them.
pub fn stored_init_only(conn: &Connection) -> Result<Option<InitOnlyValues>, DbError> {
    let (Some(a), Some(b), Some(c)) = (
        meta_get(conn, INIT_INVENTORY_168)?,
        meta_get(conn, INIT_BIGGER_POCKET)?,
        meta_get(conn, INIT_MAX_PACK_ITEM)?,
    ) else {
        return Ok(None);
    };
    Ok(Some(InitOnlyValues {
        inventory_168: a != 0,
        bigger_pocket: b != 0,
        max_pack_item: u32::try_from(c).unwrap_or(u32::MAX),
    }))
}
