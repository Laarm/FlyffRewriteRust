//! The six account routes of the admin API (Plan/12 §5.5; WP-AUTH-19): the online half of `flyff account ...`, the
//! account section of the `/admin` page and the console words. They are plain handlers over [`AccountAdmin`] (the
//! single path by which accounts are written, with its online effects); the router is mounted by
//! `admin::routes` behind the authorization middleware of 10 §13.2.
//!
//! | route | body -> answer | errors |
//! |---|---|---|
//! | `GET /admin/api/accounts?name=&gm=&online=&limit=` | -> `[AccountSummary]` | |
//! | `POST /admin/api/accounts` | `{name, pwhash, auth?, email?, by?}` -> 201 `{id, name}` | 400 `invalid_name` `invalid_level` `invalid_pwhash` `invalid_email`; 409 `exists` |
//! | `PUT /admin/api/accounts/{name}/password` | `{pwhash, by?}` -> `{}` | 404 `not_found` |
//! | `PUT /admin/api/accounts/{name}/authority` | `{level, character?, by?}` -> `{overridden_by_gm_config}` | 400 `invalid_level`; 404 `not_found` |
//! | `POST /admin/api/accounts/{name}/ban` | `{until?: unix_s, reason?, by?}` -> `{}` | 400 `invalid_until`; 404 `not_found` |
//! | `DELETE /admin/api/accounts/{name}/ban` | -> `{}` (lifts every active ban) | 404 `not_found` / `no_active_ban` |
//!
//! Error bodies are `{"error": "<token>", "detail": "<text>"}`; 500 `db` for persistence errors. A password is only
//! ever the client-side `pwhash` (32 hex): the plaintext never reaches the server.
//!
//! **Identity (Plan/25 §1.4, §1.6; WP-ADMIN-01).** The caller is the identity the authentication layer resolved. A body
//! `by` is honoured only without a cookie session (the CLI sends `by: "cli"`); under a session the audit actor is the
//! session's (`staff:<account>`, `owner:token`), and the default without `by` is the identity's label. A staff
//! identity is bound by rank protection: it cannot reset, ban, unban or re-rank an account whose resolved authority
//! is not below its own letter (`403 target_outranks`), cannot set a letter above its own, and cannot change the
//! authority of its own account. The list hides `last_login_ip` from an identity without `view.pii`. An
//! administrator's password reset (a console session resetting another account) sets `must_change_password`.

use super::accounts::{
    AccountAdmin, AccountFilter, AccountName, AdminError, NewAccount, PasswordOptions,
};
use super::auth::PwHash;
use crate::admin::api::{RouteReport, Routes};
use crate::admin::auth::Who;
use crate::admin::caps::Cap;
use crate::admin::error::ApiError;
use crate::admin::guard::rank;
use crate::admin::identity::Identity;
use crate::admin::shared::AdminShared;
use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use flyff_common::UnixTime;
use flyff_common::gm::Authority;
use flyff_db::{CreatedVia, queries};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

/// What the handlers need.
#[derive(Clone)]
pub struct AccountApi {
    /// The service with its online effects.
    pub admin: AccountAdmin,
    /// The admin console's shared state (capabilities, the authority resolver for rank protection).
    pub shared: Arc<AdminShared>,
}

/// The routes, mounted through [`Routes`] (each is in the catalogue of `admin::api::CATALOG`) and merged into the
/// admin router.
pub fn mount(admin: AccountAdmin, shared: &Arc<AdminShared>, report: &mut RouteReport) -> Router {
    Routes::<AccountApi>::new()
        .get("/admin/api/accounts", list)
        .post("/admin/api/accounts", create)
        .put("/admin/api/accounts/{name}/password", set_password)
        .put("/admin/api/accounts/{name}/authority", set_authority)
        .post("/admin/api/accounts/{name}/ban", ban)
        .delete("/admin/api/accounts/{name}/ban", unban)
        .finish(
            AccountApi {
                admin,
                shared: Arc::clone(shared),
            },
            report,
        )
}

/// `{"error": token, "detail": text}`.
fn error_body(status: StatusCode, token: &str, detail: &str) -> Response {
    (
        status,
        axum::Json(json!({"error": token, "detail": detail})),
    )
        .into_response()
}

