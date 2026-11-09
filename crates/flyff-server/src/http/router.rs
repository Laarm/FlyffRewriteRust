//! Router composition (Plan/24 §2, §16): the route table of this document, the routers of the other owners
//! mounted next to it, and the middleware stack of §3.3 around all of them.

use super::state::HttpState;
use super::{data, error, r#gen, health, prepare, res, stack, web};
use axum::Router;
use axum::middleware::from_fn_with_state;
use axum::response::Response;
use axum::routing::get;

/// Routers owned by other documents, mounted into the one route table. Each is a complete `Router<()>` (its
/// state is already applied) and must not add `Access-Control-*` headers, must use `http::client_ip` and
/// `http::origin::is_allowed`, and must answer JSON errors under `/api/*` (Plan/24 §16).
#[derive(Default)]
pub struct Mounts {
    /// Doc 12: `/api/bootstrap`, `/api/login`, `/api/register`.
    pub lobby: Option<Router>,
    /// Doc 10: `/ws` (`gateway::router`).
    pub gateway: Option<Router>,
    /// Doc 10: `/metrics`.
    pub metrics: Option<Router>,
    /// Docs 10/23: `/admin`, `/admin/api/*`.
    pub admin: Option<Router>,
}

async fn not_found() -> Response {
    error::not_found()
}

/// The routes of this document without the middleware stack (R1-R4, R8, R10-R14, `/healthz`).
pub fn routes(state: &HttpState) -> Router {
    Router::new()
        .route("/", get(web::index))
        .route("/index.html", get(web::index))
        .route("/assets/{*path}", get(web::asset))
        .route("/dev/{*path}", get(web::dev))
        .route("/healthz", get(health::healthz))
        .route("/api/prepare", get(prepare::get))
        .route("/data/{*path}", get(data::get))
        .route("/res/{*path}", get(res::get))
        .route("/gen/{*path}", get(r#gen::get))
        .route("/{file}", get(web::root_file))
        .with_state(state.clone())
}

/// The complete router: this document's routes, the `mounts`, RT1 (`404 not found`) as the fallback, and the
/// stack of §3.3 (layers 0-7) around everything.
pub fn router(state: HttpState, mounts: Mounts) -> Router {
    let mut app = routes(&state);
    for r in [mounts.lobby, mounts.gateway, mounts.metrics, mounts.admin]
        .into_iter()
        .flatten()
    {
        app = app.merge(r);
    }
    app.fallback(not_found)
        .layer(from_fn_with_state(state.clone(), stack::body_limit))
        .layer(from_fn_with_state(state.clone(), stack::timeout))
        .layer(from_fn_with_state(state.clone(), stack::origin_check))
        .layer(from_fn_with_state(state.clone(), stack::rate_limit))
        .layer(from_fn_with_state(state.clone(), stack::catch_panic))
        .layer(from_fn_with_state(state.clone(), stack::ip_block))
        .layer(from_fn_with_state(state, stack::edge))
}
