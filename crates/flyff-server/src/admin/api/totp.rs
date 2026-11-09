//! The staff TOTP routes (Plan/25 §1.3 "Optional TOTP", §9.4; WP-ADMIN-28). The algorithm and the in-memory guard are
//! [`crate::admin::totp`]; the login rule is `api/session.rs::staff_login`.
//!
//! | route | does |
//! |---|---|
//! | `POST /admin/api/session/totp {"step": "begin"}` | a fresh secret for the signed-in staff account: the setup key, the key URI and a QR code (SVG). Nothing changes until the confirmation; one setup per account, a new `begin` replaces it, it expires after ten minutes |
//! | `POST /admin/api/session/totp {"step": "confirm", code, pwhash}` | the current password and a code from the new secret turn it on; the account's OTHER console sessions end (they never passed the second factor) |
//! | `DELETE /admin/api/session/totp {code, pwhash}` | turns the own factor off: password and a code; refused from a remote session while `admin.totp_required` is on |
//! | `DELETE /admin/api/accounts/{name}/totp` (`act.accounts`) | reset by a higher authority (a lost phone): reason mandatory, rank protection, ends the target's sessions |
//!
//! The session routes are exempt from the layer's `428` rule, so the handlers check it. None of the routes is
//! idempotency-guarded (✱): a stored body would hold a password hash or a code. A secret leaves the server exactly once,
//! in the [`TotpSetup`] answer; no audit record, log field, metric label or error message carries a secret, a code, a
//! password hash, the key URI or the QR code, and every DTO that holds one has a redacting `Debug`.

use super::session::{audit_session, session_info};
use super::{ApiState, Routes};
use crate::admin::auth::{Caller, Who};
use crate::admin::error::{ApiError, ApiResult};
use crate::admin::guard::{AuditTrail, MutationJson, rank};
use crate::admin::identity::{Identity, StaffAccount};
use crate::admin::session::EndReason;
use crate::admin::totp::{self, ISSUER, SETUP_TTL_S, STEP_S};
use crate::lobby::accounts::AccountName;
use crate::lobby::auth::{AuthError, PwHash, Verify};
use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use flyff_common::AccountId;
use flyff_common::gm::Authority;
use flyff_db::{AtomicBatch, AtomicKind, AtomicOutcome, DbOp, queries};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::atomic::Ordering;

/// `POST /admin/api/session/totp`: the two steps of the enrolment.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum TotpRequest {
    /// Start: the server answers a [`TotpSetup`].
    Begin,
    /// Finish: a code from the new secret and the current password.
    Confirm {
        /// The six digits the authenticator app shows.
        code: String,
        /// The current password's hash (12 L1), never the password.
        pwhash: String,
    },
}

impl std::fmt::Debug for TotpRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TotpRequest::Begin => f.write_str("TotpRequest::Begin"),
            TotpRequest::Confirm { .. } => f.write_str("TotpRequest::Confirm(..)"),
        }
    }
}

/// `DELETE /admin/api/session/totp`: turn the own second factor off.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TotpOff {
    /// A valid code of the current secret.
    pub code: String,
    /// The current password's hash.
    pub pwhash: String,
}

impl std::fmt::Debug for TotpOff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TotpOff(..)")
    }
}

/// The body of `DELETE /admin/api/accounts/{name}/totp` inside the `{body, reason}` envelope: nothing.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct TotpReset {}

/// The answer of `begin`: what the authenticator app needs. The only place the secret ever leaves the server.
#[derive(Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TotpSetup {
    /// The secret in base32 (RFC 4648), to type into an app by hand.
    pub secret: String,
    /// The `otpauth://totp/...` key URI.
    pub uri: String,
    /// The issuer the app shows.
    pub issuer: String,
    /// The account name.
    pub account: String,
    /// Always `SHA1`: the one algorithm every authenticator app supports.
    pub algorithm: String,
    /// Always 6.
    pub digits: u32,
    /// Always 30.
    pub period_s: u32,
    /// Unix seconds when the setup expires.
    pub expires_at: i64,
    /// The key URI as an SVG QR code (`None` when it could not be rendered).
    pub qr_svg: Option<String>,
}

