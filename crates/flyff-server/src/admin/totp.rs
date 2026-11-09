//! TOTP for staff sessions (Plan/25 §1.3 "Optional TOTP", §11.6 `admin_totp`; WP-ADMIN-28): the algorithm and the
//! in-memory [`TotpGuard`]. The routes are `api/totp.rs`, the login rule is `api/session.rs::staff_login`.
//!
//! The algorithm is HOTP (RFC 4226) over HMAC-SHA1 (RFC 2104) with the time counter of RFC 6238: `T0 = 0`, a 30 s step,
//! six digits, the 160-bit secret of RFC 4226 R6, and at most one step of clock skew either side
//! ([`WINDOW`]). Everything here is pure: the clock is a parameter (a Unix second or a monotonic millisecond), so
//! the tests use fixed times. SHA-1 is the one algorithm every authenticator app supports; HMAC is written by hand
//! over a hash closure (the `sha1` crate is digest 0.10, the workspace's `sha2` is digest 0.11, so no `Digest` bound
//! could serve both) which lets the tests also run the SHA-256/512 vectors of RFC 6238 through it.
//!
//! [`TotpGuard`] holds what must not survive a restart anyway (a restart signs every console session out): the setups
//! waiting for their first code, the last accepted time step of each account (RFC 6238 §5.2: "MUST NOT accept the
//! second attempt of the same OTP") and the failure counter that locks an account's code checks (RFC 4226 §7.3).
//! A secret is never printed: the types that hold one have a `Debug` that shows counts or nothing.

use flyff_common::AccountId;
use parking_lot::Mutex;
use std::collections::HashMap;

/// The seconds of one time step (RFC 6238 §4, `X`).
pub const STEP_S: u64 = 30;
/// The digits of a code.
pub const DIGITS: u32 = 6;
/// The steps accepted before and after the current one (clock skew; never wider).
pub const WINDOW: u64 = 1;
/// The bytes of a secret: 160 bits (RFC 4226 R6).
pub const SECRET_LEN: usize = 20;
/// How long a begun setup waits for its first code, in milliseconds.
pub const SETUP_TTL_MS: u64 = 10 * 60 * 1000;
/// The same, in seconds (what the client counts down to).
pub const SETUP_TTL_S: i64 = 600;
/// Wrong codes of one account within [`ACCOUNT_LOCK_MS`] that lock its code checks (RFC 4226 §7.3).
pub const ACCOUNT_FAILURES: u32 = 5;
/// The window of those failures and the length of the lock, in milliseconds (15 minutes).
pub const ACCOUNT_LOCK_MS: u64 = 15 * 60 * 1000;
/// The issuer shown by the authenticator app.
pub const ISSUER: &str = "Flyff Admin";

// ---- HMAC and HOTP ----------------------------------------------------------------------------------------------

/// SHA-1 over the concatenation of `parts`.
fn sha1_of(parts: &[&[u8]]) -> Vec<u8> {
    use sha1::{Digest, Sha1};
    let mut h = Sha1::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().to_vec()
}

/// HMAC (RFC 2104) over the hash `hash` (the digest of the concatenation of the given parts) with the block size
/// `block` (64 bytes for SHA-1 and SHA-256, 128 for SHA-512): a key longer than the block is hashed first, then
/// zero-padded; the inner pad is 0x36, the outer 0x5c.
pub fn hmac(block: usize, hash: impl Fn(&[&[u8]]) -> Vec<u8>, key: &[u8], msg: &[u8]) -> Vec<u8> {
    let mut k = if key.len() > block {
        hash(&[key])
    } else {
        key.to_vec()
    };
    k.resize(block, 0);
    let ipad: Vec<u8> = k.iter().map(|b| b ^ 0x36).collect();
    let opad: Vec<u8> = k.iter().map(|b| b ^ 0x5c).collect();
    let inner = hash(&[&ipad, msg]);
    hash(&[&opad, &inner])
}

/// HMAC-SHA1 (RFC 2202).
pub fn hmac_sha1(key: &[u8], msg: &[u8]) -> Vec<u8> {
    hmac(64, sha1_of, key, msg)
}

/// HOTP (RFC 4226 §5.3) over any HMAC: the dynamic truncation of `HMAC(secret, counter)` to `digits` decimal
/// digits (at most 9). A digest shorter than the offset needs (never for SHA-1/256/512) reads as zeros.
pub fn hotp(
    block: usize,
    hash: impl Fn(&[&[u8]]) -> Vec<u8>,
    secret: &[u8],
    counter: u64,
    digits: u32,
) -> u32 {
    let mac = hmac(block, hash, secret, &counter.to_be_bytes());
    let offset = mac.last().map_or(0, |b| usize::from(b & 0x0f));
    let word: [u8; 4] = mac
        .get(offset..offset.saturating_add(4))
        .and_then(|s| <[u8; 4]>::try_from(s).ok())
        .unwrap_or([0; 4]);
    let binary = u32::from_be_bytes(word) & 0x7fff_ffff;
    binary % 10u32.checked_pow(digits).unwrap_or(1_000_000_000)
}

