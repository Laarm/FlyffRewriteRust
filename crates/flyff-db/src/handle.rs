//! `PersistHandle` (Plan/13 §16.1): the cheap-clone handle given to the game thread, lobby and console.

use crate::cmd::{
    AtomicBatch, AtomicKind, AtomicOutcome, BackupKind, BatchReply, DbCmd, DbOp, DbReply,
    GameReadQuery, GameReadResult, MaintenanceJob, MaintenanceReport, ReadCmd, Ticket,
};
use crate::error::{DbError, DbErrorKind};
use crate::flush::FlushTracker;
use flyff_common::rt::reply::ReplyError;
use flyff_common::rt::{ReplyRx, reply_pair};
use flyff_common::{AccountId, CharacterId, SaveGen, UnixTime};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::model::CharacterSnapshot;

/// Counters shared between the handle and the writer thread (the `flyff_db_*` metrics of Plan/13 §16.4-§22).
#[derive(Default)]
pub(crate) struct DbStats {
    pub(crate) commands: AtomicU64,
    pub(crate) batches: AtomicU64,
    pub(crate) snapshots_written: AtomicU64,
    pub(crate) snapshots_stale: AtomicU64,
    pub(crate) snapshots_coalesced: AtomicU64,
    pub(crate) anomalies: AtomicU64,
    pub(crate) maintenance_runs: AtomicU64,
    pub(crate) maintenance_failures: AtomicU64,
    pub(crate) last_maintenance: std::sync::Mutex<Option<MaintenanceReport>>,
    pub(crate) commands_failed: AtomicU64,
    pub(crate) bug_failures: AtomicU64,
    pub(crate) transient_failures: AtomicU64,
    pub(crate) last_commit_us: AtomicU64,
    pub(crate) max_commit_us: AtomicU64,
    /// Durations of the committing batches (`flyff_db_batch_commit_seconds`, the p99 budget of 60 §12.4); the
    /// gauge `last_commit_us` alone cannot show a p99.
    pub(crate) commit_hist: flyff_common::rt::Histogram,
    pub(crate) checkpoints: AtomicU64,
    pub(crate) checkpoint_busy: AtomicU64,
    /// Size of the `-wal` file after the last checkpoint (`flyff_db_wal_bytes`).
    pub(crate) wal_bytes: AtomicU64,
    /// Duration of the last checkpoint in microseconds (`flyff_db_checkpoint_seconds`).
    pub(crate) last_checkpoint_us: AtomicU64,
    /// The writer is in transient-retry mode (between `Degraded` and `Recovered`).
    pub(crate) degraded: AtomicBool,
    /// Set when the writer thread has ended (graceful or not).
    pub(crate) writer_gone: AtomicBool,
    /// Set when the writer stopped on a corrupt database (→ exit 74).
    pub(crate) corrupt: AtomicBool,
    /// Chunks of admin scans run by the reader threads (`flyff_db_admin_chunks_total`).
    pub(crate) admin_chunks: AtomicU64,
    /// Rows examined by admin scans.
    pub(crate) admin_rows: AtomicU64,
    /// Admin scans that ran to the end.
    pub(crate) admin_scans: AtomicU64,
    /// Admin scans that failed.
    pub(crate) admin_scans_failed: AtomicU64,
    /// Admin scans stopped by their consumer (cancel or drop).
    pub(crate) admin_scans_cancelled: AtomicU64,
    /// Heavy admin scans refused because another one ran.
    pub(crate) admin_scans_rejected: AtomicU64,
}

impl DbStats {
    pub(crate) fn set_last_maintenance(&self, report: MaintenanceReport) {
        *self
            .last_maintenance
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(report);
    }
}

