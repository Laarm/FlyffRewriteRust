//! The routes of the lobby (Plan/12 §6): `GET /api/bootstrap`, `POST /api/login`, `POST /api/register`.
//!
//! Mounted as `Mounts.lobby` of the HTTP layer, so the middleware stack of 24 §3.3 already applied the
//! per-IP `api` rate class, the Origin check of state-changing requests, a 16 KiB body limit and the
//! 15 s timeout. This module adds the rules of 12 §6.1: JSON only (`415`), request bodies of at most 4 KiB
//! (`413`), `Cache-Control: no-store`, and the error body `{code, tid, retry_after_ms?, detail?}`.

use super::LobbyCore;
use super::accounts::AccountAdmin;
use super::auth::PwHash;
use super::bootstrap;
use super::errors::LobbyErr;
use super::login::LoginRequest;
use super::register::RegisterRequest;
use crate::http::{ClientInfo, HttpState};
use axum::Router;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

/// The largest request body of the lobby API (12 §6.1).
pub const MAX_BODY_BYTES: usize = 4096;

/// The state of the lobby routes.
#[derive(Clone)]
pub struct LobbyHttp {
    /// The lobby.
    pub core: Arc<LobbyCore>,
    /// The HTTP layer (asset side of the bootstrap, the asset cookie).
    pub http: HttpState,
    /// The account service of `/api/register`.
    pub admin: AccountAdmin,
}

/// The router to mount as `Mounts.lobby` (12 §6): `/api/bootstrap`, `/api/login`, `/api/register`.
/// `/api/prepare` belongs to the HTTP layer and is not registered here.
pub fn router(core: Arc<LobbyCore>, http: HttpState) -> Router {
    let admin = core.account_admin();
    Router::new()
        .route("/api/bootstrap", get(get_bootstrap))
        .route("/api/login", post(post_login))
        .route("/api/register", post(post_register))
        .with_state(LobbyHttp { core, http, admin })
}

fn json_response<T: Serialize>(status: StatusCode, body: &T) -> Response {
    let bytes = serde_json::to_vec(body).unwrap_or_else(|_| b"{}".to_vec());
    let mut resp = Response::new(Body::from(bytes));
    *resp.status_mut() = status;
    let h = resp.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
}

fn client_ip_of(req: &Request) -> (IpAddr, bool) {
    req.extensions()
        .get::<ClientInfo>()
        .map_or((IpAddr::V4(Ipv4Addr::UNSPECIFIED), false), |c| {
            (c.ip, c.https)
        })
}

/// Checks the content type, reads at most [`MAX_BODY_BYTES`] and parses the JSON object.
async fn read_json<T: for<'de> Deserialize<'de>>(req: Request) -> Result<T, LobbyErr> {
    let (parts, body) = req.into_parts();
    let ct = parts
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| {
            v.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase()
        });
    if ct.as_deref() != Some("application/json") {
        return Err(LobbyErr::unsupported_media_type());
    }
    let declared = parts
        .headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<usize>().ok());
    if declared.is_some_and(|n| n > MAX_BODY_BYTES) {
        return Err(LobbyErr::too_large());
    }
    let bytes = axum::body::to_bytes(body, MAX_BODY_BYTES)
        .await
        .map_err(|_| LobbyErr::too_large())?;
    serde_json::from_slice(&bytes).map_err(|_| LobbyErr::bad_request("malformed JSON body"))
}

async fn get_bootstrap(State(st): State<LobbyHttp>) -> Response {
    json_response(StatusCode::OK, &bootstrap::build(&st.core, &st.http))
}

#[derive(Deserialize)]
struct LoginBody {
    account: String,
    pwhash: String,
    #[serde(default)]
    force: bool,
}

#[derive(Serialize)]
struct LoginResponse {
    token: String,
    account: String,
    authority: u8,
    token_grace_s: u32,
    server_time_ms: i64,
}

async fn post_login(State(st): State<LobbyHttp>, req: Request) -> Response {
    let (ip, https) = client_ip_of(&req);
    let body: LoginBody = match read_json(req).await {
        Ok(b) => b,
        Err(e) => return e.into_response(),
    };
    let Some(pwhash) = PwHash::parse(&body.pwhash) else {
        return LobbyErr::bad_request("pwhash must be 32 lowercase hex characters").into_response();
    };
    let ok = match st
        .core
        .login(LoginRequest {
            account: body.account,
            pwhash,
            force: body.force,
            ip,
        })
        .await
    {
        Ok(ok) => ok,
        Err(e) => return e.into_response(),
    };
    let mut resp = json_response(
        StatusCode::OK,
        &LoginResponse {
            token: ok.token.to_string(),
            account: ok.account.as_str().to_owned(),
            authority: ok.authority.as_u8(),
            token_grace_s: ok.token_grace_s,
            server_time_ms: ok.server_time_ms,
        },
    );
    if !st.http.cfg.public_assets {
        // the login-gated asset cookie (24 §13, WP-AUTH-20)
        resp.headers_mut().append(
            header::SET_COOKIE,
            st.http.tokens.issue(ok.account_id, https),
        );
    }
    resp
}

#[derive(Deserialize)]
struct RegisterBody {
    account: String,
    pwhash: String,
    #[serde(default)]
    email: Option<String>,
}

#[derive(Serialize)]
struct RegisterResponse {
    account: String,
}

async fn post_register(State(st): State<LobbyHttp>, req: Request) -> Response {
    let (ip, _) = client_ip_of(&req);
    let body: RegisterBody = match read_json(req).await {
        Ok(b) => b,
        Err(e) => return e.into_response(),
    };
    let Some(pwhash) = PwHash::parse(&body.pwhash) else {
        return LobbyErr::bad_request("pwhash must be 32 lowercase hex characters").into_response();
    };
    match st
        .core
        .register(
            &st.admin,
            RegisterRequest {
                account: body.account,
                pwhash,
                email: body.email,
                ip,
            },
        )
        .await
    {
        Ok(account) => json_response(StatusCode::CREATED, &RegisterResponse { account }),
        Err(e) => e.into_response(),
    }
}