impl std::fmt::Debug for TotpSetup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TotpSetup")
            .field("account", &self.account)
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

/// The answer of the reset by a higher authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TotpResetAnswer {
    /// The account name.
    pub account: String,
    /// A second factor was enrolled and is gone now (`false`: there was none; the call changed nothing).
    pub removed: bool,
    /// How many console sessions of the account ended.
    pub ended_sessions: u32,
}

/// Mounts the routes.
pub fn mount(r: Routes<ApiState>) -> Routes<ApiState> {
    r.post("/admin/api/session/totp", post_totp)
        .delete("/admin/api/session/totp", delete_totp)
        .delete("/admin/api/accounts/{name}/totp", reset_totp)
}

fn parse<T: serde::de::DeserializeOwned>(body: &Bytes) -> ApiResult<T> {
    serde_json::from_slice(body).map_err(|e| ApiError::bad_request(format!("malformed body: {e}")))
}

/// The staff account behind the caller: owners have none (403), a flagged account must change its password first (428).
fn staff_account(who: &Identity) -> ApiResult<StaffAccount> {
    let Some(account) = who.account.clone() else {
        return Err(ApiError::forbidden_text(
            "only staff accounts have two-factor sign-in",
        ));
    };
    if who.must_change_password {
        return Err(ApiError::password_change_required());
    }
    Ok(account)
}

/// A remote session cannot turn two-factor on while it is required for remote sign-ins: a stolen cookie would enrol the
/// thief's own device (25 §1.3; the staff member enrols from the host or an SSH tunnel).
fn no_remote_enrolment(st: &ApiState, who: &Identity) -> ApiResult<()> {
    if st.admin.totp_required.load(Ordering::Relaxed) && !who.local {
        return Err(ApiError::forbidden_text(
            "two-factor can only be turned on from the host while it is required for remote sign-in",
        ));
    }
    Ok(())
}

/// The stored second factor of an account.
async fn enrolled(
    st: &ApiState,
    account: AccountId,
) -> ApiResult<Option<flyff_db::admin::TotpRecord>> {
    st.ops
        .core
        .persist
        .read(move |c| flyff_db::admin::reads::totp(c, account))
        .await
        .map_err(|e| ApiError::unavailable(format!("the two-factor table is not available: {e}")))
}

/// The key URI as an SVG QR code, drawn dark on light with its quiet zone so that it scans on either theme.
fn qr_svg(uri: &str) -> Option<String> {
    let code = qrcode::QrCode::new(uri.as_bytes()).ok()?;
    Some(
        code.render::<qrcode::render::svg::Color<'_>>()
            .min_dimensions(200, 200)
            .build(),
    )
}

/// The current password of the signed-in account: the per-IP throttle is checked first, a wrong hash is charged to it.
/// `forget_on_ok`: a right password clears the address' failures (the enrolment does; the removal waits until the code
/// has passed too, so that holding the password does not reset the counter between code guesses).
async fn check_password(
    st: &ApiState,
    caller: &Caller,
    who: &Identity,
    account: AccountId,
    pwhash: &str,
    forget_on_ok: bool,
) -> ApiResult<()> {
    let core = &st.ops.core;
    let ip = caller.client.ip;
    let unix = core.clock.unix().0;
    if let Err(locked) = core.throttle.check(ip, unix) {
        return Err(ApiError::rate_limited(
            (locked.retry_after_ms / 1000).max(1),
            "too many wrong passwords from this address",
        ));
    }
    let Some(pw) = PwHash::parse(pwhash) else {
        return Err(ApiError::invalid(
            "pwhash",
            "a password hash is 32 lower-case hex characters",
        ));
    };
    let rec = core
        .persist
        .read(move |c| queries::account_by_id(c, account))
        .await
        .map_err(|e| ApiError::unavailable(format!("the account table is not available: {e}")))?
        .ok_or_else(ApiError::invalid_credentials)?;
    match core.hasher.verify(&pw, &rec.password_hash).await {
        Ok(Verify::Ok { .. }) => {
            if forget_on_ok {
                core.throttle.success(ip);
            }
            Ok(())
        }
        Ok(Verify::Mismatch) => {
            core.throttle.fail(ip, unix);
            denied(st, caller, who, "wrong_password");
            Err(ApiError::invalid_credentials())
        }
        Err(AuthError::PermitTimeout(_)) => {
            Err(ApiError::unavailable("the password hasher is busy"))
        }
        Err(e) => Err(ApiError::internal(&format!(
            "password verification failed: {e}"
        ))),
    }
}

