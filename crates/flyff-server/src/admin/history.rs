//! Metrics history, the activity ring and the sampler of the dashboard (Plan/25 §9.1, §9.2; WP-ADMIN-25).
//!
//! * [`History`] is an in-memory ring buffer with two resolutions: a sample every 10 s for 2 h ([`FINE_CAP`] = 720)
//!   and a sample every minute for 24 h ([`COARSE_CAP`] = 1,440). A coarse sample aggregates the fine samples of its
//!   minute (peaks for the gauges that matter at their peak, means for the rest, sums for the counts). There is no
//!   external database: a restart empties it, and the charts say so. About 150 KB in total.
//! * [`ActivityRing`] keeps the last [`ACTIVITY_CAP`] `admin.*` events and notable audit events (bans, authority
//!   changes, disbands, server events). `AuditFile` taps every event that goes through `flyff_db::AuditLog`.
//! * [`spawn`] is the sampler: every 10 s it reads the status of the game, the process figures, the disk and the
//!   backups, records a [`Sample`], evaluates the alerts of [`super::alerts`] and publishes their number to the status
//!   tick of the admin WebSocket (`WsHub::set_status_extras`).
//!
//! Everything here takes the time as an argument (the sampler reads the console's `LobbyClock`), so a test steps a
//! fake clock through hours in microseconds.

use parking_lot::Mutex;
use serde::Serialize;
use std::collections::VecDeque;

/// Seconds between fine samples.
pub const FINE_STEP_S: i64 = 10;
/// Fine samples kept: 2 h.
pub const FINE_CAP: usize = 720;
/// Seconds between coarse samples.
pub const COARSE_STEP_S: i64 = 60;
/// Coarse samples kept: 24 h.
pub const COARSE_CAP: usize = 1440;
/// Events the activity ring keeps.
pub const ACTIVITY_CAP: usize = 200;

/// One point of the history.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sample {
    /// Unix seconds.
    pub t: i64,
    /// Avatars in the world.
    pub in_world: u32,
    /// Sessions that are connected but not in the world.
    pub lobby: u32,
    /// Tick p50 in µs.
    pub tick_p50_us: u32,
    /// Tick p99 in µs.
    pub tick_p99_us: u32,
    /// Persistence queue depth.
    pub db_queue: u32,
    /// Resident memory in bytes.
    pub rss_bytes: u64,
    /// Bytes per second received from clients over the interval.
    pub bytes_in_s: u64,
    /// Bytes per second sent to clients over the interval.
    pub bytes_out_s: u64,
    /// Gateway sessions (WebSockets of players).
    pub ws_sessions: u32,
    /// Admin WebSockets (console tabs).
    pub admin_ws: u32,
    /// Successful logins in the interval.
    pub logins: u32,
}

/// The two resolutions of [`History::window`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Res {
    /// 10 s samples, the last 2 h.
    #[serde(rename = "10s")]
    Fine,
    /// 1 min samples, the last 24 h.
    #[serde(rename = "1m")]
    Coarse,
}

impl Res {
    /// Parses `10s` / `1m`.
    pub fn parse(s: &str) -> Option<Res> {
        match s {
            "10s" => Some(Res::Fine),
            "1m" => Some(Res::Coarse),
            _ => None,
        }
    }

    /// Seconds between samples.
    pub fn step_s(self) -> i64 {
        match self {
            Res::Fine => FINE_STEP_S,
            Res::Coarse => COARSE_STEP_S,
        }
    }

    /// The resolution that covers `span_s` seconds: fine up to its 2 h, coarse beyond.
    pub fn for_span(span_s: i64) -> Res {
        if span_s <= FINE_STEP_S * FINE_CAP as i64 {
            Res::Fine
        } else {
            Res::Coarse
        }
    }
}

