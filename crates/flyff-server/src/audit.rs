//! The audit lines of the server layer (Plan/13 §25, 12 §19.1, 10 §9.2 "CLI audit rule"): the lobby's events
//! (`account.*`, `session.takeover`, `session.reattach`, `session.login`, `char.create`, `char.delete`), the
//! admin surfaces' (`gm.command`), the runtime's (`server.start`, `server.stop`, `server.shutdown_requested`,
//! `server.config_reload`) and the offline CLI's (`account.*`, `char.undelete`).
//!
//! The writer is `flyff_db::AuditLog` (WP-DB-09): a bounded queue, one `flyff-audit` thread, daily UTC rotation to
//! `logs/audit/audit-YYYY-MM-DD.jsonl`, retention, secrets scrubbed. [`AuditFile`] is the server layer's handle on
//! it: the same `emit(ev, &Actor, data)` calls as before the writer existed, with the actor context of the server
//! layer ([`Actor`]: account, character, name, address). The running server shares the log that `flyff_db::open`
//! returned (one writer thread for the whole process); an offline CLI command starts its own through
//! [`AuditFile::open`], and its last line is on disk when the handle is dropped.

use flyff_common::{AccountId, CharacterId};
use flyff_db::{AuditActor, AuditConfig, AuditError, AuditEvent, AuditLog, AuditStats};
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// The longest line (13 §25.1).
pub use flyff_db::audit::MAX_LINE_BYTES;
/// Removes the secret keys from every object of a value.
pub use flyff_db::audit::scrub;

/// Who and where an event happened (the optional context fields of 13 §25.1 that the server layer knows).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Actor {
    /// `acc`.
    pub account: Option<AccountId>,
    /// `chr`.
    pub character: Option<CharacterId>,
    /// `name` (the character name).
    pub name: Option<String>,
    /// `ip` (only for `session.*`, `account.*`, `gm.*`).
    pub ip: Option<String>,
}

impl Actor {
    /// No context.
    pub fn none() -> Actor {
        Actor::default()
    }

    /// An account and the address of its client.
    pub fn account(account: AccountId, ip: Option<String>) -> Actor {
        Actor {
            account: Some(account),
            ip,
            ..Actor::default()
        }
    }

    /// Only the address.
    pub fn ip(ip: impl ToString) -> Actor {
        Actor {
            ip: Some(ip.to_string()),
            ..Actor::default()
        }
    }
}

impl From<&Actor> for AuditActor {
    fn from(a: &Actor) -> AuditActor {
        AuditActor {
            account_id: a.account,
            character_id: a.character,
            name: a.name.clone(),
            ip: a.ip.clone(),
            ..AuditActor::default()
        }
    }
}

/// The server layer's handle on an [`AuditLog`] (see the module documentation).
#[derive(Debug)]
pub struct AuditFile {
    log: AuditLog,
    /// `[audit] admin_required` (Plan/25 §10.1): an admin mutation whose audit record is not acknowledged is refused.
    admin_required: AtomicBool,
}

impl AuditFile {
    /// Wraps the log of `flyff_db::open` (or any other).
    pub fn from_log(log: AuditLog) -> Arc<AuditFile> {
        Arc::new(AuditFile {
            log,
            admin_required: AtomicBool::new(true),
        })
    }

    /// Sets `[audit] admin_required` (default `true`).
    pub fn set_admin_required(&self, required: bool) {
        self.admin_required.store(required, Ordering::Relaxed);
    }

    /// `[audit] admin_required`: when `true`, an admin mutation whose `requested` record could not be acknowledged
    /// answers `503 audit_unavailable` and is not dispatched.
    pub fn admin_required(&self) -> bool {
        self.admin_required.load(Ordering::Relaxed)
    }

    /// Emits an `admin.*` or `gm.command` event and waits at most 1 s for the writer to write it
    /// ([`AuditLog::emit_admin`], Plan/25 §10.1). **Blocks the calling thread** while it waits: the answer comes
    /// within microseconds from a healthy writer, and a stalled one trips a breaker that fails the next calls at
    /// once.
    pub fn emit_admin(
        &self,
        ev: &'static str,
        actor: &Actor,
        data: Value,
    ) -> Result<(), AuditError> {
        self.log
            .emit_admin(AuditEvent::new(ev, data).by(AuditActor::from(actor)))
    }

    /// [`AuditFile::emit_admin`] stamped `ts_ms` (tests).
    pub fn emit_admin_at(
        &self,
        ts_ms: i64,
        ev: &'static str,
        actor: &Actor,
        data: Value,
    ) -> Result<(), AuditError> {
        self.log.emit_admin(
            AuditEvent::new(ev, data)
                .at(ts_ms)
                .by(AuditActor::from(actor)),
        )
    }

    /// Starts an own writer over `<home>/<cfg.dir>`; `run` is the process start in Unix seconds. A disabled
    /// `[audit]` section gives [`AuditFile::disabled`]. Dropping the last handle writes what is queued.
    pub fn open(home: &Path, cfg: &AuditConfig, run: i64) -> Arc<AuditFile> {
        AuditFile::from_log(AuditLog::start(cfg, home, run))
    }

    /// A log that writes nothing.
    pub fn disabled() -> Arc<AuditFile> {
        AuditFile::from_log(AuditLog::disabled())
    }

    /// A log that keeps the formatted lines in memory ([`AuditFile::lines`]); for tests.
    pub fn in_memory() -> Arc<AuditFile> {
        AuditFile::from_log(AuditLog::in_memory())
    }

    /// The underlying log (the game and the persistence layer emit through clones of it).
    pub fn log(&self) -> &AuditLog {
        &self.log
    }

    /// The counters (`flyff_audit_lines_total`, `flyff_audit_dropped_total`).
    pub fn stats(&self) -> AuditStats {
        self.log.stats()
    }

    /// The lines of an in-memory log.
    pub fn lines(&self) -> Vec<String> {
        self.log.lines()
    }

    /// The parsed events `ev` of an in-memory log (tests).
    pub fn events(&self, ev: &str) -> Vec<Value> {
        self.log.events(ev)
    }

    /// Waits until everything emitted so far is on disk, at most 5 s.
    pub fn flush(&self) -> bool {
        self.log.flush(Duration::from_secs(5))
    }

    /// Emits one event now (the wall clock of the host stamps it).
    pub fn emit(&self, ev: &'static str, actor: &Actor, data: Value) {
        self.log
            .emit(AuditEvent::new(ev, data).by(AuditActor::from(actor)));
    }

    /// Emits one event stamped `ts_ms`.
    pub fn emit_at(&self, ts_ms: i64, ev: &'static str, actor: &Actor, data: Value) {
        self.log.emit(
            AuditEvent::new(ev, data)
                .at(ts_ms)
                .by(AuditActor::from(actor)),
        );
    }
}

/// Reads every line of every `audit-*.jsonl` under `dir`, oldest file first (tests, `flyff check`). The writers
/// of this process that append to `dir` are waited for first, so a test sees the lines of what it just did.
pub fn read_lines(dir: &Path) -> Vec<String> {
    let _ = flyff_db::audit::sync_dir(dir, Duration::from_secs(5));
    flyff_db::audit::read_lines(dir)
}
