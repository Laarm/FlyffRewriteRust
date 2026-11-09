//! Command and reply types of the persistence actor (Plan/01 §4.4, 13 §16.2-16.3).

use crate::error::DbError;
pub use crate::maintenance::MaintenanceReport;
use crate::model::{
    CharacterSnapshot, PendingGrantRecord, PendingRemovalRecord, QuizQuestionRecord,
};
pub use crate::ops::{AtomicBatch, AtomicKind, AtomicOutcome, BatchReply, DbOp, Ticket};
use flyff_common::rt::Reply;
use flyff_common::{CharacterId, UnixTime};
use serde::{Deserialize, Serialize};

/// Commands from the game thread, lobby and console to the writer thread (C4).
pub enum DbCmd {
    /// Snapshot save; coalesced per account, highest generation wins.
    Save(Box<CharacterSnapshot>),
    /// One atomic transaction.
    Atomic(Box<AtomicBatch>),
    /// Reserve a block of item serials.
    ReserveSerialBlock {
        /// Correlation ticket.
        ticket: Ticket,
    },
    /// Like [`DbCmd::ReserveSerialBlock`], answered through a one-shot instead of the game's completion queue:
    /// the blocking fallback of the serial allocator when its second block ran out (Plan/13 §15.1).
    ReserveSerialBlockBlocking {
        /// Receives the reserved serials (or the error) after the commit.
        reply: Reply<Result<std::ops::Range<u64>, DbError>>,
    },
    /// Completes after everything queued before it is committed.
    Flush {
        /// Who is told.
        reply: BatchReply,
    },
    /// Maintenance job.
    Maintenance(MaintenanceJob),
    /// Stop the writer.
    Shutdown {
        /// Graceful or after a panic.
        mode: ShutdownMode,
        /// Receives the report once the writer stopped.
        reply: Reply<ShutdownReport>,
    },
}

impl DbCmd {
    /// `true` for the commands that run inside the batch transaction (`BEGIN IMMEDIATE .. COMMIT`).
    pub(crate) fn writes(&self) -> bool {
        matches!(
            self,
            DbCmd::Save(_)
                | DbCmd::Atomic(_)
                | DbCmd::ReserveSerialBlock { .. }
                | DbCmd::ReserveSerialBlockBlocking { .. }
        )
    }
}

/// `PRAGMA wal_checkpoint(<mode>)` (Plan/13 §22).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckpointMode {
    /// Copies what readers allow, never waits.
    Passive,
    /// Waits for readers, then restarts the WAL.
    Restart,
    /// Like `Restart`, then truncates the WAL file to zero bytes.
    Truncate,
}

impl CheckpointMode {
    /// The SQL spelling.
    pub const fn pragma(self) -> &'static str {
        match self {
            CheckpointMode::Passive => "PASSIVE",
            CheckpointMode::Restart => "RESTART",
            CheckpointMode::Truncate => "TRUNCATE",
        }
    }
}

/// Maintenance jobs (Plan/13 §23).
#[derive(Debug)]
#[non_exhaustive]
pub enum MaintenanceJob {
    /// `PRAGMA wal_checkpoint(TRUNCATE)`.
    CheckpointTruncate,
    /// `PRAGMA wal_checkpoint(<mode>)` (admin console, §22).
    Checkpoint(CheckpointMode),
    /// The daily retention job, sent by the game scheduler at `[retention] maintenance_hour` (job
    /// `schedule.last_fired.maintenance`) and by the console command `db maintenance now`: the writer runs
    /// [`crate::maintenance::run_daily`] in its own transaction after the batch it arrived with, then the space
    /// step. Idempotent.
    Daily {
        /// Characters the game selected for the purge from its `PlayerDirectory` (`state = 1 AND
        /// deleted_at < now - deleted_character_days`); the writer re-checks every one.
        purge_characters: Vec<CharacterId>,
        /// The time of the run (retention windows are measured from it; tests pass a fake clock).
        now: UnixTime,
        /// Receives the report when the run finished (the console); `None` = fire and forget (the report is
        /// logged and kept for [`crate::PersistHandle::last_maintenance`]).
        reply: Option<Reply<Result<MaintenanceReport, DbError>>>,
    },
}

