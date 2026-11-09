//! The audit JSONL log (Plan/13 §25, WP-DB-09): append-only event lines that replace the legacy `LOGGING_01_DBF`
//! tables and carry the GM and economy trail.
//!
//! ```text
//! {"ts":1760443200123,"seq":1842,"run":1760440000,"ev":"trade.complete","acc":12,"chr":345,"name":"Alice",
//!  "world":1,"layer":0,"pos":[6971.9,100.0,3336.8],"data":{"with_chr":346,...}}
//! ```
//!
//! * [`AuditLog`] is the cheap-clone emitter. [`AuditLog::emit`] never blocks: the event goes into a bounded queue
//!   (`[audit] queue_capacity`); when the queue is full the event is dropped and counted
//!   ([`AuditStats::dropped`], `flyff_audit_dropped_total`).
//! * One writer thread `flyff-audit` numbers the lines (`seq`, strictly increasing within a run), formats them,
//!   scrubs secrets (`password`, `pwhash`, `token`, `pin` never reach a line), appends to
//!   `<dir>/audit-YYYY-MM-DD.jsonl` (UTC date of the event, so the file rotates at UTC midnight) and deletes files
//!   older than `[audit] retention_days` at start and at every rotation.
//! * Lines are buffered whole and written with **one** `write` call on an `O_APPEND` file as soon as the queue is
//!   empty (and at least once per second, or every 64 KiB under load), so an offline CLI process may append to the
//!   same file without tearing a line. There is no `fsync` per line.
//! * Event names are the `&'static str` constants of [`ev`]; adding an event means adding a constant and a row to
//!   the catalog of Plan/13 §25.2 ([`ev::CATALOG`] mirrors it and a test keeps the constants consistent).
//!
//! * **Admin events are reliable** (Plan/25 §10.1, 13 §25 addendum): [`AuditLog::emit_admin`] waits at most
//!   [`ADMIN_ACK_TIMEOUT`] (1 s) until the writer has written the line to the file and answers
//!   [`AuditError`] otherwise; the events `admin.*` and `gm.command` ([`NON_EXCLUDABLE`]) are never filtered by
//!   `[audit] exclude`. With `[audit] admin_required = true` the HTTP layer refuses to dispatch an admin mutation
//!   whose `requested` record could not be acknowledged (`503 audit_unavailable`).
//! * The per-day index files of the admin audit viewer live in `<dir>/index/YYYY-MM-DD.idx` ([`index_path`]); the
//!   writer deletes them together with their JSONL file at retention.
//!
//! `AuditLog::default()` is a disabled log (tests, `[audit] enabled = false`).

use crate::calendar;
use crate::config::AuditConfig;
use crate::model::ItemRecord;
use flyff_common::rt::reply_pair;
use flyff_common::{AccountId, CharacterId};
use serde_json::Value;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::thread::JoinHandle;
use std::time::Duration;

/// The longest line, newline excluded (Plan/13 §25.1: ≤ 64 KiB).
pub const MAX_LINE_BYTES: usize = 64 * 1024;

/// Keys that never reach a line (Plan/13 §25.1 "Secrets are never logged").
const SECRET_KEYS: [&str; 4] = ["password", "pwhash", "token", "pin"];

/// The writer flushes at least this often even when events keep arriving.
const FLUSH_INTERVAL: Duration = Duration::from_secs(1);

/// The writer flushes when this many bytes are buffered.
const FLUSH_BYTES: usize = 64 * 1024;

/// How long [`AuditLog::emit_admin`] waits for the writer's acknowledgement (Plan/25 §10.1).
pub const ADMIN_ACK_TIMEOUT: Duration = Duration::from_secs(1);

/// After an unacknowledged admin event further ones fail at once for this long (a stalled writer must not block a
/// worker thread for a second per request).
pub const STALL_BACKOFF: Duration = Duration::from_secs(3);

/// Events that `[audit] exclude` can never filter out (Plan/13 §25 addendum): everything under `admin.` and
/// `gm.command`.
pub const NON_EXCLUDABLE: &[&str] = &["admin.", "gm.command"];

/// The sub-directory of the audit directory that holds the per-day index files of the admin audit viewer.
pub const INDEX_DIR: &str = "index";

/// `<audit dir>/index/YYYY-MM-DD.idx`, the index of the JSONL file of `date` (`YYYY-MM-DD`).
pub fn index_path(dir: &Path, date: &str) -> PathBuf {
    dir.join(INDEX_DIR).join(format!("{date}.idx"))
}

/// `true` when `ev` can not be excluded by `[audit] exclude`.
pub fn is_non_excludable(ev: &str) -> bool {
    NON_EXCLUDABLE.iter().any(|p| {
        if p.ends_with('.') {
            ev.starts_with(p)
        } else {
            ev == *p
        }
    })
}

pub mod content;
pub mod econ;
pub mod social;

