//! Backups (Plan/13 §3, §21, WP-DB-08): the online backup of the SQLite database, the file names, the free-space
//! guard, the grandfather-father-son rotation ([`mod@rotate`]), the post-backup hook ([`hook`]), the scheduler thread
//! `flyff-persist-backup` ([`schedule`]) and the restore procedure ([`mod@restore`]).
//!
//! * [`online_backup`] copies the whole database through the SQLite backup API **in one step** on a read
//!   connection: in WAL mode that read transaction never blocks the writer, and a stepped copy would restart
//!   whenever the writer commits between steps. The copy is verified (`quick_check`, `application_id`) and moved
//!   into place atomically.
//! * [`run_backup`] is what a reader thread does for `ReadCmd::Backup`: free-space check, backup, rotation, hook,
//!   statistics. The kinds are `sched`, `premigrate`, `manual`, `shutdown`, `prerestore` and `replay` (the start
//!   snapshot of a recorded run: `<dir>/start.db`, no hook, never rotated).
//! * Everything that needs "now" or the server's local calendar goes through [`BackupClock`], so rotation and the
//!   scheduler are tested with a fake clock.

pub mod hook;
pub mod restore;
pub mod rotate;
pub mod schedule;

use crate::calendar;
use crate::cmd::BackupKind;
use crate::config::{BackupConfig, CheckOnStart, PersistenceConfig};
use crate::conn::{APPLICATION_ID, check_integrity, ensure_private_dir, open_reader, pragma_i64};
use crate::error::{DbError, DbErrorKind};
use flyff_common::UnixTime;
use rusqlite::backup::{Backup, StepResult};
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

pub use hook::HookRunner;
pub use restore::{DbSummary, RestoreReport, inspect_database, restore};
pub use rotate::{BackupFile, gfs_keep, list_backups, parse_backup_name, rotate};
pub use schedule::next_boundary;

/// `YYYYMMDD-HHMMSS` of `t` in UTC (proleptic Gregorian).
pub fn utc_compact(t: UnixTime) -> String {
    let secs = t.0;
    let (year, month, day) = calendar::civil_from_days(secs.div_euclid(calendar::DAY_S));
    let rem = secs.rem_euclid(calendar::DAY_S);
    format!(
        "{year:04}{month:02}{day:02}-{:02}{:02}{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// `flyff-YYYYMMDD-HHMMSS-<kind>.db` (Plan/13 §2.1).
pub fn backup_file_name(kind_label: &str, at: UnixTime) -> String {
    format!("flyff-{}-{kind_label}.db", utc_compact(at))
}

/// A path in `dir` for a `kind_label` backup taken at `at` that does not exist yet: a second backup within the
/// same second moves to the next free second.
pub fn unique_backup_path(dir: &Path, kind_label: &str, at: UnixTime) -> PathBuf {
    let mut t = at;
    loop {
        let candidate = dir.join(backup_file_name(kind_label, t));
        if !candidate.exists() {
            return candidate;
        }
        t = UnixTime(t.0.saturating_add(1));
    }
}

/// Copies the whole database of `src` into the new file `dest` through the SQLite online backup API (one
/// step, so the copy is a consistent snapshot), verifies the copy with `quick_check` and moves it into place
/// atomically (write to `<dest>.part`, fsync, rename). Existing `dest` is never overwritten.
///
/// `dest`'s directory is created when missing. On any failure no `dest` file remains.
pub fn online_backup(src: &Connection, dest: &Path) -> Result<(), DbError> {
    if dest.exists() {
        return Err(DbError::new(
            DbErrorKind::Conflict,
            format!("backup target {} already exists", dest.display()),
        ));
    }
    if let Some(dir) = dest.parent() {
        ensure_private_dir(dir)?;
    }
    let mut part_name = dest.as_os_str().to_owned();
    part_name.push(".part");
    let part = PathBuf::from(part_name);
    let _ = std::fs::remove_file(&part);
    let result = copy_and_verify(src, &part).and_then(|()| {
        std::fs::rename(&part, dest)?;
        if let Some(dir) = dest.parent() {
            // Best effort: make the rename durable.
            if let Ok(d) = std::fs::File::open(dir) {
                let _ = d.sync_all();
            }
        }
        Ok(())
    });
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}

fn copy_and_verify(src: &Connection, part: &Path) -> Result<(), DbError> {
    {
        let mut dst = Connection::open(part)?;
        {
            let backup = Backup::new(src, &mut dst)?;
            // -1 pages = the whole database in one step. A busy/locked source is retried briefly.
            let mut attempts = 0_u32;
            loop {
                match backup.step(-1)? {
                    StepResult::Done => break,
                    StepResult::More => {}
                    _ => {
                        attempts += 1;
                        if attempts > 100 {
                            return Err(DbError::new(
                                DbErrorKind::Transient,
                                "online backup: the source stayed busy for 5 s",
                            ));
                        }
                        std::thread::sleep(Duration::from_millis(50));
                    }
                }
            }
        }
        // The copy is a self-contained file: no -wal / -shm companions next to it.
        dst.query_row("PRAGMA journal_mode = DELETE", [], |_| Ok(()))?;
        check_integrity(&dst, CheckOnStart::Quick)?;
        dst.close().map_err(|(_, e)| DbError::from(e))?;
    }
    std::fs::File::open(part)?.sync_all()?;
    Ok(())
}

/// Deletes the oldest `flyff-*-<kind_label>.db` files in `dir` beyond the newest `keep`. Returns how many
/// were removed. Names sort chronologically (UTC `YYYYMMDD-HHMMSS`).
pub fn prune_backups(dir: &Path, kind_label: &str, keep: usize) -> Result<usize, DbError> {
    let suffix = format!("-{kind_label}.db");
    let mut names: Vec<String> = match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .filter_map(Result::ok)
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| is_backup_name(n, &suffix))
            .collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e.into()),
    };
    names.sort();
    let excess = names.len().saturating_sub(keep);
    let mut removed = 0;
    for name in names.into_iter().take(excess) {
        std::fs::remove_file(dir.join(&name))?;
        removed += 1;
    }
    Ok(removed)
}

