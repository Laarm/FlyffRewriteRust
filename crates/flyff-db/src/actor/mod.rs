//! The persistence actor: the single writer thread `flyff-persist` (Plan/13 §16.4-§16.6, §20, §22).
//!
//! One iteration of the loop is the normative algorithm of §16.4: wait for a command, drain up to
//! `batch_max_commands` more within `batch_max_wait_ms`, coalesce the standalone saves per character (highest
//! generation wins, §16.5), then apply every command inside **one** `BEGIN IMMEDIATE ... COMMIT`, each command
//! inside its own `SAVEPOINT`, so that
//!
//! * an *Expected* failure (`NameTaken`, ...) or a *Bug* rolls back only its own command,
//! * a *Transient* failure (busy, disk full, I/O) rolls back the whole batch and puts the writer into retry mode
//!   (backoff 1, 2, 5, 10, 30 s; nothing is dropped, order is preserved, new commands keep being queued and
//!   coalesced), announced by `DbReply::Degraded` and ended by `DbReply::Recovered`,
//! * a *Corrupt* failure stops the writer (`fatal() = Some(Corrupt)`, exit code 74).
//!
//! After the commit the writer publishes the flushed generations (waking `wait_flushed` callers), poisons the
//! accounts of snapshots that failed with a *Bug* (their commands are dumped to
//! `logs/persist-failed-YYYY-MM-DD.jsonl`) and sends the replies in arrival order.

pub(crate) mod failed;

use crate::audit::{AuditEvent, AuditLog, ev};
use crate::backup::BackupStats;
use crate::clock::now_unix;
use crate::cmd::{
    AtomicBatch, AtomicOutcome, BatchReply, CheckpointMode, DbCmd, DbReply, MaintenanceJob,
    MaintenanceReport, ShutdownMode, ShutdownReport,
};
use crate::config::RetentionConfig;
use crate::error::{DbError, DbErrorKind};
use crate::fault::{FaultHooks, OpProbe};
use crate::flush::FlushTracker;
use crate::handle::DbStats;
use crate::model::CharacterSnapshot;
use crate::ops::{ApplyCtx, Overlay, apply_op};
use crate::repo::meta::{self, kv};
use crate::repo::snapshot::{DocHashes, apply_snapshot};
use flyff_common::rt::Reply;
use flyff_common::{AccountId, CharacterId, SaveGen};
use rusqlite::Connection;
use std::collections::BTreeMap;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

/// Retry backoff of the transient-failure mode in seconds; the last value repeats (Plan/13 §16.6).
pub const RETRY_BACKOFF_S: [u64; 5] = [1, 2, 5, 10, 30];
/// `PRAGMA optimize` runs once per hour while the writer is idle (Plan/13 §16.4).
const OPTIMIZE_INTERVAL: Duration = Duration::from_secs(3600);
/// After a shutdown request, a writer in retry mode tries this many more times before it gives up.
const SHUTDOWN_RETRIES: u32 = 3;
/// The longest pause between two attempts once a shutdown was requested.
const SHUTDOWN_RETRY_PAUSE: Duration = Duration::from_secs(1);

/// Settings of the writer derived from the configuration.
#[derive(Clone, Debug)]
pub(crate) struct ActorConfig {
    /// `[database] batch_max_commands`.
    pub batch_max_commands: usize,
    /// `[database] batch_max_wait_ms`.
    pub batch_max_wait: Duration,
    /// `[database] checkpoint_interval_s`.
    pub checkpoint_interval: Duration,
    /// `[database] serial_block_size`.
    pub serial_block_size: u32,
    /// `<home>/logs` (the failed-command dumps go here).
    pub logs_dir: PathBuf,
    /// The database file (WAL size checks).
    pub db_path: PathBuf,
    /// `[retention]`: the windows of the daily maintenance job.
    pub retention: RetentionConfig,
}

/// What the writer shares with the handle, the game thread and the lobby.
pub(crate) struct Shared {
    pub flush: Arc<FlushTracker>,
    pub stats: Arc<DbStats>,
    pub done_tx: flume::Sender<DbReply>,
    pub faults: Arc<FaultHooks>,
    /// `persist.*` audit events.
    pub audit: AuditLog,
    /// A running backup holds the periodic `RESTART` checkpoint back (Plan/13 §22).
    pub backup: Arc<BackupStats>,
}

type ShutdownRequest = (ShutdownMode, Reply<ShutdownReport>);

/// Result of one command inside an attempt.
enum CmdResult {
    Done(AtomicOutcome),
    Serial(Range<u64>),
    Failed { error: DbError, bug: bool },
    Control,
}

/// A transaction attempt that did not commit.
enum AttemptFail {
    /// Retry mode.
    Transient(DbError),
    /// Stop the writer.
    Corrupt(DbError),
    /// The batch failed permanently (an unmapped error outside a command): every command fails.
    Batch(DbError),
}