/// Event names (Plan/13 §25.2). Families that the plan lists with a wildcard (`secret_room.*`, `housing.*`, ...)
/// have a `*_PREFIX` constant; their individual names are added by the owning WP together with the emitter.
pub mod ev {
    macro_rules! events {
        ($($(#[$m:meta])* $name:ident = $text:literal;)*) => {
            $($(#[$m])* #[doc = concat!("`", $text, "`")] pub const $name: &str = $text;)*
            /// Every explicit event name above, in catalog order.
            pub const CATALOG: &[&str] = &[$($text),*];
        };
    }

    events! {
        // ---- runtime (Plan/10 §11) ----
        SERVER_START = "server.start";
        SERVER_STOP = "server.stop";
        SERVER_CCU = "server.ccu";
        SERVER_SHUTDOWN_REQUESTED = "server.shutdown_requested";
        SERVER_CONFIG_RELOAD = "server.config_reload";
        SERVER_PANIC = "server.panic";
        // ---- sessions and accounts (Plan/12 §19.1) ----
        SESSION_LOGIN = "session.login";
        SESSION_LOGOUT = "session.logout";
        SESSION_TAKEOVER = "session.takeover";
        SESSION_REATTACH = "session.reattach";
        ACCOUNT_LOGIN = "account.login";
        ACCOUNT_LOGIN_FAILED = "account.login_failed";
        ACCOUNT_CREATE = "account.create";
        ACCOUNT_BAN = "account.ban";
        ACCOUNT_UNBAN = "account.unban";
        ACCOUNT_AUTHORITY = "account.authority";
        ACCOUNT_PASSWORD = "account.password";
        // ---- characters ----
        CHAR_CREATE = "char.create";
        CHAR_DELETE = "char.delete";
        CHAR_RENAME = "char.rename";
        CHAR_SEAL = "char.seal";
        CHAR_UNSEAL = "char.unseal";
        CHAR_PURGE = "char.purge";
        CHAR_UNDELETE = "char.undelete";
        CHAR_LEVEL_UP = "char.level_up";
        CHAR_JOB_CHANGE = "char.job_change";
        CHAR_DEATH = "char.death";
        CHAR_PK = "char.pk";
        // ---- items and economy ----
        ITEM_CREATE = "item.create";
        ITEM_DESTROY = "item.destroy";
        ITEM_DROP = "item.drop";
        ITEM_PICKUP = "item.pickup";
        ITEM_UPGRADE = "item.upgrade";
        ITEM_UNIQUE = "item.unique";
        ITEM_CREATE_MONSTER = "item.create_monster";
        SHOP_BUY = "shop.buy";
        SHOP_SELL = "shop.sell";
        SHOP_QUICK_SELL = "shop.quick_sell";
        TRADE_COMPLETE = "trade.complete";
        VENDOR_SALE = "vendor.sale";
        MAIL_SEND = "mail.send";
        MAIL_TAKE_ITEM = "mail.take_item";
        MAIL_TAKE_GOLD = "mail.take_gold";
        MAIL_DELETE = "mail.delete";
        MAIL_EXPIRE = "mail.expire";
        BANK_PUT = "bank.put";
        BANK_TAKE = "bank.take";
        POCKET_MOVE = "pocket.move";
        GRANT_DELIVER = "grant.deliver";
        GRANT_SKIP_EXPIRED = "grant.skip_expired";
        REMOVAL_APPLY = "removal.apply";
        // ---- guilds ----
        GUILD_CREATE = "guild.create";
        GUILD_DISBAND = "guild.disband";
        GUILD_RENAME = "guild.rename";
        GUILD_MEMBER_JOIN = "guild.member_join";
        GUILD_MEMBER_LEAVE = "guild.member_leave";
        GUILD_MEMBER_KICK = "guild.member_kick";
        GUILD_RANK = "guild.rank";
        GUILD_MASTER = "guild.master";
        GUILD_BANK = "guild.bank";
        GUILD_CONTRIBUTION = "guild.contribution";
        GUILD_SALARY = "guild.salary";
        GUILD_WAR_START = "guild.war_start";
        GUILD_WAR_END = "guild.war_end";
        // ---- sieges, realm, events ----
        SIEGE_BID = "siege.bid";
        SIEGE_CANCEL = "siege.cancel";
        SIEGE_RESULT = "siege.result";
        SIEGE_CLAIM = "siege.claim";
        SIEGE_CHIPS = "siege.chips";
        MINIGAME_BET = "minigame.bet";
        SECRET_ROOM_TENDER = "secret_room.tender";
        SECRET_ROOM_CANCEL = "secret_room.cancel";
        SECRET_ROOM_RESULT = "secret_room.result";
        GUILD_HOUSE_BUY = "guild_house.buy";
        GUILD_HOUSE_UPKEEP = "guild_house.upkeep";
        GUILD_HOUSE_FURNITURE_ADD = "guild_house.furniture_add";
        HOUSING_FURNITURE_ADD = "housing.furniture_add";
        RAINBOW_APPLY = "rainbow.apply";
        RAINBOW_FINISH = "rainbow.finish";
        RAINBOW_RESULT = "rainbow.result";
        QUIZ_IMPORT = "quiz.import";
        LORD_DEPOSIT = "lord.deposit";
        LORD_PLEDGE = "lord.pledge";
        LORD_VOTE = "lord.vote";
        LORD_ELECTED = "lord.elected";
        LORD_EVENT = "lord.event";
        LORD_SKILL = "lord.skill";
        TAX_PAYOUT = "tax.payout";
        TAX_RATE = "tax.rate";
        QUIZ_OPEN = "quiz.open";
        QUIZ_WINNER = "quiz.winner";
        // ---- progress ----
        QUEST_START = "quest.start";
        QUEST_COMPLETE = "quest.complete";
        QUEST_REMOVE = "quest.remove";
        SKILL_POINTS_GAIN = "skill.points_gain";
        SKILL_POINTS_SPEND = "skill.points_spend";
        PET_HATCH = "pet.hatch";
        PET_LEVEL_UP = "pet.level_up";
        PET_FEED = "pet.feed";
        PET_DEATH = "pet.death";
        HONOR_GAIN = "honor.gain";
        EXP_BOX_CREATE = "exp_box.create";
        EXP_BOX_TAKE = "exp_box.take";
        INSTANCE_CREATE = "instance.create";
        INSTANCE_DESTROY = "instance.destroy";
        // ---- GM ----
        GM_COMMAND = "gm.command";
        GM_CHAT = "gm.chat";
        GM_SNOOP = "gm.snoop";
        // ---- the admin console (Plan/25 §10.3; never excludable) ----
        ADMIN_SESSION = "admin.session";
        ADMIN_TELEPORT = "admin.teleport";
        ADMIN_HEAL = "admin.heal";
        ADMIN_KILL = "admin.kill";
        ADMIN_DESPAWN = "admin.despawn";
        ADMIN_DESPAWN_AREA = "admin.despawn_area";
        ADMIN_SPAWN = "admin.spawn";
        ADMIN_RESPAWN = "admin.respawn";
        ADMIN_KICK = "admin.kick";
        ADMIN_MUTE = "admin.mute";
        ADMIN_MESSAGE = "admin.message";
        ADMIN_GRANT = "admin.grant";
        ADMIN_GRANT_CANCEL = "admin.grant_cancel";
        ADMIN_REMOVAL_CANCEL = "admin.removal_cancel";
        ADMIN_REMOVAL = "admin.removal";
        ADMIN_GOLD = "admin.gold";
        ADMIN_CHARACTER_EDIT = "admin.character_edit";
        ADMIN_GUILD = "admin.guild";
        ADMIN_CONFIG = "admin.config";
        ADMIN_IP_BLOCK = "admin.ip_block";
        ADMIN_BOOKMARK = "admin.bookmark";
        ADMIN_TEMPLATE = "admin.template";
        ADMIN_NOTE = "admin.note";
        ADMIN_NOTICE = "admin.notice";
        ADMIN_JOB = "admin.job";
        ADMIN_BACKUP = "admin.backup";
        ADMIN_MAINTENANCE = "admin.maintenance";
        // ---- reserved: no carrier in v1, never emitted (Plan/13 §25.2) ----
        CLIENT_ERROR = "client.error";
        // ---- persistence ----
        PERSIST_REPAIR = "persist.repair";
        PERSIST_FAILURE = "persist.failure";
        PERSIST_POISONED = "persist.poisoned";
        PERSIST_DEGRADED = "persist.degraded";
    }

    /// Families whose members the plan lists with a wildcard (`gc1to1.*`, `secret_room.*`, `rainbow.*`,
    /// `housing.*`, `guild_house.*`, `campus.*`, `couple.*`): an event of the owning WP starts with one of these.
    pub const FAMILY_PREFIXES: &[&str] = &[
        "gc1to1.",
        "secret_room.",
        "rainbow.",
        "housing.",
        "guild_house.",
        "campus.",
        "couple.",
    ];

    /// `true` when `name` is in the catalog (an explicit constant or a member of a wildcard family).
    pub fn is_known(name: &str) -> bool {
        CATALOG.contains(&name) || FAMILY_PREFIXES.iter().any(|p| name.starts_with(p))
    }
}

/// Who did it and where (the optional context fields of Plan/13 §25.1).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AuditActor {
    /// `acc`.
    pub account_id: Option<AccountId>,
    /// `chr`.
    pub character_id: Option<CharacterId>,
    /// `name` (the character name).
    pub name: Option<String>,
    /// `ip`; only on `session.*`, `account.*` and `gm.*`.
    pub ip: Option<String>,
    /// `world`.
    pub world: Option<u32>,
    /// `layer`.
    pub layer: Option<i32>,
    /// `pos`.
    pub pos: Option<[f32; 3]>,
}

impl AuditActor {
    /// An account and the address of its client.
    pub fn account(account: AccountId, ip: Option<String>) -> AuditActor {
        AuditActor {
            account_id: Some(account),
            ip,
            ..AuditActor::default()
        }
    }

    /// Only an address.
    pub fn ip(ip: impl ToString) -> AuditActor {
        AuditActor {
            ip: Some(ip.to_string()),
            ..AuditActor::default()
        }
    }
}

/// One audit event (Plan/13 §25.1).
#[derive(Clone, Debug, PartialEq)]
pub struct AuditEvent {
    /// Event name, a constant of [`ev`].
    pub ev: &'static str,
    /// Who and where, when known.
    pub actor: Option<AuditActor>,
    /// Event-specific object.
    pub data: Value,
    /// Unix milliseconds at emit time (audit output only; never read by game logic).
    pub ts_ms: i64,
}

/// The wall clock in Unix milliseconds (audit output only).
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

impl AuditEvent {
    /// An event stamped now, without actor context.
    pub fn new(ev: &'static str, data: Value) -> AuditEvent {
        AuditEvent {
            ev,
            actor: None,
            data,
            ts_ms: now_ms(),
        }
    }

    /// Attaches the actor context.
    #[must_use]
    pub fn by(mut self, actor: AuditActor) -> AuditEvent {
        self.actor = Some(actor);
        self
    }

    /// Replaces the timestamp (tests and replays).
    #[must_use]
    pub fn at(mut self, ts_ms: i64) -> AuditEvent {
        self.ts_ms = ts_ms;
        self
    }

    /// An item as the audit `data` shows it: the `ItemRecord` JSON without `obj_id` (the storage index is a
    /// per-container detail that means nothing in a log).
    pub fn item(item: &ItemRecord) -> Value {
        let mut value = serde_json::to_value(item).unwrap_or(Value::Null);
        if let Value::Object(map) = &mut value {
            map.remove("obj_id");
        }
        value
    }
}

/// Counters of one audit log (`flyff_audit_lines_total`, `flyff_audit_dropped_total`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AuditStats {
    /// Lines written (excluded events do not count).
    pub written: u64,
    /// Events lost: the queue was full, the line was larger than [`MAX_LINE_BYTES`] or the write failed.
    pub dropped: u64,
    /// Events waiting in the queue.
    pub queued: usize,
    /// Failed writes of the writer thread (a full disk, a missing directory): the dashboard alert "audit write
    /// failure" (Plan/25 §9.1) watches this counter.
    pub write_errors: u64,
    /// Admin events whose acknowledgement failed or timed out ([`AuditLog::emit_admin`]).
    pub ack_failures: u64,
}

/// Why an admin audit event could not be acknowledged ([`AuditLog::emit_admin`], Plan/25 §10.1). The HTTP layer
/// answers `503 audit_unavailable` and does not dispatch the mutation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AuditError {
    /// The bounded queue is full: the writer does not keep up.
    #[error("the audit queue is full")]
    QueueFull,
    /// The writer thread did not acknowledge within [`ADMIN_ACK_TIMEOUT`].
    #[error("the audit writer did not acknowledge in time")]
    Stalled,
    /// The writer could not write the line (a full disk, a missing directory).
    #[error("the audit line could not be written")]
    WriteFailed,
    /// The line is larger than [`MAX_LINE_BYTES`].
    #[error("the audit line is too large")]
    TooLarge,
    /// The writer thread has ended.
    #[error("the audit writer has ended")]
    Closed,
}

/// The pending acknowledgement of one admin event ([`AuditLog::emit_admin_pending`]).
#[derive(Debug)]
pub struct AuditAck(Option<flyff_common::rt::ReplyRx<Result<(), AuditError>>>);

impl AuditAck {
    /// Waits at most `timeout` for the writer.
    pub fn wait(self, timeout: Duration) -> Result<(), AuditError> {
        let Some(rx) = self.0 else {
            return Ok(());
        };
        match rx.recv_timeout(timeout) {
            Ok(r) => r,
            Err(flyff_common::rt::reply::ReplyError::Disconnected) => Err(AuditError::Closed),
            Err(_) => Err(AuditError::Stalled),
        }
    }