fn admin_error(e: &AdminError) -> Response {
    let status = StatusCode::from_u16(e.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    // persistence details stay in the log, not in the answer
    let detail = match e {
        AdminError::Db(_) | AdminError::Hash(_) => "internal error".to_owned(),
        other => other.to_string(),
    };
    if matches!(e, AdminError::Db(_) | AdminError::Hash(_)) {
        tracing::error!("admin API: {e}");
    }
    error_body(status, e.token(), &detail)
}

fn bad_json(e: &serde_json::Error) -> Response {
    error_body(
        StatusCode::BAD_REQUEST,
        "bad_request",
        &format!("malformed JSON body: {e}"),
    )
}

/// Parses a JSON body (an empty body is `{}`).
fn parse<T: for<'de> Deserialize<'de>>(body: &Bytes) -> Result<T, Box<Response>> {
    let text: &[u8] = if body.iter().all(u8::is_ascii_whitespace) {
        b"{}"
    } else {
        body
    };
    serde_json::from_slice(text).map_err(|e| Box::new(bad_json(&e)))
}

/// The audit actor of a change: the session's label under a cookie session, else the body `by` (the CLI sends
/// `"cli"`), else the identity's label.
fn by_for(who: &Identity, by: Option<String>) -> String {
    if who.session.is_some() {
        return who.label();
    }
    by.filter(|b| !b.trim().is_empty())
        .map_or_else(|| who.label(), |b| b.chars().take(32).collect())
}

/// The account behind `name` and its resolved authority, for rank protection. Owners skip the lookup.
async fn rank_target(
    api: &AccountApi,
    who: &Identity,
    name: &str,
) -> Result<Option<(flyff_db::model::AccountRecord, Authority)>, Box<Response>> {
    if who.is_owner() {
        return Ok(None);
    }
    let Ok(parsed) = AccountName::parse(name) else {
        return Err(Box::new(admin_error(&AdminError::InvalidName)));
    };
    let wanted = parsed.as_str().to_owned();
    let rec = api
        .admin
        .persist()
        .read(move |c| queries::find_account(c, &wanted))
        .await
        .map_err(|e| Box::new(admin_error(&AdminError::Db(e))))?
        .ok_or_else(|| Box::new(admin_error(&AdminError::NotFound)))?;
    let resolved = api.shared.resolver.target_authority(
        Authority::from_db_byte(rec.authority),
        None,
        &rec.name,
        None,
    );
    Ok(Some((rec, resolved)))
}

fn api_error(e: &ApiError) -> Response {
    e.clone().into_response()
}

#[derive(Deserialize)]
struct ListQuery {
    name: Option<String>,
    #[serde(default)]
    gm: bool,
    #[serde(default)]
    online: bool,
    limit: Option<u32>,
}

async fn list(
    State(api): State<AccountApi>,
    Who(who): Who,
    Query(q): Query<ListQuery>,
) -> Response {
    let filter = AccountFilter {
        name_substr: q.name,
        gm_only: q.gm,
        online_only: q.online,
        limit: q.limit.unwrap_or(1000).clamp(1, 100_000),
    };
    match api.admin.list(filter).await {
        Ok(mut rows) => {
            // addresses are PII (25 §12 rule 12): only an identity with `view.pii` sees them
            if !who.has(&api.shared.caps, Cap::ViewPii) {
                for r in &mut rows {
                    r.last_login_ip = None;
                }
            }
            axum::Json(rows).into_response()
        }
        Err(e) => admin_error(&e),
    }
}

#[derive(Deserialize)]
struct CreateBody {
    name: String,
    pwhash: String,
    auth: Option<String>,
    email: Option<String>,
    by: Option<String>,
}

#[derive(Serialize)]
struct Created {
    id: u32,
    name: String,
}

async fn create(State(api): State<AccountApi>, Who(who): Who, body: Bytes) -> Response {
    let b: CreateBody = match parse(&body) {
        Ok(b) => b,
        Err(r) => return *r,
    };
    let Some(pw) = PwHash::parse(&b.pwhash) else {
        return admin_error(&AdminError::InvalidPwHash);
    };
    let Ok(name) = AccountName::parse(&b.name) else {
        return admin_error(&AdminError::InvalidName);
    };
    let authority = match b.auth.as_deref().map(Authority::parse_token) {
        None => Authority::GENERAL,
        Some(Ok(a)) => a,
        Some(Err(_)) => return admin_error(&AdminError::InvalidLevel),
    };
    // staff cannot create an account with a letter above their own (25 §1.6)
    if !who.is_owner() && authority > who.authority {
        return api_error(&ApiError::forbidden_text(
            "a letter above your own cannot be granted",
        ));
    }
    let by = by_for(&who, b.by);
    let result = api
        .admin
        .create_as(
            NewAccount {
                name: name.as_str(),
                pw: &pw,
                authority,
                via: CreatedVia::Cli,
                ip: None,
                email: b.email.as_deref(),
            },
            &by,
        )
        .await;
    match result {
        Ok(id) => (
            StatusCode::CREATED,
            axum::Json(Created {
                id: id.0,
                name: name.as_str().to_owned(),
            }),
        )
            .into_response(),
        Err(e) => admin_error(&e),
    }
}

#[derive(Deserialize)]
struct PasswordBody {
    pwhash: String,
    by: Option<String>,
    /// An administrator's reset sets `must_change_password`; the default is true for a console session resetting
    /// another account, false for the CLI and the other tools.
    must_change: Option<bool>,
}

async fn set_password(
    State(api): State<AccountApi>,
    Who(who): Who,
    Path(name): Path<String>,
    body: Bytes,
) -> Response {
    let b: PasswordBody = match parse(&body) {
        Ok(b) => b,
        Err(r) => return *r,
    };
    let Some(pw) = PwHash::parse(&b.pwhash) else {
        return admin_error(&AdminError::InvalidPwHash);
    };
    let own = match rank_target(&api, &who, &name).await {
        Err(r) => return *r,
        Ok(None) => false,
        Ok(Some((rec, resolved))) => {
            let own = who.account.as_ref().is_some_and(|a| a.id == rec.id);
            // changing one's own password through the account route is allowed; resetting another account is not
            // allowed unless it ranks below
            if !own && let Err(e) = rank::check_target(&who, resolved) {
                return api_error(&e);
            }
            own
        }
    };
    let must_change = b.must_change.unwrap_or(who.session.is_some() && !own);
    let opts = PasswordOptions {
        must_change,
        keep_session: None,
    };
    match api
        .admin
        .set_password_with(&name, &pw, &by_for(&who, b.by), opts)
        .await
    {
        Ok(()) => axum::Json(json!({})).into_response(),
        Err(e) => admin_error(&e),
    }
}

#[derive(Deserialize)]
struct AuthorityBody {
    level: String,
    character: Option<String>,
    by: Option<String>,
}

async fn set_authority(
    State(api): State<AccountApi>,
    Who(who): Who,
    Path(name): Path<String>,
    body: Bytes,
) -> Response {
    let b: AuthorityBody = match parse(&body) {
        Ok(b) => b,
        Err(r) => return *r,
    };
    let Ok(level) = Authority::parse_token(&b.level) else {
        return admin_error(&AdminError::InvalidLevel);
    };
    // rank protection (25 §1.6): the target ranks below, the new letter is not above the actor's, never the own account
    match rank_target(&api, &who, &name).await {
        Err(r) => return *r,
        Ok(None) => {}
        Ok(Some((rec, resolved))) => {
            if let Err(e) = rank::check_set_authority(&who, rec.id, resolved, level) {
                return api_error(&e);
            }
        }
    }
    match api
        .admin
        .set_authority(&name, level, b.character.as_deref(), &by_for(&who, b.by))
        .await
    {
        Ok(overridden) => {
            axum::Json(json!({"overridden_by_gm_config": overridden})).into_response()
        }
        Err(e) => admin_error(&e),
    }
}

#[derive(Deserialize)]
struct BanBody {
    until: Option<i64>,
    reason: Option<String>,
    by: Option<String>,
}

async fn ban(
    State(api): State<AccountApi>,
    Who(who): Who,
    Path(name): Path<String>,
    body: Bytes,
) -> Response {
    let b: BanBody = match parse(&body) {
        Ok(b) => b,
        Err(r) => return *r,
    };
    match rank_target(&api, &who, &name).await {
        Err(r) => return *r,
        Ok(None) => {}
        Ok(Some((_, resolved))) => {
            if let Err(e) = rank::check_target(&who, resolved) {
                return api_error(&e);
            }
        }
    }
    match api
        .admin
        .ban(
            &name,
            b.until.map(UnixTime),
            b.reason.as_deref().unwrap_or(""),
            &by_for(&who, b.by),
        )
        .await
    {
        Ok(()) => axum::Json(json!({})).into_response(),
        Err(e) => admin_error(&e),
    }
}

#[derive(Deserialize, Default)]
struct UnbanBody {
    by: Option<String>,
}

async fn unban(
    State(api): State<AccountApi>,
    Who(who): Who,
    Path(name): Path<String>,
    body: Bytes,
) -> Response {
    let b: UnbanBody = match parse(&body) {
        Ok(b) => b,
        Err(r) => return *r,
    };
    match rank_target(&api, &who, &name).await {
        Err(r) => return *r,
        Ok(None) => {}
        Ok(Some((_, resolved))) => {
            if let Err(e) = rank::check_target(&who, resolved) {
                return api_error(&e);
            }
        }
    }
    match api.admin.unban(&name, &by_for(&who, b.by)).await {
        Ok(()) => axum::Json(json!({})).into_response(),
        Err(e) => admin_error(&e),
    }
}
