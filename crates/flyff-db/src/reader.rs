//! The read-only connection threads `flyff-persist-ro-N` (Plan/13 §2.3, §16.7, ADR-012).
//!
//! `[database] read_connections` (1..=4) threads each own one `query_only` connection. They serve lobby queries
//! (login, character list, enter-world load), game-requested reads (grant poll) and online backups from one
//! shared bounded channel (C5), and never block the single writer: readers see the last committed state through
//! WAL snapshot isolation.
//!
//! **Admin reads** (Plan/25 §11.6, [`crate::admin::scan`]) arrive on a second, unbounded channel as chunks
//! (`AdminJob`). With `read_connections >= 2` one **additional** connection and thread is the dedicated admin
//! reader (`flyff-persist-ro-admin`): it serves only admin chunks, so a whole-database scan never delays lobby
//! work, and the `read_connections` lobby/game readers stay exactly as configured. With a single reader that
//! thread serves lobby/game commands first and takes an admin chunk only when none waits (or after
//! `ADMIN_TURN_AFTER` consecutive commands, so the console cannot be starved).

use crate::admin::scan::{ADMIN_TURN_AFTER, AdminCtx, AdminJob, ChunkLimits};
use crate::backup::{BackupCtx, run_backup};
use crate::cmd::ReadCmd;
use crate::config::DbConfig;
use crate::conn::open_reader;
use crate::handle::DbStats;
use rusqlite::Connection;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

/// Capacity of the shared read-command channel.
pub const READ_QUEUE_CAPACITY: usize = 1024;

/// Which channels a reader thread serves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    /// Lobby, game and backup commands only (one of several readers).
    General,
    /// Admin chunks only (the dedicated admin reader).
    Admin,
    /// Both, lobby first (the only reader).
    Shared,
}

/// The reader threads and the sending ends of their channels.
pub struct ReaderPool {
    tx: flume::Sender<ReadCmd>,
    admin_tx: flume::Sender<AdminJob>,
    threads: Vec<(Role, JoinHandle<()>)>,
    stop: Arc<AtomicBool>,
    /// Disconnects when every reader thread has ended (see `PersistHandle`'s closed signals).
    closed: flume::Receiver<()>,
}

/// Everything a reader thread needs besides its connection.
struct ReaderCtx {
    rx: flume::Receiver<ReadCmd>,
    admin_rx: flume::Receiver<AdminJob>,
    admin_tx: flume::Sender<AdminJob>,
    stop: Arc<AtomicBool>,
    backup: Arc<BackupCtx>,
    stats: Arc<DbStats>,
    limits: ChunkLimits,
}

impl ReaderPool {
    /// Opens `cfg.read_connections` (clamped to 1..=4) read-only connections on the existing database and
    /// starts one thread per connection; with two or more, the last one is the admin reader.
    pub(crate) fn spawn(
        db_path: &Path,
        cfg: &DbConfig,
        backup: Arc<BackupCtx>,
        stats: Arc<DbStats>,
        limits: ChunkLimits,
    ) -> Result<ReaderPool, crate::error::DbError> {
        let (tx, rx) = flume::bounded::<ReadCmd>(READ_QUEUE_CAPACITY);
        let (admin_tx, admin_rx) = flume::unbounded::<AdminJob>();
        let stop = Arc::new(AtomicBool::new(false));
        let (alive_tx, closed) = flume::bounded::<()>(1);
        let count = cfg.read_connections.clamp(1, 4);
        // One reader serves everything; two or more get an additional connection for the admin reads.
        let mut roles = vec![
            if count == 1 {
                Role::Shared
            } else {
                Role::General
            };
            usize::from(count)
        ];
        if count >= 2 {
            roles.push(Role::Admin);
        }
        let mut threads = Vec::new();
        for (i, role) in roles.into_iter().enumerate() {
            let conn = open_reader(db_path, cfg)?;
            let ctx = ReaderCtx {
                rx: rx.clone(),
                admin_rx: admin_rx.clone(),
                admin_tx: admin_tx.clone(),
                stop: Arc::clone(&stop),
                backup: Arc::clone(&backup),
                stats: Arc::clone(&stats),
                limits,
            };
            let alive = alive_tx.clone();
            let name = if role == Role::Admin {
                "flyff-persist-ro-admin".to_owned()
            } else {
                format!("flyff-persist-ro-{i}")
            };
            let thread = std::thread::Builder::new().name(name).spawn(move || {
                reader_loop(&conn, role, &ctx);
                drop(alive);
            })?;
            threads.push((role, thread));
        }
        Ok(ReaderPool {
            tx,
            admin_tx,
            threads,
            stop,
            closed,
        })
    }

    /// The signal that fires when every reader thread has ended.
    pub(crate) fn closed_signal(&self) -> flume::Receiver<()> {
        self.closed.clone()
    }

    /// A sender for read commands (cheap to clone; the handle keeps one).
    pub fn sender(&self) -> flume::Sender<ReadCmd> {
        self.tx.clone()
    }

    /// A sender for admin chunks (the handle keeps one).
    pub(crate) fn admin_sender(&self) -> flume::Sender<AdminJob> {
        self.admin_tx.clone()
    }

    /// Number of reader threads.
    pub fn len(&self) -> usize {
        self.threads.len()
    }

    /// `true` when no reader thread is running.
    pub fn is_empty(&self) -> bool {
        self.threads.is_empty()
    }

    /// `true` when a thread serves only admin reads (`read_connections >= 2`).
    pub fn has_dedicated_admin_reader(&self) -> bool {
        self.threads.iter().any(|(role, _)| *role == Role::Admin)
    }

