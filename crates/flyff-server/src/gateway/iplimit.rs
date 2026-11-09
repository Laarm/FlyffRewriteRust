//! `IpLimiter` (Plan/10 §7.1, WP-NET-04): the per-IP limits of the `/ws` upgrade and the admin-token charge.
//!
//! * **Upgrade bucket** (`net.upgrade_per_ip_per_sec` refill, `net.upgrade_per_ip_burst` capacity): every upgrade
//!   request takes one token; an empty bucket is `429 Retry-After: 1` (check 4).
//! * **Open sockets** (`net.max_conns_per_ip`): the number of WebSockets one address holds open; the one over the
//!   limit is `429` (check 5). The slot is a [`ConnSlot`] guard: dropping it (the socket ended, the upgrade never
//!   completed, the session task panicked) frees the slot.
//! * [`IpLimiter::charge`] takes one token of the same bucket without opening a socket: `admin::auth` calls it on
//!   every failed admin-token attempt (10 §13.2), so a client that guesses tokens also loses its upgrade budget.
//!
//! Keys are the client addresses of `http::client_ip` (the one client-IP rule); IPv4-mapped IPv6 is folded and
//! IPv6 addresses are limited per `/64` (one household is one prefix). Entries with no open socket and a full
//! bucket are removed by [`IpLimiter::sweep`] (every 60 s, driven by the upgrade path itself, so no timer task is
//! needed). No HTTP request limiting happens here: `/api/*` and the static routes are the rate classes of 24 §3.7,
//! which are not applied twice (a `POST /api/login` never touches this bucket).

use crate::http::ratelimit::limit_key;
use flyff_assets::Clock;
use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// How often idle entries are dropped.
const SWEEP_EVERY_MS: u64 = 60_000;

/// The limits (a copy of the `[net]` keys; a value of 0 switches that limit off).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IpLimits {
    /// `net.upgrade_per_ip_per_sec`.
    pub upgrade_per_sec: u32,
    /// `net.upgrade_per_ip_burst`.
    pub upgrade_burst: u32,
    /// `net.max_conns_per_ip`.
    pub max_conns: u32,
}

impl Default for IpLimits {
    fn default() -> Self {
        Self {
            upgrade_per_sec: 10,
            upgrade_burst: 50,
            max_conns: 16,
        }
    }
}

/// Why an upgrade was refused (the `reason` label of `flyff_ws_upgrades_rejected_total`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpgradeRefusal {
    /// The upgrade bucket is empty (`429`, `Retry-After` seconds).
    Rate {
        /// Seconds until a token is available.
        retry_after_s: u32,
    },
    /// The address already holds `net.max_conns_per_ip` sockets (`429`).
    Conns,
}

/// Rejections counted by reason (`flyff_ws_upgrades_rejected_total`).
#[derive(Debug, Default)]
pub struct UpgradeCounters {
    /// Empty upgrade bucket.
    pub rate: AtomicU64,
    /// Too many open sockets.
    pub conns: AtomicU64,
    /// Failed admin-token attempts charged to the bucket.
    pub charged: AtomicU64,
    /// Upgrades granted.
    pub granted: AtomicU64,
}

#[derive(Clone, Copy, Debug)]
struct IpState {
    tokens: f64,
    last_ms: u64,
    open: u32,
}

#[derive(Default)]
struct Table {
    map: BTreeMap<IpAddr, IpState>,
    last_sweep_ms: u64,
}

/// The per-IP tables of the gateway (see the module docs).
pub struct IpLimiter {
    limits: IpLimits,
    clock: Arc<dyn Clock>,
    table: Mutex<Table>,
    /// Counters by outcome.
    pub counters: UpgradeCounters,
}

impl std::fmt::Debug for IpLimiter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IpLimiter")
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

/// One open WebSocket of an address; dropping it releases the slot.
#[derive(Debug)]
pub struct ConnSlot {
    limiter: Option<Arc<IpLimiter>>,
    key: IpAddr,
}

impl ConnSlot {
    /// A slot that holds nothing (tests, a gateway without limits).
    pub fn none() -> ConnSlot {
        ConnSlot {
            limiter: None,
            key: IpAddr::from([0, 0, 0, 0]),
        }
    }
}

impl Drop for ConnSlot {
    fn drop(&mut self) {
        if let Some(l) = self.limiter.take() {
            l.release(self.key);
        }
    }
}

impl IpLimiter {
    /// A limiter over `clock` (monotonic milliseconds).
    pub fn new(limits: IpLimits, clock: Arc<dyn Clock>) -> Arc<IpLimiter> {
        Arc::new(IpLimiter {
            limits,
            clock,
            table: Mutex::new(Table::default()),
            counters: UpgradeCounters::default(),
        })
    }

    /// The limits in force.
    pub fn limits(&self) -> IpLimits {
        self.limits
    }