/// The record of a refused re-authentication on a TOTP route (`admin.session`, action `totp_denied`).
fn denied(st: &ApiState, caller: &Caller, who: &Identity, why: &str) {
    audit_session(
        st,
        caller,
        "totp_denied",
        &who.label(),
        &who.authority.letter().to_string(),
        json!({"why": why, "totp": true}),
    );
}

/// A refused code on a TOTP route: the per-IP throttle and the account's counter are charged; the answer is the uniform one.
fn code_refused(
    st: &ApiState,
    caller: &Caller,
    who: &Identity,
    account: AccountId,
    why: &str,
) -> ApiError {
    let core = &st.ops.core;
    core.throttle.fail(caller.client.ip, core.clock.unix().0);
    st.admin.totp.failed(account, core.clock.now_ms());
    denied(st, caller, who, why);
    ApiError::invalid_credentials()
}

/// The code against the stored secret with the replay rule. `Err` is the refusal, already charged.
fn check_code(
    st: &ApiState,
    caller: &Caller,
    who: &Identity,
    account: AccountId,
    secret: &[u8],
    code: &str,
) -> ApiResult<()> {
    let core = &st.ops.core;
    let step =
        totp::parse_code(code).and_then(|c| totp::matching_step(secret, &c, core.clock.unix().0));
    match step {
        None => Err(code_refused(st, caller, who, account, "totp_wrong")),
        Some(step) if !st.admin.totp.accept(account, step) => {
            Err(code_refused(st, caller, who, account, "totp_replay"))
        }
        Some(_) => Ok(()),
    }
}

/// The `admin.session` trail of a change of the own factor: the §10.3 fields (`action`, `identity`, `authority`, `remote`,
/// `totp`), `totp` being the state after the change.
fn trail<'a>(
    st: &'a ApiState,
    who: &'a Identity,
    action: &str,
    totp_after: bool,
    reason: Option<crate::admin::guard::ReasonText>,
    extra: serde_json::Value,
) -> ApiResult<AuditTrail<'a>> {
    let mut data = serde_json::Map::new();
    data.insert("action".to_owned(), json!(action));
    data.insert("identity".to_owned(), json!(who.label()));
    data.insert(
        "authority".to_owned(),
        json!(who.authority.letter().to_string()),
    );
    data.insert("remote".to_owned(), json!(!who.local));
    data.insert("totp".to_owned(), json!(totp_after));
    if let serde_json::Value::Object(m) = extra {
        data.extend(m);
    }
    AuditTrail::begin(
        &st.ops.audit,
        who,
        flyff_db::audit::ev::ADMIN_SESSION,
        reason,
        serde_json::Value::Object(data),
    )
}

// ---- POST /admin/api/session/totp --------------------------------------------------------------------------------------

async fn post_totp(
    State(st): State<ApiState>,
    caller: Caller,
    Who(who): Who,
    body: Bytes,
) -> ApiResult<Response> {
    let account = staff_account(&who)?;
    match parse::<TotpRequest>(&body)? {
        TotpRequest::Begin => begin(&st, &who, &account).await,
        TotpRequest::Confirm { code, pwhash } => {
            confirm(&st, &caller, &who, &account, &code, &pwhash).await
        }
    }
}

