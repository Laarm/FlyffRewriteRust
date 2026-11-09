//! The per-identity limits of the admin API (Plan/25 §12 rule 13; WP-ADMIN-02): the token bucket of
//! `admin.mutations_per_s` and the streaming permit ("one concurrent streaming query per identity").

use parking_lot::Mutex;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[derive(Debug)]
struct Bucket {
    tokens: f64,
    last_ms: u64,
}

/// Token buckets keyed by identity: capacity and refill rate are both `rate` per second, so a burst of `rate`
/// mutations passes and the next one waits (`429 rate_limited`).
#[derive(Debug, Default)]
pub struct TokenBuckets {
    map: Mutex<BTreeMap<String, Bucket>>,
}

/// Entries idle for this long are dropped when the table grows.
const PRUNE_IDLE_MS: u64 = 60_000;
const PRUNE_ABOVE: usize = 1024;

impl TokenBuckets {
    /// Takes one token of `key`'s bucket at `now_ms` (monotonic milliseconds). `Err(retry_after_ms)` when the bucket
    /// is empty.
    pub fn try_take(&self, key: &str, rate_per_s: u32, now_ms: u64) -> Result<(), u64> {
        let rate = f64::from(rate_per_s.max(1));
        let mut map = self.map.lock();
        if map.len() > PRUNE_ABOVE {
            map.retain(|_, b| now_ms.saturating_sub(b.last_ms) < PRUNE_IDLE_MS);
        }
        let b = map.entry(key.to_owned()).or_insert(Bucket {
            tokens: rate,
            last_ms: now_ms,
        });
        let elapsed_s = now_ms.saturating_sub(b.last_ms) as f64 / 1000.0;
        b.tokens = (b.tokens + elapsed_s * rate).min(rate);
        b.last_ms = now_ms;
        if b.tokens >= 1.0 {
            b.tokens -= 1.0;
            Ok(())
        } else {
            let missing = 1.0 - b.tokens;
            Err(((missing / rate) * 1000.0).ceil().max(1.0) as u64)
        }
    }

    /// Tracked identities (tests).
    pub fn len(&self) -> usize {
        self.map.lock().len()
    }

    /// `true` when no identity is tracked.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// At most one streaming query (NDJSON item search, ...) per identity at a time.
#[derive(Debug, Default)]
pub struct StreamPermits {
    held: Mutex<BTreeSet<String>>,
}

/// A held streaming permit; dropping it frees the identity's slot.
#[derive(Debug)]
pub struct StreamGuard {
    permits: Arc<StreamPermits>,
    key: String,
}

impl StreamPermits {
    /// Takes the permit of `key`; `None` while another stream of the identity runs. Move the guard into the
    /// response stream so the slot is freed when the stream ends or the client goes away.
    pub fn acquire(self: &Arc<Self>, key: &str) -> Option<StreamGuard> {
        if self.held.lock().insert(key.to_owned()) {
            Some(StreamGuard {
                permits: Arc::clone(self),
                key: key.to_owned(),
            })
        } else {
            None
        }
    }

    /// Streams running now (tests).
    pub fn running(&self) -> usize {
        self.held.lock().len()
    }
}

impl Drop for StreamGuard {
    fn drop(&mut self) {
        self.permits.held.lock().remove(&self.key);
    }
}
