//! Admin sessions (Plan/25 §1.3, §1.8, §11.7; WP-ADMIN-01): the in-memory table behind the `flyff_admin` cookie.
//!
//! * The cookie is 32 random bytes, base64url, `HttpOnly; SameSite=Strict; Path=/admin` (`; Secure` over TLS); the
//!   table is keyed by its blake3 digest, so a memory dump of the table holds no usable cookie.
//! * Sessions live in memory only: the idle timeout `admin.session_idle_min` (30) and the absolute lifetime
//!   `admin.session_max_h` (12) apply; a restart signs everybody out.
//! * With `web.public_assets = false` the session also owns a `flyff_assets` token (staff: issued for its account,
//!   owners: for the pseudo account [`OWNER_ASSET_KEY`]); ending the session revokes exactly that token.
//! * Every end of a session is announced on a broadcast channel ([`AdminSessions::subscribe_ended`]) with its
//!   [`EndReason`]: the WebSocket gateway closes the socket of the session with `SessionEnded{reason}` and code 4001.
//!   [`AdminSessions::end_account`] is what the account events call on a ban, an authority change or a password
//!   change (25 §1.8).

use super::auth::base64url;
use super::dto::{AdminSessionRow, IdentityKind};
use super::identity::{Identity, OWNER_ASSET_KEY, SessionRef};
use crate::http::asset_auth::{self, AssetTokens};
use crate::lobby::online::LobbyClock;
use axum::http::{HeaderMap, HeaderValue, header};
use flyff_common::AccountId;
use flyff_common::gm::Authority;
use parking_lot::{Mutex, RwLock};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;

/// The cookie name.
pub const COOKIE_NAME: &str = "flyff_admin";
/// Sessions per identity; the oldest one is dropped when a new login would exceed it.
pub const MAX_SESSIONS_PER_IDENTITY: usize = 16;
/// Length of a cookie token (32 bytes, unpadded base64url).
const TOKEN_LEN: usize = 43;

/// Why a session ended; the wire name travels in `SessionEnded{reason}` (25 §11.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EndReason {
    /// `DELETE /admin/api/session`.
    Logout,
    /// The account was banned.
    Banned,
    /// The account's authority changed.
    Authority,
    /// The account's password changed.
    Password,
    /// The server is shutting down.
    Shutdown,
    /// The idle timeout or the absolute lifetime ran out.
    Expired,
    /// Ended by an administrator (`DELETE /admin/api/admin-sessions/{id}`).
    Revoked,
}

impl EndReason {
    /// The wire name (`logout`, `ban`, `authority`, `password`, `shutdown`, `expired`, `revoked`).
    pub fn name(self) -> &'static str {
        match self {
            EndReason::Logout => "logout",
            EndReason::Banned => "ban",
            EndReason::Authority => "authority",
            EndReason::Password => "password",
            EndReason::Shutdown => "shutdown",
            EndReason::Expired => "expired",
            EndReason::Revoked => "revoked",
        }
    }
}

/// A session that ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionEnded {
    /// The public handle of the session.
    pub id: String,
    /// The staff account, when the session had one.
    pub account: Option<AccountId>,
    /// The audit actor of the session.
    pub identity: String,
    /// Why.
    pub reason: EndReason,
}

/// What the account tables said about a staff account when it signed in (the 10 s poll compares against it, 25 §1.8).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountSnapshot {
    /// The stored account-level letter (`accounts.authority`).
    pub authority_db: Authority,
    /// blake3 of the stored PHC string (`accounts.password_hash`).
    pub password_digest: [u8; 32],
}

impl AccountSnapshot {
    /// The digest of a stored password hash.
    pub fn digest(phc: &str) -> [u8; 32] {
        *blake3::hash(phc.as_bytes()).as_bytes()
    }
}

/// The live timeouts (`admin.session_idle_min`, `admin.session_max_h`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionSettings {
    /// Idle timeout.
    pub idle: Duration,
    /// Absolute lifetime.
    pub max: Duration,
}

