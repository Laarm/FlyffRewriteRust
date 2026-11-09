//! The admin API skeleton (Plan/25 §11.3; WP-ADMIN-02): the route catalogue, the one gate every `/admin/*` route passes,
//! and one mount function per area file.
//!
//! * [`CATALOG`] lists every route that can exist (method, path, capability, owning work package, ✱ idempotency).
//! * [`Routes`] is how a route is mounted: it refuses a route that is not in the catalogue
//!   ([`RouteReport::rejected`]; the `admin_routes_registered` test fails on it).
//! * [`build_router`] merges the older routes (10 §13.2, 12 §5.5), the area mounts and, for every catalogue entry nobody
//!   mounted, a stub that answers `501 needs_feature` with the work package that lands it. The **gate** wraps the whole
//!   router as a `route_layer`:
//!   1. looks up `(method, matched path)` in the catalogue (an unlisted route is refused for staff);
//!   2. checks the capability (`403 forbidden`, `details.cap`);
//!   3. takes a token of the identity's mutation bucket (`admin.mutations_per_s`, `429 rate_limited`);
//!   4. for ✱ routes runs the `Idempotency-Key` flow (`409 in_flight`, `422 idempotency_mismatch`, the stored result on
//!      a replay), around the handler.
//!
//!   The `428` rule, the CSRF header, the Origin rules and the identity itself belong to the layer of
//!   [`super::auth`], which runs first.
//!
//! **Adding a route** (every later work package): list it in `catalog.rs` if it is not there, then mount it in your
//! area file's `mount` with `r.get("/admin/api/...", handler)` / `.post` / `.put` / `.patch` / `.delete`. The handler
//! takes `State<ApiState>` and the [`Who`](super::auth::Who) extractor, checks the finer capability, rank, reason and
//! confirmation with [`super::guard`], and returns `Result<_, ApiError>`.

pub mod actions;
pub mod audit;
pub mod catalog;
pub mod characters;
pub mod comm;
pub mod content;
pub mod guilds;
pub mod items;
pub mod locations;
pub mod npc_script;
pub mod players;
pub mod server;
pub mod session;
pub mod totp;
pub mod world;

pub use catalog::{Access, CATALOG, RouteSpec};

use super::auth::Caller;
use super::error::{ApiError, MaybeApplied};
use super::guard_idempotency::{IdemBegin, Idempotency, MAX_BODY, key_of};
use super::ops::AdminOps;
use super::shared::AdminShared;
use axum::Router;
use axum::body::Body;
use axum::extract::{MatchedPath, Request, State};
use axum::handler::Handler;
use axum::http::Method;
use axum::middleware::{Next, from_fn_with_state};
use axum::response::{IntoResponse, Response};
use axum::routing::{self, MethodRouter};
use std::collections::BTreeSet;
use std::sync::Arc;

/// The state of the handlers of the new admin routes: the ops object (game thread, lobby, persistence, audit) and the
/// admin console's shared state.
#[derive(Clone)]
pub struct ApiState {
    /// The one object behind the console, the routes and the page.
    pub ops: Arc<AdminOps>,
    /// Sessions, capabilities, policy, limits, idempotency, counters.
    pub admin: Arc<AdminShared>,
}

impl ApiState {
    /// The state over `ops` (its `admin` field).
    pub fn new(ops: Arc<AdminOps>) -> ApiState {
        ApiState {
            admin: Arc::clone(&ops.admin),
            ops,
        }
    }

    /// The live guard policy.
    pub fn policy(&self) -> Arc<super::guard::Policy> {
        self.admin.policy.get()
    }
}

impl std::fmt::Debug for ApiState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ApiState")
    }
}

/// What mounting found: the routes that were mounted and the ones refused for not being in the catalogue.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RouteReport {
    /// `(method, path)` of every mounted route.
    pub mounted: BTreeSet<(String, String)>,
    /// `"METHOD path"` of every route that is not in [`CATALOG`] (it was not mounted).
    pub rejected: Vec<String>,
}

/// A router under construction that only accepts catalogued routes.
pub struct Routes<S> {
    router: Router<S>,
    mounted: Vec<(&'static str, &'static str)>,
    rejected: Vec<String>,
}

impl<S> Default for Routes<S>
where
    S: Clone + Send + Sync + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<S> Routes<S>