    /// Runs a backup of `kind` on a reader thread and waits for it, at most `timeout`.
    pub(crate) fn backup_blocking(
        &self,
        kind: crate::cmd::BackupKind,
        timeout: std::time::Duration,
    ) -> Result<std::path::PathBuf, crate::error::DbError> {
        use crate::error::{DbError, DbErrorKind};
        let (reply, rx) = flyff_common::rt::reply_pair();
        self.tx
            .try_send(ReadCmd::Backup { kind, reply })
            .map_err(|_| DbError::new(DbErrorKind::Busy, "the read queue is full or closed"))?;
        rx.recv_timeout(timeout)
            .map_err(|_| DbError::new(DbErrorKind::Timeout, "the backup did not finish in time"))?
    }

    /// Stops and joins every reader. Commands still queued are dropped, which makes their callers see
    /// `ShuttingDown`.
    pub fn stop_and_join(&mut self) {
        self.stop.store(true, Ordering::Release);
        for (role, _) in &self.threads {
            // Wake every blocked reader so it observes the stop flag. A full queue needs no wake-up: every
            // reader finds a command, sees the flag and exits.
            match role {
                Role::Admin => {
                    let _ = self.admin_tx.send(Box::new(|_, _| {}));
                }
                Role::General | Role::Shared => {
                    let _ = self.tx.try_send(ReadCmd::Run(Box::new(|_| {})));
                }
            }
        }
        for (_, thread) in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

fn reader_loop(conn: &Connection, role: Role, ctx: &ReaderCtx) {
    match role {
        Role::General => {
            while let Ok(cmd) = ctx.rx.recv() {
                if ctx.stop.load(Ordering::Acquire) {
                    // Dropping `cmd` drops its reply: the caller is told the persistence layer is shutting down.
                    break;
                }
                run_command(conn, cmd, ctx);
            }
        }
        Role::Admin => {
            while let Ok(job) = ctx.admin_rx.recv() {
                if ctx.stop.load(Ordering::Acquire) {
                    break;
                }
                run_admin(conn, job, ctx);
            }
        }
        Role::Shared => shared_loop(conn, ctx),
    }
}

/// The only reader: lobby/game/backup commands first, an admin chunk when none waits.
fn shared_loop(conn: &Connection, ctx: &ReaderCtx) {
    enum Next {
        Command(Option<ReadCmd>),
        Admin(Option<AdminJob>),
    }
    let mut streak = 0_u32;
    loop {
        if ctx.stop.load(Ordering::Acquire) {
            return;
        }
        if streak < ADMIN_TURN_AFTER {
            match ctx.rx.try_recv() {
                Ok(cmd) => {
                    if ctx.stop.load(Ordering::Acquire) {
                        return;
                    }
                    run_command(conn, cmd, ctx);
                    streak += 1;
                    continue;
                }
                Err(flume::TryRecvError::Empty) => {}
                Err(flume::TryRecvError::Disconnected) => return,
            }
        }
        if let Ok(job) = ctx.admin_rx.try_recv() {
            streak = 0;
            if ctx.stop.load(Ordering::Acquire) {
                return;
            }
            run_admin(conn, job, ctx);
            continue;
        }
        if streak >= ADMIN_TURN_AFTER {
            // Nobody from the console waits: back to lobby priority.
            streak = 0;
            continue;
        }
        let next = flume::Selector::new()
            .recv(&ctx.rx, |r| Next::Command(r.ok()))
            .recv(&ctx.admin_rx, |r| Next::Admin(r.ok()))
            .wait();
        match next {
            Next::Command(Some(cmd)) => {
                if ctx.stop.load(Ordering::Acquire) {
                    return;
                }
                run_command(conn, cmd, ctx);
                streak = 1;
            }
            Next::Admin(Some(job)) => {
                if ctx.stop.load(Ordering::Acquire) {
                    return;
                }
                run_admin(conn, job, ctx);
            }
            Next::Command(None) | Next::Admin(None) => return,
        }
    }
}

fn run_command(conn: &Connection, cmd: ReadCmd, ctx: &ReaderCtx) {
    match cmd {
        ReadCmd::Run(job) => {
            // A panicking job must not shrink the pool; its reply is dropped during the unwind.
            if catch_unwind(AssertUnwindSafe(|| job(conn))).is_err() {
                tracing::error!("a read job panicked on a flyff-persist-ro thread");
            }
        }
        ReadCmd::Backup { kind, reply } => {
            // a panic inside a backup must not shrink the pool either
            let result = catch_unwind(AssertUnwindSafe(|| run_backup(conn, &ctx.backup, &kind)))
                .unwrap_or_else(|_| {
                    tracing::error!("a backup panicked on a flyff-persist-ro thread");
                    Err(crate::error::DbError::new(
                        crate::error::DbErrorKind::Bug,
                        "the backup panicked",
                    ))
                });
            let _ = reply.send(result);
        }
    }
}

fn run_admin(conn: &Connection, job: AdminJob, ctx: &ReaderCtx) {
    let requeue = |next: AdminJob| {
        let _ = ctx.admin_tx.send(next);
    };
    let admin = AdminCtx {
        requeue: &requeue,
        limits: ctx.limits,
        stats: &ctx.stats,
    };
    if catch_unwind(AssertUnwindSafe(|| job(conn, &admin))).is_err() {
        tracing::error!("an admin read job panicked on a flyff-persist-ro thread");
    }
}