/// HOTP-SHA1 (RFC 4226): the code of `counter` as a number below `10^digits`; print it zero-padded.
pub fn hotp_sha1(secret: &[u8], counter: u64, digits: u32) -> u32 {
    hotp(64, sha1_of, secret, counter, digits)
}

// ---- TOTP -----------------------------------------------------------------------------------------------------------

/// The time step of a Unix second (RFC 6238 §4: `T0 = 0`, `X = 30`); `None` before 1970.
pub fn step_of(unix: i64) -> Option<u64> {
    u64::try_from(unix).ok().map(|u| u / STEP_S)
}

/// The six digits of the code of `step`, as ASCII.
pub fn code_bytes(secret: &[u8], step: u64) -> [u8; 6] {
    let mut n = hotp_sha1(secret, step, DIGITS);
    let mut out = [b'0'; 6];
    for slot in out.iter_mut().rev() {
        *slot = b'0' + u8::try_from(n % 10).unwrap_or(0);
        n /= 10;
    }
    out
}

/// The code of `step` as text (leading zeros kept).
pub fn code_string(secret: &[u8], step: u64) -> String {
    String::from_utf8_lossy(&code_bytes(secret, step)).into_owned()
}

/// The code a person typed: ASCII spaces are removed (apps show "123 456"), then exactly six ASCII digits; anything
/// else is `None` (a wrong code, never an error of its own).
pub fn parse_code(text: &str) -> Option<[u8; 6]> {
    let mut out = [0u8; 6];
    let mut n = 0usize;
    for b in text.bytes() {
        if b == b' ' {
            continue;
        }
        if !b.is_ascii_digit() {
            return None;
        }
        *out.get_mut(n)? = b;
        n += 1;
    }
    (n == out.len()).then_some(out)
}

/// The XOR fold of two codes: zero exactly when they are equal, with no early exit.
fn same(a: &[u8; 6], b: &[u8; 6]) -> bool {
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

/// The time step of `code` at `unix`, or `None`: the steps `t - WINDOW ..= t + WINDOW` are all computed and
/// compared (a constant-time XOR fold, no early exit), and the highest matching step wins.
pub fn matching_step(secret: &[u8], code: &[u8; 6], unix: i64) -> Option<u64> {
    let now = step_of(unix)?;
    let mut hit = None;
    for step in now.saturating_sub(WINDOW)..=now.saturating_add(WINDOW) {
        if same(&code_bytes(secret, step), code) {
            hit = Some(step);
        }
    }
    hit
}

// ---- base32 and the key URI -------------------------------------------------------------------------------------------

/// Base32 (RFC 4648 §6: the alphabet `A-Z2-7`), without padding: how authenticator apps take a secret by hand.
pub fn base32(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let letter = |v: u32| {
        let i = usize::try_from(v & 31).unwrap_or(0);
        char::from(ALPHABET.get(i).copied().unwrap_or(b'A'))
    };
    let mut out = String::with_capacity(bytes.len().div_ceil(5) * 8);
    let mut buffer = 0u32;
    let mut bits = 0u32;
    for &b in bytes {
        buffer = (buffer << 8) | u32::from(b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(letter(buffer >> bits));
        }
        buffer &= (1u32 << bits) - 1;
    }
    if bits > 0 {
        out.push(letter(buffer << (5 - bits)));
    }
    out
}

/// Percent-encodes `text` for a URI path or query component (RFC 3986 unreserved characters pass).
fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The key URI of the authenticator apps: `otpauth://totp/Flyff%20Admin:<account>?secret=<base32>&issuer=Flyff%20Admin
/// &algorithm=SHA1&digits=6&period=30`. It carries the secret: it leaves the server once, in the setup answer.
pub fn key_uri(account: &str, secret: &[u8]) -> String {
    let issuer = percent_encode(ISSUER);
    format!(
        "otpauth://totp/{issuer}:{}?secret={}&issuer={issuer}&algorithm=SHA1&digits={DIGITS}&period={STEP_S}",
        percent_encode(account),
        base32(secret),
    )
}

/// A fresh 160-bit secret from the OS random source; `None` when it fails (the route answers 500).
pub fn new_secret() -> Option<[u8; SECRET_LEN]> {
    let mut s = [0u8; SECRET_LEN];
    getrandom::fill(&mut s).ok()?;
    Some(s)
}

// ---- the guard -----------------------------------------------------------------------------------------------------------

/// A setup that waits for its first code: the secret the client was shown.
#[derive(Clone, Copy)]
pub struct PendingSetup {
    /// The secret (20 raw bytes).
    pub secret: [u8; SECRET_LEN],
    /// The monotonic millisecond the setup began.
    pub created_ms: u64,
}

impl std::fmt::Debug for PendingSetup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingSetup")
            .field("created_ms", &self.created_ms)
            .finish_non_exhaustive()
    }
}

