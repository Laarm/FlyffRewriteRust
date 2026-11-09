//! `HttpTelemetry` (Plan/24 §8.10): lock-free counters of the HTTP layer, rendered into the Prometheus text of
//! `/metrics` (owned by doc 10, which calls [`HttpTelemetry::render_prometheus`]).
//!
//! `flyff_http_requests_total{family,status}`, `flyff_http_bytes_sent_total{family,encoding}`,
//! `flyff_http_ratelimited_total{class}`, `flyff_http_conn_rejected_total{reason}`,
//! `flyff_http_gen_wait_seconds` (histogram), `flyff_http_panics_total`, plus the gauge
//! `flyff_http_connections`.

use flyff_common::rt::Histogram;
use flyff_common::rt::telemetry::TICK_BUCKET_BOUNDS_NS;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};

/// Route family of a request (the `family` label).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Family {
    /// `/`, `/assets/*`, root files.
    Web,
    /// `/api/*`.
    Api,
    /// `/ws`.
    Ws,
    /// `/data/*`.
    Data,
    /// `/res/*`.
    Res,
    /// `/gen/*`.
    Gen,
    /// `/healthz`.
    Health,
    /// `/metrics`.
    Metrics,
    /// `/admin*`.
    Admin,
    /// `/dev/*`.
    Dev,
    /// Anything else (404s).
    Other,
}

impl Family {
    /// Every family, in label order.
    pub const ALL: [Family; 11] = [
        Family::Web,
        Family::Api,
        Family::Ws,
        Family::Data,
        Family::Res,
        Family::Gen,
        Family::Health,
        Family::Metrics,
        Family::Admin,
        Family::Dev,
        Family::Other,
    ];

    /// The label value.
    pub const fn name(self) -> &'static str {
        match self {
            Family::Web => "web",
            Family::Api => "api",
            Family::Ws => "ws",
            Family::Data => "data",
            Family::Res => "res",
            Family::Gen => "gen",
            Family::Health => "health",
            Family::Metrics => "metrics",
            Family::Admin => "admin",
            Family::Dev => "dev",
            Family::Other => "other",
        }
    }

    const fn index(self) -> usize {
        match self {
            Family::Web => 0,
            Family::Api => 1,
            Family::Ws => 2,
            Family::Data => 3,
            Family::Res => 4,
            Family::Gen => 5,
            Family::Health => 6,
            Family::Metrics => 7,
            Family::Admin => 8,
            Family::Dev => 9,
            Family::Other => 10,
        }
    }

    /// The family of a request path.
    pub fn of_path(path: &str) -> Family {
        let first = path.trim_start_matches('/').split('/').next().unwrap_or("");
        match first {
            "" | "assets" | "index.html" => Family::Web,
            "api" => Family::Api,
            "ws" => Family::Ws,
            "data" => Family::Data,
            "res" => Family::Res,
            "gen" => Family::Gen,
            "healthz" => Family::Health,
            "metrics" => Family::Metrics,
            "admin" => Family::Admin,
            "dev" => Family::Dev,
            // a single-segment path may be a root file of the web dist (R3)
            _ if !path.trim_start_matches('/').contains('/') => Family::Web,
            _ => Family::Other,
        }
    }
}

/// Status codes with their own counter; everything else lands in the last slot (`other`).
const STATUSES: [u16; 14] = [
    200, 204, 206, 304, 400, 401, 403, 404, 405, 413, 416, 429, 500, 503,
];
const STATUS_SLOTS: usize = STATUSES.len() + 1;

/// Rate classes (the `class` label of `flyff_http_ratelimited_total`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RateClass {
    /// Static files and `/gen` hits.
    Static,
    /// Concurrent `/gen` cache-miss waits.
    GenWait,
    /// `/api/*`.
    Api,
}

impl RateClass {
    /// The label value.
    pub const fn name(self) -> &'static str {
        match self {
            RateClass::Static => "static",
            RateClass::GenWait => "gen_wait",
            RateClass::Api => "api",
        }
    }

    const fn index(self) -> usize {
        match self {
            RateClass::Static => 0,
            RateClass::GenWait => 1,
            RateClass::Api => 2,
        }
    }
}

/// Why a connection or request was refused before routing (the `reason` label).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RejectReason {
    /// `web.max_tcp_per_ip`.
    PerIp,
    /// `web.max_tcp_total`.
    Total,
    /// `net.ip_block`.
    IpBlock,
}

