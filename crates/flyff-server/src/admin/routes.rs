//! The admin routes (Plan/10 §13.2 table; WP-OPS-09): `GET /admin` (the page) and every `/admin/api/*` route,
//! behind `admin::auth` (loopback without forwarding headers and a local `Host`, or `admin.allow_remote` with the
//! bearer token; JSON-only mutations; no-store).
//!
//! | route | here |
//! |---|---|
//! | `GET /admin` | the embedded page ([`PAGE`]) |
//! | `GET /admin/api/status` | `StatusSnapshot` + build + the `/healthz` fields + the sessions per state |
//! | `POST /admin/api/console` | `{line, as?}` -> `{lines, result}` (the grammar of `admin::lines`) |
//! | `GET /admin/api/sessions?limit=` | `{gateway: [GatewaySessionInfo], in_world: [OnlineEntry]}` |
//! | `GET /admin/api/netstats` | 501 until WP-OPS-18 |
//! | `POST /admin/api/reload` | `{changed, overridden, restart_required, errors, warnings}` (`AdminOps::reload`, WP-OPS-16) |
//! | `GET /admin/api/reload` | the same report for what a reload would do now, nothing applied, no audit (`reload_preview`) |
//! | `POST /admin/api/shutdown` | `{countdown_s?, message?, cancel?}` -> `{countdown_s}` / `{}` |
//! | `POST /admin/api/kick` | `{character?, account?, reason?}` -> `{affected}` |
//! | `POST /admin/api/notice` | `{text}` -> `{}` |
//! | `GET /admin/api/rates`, `PUT /admin/api/rates/{key}` | `[RateEntry]`, `{value: number or null}` -> `RateEntry` |
//! | `GET /admin/api/events`, `PUT /admin/api/events/{id}` | `[EventEntry]`, `{state: 0..=255}` -> `EventEntry` |
//! | `POST /admin/api/grants` | the alias of grants v2 (`api::items::grants_alias`, WP-ADMIN-20) |
//! | `/admin/api/accounts...` | the six account routes (`lobby::admin_routes`, 12 §5.5) |
//!
//! The withdrawn names (`/admin/api/command`, `/admin/api/online`, `DELETE /admin/api/shutdown`, ...) do not
//! exist: they answer 404 / 405.

use super::api::{RouteReport, Routes};
use super::auth::Who;
use super::caps::Cap;
use super::error::ApiError;
use super::guard::rank;
use super::json;
use super::lines::{self, NoTerminal};
use super::ops::{AdminOps, OpsError, ShutdownReq};
use super::reload::ReloadBy;
use crate::http::files;
use crate::http::headers::{LITE_CSP, admin_csp_value, csp_host_ok};
use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use flyff_common::gm::Authority;
use flyff_game::input::{AdminActor, AdminCmd, AdminOutput, AdminTarget};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

/// The rescue page `/admin/lite` (one static file, no build step, no external resources, under 50 KB; 25 §9.10, 23 §8.4).
/// `%FLYFF_MD5_SALT%` is replaced by the server's `auth.legacy_md5_salt` when it is served (the login form hashes with it).
pub const PAGE: &str = include_str!("page.html");

/// The placeholder of [`PAGE`] for the salt of the legacy password hash.
const SALT_PLACEHOLDER: &str = "%FLYFF_MD5_SALT%";

/// The routes of 10 §13.2 over `ops`, mounted through [`Routes`] (so each one is in the catalogue of
/// `admin::api::CATALOG`, which also gives it its capability) and the six account routes (`lobby::admin_routes`,
/// mounted by `admin::api::build_router`). The authentication layer and the gate wrap the result.
pub fn mount(ops: &Arc<AdminOps>, report: &mut RouteReport) -> Router {
    Routes::<Arc<AdminOps>>::new()
        .get("/admin", spa)
        .get("/admin/lite", lite)
        .get("/admin/api/status", status)
        .post("/admin/api/console", console)
        .get("/admin/api/sessions", sessions)
        .get("/admin/api/netstats", not_implemented_netstats)
        .post("/admin/api/reload", reload)
        .get("/admin/api/reload", reload_preview)
        .post("/admin/api/shutdown", shutdown)
        .post("/admin/api/kick", kick)
        .post("/admin/api/notice", notice)
        .get("/admin/api/rates", rates)
        .put("/admin/api/rates/{key}", set_rate)
        .get("/admin/api/events", events)
        .put("/admin/api/events/{id}", set_event)
        .post("/admin/api/grants", super::api::items::grants_alias)
        .finish(Arc::clone(ops), report)
        // `/admin/` is the same shell as `/admin` (the gate looks it up as `/admin`, see `api::gate`)
        .route(
            "/admin/",
            axum::routing::get(spa).with_state(Arc::clone(ops)),
        )
}