/// Snapshot of the persistence counters for `/metrics` and the lobby's degraded flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct DbStatsSnapshot {
    /// The writer is in transient-retry mode (between `DbReply::Degraded` and `Recovered`, Plan/13 §16.6).
    pub degraded: bool,
    /// Commands waiting in the writer queue (`flyff_db_queue_depth`).
    pub queue_depth: usize,
    /// Commands received by the writer so far.
    pub commands: u64,
    /// Transactions committed (`flyff_db_batches_total`).
    pub batches: u64,
    /// Snapshots written (`flyff_db_snapshots_written_total`).
    pub snapshots_written: u64,
    /// Snapshots skipped by the generation guard (`flyff_db_snapshots_stale_total`).
    pub snapshots_stale: u64,
    /// Standalone saves dropped by coalescing.
    pub snapshots_coalesced: u64,
    /// Daily maintenance runs that finished (`flyff_db_maintenance_runs_total`).
    pub maintenance_runs: u64,
    /// Daily maintenance runs that failed (they are idempotent and run again).
    pub maintenance_failures: u64,
    /// Rows an op found already closed (`MarkGrantsDelivered`, `MarkRemovalsDone`): a grant delivered twice is
    /// logged as CRITICAL and counted here (`flyff_db_anomalies_total`).
    pub anomalies: u64,
    /// Commands that failed with an *Expected* error.
    pub commands_failed: u64,
    /// Commands that failed with a *Bug* (their accounts were poisoned).
    pub bug_failures: u64,
    /// Transient failures that put the writer into retry mode.
    pub transient_failures: u64,
    /// Duration of the last commit batch in microseconds.
    pub last_commit_us: u64,
    /// Longest commit batch in microseconds.
    pub max_commit_us: u64,
    /// WAL checkpoints run.
    pub checkpoints: u64,
    /// Checkpoints that reported `busy`.
    pub checkpoint_busy: u64,
    /// Size of the `-wal` file after the last checkpoint (`flyff_db_wal_bytes`).
    pub wal_bytes: u64,
    /// Duration of the last checkpoint in microseconds (`flyff_db_checkpoint_seconds`).
    pub last_checkpoint_us: u64,
    /// Chunks of admin scans run so far (`flyff_db_admin_chunks_total`).
    pub admin_chunks: u64,
    /// Rows examined by admin scans (`flyff_db_admin_rows_total`).
    pub admin_rows: u64,
    /// Admin scans that ran to the end (`flyff_db_admin_scans_total`).
    pub admin_scans: u64,
    /// Admin scans that failed.
    pub admin_scans_failed: u64,
    /// Admin scans stopped by their consumer.
    pub admin_scans_cancelled: u64,
    /// Heavy admin scans refused because another one ran.
    pub admin_scans_rejected: u64,
    /// A heavy admin scan holds the server-wide permit right now.
    pub admin_heavy_running: bool,
}

/// A signal that fires (the receiver reports `Disconnected`) when the threads it belongs to have ended. Callers
/// that wait for a reply also wait for it: a command that was queued while its thread was stopping is never
/// answered, and flume keeps such a command (and its one-shot reply sender) alive as long as any handle exists.
pub(crate) type ClosedSignal = flume::Receiver<()>;

/// How often a blocking wait looks at its [`ClosedSignal`].
const CLOSED_POLL: std::time::Duration = std::time::Duration::from_millis(25);

/// Blocking wait for a reply, `timeout` at most, that gives up with `Disconnected` once `closed` fired.
fn wait_reply<T>(
    rx: ReplyRx<T>,
    closed: Option<&ClosedSignal>,
    timeout: Option<std::time::Duration>,
) -> Result<T, ReplyError> {
    let deadline = timeout.map(|t| std::time::Instant::now() + t);
    loop {
        let slice = match deadline {
            Some(d) => {
                let left = d.saturating_duration_since(std::time::Instant::now());
                if left.is_zero() {
                    return Err(ReplyError::Empty);
                }
                left.min(CLOSED_POLL)
            }
            None => CLOSED_POLL,
        };
        match rx.recv_timeout(slice) {
            Ok(v) => return Ok(v),
            Err(ReplyError::Disconnected) => return Err(ReplyError::Disconnected),
            Err(ReplyError::Empty) => {
                if closed.is_some_and(flume::Receiver::is_disconnected) {
                    // A reply may have raced the shutdown.
                    return rx.try_recv().map_err(|_| ReplyError::Disconnected);
                }
            }
        }
    }
}