impl RejectReason {
    /// The label value.
    pub const fn name(self) -> &'static str {
        match self {
            RejectReason::PerIp => "per_ip",
            RejectReason::Total => "total",
            RejectReason::IpBlock => "ip_block",
        }
    }

    const fn index(self) -> usize {
        match self {
            RejectReason::PerIp => 0,
            RejectReason::Total => 1,
            RejectReason::IpBlock => 2,
        }
    }
}

/// The counters.
#[derive(Debug)]
pub struct HttpTelemetry {
    requests: [[AtomicU64; STATUS_SLOTS]; Family::ALL.len()],
    /// `[identity, gzip]` per family.
    bytes: [[AtomicU64; 2]; Family::ALL.len()],
    ratelimited: [AtomicU64; 3],
    conn_rejected: [AtomicU64; 3],
    /// `/gen` waits (cache misses), from the permit to the answer.
    pub gen_wait: Histogram,
    panics: AtomicU64,
    connections: AtomicU64,
    ws_blocked: AtomicU64,
}

impl Default for HttpTelemetry {
    fn default() -> Self {
        Self {
            requests: std::array::from_fn(|_| std::array::from_fn(|_| AtomicU64::new(0))),
            bytes: std::array::from_fn(|_| std::array::from_fn(|_| AtomicU64::new(0))),
            ratelimited: std::array::from_fn(|_| AtomicU64::new(0)),
            conn_rejected: std::array::from_fn(|_| AtomicU64::new(0)),
            gen_wait: Histogram::default(),
            panics: AtomicU64::new(0),
            connections: AtomicU64::new(0),
            ws_blocked: AtomicU64::new(0),
        }
    }
}

impl HttpTelemetry {
    /// A shared, zeroed set of counters.
    pub fn new() -> std::sync::Arc<HttpTelemetry> {
        std::sync::Arc::new(HttpTelemetry::default())
    }

    /// Counts one answered request.
    pub fn request(&self, family: Family, status: u16, body_bytes: u64, gzip: bool) {
        let slot = STATUSES
            .iter()
            .position(|s| *s == status)
            .unwrap_or(STATUSES.len());
        if let Some(c) = self
            .requests
            .get(family.index())
            .and_then(|row| row.get(slot))
        {
            c.fetch_add(1, Ordering::Relaxed);
        }
        if let Some(c) = self
            .bytes
            .get(family.index())
            .and_then(|row| row.get(usize::from(gzip)))
        {
            c.fetch_add(body_bytes, Ordering::Relaxed);
        }
    }

    /// Requests counted for `family` and exactly `status` (0 for statuses without their own slot).
    pub fn requests_of(&self, family: Family, status: u16) -> u64 {
        STATUSES
            .iter()
            .position(|s| *s == status)
            .and_then(|slot| {
                self.requests
                    .get(family.index())
                    .and_then(|row| row.get(slot))
            })
            .map_or(0, |c| c.load(Ordering::Relaxed))
    }

    /// Bytes sent for `family` in the given encoding.
    pub fn bytes_of(&self, family: Family, gzip: bool) -> u64 {
        self.bytes
            .get(family.index())
            .and_then(|row| row.get(usize::from(gzip)))
            .map_or(0, |c| c.load(Ordering::Relaxed))
    }

