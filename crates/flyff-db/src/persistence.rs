//! Opening the database and running the persistence threads (Plan/13 §16.1, 01 §4.2).
//!
//! `open` = instance lock -> open/create the writer connection -> integrity check -> migrations (with the
//! pre-migration backup) -> init-only check -> counter repair -> start the threads: `flyff-persist` (the
//! single writer connection, WAL) and `flyff-persist-ro-N` (`database.read_connections` read-only
//! connections serving [`crate::ReadCmd`]).

use crate::actor::{ActorConfig, Shared, Writer};
use crate::admin::scan::{ChunkLimits, HeavyGate};
use crate::audit::AuditLog;
use crate::backup::schedule::BackupSchedule;
use crate::backup::{BackupClock, BackupCtx, BackupStats, SystemClock};
use crate::clock::now_unix;
use crate::cmd::{BackupKind, DbCmd, DbReply, ShutdownMode, ShutdownReport};
use crate::conn::{WriterOpen, check_integrity, ensure_private_dir, open_writer};
use crate::error::{DbError, DbErrorKind};
use crate::fault::{FaultHooks, no_faults};
use crate::flush::FlushTracker;
use crate::handle::{DbStats, PersistHandle};
use crate::lock::InstanceLock;
use crate::migrate::{MigrateReport, apply_init_only, migrate_with_backup, repair_counters};
use crate::model::StartupState;
use crate::reader::ReaderPool;
use crate::repo::meta::{self, kv};
use flyff_common::rt::reply_pair;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;
use std::time::Duration;

pub use crate::config::{
    AuditConfig, BackupConfig, CheckOnStart, DbConfig, InitOnlyValues, PersistenceConfig,
    RetentionConfig, SaveConfig, Synchronous,
};

/// Optional parts of [`open_with`] (Plan/13 §16.1).
#[derive(Clone, Default)]
#[non_exhaustive]
pub struct OpenOptions {
    /// The clock and the local calendar of the backup layer: the file names, the day and week buckets of the
    /// rotation and the scheduler's wall-clock multiples. The server passes one with `server.timezone`; the
    /// default is the host clock with UTC days.
    pub clock: Option<Arc<dyn BackupClock>>,
    /// The budget of one chunk of an admin read (Plan/25 §11.6): at most 500 rows or 50 ms; the default is
    /// 500 rows or 5 ms ([`ChunkLimits::DEFAULT`]). Tests use tiny chunks.
    pub admin_chunk: Option<ChunkLimits>,
}

impl OpenOptions {
    /// Uses `clock` for backups.
    #[must_use]
    pub fn with_clock(mut self, clock: Arc<dyn BackupClock>) -> Self {
        self.clock = Some(clock);
        self
    }

    /// Runs admin reads with `limits` per chunk.
    #[must_use]
    pub fn with_admin_chunk(mut self, limits: ChunkLimits) -> Self {
        self.admin_chunk = Some(limits);
        self
    }
}

/// What [`open`] returns (Plan/13 §16.1).
pub struct OpenOutput {
    /// Owns the threads and the instance lock.
    pub persistence: Persistence,
    /// Cheap-clone handle for lobby and game.
    pub handle: PersistHandle,
    /// C6: completions drained by the game thread every tick.
    pub completions: flume::Receiver<DbReply>,
    /// Everything services load at boot (core groups; later migration groups add their fields).
    pub startup: StartupState,
    /// The two initial blocks of the game's `SerialAllocator` (`[database] serial_block_size` serials each),
    /// reserved and committed before they are handed out (Plan/13 §15.1).
    pub serial_blocks: [std::ops::Range<u64>; 2],
    /// Audit emitter.
    pub audit: AuditLog,
    /// `PRAGMA user_version` of the opened database (the schema version).
    pub user_version: i64,
    /// What the migrations did (new database, applied versions, pre-migration backup).
    pub migration: MigrateReport,
    /// The previous run did not shut down cleanly (`last_clean_shutdown_at < last_start_at`, Plan/13 §20):
    /// `check_on_start` was raised to at least `quick` for this start.
    pub unclean_previous_run: bool,
}