/// Step one: a fresh secret, kept in memory for ten minutes. Nothing is written, so nothing is audited.
async fn begin(st: &ApiState, who: &Identity, account: &StaffAccount) -> ApiResult<Response> {
    no_remote_enrolment(st, who)?;
    if enrolled(st, account.id).await?.is_some() {
        return Err(ApiError::invalid(
            "step",
            "two-factor is already on: turn it off first",
        ));
    }
    let secret =
        totp::new_secret().ok_or_else(|| ApiError::internal("the OS random source failed"))?;
    let session = who.session.as_ref().map_or("", |s| s.id.as_str());
    let core = &st.ops.core;
    st.admin
        .totp
        .begin_setup(account.id, session, secret, core.clock.now_ms());
    let uri = totp::key_uri(&account.name, &secret);
    Ok(Json(TotpSetup {
        secret: totp::base32(&secret),
        qr_svg: qr_svg(&uri),
        uri,
        issuer: ISSUER.to_owned(),
        account: account.name.clone(),
        algorithm: "SHA1".to_owned(),
        digits: totp::DIGITS,
        period_s: u32::try_from(STEP_S).unwrap_or(30),
        expires_at: core.clock.unix().0.saturating_add(SETUP_TTL_S),
    })
    .into_response())
}

/// Step two: the password and a code from the new secret write the row and end the account's other sessions.
async fn confirm(
    st: &ApiState,
    caller: &Caller,
    who: &Identity,
    account: &StaffAccount,
    code: &str,
    pwhash: &str,
) -> ApiResult<Response> {
    no_remote_enrolment(st, who)?;
    check_password(st, caller, who, account.id, pwhash, true).await?;
    let core = &st.ops.core;
    let session = who.session.as_ref().map_or("", |s| s.id.as_str());
    let Some(setup) = st
        .admin
        .totp
        .setup(account.id, session, core.clock.now_ms())
    else {
        return Err(ApiError::invalid("step", "the setup expired: start again"));
    };
    let Some(step) = totp::parse_code(code)
        .and_then(|c| totp::matching_step(&setup.secret, &c, core.clock.unix().0))
    else {
        // the secret is not a credential yet: the setup stays, nothing is charged
        return Err(ApiError::invalid(
            "code",
            "that code does not match: check the code and the clock of the device",
        ));
    };
    let trail = trail(st, who, "totp_enrol", true, None, json!({}))?;
    let batch = AtomicBatch::of_ops(
        AtomicKind::Other("admin.totp"),
        vec![DbOp::UpsertTotp {
            account_id: account.id,
            secret: setup.secret.to_vec(),
            enabled_at: core.clock.unix(),
        }],
    );
    if let Err(e) = core.persist.call(batch).await {
        trail.failed(&e.to_string());
        return Err(ApiError::unavailable(
            "the two-factor secret could not be saved",
        ));
    }
    // the code that proved the secret cannot sign in as well
    let _ = st.admin.totp.accept(account.id, step);
    st.admin.totp.clear_setup(account.id);
    let keep = who.session.as_ref().map(|s| s.id.clone());
    let ended = st
        .admin
        .sessions
        .end_account(account.id, EndReason::Revoked, keep.as_deref());
    trail.done(json!({"ended_sessions": ended}));
    Ok(Json(session_info(st, who).await).into_response())
}

// ---- DELETE /admin/api/session/totp ------------------------------------------------------------------------------------

