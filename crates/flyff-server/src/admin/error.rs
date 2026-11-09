//! [`ApiError`]: the error type of every admin route (Plan/25 §11.3, §11.10; WP-ADMIN-02).
//!
//! The response is `{"error": {"code": "<snake_case>", "message": "...", "details": {...}}}` with the HTTP status of
//! [`ErrorCode::status`]. The error also says whether the failed request may have changed something
//! (`maybe_applied`): the idempotency guard leaves such a key `in_flight` instead of freeing it for a retry.

use super::caps::Cap;
use super::dto::{ErrorBody, ErrorCode, ErrorDetails, ErrorEnvelope};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};

/// Marker in the extensions of a response: the request failed after the point where it could already have changed
/// something (a timeout waiting for the game thread). The idempotency guard keeps the key `in_flight`.
#[derive(Clone, Copy, Debug)]
pub struct MaybeApplied;

/// An error answer of an admin route.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiError {
    /// The code.
    pub code: ErrorCode,
    /// English message.
    pub message: String,
    /// Extra context.
    pub details: Option<ErrorDetails>,
    /// The request may have changed state before it failed.
    pub maybe_applied: bool,
}

/// `Result` of an admin handler.
pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    /// An error with a code and a message.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> ApiError {
        ApiError {
            code,
            message: message.into(),
            details: None,
            maybe_applied: false,
        }
    }

    /// With details.
    #[must_use]
    pub fn with_details(mut self, details: ErrorDetails) -> ApiError {
        self.details = Some(details);
        self
    }

    /// Marks the error as "may have been applied".
    #[must_use]
    pub fn maybe_applied(mut self) -> ApiError {
        self.maybe_applied = true;
        self
    }

    /// 403: the identity lacks `cap`.
    pub fn forbidden(cap: Cap) -> ApiError {
        ApiError::new(
            ErrorCode::Forbidden,
            format!("the capability {} is required", cap.name()),
        )
        .with_details(ErrorDetails {
            cap: Some(cap.name().to_owned()),
            ..ErrorDetails::default()
        })
    }

    /// 403 without a capability (CSRF header, owner-only routes).
    pub fn forbidden_text(message: impl Into<String>) -> ApiError {
        ApiError::new(ErrorCode::Forbidden, message)
    }

    /// 403: the target's authority is not below the actor's letter.
    pub fn target_outranks() -> ApiError {
        ApiError::new(
            ErrorCode::TargetOutranks,
            "the target's authority is not below yours",
        )
    }

    /// 401: no valid session.
    pub fn unauthorized() -> ApiError {
        ApiError::new(ErrorCode::Unauthorized, "sign in first")
    }

    /// 401: the one answer of every refused login.
    pub fn invalid_credentials() -> ApiError {
        ApiError::new(ErrorCode::InvalidCredentials, "invalid credentials")
    }

    /// 400: a field is invalid.
    pub fn invalid(field: &str, message: impl Into<String>) -> ApiError {
        ApiError::new(ErrorCode::Invalid, message).with_details(ErrorDetails {
            field: Some(field.to_owned()),
            ..ErrorDetails::default()
        })
    }

    /// 400: the body is not the expected JSON.
    pub fn bad_request(message: impl Into<String>) -> ApiError {
        ApiError::new(ErrorCode::BadRequest, message)
    }

    /// 404.
    pub fn not_found(message: impl Into<String>) -> ApiError {
        ApiError::new(ErrorCode::NotFound, message)
    }

    /// 501: the backend belongs to `wp`, which has not landed.
    pub fn needs_feature(wp: &str) -> ApiError {
        ApiError::new(
            ErrorCode::NeedsFeature,
            format!("not available yet: needs {wp}"),
        )
        .with_details(ErrorDetails {
            wp: Some(wp.to_owned()),
            ..ErrorDetails::default()
        })
    }

    /// 501.
    pub fn not_implemented(message: impl Into<String>) -> ApiError {
        ApiError::new(ErrorCode::NotImplemented, message)
    }

    /// 429 with a hint for the retry.
    pub fn rate_limited(retry_after_s: u32, message: impl Into<String>) -> ApiError {
        ApiError::new(ErrorCode::RateLimited, message).with_details(ErrorDetails {
            retry_after_s: Some(retry_after_s),
            ..ErrorDetails::default()
        })
    }

    /// 428: change the password first.
    pub fn password_change_required() -> ApiError {
        ApiError::new(
            ErrorCode::PasswordChangeRequired,
            "the password must be changed before anything else",
        )
    }

    /// 400: a reason is mandatory.
    pub fn reason_required() -> ApiError {
        ApiError::new(
            ErrorCode::ReasonRequired,
            "a reason (a preset or a text) is required",
        )
    }

    /// 400: the typed confirmation is missing or wrong; `expected` is what the client must type.
    pub fn confirm_required(expected: &str) -> ApiError {
        ApiError::new(
            ErrorCode::ConfirmRequired,
            format!("type {expected} to confirm"),
        )
    }

    /// 413: more targets than `admin.max_objects_per_action`.
    pub fn too_many_targets(max: u32) -> ApiError {
        ApiError::new(
            ErrorCode::TooManyTargets,
            format!("at most {max} targets per action"),
        )
    }

    /// 503: the audit record of an admin mutation could not be acknowledged (25 §10.1); nothing was dispatched.
    pub fn audit_unavailable() -> ApiError {
        ApiError::new(
            ErrorCode::AuditUnavailable,
            "the audit log is not available: the request was not carried out",
        )
    }

    /// 503: the game thread or the database did not answer.
    pub fn unavailable(message: impl Into<String>) -> ApiError {
        ApiError::new(ErrorCode::Unavailable, message)
    }

    /// 500; the cause stays in the log.
    pub fn internal(cause: &str) -> ApiError {
        tracing::error!("admin API: {cause}");
        ApiError::new(ErrorCode::Internal, "internal error")
    }

    /// The HTTP status.
    pub fn status(&self) -> StatusCode {
        StatusCode::from_u16(self.code.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
    }

    /// The JSON body.
    pub fn envelope(&self) -> ErrorEnvelope {
        ErrorEnvelope {
            error: ErrorBody {
                code: self.code,
                message: self.message.clone(),
                details: self.details.clone(),
            },
        }
    }
}