/// Typed reads requested by the game thread (Plan/13 §16.7); the answer arrives as `DbReply::ReadDone`.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum GameReadQuery {
    /// `SELECT 1` probe.
    Ping,
    /// The open `pending_grants` with `id > after_id` of the given (online) characters, oldest first: the
    /// `gameplay.grant_poll_s` poll (30 s, PL-055) -> [`GameReadResult::Grants`].
    OpenGrants {
        /// The online characters to look for.
        character_ids: Vec<CharacterId>,
        /// Only grants with a larger id (0 = all).
        after_id: i64,
    },
    /// The open guild-bank `pending_removals` with `id > after_id` (the 30 s `GuildService` poll)
    /// -> [`GameReadResult::GuildRemovals`].
    OpenGuildRemovals {
        /// Only removals with a larger id (0 = all).
        after_id: i64,
    },
    /// The enabled quiz questions of a type when a quiz opens (Plan/13 §16.7, 22 §15.3) -> [`GameReadResult::
    /// QuizQuestions`]: by id with `used_at` (the database hides no used question, the quiz service chooses).
    /// `quiz_type` 1 = OX, 2 = four choices; any other value answers `Invalid`.
    QuizQuestions {
        /// The `QuizType` code.
        quiz_type: u8,
    },
}

/// Result of a [`GameReadQuery`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum GameReadResult {
    /// Probe answered.
    Pong,
    /// Answer of [`GameReadQuery::OpenGrants`], ordered by id.
    Grants(Vec<PendingGrantRecord>),
    /// Answer of [`GameReadQuery::OpenGuildRemovals`], ordered by id.
    GuildRemovals(Vec<PendingRemovalRecord>),
    /// Answer of [`GameReadQuery::QuizQuestions`], ordered by id (empty without imported questions, PD-12).
    QuizQuestions(Vec<QuizQuestionRecord>),
}

/// How the writer stops (Plan/13 §16.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShutdownMode {
    /// Drain, commit, checkpoint, optional backup.
    Graceful,
    /// Flush already-queued commands only; no backup (after a panic).
    AfterPanic,
}

/// What the writer reports when it stops.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShutdownReport {
    /// Commands processed over the writer's lifetime.
    pub commands: u64,
    /// The WAL checkpoint (`TRUNCATE`) succeeded.
    pub checkpointed: bool,
    /// Transactions committed over the writer's lifetime.
    pub batches: u64,
    /// Commands given up during a shutdown in retry mode (dumped to `logs/persist-failed-*.jsonl`).
    pub abandoned: u64,
}

/// Backup kinds (Plan/13 §21.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackupKind {
    /// Scheduled.
    Sched,
    /// Before a migration.
    Premigrate,
    /// Operator-requested.
    Manual,
    /// On shutdown.
    Shutdown,
    /// Before a restore.
    Prerestore,
    /// Replay start snapshot written into `dir`.
    Replay {
        /// Target directory.
        dir: std::path::PathBuf,
    },
}

impl BackupKind {
    /// The `<kind>` part of `backups/flyff-YYYYMMDD-HHMMSS-<kind>.db` (Plan/13 §2.1).
    pub fn label(&self) -> &'static str {
        match self {
            BackupKind::Sched => "sched",
            BackupKind::Premigrate => "premigrate",
            BackupKind::Manual => "manual",
            BackupKind::Shutdown => "shutdown",
            BackupKind::Prerestore => "prerestore",
            BackupKind::Replay { .. } => "replay",
        }
    }
}

/// Commands for the read-only threads (C5).
pub enum ReadCmd {
    /// Run a closure against a `query_only` connection.
    Run(Box<dyn FnOnce(&rusqlite::Connection) + Send>),
    /// Online backup.
    Backup {
        /// Kind.
        kind: BackupKind,
        /// Receives the written file.
        reply: Reply<Result<std::path::PathBuf, DbError>>,
    },
}

/// Completions delivered to the game thread (C6). Serialisable: completions are part of the game thread's
/// input stream and are recorded and replayed (Plan/13 §16.2, D2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DbReply {
    /// An atomic batch / flush committed.
    Committed {
        /// Ticket of the request.
        ticket: Ticket,
        /// Result.
        outcome: AtomicOutcome,
    },
    /// An atomic batch failed.
    Failed {
        /// Ticket of the request.
        ticket: Ticket,
        /// Error.
        error: DbError,
    },
    /// A serial block was reserved.
    SerialBlock {
        /// Ticket of the request.
        ticket: Ticket,
        /// Reserved serials.
        block: std::ops::Range<u64>,
    },
    /// A game read finished.
    ReadDone {
        /// Ticket of the request.
        ticket: Ticket,
        /// Result.
        result: Result<GameReadResult, DbError>,
    },
    /// The writer entered transient-retry mode.
    Degraded {
        /// Since when.
        since: UnixTime,
        /// Why.
        reason: String,
    },
    /// The writer recovered.
    Recovered,
}