/// Owner of the writer/reader threads and the single-instance lock.
pub struct Persistence {
    cmd_tx: flume::Sender<DbCmd>,
    writer: Option<JoinHandle<()>>,
    readers: ReaderPool,
    flush: Arc<FlushTracker>,
    stats: Arc<DbStats>,
    db_path: PathBuf,
    audit: AuditLog,
    backup: Arc<BackupCtx>,
    schedule: Option<BackupSchedule>,
    /// `[backup] on_shutdown`.
    backup_on_shutdown: bool,
    // Held for the whole lifetime: dropping it releases the `flock`.
    _lock: InstanceLock,
}

impl Persistence {
    /// Path of the opened database file.
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// Drains the queue, commits, stamps `server.last_clean_shutdown_at` (graceful only), checkpoints
    /// (`TRUNCATE`), stops all threads and releases the lock (Plan/13 §16.8). `timeout` bounds the wait for
    /// the writer (default in the server: 60 s).
    pub fn shutdown(
        mut self,
        mode: ShutdownMode,
        timeout: Duration,
    ) -> Result<ShutdownReport, DbError> {
        // no scheduled backup may start while the writer stops
        if let Some(mut schedule) = self.schedule.take() {
            schedule.stop();
        }
        let (reply, rx) = reply_pair();
        let report = if self.cmd_tx.send(DbCmd::Shutdown { mode, reply }).is_ok() {
            rx.recv_timeout(timeout).map_err(|_| {
                DbError::new(
                    DbErrorKind::Timeout,
                    "persistence writer did not stop in time",
                )
            })?
        } else {
            // Writer already gone (it panicked or stopped on a fatal error).
            ShutdownReport {
                commands: self.stats.commands.load(Ordering::Relaxed),
                checkpointed: false,
                batches: self.stats.batches.load(Ordering::Relaxed),
                abandoned: 0,
            }
        };
        if let Some(w) = self.writer.take()
            && w.join().is_err()
        {
            return Err(DbError::new(DbErrorKind::Io, "persistence writer panicked"));
        }
        // 13 §16.8 step 5: the optional shutdown backup, from the committed and checkpointed file
        if mode == ShutdownMode::Graceful && self.backup_on_shutdown {
            match self.readers.backup_blocking(BackupKind::Shutdown, timeout) {
                Ok(path) => {
                    tracing::info!(path = %path.display(), "shutdown backup written");
                    self.backup.wait_hook_idle(self.backup.hook_wait());
                }
                Err(e) => tracing::error!(error = %e, "the shutdown backup failed"),
            }
        }
        self.readers.stop_and_join();
        self.flush.close();
        self.audit.flush(Duration::from_secs(5));
        Ok(report)
    }
}

impl Drop for Persistence {
    /// Dropping without [`Persistence::shutdown`] (an error path of the server start) still stops the writer
    /// and the readers before the instance lock is released: another process must never open the database
    /// while this one still writes to it. While unwinding from a panic only the queued commands are flushed
    /// (`AfterPanic`: no clean-shutdown stamp).
    fn drop(&mut self) {
        if let Some(mut schedule) = self.schedule.take() {
            schedule.stop();
        }
        let Some(writer) = self.writer.take() else {
            return;
        };
        let mode = if std::thread::panicking() {
            ShutdownMode::AfterPanic
        } else {
            ShutdownMode::Graceful
        };
        let (reply, rx) = reply_pair();
        let stopped = self.cmd_tx.send(DbCmd::Shutdown { mode, reply }).is_err()
            || rx.recv_timeout(Duration::from_secs(10)).is_ok();
        // A writer that does not answer (stuck in retry mode) is detached rather than joined forever.
        if stopped {
            let _ = writer.join();
        }
        self.readers.stop_and_join();
        self.flush.close();
        self.audit.flush(Duration::from_secs(2));
    }
}

