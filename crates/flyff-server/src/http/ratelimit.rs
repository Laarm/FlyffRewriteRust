//! Per-IP rate classes (Plan/24 §3.7, WP-WEB-17): token buckets for the `static` and `api` classes and a
//! counting semaphore for concurrent `/gen` cache-miss waits (`gen_wait`).
//!
//! Buckets live in one `parking_lot::Mutex<HashMap<(key, class), Bucket>>`; entries idle for more than 120 s are
//! evicted every 60 s and the map never exceeds 200 000 entries (the oldest tenth is dropped when it is full).
//! A rate of 0 disables a class. IPv6 clients are keyed by their `/64` (one household is one `/64`; a host that
//! rotates through its prefix must not get a fresh bucket per address).
//!
//! The `ws` family and `/healthz`, `/metrics`, `/admin*` are not limited here (doc 10 owns the WS upgrade bucket;
//! the others are loopback/token-guarded).

use flyff_assets::Clock;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv6Addr};
use std::sync::Arc;

/// Entries idle longer than this are evicted (ms).
const IDLE_EVICT_MS: u64 = 120_000;
/// Eviction pass period (ms).
const SWEEP_EVERY_MS: u64 = 60_000;
/// Hard cap on tracked entries.
const MAX_ENTRIES: usize = 200_000;

/// The bucket classes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Class {
    /// Static files and `/gen` hits (`web.req_per_ip_per_sec` / `web.req_per_ip_burst`).
    Static,
    /// `/api/*` (`web.api_per_ip_per_sec` / `web.api_per_ip_burst`).
    Api,
}

/// The limits (a copy of the `[web]` keys).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RateConfig {
    /// `web.req_per_ip_per_sec`; 0 = class off.
    pub static_per_sec: u32,
    /// `web.req_per_ip_burst`.
    pub static_burst: u32,
    /// `web.api_per_ip_per_sec`; 0 = class off.
    pub api_per_sec: u32,
    /// `web.api_per_ip_burst`.
    pub api_burst: u32,
    /// `web.gen_waits_per_ip`; 0 = unlimited.
    pub gen_waits_per_ip: u32,
}

impl Default for RateConfig {
    fn default() -> Self {
        Self {
            static_per_sec: 300,
            static_burst: 2000,
            api_per_sec: 5,
            api_burst: 30,
            gen_waits_per_ip: 8,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Bucket {
    tokens: f64,
    last_ms: u64,
}

/// The shared limiter state.
pub struct RateLimits {
    cfg: RateConfig,
    clock: Arc<dyn Clock>,
    buckets: Mutex<Buckets>,
    waits: Mutex<HashMap<IpAddr, u32>>,
}

#[derive(Default)]
struct Buckets {
    map: HashMap<(IpAddr, Class), Bucket>,
    last_sweep_ms: u64,
}

impl std::fmt::Debug for RateLimits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RateLimits")
            .field("cfg", &self.cfg)
            .finish_non_exhaustive()
    }
}

/// A held `gen_wait` slot; dropping it frees the slot.
#[derive(Debug)]
pub struct GenPermit {
    limits: Option<Arc<RateLimits>>,
    key: IpAddr,
}

impl Drop for GenPermit {
    fn drop(&mut self) {
        if let Some(l) = &self.limits {
            let mut w = l.waits.lock();
            if let Some(n) = w.get_mut(&self.key) {
                *n = n.saturating_sub(1);
                if *n == 0 {
                    w.remove(&self.key);
                }
            }
        }
    }
}

/// The key a client address is limited under: IPv4 as is, IPv6 by its `/64`.
pub fn limit_key(ip: IpAddr) -> IpAddr {
    match ip.to_canonical() {
        IpAddr::V6(v6) => {
            let bits = u128::from(v6) & (u128::MAX << 64);
            IpAddr::V6(Ipv6Addr::from(bits))
        }
        v4 => v4,
    }
}

impl RateLimits {
    /// A limiter over `clock` (monotonic ms).
    pub fn new(cfg: RateConfig, clock: Arc<dyn Clock>) -> Arc<RateLimits> {
        Arc::new(RateLimits {
            cfg,
            clock,
            buckets: Mutex::new(Buckets::default()),
            waits: Mutex::new(HashMap::new()),
        })
    }

    /// The limits in force.
    pub fn config(&self) -> RateConfig {
        self.cfg
    }

    /// Takes one token of `class` for `ip`. `Err(seconds)` = over the limit, retry after that many seconds.
    pub fn check(&self, ip: IpAddr, class: Class) -> Result<(), u32> {
        let (rate, burst) = match class {
            Class::Static => (self.cfg.static_per_sec, self.cfg.static_burst),
            Class::Api => (self.cfg.api_per_sec, self.cfg.api_burst),
        };
        if rate == 0 {
            return Ok(());
        }
        let rate = f64::from(rate);
        let burst = f64::from(burst.max(1));
        let now = self.clock.now_ms();
        let key = (limit_key(ip), class);
        let mut guard = self.buckets.lock();
        if now.saturating_sub(guard.last_sweep_ms) >= SWEEP_EVERY_MS {
            guard.last_sweep_ms = now;
            guard
                .map
                .retain(|_, b| now.saturating_sub(b.last_ms) <= IDLE_EVICT_MS);
        }
        if guard.map.len() >= MAX_ENTRIES && !guard.map.contains_key(&key) {
            evict_oldest_tenth(&mut guard.map);
        }
        let bucket = guard.map.entry(key).or_insert(Bucket {
            tokens: burst,
            last_ms: now,
        });
        let elapsed = now.saturating_sub(bucket.last_ms) as f64 / 1000.0;
        bucket.tokens = (bucket.tokens + elapsed * rate).min(burst);
        bucket.last_ms = now;
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            Ok(())
        } else {
            let wait = ((1.0 - bucket.tokens) / rate).ceil().max(1.0);
            Err(wait as u32)
        }
    }

    /// Takes a `gen_wait` slot for `ip`; `None` = `web.gen_waits_per_ip` waits are already running.
    pub fn acquire_gen_wait(self: &Arc<Self>, ip: IpAddr) -> Option<GenPermit> {
        let limit = self.cfg.gen_waits_per_ip;
        if limit == 0 {
            return Some(GenPermit {
                limits: None,
                key: ip,
            });
        }
        let key = limit_key(ip);
        let mut w = self.waits.lock();
        let n = w.entry(key).or_insert(0);
        if *n >= limit {
            return None;
        }
        *n += 1;
        Some(GenPermit {
            limits: Some(Arc::clone(self)),
            key,
        })
    }

    /// Concurrent `gen_wait` slots held by `ip`.
    pub fn gen_waits_of(&self, ip: IpAddr) -> u32 {
        self.waits.lock().get(&limit_key(ip)).copied().unwrap_or(0)
    }

    /// Tracked bucket entries (tests, `/admin`).
    pub fn tracked(&self) -> usize {
        self.buckets.lock().map.len()
    }
}

fn evict_oldest_tenth(map: &mut HashMap<(IpAddr, Class), Bucket>) {
    let mut ages: Vec<u64> = map.values().map(|b| b.last_ms).collect();
    ages.sort_unstable();
    let cut = ages.get(ages.len() / 10).copied().unwrap_or(0);
    map.retain(|_, b| b.last_ms > cut);
}
