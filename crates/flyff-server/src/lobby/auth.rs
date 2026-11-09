//! Password transport, hashing and the wrong-password throttle (Plan/12 §3.2, §5.2, §6.4; WP-AUTH-02).
//!
//! * [`PwHash`]: the 32 lowercase hex characters of `md5(salt + password)` the browser (and the CLI) sends
//!   instead of the password (D3, legacy `NEUZ_PHASH`).
//! * [`PasswordHasher`]: argon2id over those 32 bytes behind a 2-permit semaphore and `spawn_blocking`
//!   (R14, 01 §6.2). The argon2 constants live once, in `flyff_db::password`.
//! * [`LoginThrottle`]: the per-IP wrong-password lock (PL-031, GP-06): 1 s / 5 s with
//!   `features.wrong_pw_fix`, else the legacy 15 s / 15 min.

use flyff_db::DbError;
pub use flyff_db::password::Verify;
use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv6Addr};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

/// The default `auth.legacy_md5_salt`.
pub use flyff_db::password::DEFAULT_MD5_SALT;

/// Concurrent argon2 computations (12 §5.2: **2** permits).
pub const HASH_PERMITS: usize = 2;
/// How long a request waits for a hash permit before `503 / 108` (12 §15).
pub const HASH_PERMIT_TIMEOUT: Duration = Duration::from_secs(10);

/// The `pwhash` of a request: exactly 32 lowercase hex characters (12 §3.2), kept as text. Its `Debug` shows
/// nothing of the value.
#[derive(Clone, PartialEq, Eq)]
pub struct PwHash(String);

impl PwHash {
    /// `Some` for exactly 32 characters of `[0-9a-f]`.
    pub fn parse(text: &str) -> Option<PwHash> {
        flyff_db::password::is_valid_pwhash(text).then(|| PwHash(text.to_owned()))
    }

    /// The 32 characters.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for PwHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PwHash(..)")
    }
}

/// `lowercase_hex(md5(utf8(salt + password)))` (12 §3.2): used by the CLI and the tests, never by the server
/// for client input. `legacy_pwhash("kikugalanet", "test") == "89d1ed22aac58f5bbea53b2fde81a946"`.
pub fn legacy_pwhash(salt: &str, password: &str) -> PwHash {
    // `flyff_db::password::legacy_pwhash` always yields 32 hex digits.
    PwHash(flyff_db::password::legacy_pwhash(salt, password))
}

/// Why hashing or verifying did not produce an answer.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    /// No permit within [`HASH_PERMIT_TIMEOUT`] (`503 / 108`).
    #[error("no password-hash permit within {0:?}")]
    PermitTimeout(Duration),
    /// The semaphore was closed (server shutdown).
    #[error("the hasher is shut down")]
    Closed,
    /// argon2 failed, or the stored hash is malformed.
    #[error("password hashing failed: {0}")]
    Hash(#[from] DbError),
    /// The blocking task panicked or was cancelled.
    #[error("the hashing task ended abnormally")]
    Task,
}

/// The computation behind [`PasswordHasher`]; the default is `flyff_db::password`. Tests instrument it.
pub trait HashEngine: Send + Sync + 'static {
    /// argon2id over `pwhash` with a random salt, as a PHC string.
    fn hash(&self, pwhash: &str) -> Result<String, DbError>;
    /// Constant-time verification against a stored PHC string.
    fn verify(&self, pwhash: &str, phc: &str) -> Result<Verify, DbError>;
}

/// The production engine (`flyff_db::password`: argon2id v0x13, m = 19456 KiB, t = 2, p = 1).
#[derive(Clone, Copy, Debug, Default)]
pub struct Argon2Engine;

impl HashEngine for Argon2Engine {
    fn hash(&self, pwhash: &str) -> Result<String, DbError> {
        flyff_db::password::hash_pwhash(pwhash)
    }

    fn verify(&self, pwhash: &str, phc: &str) -> Result<Verify, DbError> {
        flyff_db::password::verify_pwhash(pwhash, phc)
    }
}