impl ApiError {
    /// A persistence error: `503 unavailable` (the cause stays in the log).
    pub fn db(e: &flyff_db::DbError) -> ApiError {
        tracing::error!("admin API: database error: {e}");
        ApiError::unavailable("the database did not answer")
    }
}

impl From<super::ops::OpsError> for ApiError {
    /// A command the game thread did not take or answer: `503 unavailable`. A timeout may have been applied late, so
    /// it is marked [`ApiError::maybe_applied`] (an idempotency key stays in flight).
    fn from(e: super::ops::OpsError) -> ApiError {
        match e {
            super::ops::OpsError::Timeout => {
                ApiError::unavailable("the game thread did not answer in time").maybe_applied()
            }
            super::ops::OpsError::GameGone => {
                ApiError::unavailable("the game thread is not running")
            }
        }
    }
}

impl From<&flyff_game::admin::AdminFailure> for ApiError {
    /// A failure of a console command or query on the game thread (`AdminOutput::Failed`,
    /// `AdminQueryResult::Failed`): the code maps to the wire code of 25 §11.10 and its HTTP status (`stale_target`
    /// 409, `too_many_targets` 413, `not_implemented` 501, ...).
    fn from(f: &flyff_game::admin::AdminFailure) -> ApiError {
        use flyff_game::admin::AdminErrorCode as G;
        let code = match f.code {
            G::NotImplemented => ErrorCode::NotImplemented,
            G::NotFound => ErrorCode::NotFound,
            G::Invalid => ErrorCode::Invalid,
            G::StaleTarget => ErrorCode::StaleTarget,
            G::TooManyTargets => ErrorCode::TooManyTargets,
            G::CharacterOnline => ErrorCode::CharacterOnline,
            G::TargetOutranks => ErrorCode::TargetOutranks,
            G::NoSpace => ErrorCode::NoSpace,
            G::MailboxFull => ErrorCode::MailboxFull,
            G::DeliveryInProgress => ErrorCode::DeliveryInProgress,
            G::AlreadyDelivered => ErrorCode::AlreadyDelivered,
            // `Internal` and any code a later work package adds
            _ => ErrorCode::Internal,
        };
        ApiError::new(code, f.message.clone())
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code.name(), self.message)
    }
}

impl std::error::Error for ApiError {}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let retry = self.details.as_ref().and_then(|d| d.retry_after_s);
        let mut resp = (self.status(), axum::Json(self.envelope())).into_response();
        if let Some(s) = retry
            && let Ok(v) = HeaderValue::from_str(&s.max(1).to_string())
        {
            resp.headers_mut().insert(header::RETRY_AFTER, v);
        }
        resp.headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        if self.maybe_applied {
            resp.extensions_mut().insert(MaybeApplied);
        }
        resp
    }
}