// ---- helpers ---------------------------------------------------------------------------------------------------

fn json_response(status: StatusCode, body: &Value) -> Response {
    (status, axum::Json(body)).into_response()
}

fn error(status: StatusCode, token: &str, detail: &str) -> Response {
    json_response(status, &json!({"error": token, "detail": detail}))
}

fn ops_error(e: OpsError) -> Response {
    let status = match e {
        OpsError::Timeout => StatusCode::GATEWAY_TIMEOUT,
        OpsError::GameGone => StatusCode::SERVICE_UNAVAILABLE,
    };
    error(status, "game_unavailable", &e.to_string())
}

fn parse<T: for<'de> Deserialize<'de>>(body: &Bytes) -> Result<T, Box<Response>> {
    let text: &[u8] = if body.iter().all(u8::is_ascii_whitespace) {
        b"{}"
    } else {
        body
    };
    serde_json::from_slice(text).map_err(|e| {
        Box::new(error(
            StatusCode::BAD_REQUEST,
            "bad_request",
            &format!("malformed JSON body: {e}"),
        ))
    })
}

/// `{"error": text}` -> the status of a refused game command (404 for "not found").
fn refused(text: &str) -> Response {
    let status = if text.contains("not found") || text.contains("not online") {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::BAD_REQUEST
    };
    error(status, "refused", text)
}

// ---- handlers --------------------------------------------------------------------------------------------------

/// `GET /admin` and `GET /admin/`: the console (Plan/25 §11.2). The page is `admin.html` of the web dist; the CSP is the
/// golden of the plan with the validated `Host` (the WebSocket `/admin/ws` is `connect-src`). A dist without the page
/// (the placeholder, a client built before the console) serves the rescue page instead, so `/admin` never is a 404.
async fn spa(State(ops): State<Arc<AdminOps>>, req: Request) -> Response {
    let (parts, _) = req.into_parts();
    let accept = flyff_assets::build::gz::accepts_gzip(files::accept_encoding(&parts.headers));
    let Some(page) = ops.http.site.admin_servable(accept) else {
        return lite_response(&ops);
    };
    let mut resp = files::respond(&parts, page).await;
    if ops.http.cfg.csp {
        set_csp(&mut resp, &admin_csp_value(host_of(&parts.headers)));
    }
    resp
}

/// `GET /admin/lite`: the rescue page.
async fn lite(State(ops): State<Arc<AdminOps>>) -> Response {
    lite_response(&ops)
}

/// The `Host` header when it is safe to splice into a CSP source list.
fn host_of(headers: &axum::http::HeaderMap) -> Option<&str> {
    headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .filter(|h| csp_host_ok(h))
}

fn set_csp(resp: &mut Response, csp: &str) {
    if let Ok(v) = HeaderValue::from_str(csp) {
        resp.headers_mut()
            .insert(header::CONTENT_SECURITY_POLICY, v);
    }
}

/// [`PAGE`] with the salt of this server filled in.
fn lite_response(ops: &AdminOps) -> Response {
    let mut r = Html(lite_html(ops.accounts.md5_salt())).into_response();
    if ops.http.cfg.csp {
        set_csp(&mut r, LITE_CSP);
    }
    r
}

/// The text of the rescue page for a server whose legacy password salt is `salt`.
pub fn lite_html(salt: &str) -> String {
    PAGE.replace(SALT_PLACEHOLDER, &html_attr_escape(salt))
}

fn html_attr_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

async fn status(State(ops): State<Arc<AdminOps>>) -> Response {
    match ops.status().await {
        Ok(v) => json_response(StatusCode::OK, &v),
        Err(e) => ops_error(e),
    }
}

#[derive(Deserialize)]
struct ConsoleBody {
    line: String,
    #[serde(rename = "as")]
    as_character: Option<String>,
}