where
    S: Clone + Send + Sync + 'static,
{
    /// No route yet.
    pub fn new() -> Routes<S> {
        Routes {
            router: Router::new(),
            mounted: Vec::new(),
            rejected: Vec::new(),
        }
    }

    fn add(mut self, method: &'static str, path: &'static str, mr: MethodRouter<S>) -> Self {
        if catalog::find(method, path).is_none() {
            tracing::error!(
                "admin route {method} {path} is not in admin::api::CATALOG: not mounted"
            );
            self.rejected.push(format!("{method} {path}"));
            return self;
        }
        if self.mounted.contains(&(method, path)) {
            tracing::error!(
                "admin route {method} {path} is mounted twice: the second one is ignored"
            );
            self.rejected.push(format!("{method} {path} (twice)"));
            return self;
        }
        self.mounted.push((method, path));
        self.router = self.router.route(path, mr);
        self
    }

    /// Mounts `GET path`.
    #[must_use]
    pub fn get<H, T>(self, path: &'static str, h: H) -> Self
    where
        H: Handler<T, S>,
        T: 'static,
    {
        self.add("GET", path, routing::get(h))
    }

    /// Mounts `POST path`.
    #[must_use]
    pub fn post<H, T>(self, path: &'static str, h: H) -> Self
    where
        H: Handler<T, S>,
        T: 'static,
    {
        self.add("POST", path, routing::post(h))
    }

    /// Mounts `PUT path`.
    #[must_use]
    pub fn put<H, T>(self, path: &'static str, h: H) -> Self
    where
        H: Handler<T, S>,
        T: 'static,
    {
        self.add("PUT", path, routing::put(h))
    }

    /// Mounts `PATCH path`.
    #[must_use]
    pub fn patch<H, T>(self, path: &'static str, h: H) -> Self
    where
        H: Handler<T, S>,
        T: 'static,
    {
        self.add("PATCH", path, routing::patch(h))
    }

    /// Mounts `DELETE path`.
    #[must_use]
    pub fn delete<H, T>(self, path: &'static str, h: H) -> Self
    where
        H: Handler<T, S>,
        T: 'static,
    {
        self.add("DELETE", path, routing::delete(h))
    }

    /// Applies the state and records what was mounted into `report`.
    pub fn finish(self, state: S, report: &mut RouteReport) -> Router {
        for (m, p) in &self.mounted {
            report.mounted.insert(((*m).to_owned(), (*p).to_owned()));
        }
        report.rejected.extend(self.rejected);
        self.router.with_state(state)
    }
}

/// The stub of a catalogued route nobody mounted: `501 needs_feature` with the work package.
fn stub(spec: &RouteSpec) -> MethodRouter<()> {
    let wp = spec.wp;
    let h = move || async move { ApiError::needs_feature(wp).into_response() };
    match spec.method {
        "GET" => routing::get(h),
        "POST" => routing::post(h),
        "PUT" => routing::put(h),
        "PATCH" => routing::patch(h),
        _ => routing::delete(h),
    }
}

/// The complete admin router (without the authentication layer): the older routes, the area mounts, the stubs, and the
/// gate. `report.rejected` must be empty.
pub fn build_router(api: &ApiState) -> (Router, RouteReport) {
    let mut report = RouteReport::default();
    let mut router = super::routes::mount(&api.ops, &mut report);
    router = router.merge(crate::lobby::admin_routes::mount(
        api.ops.accounts.clone(),
        &api.admin,
        &mut report,
    ));
    let mut r = Routes::<ApiState>::new();
    r = session::mount(r);
    r = totp::mount(r);
    r = world::mount(r);
    r = locations::mount(r);
    r = npc_script::mount(r);
    r = actions::mount(r);
    r = characters::mount(r);
    r = players::mount(r);
    r = items::mount(r);
    r = guilds::mount(r);
    r = comm::mount(r);
    r = server::mount(r);
    r = audit::mount(r);
    r = content::mount(r);
    r = super::ws::mount(r);
    router = router.merge(r.finish(api.clone(), &mut report));
    finish(router, report, &api.admin)
}

/// The last step of [`build_router`]: a stub for every catalogued route nobody mounted (`501 needs_feature`, after the
/// capability check) and the gate around everything. Also what the tests assemble their routers with.
pub fn finish(
    mut router: Router,
    mut report: RouteReport,
    shared: &Arc<AdminShared>,
) -> (Router, RouteReport) {
    let mut stubs = Router::new();
    let mut stubbed = BTreeSet::new();
    for spec in CATALOG {
        if report
            .mounted
            .contains(&(spec.method.to_owned(), spec.path.to_owned()))
        {
            continue;
        }
        stubbed.insert((spec.method.to_owned(), spec.path.to_owned()));
        stubs = stubs.route(spec.path, stub(spec));
    }
    report.mounted.extend(stubbed);
    router = router.merge(stubs);
    let router = router.route_layer(from_fn_with_state(Arc::clone(shared), gate));
    (router, report)
}

/// Marks a request that passed an idempotency-guarded route: the key the handler may log.
#[derive(Clone, Debug)]
pub struct IdemKey(pub String);

/// The gate (see the module documentation).
pub async fn gate(State(sh): State<Arc<AdminShared>>, req: Request, next: Next) -> Response {
    let method = req.method().clone();
    // `/admin/` is the shell `/admin` (mounted next to it by `routes::mount`, not a catalogue entry of its own)
    let matched = req
        .extensions()
        .get::<MatchedPath>()
        .map(|m| match m.as_str() {
            "/admin/" => "/admin".to_owned(),
            other => other.to_owned(),
        });
    let identity = req
        .extensions()
        .get::<Caller>()
        .and_then(|c| c.identity.clone());
    // axum answers HEAD with the GET handler: the gate checks it as the GET it is
    let lookup = if method == Method::HEAD {
        "GET"
    } else {
        method.as_str()
    };
    let spec = matched.as_deref().and_then(|p| catalog::find(lookup, p));
    let Some(spec) = spec else {
        // A path that is in the catalogue under other methods only: the router answers 405 itself.
        if matched.as_deref().is_some_and(catalog::has_path) {
            return next.run(req).await;
        }
        // Not in the catalogue at all: the owners pass (the registration test fails), staff do not.
        return match identity {
            Some(i) if i.is_owner() => next.run(req).await,
            Some(_) => {
                ApiError::forbidden_text("this route is not in the catalogue").into_response()
            }
            None => ApiError::unauthorized().into_response(),
        };
    };
    match spec.access {
        Access::Public => {}
        Access::Authenticated => {
            if identity.is_none() {
                return ApiError::unauthorized().into_response();
            }
        }
        Access::Cap(cap) => {
            let Some(i) = &identity else {
                return ApiError::unauthorized().into_response();
            };
            if !i.has(&sh.caps, cap) {
                return ApiError::forbidden(cap).into_response();
            }
        }
    }
    if spec.mutating()
        && let Some(i) = &identity
    {
        let rate = sh.policy.get().mutations_per_s;
        if let Err(wait_ms) = sh.buckets.try_take(&i.limit_key(), rate, sh.clock.now_ms()) {
            let secs = u32::try_from(wait_ms.div_ceil(1000)).unwrap_or(1).max(1);
            return ApiError::rate_limited(secs, "too many changes at once").into_response();
        }
    }
    if spec.idem {
        let Some(i) = identity else {
            return ApiError::unauthorized().into_response();
        };
        return idempotent(&sh, req, next, &i.label()).await;
    }
    next.run(req).await
}

/// The ✱ flow around a handler.
async fn idempotent(sh: &AdminShared, req: Request, next: Next, actor: &str) -> Response {
    let key = match key_of(req.headers()) {
        Ok(k) => k,
        Err(e) => return e.into_response(),
    };
    let (parts, body) = req.into_parts();
    let bytes = match axum::body::to_bytes(body, MAX_BODY).await {
        Ok(b) => b,
        Err(_) => {
            return ApiError::new(
                super::dto::ErrorCode::Limit,
                "the request body is too large",
            )
            .into_response();
        }
    };
    let route = format!("{} {}", parts.method, parts.uri.path());
    let hash = Idempotency::body_hash(&bytes);
    let slot = match sh.idem.begin(&key, actor, &route, hash).await {
        Err(e) => return e.into_response(),
        Ok(IdemBegin::Replay(resp)) => return resp,
        Ok(IdemBegin::InFlight) => {
            return ApiError::new(
                super::dto::ErrorCode::InFlight,
                "a request with this Idempotency-Key is still running",
            )
            .into_response();
        }
        Ok(IdemBegin::Mismatch) => {
            return ApiError::new(
                super::dto::ErrorCode::IdempotencyMismatch,
                "this Idempotency-Key belongs to another request",
            )
            .into_response();
        }
        Ok(IdemBegin::Started(slot)) => slot,
    };
    let mut req = Request::from_parts(parts, Body::from(bytes));
    req.extensions_mut().insert(IdemKey(key));
    let resp = next.run(req).await;
    let (rparts, rbody) = resp.into_parts();
    let rbytes = axum::body::to_bytes(rbody, 4 * MAX_BODY)
        .await
        .unwrap_or_default();
    if rparts.status.is_success() {
        slot.done(rparts.status.as_u16(), &rbytes).await;
    } else if rparts.extensions.get::<MaybeApplied>().is_none() {
        // refused before it changed anything: the key may be retried
        slot.failed().await;
    }
    // else: the outcome is unknown (a timeout waiting for the game thread): the key stays in flight, never a double run
    Response::from_parts(rparts, Body::from(rbytes))
}
