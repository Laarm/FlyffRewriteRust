//! The admin console's metrics (Plan/25 §11.9; WP-ADMIN-02 registers them, the owning work packages increment them):
//!
//! | metric | kind | incremented by |
//! |---|---|---|
//! | `flyff_admin_sessions{kind}` | gauge | live from [`AdminSessions`](super::session::AdminSessions) (`staff`, `owner_token`, `owner_local`) |
//! | `flyff_admin_ws_connections` | gauge | the WebSocket gateway (WP-ADMIN-05): [`AdminMetrics::ws_opened`] / [`AdminMetrics::ws_closed`] |
//! | `flyff_admin_map_subscriptions{tier}` | gauge | the map subscriptions (WP-ADMIN-05/07): [`AdminMetrics::map_subscribed`] / [`AdminMetrics::map_unsubscribed`] |
//! | `flyff_admin_map_visits_total` | counter | the game thread's collector: `Telemetry::admin_map_visits` |
//! | `flyff_admin_views_seconds` | histogram | the game thread's `post` phase of the admin views per tick with a connection (sub-millisecond buckets; Plan/25 §3.17, WP-ADMIN-33): `Telemetry::admin_views_hist` |
//! | `flyff_admin_map_frames_total{tier}` | counter | the map encoder: [`AdminMetrics::map_frame`] |
//! | `flyff_admin_map_frame_bytes` | histogram | the map encoder: [`AdminMetrics::map_frame`] |
//! | `flyff_admin_actions_total{action,result}` | counter | the action routes: [`AdminMetrics::action`] |
//! | `flyff_admin_login_failures_total{remote}` | counter | the login route: [`AdminMetrics::login_failed`] |
//! | `flyff_admin_heavy_reads_running` | gauge | live from `PersistHandle::admin_heavy_scan_running()` |
//!
//! `/metrics` also projects the admin counters of `DbStatsSnapshot` (`flyff_db_admin_*`). The text is rendered by
//! `crate::metrics` from [`AdminMetrics::snapshot`].

use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// The map tiers of 25 §3.7 (A near, B middle, C far).
pub const TIERS: [&str; 3] = ["a", "b", "c"];
/// Upper bounds (bytes) of `flyff_admin_map_frame_bytes`; one more bucket is `+Inf`.
pub const FRAME_BYTES_BOUNDS: [u64; 10] = [
    256, 1024, 4096, 16_384, 32_768, 65_536, 131_072, 262_144, 524_288, 1_048_576,
];

/// The counters of the admin console that live outside the session table.
#[derive(Debug, Default)]
pub struct AdminMetrics {
    ws: AtomicU64,
    map_subs: [AtomicU64; 3],
    map_frames: [AtomicU64; 3],
    frame_buckets: [AtomicU64; FRAME_BYTES_BOUNDS.len() + 1],
    frame_sum: AtomicU64,
    frame_count: AtomicU64,
    login_failures: [AtomicU64; 2],
    actions: Mutex<BTreeMap<(String, String), u64>>,
}

/// A copy of the counters for the exposition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdminSnapshot {
    /// Open admin WebSockets.
    pub ws_connections: u64,
    /// Map subscriptions per tier.
    pub map_subscriptions: [u64; 3],
    /// Map frames sent per tier.
    pub map_frames: [u64; 3],
    /// Non-cumulative bucket counts of the frame sizes.
    pub frame_buckets: [u64; FRAME_BYTES_BOUNDS.len() + 1],
    /// Sum of the frame sizes.
    pub frame_bytes_sum: u64,
    /// Number of frames.
    pub frame_count: u64,
    /// Failed logins: `[local, remote]`.
    pub login_failures: [u64; 2],
    /// Admin actions by `(action, result)`.
    pub actions: Vec<((String, String), u64)>,
}

fn dec(c: &AtomicU64) {
    let _ = c.try_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
        Some(v.saturating_sub(1))
    });
}

impl AdminMetrics {
    /// An admin WebSocket opened.
    pub fn ws_opened(&self) {
        self.ws.fetch_add(1, Ordering::Relaxed);
    }

    /// An admin WebSocket closed.
    pub fn ws_closed(&self) {
        dec(&self.ws);
    }

    /// A map subscription of `tier` (0 = A, 1 = B, 2 = C) started.
    pub fn map_subscribed(&self, tier: usize) {
        if let Some(c) = self.map_subs.get(tier) {
            c.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// A map subscription of `tier` ended.
    pub fn map_unsubscribed(&self, tier: usize) {
        if let Some(c) = self.map_subs.get(tier) {
            dec(c);
        }
    }

    /// A map frame of `bytes` was sent for a subscription of `tier`.
    pub fn map_frame(&self, tier: usize, bytes: u64) {
        if let Some(c) = self.map_frames.get(tier) {
            c.fetch_add(1, Ordering::Relaxed);
        }
        let idx = FRAME_BYTES_BOUNDS
            .iter()
            .position(|b| bytes <= *b)
            .unwrap_or(FRAME_BYTES_BOUNDS.len());
        if let Some(b) = self.frame_buckets.get(idx) {
            b.fetch_add(1, Ordering::Relaxed);
        }
        self.frame_sum.fetch_add(bytes, Ordering::Relaxed);
        self.frame_count.fetch_add(1, Ordering::Relaxed);
    }

    /// A login (staff or token exchange) was refused; `remote` = the request did not come from the host itself
    /// (the dashboard alert of 25 §9.1 counts the remote ones).
    pub fn login_failed(&self, remote: bool) {
        if let Some(c) = self.login_failures.get(usize::from(remote)) {
            c.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Failed logins so far: `[local, remote]`.
    pub fn login_failures(&self) -> [u64; 2] {
        [
            self.login_failures[0].load(Ordering::Relaxed),
            self.login_failures[1].load(Ordering::Relaxed),
        ]
    }

    /// An admin action finished with `result` (`ok`, or the error code).
    pub fn action(&self, action: &str, result: &str) {
        let mut m = self.actions.lock();
        // the label set is small and fixed by the routes; the cap guards against a bug that labels by input
        if m.len() < 1024 || m.contains_key(&(action.to_owned(), result.to_owned())) {
            *m.entry((action.to_owned(), result.to_owned())).or_insert(0) += 1;
        }
    }

    /// Copies the counters.
    pub fn snapshot(&self) -> AdminSnapshot {
        let load = |c: &AtomicU64| c.load(Ordering::Relaxed);
        AdminSnapshot {
            ws_connections: load(&self.ws),
            map_subscriptions: std::array::from_fn(|i| self.map_subs.get(i).map_or(0, load)),
            map_frames: std::array::from_fn(|i| self.map_frames.get(i).map_or(0, load)),
            frame_buckets: std::array::from_fn(|i| self.frame_buckets.get(i).map_or(0, load)),
            frame_bytes_sum: load(&self.frame_sum),
            frame_count: load(&self.frame_count),
            login_failures: std::array::from_fn(|i| self.login_failures.get(i).map_or(0, load)),
            actions: self
                .actions
                .lock()
                .iter()
                .map(|(k, v)| (k.clone(), *v))
                .collect(),
        }
    }
}