async fn console(State(ops): State<Arc<AdminOps>>, Who(who): Who, body: Bytes) -> Response {
    let b: ConsoleBody = match parse(&body) {
        Ok(b) => b,
        Err(r) => return *r,
    };
    if b.line.len() > 2048 {
        return error(
            StatusCode::BAD_REQUEST,
            "bad_request",
            "the line is longer than 2048 bytes",
        );
    }
    let actor = who.actor();
    // 25 §10.1: the `requested` record is acknowledged before the line is dispatched
    if !b.line.trim().is_empty()
        && let Err(e) = ops.audit_requested(actor, b.line.trim())
    {
        return e.into_response();
    }
    let reply = lines::run_line(
        &ops,
        &b.line,
        b.as_character.as_deref(),
        actor,
        &mut NoTerminal,
    )
    .await;
    json_response(
        StatusCode::OK,
        &json!({"lines": reply.lines, "result": if reply.ok { "ok" } else { "error" }}),
    )
}

#[derive(Deserialize)]
struct LimitQuery {
    limit: Option<u32>,
}

async fn sessions(
    State(ops): State<Arc<AdminOps>>,
    Who(who): Who,
    Query(q): Query<LimitQuery>,
) -> Response {
    let limit = q.limit.unwrap_or(100).clamp(1, 10_000);
    let actor = who.actor();
    let mut in_world = match ops.online(Some(limit), actor).await {
        Ok(rows) => rows.iter().map(json::online_json).collect::<Vec<_>>(),
        Err(e) => return ops_error(e),
    };
    let mut gateway: Vec<Value> = ops
        .gateway_sessions(usize::try_from(limit).unwrap_or(100))
        .iter()
        .map(|g| serde_json::to_value(g).unwrap_or(Value::Null))
        .collect();
    // addresses are PII (25 §12 rule 12): only an identity with `view.pii` sees them
    if !who.has(&ops.admin.caps, Cap::ViewPii) {
        for row in &mut in_world {
            if let Some(o) = row.as_object_mut() {
                o.remove("ip");
            }
        }
        for row in &mut gateway {
            if let Some(o) = row.as_object_mut() {
                o.remove("peer");
            }
        }
    }
    json_response(
        StatusCode::OK,
        &json!({"gateway": gateway, "in_world": in_world}),
    )
}

async fn not_implemented_netstats() -> Response {
    error(
        StatusCode::NOT_IMPLEMENTED,
        "not_implemented",
        "per-message network statistics arrive with WP-OPS-18",
    )
}

async fn reload(State(ops): State<Arc<AdminOps>>, Who(who): Who) -> Response {
    if let Err(e) = ops.audit_requested(who.actor(), "reload") {
        return e.into_response();
    }
    let r = ops.reload(ReloadBy::from(who.actor())).await;
    json_response(StatusCode::OK, &json!(r))
}

/// `GET /admin/api/reload`: the "Compare with flyff.toml" report of the configuration page; changes nothing.
async fn reload_preview(State(ops): State<Arc<AdminOps>>) -> Response {
    json_response(StatusCode::OK, &json!(ops.reload_preview().await))
}

#[derive(Deserialize, Default)]
struct ShutdownBody {
    countdown_s: Option<u32>,
    message: Option<String>,
    #[serde(default)]
    cancel: bool,
    /// Exit with `exit::RESTART` so that the supervisor starts the server again (only with a supervisor, 25 §9.7).
    #[serde(default)]
    restart: bool,
}

async fn shutdown(State(ops): State<Arc<AdminOps>>, Who(who): Who, body: Bytes) -> Response {
    let b: ShutdownBody = match parse(&body) {
        Ok(b) => b,
        Err(r) => return *r,
    };
    if b.countdown_s.is_some_and(|n| n > 3600) {
        return error(
            StatusCode::BAD_REQUEST,
            "bad_request",
            "countdown_s is 0..=3600",
        );
    }
    let actor = who.actor();
    let req = ShutdownReq {
        countdown_s: b.countdown_s,
        message: b.message.filter(|m| !m.trim().is_empty()),
        cancel: b.cancel,
    };
    if b.restart && !req.cancel && ops.admin.supervisor.is_none() {
        return error(
            StatusCode::BAD_REQUEST,
            "no_supervisor",
            "restart is offered only when a process supervisor started the server (systemd)",
        );
    }
    let what = match (req.cancel, b.restart) {
        (true, _) => "shutdown cancel",
        (false, true) => "shutdown restart",
        (false, false) => "shutdown",
    };
    // 25 §10.1: the audit record is acknowledged before the countdown starts
    if let Err(e) = ops.audit_requested(actor, what) {
        return e.into_response();
    }
    let restart_flag = &ops.admin.restart_requested;
    match ops.shutdown(&req, actor) {
        Ok(Some(n)) => {
            restart_flag.store(b.restart, std::sync::atomic::Ordering::Relaxed);
            ops.audit_command(actor, what, "ok");
            json_response(
                StatusCode::OK,
                &json!({"countdown_s": n, "restart": b.restart}),
            )
        }
        Ok(None) => {
            restart_flag.store(false, std::sync::atomic::Ordering::Relaxed);
            ops.audit_command(actor, what, "ok");
            json_response(StatusCode::OK, &json!({}))
        }
        Err(e) => error(StatusCode::CONFLICT, "no_countdown", &e),
    }
}