/// argon2id with at most [`HASH_PERMITS`] computations at a time, each on the blocking pool (R14).
#[derive(Clone)]
pub struct PasswordHasher {
    permits: Arc<Semaphore>,
    wait: Duration,
    engine: Arc<dyn HashEngine>,
}

impl Default for PasswordHasher {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for PasswordHasher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PasswordHasher")
            .field("available_permits", &self.permits.available_permits())
            .finish()
    }
}

impl PasswordHasher {
    /// The production hasher: 2 permits, a 10 s permit wait, the argon2 engine.
    pub fn new() -> PasswordHasher {
        Self::with_engine(Arc::new(Argon2Engine), HASH_PERMITS, HASH_PERMIT_TIMEOUT)
    }

    /// A hasher over another engine, permit count and wait (tests).
    pub fn with_engine(
        engine: Arc<dyn HashEngine>,
        permits: usize,
        wait: Duration,
    ) -> PasswordHasher {
        PasswordHasher {
            permits: Arc::new(Semaphore::new(permits)),
            wait,
            engine,
        }
    }

    async fn permit(&self) -> Result<tokio::sync::OwnedSemaphorePermit, AuthError> {
        match tokio::time::timeout(self.wait, Arc::clone(&self.permits).acquire_owned()).await {
            Ok(Ok(p)) => Ok(p),
            Ok(Err(_)) => Err(AuthError::Closed),
            Err(_) => Err(AuthError::PermitTimeout(self.wait)),
        }
    }

    /// argon2id of `pw` as a PHC string (`$argon2id$v=19$m=19456,t=2,p=1$...`).
    pub async fn hash(&self, pw: &PwHash) -> Result<String, AuthError> {
        let _permit = self.permit().await?;
        let engine = Arc::clone(&self.engine);
        let pw = pw.0.clone();
        tokio::task::spawn_blocking(move || engine.hash(&pw))
            .await
            .map_err(|_| AuthError::Task)?
            .map_err(AuthError::from)
    }

    /// Verifies `pw` against a stored PHC string (constant time). [`Verify::Ok`] carries `needs_rehash`
    /// when the stored parameters differ from the current constants (12 §5.2 "Rehash").
    pub async fn verify(&self, pw: &PwHash, phc: &str) -> Result<Verify, AuthError> {
        let _permit = self.permit().await?;
        let engine = Arc::clone(&self.engine);
        let pw = pw.0.clone();
        let phc = phc.to_owned();
        tokio::task::spawn_blocking(move || engine.verify(&pw, &phc))
            .await
            .map_err(|_| AuthError::Task)?
            .map_err(AuthError::from)
    }
}

/// An active wrong-password lock (`check` refused).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Locked {
    /// `true`: the long lock after three failures (`ERROR_15MIN_PREVENT` 135); `false`: 134.
    pub long: bool,
    /// Time until the lock ends (at least 1000 ms).
    pub retry_after_ms: u32,
}

impl Locked {
    /// The legacy error code (134 or 135).
    pub const fn code(self) -> i32 {
        if self.long { 135 } else { 134 }
    }
}

#[derive(Clone, Copy, Debug)]
struct Failures {
    count: u32,
    last_failure: i64,
}

/// Entries are dropped after this many seconds without a failure (the longest lock is 900 s).
const PRUNE_AFTER_S: i64 = 900;
/// Hard cap on tracked addresses; when full, the oldest tenth is dropped.
const MAX_ENTRIES: usize = 100_000;

