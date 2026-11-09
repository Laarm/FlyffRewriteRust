//! Gateway counters (10 §7.4, §12): inbound messages and bytes, drops per reason, kicks per reason and messages
//! per envelope tag. Lock-free; WP-OPS-18 renders them in `/metrics` and `/admin/api/netstats`.

use super::pipeline::DropReason;
use flyff_common::rt::KickReason;
use flyff_proto::{CLIENT_MSG_NAMES, ClientCase};
use std::sync::atomic::{AtomicU64, Ordering};

/// The counters of the whole gateway.
#[derive(Debug)]
pub struct GatewayStats {
    /// WebSocket sessions opened.
    pub sessions_opened: AtomicU64,
    /// Messages that passed the pipeline (`flyff_inbound_msgs_total`).
    pub inbound_msgs: AtomicU64,
    /// Bytes of those messages.
    pub inbound_bytes: AtomicU64,
    /// `ServerTimeReq` answered.
    pub server_time_replies: AtomicU64,
    /// WebSocket pings sent.
    pub pings_sent: AtomicU64,
    /// Sockets closed for a missing frame within `net.ws_idle_timeout_s`.
    pub idle_closes: AtomicU64,
    /// Upgrades refused by the `Origin` check (`flyff_ws_upgrades_rejected_total{reason="origin"}`).
    pub upgrades_rejected_origin: AtomicU64,
    /// Upgrades refused because the process is not serving (`reason="state"`).
    pub upgrades_rejected_state: AtomicU64,
    dropped: [AtomicU64; DropReason::COUNT],
    kicks: [AtomicU64; 16],
    by_tag: Box<[AtomicU64]>,
}

/// A copy of the counters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatsSnapshot {
    /// Drops per [`DropReason`] (index = value).
    pub dropped: [u64; DropReason::COUNT],
    /// Kicks per `KickReason` value.
    pub kicks: [u64; 16],
    /// `(tag, message name, count)` of every client message seen at least once.
    pub per_tag: Vec<(u32, &'static str, u64)>,
}

impl Default for GatewayStats {
    fn default() -> Self {
        Self::new()
    }
}

impl GatewayStats {
    /// Zeroed counters.
    pub fn new() -> Self {
        let max_tag = CLIENT_MSG_NAMES.iter().map(|(t, _)| *t).max().unwrap_or(0);
        let n = usize::try_from(max_tag).unwrap_or(0) + 1;
        Self {
            sessions_opened: AtomicU64::new(0),
            inbound_msgs: AtomicU64::new(0),
            inbound_bytes: AtomicU64::new(0),
            server_time_replies: AtomicU64::new(0),
            pings_sent: AtomicU64::new(0),
            idle_closes: AtomicU64::new(0),
            upgrades_rejected_origin: AtomicU64::new(0),
            upgrades_rejected_state: AtomicU64::new(0),
            dropped: std::array::from_fn(|_| AtomicU64::new(0)),
            kicks: std::array::from_fn(|_| AtomicU64::new(0)),
            by_tag: (0..n).map(|_| AtomicU64::new(0)).collect(),
        }
    }

    /// Counts a dropped (or kicked-for) message.
    pub fn record_dropped(&self, reason: DropReason) {
        if let Some(c) = self.dropped.get(reason as usize) {
            c.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Counts a kick.
    pub fn record_kick(&self, reason: KickReason) {
        if let Some(c) = self.kicks.get(usize::from(reason.as_u8())) {
            c.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Counts one accepted message of `case` with `bytes` bytes.
    pub fn record_msg(&self, case: ClientCase, bytes: usize) {
        self.inbound_msgs.fetch_add(1, Ordering::Relaxed);
        self.inbound_bytes
            .fetch_add(bytes as u64, Ordering::Relaxed);
        if let Some(c) = self
            .by_tag
            .get(usize::try_from(case.tag()).unwrap_or(usize::MAX))
        {
            c.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Drops counted so far for `reason`.
    pub fn dropped(&self, reason: DropReason) -> u64 {
        self.dropped
            .get(reason as usize)
            .map_or(0, |c| c.load(Ordering::Relaxed))
    }

    /// Kicks counted so far for `reason`.
    pub fn kicks(&self, reason: KickReason) -> u64 {
        self.kicks
            .get(usize::from(reason.as_u8()))
            .map_or(0, |c| c.load(Ordering::Relaxed))
    }

    /// Messages counted so far for `case`.
    pub fn count(&self, case: ClientCase) -> u64 {
        self.by_tag
            .get(usize::try_from(case.tag()).unwrap_or(usize::MAX))
            .map_or(0, |c| c.load(Ordering::Relaxed))
    }

    /// A copy of every counter.
    pub fn snapshot(&self) -> StatsSnapshot {
        StatsSnapshot {
            dropped: std::array::from_fn(|i| {
                self.dropped.get(i).map_or(0, |c| c.load(Ordering::Relaxed))
            }),
            kicks: std::array::from_fn(|i| {
                self.kicks.get(i).map_or(0, |c| c.load(Ordering::Relaxed))
            }),
            per_tag: CLIENT_MSG_NAMES
                .iter()
                .filter_map(|(tag, name)| {
                    let n = self
                        .by_tag
                        .get(usize::try_from(*tag).ok()?)?
                        .load(Ordering::Relaxed);
                    (n > 0).then_some((*tag, *name, n))
                })
                .collect(),
        }
    }
}