impl AttemptFail {
    fn classify(e: DbError) -> Self {
        match e.kind {
            DbErrorKind::Transient => AttemptFail::Transient(e),
            DbErrorKind::Corrupt => AttemptFail::Corrupt(e),
            _ => AttemptFail::Batch(e),
        }
    }
}

/// A committed attempt.
struct Committed {
    results: Vec<CmdResult>,
    overlay: Overlay,
}

enum Flow {
    Continue,
    Stop,
}

/// The writer's state. Owned by the `flyff-persist` thread.
pub(crate) struct Writer {
    conn: Connection,
    cfg: ActorConfig,
    shared: Shared,
    /// Highest committed generation per account (the gen guard of §16.5).
    last_written: BTreeMap<AccountId, SaveGen>,
    /// Hashes of the documents last written per character (hash-skip, §5.5).
    doc_hashes: BTreeMap<CharacterId, DocHashes>,
    degraded: bool,
    commands: u64,
    batches: u64,
    abandoned: u64,
}

fn is_ends_drain(cmd: &DbCmd) -> bool {
    match cmd {
        DbCmd::Flush { .. }
        | DbCmd::Shutdown { .. }
        | DbCmd::Maintenance(_)
        | DbCmd::ReserveSerialBlockBlocking { .. } => true,
        DbCmd::Atomic(batch) => batch.reply.is_waited_on(),
        DbCmd::Save(_) | DbCmd::ReserveSerialBlock { .. } => false,
    }
}

fn notify(
    reply: BatchReply,
    result: Result<AtomicOutcome, DbError>,
    done_tx: &flume::Sender<DbReply>,
) {
    match reply {
        BatchReply::None => {
            if let Err(e) = result {
                tracing::warn!(error = %e, "fire-and-forget batch failed");
            }
        }
        BatchReply::Game(ticket) => {
            let msg = match result {
                Ok(outcome) => DbReply::Committed { ticket, outcome },
                Err(error) => DbReply::Failed { ticket, error },
            };
            let _ = done_tx.send(msg);
        }
        BatchReply::Lobby(reply) => {
            let _ = reply.send(result);
        }
    }
}

impl Writer {
    pub(crate) fn new(conn: Connection, cfg: ActorConfig, shared: Shared) -> Self {
        // Savepoint-shaped batches use many distinct statements (document updates vary by changed columns).
        conn.set_prepared_statement_cache_capacity(128);
        Self {
            conn,
            cfg,
            shared,
            last_written: BTreeMap::new(),
            doc_hashes: BTreeMap::new(),
            degraded: false,
            commands: 0,
            batches: 0,
            abandoned: 0,
        }
    }

    /// The writer loop (Plan/13 §16.4). Returns when a `Shutdown` was processed, the channel closed or a
    /// fatal error stopped the actor. Commands that raced the shutdown are answered with `ShuttingDown`.
    pub(crate) fn run(mut self, rx: &flume::Receiver<DbCmd>) {
        self.run_loop(rx);
        let late: Vec<DbCmd> = rx.try_iter().collect();
        if !late.is_empty() {
            let error = DbError::shutting_down();
            tracing::warn!(
                commands = late.len(),
                "commands arrived after the writer stopped"
            );
            self.fail_pending(late, &error);
        }
    }

    fn run_loop(&mut self, rx: &flume::Receiver<DbCmd>) {
        let mut next_checkpoint = Instant::now() + self.cfg.checkpoint_interval;
        let mut next_optimize = Instant::now() + OPTIMIZE_INTERVAL;
        loop {
            let wake = next_checkpoint.min(next_optimize);
            let first = match rx.recv_deadline(wake) {
                Ok(cmd) => cmd,
                Err(flume::RecvTimeoutError::Timeout) => {
                    self.on_idle_timers(&mut next_checkpoint, &mut next_optimize);
                    continue;
                }
                Err(flume::RecvTimeoutError::Disconnected) => {
                    // Every handle and the owner are gone without a Shutdown: leave a tidy WAL.
                    let _ = self.checkpoint(CheckpointMode::Truncate);
                    return;
                }
            };
            self.shared.faults.wait_gate();
            let cmds = self.drain(rx, first);
            if matches!(self.process(rx, cmds), Flow::Stop) {
                return;
            }
            // A writer that never goes idle (a busy server) still gets its periodic checkpoint (Plan/13 §22).
            if Instant::now() >= next_checkpoint.min(next_optimize) {
                self.on_idle_timers(&mut next_checkpoint, &mut next_optimize);
            }
        }
    }

