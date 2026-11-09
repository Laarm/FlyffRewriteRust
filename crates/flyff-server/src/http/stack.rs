//! The middleware stack (Plan/24 §3.3), outermost first:
//!
//! | layer | here | behaviour |
//! |---|---|---|
//! | 0 | [`edge`] + [`ip_block`] | client IP and scheme (§3.2) into the request extensions; `net.ip_block` → `403` before anything is counted |
//! | 1 | [`edge`] | access log (`flyff::http::access` at `debug`, `warn` above 2 s), counters, `X-Request-Id` |
//! | 2 | [`edge`] | security headers on every response, the `403`/`429`/`500` of the inner layers included |
//! | 3 | [`catch_panic`] | a handler panic is `500`, counted; the game thread is unaffected (not applied to `/ws`) |
//! | 4 | [`rate_limit`] | per-IP token buckets: `/api/*` the `api` class, everything but `/ws`, `/healthz`, `/metrics`, `/admin*` the `static` class |
//! | 5 | [`origin_check`] | state-changing requests under `/api/*` and `/admin/api/*` need an allowed `Origin` |
//! | 6 | [`timeout`] | `/api/*` 15 s; `/gen/*` `assets.interactive_timeout_s` + 5 s |
//! | 7 | [`body_limit`] | `/api/*` and `/admin/api/*` 16 KiB (grants v2 and the templates 256 KiB), every other route accepts no body |
//!
//! Layer 0 and 1-2 are one function so that the headers of §3.4 reach the responses of the block list as
//! well: the block list still runs before any rate-limit accounting. The asset gate (layer 8) is applied by the
//! asset handlers themselves (`asset_auth`), which know the resolved target.

use super::client_ip::{self, ClientInfo};
use super::error;
use super::origin;
use super::ratelimit::Class;
use super::state::HttpState;
use super::telemetry::{Family, RateClass, RejectReason};
use axum::body::Body;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::Response;
use futures_util::FutureExt;
use std::net::{IpAddr, SocketAddr};
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Largest body of `/api/*` and `/admin/api/*` requests.
pub const API_BODY_LIMIT: usize = 16 * 1024;
/// Timeout of `/api/*` requests.
pub const API_TIMEOUT: Duration = Duration::from_secs(15);
/// Requests slower than this are logged at `warn` (except `/ws` and `/gen`).
const SLOW_REQUEST: Duration = Duration::from_secs(2);

/// Marker of requests that must not be unwound-caught (set for `/ws`), and the family of the request.
fn family_of(req: &Request) -> Family {
    Family::of_path(req.uri().path())
}

fn info_of(req: &Request) -> Option<&ClientInfo> {
    req.extensions().get::<ClientInfo>()
}

static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(1);

fn request_id(boot: u32) -> String {
    format!(
        "{boot:08x}{:08x}",
        REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed) & 0xffff_ffff
    )
}

/// A per-process random prefix of request ids.
fn boot_nonce() -> u32 {
    static NONCE: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *NONCE.get_or_init(|| {
        let mut b = [0u8; 4];
        let _ = getrandom::fill(&mut b);
        u32::from_le_bytes(b)
    })
}

/// Layers 0-2: client info, request id, security headers, counters and the access log.
pub async fn edge(State(st): State<HttpState>, mut req: Request, next: Next) -> Response {
    let started = Instant::now();
    let peer = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map_or_else(|| SocketAddr::from(([0, 0, 0, 0], 0)), |c| c.0);
    let tls = st.cfg.tls
        || req
            .extensions()
            .get::<super::serve::TlsConnection>()
            .is_some();
    let info = client_ip::resolve(req.headers(), peer, &st.cfg.trusted_proxies, tls);
    req.extensions_mut().insert(info.clone());
    let method = req.method().clone();
    let path = req.uri().path().to_owned();
    let family = Family::of_path(&path);
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let id = request_id(boot_nonce());

    let mut resp = next.run(req).await;

    super::headers::apply(
        resp.headers_mut(),
        st.cfg.header_policy(),
        &path,
        host.as_deref(),
        info.https,
    );
    if let Ok(v) = HeaderValue::from_str(&id) {
        resp.headers_mut().insert("x-request-id", v);
    }
    let status = resp.status().as_u16();
    let bytes = resp
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    let gzip = resp
        .headers()
        .get(header::CONTENT_ENCODING)
        .is_some_and(|v| v == "gzip");
    st.telemetry.request(
        family,
        status,
        if method == Method::HEAD { 0 } else { bytes },
        gzip,
    );
    let elapsed = started.elapsed();
    let ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
    if elapsed > SLOW_REQUEST && !matches!(family, Family::Ws | Family::Gen) {
        tracing::warn!(target: "flyff::http::access", %method, family = family.name(), status, ms, bytes, ip = %info.ip, req = %id, path = %path, "slow request");
    } else {
        tracing::debug!(target: "flyff::http::access", %method, family = family.name(), status, ms, bytes, enc = if gzip { "gzip" } else { "identity" }, ip = %info.ip, req = %id, path = %path);
    }
    resp
}