/// The samples of one resolution as columns (what `GET /admin/api/history` returns).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Columns {
    /// Unix seconds of each point.
    pub t: Vec<i64>,
    /// `in_world`.
    pub in_world: Vec<u32>,
    /// `lobby`.
    pub lobby: Vec<u32>,
    /// `tick_p50_us`.
    pub tick_p50_us: Vec<u32>,
    /// `tick_p99_us`.
    pub tick_p99_us: Vec<u32>,
    /// `db_queue`.
    pub db_queue: Vec<u32>,
    /// `rss_bytes`.
    pub rss_bytes: Vec<u64>,
    /// `bytes_in_s`.
    pub bytes_in_s: Vec<u64>,
    /// `bytes_out_s`.
    pub bytes_out_s: Vec<u64>,
    /// `ws_sessions`.
    pub ws_sessions: Vec<u32>,
    /// `admin_ws`.
    pub admin_ws: Vec<u32>,
    /// `logins`.
    pub logins: Vec<u32>,
}

impl Columns {
    fn push(&mut self, s: &Sample) {
        self.t.push(s.t);
        self.in_world.push(s.in_world);
        self.lobby.push(s.lobby);
        self.tick_p50_us.push(s.tick_p50_us);
        self.tick_p99_us.push(s.tick_p99_us);
        self.db_queue.push(s.db_queue);
        self.rss_bytes.push(s.rss_bytes);
        self.bytes_in_s.push(s.bytes_in_s);
        self.bytes_out_s.push(s.bytes_out_s);
        self.ws_sessions.push(s.ws_sessions);
        self.admin_ws.push(s.admin_ws);
        self.logins.push(s.logins);
    }

    /// Points in the window.
    pub fn len(&self) -> usize {
        self.t.len()
    }

    /// `true` when there is no point.
    pub fn is_empty(&self) -> bool {
        self.t.is_empty()
    }
}

/// The aggregate of the fine samples of one minute.
#[derive(Clone, Copy, Debug, Default)]
struct Bucket {
    minute: i64,
    n: u32,
    sum_p50: u64,
    sum_in: u64,
    sum_out: u64,
    peak: Sample,
}

impl Bucket {
    fn add(&mut self, s: &Sample) {
        if self.n == 0 {
            self.peak = *s;
            self.peak.logins = 0;
        }
        self.n += 1;
        self.sum_p50 += u64::from(s.tick_p50_us);
        self.sum_in += s.bytes_in_s;
        self.sum_out += s.bytes_out_s;
        let p = &mut self.peak;
        p.in_world = p.in_world.max(s.in_world);
        p.lobby = p.lobby.max(s.lobby);
        p.tick_p99_us = p.tick_p99_us.max(s.tick_p99_us);
        p.db_queue = p.db_queue.max(s.db_queue);
        p.rss_bytes = p.rss_bytes.max(s.rss_bytes);
        p.ws_sessions = p.ws_sessions.max(s.ws_sessions);
        p.admin_ws = p.admin_ws.max(s.admin_ws);
        p.logins = p.logins.saturating_add(s.logins);
    }

    fn finish(&self) -> Sample {
        let n = u64::from(self.n.max(1));
        let mut s = self.peak;
        s.t = self.minute * COARSE_STEP_S;
        s.tick_p50_us = u32::try_from(self.sum_p50 / n).unwrap_or(u32::MAX);
        s.bytes_in_s = self.sum_in / n;
        s.bytes_out_s = self.sum_out / n;
        s
    }
}

#[derive(Default)]
struct Rings {
    fine: VecDeque<Sample>,
    coarse: VecDeque<Sample>,
    open: Option<Bucket>,
}

/// The two-resolution ring buffer (see the module documentation).
#[derive(Default)]
pub struct History {
    rings: Mutex<Rings>,
}

impl std::fmt::Debug for History {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("History").finish_non_exhaustive()
    }
}

impl History {
    /// An empty history.
    pub fn new() -> History {
        History::default()
    }