    /// Counts a `429`.
    pub fn rate_limited(&self, class: RateClass) {
        if let Some(c) = self.ratelimited.get(class.index()) {
            c.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// `429`s counted for `class`.
    pub fn rate_limited_of(&self, class: RateClass) -> u64 {
        self.ratelimited
            .get(class.index())
            .map_or(0, |c| c.load(Ordering::Relaxed))
    }

    /// Counts a refused connection or request.
    pub fn conn_rejected(&self, reason: RejectReason) {
        if let Some(c) = self.conn_rejected.get(reason.index()) {
            c.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Refusals counted for `reason`.
    pub fn conn_rejected_of(&self, reason: RejectReason) -> u64 {
        self.conn_rejected
            .get(reason.index())
            .map_or(0, |c| c.load(Ordering::Relaxed))
    }

    /// Counts a `/ws` upgrade refused by `net.ip_block` (`flyff_ws_upgrades_rejected_total{reason="blocked"}`).
    pub fn ws_blocked(&self) {
        self.ws_blocked.fetch_add(1, Ordering::Relaxed);
    }

    /// `/ws` upgrades refused by `net.ip_block`.
    pub fn ws_blocked_total(&self) -> u64 {
        self.ws_blocked.load(Ordering::Relaxed)
    }

    /// Counts a handler panic caught by the stack.
    pub fn panic(&self) {
        self.panics.fetch_add(1, Ordering::Relaxed);
    }

    /// Handler panics so far.
    pub fn panics(&self) -> u64 {
        self.panics.load(Ordering::Relaxed)
    }

    /// An accepted connection (gauge up).
    pub fn connection_opened(&self) {
        self.connections.fetch_add(1, Ordering::Relaxed);
    }

    /// A closed connection (gauge down).
    pub fn connection_closed(&self) {
        let _ = self
            .connections
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |v| v.checked_sub(1));
    }

    /// Open connections now.
    pub fn connections(&self) -> u64 {
        self.connections.load(Ordering::Relaxed)
    }

    /// Appends the series in the Prometheus text exposition format.
    pub fn render_prometheus(&self, out: &mut String) {
        let _ = writeln!(out, "# TYPE flyff_http_requests_total counter");
        for f in Family::ALL {
            for (slot, status) in STATUSES.iter().enumerate() {
                let n = self
                    .requests
                    .get(f.index())
                    .and_then(|r| r.get(slot))
                    .map_or(0, |c| c.load(Ordering::Relaxed));
                if n > 0 {
                    let _ = writeln!(
                        out,
                        "flyff_http_requests_total{{family=\"{}\",status=\"{status}\"}} {n}",
                        f.name()
                    );
                }
            }
            let other = self
                .requests
                .get(f.index())
                .and_then(|r| r.get(STATUSES.len()))
                .map_or(0, |c| c.load(Ordering::Relaxed));
            if other > 0 {
                let _ = writeln!(
                    out,
                    "flyff_http_requests_total{{family=\"{}\",status=\"other\"}} {other}",
                    f.name()
                );
            }
        }
        let _ = writeln!(out, "# TYPE flyff_http_bytes_sent_total counter");
        for f in Family::ALL {
            for (i, enc) in ["identity", "gzip"].iter().enumerate() {
                let n = self
                    .bytes
                    .get(f.index())
                    .and_then(|r| r.get(i))
                    .map_or(0, |c| c.load(Ordering::Relaxed));
                if n > 0 {
                    let _ = writeln!(
                        out,
                        "flyff_http_bytes_sent_total{{family=\"{}\",encoding=\"{enc}\"}} {n}",
                        f.name()
                    );
                }
            }
        }
        let _ = writeln!(out, "# TYPE flyff_http_ratelimited_total counter");
        for c in [RateClass::Static, RateClass::GenWait, RateClass::Api] {
            let _ = writeln!(
                out,
                "flyff_http_ratelimited_total{{class=\"{}\"}} {}",
                c.name(),
                self.rate_limited_of(c)
            );
        }
        let _ = writeln!(out, "# TYPE flyff_http_conn_rejected_total counter");
        for r in [
            RejectReason::PerIp,
            RejectReason::Total,
            RejectReason::IpBlock,
        ] {
            let _ = writeln!(
                out,
                "flyff_http_conn_rejected_total{{reason=\"{}\"}} {}",
                r.name(),
                self.conn_rejected_of(r)
            );
        }
        let snap = self.gen_wait.snapshot();
        let _ = writeln!(out, "# TYPE flyff_http_gen_wait_seconds histogram");
        let mut acc = 0u64;
        for (i, count) in snap.buckets.iter().enumerate() {
            acc += count;
            match TICK_BUCKET_BOUNDS_NS.get(i) {
                Some(ns) => {
                    let _ = writeln!(
                        out,
                        "flyff_http_gen_wait_seconds_bucket{{le=\"{}\"}} {acc}",
                        *ns as f64 / 1e9
                    );
                }
                None => {
                    let _ = writeln!(
                        out,
                        "flyff_http_gen_wait_seconds_bucket{{le=\"+Inf\"}} {acc}"
                    );
                }
            }
        }
        let _ = writeln!(
            out,
            "flyff_http_gen_wait_seconds_sum {}",
            snap.sum_ns as f64 / 1e9
        );
        let _ = writeln!(out, "flyff_http_gen_wait_seconds_count {}", snap.count);
        let _ = writeln!(out, "# TYPE flyff_http_panics_total counter");
        let _ = writeln!(out, "flyff_http_panics_total {}", self.panics());
        let _ = writeln!(out, "# TYPE flyff_http_connections gauge");
        let _ = writeln!(out, "flyff_http_connections {}", self.connections());
    }
}
