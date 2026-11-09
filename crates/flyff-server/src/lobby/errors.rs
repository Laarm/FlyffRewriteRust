//! `LobbyErr`: the HTTP error of `/api/login` and `/api/register` (Plan/12 §6.1, §16.1).
//!
//! The body is `{"code": <i32>, "tid": <i32>, "retry_after_ms": <u32, optional>, "detail": <string,
//! optional, <= 255 B>}`; `tid = 0` means "use the client default text for `code`" (12 §16.5). `code` is the
//! legacy `ERROR_*` value where one exists, otherwise a new code >= 2000.

use axum::body::Body;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use flyff_common::defs::text::{TID_15MIN_PREVENT, TID_15SEC_PREVENT, TID_DB_INSPECTION};
use serde::Serialize;

/// 12 §16.1 codes (the legacy `ERROR_*` values and the new lobby codes).
pub mod code {
    /// `ERROR_ACCOUNT_EXISTS`: registration with a taken name.
    pub const ACCOUNT_EXISTS: i32 = 100;
    /// `ERROR_DUPLICATE_ACCOUNT`: the account is connected (409, the client asks "cut the connection?").
    pub const DUPLICATE_ACCOUNT: i32 = 103;
    /// `ERROR_OVERFLOW`: capacity / hash permit timeout.
    pub const OVERFLOW: i32 = 108;
    /// `ERROR_EXTERNAL_ADDR`: shutdown, persistence degraded, maintenance.
    pub const UNAVAILABLE: i32 = 109;
    /// `ERROR_ILLEGAL_ACCESS`: a malformed create field, a foreign or unknown `id_player` at enter (WS).
    pub const ILLEGAL_ACCESS: i32 = 104;
    /// `ERROR_DUPLICATE_SLOT`: the slot is taken or the account has three characters (WS).
    pub const DUPLICATE_SLOT: i32 = 105;
    /// `ERROR_SLOT_OUTOFRANGE`: a slot above 2 (WS).
    pub const SLOT_OUTOFRANGE: i32 = 106;
    /// `ERROR_INVALID_NAME_CHARACTER`: the name rules 1-6 of 12 §9.3 (WS).
    pub const INVALID_NAME_CHARACTER: i32 = 111;
    /// `ERROR_WARTIME`: a delete while the guild is at war (WS).
    pub const WARTIME: i32 = 112;
    /// `ERROR_BLOCKGOLD_ACCOUNT`: banned (PL-032).
    pub const BANNED: i32 = 119;
    /// `ERROR_FLYFF_PASSWORD`: wrong password.
    pub const WRONG_PASSWORD: i32 = 120;
    /// `ERROR_FLYFF_ACCOUNT`: unknown account or invalid account name.
    pub const UNKNOWN_ACCOUNT: i32 = 121;
    /// `ERROR_FLYFF_DB_JOB_ING`: the previous logout save is not committed yet.
    pub const DB_JOB_ING: i32 = 133;
    /// `ERROR_15SEC_PREVENT`: throttle after one wrong password.
    pub const PREVENT_SHORT: i32 = 134;
    /// `ERROR_15MIN_PREVENT`: throttle after three wrong passwords.
    pub const PREVENT_LONG: i32 = 135;
    /// `ERROR_CERT_GENERAL`: internal error.
    pub const CERT_GENERAL: i32 = 136;
    /// `ERROR_USER_EXISTS`: the character name is taken (WS).
    pub const USER_EXISTS: i32 = 1316;
    /// `ERROR_NO_SUCH_GROUP`: a delete of a character the account does not own, or a wrong delete password (WS).
    pub const NO_SUCH_GROUP: i32 = 1319;
    /// New: malformed request.
    pub const BAD_REQUEST: i32 = 2000;
    /// New: the character is blocked and cannot enter the world (WS).
    pub const CHARACTER_BLOCKED: i32 = 2001;
    /// New: registration disabled.
    pub const REGISTRATION_DISABLED: i32 = 2002;
    /// New: invalid account name.
    pub const INVALID_ACCOUNT_NAME: i32 = 2003;
    /// New: invalid password (the md5 of the empty password).
    pub const INVALID_PASSWORD: i32 = 2004;
    /// New: registration rate limit.
    pub const REGISTRATION_RATE_LIMITED: i32 = 2005;
}

