//! Guards of the admin API (Plan/25 §1.6, §2.3, §11.10, §12; WP-ADMIN-01/02): what every mutation passes before it
//! reaches its handler's body.
//!
//! | guard | rule | answer |
//! |---|---|---|
//! | [`rank`] | an action on a target whose resolved authority is `>=` the actor's letter is refused (owners are exempt); a staff member never sets a letter above its own and never changes its own account's authority | `403 target_outranks` / `403 forbidden` |
//! | [`Policy::reason_required`] | a preset of `[admin.reason_presets]`, a free text, or both | `400 reason_required` |
//! | [`Policy::confirm`] | the typed confirmation, **recomputed on the server** from the facts of the request (more than 10 targets, all players, irreversible, penya above `admin.confirm_gold_threshold`, more than `admin.confirm_item_threshold` items) | `400 confirm_required` |
//! | [`Policy::check_targets`] | group actions are capped at `admin.max_objects_per_action` | `413 too_many_targets` |
//! | [`MutationJson`] | parses the `Mutation<T>` envelope `{body, reason?, confirm?}` | `400 bad_request` |
//! | `guard_idempotency` | `Idempotency-Key` on the repeatable mutations (✱ routes of the catalogue) | `409 in_flight`, `422 idempotency_mismatch`, the stored result on a replay |
//! | `guard_limits` | the per-identity token bucket (`admin.mutations_per_s`) and the streaming permit | `429 rate_limited` |
//!
//! The capability check, the bucket, the idempotency flow and the `428` rule are applied to every route by the gate
//! of [`super::api`]; the reason, confirmation, group cap and rank guards are called by the handler that knows the
//! facts of its request.

use super::dto::{Mutation, Reason};
use super::error::{ApiError, ApiResult};
use super::identity::Identity;
use crate::audit::{Actor, AuditFile};
use axum::body::Bytes;
use axum::extract::{FromRequest, Request};
use flyff_common::gm::Authority;
use parking_lot::RwLock;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::sync::Arc;

pub use super::guard_idempotency::{IdemBegin, IdemSlot, Idempotency, MAX_BODY};
pub use super::guard_limits::{StreamGuard, StreamPermits, TokenBuckets};

/// Typed confirmation above this many targets (25 §2.3).
pub const CONFIRM_TARGETS: usize = 10;
/// Longest stored reason text (characters).
pub const REASON_TEXT_MAX: usize = 200;

/// The live `[admin]` values the guards read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Policy {
    /// `admin.reason_presets`.
    pub reason_presets: Vec<String>,
    /// `admin.confirm_gold_threshold`.
    pub confirm_gold_threshold: u64,
    /// `admin.confirm_item_threshold`.
    pub confirm_item_threshold: u32,
    /// `admin.max_objects_per_action`.
    pub max_objects_per_action: u32,
    /// `admin.mutations_per_s`.
    pub mutations_per_s: u32,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            reason_presets: [
                "Spam",
                "Abuse",
                "Botting",
                "RMT",
                "Exploit",
                "Impersonation",
                "Support",
                "Event",
                "Other",
            ]
            .map(str::to_owned)
            .to_vec(),
            confirm_gold_threshold: 100_000_000,
            confirm_item_threshold: 50,
            max_objects_per_action: 500,
            mutations_per_s: 30,
        }
    }
}

/// A validated reason as it is stored and audited: `Preset: free text`, `Preset` or `free text`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReasonText(String);

impl ReasonText {
    /// The text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ReasonText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// What the confirmation depends on (25 §2.3). The handler fills what it knows.
#[derive(Clone, Copy, Debug, Default)]
pub struct ConfirmFacts<'a> {
    /// How many targets the action touches.
    pub targets: usize,
    /// The target's name when there is exactly one (what the operator types).
    pub target_name: Option<&'a str>,
    /// The action addresses every player.
    pub all_players: bool,
    /// The action cannot be undone (disband, kill, removal, rename, ...).
    pub irreversible: bool,
    /// The absolute penya change of the action.
    pub gold: u64,
    /// The number of items the action creates or removes.
    pub items: u64,
}