/// `Some(())` when an online character called `name` ranks at or above a staff `who` (25 §1.6; owners never are).
async fn outranked_online(
    ops: &AdminOps,
    who: &super::identity::Identity,
    name: &str,
) -> Option<()> {
    if who.is_owner() {
        return None;
    }
    let rows = ops.online(None, who.actor()).await.ok()?;
    let e = rows.iter().find(|e| e.name.eq_ignore_ascii_case(name))?;
    let letter = Authority::from_u8(u8::try_from(e.authority).ok()?);
    rank::check_target(who, letter).err().map(|_| ())
}

#[derive(Deserialize)]
struct KickBody {
    character: Option<String>,
    account: Option<String>,
    reason: Option<String>,
}

async fn kick(State(ops): State<Arc<AdminOps>>, Who(who): Who, body: Bytes) -> Response {
    let b: KickBody = match parse(&body) {
        Ok(b) => b,
        Err(r) => return *r,
    };
    let actor = who.actor();
    let line = format!(
        "kick {}",
        b.account
            .as_deref()
            .or(b.character.as_deref())
            .unwrap_or("")
    );
    if let Err(e) = ops.audit_requested(actor, &line) {
        return e.into_response();
    }
    let result = match (b.account.as_deref(), b.character.as_deref()) {
        (Some(name), None) => {
            let acct = crate::lobby::accounts::AccountName::parse(name);
            match acct {
                Err(_) => Err(error(
                    StatusCode::BAD_REQUEST,
                    "invalid_name",
                    "not an account name",
                )),
                Ok(acct) => {
                    let n = acct.as_str().to_owned();
                    match ops
                        .persist
                        .read(move |c| flyff_db::queries::find_account(c, &n))
                        .await
                    {
                        // rank protection (25 §1.6): staff cannot kick an account that does not rank below them
                        Ok(Some(rec)) => match rank::check_target(
                            &who,
                            ops.admin.resolver.target_authority(
                                Authority::from_db_byte(rec.authority),
                                None,
                                &rec.name,
                                None,
                            ),
                        ) {
                            Err(e) => Err(e.into_response()),
                            Ok(()) => ops.kick_account(rec.id).await.map_err(ops_error),
                        },
                        Ok(None) => {
                            Err(error(StatusCode::NOT_FOUND, "not_found", "no such account"))
                        }
                        Err(e) => Err(error(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "db",
                            &e.to_string(),
                        )),
                    }
                }
            }
        }
        (None, Some(name)) if outranked_online(&ops, &who, name).await.is_some() => {
            Err(ApiError::target_outranks().into_response())
        }
        (None, Some(name)) => match ops
            .game(
                AdminCmd::Kick {
                    target: AdminTarget::Character(name.to_owned()),
                    reason: b.reason.clone(),
                    banned: false,
                },
                actor,
            )
            .await
        {
            Ok(AdminOutput::Done { affected }) => Ok(affected),
            Ok(AdminOutput::Error(e)) => Err(refused(&e)),
            Ok(_) => Err(error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "unexpected answer",
            )),
            Err(e) => Err(ops_error(e)),
        },
        _ => Err(error(
            StatusCode::BAD_REQUEST,
            "bad_request",
            "give exactly one of character and account",
        )),
    };
    match result {
        Ok(affected) => {
            ops.audit_command(actor, &line, "ok");
            json_response(StatusCode::OK, &json!({"affected": affected}))
        }
        Err(r) => {
            ops.audit_command(actor, &line, "error");
            r
        }
    }
}

