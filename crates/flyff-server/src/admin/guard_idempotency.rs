//! The `Idempotency-Key` guard (Plan/25 §11.6, §12 rule 9; WP-ADMIN-02) over the table `admin_idempotency`.
//!
//! The routes marked ✱ in the catalogue need an `Idempotency-Key` header (1..=128 printable ASCII characters).
//! [`Idempotency::begin`] writes the key row **before** the handler runs (one writer transaction, so two concurrent
//! requests with one key cannot both start) and answers:
//!
//! | decision | answer |
//! |---|---|
//! | [`IdemBegin::Started`] | run the handler, then [`IdemSlot::done`] with the response (or [`IdemSlot::failed`] when nothing changed) |
//! | [`IdemBegin::Replay`] | the stored response, byte for byte, with `Idempotency-Replayed: true`; the handler never runs again, also after a restart |
//! | [`IdemBegin::InFlight`] | `409 in_flight` |
//! | [`IdemBegin::Mismatch`] | `422 idempotency_mismatch` (the key belongs to another body, route or actor) |
//!
//! The comparison covers actor, `METHOD path` and the SHA-256 of the body. The stored result is a small JSON
//! object `{status, body}`; a response too large for the table (1 MiB) is stored without its body and replayed as
//! `{"replayed":true,"truncated":true}` with the stored status.

use super::error::{ApiError, ApiResult};
use crate::lobby::online::LobbyClock;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use flyff_db::{
    AtomicBatch, AtomicKind, AtomicOutcome, DbOp, IdempotencyDecision, IdempotencyState,
    PersistHandle,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Largest request body the guard buffers (the HTTP stack's 16 KiB limit applies first; routes with larger bodies
/// raise their own limit up to this).
pub const MAX_BODY: usize = 256 * 1024;
/// A stored response over this size is not kept.
const MAX_STORED_BODY: usize = 900 * 1024;

/// The `Idempotency-Key` guard.
#[derive(Clone)]
pub struct Idempotency {
    persist: PersistHandle,
    clock: LobbyClock,
}

impl std::fmt::Debug for Idempotency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Idempotency")
    }
}

#[derive(Serialize, Deserialize)]
struct Stored {
    status: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    body: Option<String>,
}

/// What [`Idempotency::begin`] decided.
pub enum IdemBegin {
    /// A new row (or a failed one of the same request, re-armed): run the request.
    Started(IdemSlot),
    /// The request finished before: answer with this response.
    Replay(Response),
    /// The same request is running.
    InFlight,
    /// The key belongs to another request.
    Mismatch,
}

/// The open key of a running request.
pub struct IdemSlot {
    persist: PersistHandle,
    key: String,
}

impl Idempotency {
    /// The guard over `persist`; `clock` stamps the rows.
    pub fn new(persist: PersistHandle, clock: LobbyClock) -> Idempotency {
        Idempotency { persist, clock }
    }

    /// SHA-256 of a request body.
    pub fn body_hash(body: &[u8]) -> [u8; 32] {
        Sha256::digest(body).into()
    }

    /// Registers the request; see the module table.
    pub async fn begin(
        &self,
        key: &str,
        actor: &str,
        route: &str,
        body_sha256: [u8; 32],
    ) -> ApiResult<IdemBegin> {
        let batch = AtomicBatch::of_ops(
            AtomicKind::Other("admin.idempotency"),
            vec![DbOp::IdempotencyBegin {
                key: key.to_owned(),
                actor: actor.to_owned(),
                route: route.to_owned(),
                body_sha256,
                at: self.clock.unix(),
            }],
        );
        let outcome = self.persist.call(batch).await.map_err(|e| {
            if e.kind == flyff_db::DbErrorKind::Invalid {
                ApiError::invalid("Idempotency-Key", e.to_string())
            } else {
                ApiError::unavailable(format!("the idempotency table is not available: {e}"))
            }
        })?;
        let AtomicOutcome::Idempotency(decision) = outcome else {
            return Err(ApiError::internal(
                "IdempotencyBegin answered another outcome",
            ));
        };
        Ok(match decision {
            IdempotencyDecision::Started => IdemBegin::Started(IdemSlot {
                persist: self.persist.clone(),
                key: key.to_owned(),
            }),
            IdempotencyDecision::InFlight { .. } => IdemBegin::InFlight,
            IdempotencyDecision::Mismatch => IdemBegin::Mismatch,
            IdempotencyDecision::Done { result } => IdemBegin::Replay(replay(result.as_deref())),
        })
    }
}

fn replay(result: Option<&str>) -> Response {
    let stored: Option<Stored> = result.and_then(|r| serde_json::from_str(r).ok());
    let (status, body) = match stored {
        Some(Stored {
            status,
            body: Some(b),
        }) => (status, b),
        Some(Stored { status, body: None }) => {
            (status, r#"{"replayed":true,"truncated":true}"#.to_owned())
        }
        None => (200, r#"{"replayed":true}"#.to_owned()),
    };
    let mut resp = (StatusCode::from_u16(status).unwrap_or(StatusCode::OK), body).into_response();
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    resp.headers_mut()
        .insert("idempotency-replayed", HeaderValue::from_static("true"));
    resp
}

impl IdemSlot {
    /// The request changed something (or answered successfully): keep its response for replays.
    pub async fn done(self, status: u16, body: &[u8]) {
        let body = if body.len() <= MAX_STORED_BODY {
            String::from_utf8(body.to_vec()).ok()
        } else {
            None
        };
        let result = serde_json::to_string(&Stored { status, body }).ok();
        self.finish(IdempotencyState::Done, result).await;
    }

    /// The request failed before it changed anything: the key may be retried with the same body.
    pub async fn failed(self) {
        self.finish(IdempotencyState::Failed, None).await;
    }

    async fn finish(self, state: IdempotencyState, result: Option<String>) {
        let batch = AtomicBatch::of_ops(
            AtomicKind::Other("admin.idempotency"),
            vec![DbOp::IdempotencyFinish {
                key: self.key.clone(),
                state,
                result,
            }],
        );
        if let Err(e) = self.persist.call(batch).await {
            // the row stays in flight: a retry gets 409, the operator uses a new key (never a double run)
            tracing::error!(key = %self.key, "cannot close an idempotency key: {e}");
        }
    }
}

/// The `Idempotency-Key` header of a request: 1..=128 printable ASCII characters.
pub fn key_of(headers: &axum::http::HeaderMap) -> ApiResult<String> {
    let value = headers.get("idempotency-key").ok_or_else(|| {
        ApiError::invalid(
            "Idempotency-Key",
            "this route needs an Idempotency-Key header",
        )
    })?;
    let text = value
        .to_str()
        .map_err(|_| ApiError::invalid("Idempotency-Key", "the key must be printable ASCII"))?
        .trim();
    if text.is_empty() || text.len() > 128 || !text.bytes().all(|b| (0x21..=0x7e).contains(&b)) {
        return Err(ApiError::invalid(
            "Idempotency-Key",
            "the key is 1..=128 printable ASCII characters",
        ));
    }
    Ok(text.to_owned())
}