    /// Drains up to `batch_max_commands` commands within `batch_max_wait` after the first one (§16.4). The
    /// window closes early when a caller is blocked on the answer (lobby batch, flush, shutdown).
    fn drain(&self, rx: &flume::Receiver<DbCmd>, first: DbCmd) -> Vec<DbCmd> {
        let deadline = Instant::now() + self.cfg.batch_max_wait;
        let mut urgent = is_ends_drain(&first);
        let mut cmds = vec![first];
        while cmds.len() < self.cfg.batch_max_commands
            && !matches!(cmds.last(), Some(DbCmd::Shutdown { .. }))
        {
            let next = if urgent {
                rx.try_recv().ok()
            } else {
                rx.recv_deadline(deadline).ok()
            };
            let Some(cmd) = next else { break };
            urgent |= is_ends_drain(&cmd);
            cmds.push(cmd);
        }
        cmds
    }

    /// Coalesces the standalone saves: per character only the highest generation survives, at the position of
    /// its first occurrence (§16.5). Atomic batches are never merged, split or reordered.
    fn coalesce(&self, cmds: &mut Vec<DbCmd>) {
        let mut first_pos: BTreeMap<(AccountId, CharacterId), usize> = BTreeMap::new();
        let mut out: Vec<DbCmd> = Vec::with_capacity(cmds.len());
        let mut dropped = 0u64;
        for cmd in cmds.drain(..) {
            match cmd {
                DbCmd::Save(snap) => {
                    let key = (snap.account_id, snap.character_id);
                    if let Some(&pos) = first_pos.get(&key) {
                        if let Some(DbCmd::Save(kept)) = out.get_mut(pos)
                            && snap.r#gen > kept.r#gen
                        {
                            **kept = *snap;
                        }
                        dropped += 1;
                    } else {
                        first_pos.insert(key, out.len());
                        out.push(DbCmd::Save(snap));
                    }
                }
                other => out.push(other),
            }
        }
        *cmds = out;
        if dropped > 0 {
            self.shared
                .stats
                .snapshots_coalesced
                .fetch_add(dropped, Ordering::Relaxed);
        }
    }

    /// Applies one drained batch until it commits (retrying transient failures) or the writer must stop.
    fn process(&mut self, rx: &flume::Receiver<DbCmd>, mut cmds: Vec<DbCmd>) -> Flow {
        self.commands += cmds.len() as u64;
        self.shared
            .stats
            .commands
            .store(self.commands, Ordering::Relaxed);
        let mut shutdown: Option<ShutdownRequest> = match cmds.pop() {
            Some(DbCmd::Shutdown { mode, reply }) => Some((mode, reply)),
            Some(other) => {
                cmds.push(other);
                None
            }
            None => None,
        };
        self.coalesce(&mut cmds);
        self.inject_panic_if_requested();

        let mut attempt_no = 0u32;
        loop {
            let started = Instant::now();
            match self.attempt(&cmds) {
                Ok(committed) => {
                    self.finish(cmds, committed, started);
                    break;
                }
                Err(AttemptFail::Transient(e)) => {
                    attempt_no += 1;
                    self.shared
                        .stats
                        .transient_failures
                        .fetch_add(1, Ordering::Relaxed);
                    if shutdown.is_some() && attempt_no > SHUTDOWN_RETRIES {
                        self.abandon(cmds, &e);
                        break;
                    }
                    self.enter_retry(&e);
                    self.retry_wait(rx, &mut cmds, &mut shutdown, attempt_no);
                }
                Err(AttemptFail::Corrupt(e)) => {
                    self.fatal(cmds, shutdown, &e);
                    return Flow::Stop;
                }
                Err(AttemptFail::Batch(e)) => {
                    self.fail_batch(cmds, &e);
                    break;
                }
            }
        }

        match shutdown {
            Some((mode, reply)) => {
                self.shutdown_sequence(mode, reply);
                Flow::Stop
            }
            None => Flow::Continue,
        }
    }

    #[allow(clippy::panic)] // The test hook injects a writer panic; in shipped builds the guard is `false`.
    fn inject_panic_if_requested(&self) {
        if self.shared.faults.should_panic() {
            panic!("injected panic in the persistence writer");
        }
    }

    // ---- one transaction attempt ----

    fn control(&self, sql: &str) -> Result<(), AttemptFail> {
        self.shared.faults.trace(sql);
        self.conn
            .execute_batch(sql)
            .map_err(|e| AttemptFail::classify(DbError::from(e)))
    }

    /// Applies every command inside one `BEGIN IMMEDIATE ... COMMIT`; each command inside its own savepoint.
    /// Nothing is replied and no in-memory state changes until the caller [`Writer::finish`]es the result,
    /// so a failed attempt can be repeated verbatim.
    fn attempt(&self, cmds: &[DbCmd]) -> Result<Committed, AttemptFail> {
        let needs_tx = cmds.iter().any(DbCmd::writes);
        let result = self.attempt_inner(cmds, needs_tx);
        if result.is_err() && needs_tx {
            // Best effort: the transaction may not have started, or the connection may be unusable.
            self.shared.faults.trace("ROLLBACK");
            let _ = self.conn.execute_batch("ROLLBACK");
        }
        result
    }