    fn refill(&self, st: &mut IpState, now: u64) {
        let burst = f64::from(self.limits.upgrade_burst.max(1));
        let rate = f64::from(self.limits.upgrade_per_sec);
        let elapsed = now.saturating_sub(st.last_ms) as f64 / 1000.0;
        st.tokens = (st.tokens + elapsed * rate).min(burst);
        st.last_ms = now;
    }

    fn fresh(&self, now: u64) -> IpState {
        IpState {
            tokens: f64::from(self.limits.upgrade_burst.max(1)),
            last_ms: now,
            open: 0,
        }
    }

    fn sweep_locked(&self, t: &mut Table, now: u64) {
        if now.saturating_sub(t.last_sweep_ms) < SWEEP_EVERY_MS {
            return;
        }
        t.last_sweep_ms = now;
        let burst = f64::from(self.limits.upgrade_burst.max(1));
        let rate = f64::from(self.limits.upgrade_per_sec);
        t.map.retain(|_, st| {
            let elapsed = now.saturating_sub(st.last_ms) as f64 / 1000.0;
            let full = st.tokens + elapsed * rate >= burst;
            st.open > 0 || !full
        });
    }

    /// Checks 4 and 5 of 10 §7.1 for an upgrade from `ip`: takes a token, then a connection slot. Both are
    /// undone when the second refuses (a refused upgrade costs nothing but its own attempt... except that the
    /// token of an attempt over `max_conns` stays spent, so a flood of refused upgrades still drains the bucket).
    pub fn try_upgrade(self: &Arc<Self>, ip: IpAddr) -> Result<ConnSlot, UpgradeRefusal> {
        let key = limit_key(ip);
        let now = self.clock.now_ms();
        let mut guard = self.table.lock();
        let t = &mut *guard;
        self.sweep_locked(t, now);
        let mut st = t.map.get(&key).copied().unwrap_or_else(|| self.fresh(now));
        let rate = self.limits.upgrade_per_sec;
        if rate > 0 {
            self.refill(&mut st, now);
            if st.tokens < 1.0 {
                let wait = ((1.0 - st.tokens) / f64::from(rate)).ceil().max(1.0);
                t.map.insert(key, st);
                self.counters.rate.fetch_add(1, Ordering::Relaxed);
                return Err(UpgradeRefusal::Rate {
                    retry_after_s: wait as u32,
                });
            }
            st.tokens -= 1.0;
        }
        if self.limits.max_conns > 0 && st.open >= self.limits.max_conns {
            t.map.insert(key, st);
            self.counters.conns.fetch_add(1, Ordering::Relaxed);
            return Err(UpgradeRefusal::Conns);
        }
        st.open = st.open.saturating_add(1);
        t.map.insert(key, st);
        drop(guard);
        self.counters.granted.fetch_add(1, Ordering::Relaxed);
        Ok(ConnSlot {
            limiter: Some(Arc::clone(self)),
            key,
        })
    }

    fn release(&self, key: IpAddr) {
        let mut t = self.table.lock();
        if let Some(st) = t.map.get_mut(&key) {
            st.open = st.open.saturating_sub(1);
        }
    }

    /// Takes one token of the upgrade bucket of `ip` without opening a socket (`admin::auth` on a failed
    /// token, 10 §13.2). The bucket never goes below zero; with the rate switched off nothing is charged.
    pub fn charge(&self, ip: IpAddr) {
        if self.limits.upgrade_per_sec == 0 {
            return;
        }
        let key = limit_key(ip);
        let now = self.clock.now_ms();
        let mut guard = self.table.lock();
        let t = &mut *guard;
        self.sweep_locked(t, now);
        let mut st = t.map.get(&key).copied().unwrap_or_else(|| self.fresh(now));
        self.refill(&mut st, now);
        st.tokens = (st.tokens - 1.0).max(0.0);
        t.map.insert(key, st);
        drop(guard);
        self.counters.charged.fetch_add(1, Ordering::Relaxed);
    }

    /// Open sockets of `ip` (its `/64` for IPv6).
    pub fn open_of(&self, ip: IpAddr) -> u32 {
        self.table
            .lock()
            .map
            .get(&limit_key(ip))
            .map_or(0, |s| s.open)
    }

    /// Whole tokens left in the bucket of `ip` (tests, `/admin`).
    pub fn tokens_of(&self, ip: IpAddr) -> f64 {
        let now = self.clock.now_ms();
        let key = limit_key(ip);
        let mut t = self.table.lock();
        match t.map.get_mut(&key) {
            Some(st) => {
                let mut s = *st;
                self.refill(&mut s, now);
                s.tokens
            }
            None => f64::from(self.limits.upgrade_burst.max(1)),
        }
    }

    /// Tracked addresses (tests).
    pub fn tracked(&self) -> usize {
        self.table.lock().map.len()
    }

    /// Drops idle entries now (the upgrade path also does it every 60 s).
    pub fn sweep(&self) {
        let now = self.clock.now_ms();
        let mut t = self.table.lock();
        t.last_sweep_ms = 0;
        self.sweep_locked(&mut t, now.max(SWEEP_EVERY_MS));
    }
}
