//! Error responses (Plan/24 §3.8): `text/plain; charset=utf-8` one-liners, `X-Flyff-Asset-Error` for asset
//! build failures. `/api/*` errors are JSON and owned by 12 (except R8).

use axum::body::Body;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;
use flyff_assets::{AssetError, AssetErrorKind};

/// The header that names the class of an asset build failure.
pub const ASSET_ERROR_HEADER: &str = "x-flyff-asset-error";

/// A `text/plain` response with `body` (a newline is appended).
pub fn plain(status: StatusCode, body: &str) -> Response {
    let mut text = String::with_capacity(body.len() + 1);
    text.push_str(body);
    text.push('\n');
    let mut resp = Response::new(Body::from(text));
    *resp.status_mut() = status;
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    resp.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
}

/// `404 not found` (RT1).
pub fn not_found() -> Response {
    plain(StatusCode::NOT_FOUND, "not found")
}

/// `400 bad path`.
pub fn bad_path() -> Response {
    plain(StatusCode::BAD_REQUEST, "bad path")
}

/// `401 login required` (§13).
pub fn login_required() -> Response {
    plain(StatusCode::UNAUTHORIZED, "login required")
}

/// `403 forbidden`.
pub fn forbidden(what: &str) -> Response {
    plain(StatusCode::FORBIDDEN, what)
}

/// `429 rate limited` with `Retry-After`.
pub fn rate_limited(retry_after_s: u32) -> Response {
    let mut r = plain(StatusCode::TOO_MANY_REQUESTS, "rate limited");
    if let Ok(v) = HeaderValue::from_str(&retry_after_s.to_string()) {
        r.headers_mut().insert(header::RETRY_AFTER, v);
    }
    r
}

/// `503` with `Retry-After`.
pub fn unavailable(body: &str, retry_after_s: u32) -> Response {
    let mut r = plain(StatusCode::SERVICE_UNAVAILABLE, body);
    if let Ok(v) = HeaderValue::from_str(&retry_after_s.to_string()) {
        r.headers_mut().insert(header::RETRY_AFTER, v);
    }
    r
}

/// Maps an [`AssetError`] to its response (Plan/24 §7.3, §7.6, §9.1):
/// `NotFound`/`NotConvertible`/`Disabled` 404 (the body says which), `BadKey` 400, `Build`/`Io` 500 with
/// `X-Flyff-Asset-Error`, `Timeout`/`ShuttingDown`/`OverBudget` 503 with `Retry-After`.
pub fn asset_error(err: &AssetError) -> Response {
    let mut resp = match err.kind {
        AssetErrorKind::NotFound => plain(StatusCode::NOT_FOUND, "not found"),
        AssetErrorKind::NotConvertible => plain(StatusCode::NOT_FOUND, "not convertible"),
        AssetErrorKind::Disabled => plain(StatusCode::NOT_FOUND, "asset kind not available"),
        AssetErrorKind::BadKey => plain(StatusCode::BAD_REQUEST, "bad path"),
        AssetErrorKind::Build | AssetErrorKind::Io => {
            plain(StatusCode::INTERNAL_SERVER_ERROR, "asset build failed")
        }
        AssetErrorKind::Timeout => unavailable("preparing, retry", 2),
        AssetErrorKind::ShuttingDown => unavailable("shutting down", 5),
        AssetErrorKind::OverBudget => unavailable("cache over budget", 30),
    };
    if matches!(
        err.kind,
        AssetErrorKind::Build | AssetErrorKind::Io | AssetErrorKind::NotConvertible
    ) && let Ok(v) = HeaderValue::from_str(err.kind.as_str())
    {
        resp.headers_mut().insert(ASSET_ERROR_HEADER, v);
    }
    resp
}