/// The per-IP wrong-password throttle (12 §6.4; legacy `CAccountMgr::Check`, `src:CERTIFIER/AccountMgr.cpp:34-105`).
///
/// ```text
/// lock1_s, lock3_s = if wrong_pw_fix { (1, 5) } else { (15, 900) }
/// check:   failures >= 3 and now - last <= lock3_s -> locked (135);  failures >= 1 and now - last <= lock1_s -> locked (134)
/// fail:    failures += 1, last = now     success: forget the address
/// ```
/// Times are whole UNIX seconds, as the legacy `time(NULL)` comparison (`<=`), so "1 s" admits retries after
/// 2 s at most. `check` never extends a lock. IPv4-mapped addresses are folded to IPv4 and IPv6 clients are
/// keyed by their `/64` (like the HTTP rate limiter).
#[derive(Debug)]
pub struct LoginThrottle {
    lock1_s: i64,
    lock3_s: i64,
    map: Mutex<BTreeMap<IpAddr, Failures>>,
}

/// The key a client address is tracked under: IPv4-mapped addresses fold to IPv4, IPv6 clients are keyed by
/// their `/64` (like the HTTP rate limiter).
pub(crate) fn ip_key(ip: IpAddr) -> IpAddr {
    match ip.to_canonical() {
        IpAddr::V6(v6) => {
            let o = v6.octets();
            let mut masked = [0u8; 16];
            masked[..8].copy_from_slice(&o[..8]);
            IpAddr::V6(Ipv6Addr::from(masked))
        }
        v4 => v4,
    }
}

impl LoginThrottle {
    /// `features.wrong_pw_fix = true`: 1 s / 5 s; `false`: the legacy 15 s / 900 s.
    pub fn new(wrong_pw_fix: bool) -> LoginThrottle {
        let (lock1_s, lock3_s) = if wrong_pw_fix { (1, 5) } else { (15, 900) };
        LoginThrottle {
            lock1_s,
            lock3_s,
            map: Mutex::new(BTreeMap::new()),
        }
    }

    /// May `ip` try a password at `now` (Unix seconds)? Never changes the state.
    pub fn check(&self, ip: IpAddr, now: i64) -> Result<(), Locked> {
        let map = self.map.lock();
        let Some(e) = map.get(&ip_key(ip)) else {
            return Ok(());
        };
        let age = now.saturating_sub(e.last_failure);
        let retry = |lock: i64, long: bool| {
            let left = lock.saturating_sub(age).saturating_add(1).max(1);
            Locked {
                long,
                retry_after_ms: u32::try_from(left.saturating_mul(1000)).unwrap_or(u32::MAX),
            }
        };
        if e.count >= 3 {
            return if age <= self.lock3_s {
                Err(retry(self.lock3_s, true))
            } else {
                Ok(())
            };
        }
        if e.count >= 1 && age <= self.lock1_s {
            return Err(retry(self.lock1_s, false));
        }
        Ok(())
    }

    /// A wrong password from `ip` at `now`.
    pub fn fail(&self, ip: IpAddr, now: i64) {
        let mut map = self.map.lock();
        if map.len() >= MAX_ENTRIES && !map.contains_key(&ip_key(ip)) {
            let mut by_age: Vec<(i64, IpAddr)> =
                map.iter().map(|(k, v)| (v.last_failure, *k)).collect();
            by_age.sort_unstable();
            for (_, k) in by_age.into_iter().take(MAX_ENTRIES / 10) {
                map.remove(&k);
            }
        }
        let e = map.entry(ip_key(ip)).or_insert(Failures {
            count: 0,
            last_failure: now,
        });
        e.count = e.count.saturating_add(1);
        e.last_failure = now;
    }

    /// A correct password: the address is forgotten (legacy `nError = 0`).
    pub fn success(&self, ip: IpAddr) {
        self.map.lock().remove(&ip_key(ip));
    }

    /// Drops the entries without a failure for more than 900 s (the sweeper, 1 Hz).
    pub fn prune(&self, now: i64) {
        self.map
            .lock()
            .retain(|_, e| now.saturating_sub(e.last_failure) <= PRUNE_AFTER_S);
    }

    /// Tracked addresses (tests, metrics).
    pub fn len(&self) -> usize {
        self.map.lock().len()
    }

    /// `true` when no address is tracked.
    pub fn is_empty(&self) -> bool {
        self.map.lock().is_empty()
    }
}
