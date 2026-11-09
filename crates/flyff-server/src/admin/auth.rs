//! `admin::auth` (Plan/10 §13.2, GP-07; Plan/25 §1.1-§1.3, §1.7): the one authentication rule of `/metrics`,
//! `/admin`, `/admin/ws` and `/admin/api/*`. Every request that passes gets a [`Caller`] in its extensions: the
//! client, whether it comes from the host itself, and the [`Identity`] it resolved to.
//!
//! **Who is the caller** (first match wins):
//!
//! 1. a valid `flyff_admin` cookie: the staff or owner session behind it. A cookie always wins over the implicit
//!    local owner (25 §1.1), so a staff member who signs in on the host acts as the staff identity;
//! 2. `Authorization: Bearer <content of data/admin.token>` (constant-time comparison): the owner (`owner:token`),
//!    for REST tools; not accepted on `/admin/ws`. A wrong token from a remote client is charged to the
//!    failure limiter (60 per second and address, `429`) and to the gateway's per-IP upgrade bucket; from the host
//!    itself it is ignored;
//! 3. the peer is loopback **and** the request has no `Forwarded`, `X-Forwarded-For` or `X-Real-IP` header **and** the
//!    `Host` is `localhost`, `127.0.0.1` or `[::1]` (the DNS-rebinding guard), and `admin.owner_loopback` is on:
//!    the local owner (`owner:local`, ADMINISTRATOR);
//! 4. nobody: only the HTML shells (`/admin`, `/admin/lite`) and the session routes (`/admin/api/session*`, the login)
//!    are served; everything else answers `401`.
//!
//! A request that is not from the host itself needs `admin.allow_remote = true` (else `403` for everything).
//!
//! **Rules on top** (25 §1.2, §1.3): mutating requests (`POST`, `PUT`, `DELETE`, `PATCH`) need
//! `Content-Type: application/json` and, when an `Origin` header is present, an `Origin` equal to the request's own
//! origin, else `403`; a mutation authenticated by a cookie also needs `X-Flyff-Admin: 1`; the WebSocket upgrade checks
//! `Origin`; while the account's `must_change_password` flag is set every route except the session routes answers
//! `428 password_change_required`. Every response carries `Cache-Control: no-store`.
//!
//! Errors under `/admin/api/*` are the JSON envelope of 25 §11.3; the other paths answer short plain texts.
//!
//! The token file `data/admin.token` is created at start-up ([`ensure_admin_token`]): 32 random bytes,
//! base64url, mode 0600.

use super::error::ApiError;
use super::identity::Identity;
use super::metrics::AdminMetrics;
use super::session::{AdminSessions, SessionSettings};
use crate::http::ClientInfo;
use crate::lobby::online::LobbyClock;
use axum::Router;
use axum::extract::{FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::{Next, from_fn_with_state};
use axum::response::{IntoResponse, Response};
use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::net::IpAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// File name of the admin token inside `data/`.
pub const ADMIN_TOKEN_FILE: &str = "admin.token";

/// Failed token attempts per client IP and second before the answer becomes 429.
const MAX_FAILURES_PER_SEC: u32 = 60;

/// The header every cookie-authenticated mutation must carry (25 §1.3).
pub const CSRF_HEADER: &str = "x-flyff-admin";

/// What the authentication layer resolved a request to (inserted into the request extensions).
#[derive(Clone, Debug)]
pub struct Caller {
    /// The client (address, scheme).
    pub client: ClientInfo,
    /// The request comes from the host itself.
    pub local: bool,
    /// Who it is; `None` for the anonymous requests the layer lets through (the shells and the session routes).
    pub identity: Option<Identity>,
}

/// Extractor: the identity of the request (`401 unauthorized` without one).
#[derive(Clone, Debug)]
pub struct Who(pub Identity);

impl<S: Send + Sync> FromRequestParts<S> for Who {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Caller>()
            .and_then(|c| c.identity.clone())
            .map(Who)
            .ok_or_else(ApiError::unauthorized)
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Caller {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Caller>()
            .cloned()
            .ok_or_else(ApiError::unauthorized)
    }
}

/// The request classes of 25 §1.2.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    /// `/admin`, `/admin/`, `/admin/lite` and the other pages under `/admin`.
    Shell,
    /// `/admin/api/session` and below.
    Session,
    /// The other `/admin/api/*`.
    Api,
    /// `/admin/ws`.
    Ws,
    /// `/metrics` and anything else behind the layer.
    Other,
}