/// `true` when the previous run did not end with a clean shutdown: `server.last_start_at` exists and no
/// `server.last_clean_shutdown_at >= last_start_at` follows it (Plan/13 §20). A new database is clean.
fn previous_run_unclean(conn: &rusqlite::Connection, fresh: bool) -> bool {
    if fresh {
        return false;
    }
    let Some(started) = meta::get_kv_i64(conn, kv::LAST_START_AT) else {
        return false;
    };
    match meta::get_kv_i64(conn, kv::LAST_CLEAN_SHUTDOWN_AT) {
        Some(clean) => clean < started,
        None => true,
    }
}

/// Takes the single-instance lock, opens/creates the database, runs the migrations, loads the startup state
/// and starts the threads (Plan/10 §3.1 S6, 13 §16.1). The lock file is `<db file>.lock`
/// (`data/flyff.db.lock`).
///
/// Errors: `InvalidConfig`, `Locked` (exit 75), `Corrupt` (74; also a foreign SQLite file and a failed
/// `check_on_start`), `NewerSchema`, `InitOnlyMismatch` (78), `Io`.
pub fn open(cfg: &PersistenceConfig, base_dir: &Path) -> Result<OpenOutput, DbError> {
    open_inner(cfg, base_dir, no_faults(), &OpenOptions::default())
}

/// [`open`] with the optional parts of [`OpenOptions`] (the server passes the clock of `server.timezone`).
pub fn open_with(
    cfg: &PersistenceConfig,
    base_dir: &Path,
    options: &OpenOptions,
) -> Result<OpenOutput, DbError> {
    open_inner(cfg, base_dir, no_faults(), options)
}