    fn attempt_inner(&self, cmds: &[DbCmd], needs_tx: bool) -> Result<Committed, AttemptFail> {
        let now = now_unix();
        let mut overlay = Overlay::default();
        let mut results = Vec::with_capacity(cmds.len());
        if needs_tx {
            if let Some(e) = self.shared.faults.on_begin() {
                return Err(AttemptFail::classify(e));
            }
            self.control("BEGIN IMMEDIATE")?;
        }
        for cmd in cmds {
            let result = match cmd {
                DbCmd::Save(snap) => self.run_snapshot_command(snap, now, &mut overlay)?,
                DbCmd::Atomic(batch) => self.run_atomic(batch, now, &mut overlay)?,
                DbCmd::ReserveSerialBlock { .. } | DbCmd::ReserveSerialBlockBlocking { .. } => {
                    self.run_reserve(now, &mut overlay)?
                }
                DbCmd::Flush { .. } | DbCmd::Maintenance(_) | DbCmd::Shutdown { .. } => {
                    CmdResult::Control
                }
            };
            results.push(result);
        }
        if needs_tx {
            if let Some(e) = self.shared.faults.on_commit() {
                return Err(AttemptFail::classify(e));
            }
            self.control("COMMIT")?;
        }
        Ok(Committed { results, overlay })
    }

    /// Runs `f` inside `SAVEPOINT c`: `RELEASE` on success; on an Expected or Bug failure `ROLLBACK TO` +
    /// `RELEASE` and the error becomes the command's result; Transient and Corrupt abort the attempt.
    fn scoped<F>(
        &self,
        now: flyff_common::UnixTime,
        overlay: &mut Overlay,
        f: F,
    ) -> Result<Result<(), DbError>, AttemptFail>
    where
        F: FnOnce(&Connection, &mut ApplyCtx<'_>) -> Result<(), DbError>,
    {
        self.control("SAVEPOINT c")?;
        let mut ctx = ApplyCtx::new(now, &self.last_written, &self.doc_hashes, overlay);
        let result = f(&self.conn, &mut ctx);
        let own = ctx.own;
        match result {
            Ok(()) => {
                self.control("RELEASE c")?;
                overlay.merge(own);
                Ok(Ok(()))
            }
            Err(e) => match e.kind {
                DbErrorKind::Transient => Err(AttemptFail::Transient(e)),
                DbErrorKind::Corrupt => Err(AttemptFail::Corrupt(e)),
                _ => {
                    self.control("ROLLBACK TO c")?;
                    self.control("RELEASE c")?;
                    Ok(Err(e))
                }
            },
        }
    }

    fn command_result(
        &self,
        scoped: Result<Result<(), DbError>, AttemptFail>,
        outcome: AtomicOutcome,
    ) -> Result<CmdResult, AttemptFail> {
        Ok(match scoped? {
            Ok(()) => CmdResult::Done(outcome),
            Err(error) => CmdResult::Failed {
                bug: !error.kind.is_expected(),
                error,
            },
        })
    }

    fn run_snapshot_command(
        &self,
        snap: &CharacterSnapshot,
        now: flyff_common::UnixTime,
        overlay: &mut Overlay,
    ) -> Result<CmdResult, AttemptFail> {
        let faults = &self.shared.faults;
        let scoped = self.scoped(now, overlay, |conn, ctx| {
            if let Some(e) = faults.probe(&OpProbe {
                batch: "Save",
                what: "snapshot",
                index: 0,
                count: 1,
            }) {
                return Err(e);
            }
            apply_snapshot(conn, ctx, snap).map(|_| ())
        });
        self.command_result(scoped, AtomicOutcome::Done)
    }

    fn run_atomic(
        &self,
        batch: &AtomicBatch,
        now: flyff_common::UnixTime,
        overlay: &mut Overlay,
    ) -> Result<CmdResult, AttemptFail> {
        let faults = &self.shared.faults;
        let label = batch.kind.label();
        let count = batch.snapshots.len() + batch.ops.len();
        let mut outcome = AtomicOutcome::Done;
        let scoped = self.scoped(now, overlay, |conn, ctx| {
            for (index, snap) in batch.snapshots.iter().enumerate() {
                if let Some(e) = faults.probe(&OpProbe {
                    batch: label,
                    what: "snapshot",
                    index,
                    count,
                }) {
                    return Err(e);
                }
                apply_snapshot(conn, ctx, snap)?;
            }
            for (i, op) in batch.ops.iter().enumerate() {
                if let Some(e) = faults.probe(&OpProbe {
                    batch: label,
                    what: op.name(),
                    index: batch.snapshots.len() + i,
                    count,
                }) {
                    return Err(e);
                }
                let produced = apply_op(conn, ctx, op)?;
                if outcome == AtomicOutcome::Done {
                    outcome = produced;
                }
            }
            Ok(())
        });
        self.command_result(scoped, outcome)
    }

    fn run_reserve(
        &self,
        now: flyff_common::UnixTime,
        overlay: &mut Overlay,
    ) -> Result<CmdResult, AttemptFail> {
        let size = self.cfg.serial_block_size;
        let mut block = None;
        let scoped = self.scoped(now, overlay, |conn, _ctx| {
            block = Some(meta::reserve_serial_block(conn, size)?);
            Ok(())
        });
        Ok(match scoped? {
            Ok(()) => match block {
                Some(range) => CmdResult::Serial(range),
                None => CmdResult::Failed {
                    error: DbError::new(DbErrorKind::Bug, "serial block missing after reserve"),
                    bug: true,
                },
            },
            Err(error) => CmdResult::Failed {
                bug: !error.kind.is_expected(),
                error,
            },
        })
    }

    // ---- after the commit ----

    /// Publishes a committed attempt: in-memory state, flush tracker, poisoning, failed-command dump,
    /// `Recovered`, replies in arrival order, then the maintenance commands.
    fn finish(&mut self, cmds: Vec<DbCmd>, committed: Committed, started: Instant) {
        let Committed { results, overlay } = committed;
        let wrote_tx = cmds.iter().any(DbCmd::writes);
        if wrote_tx {
            self.shared.faults.note_commit();
            self.batches += 1;
            let elapsed = started.elapsed();
            let micros = u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX);
            let stats = &self.shared.stats;
            stats.batches.store(self.batches, Ordering::Relaxed);
            stats.last_commit_us.store(micros, Ordering::Relaxed);
            stats.max_commit_us.fetch_max(micros, Ordering::Relaxed);
            stats
                .commit_hist
                .observe_ns(u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX));
        }
        let stats = &self.shared.stats;
        stats
            .snapshots_written
            .fetch_add(overlay.snapshots_written, Ordering::Relaxed);
        stats
            .snapshots_stale
            .fetch_add(overlay.snapshots_stale, Ordering::Relaxed);
        stats
            .anomalies
            .fetch_add(overlay.anomalies, Ordering::Relaxed);