impl Class {
    fn of(path: &str) -> Class {
        if path == "/admin/api/session" || path.starts_with("/admin/api/session/") {
            Class::Session
        } else if path.starts_with("/admin/api/") || path == "/admin/api" {
            Class::Api
        } else if path == "/admin/ws" {
            Class::Ws
        } else if path == "/admin" || path.starts_with("/admin/") {
            Class::Shell
        } else {
            Class::Other
        }
    }

    /// The cookie has `Path=/admin`: only these requests carry it.
    fn uses_cookie(self) -> bool {
        !matches!(self, Class::Other)
    }

    /// A bearer token is not accepted on the WebSocket (browsers cannot send it).
    fn bearer_ok(self) -> bool {
        !matches!(self, Class::Ws)
    }

    /// The classes served without an identity.
    fn anonymous_ok(self) -> bool {
        matches!(self, Class::Shell | Class::Session)
    }

    fn json_errors(self) -> bool {
        matches!(self, Class::Session | Class::Api)
    }
}

/// The shared authorization state.
#[derive(Debug)]
pub struct AdminAuth {
    allow_remote: bool,
    owner_loopback: bool,
    token: Vec<u8>,
    failures: Mutex<BTreeMap<IpAddr, (Instant, u32)>>,
    /// Every failed token attempt also takes a token of the `/ws` upgrade bucket of that address (10 §13.2).
    limiter: Option<Arc<crate::gateway::IpLimiter>>,
    /// The admin sessions behind the `flyff_admin` cookie.
    pub sessions: Arc<AdminSessions>,
    /// The admin console's counters (failed logins).
    pub metrics: Arc<AdminMetrics>,
}

/// The verdict of [`AdminAuth::check`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Let the request through.
    Allow,
    /// 401: a remote caller without (or with a wrong) token.
    Unauthorized,
    /// 403: remote access is disabled, or the request breaks a rule of the table.
    Forbidden,
    /// 429: too many failed token attempts from this IP.
    TooMany,
}

/// Why a request was refused (the response is built from it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Denial {
    /// 401.
    Unauthorized,
    /// 403, the plain rule.
    Forbidden,
    /// 403: a cookie-authenticated mutation without the CSRF header.
    Csrf,
    /// 403: a mutation (`POST`, `PUT`, `PATCH`, `DELETE`, a bodyless one too) without `Content-Type: application/json`
    /// (10 §13: the one rule that keeps a form or a `text/plain` post out).
    NotJson,
    /// 429.
    TooMany,
    /// 428: the password must be changed first.
    PasswordChange,
}

impl Denial {
    fn verdict(&self) -> Verdict {
        match self {
            Denial::Unauthorized => Verdict::Unauthorized,
            Denial::TooMany => Verdict::TooMany,
            Denial::Forbidden | Denial::Csrf | Denial::NotJson | Denial::PasswordChange => {
                Verdict::Forbidden
            }
        }
    }

    fn response(&self, json: bool) -> Response {
        if json {
            let err = match self {
                Denial::Unauthorized => ApiError::unauthorized(),
                Denial::Forbidden => ApiError::forbidden_text("admin access denied"),
                Denial::Csrf => {
                    ApiError::forbidden_text("mutations need the X-Flyff-Admin: 1 header")
                }
                Denial::NotJson => {
                    ApiError::forbidden_text("mutations need Content-Type: application/json")
                }
                Denial::TooMany => ApiError::rate_limited(1, "too many attempts"),
                Denial::PasswordChange => ApiError::password_change_required(),
            };
            let mut r = err.into_response();
            if matches!(self, Denial::Unauthorized) {
                r.headers_mut().insert(
                    header::WWW_AUTHENTICATE,
                    HeaderValue::from_static("Bearer realm=\"flyff-admin\""),
                );
            }
            return r;
        }
        match self {
            Denial::Unauthorized => reject(StatusCode::UNAUTHORIZED, "admin token required"),
            Denial::Forbidden | Denial::Csrf => {
                reject(StatusCode::FORBIDDEN, "admin access denied")
            }
            Denial::NotJson => reject(
                StatusCode::FORBIDDEN,
                "mutations need Content-Type: application/json",
            ),
            Denial::TooMany => reject(StatusCode::TOO_MANY_REQUESTS, "too many attempts"),
            Denial::PasswordChange => reject(
                StatusCode::PRECONDITION_REQUIRED,
                "password change required",
            ),
        }
    }
}

