//! Login-gated assets (`web.public_assets = false`, Plan/24 §13, PD-09).
//!
//! * [`AssetTokens`]: 32 random bytes (OS CSPRNG, base64url) per login, held in memory with the account and the
//!   time of last use. A token unused for `web.asset_cookie_ttl_s` is invalid (the validity slides on use); all
//!   tokens die with the process; [`AssetTokens::revoke_account`] drops every token of an account (called by
//!   doc 12 on a ban). Doc 12's `/api/login` success path calls [`AssetTokens::issue`] and sets the returned
//!   `Set-Cookie` value.
//! * [`GateTarget`] / [`is_public`]: the public set that stays reachable without a cookie while gated: `/res` and
//!   `/gen/png` under `theme/`, `/gen/cursor`, the title music and the UI click sound, and the bundles flagged
//!   public (`texts`, `resdata-layouts`, `ui-atlases`, `sounds`, `music`, `names`).
//!
//! The gate is applied by the asset handlers (they know the resolved target), after the path is validated and
//! the source exists (so a 404 stays a 404 and a 401 is only given for something that would be served).

use axum::http::{HeaderMap, HeaderValue, header};
use flyff_assets::{AssetKind, Clock};
use flyff_common::AccountId;
use parking_lot::Mutex;
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

/// The cookie name.
pub const COOKIE_NAME: &str = "flyff_assets";
/// `Max-Age` of the cookie (30 days; the server-side sliding TTL is what counts).
pub const COOKIE_MAX_AGE_S: u32 = 2_592_000;

#[derive(Debug)]
struct TokenEntry {
    account: AccountId,
    last_used_ms: u64,
}

/// The in-memory token table.
pub struct AssetTokens {
    ttl_ms: u64,
    clock: Arc<dyn Clock>,
    map: Mutex<TokenMap>,
}

#[derive(Default)]
struct TokenMap {
    tokens: HashMap<String, TokenEntry>,
    last_sweep_ms: u64,
}

impl std::fmt::Debug for AssetTokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AssetTokens")
            .field("ttl_ms", &self.ttl_ms)
            .finish_non_exhaustive()
    }
}

impl AssetTokens {
    /// A table whose tokens expire after `ttl_s` seconds without use (`web.asset_cookie_ttl_s`).
    pub fn new(ttl_s: u32, clock: Arc<dyn Clock>) -> Arc<AssetTokens> {
        Arc::new(AssetTokens {
            ttl_ms: u64::from(ttl_s) * 1000,
            clock,
            map: Mutex::new(TokenMap::default()),
        })
    }

    /// Issues a token for `account` and returns the `Set-Cookie` header value
    /// (`flyff_assets=<token>; Path=/; HttpOnly; SameSite=Strict; Max-Age=2592000[; Secure]`). `secure` is
    /// whether the login request arrived over https.
    pub fn issue(&self, account: AccountId, secure: bool) -> HeaderValue {
        cookie_header(&self.issue_token(account), secure)
    }

    /// Issues a token for `account` and returns the raw token (the admin console keeps it with its session so that
    /// sign-out and expiry revoke exactly this token with [`AssetTokens::revoke_token`], 25 §11.7).
    pub fn issue_token(&self, account: AccountId) -> String {
        let token = new_token();
        let now = self.clock.now_ms();
        let mut m = self.map.lock();
        self.sweep_locked(&mut m, now);
        m.tokens.insert(
            token.clone(),
            TokenEntry {
                account,
                last_used_ms: now,
            },
        );
        token
    }

    /// Drops one token; `true` when it existed.
    pub fn revoke_token(&self, token: &str) -> bool {
        self.map.lock().tokens.remove(token).is_some()
    }

    /// `true` when `token` is valid; a valid token's idle timer restarts.
    pub fn validate(&self, token: &str) -> bool {
        let now = self.clock.now_ms();
        let mut m = self.map.lock();
        match m.tokens.get_mut(token) {
            Some(e) if now.saturating_sub(e.last_used_ms) <= self.ttl_ms => {
                e.last_used_ms = now;
                true
            }
            Some(_) => {
                m.tokens.remove(token);
                false
            }
            None => false,
        }
    }

    /// `true` when the request carries a valid asset cookie.
    pub fn validate_headers(&self, headers: &HeaderMap) -> bool {
        cookie_token(headers).is_some_and(|t| self.validate(t))
    }

