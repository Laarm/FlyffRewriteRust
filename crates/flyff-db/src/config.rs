//! Persistence configuration: the `[database]`, `[save]`, `[backup]`, `[audit]` and `[retention]` sections of
//! `flyff.toml` (Plan/13 §2.4, §16.1) and their documentation table [`KEY_DOCS`].
//!
//! Defaults are exactly the §2.4 block. The sections deny unknown keys so a typo in `flyff.toml` is an error
//! instead of a silently ignored setting. [`PersistenceConfig::init_only`] is not a TOML key: the server
//! fills it from the feature/gameplay configuration before calling [`crate::open`].

use crate::error::{DbError, DbErrorKind};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// `PRAGMA synchronous` setting (`[database] synchronous`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Synchronous {
    /// `OFF`: unsafe against power loss; tests and benchmarks only (not offered by `flyff.toml` docs).
    Off,
    /// `NORMAL` (default with WAL, D5).
    #[default]
    Normal,
    /// `FULL`.
    Full,
}

impl Synchronous {
    /// The value of `PRAGMA synchronous = <value>`.
    pub fn pragma_value(self) -> &'static str {
        match self {
            Synchronous::Off => "OFF",
            Synchronous::Normal => "NORMAL",
            Synchronous::Full => "FULL",
        }
    }
}

/// Integrity check at start (`[database] check_on_start`, Plan/13 §20).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckOnStart {
    /// No check.
    Off,
    /// `PRAGMA quick_check`.
    #[default]
    Quick,
    /// `PRAGMA integrity_check`.
    Full,
}

/// `[database]` (Plan/13 §2.4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DbConfig {
    /// Database file, relative to the home directory.
    pub path: PathBuf,
    /// `PRAGMA synchronous` (`"normal"` | `"full"`).
    pub synchronous: Synchronous,
    /// `PRAGMA busy_timeout` in milliseconds (every connection).
    pub busy_timeout_ms: u32,
    /// Writer page cache in MiB (readers use 16).
    pub cache_size_mib: u32,
    /// Number of read-only connections/threads (1..=4).
    pub read_connections: u8,
    /// Actor drain limit: commands per batch.
    pub batch_max_commands: u32,
    /// Actor drain window after the first command, in milliseconds.
    pub batch_max_wait_ms: u32,
    /// Enter-world wait for the previous session's logout save, in milliseconds.
    pub flush_wait_timeout_ms: u32,
    /// Item serial block reservation size.
    pub serial_block_size: u32,
    /// Periodic `PASSIVE` checkpoint interval in seconds.
    pub checkpoint_interval_s: u32,
    /// Integrity check at start.
    pub check_on_start: CheckOnStart,
}

impl Default for DbConfig {
    fn default() -> Self {
        Self {
            path: PathBuf::from("data/flyff.db"),
            synchronous: Synchronous::Normal,
            busy_timeout_ms: 5000,
            cache_size_mib: 64,
            read_connections: 1,
            batch_max_commands: 256,
            batch_max_wait_ms: 20,
            flush_wait_timeout_ms: 5000,
            serial_block_size: 10_000,
            checkpoint_interval_s: 300,
            check_on_start: CheckOnStart::Quick,
        }
    }
}

/// `[save]` (Plan/13 §2.4, §17).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SaveConfig {
    /// Periodic snapshot interval per character in seconds (`SEC_SAVEPLAYER`).
    pub interval_s: u32,
    /// Cap of periodic snapshots per 66.7 ms tick.
    pub max_periodic_per_tick: u32,
    /// Minimum distance between two "urgent" snapshots of one character, in milliseconds.
    pub urgent_min_interval_ms: u32,
    /// Snapshot when changing world id.
    pub on_world_change: bool,
}

impl Default for SaveConfig {
    fn default() -> Self {
        Self {
            interval_s: 90,
            max_periodic_per_tick: 8,
            urgent_min_interval_ms: 1000,
            on_world_change: true,
        }
    }
}