/// The confirmation level of an action (the client shows the matching dialog; the server enforces `Typed`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmLevel {
    /// Reversible, one target: a click confirms (enforced by the client only).
    Simple,
    /// The operator types the target name, or `CONFIRM` for several targets.
    Typed,
}

impl Policy {
    /// The reason of a mutation that must have one (kick, mute, ban, login block, IP block, removals, penya
    /// changes, grants, disbanding; 25 §2.3). A preset must be one of `reason_presets`; free text is cut to
    /// [`REASON_TEXT_MAX`] characters and loses control characters.
    pub fn reason_required(&self, reason: Option<&Reason>) -> ApiResult<ReasonText> {
        self.reason_optional(reason)?
            .ok_or_else(ApiError::reason_required)
    }

    /// Like [`Policy::reason_required`], `None` when no reason was given.
    pub fn reason_optional(&self, reason: Option<&Reason>) -> ApiResult<Option<ReasonText>> {
        let Some(r) = reason else {
            return Ok(None);
        };
        let preset = match r.preset.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
            None => None,
            Some(p) => Some(
                self.reason_presets
                    .iter()
                    .find(|k| k.eq_ignore_ascii_case(p))
                    .ok_or_else(|| {
                        ApiError::invalid("reason.preset", format!("'{p}' is not a reason preset"))
                    })?,
            ),
        };
        let text: String = r
            .text
            .as_deref()
            .unwrap_or("")
            .chars()
            .filter(|c| !c.is_control())
            .collect::<String>()
            .trim()
            .chars()
            .take(REASON_TEXT_MAX)
            .collect();
        Ok(match (preset, text.is_empty()) {
            (None, true) => None,
            (Some(p), true) => Some(ReasonText(p.clone())),
            (None, false) => Some(ReasonText(text)),
            (Some(p), false) => Some(ReasonText(format!("{p}: {text}"))),
        })
    }

    /// The confirmation level of an action (25 §2.3): typed for irreversible actions, more than 10 targets, all
    /// players, penya above `confirm_gold_threshold` or more than `confirm_item_threshold` items.
    pub fn confirm_level(&self, f: &ConfirmFacts<'_>) -> ConfirmLevel {
        if f.irreversible
            || f.all_players
            || f.targets > CONFIRM_TARGETS
            || f.gold > self.confirm_gold_threshold
            || f.items > u64::from(self.confirm_item_threshold)
        {
            ConfirmLevel::Typed
        } else {
            ConfirmLevel::Simple
        }
    }

    /// What the operator must type for a typed confirmation: the target's name for one target, else `CONFIRM`.
    pub fn confirm_expected<'a>(&self, f: &ConfirmFacts<'a>) -> &'a str {
        match (f.targets, f.target_name, f.all_players) {
            (1, Some(name), false) => name,
            _ => "CONFIRM",
        }
    }

    /// Enforces the typed confirmation when the facts need one (`400 confirm_required`): `given` must equal the
    /// target name (ASCII case-insensitive) or `CONFIRM`.
    pub fn confirm(&self, f: &ConfirmFacts<'_>, given: Option<&str>) -> ApiResult<ConfirmLevel> {
        let level = self.confirm_level(f);
        if level == ConfirmLevel::Typed {
            let expected = self.confirm_expected(f);
            let ok = given.map(str::trim).is_some_and(|g| {
                if expected == "CONFIRM" {
                    g == "CONFIRM"
                } else {
                    g.eq_ignore_ascii_case(expected)
                }
            });
            if !ok {
                return Err(ApiError::confirm_required(expected));
            }
        }
        Ok(level)
    }

    /// The group cap of 25 §11.5: more than `max_objects_per_action` targets answer `413 too_many_targets`.
    pub fn check_targets(&self, targets: usize) -> ApiResult<()> {
        if targets > usize::try_from(self.max_objects_per_action).unwrap_or(usize::MAX) {
            Err(ApiError::too_many_targets(self.max_objects_per_action))
        } else {
            Ok(())
        }
    }
}