/// Async wait for a reply that gives up with `Disconnected` once `closed` fired.
async fn wait_reply_async<T>(
    rx: ReplyRx<T>,
    closed: Option<&ClosedSignal>,
) -> Result<T, ReplyError> {
    use std::future::Future;
    use std::task::Poll;
    let mut reply = std::pin::pin!(rx.recv_async());
    let Some(signal) = closed else {
        return reply.await;
    };
    let mut gone = std::pin::pin!(signal.recv_async());
    std::future::poll_fn(|cx| {
        if let Poll::Ready(v) = reply.as_mut().poll(cx) {
            return Poll::Ready(v);
        }
        match gone.as_mut().poll(cx) {
            Poll::Ready(_) => Poll::Ready(match reply.as_mut().poll(cx) {
                Poll::Ready(v) => v,
                Poll::Pending => Err(ReplyError::Disconnected),
            }),
            Poll::Pending => Poll::Pending,
        }
    })
    .await
}

/// Executes a [`GameReadQuery`] on a reader connection (one read transaction).
fn run_game_read(
    conn: &rusqlite::Connection,
    query: &GameReadQuery,
) -> Result<GameReadResult, DbError> {
    match query {
        GameReadQuery::Ping => conn
            .query_row("SELECT 1", [], |r| r.get::<_, i64>(0))
            .map(|_| GameReadResult::Pong)
            .map_err(DbError::from),
        GameReadQuery::OpenGrants {
            character_ids,
            after_id,
        } => crate::repo::messaging::open_grants(conn, character_ids, *after_id)
            .map(GameReadResult::Grants),
        GameReadQuery::OpenGuildRemovals { after_id } => {
            crate::repo::messaging::open_guild_removals(conn, *after_id)
                .map(GameReadResult::GuildRemovals)
        }
        GameReadQuery::QuizQuestions { quiz_type } => {
            let quiz_type = crate::model::QuizType::from_code(*quiz_type).ok_or_else(|| {
                DbError::new(
                    DbErrorKind::Invalid,
                    format!("QuizQuestions: quiz type {quiz_type} is neither 1 (OX) nor 2 (four choices)"),
                )
            })?;
            crate::repo::houses::load_quiz_questions(conn, quiz_type)
                .map(GameReadResult::QuizQuestions)
        }
    }
}

/// Cheap-clone handle to the persistence actor.
#[derive(Clone)]
pub struct PersistHandle {
    pub(crate) cmd_tx: Option<flume::Sender<DbCmd>>,
    pub(crate) read_tx: Option<flume::Sender<ReadCmd>>,
    pub(crate) completions_tx: Option<flume::Sender<DbReply>>,
    pub(crate) flush: Arc<FlushTracker>,
    pub(crate) stats: Arc<DbStats>,
    /// Fires when the writer thread has ended.
    pub(crate) writer_closed: Option<ClosedSignal>,
    /// Fires when every reader thread has ended.
    pub(crate) readers_closed: Option<ClosedSignal>,
    /// Counters of the backup layer.
    pub(crate) backup_stats: Arc<crate::backup::BackupStats>,
    /// Queue of the admin reader (`None` for a detached handle; Plan/25 §11.6).
    pub(crate) admin_tx: Option<flume::Sender<crate::admin::scan::AdminJob>>,
    /// The server-wide "one heavy admin scan" gate.
    pub(crate) heavy: Arc<crate::admin::scan::HeavyGate>,
    /// Chunk budget of the admin scans.
    pub(crate) admin_limits: crate::admin::scan::ChunkLimits,
}

impl PersistHandle {
    /// A handle without database or threads (replay, tests): game-thread methods are accepted and
    /// discarded; `call`/`read`/`backup` return `Err(ShuttingDown)`; `is_closed` is `false`.
    pub fn detached() -> PersistHandle {
        PersistHandle {
            cmd_tx: None,
            read_tx: None,
            completions_tx: None,
            flush: Arc::new(FlushTracker::default()),
            stats: Arc::new(DbStats::default()),
            writer_closed: None,
            readers_closed: None,
            backup_stats: Arc::new(crate::backup::BackupStats::default()),
            admin_tx: None,
            heavy: Arc::new(crate::admin::scan::HeavyGate::default()),
            admin_limits: crate::admin::scan::ChunkLimits::DEFAULT,
        }
    }

    /// `true` for [`PersistHandle::detached`] handles (no database, no threads).
    pub fn is_detached(&self) -> bool {
        self.cmd_tx.is_none()
    }

    fn send_cmd(&self, cmd: DbCmd) {
        if let Some(tx) = &self.cmd_tx {
            // The queue is unbounded: the game thread never blocks on persistence. A send error
            // means the writer is gone, which `is_closed()` reports to the tick loop.
            let _ = tx.send(cmd);
        }
    }