#[derive(Deserialize)]
struct NoticeBody {
    text: String,
}

async fn notice(State(ops): State<Arc<AdminOps>>, Who(who): Who, body: Bytes) -> Response {
    let b: NoticeBody = match parse(&body) {
        Ok(b) => b,
        Err(r) => return *r,
    };
    let actor = who.actor();
    if let Err(e) = ops.audit_requested(actor, "notice") {
        return e.into_response();
    }
    match ops.notice(&b.text, actor).await {
        Ok(AdminOutput::Done { affected }) => {
            ops.audit_command(actor, "notice", "ok");
            json_response(StatusCode::OK, &json!({"affected": affected}))
        }
        Ok(AdminOutput::Error(e)) => refused(&e),
        Ok(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "unexpected answer",
        ),
        Err(e) => ops_error(e),
    }
}

async fn rates(State(ops): State<Arc<AdminOps>>) -> Response {
    match ops.game(AdminCmd::GetRates, AdminActor::Console).await {
        Ok(AdminOutput::Rates(rows)) => json_response(
            StatusCode::OK,
            &Value::Array(rows.iter().map(json::rate_json).collect()),
        ),
        Ok(AdminOutput::Error(e)) => refused(&e),
        Ok(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "unexpected answer",
        ),
        Err(e) => ops_error(e),
    }
}

#[derive(Deserialize)]
struct RateBody {
    /// A number, or `null` / absent to go back to the default.
    value: Option<f64>,
}

async fn set_rate(
    State(ops): State<Arc<AdminOps>>,
    Who(who): Who,
    Path(key): Path<String>,
    body: Bytes,
) -> Response {
    let b: RateBody = match parse(&body) {
        Ok(b) => b,
        Err(r) => return *r,
    };
    let actor = who.actor();
    if let Err(e) = ops.audit_requested(actor, &format!("rates {key}")) {
        return e.into_response();
    }
    match ops
        .game(
            AdminCmd::SetRate {
                key: key.clone(),
                value: b.value,
            },
            actor,
        )
        .await
    {
        Ok(AdminOutput::Rates(rows)) => {
            ops.audit_command(actor, &format!("rates {key}"), "ok");
            match rows.first() {
                Some(r) => json_response(StatusCode::OK, &json::rate_json(r)),
                None => json_response(StatusCode::OK, &json!({})),
            }
        }
        Ok(AdminOutput::Error(e)) => {
            ops.audit_command(actor, &format!("rates {key}"), "error");
            error(StatusCode::BAD_REQUEST, "refused", &e)
        }
        Ok(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "unexpected answer",
        ),
        Err(e) => ops_error(e),
    }
}

async fn events(State(ops): State<Arc<AdminOps>>) -> Response {
    match ops.game(AdminCmd::GetEvents, AdminActor::Console).await {
        Ok(AdminOutput::Events(rows)) => json_response(
            StatusCode::OK,
            &Value::Array(rows.iter().map(json::event_json).collect()),
        ),
        Ok(AdminOutput::Error(e)) => refused(&e),
        Ok(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "unexpected answer",
        ),
        Err(e) => ops_error(e),
    }
}

#[derive(Deserialize)]
struct EventBody {
    state: u8,
}

async fn set_event(
    State(ops): State<Arc<AdminOps>>,
    Who(who): Who,
    Path(id): Path<u16>,
    body: Bytes,
) -> Response {
    let b: EventBody = match parse(&body) {
        Ok(b) => b,
        Err(r) => return *r,
    };
    let actor = who.actor();
    if let Err(e) = ops.audit_requested(actor, &format!("events {id} {}", b.state)) {
        return e.into_response();
    }
    match ops
        .game(AdminCmd::SetEvent { id, state: b.state }, actor)
        .await
    {
        Ok(AdminOutput::Events(rows)) => {
            ops.audit_command(actor, &format!("events {id} {}", b.state), "ok");
            match rows.first() {
                Some(e) => json_response(StatusCode::OK, &json::event_json(e)),
                None => json_response(StatusCode::OK, &json!({})),
            }
        }
        Ok(AdminOutput::Error(e)) => {
            ops.audit_command(actor, &format!("events {id} {}", b.state), "error");
            error(StatusCode::BAD_REQUEST, "refused", &e)
        }
        Ok(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "unexpected answer",
        ),
        Err(e) => ops_error(e),
    }
}