    /// Records one fine sample (its `t` in Unix seconds, normally every 10 s). A sample of a new minute closes the
    /// coarse sample of the previous one.
    pub fn record(&self, s: Sample) {
        let mut g = self.rings.lock();
        let minute = s.t.div_euclid(COARSE_STEP_S);
        if g.open.as_ref().is_some_and(|b| b.minute != minute)
            && let Some(done) = g.open.take()
        {
            g.coarse.push_back(done.finish());
            while g.coarse.len() > COARSE_CAP {
                g.coarse.pop_front();
            }
        }
        g.open
            .get_or_insert_with(|| Bucket {
                minute,
                ..Bucket::default()
            })
            .add(&s);
        g.fine.push_back(s);
        while g.fine.len() > FINE_CAP {
            g.fine.pop_front();
        }
    }

    /// The samples of `res` that are not older than `since` (Unix seconds; `None` = everything). The minute that
    /// is still open is the last point of the coarse resolution.
    pub fn window(&self, res: Res, since: Option<i64>) -> Columns {
        let g = self.rings.lock();
        let mut out = Columns::default();
        let keep = |s: &Sample| since.is_none_or(|t| s.t >= t);
        match res {
            Res::Fine => g.fine.iter().filter(|s| keep(s)).for_each(|s| out.push(s)),
            Res::Coarse => {
                g.coarse
                    .iter()
                    .filter(|s| keep(s))
                    .for_each(|s| out.push(s));
                if let Some(b) = &g.open {
                    let s = b.finish();
                    if keep(&s) {
                        out.push(&s);
                    }
                }
            }
        }
        out
    }

    /// Points kept per resolution.
    pub fn counts(&self) -> (usize, usize) {
        let g = self.rings.lock();
        (g.fine.len(), g.coarse.len() + usize::from(g.open.is_some()))
    }

    /// The newest fine sample.
    pub fn latest(&self) -> Option<Sample> {
        self.rings.lock().fine.back().copied()
    }

    /// The highest number of avatars in the world since `since` (Unix seconds), over the coarse points and the open
    /// minute: the "peak" card.
    pub fn peak_in_world(&self, since: i64) -> u32 {
        let g = self.rings.lock();
        g.coarse
            .iter()
            .filter(|s| s.t >= since)
            .map(|s| s.in_world)
            .chain(g.open.iter().map(|b| b.peak.in_world))
            .chain(g.fine.iter().filter(|s| s.t >= since).map(|s| s.in_world))
            .max()
            .unwrap_or(0)
    }

    /// Logins since `since` (Unix seconds), from the coarse points and the open minute.
    pub fn logins_since(&self, since: i64) -> u64 {
        let g = self.rings.lock();
        g.coarse
            .iter()
            .filter(|s| s.t >= since)
            .map(|s| u64::from(s.logins))
            .chain(g.open.iter().map(|b| u64::from(b.peak.logins)))
            .sum()
    }
}

// ---- the activity ring ----------------------------------------------------------------------------------------------

/// One entry of the activity feed.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Activity {
    /// Unix milliseconds of the event.
    pub ts_ms: i64,
    /// The event name (`admin.mute`).
    pub ev: String,
    /// Who did it (`staff:joe`), when the record names one.
    pub actor: Option<String>,
    /// `requested`, `done`, `failed` (admin events), else absent.
    pub state: Option<String>,
    /// A one-line description.
    pub summary: String,
    /// The reason, when one was given.
    pub reason: Option<String>,
    /// `true` for `admin.*` and `gm.command`, `false` for the notable events of the rest of the server.
    pub admin: bool,
}

/// Events outside `admin.*` that the activity feed shows.
pub const NOTABLE: &[&str] = &[
    "account.ban",
    "account.unban",
    "account.authority",
    "account.password",
    "account.create",
    "guild.disband",
    "char.delete",
    "char.undelete",
    "char.rename",
    "server.start",
    "server.stop",
    "server.shutdown_requested",
    "server.config_reload",
    "server.panic",
    "persist.failure",
    "persist.degraded",
    "persist.poisoned",
];