fn open_inner(
    cfg: &PersistenceConfig,
    base_dir: &Path,
    faults: Arc<FaultHooks>,
    options: &OpenOptions,
) -> Result<OpenOutput, DbError> {
    cfg.validate()?;
    let db_path = base_dir.join(&cfg.database.path);
    if let Some(parent) = db_path.parent() {
        ensure_private_dir(parent)?;
    }
    let lock = InstanceLock::acquire(&db_path)?;

    let WriterOpen {
        conn: mut writer_conn,
        fresh,
    } = open_writer(&db_path, &cfg.database)?;
    let unclean = previous_run_unclean(&writer_conn, fresh);
    let check = match (unclean, cfg.database.check_on_start) {
        (true, CheckOnStart::Off) => {
            tracing::warn!(
                "the previous run did not shut down cleanly: running a quick integrity check"
            );
            CheckOnStart::Quick
        }
        (true, other) => {
            tracing::warn!("the previous run did not shut down cleanly");
            other
        }
        (false, other) => other,
    };
    check_integrity(&writer_conn, check)?;
    let backup_dir = base_dir.join(&cfg.backup.dir);
    let migration = migrate_with_backup(
        &mut writer_conn,
        &backup_dir,
        cfg.backup.keep_premigrate,
        now_unix(),
    )?;
    apply_init_only(&writer_conn, &cfg.init_only)?;
    repair_counters(&writer_conn)?;

    let user_version = migration.to;

    // Start bookkeeping, the two initial serial blocks (committed before they are handed out), the startup state.
    let now = now_unix();
    let serial_blocks = {
        let tx = writer_conn.unchecked_transaction()?;
        if meta::get_kv(&tx, kv::FIRST_START_AT)?.is_none() {
            meta::set_kv(&tx, kv::FIRST_START_AT, &now.0.into(), now)?;
        }
        meta::set_kv(&tx, kv::LAST_START_AT, &now.0.into(), now)?;
        let first = meta::reserve_serial_block(&tx, cfg.database.serial_block_size)?;
        let second = meta::reserve_serial_block(&tx, cfg.database.serial_block_size)?;
        tx.commit()?;
        [first, second]
    };
    let startup = crate::startup::load(&writer_conn)?;

    let flush = Arc::new(FlushTracker::default());
    let stats = Arc::new(DbStats::default());
    // The audit writer thread (13 §25), the backup layer (13 §21): the log is cheap to clone, the context is
    // what the reader threads use for `ReadCmd::Backup`.
    let audit = AuditLog::start(&cfg.audit, base_dir, now.0);
    let backup_stats = Arc::new(BackupStats::default());
    let clock: Arc<dyn BackupClock> = options
        .clock
        .clone()
        .unwrap_or_else(|| Arc::new(SystemClock::utc()));
    let backup_ctx = BackupCtx::new(
        base_dir,
        &db_path,
        &cfg.backup,
        Arc::clone(&clock),
        Arc::clone(&backup_stats),
    );
    let (cmd_tx, cmd_rx) = flume::unbounded::<DbCmd>();
    let (done_tx, done_rx) = flume::unbounded::<DbReply>();

    // Dropped when the writer thread ends, however it ends: callers waiting on a command that raced the
    // shutdown are released (flume keeps a queued command alive while any handle exists).
    let (writer_alive, writer_closed) = flume::bounded::<()>(1);
    let writer = {
        let writer = Writer::new(
            writer_conn,
            ActorConfig {
                batch_max_commands: usize::try_from(cfg.database.batch_max_commands)
                    .unwrap_or(usize::MAX),
                batch_max_wait: Duration::from_millis(u64::from(cfg.database.batch_max_wait_ms)),
                checkpoint_interval: Duration::from_secs(u64::from(
                    cfg.database.checkpoint_interval_s.max(1),
                )),
                serial_block_size: cfg.database.serial_block_size,
                logs_dir: base_dir.join("logs"),
                db_path: db_path.clone(),
                retention: cfg.retention.clone(),
            },
            Shared {
                flush: Arc::clone(&flush),
                stats: Arc::clone(&stats),
                done_tx: done_tx.clone(),
                faults,
                audit: audit.clone(),
                backup: Arc::clone(&backup_stats),
            },
        );
        let stats = Arc::clone(&stats);
        let flush = Arc::clone(&flush);
        std::thread::Builder::new()
            .name("flyff-persist".to_owned())
            .spawn(move || {
                // A panic inside the actor rolls back the open transaction when the connection is dropped
                // during the unwind; the thread then only reports itself gone (Plan/13 §20).
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    writer.run(&cmd_rx);
                }));
                if outcome.is_err() {
                    tracing::error!("the persistence writer panicked: persistence is closed");
                }
                stats.writer_gone.store(true, Ordering::Release);
                flush.close();
                drop(writer_alive);
            })?
    };
    let admin_limits = options.admin_chunk.unwrap_or_default();
    let readers = ReaderPool::spawn(
        &db_path,
        &cfg.database,
        Arc::clone(&backup_ctx),
        Arc::clone(&stats),
        admin_limits,
    )?;
    let schedule = BackupSchedule::spawn(
        cfg.backup.interval_min,
        clock,
        readers.sender(),
        Arc::clone(&backup_stats),
    )?;

    let readers_dedicated = readers.has_dedicated_admin_reader();
    let handle = PersistHandle {
        cmd_tx: Some(cmd_tx.clone()),
        read_tx: Some(readers.sender()),
        completions_tx: Some(done_tx),
        flush: Arc::clone(&flush),
        stats: Arc::clone(&stats),
        writer_closed: Some(writer_closed),
        readers_closed: Some(readers.closed_signal()),
        backup_stats,
        admin_tx: Some(readers.admin_sender()),
        heavy: Arc::new(HeavyGate::default()),
        admin_limits,
    };
    tracing::info!(
        path = %db_path.display(),
        user_version,
        new_database = fresh,
        applied = ?migration.applied,
        unclean_previous_run = unclean,
        admin_reader = readers_dedicated,
        "database opened (WAL)"
    );
    Ok(OpenOutput {
        persistence: Persistence {
            cmd_tx,
            writer: Some(writer),
            readers,
            flush,
            stats,
            db_path,
            audit: audit.clone(),
            backup: backup_ctx,
            schedule,
            backup_on_shutdown: cfg.backup.on_shutdown,
            _lock: lock,
        },
        handle,
        completions: done_rx,
        startup,
        serial_blocks,
        audit,
        user_version,
        migration,
        unclean_previous_run: unclean,
    })
}