    /// Drops every token of `account` (a ban); returns how many.
    pub fn revoke_account(&self, account: AccountId) -> usize {
        let mut m = self.map.lock();
        let before = m.tokens.len();
        m.tokens.retain(|_, e| e.account != account);
        before - m.tokens.len()
    }

    /// Live tokens (tests, `/admin`).
    pub fn len(&self) -> usize {
        self.map.lock().tokens.len()
    }

    /// `true` when no token exists.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn sweep_locked(&self, m: &mut TokenMap, now: u64) {
        // At most once a minute: drop what has been idle past the TTL.
        if now.saturating_sub(m.last_sweep_ms) < 60_000 {
            return;
        }
        m.last_sweep_ms = now;
        let ttl = self.ttl_ms;
        m.tokens
            .retain(|_, e| now.saturating_sub(e.last_used_ms) <= ttl);
    }
}

/// The `Set-Cookie` value of an asset token (`flyff_assets=<token>; Path=/; HttpOnly; SameSite=Strict;
/// Max-Age=2592000[; Secure]`).
pub fn cookie_header(token: &str, secure: bool) -> HeaderValue {
    let mut text = format!(
        "{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={COOKIE_MAX_AGE_S}"
    );
    if secure {
        text.push_str("; Secure");
    }
    // base64url and the fixed attributes are valid header text
    HeaderValue::from_str(&text).unwrap_or_else(|_| HeaderValue::from_static(""))
}

/// The `Set-Cookie` value that removes the asset cookie (sign-out of the admin console).
pub fn cookie_clear_header(secure: bool) -> HeaderValue {
    let mut text = format!("{COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0");
    if secure {
        text.push_str("; Secure");
    }
    HeaderValue::from_str(&text).unwrap_or_else(|_| HeaderValue::from_static(""))
}

/// The value of the `flyff_assets` cookie of a request.
pub fn cookie_token(headers: &HeaderMap) -> Option<&str> {
    for v in headers.get_all(header::COOKIE) {
        let Ok(text) = v.to_str() else {
            continue;
        };
        for part in text.split(';') {
            if let Some((name, value)) = part.trim().split_once('=')
                && name == COOKIE_NAME
            {
                return Some(value.trim());
            }
        }
    }
    None
}

fn new_token() -> String {
    let mut bytes = [0u8; 32];
    if getrandom::fill(&mut bytes).is_err() {
        // The OS CSPRNG failing is not recoverable for a session secret: derive nothing predictable,
        // hand out a token that no cookie can ever match.
        return String::new();
    }
    base64url(&bytes)
}

/// Unpadded base64url.
fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk.first().copied().unwrap_or(0));
        let b1 = u32::from(chunk.get(1).copied().unwrap_or(0));
        let b2 = u32::from(chunk.get(2).copied().unwrap_or(0));
        let n = (b0 << 16) | (b1 << 8) | b2;
        let take = chunk.len() + 1;
        for i in 0..take {
            let idx = ((n >> (18 - 6 * i)) & 63) as usize;
            if let Some(c) = ALPHABET.get(idx) {
                out.push(char::from(*c));
            }
        }
    }
    out
}

// ---- the public set ------------------------------------------------------------------------------------------

/// What a request asks for, as far as the gate is concerned.
#[derive(Clone, Copy, Debug)]
pub enum GateTarget<'a> {
    /// `/res/<fp>/<path>`: the canonical VFS path.
    Res(&'a str),
    /// `/gen/<kind>/<fp>/<key>`.
    Gen {
        /// The kind.
        kind: AssetKind,
        /// The request key.
        key: &'a str,
    },
    /// `/data/<bundle>…`; `public` is the bundle's flag.
    Data {
        /// Reachable without login.
        public: bool,
    },
}

/// Is the target part of the public set (reachable without a cookie while gated)? `public_files` are the
/// resolved title music and click sound paths.
pub fn is_public(public_files: &BTreeSet<String>, target: GateTarget<'_>) -> bool {
    match target {
        GateTarget::Res(path) => path.starts_with("theme/") || public_files.contains(path),
        GateTarget::Gen { kind, key } => match kind {
            AssetKind::CursorPng => true,
            AssetKind::UiPng => key.starts_with("theme/"),
            _ => false,
        },
        GateTarget::Data { public } => public,
    }
}