/// `flyff-YYYYMMDD-HHMMSS` + `suffix`.
fn is_backup_name(name: &str, suffix: &str) -> bool {
    let Some(stamp) = name
        .strip_prefix("flyff-")
        .and_then(|rest| rest.strip_suffix(suffix))
    else {
        return false;
    };
    let bytes = stamp.as_bytes();
    bytes.len() == 15
        && bytes.iter().enumerate().all(|(i, b)| {
            if i == 8 {
                *b == b'-'
            } else {
                b.is_ascii_digit()
            }
        })
}

// ---- time ------------------------------------------------------------------------------------------------------

/// Time and calendar of the backup layer: "now" and the UTC offset of the server's local zone (the day and week
/// boundaries of the rotation are local, Plan/13 §21.2). Tests use a fake.
pub trait BackupClock: Send + Sync + 'static {
    /// The current Unix time.
    fn now(&self) -> UnixTime;
    /// Seconds to add to UTC to get the local time of the server at `at`.
    fn utc_offset_s(&self, at: UnixTime) -> i32 {
        let _ = at;
        0
    }
    /// How often the scheduler thread looks at the clock.
    fn poll_interval(&self) -> Duration {
        Duration::from_millis(500)
    }
}

/// The offset of a time zone at an instant, for [`SystemClock::with_zone`].
pub type ZoneOffset = Arc<dyn Fn(UnixTime) -> i32 + Send + Sync>;

/// The host clock with an optional zone (UTC without).
#[derive(Clone, Default)]
pub struct SystemClock {
    zone: Option<ZoneOffset>,
}

impl SystemClock {
    /// The host clock; day boundaries in UTC.
    pub fn utc() -> SystemClock {
        SystemClock { zone: None }
    }

    /// The host clock; day boundaries in the zone described by `zone`.
    pub fn with_zone(zone: ZoneOffset) -> SystemClock {
        SystemClock { zone: Some(zone) }
    }
}

impl std::fmt::Debug for SystemClock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemClock")
            .field("zone", &self.zone.is_some())
            .finish()
    }
}

impl BackupClock for SystemClock {
    fn now(&self) -> UnixTime {
        crate::clock::now_unix()
    }

    fn utc_offset_s(&self, at: UnixTime) -> i32 {
        self.zone.as_ref().map_or(0, |z| z(at))
    }
}

// ---- statistics ------------------------------------------------------------------------------------------------

/// Counters of the backup layer (`flyff_db_last_backup_timestamp_seconds`, `flyff_db_backup_hook_failures_total`, ...).
#[derive(Debug, Default)]
pub struct BackupStats {
    last_ok_unix: AtomicI64,
    total: AtomicU64,
    failures: AtomicU64,
    skipped: AtomicU64,
    hook_runs: AtomicU64,
    hook_failures: AtomicU64,
    last_ms: AtomicU64,
    last_bytes: AtomicU64,
    running: AtomicU32,
}