const MAX_DETAIL: usize = 255;

fn tid(v: u32) -> i32 {
    i32::try_from(v).unwrap_or(0)
}

/// An error answer of the lobby HTTP API.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LobbyErr {
    /// HTTP status.
    pub status: StatusCode,
    /// `code` of the body.
    pub code: i32,
    /// `tid` of the body (0 = the client's default text for `code`).
    pub tid: i32,
    /// `retry_after_ms`.
    pub retry_after_ms: Option<u32>,
    /// `detail`.
    pub detail: Option<String>,
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    code: i32,
    tid: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    retry_after_ms: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<&'a str>,
}

impl LobbyErr {
    fn new(status: StatusCode, code: i32, tid: i32) -> LobbyErr {
        LobbyErr {
            status,
            code,
            tid,
            retry_after_ms: None,
            detail: None,
        }
    }

    /// Adds `retry_after_ms`.
    #[must_use]
    pub fn with_retry(mut self, ms: u32) -> LobbyErr {
        self.retry_after_ms = Some(ms);
        self
    }

    /// Adds `detail` (cut to 255 bytes at a character boundary).
    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> LobbyErr {
        let mut d: String = detail.into();
        if d.len() > MAX_DETAIL {
            let mut end = MAX_DETAIL;
            while !d.is_char_boundary(end) {
                end -= 1;
            }
            d.truncate(end);
        }
        self.detail = Some(d);
        self
    }

    /// 400, 2000: malformed request (JSON, `pwhash` format, field values).
    pub fn bad_request(detail: &str) -> LobbyErr {
        LobbyErr::new(StatusCode::BAD_REQUEST, code::BAD_REQUEST, 0).with_detail(detail)
    }

    /// 413, 2000.
    pub fn too_large() -> LobbyErr {
        LobbyErr::new(StatusCode::PAYLOAD_TOO_LARGE, code::BAD_REQUEST, 0)
            .with_detail("request body too large")
    }

    /// 415, 2000.
    pub fn unsupported_media_type() -> LobbyErr {
        LobbyErr::new(StatusCode::UNSUPPORTED_MEDIA_TYPE, code::BAD_REQUEST, 0)
            .with_detail("Content-Type must be application/json")
    }

    /// 401, 121 (`TID_DIAG_0038`): unknown account or invalid account name.
    pub fn unknown_account() -> LobbyErr {
        LobbyErr::new(
            StatusCode::UNAUTHORIZED,
            code::UNKNOWN_ACCOUNT,
            tid(flyff_common::defs::text::TID_DIAG_0038),
        )
    }

    /// 401, 120 (`TID_DIAG_0016`): wrong password.
    pub fn wrong_password() -> LobbyErr {
        LobbyErr::new(
            StatusCode::UNAUTHORIZED,
            code::WRONG_PASSWORD,
            tid(flyff_common::defs::text::TID_DIAG_0016),
        )
    }

    /// 403, 119 (`TID_DIAG_0015`): banned; `until` is an RFC 3339 time or `"permanent"`.
    pub fn banned(until: &str) -> LobbyErr {
        LobbyErr::new(
            StatusCode::FORBIDDEN,
            code::BANNED,
            tid(flyff_common::defs::text::TID_DIAG_0015),
        )
        .with_detail(until)
    }

    /// 409, 103 (`TID_DIAG_0065`): the account is connected; the client asks whether to cut the connection.
    pub fn duplicate() -> LobbyErr {
        LobbyErr::new(
            StatusCode::CONFLICT,
            code::DUPLICATE_ACCOUNT,
            tid(flyff_common::defs::text::TID_DIAG_0065),
        )
    }

    /// 429, 134 / 135: the per-IP wrong-password lock.
    pub fn throttled(long: bool, retry_after_ms: u32) -> LobbyErr {
        if long {
            LobbyErr::new(
                StatusCode::TOO_MANY_REQUESTS,
                code::PREVENT_LONG,
                tid(TID_15MIN_PREVENT),
            )
        } else {
            LobbyErr::new(
                StatusCode::TOO_MANY_REQUESTS,
                code::PREVENT_SHORT,
                tid(TID_15SEC_PREVENT),
            )
        }
        .with_retry(retry_after_ms)
    }