    /// Awaits the writer without a deadline: the caller wraps it in its runtime's timeout.
    pub async fn recv_async(self) -> Result<(), AuditError> {
        let Some(rx) = self.0 else {
            return Ok(());
        };
        match rx.recv_async().await {
            Ok(r) => r,
            Err(_) => Err(AuditError::Closed),
        }
    }
}

#[derive(Default)]
struct Counters {
    written: AtomicU64,
    dropped: AtomicU64,
    write_errors: AtomicU64,
    ack_failures: AtomicU64,
}

enum Msg {
    Event(AuditEvent),
    /// An admin event: answered once its line is in the file (or why it is not).
    EventAck(AuditEvent, flyff_common::rt::Reply<Result<(), AuditError>>),
    /// Answered once everything queued before it is written to the file.
    Sync(flyff_common::rt::Reply<()>),
}

enum Sink {
    /// Queue to the `flyff-audit` writer thread.
    Queue {
        tx: Option<flume::Sender<Msg>>,
        thread: Option<JoinHandle<()>>,
    },
    /// A queue nobody reads: the writer is stalled (tests of the `503 audit_unavailable` path).
    Stalled {
        tx: flume::Sender<Msg>,
        _rx: flume::Receiver<Msg>,
    },
    /// Format at emit time and keep the lines (unit tests).
    Memory {
        lines: Mutex<Vec<String>>,
        seq: AtomicU64,
        run: i64,
    },
}

struct Inner {
    sink: Sink,
    exclude: Vec<String>,
    counters: Arc<Counters>,
    /// The start of the log: the zero of `breaker_until_ms`.
    epoch: std::time::Instant,
    /// Admin events fail at once (no wait) until this many milliseconds after `epoch`: the circuit breaker that a
    /// stalled writer trips, so a flood of admin requests does not block a worker for a second each.
    breaker_until_ms: AtomicU64,
    /// Sees every event that passes the exclusion filter (the admin console's activity feed).
    tap: std::sync::RwLock<Option<AuditTap>>,
}

/// A function that sees the events of an audit log before they are queued (see [`AuditLog::set_tap`]).
pub type AuditTap = Arc<dyn Fn(&AuditEvent) + Send + Sync>;

impl Inner {
    fn new(sink: Sink, exclude: Vec<String>, counters: Arc<Counters>) -> Inner {
        Inner {
            sink,
            exclude,
            counters,
            epoch: std::time::Instant::now(),
            breaker_until_ms: AtomicU64::new(0),
            tap: std::sync::RwLock::new(None),
        }
    }

