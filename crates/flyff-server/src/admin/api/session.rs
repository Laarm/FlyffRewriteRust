//! The session routes (Plan/25 §1.3, §11.3; WP-ADMIN-01):
//!
//! | route | does |
//! |---|---|
//! | `POST /admin/api/session {account, pwhash, totp?}` | staff login: the pwhash is verified against the stored argon2id through the lobby's `HashPermits`; 12's per-IP `LoginThrottle` applies; a banned account, an authority below `L` and `admin.staff_login = false` answer the same `401 invalid_credentials`; an account with a second factor needs a valid `totp` code on every sign-in, and with `admin.totp_required` a remote sign-in of an account without one is refused (WP-ADMIN-28) |
//! | `POST /admin/api/session {token}` | the owner-token exchange: constant-time comparison, failures charged to the 60-per-second limiter |
//! | `GET /admin/api/session` | `{identity, kind, account, authority, caps[], acting_character, must_change_password, remote, tls, totp_enabled, ...}` |
//! | `DELETE /admin/api/session` | sign out: ends the session, revokes the `flyff_assets` token, clears both cookies |
//! | `PUT /admin/api/session/password {old_pwhash, new_pwhash}` | stores `argon2id(new)` through `AccountAdmin`, clears `must_change_password`, ends the account's other sessions |
//! | `POST /admin/api/session/totp`, `DELETE /admin/api/session/totp` | turn the own second factor on (two steps) and off: `api/totp.rs` (WP-ADMIN-28) |
//! | `DELETE /admin/api/accounts/{name}/totp` | the reset of another account's second factor by a higher authority (`act.accounts`): `api/totp.rs` |
//!
//! A login sets `flyff_admin` (`HttpOnly; SameSite=Strict; Path=/admin`) and, with `web.public_assets = false`,
//! `flyff_assets`. Every login, refusal, sign-out and password change is audited as `admin.session`.
//! The TOTP rule of the staff login is `staff_login`'s (`second_factor`); the algorithm is `admin::totp`.

use super::ApiState;
use crate::admin::auth::{Caller, Who};
use crate::admin::dto::{ActingCharacter, LoginRequest, PasswordChange, SessionInfo};
use crate::admin::error::{ApiError, ApiResult};
use crate::admin::identity::{Identity, STAFF_MIN, StaffAccount};
use crate::admin::session::{AccountSnapshot, Created, EndReason, NewSession, cookie_clear_header};
use crate::admin::totp;
use crate::audit::Actor;
use crate::http::asset_auth;
use crate::lobby::accounts::{AccountName, AdminError, PasswordOptions};
use crate::lobby::auth::{AuthError, PwHash, Verify};
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use flyff_common::AccountId;
use flyff_common::gm::Authority;
use flyff_db::{AtomicBatch, AtomicKind, DbOp, queries};
use serde::Serialize;
use serde_json::json;
use std::time::Instant;

/// Mounts the session routes.
pub fn mount(r: super::Routes<ApiState>) -> super::Routes<ApiState> {
    r.post("/admin/api/session", login)
        .get("/admin/api/session", info)
        .delete("/admin/api/session", logout)
        .put("/admin/api/session/password", change_password)
}

fn json_response<T: Serialize>(status: StatusCode, body: &T) -> Response {
    (status, axum::Json(body)).into_response()
}

fn parse<T: serde::de::DeserializeOwned>(body: &Bytes) -> ApiResult<T> {
    serde_json::from_slice(body).map_err(|e| ApiError::bad_request(format!("malformed body: {e}")))
}

/// An `AdminError` of `AccountAdmin` as an API error.
fn admin_error(e: &AdminError) -> ApiError {
    match e {
        AdminError::InvalidPwHash => {
            ApiError::invalid("new_pwhash", "that password is not allowed")
        }
        AdminError::NotFound => ApiError::not_found("no such account"),
        AdminError::Hash(AuthError::PermitTimeout(_)) => {
            ApiError::unavailable("the password hasher is busy")
        }
        other => ApiError::internal(&other.to_string()),
    }
}

/// `admin.session` audit record (25 §10.3).
pub(super) fn audit_session(
    st: &ApiState,
    caller: &Caller,
    action: &str,
    identity: &str,
    authority: &str,
    extra: serde_json::Value,
) {
    let mut data = serde_json::Map::new();
    data.insert("action".to_owned(), json!(action));
    data.insert("identity".to_owned(), json!(identity));
    data.insert("authority".to_owned(), json!(authority));
    data.insert("remote".to_owned(), json!(!caller.local));
    data.insert("totp".to_owned(), json!(false));
    if let serde_json::Value::Object(m) = extra {
        data.extend(m);
    }
    st.ops.audit.emit(
        "admin.session",
        &Actor::ip(caller.client.ip),
        serde_json::Value::Object(data),
    );
}