/// `[backup]` (Plan/13 §2.4, §21).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BackupConfig {
    /// Backup directory, relative to the home directory.
    pub dir: PathBuf,
    /// Scheduled backup interval in minutes.
    pub interval_min: u32,
    /// Hourly generations kept.
    pub keep_hourly: u32,
    /// Daily generations kept.
    pub keep_daily: u32,
    /// Weekly generations kept.
    pub keep_weekly: u32,
    /// `premigrate` backups kept.
    pub keep_premigrate: u32,
    /// `prerestore` backups kept.
    pub keep_prerestore: u32,
    /// `shutdown` backups kept.
    pub keep_shutdown: u32,
    /// Write a backup on graceful shutdown.
    pub on_shutdown: bool,
    /// Skip a backup when free space is below `factor x db size`.
    pub min_free_space_factor: f64,
    /// Hook argv; the backup path and kind are appended.
    pub hook: Vec<String>,
    /// Hook timeout in seconds.
    pub hook_timeout_s: u32,
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self {
            dir: PathBuf::from("backups"),
            interval_min: 60,
            keep_hourly: 24,
            keep_daily: 7,
            keep_weekly: 4,
            keep_premigrate: 5,
            keep_prerestore: 5,
            keep_shutdown: 5,
            on_shutdown: false,
            min_free_space_factor: 2.0,
            hook: Vec::new(),
            hook_timeout_s: 600,
        }
    }
}

/// `[audit]` (Plan/13 §2.4, §25).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AuditConfig {
    /// Write the audit log.
    pub enabled: bool,
    /// Audit directory, relative to the home directory.
    pub dir: PathBuf,
    /// Days of audit files kept.
    pub retention_days: u32,
    /// Event names or prefixes that are not logged (`"item.pickup"`, `"session."`).
    pub exclude: Vec<String>,
    /// Capacity of the bounded audit queue.
    pub queue_capacity: u32,
    /// An admin console mutation whose audit record the writer cannot acknowledge within 1 s is not dispatched and
    /// answers `503 audit_unavailable` (Plan/25 §10.1). The events `admin.*` and `gm.command` can not be excluded
    /// by `exclude`.
    pub admin_required: bool,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            dir: PathBuf::from("logs/audit"),
            retention_days: 90,
            exclude: Vec::new(),
            queue_capacity: 65_536,
            admin_required: true,
        }
    }
}

/// `[retention]` (Plan/13 §2.4, §23).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RetentionConfig {
    /// Days a soft-deleted character is kept before the purge.
    pub deleted_character_days: u32,
    /// Days a soft-deleted mail is kept.
    pub deleted_mail_days: u32,
    /// Days a delivered note is kept.
    pub delivered_note_days: u32,
    /// Days a finished grant is kept.
    pub finished_grant_days: u32,
    /// Newest `guild_bank_log` rows kept per kind and guild.
    pub guild_bank_log_per_kind: u32,
    /// Local hour of the daily maintenance job.
    pub maintenance_hour: u8,
}

impl Default for RetentionConfig {
    fn default() -> Self {
        Self {
            deleted_character_days: 7,
            deleted_mail_days: 30,
            delivered_note_days: 30,
            finished_grant_days: 90,
            guild_bank_log_per_kind: 100,
            maintenance_hour: 5,
        }
    }
}

/// The init-only configuration values recorded in `meta.init.*` when the database is created and compared at
/// every start (Plan/13 §15.1, 03 §4.1 class I): the canonical type of WP-CORE-90 in `flyff-common`.
pub use flyff_common::config::InitOnlyValues;

/// All persistence settings (Plan/13 §16.1).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PersistenceConfig {
    /// `[database]`.
    pub database: DbConfig,
    /// `[save]`.
    pub save: SaveConfig,
    /// `[backup]`.
    pub backup: BackupConfig,
    /// `[audit]`.
    pub audit: AuditConfig,
    /// `[retention]`.
    pub retention: RetentionConfig,
    /// Init-only values of this run; never part of the TOML.
    #[serde(skip)]
    pub init_only: InitOnlyValues,
}