impl SessionSettings {
    /// From the configuration values.
    pub fn from_minutes_hours(idle_min: u32, max_h: u32) -> SessionSettings {
        SessionSettings {
            idle: Duration::from_secs(u64::from(idle_min) * 60),
            max: Duration::from_secs(u64::from(max_h) * 3600),
        }
    }
}

impl Default for SessionSettings {
    fn default() -> Self {
        SessionSettings::from_minutes_hours(30, 12)
    }
}

/// The asset-cookie side of sessions: with `web.public_assets = false` a session issues and revokes a
/// `flyff_assets` token.
#[derive(Clone)]
pub struct AssetGate {
    /// The token table of the HTTP layer.
    pub tokens: Arc<AssetTokens>,
    /// `true` when `web.public_assets = false` (the cookie matters).
    pub gated: bool,
}

/// What a login hands to [`AdminSessions::create`].
#[derive(Clone, Debug)]
pub struct NewSession {
    /// The identity (its `session` field is filled by the table).
    pub identity: Identity,
    /// The account snapshot of a staff login.
    pub snapshot: Option<AccountSnapshot>,
}

/// The result of [`AdminSessions::create`]: the identity with its session handle and the `Set-Cookie` values.
#[derive(Clone, Debug)]
pub struct Created {
    /// The identity of the new session (`session` is set).
    pub identity: Identity,
    /// `Set-Cookie: flyff_admin=...`.
    pub cookie: HeaderValue,
    /// `Set-Cookie: flyff_assets=...` when assets are gated.
    pub assets_cookie: Option<HeaderValue>,
}

struct Record {
    /// Insertion order: breaks ties of equal creation times (the oldest session is the first to go).
    seq: u64,
    public_id: String,
    identity: Identity,
    snapshot: Option<AccountSnapshot>,
    created_ms: u64,
    created_unix: i64,
    last_ms: u64,
    last_unix: i64,
    asset_token: Option<String>,
}

#[derive(Default)]
struct Inner {
    by_key: BTreeMap<[u8; 32], Record>,
    next_seq: u64,
}

/// The session table.
pub struct AdminSessions {
    inner: Mutex<Inner>,
    settings: RwLock<SessionSettings>,
    clock: LobbyClock,
    assets: Option<AssetGate>,
    ended: broadcast::Sender<SessionEnded>,
}

impl std::fmt::Debug for AdminSessions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdminSessions")
            .field("live", &self.len())
            .finish_non_exhaustive()
    }
}

fn key_of(token: &str) -> [u8; 32] {
    *blake3::hash(token.as_bytes()).as_bytes()
}

fn random_token() -> Option<String> {
    let mut raw = [0u8; 32];
    getrandom::fill(&mut raw).ok()?;
    Some(base64url(&raw))
}

fn random_public_id() -> Option<String> {
    let mut raw = [0u8; 8];
    getrandom::fill(&mut raw).ok()?;
    Some(raw.iter().map(|b| format!("{b:02x}")).collect())
}

/// The value of the cookie `name` in a request's headers.
pub fn cookie_named<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    for v in headers.get_all(header::COOKIE) {
        let Ok(text) = v.to_str() else {
            continue;
        };
        for part in text.split(';') {
            if let Some((n, value)) = part.trim().split_once('=')
                && n == name
            {
                return Some(value.trim());
            }
        }
    }
    None
}

impl AdminSessions {
    /// A table over `clock` with `settings`; `assets` is `Some` where the server can issue asset cookies.
    pub fn new(
        clock: LobbyClock,
        settings: SessionSettings,
        assets: Option<AssetGate>,
    ) -> Arc<AdminSessions> {
        let (ended, _) = broadcast::channel(256);
        Arc::new(AdminSessions {
            inner: Mutex::new(Inner::default()),
            settings: RwLock::new(settings),
            clock,
            assets,
            ended,
        })
    }

    /// Changes the timeouts (`reload`).
    pub fn set_settings(&self, settings: SessionSettings) {
        *self.settings.write() = settings;
    }

    /// The timeouts in force.
    pub fn settings(&self) -> SessionSettings {
        *self.settings.read()
    }