/// `true` when an event of this name enters the activity ring.
pub fn is_activity(ev: &str) -> bool {
    flyff_db::audit::is_non_excludable(ev) || NOTABLE.contains(&ev)
}

/// The recent admin and notable audit events, in memory.
#[derive(Default)]
pub struct ActivityRing {
    items: Mutex<VecDeque<Activity>>,
}

impl std::fmt::Debug for ActivityRing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActivityRing").finish_non_exhaustive()
    }
}

impl ActivityRing {
    /// An empty ring.
    pub fn new() -> ActivityRing {
        ActivityRing::default()
    }

    /// Notes an audit event if it belongs in the feed. `requested` records of admin events are not shown (the
    /// `done` / `failed` record follows).
    pub fn note(&self, ev: &str, ts_ms: i64, data: &serde_json::Value) {
        if !is_activity(ev) {
            return;
        }
        let state = data
            .get("state")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        if state.as_deref() == Some("requested") {
            return;
        }
        let text = |k: &str| {
            data.get(k)
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        };
        let entry = Activity {
            ts_ms,
            ev: ev.to_owned(),
            actor: text("actor").or_else(|| text("by")),
            state,
            summary: super::audit_index::summarize(ev, data),
            reason: text("reason"),
            admin: flyff_db::audit::is_non_excludable(ev),
        };
        let mut g = self.items.lock();
        g.push_back(entry);
        while g.len() > ACTIVITY_CAP {
            g.pop_front();
        }
    }

    /// The newest `limit` entries, newest first; `admin_only` drops the notable events of the rest of the server.
    pub fn recent(&self, limit: usize, admin_only: bool) -> Vec<Activity> {
        self.items
            .lock()
            .iter()
            .rev()
            .filter(|a| !admin_only || a.admin)
            .take(limit)
            .cloned()
            .collect()
    }

    /// Entries held.
    pub fn len(&self) -> usize {
        self.items.lock().len()
    }

    /// `true` when nothing was noted.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// ---- the dashboard state --------------------------------------------------------------------------------------------

/// The latest probe of the cards that are not time series (`current` of `GET /admin/api/history`).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Current {
    /// Unix seconds of the probe.
    pub sampled_at: i64,
    /// Unix seconds of the process start.
    pub started_unix: i64,
    /// Resident memory.
    pub rss_bytes: u64,
    /// Free bytes of the data directory.
    pub disk_free_bytes: Option<u64>,
    /// Bytes in the derived-asset cache.
    pub cache_bytes: u64,
    /// `assets.disk_budget_mb` in bytes.
    pub cache_budget_bytes: u64,
    /// Unix seconds of the newest backup file.
    pub last_backup_unix: Option<i64>,
    /// Backup files found.
    pub backups: u32,
    /// Size of the write-ahead log.
    pub db_wal_bytes: u64,
    /// Checkpoints done by this process.
    pub db_checkpoints: u64,
    /// Peak avatars in the world since the process start (at most 24 h).
    pub peak_in_world: u32,
    /// Successful logins in the last hour.
    pub logins_last_hour: u64,
    /// `auth.maintenance`.
    pub maintenance: bool,
}

/// The outcome of the last verification of a backup file (the backups page; lost on restart).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Verified {
    /// Unix seconds of the check.
    pub at: i64,
    /// `integrity_check` passed and the file is a flyff database this binary understands.
    pub ok: bool,
    /// The summary of the database, or why it failed.
    pub detail: String,
}

/// Everything the dashboard keeps in memory (`AdminShared.dash`).
#[derive(Debug)]
pub struct Dash {
    /// The metrics history.
    pub history: History,
    /// The alert state machine.
    pub alerts: super::alerts::AlertEngine,
    /// The activity feed.
    pub activity: ActivityRing,
    /// The last verification of each backup file by name.
    pub verified: Mutex<std::collections::BTreeMap<String, Verified>>,
    current: Mutex<Current>,
}