    // ---- game thread: never blocks, never fails ----

    /// Queues a character snapshot (coalesced per account, highest generation wins).
    pub fn save(&self, snapshot: CharacterSnapshot) {
        self.send_cmd(DbCmd::Save(Box::new(snapshot)));
    }

    /// Queues an atomic multi-entity batch.
    pub fn atomic(&self, batch: AtomicBatch) {
        self.send_cmd(DbCmd::Atomic(Box::new(batch)));
    }

    /// Queues one non-snapshot write.
    pub fn op(&self, op: DbOp) {
        self.atomic(AtomicBatch {
            kind: AtomicKind::Other("op"),
            snapshots: Vec::new(),
            ops: vec![op],
            reply: BatchReply::None,
        });
    }

    /// Requests a block of item serials; the answer arrives as `DbReply::SerialBlock`.
    pub fn reserve_serial_block(&self, ticket: Ticket) {
        self.send_cmd(DbCmd::ReserveSerialBlock { ticket });
    }

    /// Reserves a block of `[database] serial_block_size` item serials and waits for the commit, at most
    /// `timeout`: the blocking fallback of the serial allocator when its second block ran out (Plan/13 §15.1;
    /// the caller logs a warning). `Err(Timeout)` when the writer is slower than that.
    pub fn reserve_serial_block_blocking(
        &self,
        timeout: std::time::Duration,
    ) -> Result<std::ops::Range<u64>, DbError> {
        let Some(tx) = &self.cmd_tx else {
            return Err(DbError::shutting_down());
        };
        let (reply, rx) = reply_pair();
        tx.send(DbCmd::ReserveSerialBlockBlocking { reply })
            .map_err(|_| DbError::shutting_down())?;
        match wait_reply(rx, self.writer_closed.as_ref(), Some(timeout)) {
            Ok(result) => result,
            Err(ReplyError::Empty) => Err(DbError::new(
                DbErrorKind::Timeout,
                "the serial block reservation did not complete in time",
            )),
            Err(ReplyError::Disconnected) => Err(DbError::shutting_down()),
        }
    }

    /// Queues a typed read for the game thread; the answer arrives as `DbReply::ReadDone`.
    pub fn read_for_game(&self, ticket: Ticket, query: GameReadQuery) {
        let (Some(read_tx), Some(done_tx)) = (&self.read_tx, &self.completions_tx) else {
            return;
        };
        let done_tx = done_tx.clone();
        let job = ReadCmd::Run(Box::new(move |conn| {
            let result = run_game_read(conn, &query);
            let _ = done_tx.send(DbReply::ReadDone { ticket, result });
        }));
        // The game thread never blocks: a saturated reader queue answers `Busy` (the game retries later).
        if let Err(flume::TrySendError::Full(_) | flume::TrySendError::Disconnected(_)) =
            read_tx.try_send(job)
        {
            let _ = self.completions_tx.as_ref().map(|tx| {
                tx.send(DbReply::ReadDone {
                    ticket,
                    result: Err(DbError::new(
                        DbErrorKind::Busy,
                        "the read queue is full or closed",
                    )),
                })
            });
        }
    }

    /// Queues a maintenance job.
    pub fn maintenance(&self, job: MaintenanceJob) {
        self.send_cmd(DbCmd::Maintenance(job));
    }

    /// Queues the daily retention job (Plan/13 §23) without waiting; the report is logged and kept for
    /// [`PersistHandle::last_maintenance`]. `purge_characters` are the soft-deleted characters the game's
    /// directory selected (the writer re-checks their age); `now` is the time the retention windows count from.
    pub fn maintenance_daily(&self, purge_characters: Vec<CharacterId>, now: UnixTime) {
        self.maintenance(MaintenanceJob::Daily {
            purge_characters,
            now,
            reply: None,
        });
    }

    /// Runs the daily retention job and awaits its report (the admin console's `db maintenance now`).
    pub async fn run_daily(
        &self,
        purge_characters: Vec<CharacterId>,
        now: UnixTime,
    ) -> Result<MaintenanceReport, DbError> {
        let Some(tx) = &self.cmd_tx else {
            return Err(DbError::shutting_down());
        };
        let (reply, rx) = reply_pair();
        tx.send(DbCmd::Maintenance(MaintenanceJob::Daily {
            purge_characters,
            now,
            reply: Some(reply),
        }))
        .map_err(|_| DbError::shutting_down())?;
        wait_reply_async(rx, self.writer_closed.as_ref())
            .await
            .map_err(|_| DbError::shutting_down())?
    }