    /// Live sessions.
    pub fn len(&self) -> usize {
        self.inner.lock().by_key.len()
    }

    /// `true` without any session.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Announces every session end (the WebSocket gateway closes the sockets of the session). A receiver that falls
    /// behind (`RecvError::Lagged`) asks [`AdminSessions::is_live`] about the sessions it serves.
    pub fn subscribe_ended(&self) -> broadcast::Receiver<SessionEnded> {
        self.ended.subscribe()
    }

    /// `true` while the session with this public handle exists (it may be past its idle timeout until the next
    /// [`AdminSessions::sweep`] or [`AdminSessions::resolve`]).
    pub fn is_live(&self, public_id: &str) -> bool {
        self.inner
            .lock()
            .by_key
            .values()
            .any(|r| r.public_id == public_id)
    }

    fn announce(&self, rec: &Record, reason: EndReason) {
        if let (Some(gate), Some(token)) = (&self.assets, &rec.asset_token) {
            gate.tokens.revoke_token(token);
        }
        // no receiver: nobody listens, which is fine
        let _ = self.ended.send(SessionEnded {
            id: rec.public_id.clone(),
            account: rec.identity.account.as_ref().map(|a| a.id),
            identity: rec.identity.label(),
            reason,
        });
    }

    /// Opens a session. `None` when the OS random source fails (the caller answers 500).
    pub fn create(&self, new: NewSession) -> Option<Created> {
        let token = random_token()?;
        let public_id = random_public_id()?;
        let now_ms = self.clock.now_ms();
        let unix = self.clock.unix().0;
        let secure = new.identity.tls;
        let mut identity = new.identity;
        identity.session = Some(SessionRef {
            id: public_id.clone(),
        });
        let asset_token = self.assets.as_ref().filter(|g| g.gated).map(|g| {
            let account = identity.account.as_ref().map_or(OWNER_ASSET_KEY, |a| a.id);
            g.tokens.issue_token(account)
        });
        let assets_cookie = asset_token
            .as_deref()
            .map(|t| asset_auth::cookie_header(t, secure));
        let mut record = Record {
            seq: 0,
            public_id,
            identity: identity.clone(),
            snapshot: new.snapshot,
            created_ms: now_ms,
            created_unix: unix,
            last_ms: now_ms,
            last_unix: unix,
            asset_token,
        };
        let mut dropped = Vec::new();
        {
            let mut inner = self.inner.lock();
            let key = identity.limit_key();
            inner.next_seq += 1;
            record.seq = inner.next_seq;
            let mut same: Vec<([u8; 32], (u64, u64))> = inner
                .by_key
                .iter()
                .filter(|(_, r)| r.identity.limit_key() == key)
                .map(|(k, r)| (*k, (r.created_ms, r.seq)))
                .collect();
            if same.len() >= MAX_SESSIONS_PER_IDENTITY {
                same.sort_by_key(|(_, order)| *order);
                let excess = same.len() + 1 - MAX_SESSIONS_PER_IDENTITY;
                for (k, _) in same.into_iter().take(excess) {
                    if let Some(r) = inner.by_key.remove(&k) {
                        dropped.push(r);
                    }
                }
            }
            inner.by_key.insert(key_of(&token), record);
        }
        for r in &dropped {
            self.announce(r, EndReason::Revoked);
        }
        let cookie = cookie_header(&token, secure);
        Some(Created {
            identity,
            cookie,
            assets_cookie,
        })
    }

