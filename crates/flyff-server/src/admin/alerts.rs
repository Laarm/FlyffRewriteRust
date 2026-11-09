//! The alerts of the dashboard (Plan/25 §9.1; WP-ADMIN-25): server-side thresholds from `[admin.alerts]`, evaluated
//! every 10 s by the sampler of [`super::history`].
//!
//! | id | fires when | clears when |
//! |---|---|---|
//! | `tick_p99` | the tick p99 stays above `tick_p99_ms` for `tick_p99_for_s` (60 ms, 60 s) | it is at or below the limit at an evaluation |
//! | `db_degraded` | persistence is in retry mode | it recovered |
//! | `backup_old` | the newest backup is older than `backup_max_age_h` (26 h; no backup after 2 h of uptime counts) | a fresh backup exists |
//! | `disk_low` | the free space of `data/` is below `disk_min_gb` (2 GB) | it is above again |
//! | `audit_write` | the audit writer failed to write or to acknowledge (counters rose) | 10 minutes without a new failure |
//! | `failed_logins` | more than `failed_logins` failed remote staff logins within `failed_logins_window_min` (5 in 10 min) | the window holds fewer |
//! | `remote_plain_http` | an admin session from another host holds over plain HTTP (`admin.allow_remote`) | no such session is left |
//!
//! [`AlertEngine::evaluate`] is a pure function of the inputs and the time it is given: a test steps a fake clock
//! through minutes and hours. [`AlertEngine::active`] and [`AlertEngine::recent`] feed `GET /admin/api/alerts`, the
//! number of active alerts feeds the `alerts` pill of the status tick.

use parking_lot::Mutex;
use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};

/// Fired and cleared events kept for the Alerts tab.
const RECENT_CAP: usize = 100;
/// The audit alert stays this long after the last failure (seconds).
pub const AUDIT_HOLD_S: i64 = 600;
/// No backup at all counts as an alert once the server has been up this long (seconds).
pub const FIRST_BACKUP_GRACE_S: i64 = 7200;

/// How serious an alert is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Needs a look.
    Warn,
    /// Needs action now.
    Critical,
}

/// One active alert.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Alert {
    /// The stable id (`tick_p99`).
    pub id: &'static str,
    /// How serious.
    pub severity: Severity,
    /// A short title.
    pub title: String,
    /// What the numbers are.
    pub detail: String,
    /// Unix seconds since when it holds.
    pub since: i64,
}

/// Fired or cleared.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// The alert became active.
    Fired,
    /// The alert ended.
    Cleared,
}

/// One entry of the alert history.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AlertEvent {
    /// Unix seconds.
    pub ts: i64,
    /// The alert id.
    pub id: &'static str,
    /// Fired or cleared.
    pub kind: EventKind,
    /// The title of the alert.
    pub title: String,
    /// The detail at that moment.
    pub detail: String,
}

/// The thresholds (`[admin.alerts]`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AlertConfig {
    /// `tick_p99_ms` in µs.
    pub tick_p99_us: u32,
    /// `tick_p99_for_s`.
    pub tick_p99_for_s: u32,
    /// `backup_max_age_h` in seconds; `None` = off.
    pub backup_max_age_s: Option<i64>,
    /// `disk_min_gb` in bytes; `None` = off.
    pub disk_min_bytes: Option<u64>,
    /// `failed_logins`.
    pub failed_logins: u32,
    /// `failed_logins_window_min` in seconds.
    pub failed_logins_window_s: i64,
}

impl AlertConfig {
    /// The thresholds of a configuration.
    pub fn of(cfg: &crate::config::Config) -> AlertConfig {
        let a = &cfg.admin.alerts;
        AlertConfig {
            tick_p99_us: a.tick_p99_ms.saturating_mul(1000),
            tick_p99_for_s: a.tick_p99_for_s,
            backup_max_age_s: (a.backup_max_age_h > 0)
                .then(|| i64::from(a.backup_max_age_h) * 3600),
            disk_min_bytes: (a.disk_min_gb > 0).then(|| u64::from(a.disk_min_gb) << 30),
            failed_logins: a.failed_logins,
            failed_logins_window_s: i64::from(a.failed_logins_window_min) * 60,
        }
    }
}

impl Default for AlertConfig {
    fn default() -> Self {
        AlertConfig::of(&crate::config::Config::default())
    }
}