    /// 503, 108 (`TID_DIAG_0041`): capacity reached or no hash permit within 10 s.
    pub fn overflow() -> LobbyErr {
        LobbyErr::new(
            StatusCode::SERVICE_UNAVAILABLE,
            code::OVERFLOW,
            tid(flyff_common::defs::text::TID_DIAG_0041),
        )
    }

    /// 503, 109 (`TID_DIAG_0053`): shutdown, persistence degraded or maintenance.
    pub fn unavailable() -> LobbyErr {
        LobbyErr::new(
            StatusCode::SERVICE_UNAVAILABLE,
            code::UNAVAILABLE,
            tid(flyff_common::defs::text::TID_DIAG_0053),
        )
    }

    /// 503, 133 (`TID_DB_INSPECTION`): the previous logout save is not committed within the flush wait.
    pub fn db_job_in_progress() -> LobbyErr {
        LobbyErr::new(
            StatusCode::SERVICE_UNAVAILABLE,
            code::DB_JOB_ING,
            tid(TID_DB_INSPECTION),
        )
    }

    /// 500 / 503, 136 (`TID_ERROR_CERT_GENERAL`): database error, poisoned account, internal error.
    pub fn internal(status: StatusCode) -> LobbyErr {
        LobbyErr::new(
            status,
            code::CERT_GENERAL,
            tid(flyff_common::defs::text::TID_ERROR_CERT_GENERAL),
        )
    }

    /// 403, 2002: registration is disabled.
    pub fn registration_disabled() -> LobbyErr {
        LobbyErr::new(StatusCode::FORBIDDEN, code::REGISTRATION_DISABLED, 0)
    }

    /// 400, 2003 (`TID_DIAG_0005`): invalid account name at registration.
    pub fn invalid_account_name() -> LobbyErr {
        LobbyErr::new(
            StatusCode::BAD_REQUEST,
            code::INVALID_ACCOUNT_NAME,
            tid(flyff_common::defs::text::TID_DIAG_0005),
        )
    }

    /// 400, 2004 (`TID_DIAG_0030`): the hash of the empty password.
    pub fn invalid_password() -> LobbyErr {
        LobbyErr::new(
            StatusCode::BAD_REQUEST,
            code::INVALID_PASSWORD,
            tid(flyff_common::defs::text::TID_DIAG_0030),
        )
    }

    /// 429, 2005: too many registrations from one address.
    pub fn registration_rate_limited(retry_after_ms: u32) -> LobbyErr {
        LobbyErr::new(
            StatusCode::TOO_MANY_REQUESTS,
            code::REGISTRATION_RATE_LIMITED,
            0,
        )
        .with_retry(retry_after_ms)
    }

    /// 409, 100 (`TID_DIAG_0032`): the account name is taken.
    pub fn account_exists() -> LobbyErr {
        LobbyErr::new(
            StatusCode::CONFLICT,
            code::ACCOUNT_EXISTS,
            tid(flyff_common::defs::text::TID_DIAG_0032),
        )
    }
}

impl std::fmt::Display for LobbyErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (code {}, tid {})", self.status, self.code, self.tid)
    }
}

impl std::error::Error for LobbyErr {}

impl IntoResponse for LobbyErr {
    fn into_response(self) -> Response {
        let body = serde_json::to_vec(&ErrorBody {
            code: self.code,
            tid: self.tid,
            retry_after_ms: self.retry_after_ms,
            detail: self.detail.as_deref(),
        })
        .unwrap_or_else(|_| b"{\"code\":136,\"tid\":0}".to_vec());
        let mut resp = Response::new(Body::from(body));
        *resp.status_mut() = self.status;
        let h = resp.headers_mut();
        h.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        if let Some(ms) = self.retry_after_ms
            && let Ok(v) = HeaderValue::from_str(&ms.div_ceil(1000).max(1).to_string())
        {
            h.insert(header::RETRY_AFTER, v);
        }
        resp
    }
}