    /// Looks up the session of a cookie value. A live session's idle timer restarts; one past its idle timeout or
    /// its absolute lifetime ends (`Expired`) and yields `None`. `local`, `ip` and `tls` are those of this request.
    pub fn resolve(
        &self,
        token: &str,
        ip: std::net::IpAddr,
        local: bool,
        tls: bool,
    ) -> Option<Identity> {
        if token.len() != TOKEN_LEN {
            return None;
        }
        let key = key_of(token);
        let now_ms = self.clock.now_ms();
        let unix = self.clock.unix().0;
        let s = self.settings();
        let expired = {
            let mut inner = self.inner.lock();
            let rec = inner.by_key.get_mut(&key)?;
            let idle = now_ms.saturating_sub(rec.last_ms);
            let age = now_ms.saturating_sub(rec.created_ms);
            if idle > millis(s.idle) || age > millis(s.max) {
                inner.by_key.remove(&key)
            } else {
                rec.last_ms = now_ms;
                rec.last_unix = unix;
                let mut id = rec.identity.clone();
                id.ip = ip;
                id.local = local;
                id.tls = tls;
                return Some(id);
            }
        };
        if let Some(rec) = expired {
            self.announce(&rec, EndReason::Expired);
        }
        None
    }

    /// The cookie of a request (header lookup plus [`AdminSessions::resolve`]).
    pub fn from_headers(
        &self,
        headers: &HeaderMap,
        ip: std::net::IpAddr,
        local: bool,
        tls: bool,
    ) -> Option<Identity> {
        let token = cookie_named(headers, COOKIE_NAME)?;
        self.resolve(token, ip, local, tls)
    }

    /// Clears the "must change the password" flag of a session (after `PUT /admin/api/session/password`).
    pub fn password_changed(&self, public_id: &str, new_digest: Option<[u8; 32]>) {
        let mut inner = self.inner.lock();
        for rec in inner.by_key.values_mut() {
            if rec.public_id == public_id {
                rec.identity.must_change_password = false;
                if let (Some(snap), Some(d)) = (&mut rec.snapshot, new_digest) {
                    snap.password_digest = d;
                }
            }
        }
    }

    /// Ends the session with this public handle; `true` when it existed.
    pub fn end(&self, public_id: &str, reason: EndReason) -> bool {
        let removed = {
            let mut inner = self.inner.lock();
            let key = inner
                .by_key
                .iter()
                .find(|(_, r)| r.public_id == public_id)
                .map(|(k, _)| *k);
            key.and_then(|k| inner.by_key.remove(&k))
        };
        match removed {
            Some(rec) => {
                self.announce(&rec, reason);
                true
            }
            None => false,
        }
    }

    /// Ends every session of a staff account, except the one with the public handle `keep`; returns how many ended
    /// (25 §1.8: a ban, an authority change or a password change of the account).
    pub fn end_account(&self, account: AccountId, reason: EndReason, keep: Option<&str>) -> usize {
        let removed: Vec<Record> = {
            let mut inner = self.inner.lock();
            let keys: Vec<[u8; 32]> = inner
                .by_key
                .iter()
                .filter(|(_, r)| {
                    r.identity.account.as_ref().is_some_and(|a| a.id == account)
                        && keep.is_none_or(|k| k != r.public_id)
                })
                .map(|(k, _)| *k)
                .collect();
            keys.iter().filter_map(|k| inner.by_key.remove(k)).collect()
        };
        for rec in &removed {
            self.announce(rec, reason);
        }
        removed.len()
    }

    /// Ends every session (shutdown); returns how many ended.
    pub fn end_all(&self, reason: EndReason) -> usize {
        let removed: Vec<Record> = {
            let mut inner = self.inner.lock();
            std::mem::take(&mut inner.by_key).into_values().collect()
        };
        for rec in &removed {
            self.announce(rec, reason);
        }
        removed.len()
    }

    /// Ends the sessions past their idle timeout or absolute lifetime (the sweeper, every few seconds); returns how
    /// many ended.
    pub fn sweep(&self) -> usize {
        let now_ms = self.clock.now_ms();
        let s = self.settings();
        let removed: Vec<Record> = {
            let mut inner = self.inner.lock();
            let keys: Vec<[u8; 32]> = inner
                .by_key
                .iter()
                .filter(|(_, r)| {
                    now_ms.saturating_sub(r.last_ms) > millis(s.idle)
                        || now_ms.saturating_sub(r.created_ms) > millis(s.max)
                })
                .map(|(k, _)| *k)
                .collect();
            keys.iter().filter_map(|k| inner.by_key.remove(k)).collect()
        };
        for rec in &removed {
            self.announce(rec, EndReason::Expired);
        }
        removed.len()
    }