impl PersistenceConfig {
    /// Checks the ranges of Plan/13 §2.4. Returns `InvalidConfig` naming the first offending key.
    pub fn validate(&self) -> Result<(), DbError> {
        fn check(ok: bool, key: &str, rule: &str) -> Result<(), DbError> {
            if ok {
                Ok(())
            } else {
                Err(DbError::new(
                    DbErrorKind::InvalidConfig,
                    format!("{key} must be {rule}"),
                ))
            }
        }
        let d = &self.database;
        check(
            (1..=4).contains(&d.read_connections),
            "database.read_connections",
            "1..=4",
        )?;
        check(
            d.batch_max_commands >= 1,
            "database.batch_max_commands",
            ">= 1",
        )?;
        check(d.cache_size_mib >= 1, "database.cache_size_mib", ">= 1")?;
        check(
            d.serial_block_size >= 1,
            "database.serial_block_size",
            ">= 1",
        )?;
        check(!d.path.as_os_str().is_empty(), "database.path", "not empty")?;
        check(self.save.interval_s >= 1, "save.interval_s", ">= 1")?;
        check(
            self.save.max_periodic_per_tick >= 1,
            "save.max_periodic_per_tick",
            ">= 1",
        )?;
        check(
            self.backup.min_free_space_factor.is_finite()
                && self.backup.min_free_space_factor >= 0.0,
            "backup.min_free_space_factor",
            "a finite number >= 0",
        )?;
        check(
            self.retention.maintenance_hour <= 23,
            "retention.maintenance_hour",
            "0..=23",
        )?;
        check(
            self.audit.queue_capacity >= 1,
            "audit.queue_capacity",
            ">= 1",
        )?;
        Ok(())
    }
}

/// One documented configuration key (consumed by `cargo xtask config-docs` and the `flyff.toml.example`
/// generator of doc 10, next to its `OPS_KEYS` and 03's `KEY_REGISTRY`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyDoc {
    /// `section.key`.
    pub key: &'static str,
    /// TOML type: `string`, `integer`, `float`, `bool`, `array of strings`.
    pub ty: &'static str,
    /// Default as TOML text (`"data/flyff.db"`, `64`, `[]`).
    pub default: &'static str,
    /// Allowed values / range, `""` when unconstrained.
    pub range: &'static str,
    /// `restart` (read at start); every persistence key is startup-class.
    pub reload: &'static str,
    /// One-line description.
    pub description: &'static str,
}

macro_rules! key_doc {
    ($key:literal, $ty:literal, $default:literal, $range:literal, $desc:literal) => {
        KeyDoc {
            key: $key,
            ty: $ty,
            default: $default,
            range: $range,
            reload: "restart",
            description: $desc,
        }
    };
}

