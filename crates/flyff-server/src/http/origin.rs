//! The Origin rule (Plan/24 §3.5, 11 §1.1): CSRF hygiene for state-changing requests and the WebSocket upgrade.
//!
//! Implemented once, here: [`is_allowed`]. A request passes when it carries no `Origin` header (non-browser
//! clients such as `flyff-bot`), when the origin equals `<scheme>://<Host>` of the request, when it is listed in
//! `net.allowed_origins`, or (only with `--dev`) when it is one of the two Vite dev origins.

/// The Vite dev-server origins (Plan/02 §7.2) that pass the check only with `--dev`.
pub const DEV_ORIGINS: [&str; 2] = ["http://localhost:5173", "http://127.0.0.1:5173"];

/// May a request with this `Origin` header be served?
///
/// * `origin`: the `Origin` header, `None` when absent;
/// * `host`: the `Host` header;
/// * `scheme`: the scheme of the request (`"http"` / `"https"`, 24 §3.2). `None` compares the authority only
///   (callers that cannot know the scheme);
/// * `allowed`: `net.allowed_origins`;
/// * `dev`: `--dev` is active.
pub fn is_allowed(
    origin: Option<&str>,
    host: Option<&str>,
    scheme: Option<&str>,
    allowed: &[String],
    dev: bool,
) -> bool {
    let Some(origin) = origin else {
        return true;
    };
    let origin = origin.trim().trim_end_matches('/');
    if allowed
        .iter()
        .any(|a| a.trim().trim_end_matches('/').eq_ignore_ascii_case(origin))
    {
        return true;
    }
    if dev && DEV_ORIGINS.iter().any(|d| d.eq_ignore_ascii_case(origin)) {
        return true;
    }
    let Some((origin_scheme, authority)) = origin.split_once("://") else {
        return false;
    };
    let Some(host) = host else {
        return false;
    };
    if !host.trim().eq_ignore_ascii_case(authority) {
        return false;
    }
    match scheme {
        Some(s) => s.eq_ignore_ascii_case(origin_scheme),
        None => {
            origin_scheme.eq_ignore_ascii_case("http")
                || origin_scheme.eq_ignore_ascii_case("https")
        }
    }
}