/// A copy of the [`BackupStats`] counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BackupSnapshot {
    /// Unix time of the last successful backup (0 = none yet).
    pub last_ok_unix: i64,
    /// Successful backups.
    pub total: u64,
    /// Failed backups.
    pub failures: u64,
    /// Scheduled backups skipped because the previous one was still running.
    pub skipped: u64,
    /// Hook processes started.
    pub hook_runs: u64,
    /// Hooks that failed to start, exited non-zero or timed out.
    pub hook_failures: u64,
    /// Duration of the last backup in milliseconds.
    pub last_duration_ms: u64,
    /// Size of the last backup file.
    pub last_bytes: u64,
    /// A backup is running right now.
    pub running: bool,
}

impl BackupStats {
    /// `true` while at least one backup runs (the periodic `RESTART` checkpoint waits, Plan/13 §22).
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire) > 0
    }

    /// A copy of the counters.
    pub fn snapshot(&self) -> BackupSnapshot {
        BackupSnapshot {
            last_ok_unix: self.last_ok_unix.load(Ordering::Relaxed),
            total: self.total.load(Ordering::Relaxed),
            failures: self.failures.load(Ordering::Relaxed),
            skipped: self.skipped.load(Ordering::Relaxed),
            hook_runs: self.hook_runs.load(Ordering::Relaxed),
            hook_failures: self.hook_failures.load(Ordering::Relaxed),
            last_duration_ms: self.last_ms.load(Ordering::Relaxed),
            last_bytes: self.last_bytes.load(Ordering::Relaxed),
            running: self.is_running(),
        }
    }

    pub(crate) fn note_skipped(&self) {
        self.skipped.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn note_hook_started(&self) {
        self.hook_runs.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn note_hook_failed(&self) {
        self.hook_failures.fetch_add(1, Ordering::Relaxed);
    }
}

/// Keeps [`BackupStats::is_running`] true while it lives.
struct RunningGuard<'a>(&'a BackupStats);

impl<'a> RunningGuard<'a> {
    fn new(stats: &'a BackupStats) -> Self {
        stats.running.fetch_add(1, Ordering::AcqRel);
        RunningGuard(stats)
    }
}

impl Drop for RunningGuard<'_> {
    fn drop(&mut self) {
        self.0.running.fetch_sub(1, Ordering::AcqRel);
    }
}

// ---- running one backup ----------------------------------------------------------------------------------------

/// Everything a reader thread needs to run a [`BackupKind`] backup (Plan/13 §21.1).
pub struct BackupCtx {
    dir: PathBuf,
    db_path: PathBuf,
    cfg: BackupConfig,
    clock: Arc<dyn BackupClock>,
    stats: Arc<BackupStats>,
    hook: HookRunner,
}

impl BackupCtx {
    /// The context for the database `db_path`; `cfg.dir` is relative to `base_dir` unless absolute.
    pub fn new(
        base_dir: &Path,
        db_path: &Path,
        cfg: &BackupConfig,
        clock: Arc<dyn BackupClock>,
        stats: Arc<BackupStats>,
    ) -> Arc<BackupCtx> {
        let dir = if cfg.dir.is_absolute() {
            cfg.dir.clone()
        } else {
            base_dir.join(&cfg.dir)
        };
        Arc::new(BackupCtx {
            dir,
            db_path: db_path.to_owned(),
            cfg: cfg.clone(),
            clock,
            stats,
            hook: HookRunner::default(),
        })
    }

    /// `<home>/backups`.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The shared counters.
    pub fn stats(&self) -> &Arc<BackupStats> {
        &self.stats
    }

    /// The clock.
    pub fn clock(&self) -> &Arc<dyn BackupClock> {
        &self.clock
    }

    /// Waits until no hook process runs (CLI processes wait before they exit).
    pub fn wait_hook_idle(&self, timeout: Duration) -> bool {
        self.hook.wait_idle(timeout)
    }

    /// The `[backup] hook_timeout_s` of this context plus a grace second.
    pub fn hook_wait(&self) -> Duration {
        Duration::from_secs(u64::from(self.cfg.hook_timeout_s) + 2)
    }
}