/// Documentation table of the Plan/13 §2.4 keys, in file order (one entry per key).
pub static KEY_DOCS: &[KeyDoc] = &[
    key_doc!(
        "database.path",
        "string",
        "\"data/flyff.db\"",
        "",
        "SQLite database file, relative to the home directory."
    ),
    key_doc!(
        "database.synchronous",
        "string",
        "\"normal\"",
        "\"normal\" | \"full\"",
        "PRAGMA synchronous; NORMAL with WAL loses at most the last commits on power loss, FULL none."
    ),
    key_doc!(
        "database.busy_timeout_ms",
        "integer",
        "5000",
        "",
        "SQLite busy timeout of every connection (CLI processes and readers)."
    ),
    key_doc!(
        "database.cache_size_mib",
        "integer",
        "64",
        ">= 1",
        "Page cache of the writer connection in MiB (readers use 16)."
    ),
    key_doc!(
        "database.read_connections",
        "integer",
        "1",
        "1..=4",
        "Number of read-only connections/threads (lobby queries, game reads, backups)."
    ),
    key_doc!(
        "database.batch_max_commands",
        "integer",
        "256",
        ">= 1",
        "Writer drain limit: commands per transaction."
    ),
    key_doc!(
        "database.batch_max_wait_ms",
        "integer",
        "20",
        "",
        "Writer drain window after the first command of a batch."
    ),
    key_doc!(
        "database.flush_wait_timeout_ms",
        "integer",
        "5000",
        "",
        "How long enter-world waits for the previous session's logout save."
    ),
    key_doc!(
        "database.serial_block_size",
        "integer",
        "10000",
        ">= 1",
        "Size of one item serial block reserved from meta.next_item_serial."
    ),
    key_doc!(
        "database.checkpoint_interval_s",
        "integer",
        "300",
        "",
        "Periodic PASSIVE WAL checkpoint interval."
    ),
    key_doc!(
        "database.check_on_start",
        "string",
        "\"quick\"",
        "\"off\" | \"quick\" | \"full\"",
        "Integrity check at start: quick_check or integrity_check."
    ),
    key_doc!(
        "save.interval_s",
        "integer",
        "90",
        ">= 1",
        "Periodic snapshot interval per online character (the legacy SEC_SAVEPLAYER)."
    ),
    key_doc!(
        "save.max_periodic_per_tick",
        "integer",
        "8",
        ">= 1",
        "Cap of periodic snapshots per 66.7 ms tick (spreads characters that joined together)."
    ),
    key_doc!(
        "save.urgent_min_interval_ms",
        "integer",
        "1000",
        "",
        "Minimum distance between two urgent snapshots of one character."
    ),
    key_doc!(
        "save.on_world_change",
        "bool",
        "true",
        "",
        "Take a snapshot when the character changes world id."
    ),
    key_doc!(
        "backup.dir",
        "string",
        "\"backups\"",
        "",
        "Backup directory, relative to the home directory."
    ),
    key_doc!(
        "backup.interval_min",
        "integer",
        "60",
        "",
        "Scheduled online backup interval in minutes."
    ),
    key_doc!(
        "backup.keep_hourly",
        "integer",
        "24",
        "",
        "Hourly backups kept by the grandfather-father-son rotation."
    ),
    key_doc!(
        "backup.keep_daily",
        "integer",
        "7",
        "",
        "Daily backups kept."
    ),
    key_doc!(
        "backup.keep_weekly",
        "integer",
        "4",
        "",
        "Weekly backups kept."
    ),
    key_doc!(
        "backup.keep_premigrate",
        "integer",
        "5",
        "",
        "Pre-migration backups kept."
    ),
    key_doc!(
        "backup.keep_prerestore",
        "integer",
        "5",
        "",
        "Pre-restore backups kept."
    ),
    key_doc!(
        "backup.keep_shutdown",
        "integer",
        "5",
        "",
        "Shutdown backups kept."
    ),
    key_doc!(
        "backup.on_shutdown",
        "bool",
        "false",
        "",
        "Write a backup on graceful shutdown."
    ),
    key_doc!(
        "backup.min_free_space_factor",
        "float",
        "2.0",
        ">= 0",
        "Skip a backup when the free disk space is below this factor times the database size."
    ),
    key_doc!(
        "backup.hook",
        "array of strings",
        "[]",
        "",
        "Optional command (argv) run after a backup; the backup path and kind are appended."
    ),
    key_doc!(
        "backup.hook_timeout_s",
        "integer",
        "600",
        "",
        "Timeout of the backup hook."
    ),
    key_doc!(
        "audit.enabled",
        "bool",
        "true",
        "",
        "Write the append-only JSONL audit log."
    ),
    key_doc!(
        "audit.dir",
        "string",
        "\"logs/audit\"",
        "",
        "Audit log directory, relative to the home directory."
    ),
    key_doc!(
        "audit.retention_days",
        "integer",
        "90",
        "",
        "Days of audit files kept."
    ),
    key_doc!(
        "audit.exclude",
        "array of strings",
        "[]",
        "",
        "Event names or prefixes that are not logged (for example \"item.pickup\", \"session.\"); admin.* and gm.command are never excluded."
    ),
    key_doc!(
        "audit.queue_capacity",
        "integer",
        "65536",
        ">= 1",
        "Capacity of the bounded audit queue (events are dropped and counted when full)."
    ),
    key_doc!(
        "audit.admin_required",
        "bool",
        "true",
        "",
        "Refuse (503 audit_unavailable) an admin console mutation whose audit record cannot be acknowledged within 1 s."
    ),
    key_doc!(
        "retention.deleted_character_days",
        "integer",
        "7",
        "",
        "Days a soft-deleted character is kept before the purge."
    ),
    key_doc!(
        "retention.deleted_mail_days",
        "integer",
        "30",
        "",
        "Days a soft-deleted mail is kept."
    ),
    key_doc!(
        "retention.delivered_note_days",
        "integer",
        "30",
        "",
        "Days a delivered offline note is kept."
    ),
    key_doc!(
        "retention.finished_grant_days",
        "integer",
        "90",
        "",
        "Days a delivered grant or finished removal is kept."
    ),
    key_doc!(
        "retention.guild_bank_log_per_kind",
        "integer",
        "100",
        "",
        "Newest guild bank log rows kept per guild and kind."
    ),
    key_doc!(
        "retention.maintenance_hour",
        "integer",
        "5",
        "0..=23",
        "Local hour of the daily maintenance job."
    ),
];