        // In-memory writer state follows the commit, never precedes it.
        for (account, gen_) in &overlay.gens {
            let slot = self.last_written.entry(*account).or_insert(SaveGen::ZERO);
            if *gen_ > *slot {
                *slot = *gen_;
            }
        }
        self.doc_hashes.extend(overlay.hashes);
        // Wake `wait_flushed` callers.
        for (account, gen_) in &overlay.gens {
            self.shared.flush.advance(*account, *gen_);
        }

        // Permanent failures: dump, poison.
        let now = now_unix();
        for (cmd, result) in cmds.iter().zip(&results) {
            if let CmdResult::Failed { error, bug } = result {
                if *bug {
                    stats.bug_failures.fetch_add(1, Ordering::Relaxed);
                    failed::record(&self.cfg.logs_dir, cmd, error, now);
                    self.shared.audit.emit(AuditEvent::new(
                        ev::PERSIST_FAILURE,
                        serde_json::json!({"kind": format!("{:?}", error.kind), "error": error.context}),
                    ));
                    for account in failed::accounts_of(cmd) {
                        tracing::error!(
                            account = account.0,
                            "account poisoned: logins fail with SaveFailed until `db release`"
                        );
                        self.shared.flush.poison(account, now);
                        self.shared.audit.emit(AuditEvent::new(
                            ev::PERSIST_POISONED,
                            serde_json::json!({"account": account.0}),
                        ));
                    }
                } else {
                    stats.commands_failed.fetch_add(1, Ordering::Relaxed);
                }
            }
        }

        if self.degraded {
            self.degraded = false;
            self.shared.stats.degraded.store(false, Ordering::Release);
            tracing::info!("persistence recovered");
            let _ = self.shared.done_tx.send(DbReply::Recovered);
        }