/// Everything [`AdminAuth::build`] needs.
pub struct AdminAuthInit {
    /// `admin.allow_remote`.
    pub allow_remote: bool,
    /// `admin.owner_loopback`.
    pub owner_loopback: bool,
    /// The content of `data/admin.token` (empty = no token access).
    pub token: String,
    /// The gateway's per-IP limiter (failed token attempts are charged to it).
    pub limiter: Option<Arc<crate::gateway::IpLimiter>>,
    /// The session table.
    pub sessions: Arc<AdminSessions>,
    /// The counters.
    pub metrics: Arc<AdminMetrics>,
}

impl AdminAuth {
    /// The full constructor.
    pub fn build(init: AdminAuthInit) -> Arc<AdminAuth> {
        Arc::new(AdminAuth {
            allow_remote: init.allow_remote,
            owner_loopback: init.owner_loopback,
            token: init.token.trim().as_bytes().to_vec(),
            failures: Mutex::new(BTreeMap::new()),
            limiter: init.limiter,
            sessions: init.sessions,
            metrics: init.metrics,
        })
    }

    /// `token` is the content of `data/admin.token` (empty = remote access is impossible). The local owner is on and
    /// the session table is private to this instance.
    pub fn new(allow_remote: bool, token: &str) -> Arc<AdminAuth> {
        Self::build(AdminAuthInit {
            allow_remote,
            owner_loopback: true,
            token: token.to_owned(),
            limiter: None,
            sessions: AdminSessions::new(LobbyClock::system(), SessionSettings::default(), None),
            metrics: Arc::new(AdminMetrics::default()),
        })
    }

    /// Like [`AdminAuth::new`], charging every failed token attempt to the gateway's per-IP limiter
    /// (`IpLimiter::charge`, 10 §13.2 rule 2: repeated failures also lose the upgrade budget of the address).
    pub fn with_limiter(
        allow_remote: bool,
        token: &str,
        limiter: Arc<crate::gateway::IpLimiter>,
    ) -> Arc<AdminAuth> {
        Self::build(AdminAuthInit {
            allow_remote,
            owner_loopback: true,
            token: token.to_owned(),
            limiter: Some(limiter),
            sessions: AdminSessions::new(LobbyClock::system(), SessionSettings::default(), None),
            metrics: Arc::new(AdminMetrics::default()),
        })
    }

    /// `admin.allow_remote`.
    pub fn allow_remote(&self) -> bool {
        self.allow_remote
    }

    /// `admin.owner_loopback`.
    pub fn owner_loopback(&self) -> bool {
        self.owner_loopback
    }

    /// Compares a presented owner token in constant time (the login route's token exchange).
    pub fn token_matches(&self, presented: &str) -> bool {
        !self.token.is_empty() && constant_time_eq(presented.trim().as_bytes(), &self.token)
    }

    /// Charges a failed owner-token attempt of `ip` (the window of 60 per second, the gateway's upgrade bucket).
    /// Returns `true` when the address is throttled now.
    pub fn token_failed(&self, ip: IpAddr, now: Instant) -> bool {
        self.record_failure(ip, now);
        if let Some(l) = &self.limiter {
            l.charge(ip);
        }
        self.is_throttled(ip, now)
    }

    /// `true` while `ip` has used up its failed-attempt window.
    pub fn throttled(&self, ip: IpAddr, now: Instant) -> bool {
        self.is_throttled(ip, now)
    }