/// The size of the database file plus its `-wal`.
fn db_footprint(db_path: &Path) -> u64 {
    let size = |p: &Path| std::fs::metadata(p).map_or(0, |m| m.len());
    let mut wal = db_path.as_os_str().to_owned();
    wal.push("-wal");
    size(db_path).saturating_add(size(Path::new(&wal)))
}

/// Free bytes available to an unprivileged writer on the file system of `dir` (`None` when unknown).
#[allow(clippy::unnecessary_cast)] // the field types differ between the rustix backends
pub fn free_bytes(dir: &Path) -> Option<u64> {
    let st = rustix::fs::statvfs(dir).ok()?;
    Some((st.f_bavail as u64).saturating_mul(st.f_frsize as u64))
}

/// `Err(Io)` unless `dir` has `factor` times the footprint of the database free (Plan/13 §21.1).
pub fn require_free_space(db_path: &Path, dir: &Path, factor: f64) -> Result<(), DbError> {
    if !factor.is_finite() || factor <= 0.0 {
        return Ok(());
    }
    let need = (db_footprint(db_path) as f64 * factor) as u64;
    match free_bytes(dir) {
        Some(free) if free < need => Err(DbError::new(
            DbErrorKind::Io,
            format!(
                "not enough free space in {}: {free} bytes free, {need} needed ({factor} x the database)",
                dir.display()
            ),
        )),
        _ => Ok(()),
    }
}

/// Leftovers of a backup that was interrupted by a crash (`flyff-*.db.part`) older than an hour.
fn sweep_stale_parts(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.filter_map(Result::ok) {
        let name = e.file_name();
        let Some(name) = name.to_str() else { continue };
        if !(name.starts_with("flyff-") && name.ends_with(".db.part")) {
            continue;
        }
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > Duration::from_secs(3600));
        if old {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// `application_id` of the new file must be the flyff stamp (a backup of something else is `Corrupt`).
fn verify_flyff_file(path: &Path) -> Result<(), DbError> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let app_id = pragma_i64(&conn, "application_id")?;
    if app_id != APPLICATION_ID {
        return Err(DbError::new(
            DbErrorKind::Corrupt,
            format!(
                "backup {} has application_id {app_id:#x}, expected {APPLICATION_ID:#x}",
                path.display()
            ),
        ));
    }
    Ok(())
}

/// Runs one backup of `kind` from the read connection `conn` (Plan/13 §21.1): free-space check, online backup,
/// verification, rotation, hook. Returns the file written. Updates the [`BackupStats`] of `ctx`.
pub fn run_backup(
    conn: &Connection,
    ctx: &BackupCtx,
    kind: &BackupKind,
) -> Result<PathBuf, DbError> {
    let _running = RunningGuard::new(&ctx.stats);
    let started = Instant::now();
    let result = backup_inner(conn, ctx, kind);
    match &result {
        Ok(path) => {
            let s = &ctx.stats;
            s.total.fetch_add(1, Ordering::Relaxed);
            s.last_ok_unix.store(ctx.clock.now().0, Ordering::Relaxed);
            s.last_ms.store(
                u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                Ordering::Relaxed,
            );
            s.last_bytes.store(
                std::fs::metadata(path).map_or(0, |m| m.len()),
                Ordering::Relaxed,
            );
            tracing::info!(path = %path.display(), kind = kind.label(), ms = started.elapsed().as_millis(), "backup written");
        }
        Err(e) => {
            ctx.stats.failures.fetch_add(1, Ordering::Relaxed);
            tracing::error!(kind = kind.label(), error = %e, "backup failed");
        }
    }
    result
}

fn backup_inner(conn: &Connection, ctx: &BackupCtx, kind: &BackupKind) -> Result<PathBuf, DbError> {
    if let BackupKind::Replay { dir } = kind {
        ensure_private_dir(dir)?;
        require_free_space(&ctx.db_path, dir, ctx.cfg.min_free_space_factor)?;
        let dest = dir.join("start.db");
        online_backup(conn, &dest)?;
        if let Err(e) = verify_flyff_file(&dest) {
            let _ = std::fs::remove_file(&dest);
            return Err(e);
        }
        return Ok(dest);
    }
    ensure_private_dir(&ctx.dir)?;
    sweep_stale_parts(&ctx.dir);
    require_free_space(&ctx.db_path, &ctx.dir, ctx.cfg.min_free_space_factor)?;
    let label = kind.label();
    let dest = unique_backup_path(&ctx.dir, label, ctx.clock.now());
    online_backup(conn, &dest)?;
    if let Err(e) = verify_flyff_file(&dest) {
        let _ = std::fs::remove_file(&dest);
        return Err(e);
    }
    // Rotation problems never fail the backup that was just written.
    let clock = Arc::clone(&ctx.clock);
    let offset = move |t: UnixTime| clock.utc_offset_s(t);
    match rotate(&ctx.dir, &ctx.cfg, label, &dest, &offset) {
        Ok(deleted) if !deleted.is_empty() => {
            tracing::info!(
                kind = label,
                deleted = deleted.len(),
                "old backups rotated out"
            );
        }
        Ok(_) => {}
        Err(e) => tracing::warn!(error = %e, "backup rotation failed"),
    }
    if !ctx.cfg.hook.is_empty() {
        ctx.hook.spawn(
            &ctx.cfg.hook,
            Duration::from_secs(u64::from(ctx.cfg.hook_timeout_s.max(1))),
            &dest,
            label,
            &ctx.db_path,
            &ctx.stats,
        );
    }
    Ok(dest)
}

// ---- `--with-config` and the CLI entry point ------------------------------------------------------------------

/// What `flyff db backup` produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CliBackup {
    /// The database backup.
    pub path: PathBuf,
    /// `backups/config-YYYYMMDD-HHMMSS/` when `--with-config` was given.
    pub config_dir: Option<PathBuf>,
    /// Size of the backup file.
    pub bytes: u64,
}