impl Dash {
    /// An empty dashboard with the alert thresholds `alerts`.
    pub fn new(alerts: super::alerts::AlertConfig) -> Dash {
        Dash {
            history: History::new(),
            alerts: super::alerts::AlertEngine::new(alerts),
            activity: ActivityRing::new(),
            verified: Mutex::new(std::collections::BTreeMap::new()),
            current: Mutex::new(Current::default()),
        }
    }

    /// The latest probe.
    pub fn current(&self) -> Current {
        self.current.lock().clone()
    }

    /// Stores a probe.
    pub fn set_current(&self, c: Current) {
        *self.current.lock() = c;
    }
}

// ---- the sampler ----------------------------------------------------------------------------------------------------

/// Seconds between samples and alert evaluations.
pub const SAMPLE_EVERY: std::time::Duration = std::time::Duration::from_secs(FINE_STEP_S as u64);
/// The cache directory is measured at most this often (seconds).
const CACHE_PROBE_EVERY_S: i64 = 60;

/// What one sampling step carries from the previous one (counter deltas).
#[derive(Debug, Default)]
pub struct Prev {
    at_s: i64,
    bytes_in: u64,
    bytes_out: u64,
    logins: u64,
    cache_probe_s: i64,
    cache_bytes: u64,
}

/// One sampling step: reads the game's status, the process, the disk and the backups, records a [`Sample`], stores
/// the probe for the cards and evaluates the alerts. `now_s` is the clock of the console.
pub async fn sample_once(ops: &super::ops::AdminOps, prev: &mut Prev, now_s: i64) {
    use std::sync::atomic::Ordering;
    let t = &ops.telemetry;
    let status = ops.status_snapshot().await.ok();
    let (in_world, lobby, p50, p99, db_queue, db_degraded, uptime_s) = match &status {
        Some(s) => (
            s.in_world,
            s.lobby,
            s.tick_p50_us,
            s.tick_p99_us,
            s.db_queue,
            s.db_degraded,
            i64::try_from(s.uptime_s).unwrap_or(0),
        ),
        None => (
            u32::try_from(t.players_in_world.load(Ordering::Relaxed)).unwrap_or(u32::MAX),
            0,
            u32::try_from(t.tick_p50_ns.load(Ordering::Relaxed) / 1000).unwrap_or(u32::MAX),
            u32::try_from(t.tick_p99_ns.load(Ordering::Relaxed) / 1000).unwrap_or(u32::MAX),
            u32::try_from(ops.persist.stats().queue_depth).unwrap_or(u32::MAX),
            ops.persist.stats().degraded,
            i64::try_from(ops.started.elapsed().as_secs()).unwrap_or(0),
        ),
    };
    let (cfg, paths) = {
        let cfg = ops.current.lock().clone();
        let paths = cfg.resolve_paths(&ops.reload_source.home);
        (cfg, paths)
    };
    let proc_ = crate::metrics::ProcStats::read();
    let bytes_in = t.inbound_bytes.load(Ordering::Relaxed);
    let bytes_out = t.out_bytes.load(Ordering::Relaxed);
    let logins = ops
        .core
        .stats
        .logins()
        .into_iter()
        .find(|(label, _)| *label == "ok")
        .map_or(0, |(_, n)| n);
    let dt = (now_s - prev.at_s).clamp(1, 3600).unsigned_abs();
    let first = prev.at_s == 0;
    let rate = |now: u64, before: u64| {
        if first {
            0
        } else {
            now.saturating_sub(before) / dt
        }
    };
    let new_logins = if first {
        0
    } else {
        logins.saturating_sub(prev.logins)
    };
    let shared = &ops.admin;
    shared.dash.history.record(Sample {
        t: now_s,
        in_world,
        lobby,
        tick_p50_us: p50,
        tick_p99_us: p99,
        db_queue,
        rss_bytes: proc_.rss_bytes,
        bytes_in_s: rate(bytes_in, prev.bytes_in),
        bytes_out_s: rate(bytes_out, prev.bytes_out),
        ws_sessions: u32::try_from(t.sessions.load(Ordering::Relaxed)).unwrap_or(u32::MAX),
        admin_ws: u32::try_from(shared.metrics.snapshot().ws_connections).unwrap_or(u32::MAX),
        logins: u32::try_from(new_logins).unwrap_or(u32::MAX),
    });
    prev.at_s = now_s;
    prev.bytes_in = bytes_in;
    prev.bytes_out = bytes_out;
    prev.logins = logins;
    // the cache directory is walked by the asset layer's own accounting: no directory walk here
    if now_s - prev.cache_probe_s >= CACHE_PROBE_EVERY_S {
        prev.cache_probe_s = now_s;
        prev.cache_bytes = ops
            .http
            .assets
            .prep
            .telemetry_snapshot()
            .cache_bytes
            .iter()
            .sum();
    }
    let backups = flyff_db::backup::list_backups(&paths.backups).unwrap_or_default();
    let last_backup = backups.iter().map(|b| b.at.0).max();
    let disk_free = flyff_db::backup::free_bytes(&paths.data);
    let db = ops.persist.stats();
    let started_unix = now_s - uptime_s;
    shared.dash.set_current(Current {
        sampled_at: now_s,
        started_unix,
        rss_bytes: proc_.rss_bytes,
        disk_free_bytes: disk_free,
        cache_bytes: prev.cache_bytes,
        cache_budget_bytes: u64::from(cfg.assets.disk_budget_mb) << 20,
        last_backup_unix: last_backup,
        backups: u32::try_from(backups.len()).unwrap_or(u32::MAX),
        db_wal_bytes: db.wal_bytes,
        db_checkpoints: db.checkpoints,
        peak_in_world: shared
            .dash
            .history
            .peak_in_world(started_unix.max(now_s - 86_400)),
        logins_last_hour: shared.dash.history.logins_since(now_s - 3600),
        maintenance: ops.core.settings().auth.maintenance,
    });
    let audit = ops.audit.stats();
    let changes = shared.dash.alerts.evaluate(&super::alerts::Inputs {
        now_s,
        uptime_s,
        tick_p99_us: p99,
        db_degraded,
        last_backup_s: last_backup,
        disk_free,
        audit_failures: audit.write_errors + audit.ack_failures,
        remote_login_failures: shared.metrics.login_failures()[1],
        plain_remote_sessions: u32::try_from(shared.sessions.plain_remote_sessions())
            .unwrap_or(u32::MAX),
    });
    for a in &changes.fired {
        tracing::warn!(alert = a.id, "admin alert: {} ({})", a.title, a.detail);
    }
    for a in &changes.cleared {
        tracing::info!(alert = a.id, "admin alert cleared: {}", a.title);
    }
}

/// Starts the sampler on `rt` and publishes the alert count and the maintenance flag to the status tick.
pub fn spawn(rt: &tokio::runtime::Handle, ops: std::sync::Arc<super::ops::AdminOps>) {
    // `[audit] admin_required`, and every `admin.*` / notable audit event into the activity feed
    ops.audit
        .set_admin_required(ops.current.lock().audit.admin_required);
    {
        let dash = std::sync::Arc::clone(&ops.admin.dash);
        ops.audit.log().set_tap(std::sync::Arc::new(move |e| {
            dash.activity.note(e.ev, e.ts_ms, &e.data);
        }));
    }
    {
        let dash_ops = std::sync::Arc::clone(&ops);
        ops.admin.ws.set_status_extras(std::sync::Arc::new(move || {
            (
                dash_ops.admin.dash.alerts.count(),
                dash_ops.core.settings().auth.maintenance,
            )
        }));
    }
    rt.spawn(async move {
        let mut tick = tokio::time::interval(SAMPLE_EVERY);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut prev = Prev::default();
        loop {
            tick.tick().await;
            let now_s = ops.admin.clock.unix().0;
            sample_once(&ops, &mut prev, now_s).await;
        }
    });
}