    /// Applies the rules above to a request (`now` is injected for the failure window).
    pub fn authenticate(
        &self,
        req: &Request,
        client: &ClientInfo,
        now: Instant,
    ) -> Result<Caller, Denial> {
        let headers = req.headers();
        let class = Class::of(req.uri().path());
        let local = trusted_local(headers, client.peer);
        if !local && !self.allow_remote {
            return Err(Denial::Forbidden);
        }
        let mutating = is_mutating(req.method());

        // 1. a valid session cookie wins over everything (25 §1.1)
        let mut identity = None;
        let mut by_cookie = false;
        if class.uses_cookie()
            && let Some(id) = self
                .sessions
                .from_headers(headers, client.ip, local, client.https)
        {
            identity = Some(id);
            by_cookie = true;
        }
        // 2. the bearer token
        if identity.is_none() {
            if !local && self.is_throttled(client.ip, now) {
                return Err(Denial::TooMany);
            }
            let presented = headers
                .get(header::AUTHORIZATION)
                .and_then(|h| h.to_str().ok())
                .and_then(|h| h.strip_prefix("Bearer "))
                .map(str::as_bytes);
            if class.bearer_ok()
                && let Some(p) = presented
            {
                let ok = !self.token.is_empty() && constant_time_eq(p, &self.token);
                if ok {
                    identity = Some(Identity::owner_token(client.ip, local, client.https, None));
                } else if !local {
                    self.record_failure(client.ip, now);
                    if let Some(l) = &self.limiter {
                        l.charge(client.ip);
                    }
                    return Err(Denial::Unauthorized);
                }
                // a wrong token from the host itself is ignored: the local owner rule decides
            }
        }
        // 3. the implicit owner of the host
        if identity.is_none() && local && self.owner_loopback {
            identity = Some(Identity::owner_local(client.ip, client.https));
        }
        // 4. nobody
        if identity.is_none() && !class.anonymous_ok() {
            if !local {
                self.record_failure(client.ip, now);
                if let Some(l) = &self.limiter {
                    l.charge(client.ip);
                }
            }
            return Err(Denial::Unauthorized);
        }

        // rules on top
        if mutating {
            let json = headers
                .get(header::CONTENT_TYPE)
                .and_then(|h| h.to_str().ok())
                .is_some_and(is_json);
            if !json {
                return Err(Denial::NotJson);
            }
            if !origin_matches(headers) {
                return Err(Denial::Forbidden);
            }
            if by_cookie && !csrf_header_ok(headers) {
                return Err(Denial::Csrf);
            }
        }
        if class == Class::Ws && !origin_matches(headers) {
            return Err(Denial::Forbidden);
        }
        if identity.as_ref().is_some_and(|i| i.must_change_password)
            && matches!(class, Class::Api | Class::Ws)
        {
            return Err(Denial::PasswordChange);
        }
        Ok(Caller {
            client: client.clone(),
            local,
            identity,
        })
    }

    /// Applies the rules to a request and reduces the answer to a [`Verdict`] (the unit tests of the rules).
    pub fn check(&self, req: &Request, client: &ClientInfo, now: Instant) -> Verdict {
        match self.authenticate(req, client, now) {
            Ok(_) => Verdict::Allow,
            Err(d) => d.verdict(),
        }
    }

    fn is_throttled(&self, ip: IpAddr, now: Instant) -> bool {
        let map = self.failures.lock();
        map.get(&ip).is_some_and(|(start, n)| {
            now.duration_since(*start) < Duration::from_secs(1) && *n >= MAX_FAILURES_PER_SEC
        })
    }

    fn record_failure(&self, ip: IpAddr, now: Instant) {
        let mut map = self.failures.lock();
        if map.len() > 4096 {
            map.retain(|_, (start, _)| now.duration_since(*start) < Duration::from_secs(1));
        }
        let e = map.entry(ip).or_insert((now, 0));
        if now.duration_since(e.0) >= Duration::from_secs(1) {
            *e = (now, 0);
        }
        e.1 = e.1.saturating_add(1);
    }

    /// Wraps `router` in the authorization middleware (every response gets `Cache-Control: no-store`).
    pub fn layer(self: &Arc<Self>, router: Router) -> Router {
        router.layer(from_fn_with_state(Arc::clone(self), middleware))
    }
}

async fn middleware(State(auth): State<Arc<AdminAuth>>, mut req: Request, next: Next) -> Response {
    let class = Class::of(req.uri().path());
    let client = req.extensions().get::<ClientInfo>().cloned();
    let decision = match &client {
        Some(c) => auth.authenticate(&req, c, Instant::now()),
        // No client info means the request did not come through the HTTP stack: refuse.
        None => Err(Denial::Forbidden),
    };
    let mut resp = match decision {
        Ok(caller) => {
            req.extensions_mut().insert(caller);
            next.run(req).await
        }
        Err(d) => d.response(class.json_errors()),
    };
    resp.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
}