/// The in-world character of a staff account (25 §1.5): the registry knows the id, the character list the name.
async fn acting_character(st: &ApiState, account: AccountId) -> Option<ActingCharacter> {
    let character = st.ops.core.registry.view(account)?.character?;
    let list = st
        .ops
        .persist
        .read(move |c| queries::character_list(c, account))
        .await
        .ok()?;
    list.into_iter()
        .find(|c| c.id == character)
        .map(|c| ActingCharacter {
            id: c.id.0,
            name: c.name,
        })
}

/// The `SessionInfo` of an identity.
pub async fn session_info(st: &ApiState, id: &Identity) -> SessionInfo {
    let acting_character = match &id.account {
        Some(a) => acting_character(st, a.id).await,
        None => None,
    };
    let (idle_expires_at, expires_at) = id
        .session
        .as_ref()
        .and_then(|s| st.admin.sessions.expiry(&s.id))
        .map_or((None, None), |(i, e)| (Some(i), Some(e)));
    let totp_enabled = match &id.account {
        Some(a) => {
            let account = a.id;
            matches!(
                st.ops
                    .core
                    .persist
                    .read(move |c| flyff_db::admin::reads::totp(c, account))
                    .await,
                Ok(Some(_))
            )
        }
        None => false,
    };
    SessionInfo {
        identity: id.label(),
        kind: id.kind,
        account: id.account.as_ref().map(|a| a.name.clone()),
        authority: id.authority.letter().to_string(),
        caps: id
            .caps(&st.admin.caps)
            .into_iter()
            .map(str::to_owned)
            .collect(),
        acting_character,
        must_change_password: id.must_change_password,
        remote: !id.local,
        tls: id.tls,
        totp_enabled,
        totp_required: st
            .admin
            .totp_required
            .load(std::sync::atomic::Ordering::Relaxed),
        idle_expires_at,
        expires_at,
    }
}

/// A 200 answer with the cookies of a new session.
async fn created_response(st: &ApiState, created: &Created) -> Response {
    let info = session_info(st, &created.identity).await;
    let mut resp = json_response(StatusCode::OK, &info);
    let h = resp.headers_mut();
    h.append(header::SET_COOKIE, created.cookie.clone());
    if let Some(a) = &created.assets_cookie {
        h.append(header::SET_COOKIE, a.clone());
    }
    resp
}

// ---- POST /admin/api/session --------------------------------------------------------------------------------------

async fn login(
    State(st): State<ApiState>,
    caller: Caller,
    body: Bytes,
) -> Result<Response, ApiError> {
    let req: LoginRequest = parse(&body)?;
    match req {
        LoginRequest::Token { token } => token_exchange(&st, &caller, &token).await,
        LoginRequest::Staff {
            account,
            pwhash,
            totp,
        } => staff_login(&st, &caller, &account, &pwhash, totp.as_deref()).await,
    }
}

async fn token_exchange(st: &ApiState, caller: &Caller, token: &str) -> Result<Response, ApiError> {
    let ip = caller.client.ip;
    let now = Instant::now();
    let auth_throttled = !caller.local && st.ops.auth.throttled(ip, now);
    if auth_throttled {
        return Err(ApiError::rate_limited(1, "too many attempts"));
    }
    if !st.ops.auth.token_matches(token) {
        st.admin.metrics.login_failed(!caller.local);
        if !caller.local {
            st.ops.auth.token_failed(ip, now);
        }
        audit_session(
            st,
            caller,
            "denied",
            "owner:token",
            "P",
            json!({"kind": "token"}),
        );
        return Err(ApiError::invalid_credentials());
    }
    let identity = Identity::owner_token(ip, caller.local, caller.client.https, None);
    let created = st
        .admin
        .sessions
        .create(NewSession {
            identity,
            snapshot: None,
        })
        .ok_or_else(|| ApiError::internal("the OS random source failed"))?;
    audit_session(st, caller, "token_exchange", "owner:token", "P", json!({}));
    Ok(created_response(st, &created).await)
}