/// Layer 0: the `net.ip_block` list (a loopback client is never blocked).
pub async fn ip_block(State(st): State<HttpState>, req: Request, next: Next) -> Response {
    if let Some(info) = info_of(&req)
        && st.live.is_blocked(info.ip)
    {
        st.telemetry.conn_rejected(RejectReason::IpBlock);
        if family_of(&req) == Family::Ws {
            st.telemetry.ws_blocked();
        }
        return error::forbidden("forbidden");
    }
    next.run(req).await
}

/// Layer 3: a panicking handler becomes `500`.
pub async fn catch_panic(State(st): State<HttpState>, req: Request, next: Next) -> Response {
    if family_of(&req) == Family::Ws {
        return next.run(req).await;
    }
    let path = req.uri().path().to_owned();
    match AssertUnwindSafe(next.run(req)).catch_unwind().await {
        Ok(r) => r,
        Err(payload) => {
            st.telemetry.panic();
            let what = payload
                .downcast_ref::<&str>()
                .map(|s| (*s).to_owned())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "non-string panic payload".to_owned());
            tracing::error!(path = %path, "handler panic: {what}");
            error::plain(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
        }
    }
}

/// Layer 4: per-IP rate classes.
pub async fn rate_limit(State(st): State<HttpState>, req: Request, next: Next) -> Response {
    let family = family_of(&req);
    let class = match family {
        Family::Api => Class::Api,
        Family::Ws | Family::Health | Family::Metrics | Family::Admin => {
            return next.run(req).await;
        }
        _ => Class::Static,
    };
    let ip = info_of(&req).map_or(IpAddr::from([0, 0, 0, 0]), |i| i.ip);
    match st.limits.check(ip, class) {
        Ok(()) => next.run(req).await,
        Err(retry) => {
            st.telemetry.rate_limited(match class {
                Class::Api => RateClass::Api,
                Class::Static => RateClass::Static,
            });
            if class == Class::Api {
                let mut r = Response::new(Body::from("{\"error\":\"rate_limited\"}"));
                *r.status_mut() = StatusCode::TOO_MANY_REQUESTS;
                r.headers_mut().insert(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("application/json"),
                );
                r.headers_mut()
                    .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
                if let Ok(v) = HeaderValue::from_str(&retry.to_string()) {
                    r.headers_mut().insert(header::RETRY_AFTER, v);
                }
                r
            } else {
                error::rate_limited(1)
            }
        }
    }
}

fn is_api_path(path: &str) -> bool {
    path.starts_with("/api/") || path == "/api" || path.starts_with("/admin/api/")
}

/// Layer 5: the Origin check of §3.5 for state-changing requests under `/api/*` and `/admin/api/*`.
pub async fn origin_check(State(st): State<HttpState>, req: Request, next: Next) -> Response {
    let state_changing = !matches!(
        *req.method(),
        Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE
    );
    if state_changing && is_api_path(req.uri().path()) {
        let origin_hdr = req
            .headers()
            .get(header::ORIGIN)
            .and_then(|v| v.to_str().ok());
        let host = req
            .headers()
            .get(header::HOST)
            .and_then(|v| v.to_str().ok());
        let scheme = info_of(&req).map_or("http", ClientInfo::scheme);
        let allowed = st.live.allowed_origins();
        if !origin::is_allowed(origin_hdr, host, Some(scheme), &allowed, st.cfg.dev) {
            return error::forbidden("forbidden origin");
        }
    }
    next.run(req).await
}

/// Layer 6: request timeouts (`/api/*` 15 s, `/gen/*` the interactive wait plus 5 s).
pub async fn timeout(State(st): State<HttpState>, req: Request, next: Next) -> Response {
    let limit = match family_of(&req) {
        Family::Api => Some(API_TIMEOUT),
        Family::Gen => Some(st.cfg.interactive_timeout() + Duration::from_secs(5)),
        _ => None,
    };
    match limit {
        None => next.run(req).await,
        Some(d) => match tokio::time::timeout(d, next.run(req)).await {
            Ok(r) => r,
            Err(_) => error::unavailable("timeout", 2),
        },
    }
}

/// Largest body of the admin routes that carry baskets and imports (25 §11.3): grants v2, the templates and kits.
pub const ADMIN_BIG_BODY_LIMIT: usize = 256 * 1024;

/// The admin routes with the 256 KiB limit.
fn is_big_admin_body(path: &str) -> bool {
    path == "/admin/api/grants/v2"
        || path == "/admin/api/templates"
        || path.starts_with("/admin/api/templates/")
}

/// Layer 7: `/api/*` and `/admin/api/*` accept 16 KiB (the basket and import routes 256 KiB), every other route
/// accepts no body.
pub async fn body_limit(req: Request, next: Next) -> Response {
    let limit = if is_big_admin_body(req.uri().path()) {
        ADMIN_BIG_BODY_LIMIT
    } else if is_api_path(req.uri().path()) {
        API_BODY_LIMIT
    } else {
        0
    };
    let declared = req
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    if declared.is_some_and(|n| n > limit as u64) {
        return error::plain(StatusCode::PAYLOAD_TOO_LARGE, "payload too large");
    }
    let (parts, body) = req.into_parts();
    let body = Body::new(http_body_util::Limited::new(body, limit));
    next.run(Request::from_parts(parts, body)).await
}