    /// `(public id, account, snapshot)` of every staff session (the poll of the account tables).
    pub fn staff_snapshots(&self) -> Vec<(AccountId, String, Authority, AccountSnapshot)> {
        let inner = self.inner.lock();
        let mut seen = std::collections::BTreeSet::new();
        let mut out = Vec::new();
        for rec in inner.by_key.values() {
            if let (Some(a), Some(snap)) = (&rec.identity.account, &rec.snapshot)
                && seen.insert((a.id, snap.password_digest))
            {
                out.push((a.id, a.name.clone(), rec.identity.authority, snap.clone()));
            }
        }
        out
    }

    /// The sessions as the staff overview lists them (`ip` filled; the route drops it without `view.pii`).
    pub fn rows(&self) -> Vec<AdminSessionRow> {
        let inner = self.inner.lock();
        let mut rows: Vec<AdminSessionRow> = inner
            .by_key
            .values()
            .map(|r| AdminSessionRow {
                id: r.public_id.clone(),
                identity: r.identity.label(),
                kind: r.identity.kind,
                authority: r.identity.authority.letter().to_string(),
                ip: Some(r.identity.ip.to_string()),
                remote: !r.identity.local,
                created_at: r.created_unix,
                last_seen_at: r.last_unix,
            })
            .collect();
        rows.sort_by(|a, b| b.last_seen_at.cmp(&a.last_seen_at).then(a.id.cmp(&b.id)));
        rows
    }

    /// Sessions that came from another host over plain HTTP (the dashboard alert "remote admin over plain HTTP",
    /// Plan/25 §9.1).
    pub fn plain_remote_sessions(&self) -> usize {
        self.inner
            .lock()
            .by_key
            .values()
            .filter(|r| !r.identity.local && !r.identity.tls)
            .count()
    }

    /// Sessions per kind (`flyff_admin_sessions{kind}`): staff, owner (token), owner (local).
    pub fn counts(&self) -> [usize; 3] {
        let inner = self.inner.lock();
        let mut c = [0usize; 3];
        for r in inner.by_key.values() {
            let i = match r.identity.kind {
                IdentityKind::Staff => 0,
                IdentityKind::OwnerToken => 1,
                IdentityKind::OwnerLocal => 2,
            };
            if let Some(slot) = c.get_mut(i) {
                *slot += 1;
            }
        }
        c
    }

    /// `(idle expiry, absolute expiry)` in Unix seconds of the session with this handle.
    pub fn expiry(&self, public_id: &str) -> Option<(i64, i64)> {
        let s = self.settings();
        let inner = self.inner.lock();
        inner
            .by_key
            .values()
            .find(|r| r.public_id == public_id)
            .map(|r| {
                (
                    r.last_unix
                        .saturating_add(i64::try_from(s.idle.as_secs()).unwrap_or(i64::MAX)),
                    r.created_unix
                        .saturating_add(i64::try_from(s.max.as_secs()).unwrap_or(i64::MAX)),
                )
            })
    }
}

fn millis(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

/// `flyff_admin=<token>; HttpOnly; SameSite=Strict; Path=/admin[; Secure]`.
pub fn cookie_header(token: &str, secure: bool) -> HeaderValue {
    let mut text = format!("{COOKIE_NAME}={token}; HttpOnly; SameSite=Strict; Path=/admin");
    if secure {
        text.push_str("; Secure");
    }
    HeaderValue::from_str(&text).unwrap_or_else(|_| HeaderValue::from_static(""))
}

/// The `Set-Cookie` value that removes the cookie.
pub fn cookie_clear_header(secure: bool) -> HeaderValue {
    let mut text = format!("{COOKIE_NAME}=; HttpOnly; SameSite=Strict; Path=/admin; Max-Age=0");
    if secure {
        text.push_str("; Secure");
    }
    HeaderValue::from_str(&text).unwrap_or_else(|_| HeaderValue::from_static(""))
}