async fn staff_login(
    st: &ApiState,
    caller: &Caller,
    account: &str,
    pwhash: &str,
    code: Option<&str>,
) -> Result<Response, ApiError> {
    let ip = caller.client.ip;
    let core = &st.ops.core;
    // every refusal is the same 401; only the audit record tells the cases apart (`totp`: the account has a second factor)
    let denied = |why: &str, totp: bool| {
        st.admin.metrics.login_failed(!caller.local);
        audit_session(
            st,
            caller,
            "denied",
            &format!("staff:{}", account.chars().take(32).collect::<String>()),
            "",
            json!({"kind": "staff", "why": why, "totp": totp}),
        );
        ApiError::invalid_credentials()
    };
    if !st.admin.staff_login {
        return Err(denied("staff_login_off", false));
    }
    let unix = core.clock.unix().0;
    if let Err(locked) = core.throttle.check(ip, unix) {
        let secs = (locked.retry_after_ms / 1000).max(1);
        return Err(ApiError::rate_limited(
            secs,
            "too many wrong passwords from this address",
        ));
    }
    let (Some(pw), Ok(name)) = (PwHash::parse(pwhash), AccountName::parse(account)) else {
        return Err(denied("malformed", false));
    };
    let wanted = name.as_str().to_owned();
    let rec = core
        .persist
        .read(move |c| queries::find_account(c, &wanted))
        .await
        .map_err(|e| ApiError::unavailable(format!("the account table is not available: {e}")))?;
    let Some(rec) = rec else {
        return Err(denied("unknown_account", false));
    };
    let verdict = core.hasher.verify(&pw, &rec.password_hash).await;
    let needs_rehash = match verdict {
        Ok(Verify::Ok { needs_rehash }) => needs_rehash,
        Ok(Verify::Mismatch) => {
            core.throttle.fail(ip, unix);
            return Err(denied("wrong_password", false));
        }
        Err(AuthError::PermitTimeout(_)) => {
            return Err(ApiError::unavailable("the password hasher is busy"));
        }
        Err(e) => {
            return Err(ApiError::internal(&format!(
                "password verification failed: {e}"
            )));
        }
    };
    let id = rec.id;
    let now = core.clock.unix();
    let banned = core
        .persist
        .read(move |c| queries::active_ban(c, id, now))
        .await
        .map_err(|e| ApiError::unavailable(format!("the ban table is not available: {e}")))?
        .is_some();
    if banned {
        return Err(denied("banned", false));
    }
    let account_db = Authority::from_db_byte(rec.authority);
    let authority = st.admin.resolver.staff_authority(account_db, &rec.name);
    if authority < STAFF_MIN {
        return Err(denied("authority", false));
    }
    // the second factor (25 §1.3, WP-ADMIN-28); the address' failures are forgotten only once it has passed
    let used_code = match second_factor(st, caller, id, code).await? {
        SecondFactor::Passed { used_code } => used_code,
        SecondFactor::Refused { why, enrolled } => {
            core.throttle.fail(ip, unix);
            return Err(denied(why, enrolled));
        }
    };
    core.throttle.success(ip);
    // the transparent upgrade of an old hash (12 §5.2); the session's snapshot is taken of the stored result, so the
    // poll of the account table does not read it as a password change
    let mut stored = rec.password_hash.clone();
    if needs_rehash && let Ok(phc) = core.hasher.hash(&pw).await {
        let batch = AtomicBatch::of_ops(
            AtomicKind::Other("account.password"),
            vec![DbOp::SetAccountPassword {
                account_id: rec.id,
                password_hash: phc.clone(),
            }],
        );
        if core.persist.call(batch).await.is_ok() {
            stored = phc;
        }
    }
    let identity = Identity::staff(
        StaffAccount {
            id: rec.id,
            name: rec.name.clone(),
        },
        authority,
        ip,
        caller.local,
        caller.client.https,
        None,
        rec.must_change_password,
    );
    let created = st
        .admin
        .sessions
        .create(NewSession {
            identity,
            snapshot: Some(AccountSnapshot {
                authority_db: account_db,
                password_digest: AccountSnapshot::digest(&stored),
            }),
        })
        .ok_or_else(|| ApiError::internal("the OS random source failed"))?;
    audit_session(
        st,
        caller,
        "login",
        &created.identity.label(),
        &authority.letter().to_string(),
        json!({"must_change_password": rec.must_change_password, "totp": used_code}),
    );
    Ok(created_response(st, &created).await)
}