/// The policy in force (`reload` replaces it).
#[derive(Debug)]
pub struct LivePolicy(RwLock<Arc<Policy>>);

impl LivePolicy {
    /// Holds `policy`.
    pub fn new(policy: Policy) -> LivePolicy {
        LivePolicy(RwLock::new(Arc::new(policy)))
    }

    /// The policy in force.
    pub fn get(&self) -> Arc<Policy> {
        Arc::clone(&self.0.read())
    }

    /// Replaces the policy.
    pub fn set(&self, policy: Policy) {
        *self.0.write() = Arc::new(policy);
    }
}

// ---- rank protection ----------------------------------------------------------------------------------------------

/// Rank protection (25 §1.6, §12 rule 2): `admin::guard::rank`.
pub mod rank {
    use super::{ApiError, ApiResult, Authority, Identity};
    use flyff_common::AccountId;

    /// An action on a target with the resolved authority `target`: refused with `403 target_outranks` when the
    /// target's letter is not below the actor's own. The owner identities are exempt.
    pub fn check_target(actor: &Identity, target: Authority) -> ApiResult<()> {
        if actor.is_owner() || target < actor.authority {
            Ok(())
        } else {
            Err(ApiError::target_outranks())
        }
    }

    /// Setting `new_level` on the account `target_account` whose current resolved letter is `target_current`: the
    /// target must rank below the actor, the new letter must not exceed the actor's own, and staff never change the
    /// authority of their own account. The owner identities are exempt.
    pub fn check_set_authority(
        actor: &Identity,
        target_account: AccountId,
        target_current: Authority,
        new_level: Authority,
    ) -> ApiResult<()> {
        if actor.is_owner() {
            return Ok(());
        }
        if actor
            .account
            .as_ref()
            .is_some_and(|a| a.id == target_account)
        {
            return Err(ApiError::forbidden_text(
                "staff cannot change the authority of their own account",
            ));
        }
        check_target(actor, target_current)?;
        if new_level > actor.authority {
            return Err(ApiError::forbidden_text(
                "a letter above your own cannot be granted",
            ));
        }
        Ok(())
    }
}

// ---- the mutation envelope ----------------------------------------------------------------------------------------

/// Extracts the `Mutation<T>` envelope of a request: `{body, reason?, confirm?}`. A malformed body is
/// `400 bad_request` with the parser's message.
#[derive(Debug)]
pub struct MutationJson<T>(pub Mutation<T>);

impl<S, T> FromRequest<S> for MutationJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let bytes = Bytes::from_request(req, state)
            .await
            .map_err(|e| ApiError::bad_request(format!("cannot read the body: {e}")))?;
        serde_json::from_slice::<Mutation<T>>(&bytes)
            .map(MutationJson)
            .map_err(|e| ApiError::bad_request(format!("malformed body: {e}")))
    }
}

// ---- audit ---------------------------------------------------------------------------------------------------------