    fn tapped(&self, event: &AuditEvent) {
        if let Ok(tap) = self.tap.read()
            && let Some(f) = tap.as_ref()
        {
            f(event);
        }
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.epoch.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    fn breaker_open(&self) -> bool {
        self.now_ms() < self.breaker_until_ms.load(Ordering::Relaxed)
    }

    fn trip_breaker(&self) {
        self.breaker_until_ms.store(
            self.now_ms()
                .saturating_add(u64::try_from(STALL_BACKOFF.as_millis()).unwrap_or(0)),
            Ordering::Relaxed,
        );
    }
}

impl Drop for Inner {
    /// The last clone is gone: the writer drains what is queued, flushes and ends.
    fn drop(&mut self) {
        if let Sink::Queue { tx, thread, .. } = &mut self.sink {
            drop(tx.take());
            if let Some(t) = thread.take() {
                let _ = t.join();
            }
            // this log's strong count is already 0: only the other logs of the process stay registered
            if let Ok(mut reg) = registry().lock() {
                reg.retain(|(_, w)| w.strong_count() > 0);
            }
        }
    }
}

/// The open file-backed logs of this process by directory.
type Registry = Mutex<Vec<(PathBuf, Weak<Inner>)>>;

/// Open file-backed logs of this process, so tests and diagnostics can wait for a directory's writer.
fn registry() -> &'static Registry {
    static REGISTRY: std::sync::OnceLock<Registry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Vec::new()))
}