    /// Blocking variant of [`PersistHandle::run_daily`] for threads outside the async runtime (tests, CLI).
    pub fn run_daily_blocking(
        &self,
        purge_characters: Vec<CharacterId>,
        now: UnixTime,
    ) -> Result<MaintenanceReport, DbError> {
        let Some(tx) = &self.cmd_tx else {
            return Err(DbError::shutting_down());
        };
        let (reply, rx) = reply_pair();
        tx.send(DbCmd::Maintenance(MaintenanceJob::Daily {
            purge_characters,
            now,
            reply: Some(reply),
        }))
        .map_err(|_| DbError::shutting_down())?;
        wait_reply(rx, self.writer_closed.as_ref(), None).map_err(|_| DbError::shutting_down())?
    }

    /// The report of the last daily maintenance run that finished in this process.
    pub fn last_maintenance(&self) -> Option<MaintenanceReport> {
        self.stats
            .last_maintenance
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Queues a flush: `DbReply::Committed { ticket, Done }` arrives once everything queued before
    /// it is committed.
    pub fn flush(&self, ticket: Ticket) {
        self.send_cmd(DbCmd::Flush {
            reply: BatchReply::Game(ticket),
        });
    }

    // ---- lobby / admin ----

    /// Runs `batch` and awaits its outcome (the batch's own `reply` is replaced).
    pub async fn call(&self, mut batch: AtomicBatch) -> Result<AtomicOutcome, DbError> {
        let Some(tx) = &self.cmd_tx else {
            return Err(DbError::shutting_down());
        };
        let (reply, rx) = reply_pair();
        batch.reply = BatchReply::Lobby(reply);
        tx.send(DbCmd::Atomic(Box::new(batch)))
            .map_err(|_| DbError::shutting_down())?;
        wait_reply_async(rx, self.writer_closed.as_ref())
            .await
            .map_err(|_| DbError::shutting_down())?
    }

    /// Blocking variant of [`PersistHandle::call`] for threads outside the async runtime (console, offline
    /// CLI, tests): runs `batch` and waits for its outcome.
    pub fn call_blocking(&self, mut batch: AtomicBatch) -> Result<AtomicOutcome, DbError> {
        let Some(tx) = &self.cmd_tx else {
            return Err(DbError::shutting_down());
        };
        let (reply, rx) = reply_pair();
        batch.reply = BatchReply::Lobby(reply);
        tx.send(DbCmd::Atomic(Box::new(batch)))
            .map_err(|_| DbError::shutting_down())?;
        wait_reply(rx, self.writer_closed.as_ref(), None).map_err(|_| DbError::shutting_down())?
    }

    /// Blocks until everything queued before this call is committed (the writer's `Flush`), at most
    /// `timeout`. `Err(Timeout)` when it takes longer, `Err(ShuttingDown)` when the writer is gone.
    pub fn flush_blocking(&self, timeout: std::time::Duration) -> Result<(), DbError> {
        let Some(tx) = &self.cmd_tx else {
            return Err(DbError::shutting_down());
        };
        let (reply, rx) = reply_pair();
        tx.send(DbCmd::Flush {
            reply: BatchReply::Lobby(reply),
        })
        .map_err(|_| DbError::shutting_down())?;
        match wait_reply(rx, self.writer_closed.as_ref(), Some(timeout)) {
            Ok(result) => result.map(|_| ()),
            Err(ReplyError::Empty) => Err(DbError::new(
                DbErrorKind::Timeout,
                "the persistence writer did not flush in time",
            )),
            Err(ReplyError::Disconnected) => Err(DbError::shutting_down()),
        }
    }

    /// Runs `f` on a read-only connection and awaits the result.
    pub async fn read<R, F>(&self, f: F) -> Result<R, DbError>
    where
        F: FnOnce(&rusqlite::Connection) -> Result<R, DbError> + Send + 'static,
        R: Send + 'static,
    {
        let rx = self.start_read(f)?;
        wait_reply_async(rx, self.readers_closed.as_ref())
            .await
            .map_err(|_| DbError::shutting_down())?
    }

    /// Blocking variant of [`PersistHandle::read`] for threads outside the async runtime
    /// (console, CLI, tests).
    pub fn read_blocking<R, F>(&self, f: F) -> Result<R, DbError>
    where
        F: FnOnce(&rusqlite::Connection) -> Result<R, DbError> + Send + 'static,
        R: Send + 'static,
    {
        let rx = self.start_read(f)?;
        wait_reply(rx, self.readers_closed.as_ref(), None).map_err(|_| DbError::shutting_down())?
    }

    fn start_read<R, F>(&self, f: F) -> Result<ReplyRx<Result<R, DbError>>, DbError>
    where
        F: FnOnce(&rusqlite::Connection) -> Result<R, DbError> + Send + 'static,
        R: Send + 'static,
    {
        let Some(tx) = &self.read_tx else {
            return Err(DbError::shutting_down());
        };
        let (reply, rx) = reply_pair();
        let job = ReadCmd::Run(Box::new(move |conn| {
            let _ = reply.send(f(conn));
        }));
        // Async callers must not block a runtime worker: a saturated queue is `Busy`.
        tx.try_send(job).map_err(|e| match e {
            flume::TrySendError::Full(_) => {
                DbError::new(DbErrorKind::Busy, "the read queue is full")
            }
            flume::TrySendError::Disconnected(_) => DbError::shutting_down(),
        })?;
        Ok(rx)
    }

    /// Resolves when `account` has committed save generation `save_gen`.
    pub fn wait_flushed(
        &self,
        account: AccountId,
        save_gen: SaveGen,
    ) -> ReplyRx<Result<(), DbError>> {
        self.flush.wait_flushed(account, save_gen)
    }

    /// [`PersistHandle::wait_flushed`] with a deadline for threads outside the async runtime: `FlushTimeout`
    /// when `timeout` (`[database] flush_wait_timeout_ms`) passes first, `SaveFailed` for a poisoned account,
    /// `ShuttingDown` when the writer stopped. (Async callers wrap the `ReplyRx` in their runtime's timeout.)
    pub fn wait_flushed_timeout(
        &self,
        account: AccountId,
        save_gen: SaveGen,
        timeout: std::time::Duration,
    ) -> Result<(), DbError> {
        match self
            .flush
            .wait_flushed(account, save_gen)
            .recv_timeout(timeout)
        {
            Ok(result) => result,
            Err(ReplyError::Empty) => Err(DbError::new(
                DbErrorKind::FlushTimeout,
                format!(
                    "account {account}: generation {save_gen} was not flushed within {timeout:?}"
                ),
            )),
            Err(ReplyError::Disconnected) => Err(DbError::shutting_down()),
        }
    }

    /// Highest committed generation of `account`.
    pub fn flushed_gen(&self, account: AccountId) -> SaveGen {
        self.flush.flushed_gen(account)
    }

    /// Online backup of `kind` on a reader thread (Plan/13 §21.1): `Sched` and `Manual` land in `backups/`
    /// (rotated and hooked per the configuration), `Replay { dir }` as `<dir>/start.db`. The writer is never
    /// stalled. Returns the file written; `Busy` when the read queue is full.
    pub async fn backup(&self, kind: BackupKind) -> Result<std::path::PathBuf, DbError> {
        let rx = self.start_backup(kind)?;
        wait_reply_async(rx, self.readers_closed.as_ref())
            .await
            .map_err(|_| DbError::shutting_down())?
    }

    /// Blocking variant of [`PersistHandle::backup`] for threads outside the async runtime (console, CLI, the
    /// replay recorder on the game thread's helper, tests).
    pub fn backup_blocking(&self, kind: BackupKind) -> Result<std::path::PathBuf, DbError> {
        let rx = self.start_backup(kind)?;
        wait_reply(rx, self.readers_closed.as_ref(), None).map_err(|_| DbError::shutting_down())?
    }

    fn start_backup(
        &self,
        kind: BackupKind,
    ) -> Result<ReplyRx<Result<std::path::PathBuf, DbError>>, DbError> {
        let Some(tx) = &self.read_tx else {
            return Err(DbError::shutting_down());
        };
        let (reply, rx) = reply_pair();
        tx.try_send(ReadCmd::Backup { kind, reply })
            .map_err(|e| match e {
                flume::TrySendError::Full(_) => {
                    DbError::new(DbErrorKind::Busy, "the read queue is full")
                }
                flume::TrySendError::Disconnected(_) => DbError::shutting_down(),
            })?;
        Ok(rx)
    }

    /// Counters of the backup layer (`flyff_db_last_backup_timestamp_seconds`, ...).
    pub fn backup_stats(&self) -> crate::backup::BackupSnapshot {
        self.backup_stats.snapshot()
    }

    /// Releases an account whose last save failed permanently (admin console `db release <account>` only;
    /// Plan/13 §16.6): its logins stop failing with `SaveFailed`.
    pub fn release_poisoned_account(&self, account: AccountId) {
        if self.flush.release(account) {
            tracing::warn!(
                account = account.0,
                "poisoned account released by an administrator"
            );
        }
    }

    /// `true` while logins of `account` fail with `SaveFailed` (its last save failed permanently).
    pub fn is_poisoned(&self, account: AccountId) -> bool {
        self.flush.is_poisoned(account)
    }

    /// The histogram of commit durations of the writer (`flyff_db_batch_commit_seconds`): one observation per
    /// committed batch (the whole attempt: `BEGIN`, the ops of the drained commands, `COMMIT`; failed and retried
    /// attempts are not observed). Kept apart from the `Copy` [`DbStatsSnapshot`].
    pub fn commit_histogram(&self) -> flyff_common::rt::telemetry::HistogramSnapshot {
        self.stats.commit_hist.snapshot()
    }

    /// Counters for `/metrics`.
    pub fn stats(&self) -> DbStatsSnapshot {
        let s = &self.stats;
        DbStatsSnapshot {
            degraded: s.degraded.load(Ordering::Relaxed),
            queue_depth: self.cmd_tx.as_ref().map_or(0, flume::Sender::len),
            commands: s.commands.load(Ordering::Relaxed),
            batches: s.batches.load(Ordering::Relaxed),
            snapshots_written: s.snapshots_written.load(Ordering::Relaxed),
            snapshots_stale: s.snapshots_stale.load(Ordering::Relaxed),
            snapshots_coalesced: s.snapshots_coalesced.load(Ordering::Relaxed),
            anomalies: s.anomalies.load(Ordering::Relaxed),
            maintenance_runs: s.maintenance_runs.load(Ordering::Relaxed),
            maintenance_failures: s.maintenance_failures.load(Ordering::Relaxed),
            commands_failed: s.commands_failed.load(Ordering::Relaxed),
            bug_failures: s.bug_failures.load(Ordering::Relaxed),
            transient_failures: s.transient_failures.load(Ordering::Relaxed),
            last_commit_us: s.last_commit_us.load(Ordering::Relaxed),
            max_commit_us: s.max_commit_us.load(Ordering::Relaxed),
            checkpoints: s.checkpoints.load(Ordering::Relaxed),
            checkpoint_busy: s.checkpoint_busy.load(Ordering::Relaxed),
            wal_bytes: s.wal_bytes.load(Ordering::Relaxed),
            last_checkpoint_us: s.last_checkpoint_us.load(Ordering::Relaxed),
            admin_chunks: s.admin_chunks.load(Ordering::Relaxed),
            admin_rows: s.admin_rows.load(Ordering::Relaxed),
            admin_scans: s.admin_scans.load(Ordering::Relaxed),
            admin_scans_failed: s.admin_scans_failed.load(Ordering::Relaxed),
            admin_scans_cancelled: s.admin_scans_cancelled.load(Ordering::Relaxed),
            admin_scans_rejected: s.admin_scans_rejected.load(Ordering::Relaxed),
            admin_heavy_running: self.heavy.is_busy(),
        }
    }

    /// The chunk budget the admin reads run with.
    pub fn admin_limits(&self) -> crate::admin::scan::ChunkLimits {
        self.admin_limits
    }

    // ---- liveness ----

    /// The writer thread is gone (the game maps this to exit code 70).
    pub fn is_closed(&self) -> bool {
        self.stats.writer_gone.load(Ordering::Acquire)
    }

    /// `Some(Corrupt)` once the writer stopped on a corrupt database (exit code 74).
    pub fn fatal(&self) -> Option<DbErrorKind> {
        self.stats
            .corrupt
            .load(Ordering::Acquire)
            .then_some(DbErrorKind::Corrupt)
    }
}