async fn delete_totp(
    State(st): State<ApiState>,
    caller: Caller,
    Who(who): Who,
    body: Bytes,
) -> ApiResult<Response> {
    let account = staff_account(&who)?;
    let b: TotpOff = parse(&body)?;
    let Some(row) = enrolled(&st, account.id).await? else {
        return Err(ApiError::invalid("code", "two-factor is not on"));
    };
    if st.admin.totp_required.load(Ordering::Relaxed) && !who.local {
        return Err(ApiError::forbidden_text(
            "two-factor is required for remote sign-in: turn it off from the host",
        ));
    }
    let core = &st.ops.core;
    check_password(&st, &caller, &who, account.id, &b.pwhash, false).await?;
    if st.admin.totp.locked(account.id, core.clock.now_ms()) {
        core.throttle.fail(caller.client.ip, core.clock.unix().0);
        denied(&st, &caller, &who, "totp_locked");
        return Err(ApiError::invalid_credentials());
    }
    check_code(&st, &caller, &who, account.id, &row.secret, &b.code)?;
    core.throttle.success(caller.client.ip);
    let trail = trail(&st, &who, "totp_remove", false, None, json!({}))?;
    let batch = AtomicBatch::of_ops(
        AtomicKind::Other("admin.totp"),
        vec![DbOp::DeleteTotp {
            account_id: account.id,
        }],
    );
    if let Err(e) = core.persist.call(batch).await {
        trail.failed(&e.to_string());
        return Err(ApiError::unavailable(
            "the two-factor secret could not be removed",
        ));
    }
    st.admin.totp.forget(account.id);
    trail.done(json!({}));
    Ok(Json(session_info(&st, &who).await).into_response())
}

// ---- DELETE /admin/api/accounts/{name}/totp ----------------------------------------------------------------------------

/// The reset by a higher authority: a lost phone. `act.accounts`; the body envelope is extracted first, so a malformed
/// body is a `400` before any rule is looked at.
async fn reset_totp(
    State(st): State<ApiState>,
    Who(who): Who,
    Path(name): Path<String>,
    MutationJson(m): MutationJson<TotpReset>,
) -> ApiResult<Json<TotpResetAnswer>> {
    let reason = st.admin.policy.get().reason_required(m.reason.as_ref())?;
    let parsed = AccountName::parse(&name).map_err(|_| ApiError::not_found("no such account"))?;
    let wanted = parsed.as_str().to_owned();
    let core = &st.ops.core;
    let rec = core
        .persist
        .read(move |c| queries::find_account(c, &wanted))
        .await
        .map_err(|e| ApiError::unavailable(format!("the account table is not available: {e}")))?
        .ok_or_else(|| ApiError::not_found("no such account"))?;
    if who.account.as_ref().is_some_and(|a| a.id == rec.id) {
        return Err(ApiError::forbidden_text(
            "use your own profile to turn two-factor off",
        ));
    }
    rank::check_target(
        &who,
        st.admin.resolver.target_authority(
            Authority::from_db_byte(rec.authority),
            None,
            &rec.name,
            None,
        ),
    )?;
    let trail = trail(
        &st,
        &who,
        "totp_reset",
        false,
        Some(reason),
        json!({"target": rec.name}),
    )?;
    let batch = AtomicBatch::of_ops(
        AtomicKind::Other("admin.totp"),
        vec![DbOp::DeleteTotp { account_id: rec.id }],
    );
    let removed = match core.persist.call(batch).await {
        Ok(AtomicOutcome::Changed(n)) => n > 0,
        Ok(_) => false,
        Err(e) => {
            trail.failed(&e.to_string());
            return Err(ApiError::unavailable(
                "the two-factor secret could not be removed",
            ));
        }
    };
    st.admin.totp.forget(rec.id);
    // a lost or stolen factor: whoever holds a session of the account signed in with it (or before it existed)
    let ended = if removed {
        st.admin
            .sessions
            .end_account(rec.id, EndReason::Revoked, None)
    } else {
        0
    };
    trail.done(json!({"removed": removed, "ended_sessions": ended}));
    Ok(Json(TotpResetAnswer {
        account: rec.name,
        removed,
        ended_sessions: u32::try_from(ended).unwrap_or(u32::MAX),
    }))
}

/// Declares the types of this module for the generated TypeScript of the DTO catalogue (feature `ts`).
#[cfg(feature = "ts")]
pub fn declare_ts(out: &mut String, cfg: &ts_rs::Config) {
    use ts_rs::TS;
    for decl in [
        TotpRequest::decl(cfg),
        TotpOff::decl(cfg),
        TotpSetup::decl(cfg),
        TotpResetAnswer::decl(cfg),
    ] {
        out.push_str("export ");
        out.push_str(&decl);
        out.push('\n');
    }
}