        // Replies and maintenance jobs in arrival order: a Flush queued behind a checkpoint answers after it.
        for (cmd, result) in cmds.into_iter().zip(results) {
            match cmd {
                DbCmd::Save(_) => {}
                DbCmd::Atomic(batch) => {
                    let AtomicBatch { reply, .. } = *batch;
                    let outcome = match result {
                        CmdResult::Done(o) => Ok(o),
                        CmdResult::Failed { error, .. } => Err(error),
                        CmdResult::Serial(_) | CmdResult::Control => Ok(AtomicOutcome::Done),
                    };
                    notify(reply, outcome, &self.shared.done_tx);
                }
                DbCmd::ReserveSerialBlock { ticket } => {
                    let msg = match result {
                        CmdResult::Serial(block) => DbReply::SerialBlock { ticket, block },
                        CmdResult::Failed { error, .. } => DbReply::Failed { ticket, error },
                        CmdResult::Done(_) | CmdResult::Control => DbReply::Failed {
                            ticket,
                            error: DbError::new(DbErrorKind::Bug, "no serial block produced"),
                        },
                    };
                    let _ = self.shared.done_tx.send(msg);
                }
                DbCmd::ReserveSerialBlockBlocking { reply } => {
                    let _ = reply.send(match result {
                        CmdResult::Serial(block) => Ok(block),
                        CmdResult::Failed { error, .. } => Err(error),
                        CmdResult::Done(_) | CmdResult::Control => {
                            Err(DbError::new(DbErrorKind::Bug, "no serial block produced"))
                        }
                    });
                }
                DbCmd::Flush { reply } => {
                    notify(reply, Ok(AtomicOutcome::Done), &self.shared.done_tx)
                }
                DbCmd::Maintenance(job) => self.run_maintenance(job),
                DbCmd::Shutdown { .. } => {}
            }
        }
    }

    /// Every command of the batch fails with `error` (an unmapped failure outside a single command, e.g. at
    /// `COMMIT`): the callers are told, snapshot owners are poisoned and the commands dumped.
    fn fail_batch(&mut self, cmds: Vec<DbCmd>, error: &DbError) {
        tracing::error!(error = %error, "batch failed permanently");
        let now = now_unix();
        for cmd in cmds {
            match cmd {
                DbCmd::Save(_) | DbCmd::Atomic(_) => {
                    self.shared
                        .stats
                        .bug_failures
                        .fetch_add(1, Ordering::Relaxed);
                    failed::record(&self.cfg.logs_dir, &cmd, error, now);
                    for account in failed::accounts_of(&cmd) {
                        self.shared.flush.poison(account, now);
                    }
                    if let DbCmd::Atomic(batch) = cmd {
                        notify(batch.reply, Err(error.clone()), &self.shared.done_tx);
                    }
                }
                DbCmd::ReserveSerialBlock { ticket } => {
                    let _ = self.shared.done_tx.send(DbReply::Failed {
                        ticket,
                        error: error.clone(),
                    });
                }
                DbCmd::ReserveSerialBlockBlocking { reply } => {
                    let _ = reply.send(Err(error.clone()));
                }
                DbCmd::Flush { reply } => notify(reply, Err(error.clone()), &self.shared.done_tx),
                DbCmd::Maintenance(_) | DbCmd::Shutdown { .. } => {}
            }
        }
    }

    // ---- retry mode (§16.6) ----

    fn enter_retry(&mut self, e: &DbError) {
        tracing::error!(error = %e, "persistence: transient failure, retrying with backoff");
        if !self.degraded {
            self.degraded = true;
            self.shared.stats.degraded.store(true, Ordering::Release);
            self.shared.audit.emit(AuditEvent::new(
                ev::PERSIST_DEGRADED,
                serde_json::json!({"since": now_unix().0, "reason": e.to_string()}),
            ));
            let _ = self.shared.done_tx.send(DbReply::Degraded {
                since: now_unix(),
                reason: e.to_string(),
            });
        }
    }

    fn backoff_delay(&self, attempt: u32, shutting_down: bool) -> Duration {
        let index = usize::try_from(attempt.saturating_sub(1)).unwrap_or(usize::MAX);
        let delay = match self.shared.faults.backoff() {
            Some(schedule) => schedule
                .get(index)
                .or_else(|| schedule.last())
                .copied()
                .unwrap_or(Duration::from_millis(1)),
            None => {
                let s = RETRY_BACKOFF_S
                    .get(index)
                    .or_else(|| RETRY_BACKOFF_S.last())
                    .copied()
                    .unwrap_or(30);
                Duration::from_secs(s)
            }
        };
        if shutting_down {
            delay.min(SHUTDOWN_RETRY_PAUSE)
        } else {
            delay
        }
    }

    /// Waits out the backoff while still draining the channel into `cmds` (and coalescing), so nothing queued
    /// during an outage is lost and order is preserved (head-of-line). A `Shutdown` that arrives meanwhile
    /// shortens the wait to one second: the writer must not sleep through a shutdown.
    fn retry_wait(
        &self,
        rx: &flume::Receiver<DbCmd>,
        cmds: &mut Vec<DbCmd>,
        shutdown: &mut Option<ShutdownRequest>,
        attempt: u32,
    ) {
        let mut until = Instant::now() + self.backoff_delay(attempt, shutdown.is_some());
        while shutdown.is_none() {
            match rx.recv_deadline(until) {
                Ok(DbCmd::Shutdown { mode, reply }) => {
                    *shutdown = Some((mode, reply));
                    until = until.min(Instant::now() + SHUTDOWN_RETRY_PAUSE);
                }
                Ok(cmd) => cmds.push(cmd),
                Err(_) => break,
            }
        }
        let rest = until.saturating_duration_since(Instant::now());
        if !rest.is_zero() {
            std::thread::sleep(rest);
        }
        self.coalesce(cmds);
    }

    /// The writer could not commit before shutdown: the commands are dumped for manual recovery and their
    /// callers told.
    fn abandon(&mut self, cmds: Vec<DbCmd>, cause: &DbError) {
        let error = DbError::new(
            DbErrorKind::ShuttingDown,
            format!("not committed before shutdown: {cause}"),
        );
        tracing::error!(commands = cmds.len(), error = %cause, "abandoning commands at shutdown");
        self.fail_pending(cmds, &error);
    }

    /// Dumps the data-carrying commands to `persist-failed-*.jsonl` and answers every caller with `error`.
    fn fail_pending(&mut self, cmds: Vec<DbCmd>, error: &DbError) {
        let now = now_unix();
        for cmd in cmds {
            self.abandoned += 1;
            if matches!(cmd, DbCmd::Save(_) | DbCmd::Atomic(_)) {
                failed::record(&self.cfg.logs_dir, &cmd, error, now);
            }
            match cmd {
                DbCmd::Atomic(batch) => {
                    notify(batch.reply, Err(error.clone()), &self.shared.done_tx)
                }
                DbCmd::Flush { reply } => notify(reply, Err(error.clone()), &self.shared.done_tx),
                DbCmd::ReserveSerialBlock { ticket } => {
                    let _ = self.shared.done_tx.send(DbReply::Failed {
                        ticket,
                        error: error.clone(),
                    });
                }
                DbCmd::ReserveSerialBlockBlocking { reply } => {
                    let _ = reply.send(Err(error.clone()));
                }
                DbCmd::Save(_) | DbCmd::Maintenance(_) | DbCmd::Shutdown { .. } => {}
            }
        }
    }

    /// `Corrupt`: stop applying, fail what is in hand, report to the shutdown waiter and end the thread
    /// (the handle's `fatal()` now says `Corrupt`, exit code 74).
    fn fatal(&mut self, cmds: Vec<DbCmd>, shutdown: Option<ShutdownRequest>, error: &DbError) {
        tracing::error!(error = %error, "database is corrupt: the persistence writer stops (restore a backup)");
        self.shared.stats.corrupt.store(true, Ordering::Release);
        self.shared.stats.writer_gone.store(true, Ordering::Release);
        self.fail_pending(cmds, error);
        if let Some((_, reply)) = shutdown {
            let _ = reply.send(self.report(false));
        }
    }

    // ---- shutdown (§16.8) and maintenance (§22, §23) ----

    fn report(&self, checkpointed: bool) -> ShutdownReport {
        ShutdownReport {
            commands: self.commands,
            checkpointed,
            batches: self.batches,
            abandoned: self.abandoned,
        }
    }

    /// Graceful: stamp `server.last_clean_shutdown_at`, then `wal_checkpoint(TRUNCATE)`. After a panic only the
    /// checkpoint runs (no bookkeeping, no backup).
    fn shutdown_sequence(&mut self, mode: ShutdownMode, reply: Reply<ShutdownReport>) {
        if matches!(mode, ShutdownMode::Graceful) {
            let now = now_unix();
            if let Err(e) = meta::set_kv(&self.conn, kv::LAST_CLEAN_SHUTDOWN_AT, &now.0.into(), now)
            {
                tracing::warn!(error = %e, "could not record the clean shutdown");
            }
        }
        let checkpointed = match self.checkpoint(CheckpointMode::Truncate) {
            Ok(()) => true,
            Err(e) => {
                tracing::warn!(error = %e, "wal checkpoint at shutdown failed");
                false
            }
        };
        let _ = reply.send(self.report(checkpointed));
    }

    fn run_maintenance(&mut self, job: MaintenanceJob) {
        let mode = match job {
            MaintenanceJob::CheckpointTruncate => CheckpointMode::Truncate,
            MaintenanceJob::Checkpoint(mode) => mode,
            MaintenanceJob::Daily {
                purge_characters,
                now,
                reply,
            } => {
                self.run_daily(&purge_characters, now, reply);
                return;
            }
        };
        if let Err(e) = self.checkpoint(mode) {
            tracing::warn!(error = %e, ?mode, "wal checkpoint failed");
        }
    }

    /// `MaintenanceJob::Daily` (Plan/13 §23): the retention steps in their own transaction, then the space step.
    /// A failure rolls the transaction back, is logged and reported to the waiter; the job is idempotent, so the
    /// next run (tomorrow, or `db maintenance now`) simply tries again.
    fn run_daily(
        &mut self,
        purge: &[CharacterId],
        now: flyff_common::UnixTime,
        reply: Option<Reply<Result<MaintenanceReport, DbError>>>,
    ) {
        let result = self.daily_transaction(purge, now);
        let stats = &self.shared.stats;
        match &result {
            Ok(report) => {
                stats.maintenance_runs.fetch_add(1, Ordering::Relaxed);
                tracing::info!(
                    characters = report.characters_purged,
                    mails = report.mails_purged,
                    notes = report.notes_purged,
                    grants = report.grants_purged,
                    removals = report.removals_purged,
                    guild_bank_log = report.guild_bank_log_trimmed,
                    couple_proposals = report.couple_proposals_purged,
                    rejoin_cooldowns = report.rejoin_cooldowns_purged,
                    vacuumed = report.vacuumed,
                    elapsed_ms = report.elapsed_ms,
                    "daily maintenance finished"
                );
                if report.characters_purged > 0 {
                    self.shared.audit.emit(AuditEvent::new(
                        ev::CHAR_PURGE,
                        serde_json::json!({
                            "requested": purge.iter().map(|c| c.0).collect::<Vec<_>>(),
                            "purged": report.characters_purged,
                        }),
                    ));
                }
                stats.set_last_maintenance(report.clone());
            }
            Err(e) => {
                stats.maintenance_failures.fetch_add(1, Ordering::Relaxed);
                tracing::error!(error = %e, "daily maintenance failed (it is idempotent and runs again)");
            }
        }
        if let Some(reply) = reply {
            let _ = reply.send(result);
        }
    }

    fn daily_transaction(
        &self,
        purge: &[CharacterId],
        now: flyff_common::UnixTime,
    ) -> Result<MaintenanceReport, DbError> {
        let started = Instant::now();
        let faults = &self.shared.faults;
        faults.trace("BEGIN IMMEDIATE");
        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        let mut report =
            match crate::maintenance::run_daily(&self.conn, &self.cfg.retention, purge, now) {
                Ok(report) => report,
                Err(e) => {
                    faults.trace("ROLLBACK");
                    let _ = self.conn.execute_batch("ROLLBACK");
                    return Err(e);
                }
            };
        faults.trace("COMMIT");
        if let Err(e) = self.conn.execute_batch("COMMIT") {
            faults.trace("ROLLBACK");
            let _ = self.conn.execute_batch("ROLLBACK");
            return Err(e.into());
        }
        // The space step cannot run inside a transaction. A running backup holds a read transaction on the
        // pages the vacuum would move: skip the vacuum then, the next run catches up (Plan/13 §22).
        match crate::maintenance::space_and_stats(&self.conn, !self.shared.backup.is_running()) {
            Ok(vacuumed) => report.vacuumed = vacuumed,
            Err(e) => tracing::warn!(error = %e, "maintenance: the space step failed"),
        }
        report.elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        Ok(report)
    }

    /// `PRAGMA wal_checkpoint(<mode>)`; a `busy` result (readers or another writer in the way) is counted and
    /// reported as success: the next checkpoint catches up. Updates `flyff_db_wal_bytes` and
    /// `flyff_db_checkpoint_seconds`.
    fn checkpoint(&self, mode: CheckpointMode) -> Result<(), DbError> {
        let started = Instant::now();
        let busy: i64 = self.conn.query_row(
            &format!("PRAGMA wal_checkpoint({})", mode.pragma()),
            [],
            |r| r.get(0),
        )?;
        let stats = &self.shared.stats;
        stats.checkpoints.fetch_add(1, Ordering::Relaxed);
        if busy != 0 {
            stats.checkpoint_busy.fetch_add(1, Ordering::Relaxed);
        }
        let micros = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
        stats.last_checkpoint_us.store(micros, Ordering::Relaxed);
        stats.wal_bytes.store(self.wal_bytes(), Ordering::Relaxed);
        tracing::debug!(?mode, busy, micros, "wal checkpoint");
        Ok(())
    }

    fn wal_bytes(&self) -> u64 {
        crate::maintenance::wal_size(&self.cfg.db_path)
    }

    fn on_idle_timers(&mut self, next_checkpoint: &mut Instant, next_optimize: &mut Instant) {
        let now = Instant::now();
        if now >= *next_checkpoint {
            // Periodic PASSIVE checkpoint; a WAL that stays larger than 64 MiB is restarted unless a backup holds
            // a read transaction open (§22, `maintenance::periodic_checkpoints`).
            if let Err(e) = self.checkpoint(CheckpointMode::Passive) {
                tracing::warn!(error = %e, "periodic wal checkpoint failed");
            } else {
                let modes = crate::maintenance::periodic_checkpoints(
                    self.wal_bytes(),
                    self.shared.backup.is_running(),
                );
                for mode in modes.into_iter().skip(1) {
                    if let Err(e) = self.checkpoint(mode) {
                        tracing::warn!(error = %e, ?mode, "wal restart checkpoint failed");
                    }
                }
            }
            *next_checkpoint = Instant::now() + self.cfg.checkpoint_interval;
        }
        if now >= *next_optimize {
            if let Err(e) = self.conn.execute_batch("PRAGMA optimize") {
                tracing::warn!(error = %e, "PRAGMA optimize failed");
            }
            *next_optimize = Instant::now() + OPTIMIZE_INTERVAL;
        }
    }
}