/// What the second-factor step of a staff login decided.
enum SecondFactor {
    /// Nothing is in the way; `used_code`: the account has a second factor and the code was valid.
    Passed { used_code: bool },
    /// Refused (the audit `why`); `enrolled`: the account has a second factor.
    Refused { why: &'static str, enrolled: bool },
}

/// The TOTP rule of the staff login (WP-ADMIN-28), after the password, the ban and the authority checks:
///
/// * enrolled: the account's code checks must not be locked (`totp_locked`, nothing is verified), a code must be given
///   (`totp_missing`), parse and match within one step either side (`totp_wrong`, counted against the account) and not
///   be a step that was accepted before (`totp_replay`, counted too). A code is required from the host as well.
/// * not enrolled: `admin.totp_required` refuses a remote sign-in (`totp_not_enrolled`); otherwise the step passes and
///   a code sent anyway is ignored.
///
/// `Err` is a database that does not answer (503). The caller charges every refusal to 12's per-IP throttle, so that
/// the throttle is no oracle for the password.
async fn second_factor(
    st: &ApiState,
    caller: &Caller,
    id: AccountId,
    code: Option<&str>,
) -> Result<SecondFactor, ApiError> {
    let core = &st.ops.core;
    let row = core
        .persist
        .read(move |c| flyff_db::admin::reads::totp(c, id))
        .await
        .map_err(|e| {
            ApiError::unavailable(format!("the two-factor table is not available: {e}"))
        })?;
    let Some(row) = row else {
        if st
            .admin
            .totp_required
            .load(std::sync::atomic::Ordering::Relaxed)
            && !caller.local
        {
            return Ok(SecondFactor::Refused {
                why: "totp_not_enrolled",
                enrolled: false,
            });
        }
        return Ok(SecondFactor::Passed { used_code: false });
    };
    let refused = |why| {
        Ok(SecondFactor::Refused {
            why,
            enrolled: true,
        })
    };
    let now_ms = core.clock.now_ms();
    if st.admin.totp.locked(id, now_ms) {
        return refused("totp_locked");
    }
    let Some(code) = code.filter(|c| !c.trim().is_empty()) else {
        return refused("totp_missing");
    };
    let Some(step) = totp::parse_code(code)
        .and_then(|c| totp::matching_step(&row.secret, &c, core.clock.unix().0))
    else {
        st.admin.totp.failed(id, now_ms);
        return refused("totp_wrong");
    };
    if !st.admin.totp.accept(id, step) {
        st.admin.totp.failed(id, now_ms);
        return refused("totp_replay");
    }
    Ok(SecondFactor::Passed { used_code: true })
}

// ---- GET / DELETE /admin/api/session ------------------------------------------------------------------------------

async fn info(State(st): State<ApiState>, Who(id): Who) -> Response {
    json_response(StatusCode::OK, &session_info(&st, &id).await)
}

async fn logout(State(st): State<ApiState>, caller: Caller, Who(id): Who) -> Response {
    if let Some(s) = &id.session {
        st.admin.sessions.end(&s.id, EndReason::Logout);
    }
    audit_session(
        &st,
        &caller,
        "logout",
        &id.label(),
        &id.authority.letter().to_string(),
        json!({}),
    );
    let mut resp = json_response(StatusCode::OK, &json!({}));
    let h = resp.headers_mut();
    h.append(header::SET_COOKIE, cookie_clear_header(id.tls));
    h.append(header::SET_COOKIE, asset_auth::cookie_clear_header(id.tls));
    resp
}

// ---- PUT /admin/api/session/password ------------------------------------------------------------------------------

async fn change_password(
    State(st): State<ApiState>,
    caller: Caller,
    Who(id): Who,
    body: Bytes,
) -> Result<Response, ApiError> {
    let Some(acct) = id.account.clone() else {
        return Err(ApiError::forbidden_text(
            "only staff sessions have a password",
        ));
    };
    let b: PasswordChange = parse(&body)?;
    let (Some(old), Some(new)) = (PwHash::parse(&b.old_pwhash), PwHash::parse(&b.new_pwhash))
    else {
        return Err(ApiError::invalid(
            "old_pwhash",
            "a password hash is 32 lower-case hex characters",
        ));
    };
    if old == new {
        return Err(ApiError::invalid(
            "new_pwhash",
            "the new password must differ from the old one",
        ));
    }
    let core = &st.ops.core;
    let ip = caller.client.ip;
    let unix = core.clock.unix().0;
    if let Err(locked) = core.throttle.check(ip, unix) {
        return Err(ApiError::rate_limited(
            (locked.retry_after_ms / 1000).max(1),
            "too many wrong passwords from this address",
        ));
    }
    let account = acct.id;
    let rec = core
        .persist
        .read(move |c| queries::account_by_id(c, account))
        .await
        .map_err(|e| ApiError::unavailable(format!("the account table is not available: {e}")))?
        .ok_or_else(ApiError::invalid_credentials)?;
    match core.hasher.verify(&old, &rec.password_hash).await {
        Ok(Verify::Ok { .. }) => core.throttle.success(ip),
        Ok(Verify::Mismatch) => {
            core.throttle.fail(ip, unix);
            audit_session(&st, &caller, "password_denied", &id.label(), "", json!({}));
            return Err(ApiError::invalid_credentials());
        }
        Err(AuthError::PermitTimeout(_)) => {
            return Err(ApiError::unavailable("the password hasher is busy"));
        }
        Err(e) => {
            return Err(ApiError::internal(&format!(
                "password verification failed: {e}"
            )));
        }
    }
    let keep = id.session.as_ref().map(|s| s.id.clone());
    st.ops
        .accounts
        .set_password_with(
            &acct.name,
            &new,
            &id.label(),
            PasswordOptions {
                must_change: false,
                keep_session: keep.clone(),
            },
        )
        .await
        .map_err(|e| admin_error(&e))?;
    // the session stays: it learns the new stored hash, so the poll does not read the change as foreign
    let fresh = core
        .persist
        .read(move |c| queries::account_by_id(c, account))
        .await
        .ok()
        .flatten();
    if let Some(s) = &keep {
        st.admin.sessions.password_changed(
            s,
            fresh
                .as_ref()
                .map(|r| AccountSnapshot::digest(&r.password_hash)),
        );
    }
    audit_session(
        &st,
        &caller,
        "password_change",
        &id.label(),
        &id.authority.letter().to_string(),
        json!({}),
    );
    let mut identity = id.clone();
    identity.must_change_password = false;
    Ok(json_response(
        StatusCode::OK,
        &session_info(&st, &identity).await,
    ))
}

// ---- the acting GM character (25 §1.5) ----------------------------------------------------------------------------

/// The acting GM character of an action that moves or uses the admin's own in-game character ("Summon to me", "Go to",
/// "Go here"; 25 §1.5). `as_character` is what the client sent with the action.
///
/// * **Staff**: the in-world character of the staff account (at most one per account is in the world). A different
///   `as_character` is `403 forbidden` (a staff member acts only as its own character); none in the world is `404`.
/// * **Owner**: `as_character` is required and must be an online character with an authority of `L` or higher.
pub async fn resolve_acting(
    st: &ApiState,
    who: &Identity,
    as_character: Option<u32>,
) -> ApiResult<ActingCharacter> {
    match &who.account {
        Some(account) => {
            let found = acting_character(st, account.id).await;
            pick_staff_acting(found, as_character)
        }
        None => {
            let id = as_character.ok_or_else(|| {
                ApiError::invalid(
                    "as_character",
                    "pick the online GM character the action acts as",
                )
            })?;
            let rows = st.ops.online(None, who.actor()).await?;
            pick_owner_acting(&rows, id)
        }
    }
}

fn pick_staff_acting(
    found: Option<ActingCharacter>,
    wanted: Option<u32>,
) -> ApiResult<ActingCharacter> {
    let found =
        found.ok_or_else(|| ApiError::not_found("log in a GM character in the game first"))?;
    match wanted {
        Some(w) if w != found.id => Err(ApiError::forbidden_text(
            "a staff member acts only as the character of its own account",
        )),
        _ => Ok(found),
    }
}

fn pick_owner_acting(
    rows: &[flyff_game::input::OnlineEntry],
    wanted: u32,
) -> ApiResult<ActingCharacter> {
    let row = rows
        .iter()
        .find(|r| r.character_id.0 == wanted)
        .ok_or_else(|| ApiError::not_found("that character is not in the world"))?;
    let letter = u8::try_from(row.authority).map(Authority::from_u8).ok();
    if letter.is_none_or(|a| a < STAFF_MIN) {
        return Err(ApiError::forbidden_text(
            "the acting character must be a game master (authority L or higher)",
        ));
    }
    Ok(ActingCharacter {
        id: row.character_id.0,
        name: row.name.clone(),
    })
}