/// Waits until every audit log of this process that writes into `dir` has written what was emitted before the
/// call (`true` when there was nothing to wait for or all answered within `timeout`). Readers of the audit files
/// in tests call it before reading, because the writer is asynchronous.
pub fn sync_dir(dir: &Path, timeout: Duration) -> bool {
    let logs: Vec<Arc<Inner>> = registry()
        .lock()
        .map(|reg| {
            reg.iter()
                .filter(|(d, _)| d == dir)
                .filter_map(|(_, w)| w.upgrade())
                .collect()
        })
        .unwrap_or_default();
    logs.iter().all(|inner| inner.sync(timeout))
}

impl Inner {
    fn excluded(&self, ev: &str) -> bool {
        if is_non_excludable(ev) {
            return false;
        }
        self.exclude.iter().any(|p| {
            ev == p
                || ev
                    .strip_prefix(p.as_str())
                    .is_some_and(|rest| p.ends_with('.') || rest.starts_with('.'))
        })
    }

    fn sync(&self, timeout: Duration) -> bool {
        if matches!(self.sink, Sink::Stalled { .. }) {
            return false;
        }
        let Sink::Queue { tx: Some(tx), .. } = &self.sink else {
            return true;
        };
        let (reply, rx) = reply_pair();
        if tx.send_timeout(Msg::Sync(reply), timeout).is_err() {
            return false;
        }
        rx.recv_timeout(timeout).is_ok()
    }
}

/// The cheap-clone, non-blocking emitter (Plan/13 §25.1). The default value is a disabled log.
#[derive(Clone, Default)]
pub struct AuditLog {
    inner: Option<Arc<Inner>>,
}

impl std::fmt::Debug for AuditLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuditLog")
            .field("enabled", &self.inner.is_some())
            .finish()
    }
}

impl AuditLog {
    /// A log that discards everything (`[audit] enabled = false`, tests).
    pub fn disabled() -> AuditLog {
        AuditLog { inner: None }
    }

    /// `true` unless the log is disabled.
    pub fn is_enabled(&self) -> bool {
        self.inner.is_some()
    }

    /// Starts the writer thread for `cfg` (directory relative to `base_dir` unless absolute). `run` is the
    /// process start in Unix seconds. A disabled `[audit]` section, or a writer thread that cannot be spawned,
    /// gives a disabled log.
    pub fn start(cfg: &AuditConfig, base_dir: &Path, run: i64) -> AuditLog {
        if !cfg.enabled {
            return AuditLog::disabled();
        }
        let dir = if cfg.dir.is_absolute() {
            cfg.dir.clone()
        } else {
            base_dir.join(&cfg.dir)
        };
        let capacity = usize::try_from(cfg.queue_capacity.max(1)).unwrap_or(usize::MAX);
        let (tx, rx) = flume::bounded::<Msg>(capacity);
        let counters = Arc::new(Counters::default());
        let writer = Writer {
            dir: dir.clone(),
            retention_days: cfg.retention_days,
            run,
            seq: 0,
            day: i64::MIN,
            file: None,
            buf: Vec::new(),
            buf_lines: 0,
            counters: Arc::clone(&counters),
            last_warning: None,
        };
        let thread = match std::thread::Builder::new()
            .name("flyff-audit".to_owned())
            .spawn(move || writer.run(&rx))
        {
            Ok(t) => t,
            Err(e) => {
                tracing::error!(error = %e, "cannot start the audit writer thread: audit is disabled");
                return AuditLog::disabled();
            }
        };
        let inner = Arc::new(Inner::new(
            Sink::Queue {
                tx: Some(tx),
                thread: Some(thread),
            },
            cfg.exclude.clone(),
            counters,
        ));
        if let Ok(mut reg) = registry().lock() {
            reg.retain(|(_, w)| w.strong_count() > 0);
            reg.push((dir, Arc::downgrade(&inner)));
        }
        AuditLog { inner: Some(inner) }
    }

    /// A log that formats at emit time and keeps the lines in memory ([`AuditLog::lines`]); for unit tests.
    pub fn in_memory() -> AuditLog {
        AuditLog::in_memory_with(Vec::new())
    }

    /// [`AuditLog::in_memory`] with exclude prefixes.
    pub fn in_memory_with(exclude: Vec<String>) -> AuditLog {
        AuditLog {
            inner: Some(Arc::new(Inner::new(
                Sink::Memory {
                    lines: Mutex::new(Vec::new()),
                    seq: AtomicU64::new(0),
                    run: 0,
                },
                exclude,
                Arc::new(Counters::default()),
            ))),
        }
    }