/// What one evaluation looks at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Inputs {
    /// Unix seconds.
    pub now_s: i64,
    /// Seconds since the process started.
    pub uptime_s: i64,
    /// Rolling tick p99 in µs.
    pub tick_p99_us: u32,
    /// Persistence in retry mode.
    pub db_degraded: bool,
    /// Unix seconds of the newest backup; `None` = none found.
    pub last_backup_s: Option<i64>,
    /// Free bytes of the data directory; `None` = unknown.
    pub disk_free: Option<u64>,
    /// Counter: failed or unacknowledged audit writes so far.
    pub audit_failures: u64,
    /// Counter: failed remote staff logins so far.
    pub remote_login_failures: u64,
    /// Admin sessions from another host that were opened over plain HTTP and still hold.
    pub plain_remote_sessions: u32,
}

/// What changed in one evaluation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Changes {
    /// Alerts that became active.
    pub fired: Vec<Alert>,
    /// Alerts that ended.
    pub cleared: Vec<Alert>,
}

#[derive(Default)]
struct State {
    active: BTreeMap<&'static str, Alert>,
    recent: VecDeque<AlertEvent>,
    above_since: Option<i64>,
    audit_last: u64,
    audit_seen_at: Option<i64>,
    failed: VecDeque<(i64, u64)>,
}

/// The alert state machine (see the module documentation).
#[derive(Default)]
pub struct AlertEngine {
    state: Mutex<State>,
    config: Mutex<AlertConfig>,
}

impl std::fmt::Debug for AlertEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AlertEngine").finish_non_exhaustive()
    }
}

impl AlertEngine {
    /// An engine with the thresholds `config`.
    pub fn new(config: AlertConfig) -> AlertEngine {
        AlertEngine {
            state: Mutex::new(State::default()),
            config: Mutex::new(config),
        }
    }

    /// The thresholds in force.
    pub fn config(&self) -> AlertConfig {
        self.config.lock().clone()
    }

    /// Replaces the thresholds (a reload of the live `[admin.alerts]` keys).
    pub fn set_config(&self, config: AlertConfig) {
        *self.config.lock() = config;
    }

    /// Evaluates every rule against `i` with the thresholds in force.
    pub fn evaluate(&self, i: &Inputs) -> Changes {
        let cfg = self.config();
        self.evaluate_with(&cfg, i)
    }