/// `flyff db backup [--with-config]` (Plan/13 §21.1): a `manual` backup through an own read connection; works
/// with the server running or stopped. The post-backup hook runs and is waited for.
pub fn cli_backup(
    cfg: &PersistenceConfig,
    base_dir: &Path,
    with_config: bool,
    clock: Arc<dyn BackupClock>,
) -> Result<CliBackup, DbError> {
    cli_backup_kind(cfg, base_dir, with_config, clock, &BackupKind::Manual)
}

/// [`cli_backup`] with an explicit kind (`flyff db vacuum` uses `manual` as well; the replay tests use `replay`).
pub fn cli_backup_kind(
    cfg: &PersistenceConfig,
    base_dir: &Path,
    with_config: bool,
    clock: Arc<dyn BackupClock>,
    kind: &BackupKind,
) -> Result<CliBackup, DbError> {
    cfg.validate()?;
    let db_path = base_dir.join(&cfg.database.path);
    if !db_path.is_file() {
        return Err(DbError::new(
            DbErrorKind::NotFound,
            format!("there is no database at {}", db_path.display()),
        ));
    }
    let conn = open_reader(&db_path, &cfg.database)?;
    let ctx = BackupCtx::new(
        base_dir,
        &db_path,
        &cfg.backup,
        clock,
        Arc::new(BackupStats::default()),
    );
    let path = run_backup(&conn, &ctx, kind)?;
    let config_dir = if with_config {
        Some(copy_config(base_dir, ctx.dir(), ctx.clock().now())?)
    } else {
        None
    };
    ctx.wait_hook_idle(ctx.hook_wait());
    let bytes = std::fs::metadata(&path).map_or(0, |m| m.len());
    Ok(CliBackup {
        path,
        config_dir,
        bytes,
    })
}

/// Copies `flyff.toml`, `overrides/` and `scripts/` of `home` into `<backups>/config-YYYYMMDD-HHMMSS/`.
pub fn copy_config(home: &Path, backups: &Path, at: UnixTime) -> Result<PathBuf, DbError> {
    let mut stamp = at;
    let dest = loop {
        let candidate = backups.join(format!("config-{}", utc_compact(stamp)));
        if !candidate.exists() {
            break candidate;
        }
        stamp = UnixTime(stamp.0.saturating_add(1));
    };
    ensure_private_dir(&dest)?;
    let toml = home.join("flyff.toml");
    if toml.is_file() {
        std::fs::copy(&toml, dest.join("flyff.toml"))?;
    }
    for dir in ["overrides", "scripts"] {
        let from = home.join(dir);
        if from.is_dir() {
            copy_tree(&from, &dest.join(dir), 0)?;
        }
    }
    Ok(dest)
}

/// Recursive copy of regular files and directories (symbolic links are skipped).
fn copy_tree(from: &Path, to: &Path, depth: u32) -> Result<(), DbError> {
    if depth > 32 {
        return Ok(());
    }
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target, depth + 1)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
