//! Security headers (Plan/24 §3.4), added to every response including errors by the outermost layer.
//!
//! | header | applies to |
//! |---|---|
//! | `X-Content-Type-Options: nosniff`, `Referrer-Policy: same-origin`, `X-Frame-Options: DENY` | all |
//! | `Cross-Origin-Resource-Policy: same-origin` | `/assets`, `/data`, `/res`, `/gen` |
//! | `Cross-Origin-Opener-Policy`, `Permissions-Policy`, `Content-Security-Policy` | HTML documents |
//! | `Strict-Transport-Security` | TLS (or `X-Forwarded-Proto: https` from a trusted proxy), `web.hsts_max_age_s > 0` |
//!
//! `Cross-Origin-Embedder-Policy` is never set (no `SharedArrayBuffer`, 01 C1). A response that already carries
//! its own CSP (`/admin`) keeps it. `web.csp = false` drops the CSP header.

use axum::http::{HeaderMap, HeaderName, HeaderValue, header};

/// The settings of [`apply`] (`[web]` keys).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeaderPolicy {
    /// `web.csp`.
    pub csp: bool,
    /// `web.hsts_max_age_s`; 0 = off.
    pub hsts_max_age_s: u32,
}

impl Default for HeaderPolicy {
    fn default() -> Self {
        Self {
            csp: true,
            hsts_max_age_s: 0,
        }
    }
}

const PERMISSIONS_POLICY: &str =
    "camera=(), microphone=(), geolocation=(), payment=(), usb=(), serial=(), hid=()";

/// Is `host` safe to splice into a CSP source list? (host name, IPv4, or bracketed IPv6, with an optional port.)
pub fn csp_host_ok(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 255
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':' | b'[' | b']'))
}

/// The `Content-Security-Policy` value for a page served to `host` (the validated `Host` header).
pub fn csp_value(host: Option<&str>) -> String {
    let connect = match host.filter(|h| csp_host_ok(h)) {
        Some(h) => format!("'self' ws://{h} wss://{h}"),
        None => "'self'".to_owned(),
    };
    format!(
        "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; \
         img-src 'self' data: blob:; font-src 'self'; connect-src {connect}; worker-src 'self' blob:; \
         media-src 'self' blob:; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'"
    )
}

/// The `Content-Security-Policy` of the admin console `/admin` (Plan/25 §11.2) for a page served to `host`: no
/// default source, scripts and styles from the own origin (inline styles stay: the console sets style attributes),
/// data/blob images, workers from blobs, the WebSocket `/admin/ws` on the validated `Host`, no framing, no `<base>`.
pub fn admin_csp_value(host: Option<&str>) -> String {
    let connect = match host.filter(|h| csp_host_ok(h)) {
        Some(h) => format!("'self' ws://{h} wss://{h}"),
        None => "'self'".to_owned(),
    };
    format!(
        "default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; \
         img-src 'self' data: blob:; connect-src {connect}; worker-src 'self' blob:; font-src 'self'; \
         object-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'"
    )
}

/// The `Content-Security-Policy` of the rescue page `/admin/lite` (Plan/25 §9.10): one inline script and style, requests to
/// the own origin only, no framing.
pub const LITE_CSP: &str = "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; \
                            connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

fn insert(h: &mut HeaderMap, name: HeaderName, value: &'static str) {
    h.insert(name, HeaderValue::from_static(value));
}

/// Adds the headers of §3.4 to a response. `path` is the request path, `host` its `Host` header, `https`
/// whether the request arrived over TLS (or through a trusted TLS-terminating proxy).
pub fn apply(
    headers: &mut HeaderMap,
    policy: HeaderPolicy,
    path: &str,
    host: Option<&str>,
    https: bool,
) {
    insert(headers, header::X_CONTENT_TYPE_OPTIONS, "nosniff");
    insert(headers, header::REFERRER_POLICY, "same-origin");
    insert(headers, header::X_FRAME_OPTIONS, "DENY");
    let first = path.trim_start_matches('/').split('/').next().unwrap_or("");
    if matches!(first, "assets" | "data" | "res" | "gen") {
        insert(
            headers,
            HeaderName::from_static("cross-origin-resource-policy"),
            "same-origin",
        );
    }
    let is_html = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("text/html"));
    if is_html {
        insert(
            headers,
            HeaderName::from_static("cross-origin-opener-policy"),
            "same-origin",
        );
        insert(
            headers,
            HeaderName::from_static("permissions-policy"),
            PERMISSIONS_POLICY,
        );
        if policy.csp
            && !headers.contains_key(header::CONTENT_SECURITY_POLICY)
            && let Ok(v) = HeaderValue::from_str(&csp_value(host))
        {
            headers.insert(header::CONTENT_SECURITY_POLICY, v);
        }
    }
    if https
        && policy.hsts_max_age_s > 0
        && let Ok(v) = HeaderValue::from_str(&format!("max-age={}", policy.hsts_max_age_s))
    {
        headers.insert(header::STRICT_TRANSPORT_SECURITY, v);
    }
}