    /// Evaluates every rule against `i` with explicit thresholds.
    pub fn evaluate_with(&self, cfg: &AlertConfig, i: &Inputs) -> Changes {
        let mut st = self.state.lock();
        let mut now_active: Vec<Alert> = Vec::new();

        // tick p99 above the limit for a while
        if i.tick_p99_us > cfg.tick_p99_us {
            let since = *st.above_since.get_or_insert(i.now_s);
            if i.now_s - since >= i64::from(cfg.tick_p99_for_s) {
                now_active.push(Alert {
                    id: "tick_p99",
                    severity: Severity::Warn,
                    title: "Tick p99 too high".to_owned(),
                    detail: format!(
                        "p99 {:.1} ms, limit {:.0} ms, for {} s",
                        f64::from(i.tick_p99_us) / 1000.0,
                        f64::from(cfg.tick_p99_us) / 1000.0,
                        i.now_s - since
                    ),
                    since,
                });
            }
        } else {
            st.above_since = None;
        }

        if i.db_degraded {
            now_active.push(Alert {
                id: "db_degraded",
                severity: Severity::Critical,
                title: "Database degraded".to_owned(),
                detail: "persistence is in retry mode: writes are failing".to_owned(),
                since: i.now_s,
            });
        }

        if let Some(max_age) = cfg.backup_max_age_s {
            let detail = match i.last_backup_s {
                Some(t) if i.now_s - t > max_age => Some(format!(
                    "the newest backup is {} h old (limit {} h)",
                    (i.now_s - t) / 3600,
                    max_age / 3600
                )),
                None if i.uptime_s >= FIRST_BACKUP_GRACE_S => {
                    Some("no backup was found".to_owned())
                }
                _ => None,
            };
            if let Some(detail) = detail {
                now_active.push(Alert {
                    id: "backup_old",
                    severity: Severity::Warn,
                    title: "Backup too old".to_owned(),
                    detail,
                    since: i.now_s,
                });
            }
        }

        if let (Some(min), Some(free)) = (cfg.disk_min_bytes, i.disk_free)
            && free < min
        {
            now_active.push(Alert {
                id: "disk_low",
                severity: Severity::Critical,
                title: "Low disk space".to_owned(),
                detail: format!(
                    "{:.1} GB free in the data directory (limit {:.1} GB)",
                    gb(free),
                    gb(min)
                ),
                since: i.now_s,
            });
        }

        // audit write failures: hold the alert for ten minutes after the last one
        if i.audit_failures > st.audit_last {
            st.audit_seen_at = Some(i.now_s);
        }
        st.audit_last = i.audit_failures;
        if let Some(seen) = st.audit_seen_at {
            if i.now_s - seen < AUDIT_HOLD_S {
                now_active.push(Alert {
                    id: "audit_write",
                    severity: Severity::Critical,
                    title: "Audit write failure".to_owned(),
                    detail: format!(
                        "the audit log could not be written ({} failures so far)",
                        i.audit_failures
                    ),
                    since: seen,
                });
            } else {
                st.audit_seen_at = None;
            }
        }

        // failed remote staff logins in a window
        st.failed.push_back((i.now_s, i.remote_login_failures));
        while st.failed.len() > 2
            && st
                .failed
                .get(1)
                .is_some_and(|(t, _)| *t <= i.now_s - cfg.failed_logins_window_s)
        {
            st.failed.pop_front();
        }
        let base = st.failed.front().map_or(0, |(_, n)| *n);
        let failures = i.remote_login_failures.saturating_sub(base);
        if failures > u64::from(cfg.failed_logins) {
            now_active.push(Alert {
                id: "failed_logins",
                severity: Severity::Warn,
                title: "Failed remote staff logins".to_owned(),
                detail: format!(
                    "{failures} failed remote logins in {} min (limit {})",
                    cfg.failed_logins_window_s / 60,
                    cfg.failed_logins
                ),
                since: i.now_s,
            });
        }

        if i.plain_remote_sessions > 0 {
            now_active.push(Alert {
                id: "remote_plain_http",
                severity: Severity::Warn,
                title: "Remote admin over plain HTTP".to_owned(),
                detail: format!(
                    "{} admin session(s) from another host run over plain HTTP: the cookies are visible on the network",
                    i.plain_remote_sessions
                ),
                since: i.now_s,
            });
        }

        // diff against the active set
        let mut changes = Changes::default();
        let ids: Vec<&'static str> = now_active.iter().map(|a| a.id).collect();
        let gone: Vec<&'static str> = st
            .active
            .keys()
            .copied()
            .filter(|id| !ids.contains(id))
            .collect();
        for id in gone {
            if let Some(a) = st.active.remove(id) {
                push_event(&mut st.recent, i.now_s, &a, EventKind::Cleared);
                changes.cleared.push(a);
            }
        }
        for a in now_active {
            match st.active.get_mut(a.id) {
                Some(cur) => {
                    // the numbers move while the alert holds; `since` stays
                    cur.detail = a.detail;
                }
                None => {
                    push_event(&mut st.recent, i.now_s, &a, EventKind::Fired);
                    st.active.insert(a.id, a.clone());
                    changes.fired.push(a);
                }
            }
        }
        changes
    }

    /// The alerts that hold now, most serious first.
    pub fn active(&self) -> Vec<Alert> {
        let mut v: Vec<Alert> = self.state.lock().active.values().cloned().collect();
        v.sort_by_key(|a| (std::cmp::Reverse(a.severity == Severity::Critical), a.since));
        v
    }

    /// Fired and cleared events, newest first.
    pub fn recent(&self) -> Vec<AlertEvent> {
        self.state.lock().recent.iter().rev().cloned().collect()
    }

    /// How many alerts hold now.
    pub fn count(&self) -> u32 {
        u32::try_from(self.state.lock().active.len()).unwrap_or(u32::MAX)
    }
}

fn push_event(recent: &mut VecDeque<AlertEvent>, ts: i64, a: &Alert, kind: EventKind) {
    recent.push_back(AlertEvent {
        ts,
        id: a.id,
        kind,
        title: a.title.clone(),
        detail: a.detail.clone(),
    });
    while recent.len() > RECENT_CAP {
        recent.pop_front();
    }
}

fn gb(bytes: u64) -> f64 {
    bytes as f64 / f64::from(1_u32 << 30)
}