/// Emits an `admin.*` audit record (25 §10.1, §10.3): `actor`, the peer `ip`, `state` (`requested`, `done`,
/// `failed`) and the `reason` join `data`, and waits (at most 1 s) until the writer has written the line
/// ([`AuditFile::emit_admin`]).
///
/// **The rule of 25 §10.1:** a handler writes the `requested` record *before* it dispatches anything and returns
/// the error of this function (`503 audit_unavailable`) without dispatching when it fails; after the result it
/// writes `done` or `failed`. With `[audit] admin_required = false` a failed acknowledgement is only logged. The
/// `rid` of the record (correlating `requested` with its `done`) is the handler's: put it into `data`.
pub fn audit_admin(
    audit: &AuditFile,
    ev: &'static str,
    who: &Identity,
    state: &str,
    reason: Option<&ReasonText>,
    data: Value,
) -> ApiResult<()> {
    let mut record = match data {
        Value::Object(m) => m,
        other => {
            let mut m = serde_json::Map::new();
            m.insert("detail".to_owned(), other);
            m
        }
    };
    record.insert("actor".to_owned(), json!(who.label()));
    record.insert("state".to_owned(), json!(state));
    if let Some(r) = reason {
        record.insert("reason".to_owned(), json!(r.as_str()));
    }
    let actor = Actor {
        ip: Some(who.ip.to_string()),
        account: who.account.as_ref().map(|a| a.id),
        ..Actor::none()
    };
    match audit.emit_admin(ev, &actor, Value::Object(record)) {
        Ok(()) => Ok(()),
        Err(e) if audit.admin_required() => {
            tracing::error!(event = ev, error = %e, "the admin audit record was not acknowledged: the request is refused");
            Err(ApiError::audit_unavailable())
        }
        Err(e) => {
            tracing::warn!(event = ev, error = %e, "the admin audit record was not acknowledged (audit.admin_required is off)");
            Ok(())
        }
    }
}

/// The audit trail of one admin mutation (25 §10.1): `requested` before anything is dispatched, then `done` or
/// `failed` with the same `rid`. [`AuditTrail::begin`] fails with `503 audit_unavailable` when the `requested` record
/// is not acknowledged (and `[audit] admin_required` is on); [`AuditTrail::done`] and [`AuditTrail::failed`] never
/// fail the request (the action has happened; a lost closing record is logged and counted by the audit stats).
///
/// ```text
/// let trail = AuditTrail::begin(&ops.audit, &who, "admin.config", reason, json!({"key": key}))?;
/// match dispatch().await {
///     Ok(out) => trail.done(json!({"after": out})),
///     Err(e) => trail.failed(&e.to_string()),
/// }
/// ```
pub struct AuditTrail<'a> {
    audit: &'a AuditFile,
    who: &'a Identity,
    ev: &'static str,
    reason: Option<ReasonText>,
    base: serde_json::Map<String, Value>,
}

impl<'a> AuditTrail<'a> {
    /// Writes the `requested` record and returns the trail, or the `503` of an unacknowledged record.
    pub fn begin(
        audit: &'a AuditFile,
        who: &'a Identity,
        ev: &'static str,
        reason: Option<ReasonText>,
        data: Value,
    ) -> ApiResult<AuditTrail<'a>> {
        let mut base = match data {
            Value::Object(m) => m,
            other => {
                let mut m = serde_json::Map::new();
                m.insert("detail".to_owned(), other);
                m
            }
        };
        base.insert("rid".to_owned(), json!(next_rid()));
        audit_admin(
            audit,
            ev,
            who,
            "requested",
            reason.as_ref(),
            Value::Object(base.clone()),
        )?;
        Ok(AuditTrail {
            audit,
            who,
            ev,
            reason,
            base,
        })
    }

    /// The request id that joins `requested` and `done` / `failed`.
    pub fn rid(&self) -> &str {
        self.base.get("rid").and_then(Value::as_str).unwrap_or("")
    }

    fn close(&self, state: &str, extra: Value) {
        let mut data = self.base.clone();
        if let Value::Object(e) = extra {
            data.extend(e);
        }
        if let Err(e) = audit_admin(
            self.audit,
            self.ev,
            self.who,
            state,
            self.reason.as_ref(),
            Value::Object(data),
        ) {
            tracing::error!(event = self.ev, state, error = %e, "the closing admin audit record was lost");
        }
    }

    /// Writes the `done` record with `extra` merged into the data of the `requested` one.
    pub fn done(&self, extra: Value) {
        self.close("done", extra);
    }

    /// Writes the `failed` record with the error text.
    pub fn failed(&self, error: &str) {
        self.close("failed", json!({"error": error}));
    }
}

/// A request id: unique within a process run, readable in the log (`<unix ms hex>-<counter hex>`).
pub fn next_rid() -> String {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    format!("{ms:x}-{n:x}")
}