struct Setup {
    /// The public id of the console session that began it: another session of the same account cannot confirm it.
    session: String,
    pending: PendingSetup,
}

#[derive(Clone, Copy)]
struct Failures {
    count: u32,
    since_ms: u64,
    /// 0 = not locked.
    locked_until_ms: u64,
}

#[derive(Default)]
struct Inner {
    setups: HashMap<AccountId, Setup>,
    last_step: HashMap<AccountId, u64>,
    failures: HashMap<AccountId, Failures>,
}

/// The in-memory state of staff TOTP: pending setups, the replay rule and the per-account lock. One lock; the times
/// are monotonic milliseconds of the caller's clock (`LobbyClock::now_ms`).
#[derive(Default)]
pub struct TotpGuard {
    inner: Mutex<Inner>,
}

impl std::fmt::Debug for TotpGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let g = self.inner.lock();
        f.debug_struct("TotpGuard")
            .field("setups", &g.setups.len())
            .field("accepted_accounts", &g.last_step.len())
            .field("failing_accounts", &g.failures.len())
            .finish()
    }
}

impl TotpGuard {
    /// Remembers a setup of `account` for the session `session` (a new one replaces the old) and forgets expired
    /// setups and failure records.
    pub fn begin_setup(
        &self,
        account: AccountId,
        session: &str,
        secret: [u8; SECRET_LEN],
        now_ms: u64,
    ) {
        let mut g = self.inner.lock();
        g.setups
            .retain(|_, s| now_ms.saturating_sub(s.pending.created_ms) < SETUP_TTL_MS);
        g.failures.retain(|_, f| {
            f.locked_until_ms > now_ms || now_ms.saturating_sub(f.since_ms) <= ACCOUNT_LOCK_MS
        });
        g.setups.insert(
            account,
            Setup {
                session: session.to_owned(),
                pending: PendingSetup {
                    secret,
                    created_ms: now_ms,
                },
            },
        );
    }

    /// The live setup of `account` begun by `session`; `None` when there is none, it is another session's, or it
    /// expired (an expired one is dropped).
    pub fn setup(&self, account: AccountId, session: &str, now_ms: u64) -> Option<PendingSetup> {
        let mut g = self.inner.lock();
        let live = g
            .setups
            .get(&account)
            .is_some_and(|s| now_ms.saturating_sub(s.pending.created_ms) < SETUP_TTL_MS);
        if !live {
            g.setups.remove(&account);
            return None;
        }
        g.setups
            .get(&account)
            .filter(|s| s.session == session)
            .map(|s| s.pending)
    }

    /// Drops the setup of `account` (after a successful confirmation).
    pub fn clear_setup(&self, account: AccountId) {
        self.inner.lock().setups.remove(&account);
    }

    /// The replay rule (RFC 6238 §5.2): an atomic check-and-set of the last accepted step of `account`. `true` only
    /// when `step` is newer than every step accepted before (also across two concurrent sign-ins); an accepted code
    /// clears the account's failure count.
    pub fn accept(&self, account: AccountId, step: u64) -> bool {
        let mut g = self.inner.lock();
        if g.last_step.get(&account).is_some_and(|last| step <= *last) {
            return false;
        }
        g.last_step.insert(account, step);
        g.failures.remove(&account);
        true
    }

    /// Counts a wrong or replayed code of `account`: the [`ACCOUNT_FAILURES`]th within [`ACCOUNT_LOCK_MS`] locks the
    /// account's code checks for [`ACCOUNT_LOCK_MS`] from that moment. A failure while locked changes nothing.
    pub fn failed(&self, account: AccountId, now_ms: u64) {
        let mut g = self.inner.lock();
        let f = g.failures.entry(account).or_insert(Failures {
            count: 0,
            since_ms: now_ms,
            locked_until_ms: 0,
        });
        if f.locked_until_ms > now_ms {
            return;
        }
        if f.locked_until_ms != 0 || now_ms.saturating_sub(f.since_ms) > ACCOUNT_LOCK_MS {
            *f = Failures {
                count: 0,
                since_ms: now_ms,
                locked_until_ms: 0,
            };
        }
        f.count = f.count.saturating_add(1);
        if f.count >= ACCOUNT_FAILURES {
            f.locked_until_ms = now_ms.saturating_add(ACCOUNT_LOCK_MS);
        }
    }

    /// The code checks of `account` are locked.
    pub fn locked(&self, account: AccountId, now_ms: u64) -> bool {
        self.inner
            .lock()
            .failures
            .get(&account)
            .is_some_and(|f| f.locked_until_ms > now_ms)
    }

    /// Forgets the failures and the setup of `account` (its factor was removed or reset). The last accepted step
    /// stays: a new secret never makes an old code usable again.
    pub fn forget(&self, account: AccountId) {
        let mut g = self.inner.lock();
        g.failures.remove(&account);
        g.setups.remove(&account);
    }
}