    /// Emits one event. Never blocks; a full queue drops the event and counts it.
    pub fn emit(&self, event: AuditEvent) {
        let Some(inner) = &self.inner else {
            return;
        };
        if inner.excluded(event.ev) {
            return;
        }
        inner.tapped(&event);
        match &inner.sink {
            Sink::Queue { tx, .. } => {
                let sent = tx
                    .as_ref()
                    .is_some_and(|tx| tx.try_send(Msg::Event(event)).is_ok());
                if !sent {
                    inner.counters.dropped.fetch_add(1, Ordering::Relaxed);
                }
            }
            Sink::Stalled { tx, .. } => {
                if tx.try_send(Msg::Event(event)).is_err() {
                    inner.counters.dropped.fetch_add(1, Ordering::Relaxed);
                }
            }
            Sink::Memory { lines, seq, run } => {
                let n = seq.fetch_add(1, Ordering::Relaxed) + 1;
                match format_line(n, *run, event) {
                    Some(line) => {
                        if let Ok(mut l) = lines.lock() {
                            l.push(line);
                        }
                        inner.counters.written.fetch_add(1, Ordering::Relaxed);
                    }
                    None => {
                        inner.counters.dropped.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }
    }

    /// Emits an admin event and waits at most [`ADMIN_ACK_TIMEOUT`] until the writer has written its line
    /// (Plan/25 §10.1). `admin.*` and `gm.command` are never filtered by `[audit] exclude`. A disabled log has
    /// nothing to acknowledge and answers `Ok`. **Blocks the calling thread**: async callers use
    /// [`AuditLog::emit_admin_pending`] and await [`AuditAck::recv_async`] under their own timeout.
    pub fn emit_admin(&self, event: AuditEvent) -> Result<(), AuditError> {
        let result = self.emit_admin_pending(event)?.wait(ADMIN_ACK_TIMEOUT);
        self.note_ack_result(&result);
        result
    }

    /// Feeds the outcome of an acknowledgement wait back (the circuit breaker, the failure counter): a timeout
    /// trips the breaker for [`STALL_BACKOFF`], a success closes it. [`AuditLog::emit_admin`] calls it; callers
    /// that wait on [`AuditAck`] themselves (async code with its own timeout) call it with the result.
    pub fn note_ack_result(&self, result: &Result<(), AuditError>) {
        let Some(inner) = &self.inner else {
            return;
        };
        match result {
            Err(AuditError::Stalled) => {
                inner.counters.ack_failures.fetch_add(1, Ordering::Relaxed);
                inner.trip_breaker();
            }
            Ok(()) => inner.breaker_until_ms.store(0, Ordering::Relaxed),
            Err(_) => {}
        }
    }

    /// Queues an admin event and returns the pending acknowledgement. `Err` at once when the queue is full or the
    /// writer has ended (nothing was queued).
    pub fn emit_admin_pending(&self, event: AuditEvent) -> Result<AuditAck, AuditError> {
        let Some(inner) = &self.inner else {
            return Ok(AuditAck(None));
        };
        let fail = |e: AuditError| {
            inner.counters.ack_failures.fetch_add(1, Ordering::Relaxed);
            inner.counters.dropped.fetch_add(1, Ordering::Relaxed);
            Err(e)
        };
        inner.tapped(&event);
        if inner.breaker_open() {
            // the writer did not answer a moment ago: refuse at once instead of waiting again
            return fail(AuditError::Stalled);
        }
        let (reply, rx) = reply_pair();
        match &inner.sink {
            Sink::Queue { tx, .. } => {
                let Some(tx) = tx else {
                    return fail(AuditError::Closed);
                };
                match tx.try_send(Msg::EventAck(event, reply)) {
                    Ok(()) => Ok(AuditAck(Some(rx))),
                    Err(flume::TrySendError::Full(_)) => fail(AuditError::QueueFull),
                    Err(flume::TrySendError::Disconnected(_)) => fail(AuditError::Closed),
                }
            }
            Sink::Stalled { tx, .. } => match tx.try_send(Msg::EventAck(event, reply)) {
                Ok(()) => Ok(AuditAck(Some(rx))),
                Err(_) => fail(AuditError::QueueFull),
            },
            Sink::Memory { lines, seq, run } => {
                let n = seq.fetch_add(1, Ordering::Relaxed) + 1;
                match format_line(n, *run, event) {
                    Some(line) => {
                        if let Ok(mut l) = lines.lock() {
                            l.push(line);
                        }
                        inner.counters.written.fetch_add(1, Ordering::Relaxed);
                        Ok(AuditAck(None))
                    }
                    None => fail(AuditError::TooLarge),
                }
            }
        }
    }

    /// Installs the tap: `f` sees every event that passes the exclusion filter, on the emitting thread, before it is
    /// queued (so it must be quick and must not block). One tap per log; the clones of the log share it.
    pub fn set_tap(&self, f: AuditTap) {
        if let Some(inner) = &self.inner
            && let Ok(mut tap) = inner.tap.write()
        {
            *tap = Some(f);
        }
    }

    /// A log whose writer never answers: events queue up and are never written, so every
    /// [`AuditLog::emit_admin`] times out. For the tests of the `503 audit_unavailable` path.
    pub fn stalled() -> AuditLog {
        let (tx, rx) = flume::bounded::<Msg>(1024);
        AuditLog {
            inner: Some(Arc::new(Inner::new(
                Sink::Stalled { tx, _rx: rx },
                Vec::new(),
                Arc::new(Counters::default()),
            ))),
        }
    }

    /// Emits a record in the flat shape of the line itself: `{acc, chr, name, ip, world, layer, pos, data}`
    /// (the shape the chat and GM code builds, Plan/23 §3.9). Missing members stay absent.
    pub fn emit_record(&self, ev: &'static str, record: &Value) {
        if self.inner.is_none() {
            return;
        }
        let Value::Object(map) = record else {
            self.emit(AuditEvent::new(ev, record.clone()));
            return;
        };
        let uint = |k: &str| {
            map.get(k)
                .and_then(Value::as_u64)
                .and_then(|v| u32::try_from(v).ok())
        };
        let pos = map.get("pos").and_then(Value::as_array).and_then(|a| {
            let f = |i: usize| a.get(i).and_then(Value::as_f64).map(|v| v as f32);
            Some([f(0)?, f(1)?, f(2)?])
        });
        let actor = AuditActor {
            account_id: uint("acc").map(AccountId),
            character_id: uint("chr").map(CharacterId),
            name: map.get("name").and_then(Value::as_str).map(str::to_owned),
            ip: map.get("ip").and_then(Value::as_str).map(str::to_owned),
            world: uint("world"),
            layer: map
                .get("layer")
                .and_then(Value::as_i64)
                .and_then(|v| i32::try_from(v).ok()),
            pos,
        };
        let data = map
            .get("data")
            .cloned()
            .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
        let has_actor = actor != AuditActor::default();
        let mut event = AuditEvent::new(ev, data);
        if has_actor {
            event = event.by(actor);
        }
        self.emit(event);
    }

    /// Waits until everything emitted before the call is written to the file, at most `timeout`. `true` for a
    /// disabled or in-memory log.
    pub fn flush(&self, timeout: Duration) -> bool {
        self.inner.as_ref().is_none_or(|i| i.sync(timeout))
    }

    /// The counters of this log.
    pub fn stats(&self) -> AuditStats {
        let Some(inner) = &self.inner else {
            return AuditStats::default();
        };
        AuditStats {
            written: inner.counters.written.load(Ordering::Relaxed),
            dropped: inner.counters.dropped.load(Ordering::Relaxed),
            queued: match &inner.sink {
                Sink::Queue { tx, .. } => tx.as_ref().map_or(0, flume::Sender::len),
                Sink::Stalled { tx, .. } => tx.len(),
                Sink::Memory { .. } => 0,
            },
            write_errors: inner.counters.write_errors.load(Ordering::Relaxed),
            ack_failures: inner.counters.ack_failures.load(Ordering::Relaxed),
        }
    }

    /// The lines of an in-memory log.
    pub fn lines(&self) -> Vec<String> {
        match self.inner.as_deref().map(|i| &i.sink) {
            Some(Sink::Memory { lines, .. }) => lines.lock().map(|l| l.clone()).unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// The parsed events named `ev` of an in-memory log.
    pub fn events(&self, ev: &str) -> Vec<Value> {
        self.lines()
            .iter()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .filter(|v| v.get("ev").and_then(Value::as_str) == Some(ev))
            .collect()
    }
}

/// Removes the secret keys from every object of `v`.
pub fn scrub(v: &mut Value) {
    match v {
        Value::Object(m) => {
            m.retain(|k, _| !SECRET_KEYS.contains(&k.to_ascii_lowercase().as_str()));
            for x in m.values_mut() {
                scrub(x);
            }
        }
        Value::Array(a) => a.iter_mut().for_each(scrub),
        _ => {}
    }
}

/// Formats one line without its newline, in the field order of Plan/13 §25.1 (`ts`, `seq`, `run`, `ev`, the
/// actor context, `data`). `None` when the line would exceed [`MAX_LINE_BYTES`].
fn format_line(seq: u64, run: i64, mut event: AuditEvent) -> Option<String> {
    use std::fmt::Write as _;
    scrub(&mut event.data);
    let quote = |s: &str| serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_owned());
    let mut text = String::with_capacity(160);
    let _ = write!(
        text,
        "{{\"ts\":{},\"seq\":{seq},\"run\":{run},\"ev\":{}",
        event.ts_ms,
        quote(event.ev)
    );
    if let Some(a) = &event.actor {
        if let Some(v) = a.account_id {
            let _ = write!(text, ",\"acc\":{}", v.0);
        }
        if let Some(v) = a.character_id {
            let _ = write!(text, ",\"chr\":{}", v.0);
        }
        if let Some(v) = &a.name {
            let _ = write!(text, ",\"name\":{}", quote(v));
        }
        if let Some(v) = &a.ip {
            let _ = write!(text, ",\"ip\":{}", quote(v));
        }
        if let Some(v) = a.world {
            let _ = write!(text, ",\"world\":{v}");
        }
        if let Some(v) = a.layer {
            let _ = write!(text, ",\"layer\":{v}");
        }
        if let Some(v) = &a.pos {
            let _ = write!(
                text,
                ",\"pos\":{}",
                serde_json::to_string(v).unwrap_or_else(|_| "null".to_owned())
            );
        }
    }
    text.push_str(",\"data\":");
    text.push_str(&serde_json::to_string(&event.data).ok()?);
    text.push('}');
    (text.len() <= MAX_LINE_BYTES).then_some(text)
}

/// The writer thread's state.
struct Writer {
    dir: PathBuf,
    retention_days: u32,
    run: i64,
    seq: u64,
    /// UTC day index of the open file (`i64::MIN` before the first line).
    day: i64,
    file: Option<File>,
    /// Complete lines (each with its newline) not yet written.
    buf: Vec<u8>,
    buf_lines: u64,
    counters: Arc<Counters>,
    last_warning: Option<std::time::Instant>,
}

impl Writer {
    fn run(mut self, rx: &flume::Receiver<Msg>) {
        loop {
            match rx.recv_timeout(FLUSH_INTERVAL) {
                Ok(msg) => {
                    self.handle(msg);
                    while let Ok(next) = rx.try_recv() {
                        self.handle(next);
                        if self.buf.len() >= FLUSH_BYTES {
                            self.flush();
                        }
                    }
                    self.flush();
                }
                Err(flume::RecvTimeoutError::Timeout) => {
                    self.flush();
                }
                Err(flume::RecvTimeoutError::Disconnected) => {
                    self.flush();
                    return;
                }
            }
        }
    }

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Sync(reply) => {
                self.flush();
                let _ = reply.send(());
            }
            Msg::Event(event) => {
                self.append(event);
            }
            Msg::EventAck(event, reply) => {
                // an admin event is on disk before it is acknowledged
                let queued = self.append(event);
                let ok = self.flush();
                let answer = match (queued, ok) {
                    (false, _) => Err(AuditError::TooLarge),
                    (true, false) => Err(AuditError::WriteFailed),
                    (true, true) => Ok(()),
                };
                if answer.is_err() {
                    self.counters.ack_failures.fetch_add(1, Ordering::Relaxed);
                }
                let _ = reply.send(answer);
            }
        }
    }

    /// Formats one event into the buffer. `false` when the line was too large (dropped and counted).
    fn append(&mut self, event: AuditEvent) -> bool {
        // The file follows the UTC date of the event; a late event of an earlier day goes to the open file.
        let day = calendar::utc_day_of_ms(event.ts_ms).max(self.day);
        if day != self.day {
            self.flush();
            self.file = None;
            self.day = day;
            self.sweep_retention();
        }
        self.seq += 1;
        match format_line(self.seq, self.run, event) {
            Some(line) => {
                self.buf.extend_from_slice(line.as_bytes());
                self.buf.push(b'\n');
                self.buf_lines += 1;
                true
            }
            None => {
                self.counters.dropped.fetch_add(1, Ordering::Relaxed);
                false
            }
        }
    }

    /// Writes the buffered lines with one `write` call. `false` when the write failed (the lines are dropped).
    fn flush(&mut self) -> bool {
        if self.buf.is_empty() {
            return true;
        }
        let lines = self.buf_lines;
        let ok = match self.write_buffer() {
            Ok(()) => {
                self.counters.written.fetch_add(lines, Ordering::Relaxed);
                true
            }
            Err(e) => {
                self.counters.dropped.fetch_add(lines, Ordering::Relaxed);
                self.counters.write_errors.fetch_add(1, Ordering::Relaxed);
                self.file = None;
                // one warning per 10 s: a full disk must not flood the log
                let now = std::time::Instant::now();
                if self
                    .last_warning
                    .is_none_or(|t| now.duration_since(t) >= Duration::from_secs(10))
                {
                    self.last_warning = Some(now);
                    tracing::warn!(error = %e, lines, dir = %self.dir.display(), "audit lines not written");
                }
                false
            }
        };
        self.buf.clear();
        self.buf_lines = 0;
        ok
    }

    fn write_buffer(&mut self) -> std::io::Result<()> {
        if self.file.is_none() {
            std::fs::create_dir_all(&self.dir)?;
            let mut options = OpenOptions::new();
            options.create(true).append(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o640);
            }
            let path = self
                .dir
                .join(format!("audit-{}.jsonl", calendar::date_string(self.day)));
            self.file = Some(options.open(path)?);
        }
        match self.file.as_mut() {
            Some(f) => f.write_all(&self.buf),
            None => Err(std::io::Error::other("no audit file")),
        }
    }

    /// Deletes `audit-*.jsonl` files older than `retention_days` before the current day, and the index files of
    /// the admin audit viewer (`index/YYYY-MM-DD.idx`) of the same days (an index without its JSONL file too).
    fn sweep_retention(&self) {
        let Some(oldest_kept) = self.day.checked_sub(i64::from(self.retention_days)) else {
            return;
        };
        sweep_dir(&self.dir, oldest_kept);
    }
}

/// Deletes the `audit-DATE.jsonl` files of `dir` and the `index/DATE.idx` files that are older than the UTC day
/// `oldest_kept` (days since the epoch). Used by the writer at start and at every rotation.
pub fn sweep_dir(dir: &Path, oldest_kept: i64) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name();
            let Some(date) = name
                .to_str()
                .and_then(|n| n.strip_prefix("audit-"))
                .and_then(|n| n.strip_suffix(".jsonl"))
                .and_then(calendar::parse_date)
            else {
                continue;
            };
            if date < oldest_kept && std::fs::remove_file(entry.path()).is_ok() {
                tracing::info!(file = ?name, "audit file past retention deleted");
            }
        }
    }
    if let Ok(entries) = std::fs::read_dir(dir.join(INDEX_DIR)) {
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name();
            let Some(date) = name
                .to_str()
                .and_then(|n| n.strip_suffix(".idx"))
                .and_then(calendar::parse_date)
            else {
                continue;
            };
            if date < oldest_kept && std::fs::remove_file(entry.path()).is_ok() {
                tracing::info!(file = ?name, "audit index past retention deleted");
            }
        }
    }
}

/// Reads every line of every `audit-*.jsonl` under `dir`, oldest file first (tests, `flyff check`).
pub fn read_lines(dir: &Path) -> Vec<String> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("audit-") && n.ends_with(".jsonl"))
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .flat_map(|t| t.lines().map(str::to_owned).collect::<Vec<_>>())
        .collect()
}