fn reject(status: StatusCode, text: &'static str) -> Response {
    let mut r = (status, text).into_response();
    if status == StatusCode::UNAUTHORIZED {
        r.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            HeaderValue::from_static("Bearer realm=\"flyff-admin\""),
        );
    }
    if status == StatusCode::TOO_MANY_REQUESTS {
        r.headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
    }
    r
}

fn is_mutating(m: &Method) -> bool {
    matches!(
        *m,
        Method::POST | Method::PUT | Method::DELETE | Method::PATCH
    )
}

fn is_json(content_type: &str) -> bool {
    content_type
        .split(';')
        .next()
        .is_some_and(|t| t.trim().eq_ignore_ascii_case("application/json"))
}

/// An `Origin` header, when present, must name the request's own host (scheme-insensitive: the stack's layer 5 checks
/// the scheme).
fn origin_matches(headers: &axum::http::HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN) else {
        return true;
    };
    let own = headers.get(header::HOST).and_then(|h| h.to_str().ok());
    origin
        .to_str()
        .ok()
        .and_then(|o| o.split_once("://").map(|(_, rest)| rest))
        .zip(own)
        .is_some_and(|(o, h)| o.eq_ignore_ascii_case(h))
}

fn csrf_header_ok(headers: &axum::http::HeaderMap) -> bool {
    headers
        .get(CSRF_HEADER)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.trim() == "1")
}

/// The loopback rule of 10 §13.2 (rule 1): the TCP peer is loopback **and** the request has no `Forwarded`,
/// `X-Forwarded-For` or `X-Real-IP` header **and** the `Host` is local (the DNS-rebinding guard). The asset gate
/// evaluates it too (25 §11.7).
pub fn trusted_local(headers: &axum::http::HeaderMap, peer: IpAddr) -> bool {
    let local_host = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .is_some_and(is_local_host);
    let forwarded = ["forwarded", "x-forwarded-for", "x-real-ip"]
        .iter()
        .any(|h| headers.contains_key(*h));
    peer.is_loopback() && !forwarded && local_host
}

/// `localhost`, `127.0.0.1` or `[::1]` with an optional port.
fn is_local_host(host: &str) -> bool {
    let name = if let Some(rest) = host.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else {
        host.split(':').next().unwrap_or("")
    };
    name.eq_ignore_ascii_case("localhost") || name == "127.0.0.1" || name == "::1"
}

/// Compares without an early exit on the first differing byte (the lengths may leak).
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// base64url without padding.
pub fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk.first().copied().unwrap_or(0));
        let b1 = u32::from(chunk.get(1).copied().unwrap_or(0));
        let b2 = u32::from(chunk.get(2).copied().unwrap_or(0));
        let n = (b0 << 16) | (b1 << 8) | b2;
        let sextets = chunk.len() + 1;
        for i in 0..sextets {
            let idx = ((n >> (18 - 6 * i)) & 63) as usize;
            if let Some(c) = ALPHABET.get(idx) {
                out.push(char::from(*c));
            }
        }
    }
    out
}

/// Reads `<data_dir>/admin.token`, creating it (32 random bytes, base64url, mode 0600) when missing or empty.
/// Returns the token text.
pub fn ensure_admin_token(data_dir: &Path) -> std::io::Result<String> {
    let path = data_dir.join(ADMIN_TOKEN_FILE);
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let t = existing.trim();
        if !t.is_empty() {
            return Ok(t.to_owned());
        }
    }
    let mut raw = [0u8; 32];
    getrandom::fill(&mut raw).map_err(|e| std::io::Error::other(format!("OS random: {e}")))?;
    let token = base64url(&raw);
    write_private(&path, token.as_bytes())?;
    Ok(token)
}

/// Writes `bytes` to `path` with mode 0600 (created with that mode; an existing file is replaced).
pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let tmp = path.with_extension("tmp");
    let _ = std::fs::remove_file(&tmp);
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    drop(f);
    std::fs::rename(&tmp, path)
}
