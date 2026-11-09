//! The route catalogue of the admin API (Plan/25 §11.3): every route that can exist under `/admin`, with the
//! capability that opens it and the work package that implements it.
//!
//! * The gate of [`super`] looks a request up here by `(method, matched path)`: an identity without the capability gets
//!   `403 forbidden`, a request to a route that is not in the catalogue gets `403` too (owners pass; the registration
//!   test fails).
//! * [`Routes`](super::Routes) refuses to mount a route that is not listed here (`admin_routes_registered`).
//! * A listed route whose work package has not landed is mounted as a stub that answers `501 needs_feature` with
//!   `details.wp` after the capability check (ADM-D10: panels show "Not available yet (needs WP-...)").
//! * `idem` marks the ✱ routes: they need an `Idempotency-Key` and are replayed from `admin_idempotency`.
//!
//! The `cap` of a route that serves several actions (`POST /admin/api/actions/{action}`, the content panels) is the
//! lowest capability any of them needs; the handler checks the finer one for its action.

use super::super::caps::Cap;

/// Who may call a route.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// No identity needed: the HTML shells and the login (the layer of `admin::auth` decides who gets so far).
    Public,
    /// Any identity (the session routes, the WebSocket).
    Authenticated,
    /// An identity that holds the capability.
    Cap(Cap),
}

/// One route of the catalogue (`CATALOG: &[(Method, &str, Cap, Owner WP)]` of 25 §11.3, plus the idempotency mark).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteSpec {
    /// `GET`, `POST`, `PUT`, `PATCH` or `DELETE`.
    pub method: &'static str,
    /// The axum path pattern (`/admin/api/guilds/{id}`).
    pub path: &'static str,
    /// Who may call it.
    pub access: Access,
    /// The work package that owns the route (`WP-ADMIN-20`, or `WP-OPS-09` / `WP-AUTH-19` for the older routes).
    pub wp: &'static str,
    /// The route needs an `Idempotency-Key` (✱).
    pub idem: bool,
}

impl RouteSpec {
    const fn new(
        method: &'static str,
        path: &'static str,
        access: Access,
        wp: &'static str,
    ) -> Self {
        RouteSpec {
            method,
            path,
            access,
            wp,
            idem: false,
        }
    }

    /// Marks the route ✱.
    const fn idem(mut self) -> Self {
        self.idem = true;
        self
    }

    /// `true` for the methods that change state.
    pub fn mutating(&self) -> bool {
        self.method != "GET"
    }

    /// The capability, when the route has one.
    pub fn cap(&self) -> Option<Cap> {
        match self.access {
            Access::Cap(c) => Some(c),
            _ => None,
        }
    }
}

const fn get(path: &'static str, cap: Cap, wp: &'static str) -> RouteSpec {
    RouteSpec::new("GET", path, Access::Cap(cap), wp)
}
const fn post(path: &'static str, cap: Cap, wp: &'static str) -> RouteSpec {
    RouteSpec::new("POST", path, Access::Cap(cap), wp)
}
const fn put(path: &'static str, cap: Cap, wp: &'static str) -> RouteSpec {
    RouteSpec::new("PUT", path, Access::Cap(cap), wp)
}
const fn patch(path: &'static str, cap: Cap, wp: &'static str) -> RouteSpec {
    RouteSpec::new("PATCH", path, Access::Cap(cap), wp)
}
const fn delete(path: &'static str, cap: Cap, wp: &'static str) -> RouteSpec {
    RouteSpec::new("DELETE", path, Access::Cap(cap), wp)
}
const fn open(method: &'static str, path: &'static str, wp: &'static str) -> RouteSpec {
    RouteSpec::new(method, path, Access::Public, wp)
}
const fn authed(method: &'static str, path: &'static str, wp: &'static str) -> RouteSpec {
    RouteSpec::new(method, path, Access::Authenticated, wp)
}

use Cap::{
    ActAccounts, ActBan, ActBroadcast, ActIpBlock, ActItems, ActKick, ActMessagePlayer, ActMute,
    ActServer, ActUnstuck, ActWorld, ViewAuditOwn, ViewDashboard, ViewInventory, ViewMap, ViewPii,
    ViewPlayers, ViewStaff,
};

/// Every route of the admin console (25 §11.3), in the order of the plan's table, then the routes of 10 §13.2,
/// 12 §5.5 and 23 §8.3 that the console reuses.
pub static CATALOG: &[RouteSpec] = &[
    // ---- the shells and the session (WP-ADMIN-01, -03) ----
    open("GET", "/admin", "WP-ADMIN-03"),
    open("GET", "/admin/lite", "WP-ADMIN-03"),
    open("POST", "/admin/api/session", "WP-ADMIN-01"),
    authed("GET", "/admin/api/session", "WP-ADMIN-01"),
    authed("DELETE", "/admin/api/session", "WP-ADMIN-01"),
    authed("PUT", "/admin/api/session/password", "WP-ADMIN-01"),
    authed("POST", "/admin/api/session/totp", "WP-ADMIN-28"),
    authed("DELETE", "/admin/api/session/totp", "WP-ADMIN-28"),
    authed("GET", "/admin/ws", "WP-ADMIN-05"),
    // ---- search, world, locations, objects, actions (WP-ADMIN-17, -08, -10, -14, -12) ----
    get("/admin/api/search", ViewPlayers, "WP-ADMIN-17"),
    get("/admin/api/worlds", ViewMap, "WP-ADMIN-08"),
    get("/admin/api/world/{id}/meta", ViewMap, "WP-ADMIN-08"),
    get("/admin/api/world/find", ViewMap, "WP-ADMIN-10"),
    get("/admin/api/locations", ViewMap, "WP-ADMIN-14"),
    post("/admin/api/locations", ActWorld, "WP-ADMIN-14"),
    put("/admin/api/locations/{id}", ActWorld, "WP-ADMIN-14"),
    delete("/admin/api/locations/{id}", ActWorld, "WP-ADMIN-14"),
    get("/admin/api/objects/{obj}", ViewMap, "WP-ADMIN-12"),
    get("/admin/api/npc-scripts/{key}", ViewMap, "WP-ADMIN-13"),
    post(
        "/admin/api/actions/{action}",
        ActMessagePlayer,
        "WP-ADMIN-12",
    )
    .idem(),
    // ---- characters (WP-ADMIN-16, -17) ----
    get("/admin/api/characters", ViewPlayers, "WP-ADMIN-16"),
    get("/admin/api/characters/{id}", ViewPlayers, "WP-ADMIN-16"),
    get(
        "/admin/api/characters/{id}/items",
        ViewInventory,
        "WP-ADMIN-16",
    ),
    get(
        "/admin/api/characters/{id}/mail",
        ViewInventory,
        "WP-ADMIN-16",
    ),
    post(
        "/admin/api/characters/{id}/items/remove",
        ActItems,
        "WP-ADMIN-16",
    )
    .idem(),
    post("/admin/api/characters/{id}/gold", ActItems, "WP-ADMIN-16").idem(),
    post(
        "/admin/api/characters/{id}/position",
        ActUnstuck,
        "WP-ADMIN-16",
    )
    .idem(),
    post(
        "/admin/api/characters/{id}/rename",
        ActAccounts,
        "WP-ADMIN-16",
    )
    .idem(),
    post(
        "/admin/api/characters/{id}/undelete",
        ActAccounts,
        "WP-ADMIN-16",
    )
    .idem(),
    post("/admin/api/characters/{id}/pet", ActItems, "WP-ADMIN-16").idem(),
    put(
        "/admin/api/characters/{id}/login-block",
        ActBan,
        "WP-ADMIN-17",
    ),
    post("/admin/api/characters/{id}/ban", ActBan, "WP-ADMIN-17"),
    // ---- accounts, notes, moderation, IP blocks, staff (WP-ADMIN-17) ----
    get("/admin/api/accounts/{name}", ViewPlayers, "WP-ADMIN-17"),
    get("/admin/api/accounts/{name}/alts", ViewPii, "WP-ADMIN-17"),
    get("/admin/api/notes", ViewPlayers, "WP-ADMIN-17"),
    post("/admin/api/notes", ActMute, "WP-ADMIN-17"),
    delete("/admin/api/notes/{id}", ActMute, "WP-ADMIN-17"),
    get("/admin/api/moderation/bans", ViewPlayers, "WP-ADMIN-17"),
    get("/admin/api/moderation/mutes", ViewPlayers, "WP-ADMIN-17"),
    get("/admin/api/moderation/blocks", ViewPlayers, "WP-ADMIN-17"),
    get("/admin/api/moderation/alts", ViewPii, "WP-ADMIN-17"),
    get("/admin/api/ip-blocks", ViewPii, "WP-ADMIN-17"),
    post("/admin/api/ip-blocks", ActIpBlock, "WP-ADMIN-17"),
    delete("/admin/api/ip-blocks/{id}", ActIpBlock, "WP-ADMIN-17"),
    get("/admin/api/staff", ViewStaff, "WP-ADMIN-17"),
    get("/admin/api/admin-sessions", ViewStaff, "WP-ADMIN-17"),
    delete("/admin/api/admin-sessions/{id}", ActAccounts, "WP-ADMIN-17"),
    // ---- items and economy (WP-ADMIN-20, -22) ----
    get("/admin/api/items/catalog-meta", ActItems, "WP-ADMIN-20"),
    post("/admin/api/items/validate", ActItems, "WP-ADMIN-20"),
    post("/admin/api/grants/v2", ActItems, "WP-ADMIN-20").idem(),
    get("/admin/api/grants", ActItems, "WP-ADMIN-20"),
    post("/admin/api/grants", ActItems, "WP-OPS-09"),
    post("/admin/api/grants/{id}/cancel", ActItems, "WP-ADMIN-20"),
    get("/admin/api/removals", ActItems, "WP-ADMIN-20"),
    post("/admin/api/removals/{id}/cancel", ActItems, "WP-ADMIN-20"),
    get("/admin/api/templates", ActItems, "WP-ADMIN-20"),
    post("/admin/api/templates", ActItems, "WP-ADMIN-20"),
    put("/admin/api/templates/{id}", ActItems, "WP-ADMIN-20"),
    delete("/admin/api/templates/{id}", ActItems, "WP-ADMIN-20"),
    post("/admin/api/items/search", ViewInventory, "WP-ADMIN-20"),
    post("/admin/api/items/duplicates", ViewInventory, "WP-ADMIN-20"),
    get("/admin/api/economy", ViewInventory, "WP-ADMIN-22"),
    // ---- guilds (WP-ADMIN-23) ----
    get("/admin/api/guilds", ViewPlayers, "WP-ADMIN-23"),
    get(
        "/admin/api/guilds/validate-name",
        ViewPlayers,
        "WP-ADMIN-23",
    ),
    get("/admin/api/guilds/meta", ViewPlayers, "WP-ADMIN-23"),
    get("/admin/api/guilds/{id}", ViewPlayers, "WP-ADMIN-23"),
    get("/admin/api/guilds/{id}/log", ViewInventory, "WP-ADMIN-23"),
    get("/admin/api/guilds/{id}/wars", ViewPlayers, "WP-ADMIN-23"),
    post("/admin/api/guilds", Cap::ActGuild, "WP-ADMIN-23").idem(),
    patch("/admin/api/guilds/{id}", Cap::ActGuild, "WP-ADMIN-23").idem(),
    post(
        "/admin/api/guilds/{id}/members",
        Cap::ActGuild,
        "WP-ADMIN-23",
    ),
    delete(
        "/admin/api/guilds/{id}/members/{character}",
        Cap::ActGuild,
        "WP-ADMIN-23",
    ),
    delete("/admin/api/guilds/{id}", Cap::ActGuild, "WP-ADMIN-23"),
    // ---- world and content (WP-ADMIN-30, -31) ----
    get("/admin/api/spawns", ViewMap, "WP-ADMIN-30"),
    post(
        "/admin/api/spawns/{region}/respawn",
        ActWorld,
        "WP-ADMIN-30",
    ),
    get("/admin/api/census", ViewMap, "WP-ADMIN-30"),
    get("/admin/api/events/modifiers", ViewDashboard, "WP-ADMIN-30"),
    put("/admin/api/weather/{world}", ActWorld, "WP-ADMIN-30"),
    get("/admin/api/content/{panel}", ViewMap, "WP-ADMIN-31"),
    post(
        "/admin/api/content/{panel}/{action}",
        ActWorld,
        "WP-ADMIN-31",
    ),
    // ---- communication (WP-ADMIN-27) ----
    get("/admin/api/notices", ActBroadcast, "WP-ADMIN-27"),
    post("/admin/api/notices", ActBroadcast, "WP-ADMIN-27"),
    put("/admin/api/notices/{id}", ActBroadcast, "WP-ADMIN-27"),
    delete("/admin/api/notices/{id}", ActBroadcast, "WP-ADMIN-27"),
    post("/admin/api/broadcast", ActBroadcast, "WP-ADMIN-27"),
    post("/admin/api/mail", ActItems, "WP-ADMIN-27").idem(),
    get("/admin/api/mail", ActItems, "WP-ADMIN-27"),
    // ---- dashboard (WP-ADMIN-25) ----
    get("/admin/api/history", ViewDashboard, "WP-ADMIN-25"),
    get("/admin/api/alerts", ViewDashboard, "WP-ADMIN-25"),
    get("/admin/api/activity", ViewDashboard, "WP-ADMIN-25"),
    // ---- server (WP-ADMIN-26) ----
    get("/admin/api/config", ActServer, "WP-ADMIN-26"),
    put("/admin/api/config/{key}", ActServer, "WP-ADMIN-26"),
    delete("/admin/api/config/{key}", ActServer, "WP-ADMIN-26"),
    get("/admin/api/jobs", ActServer, "WP-ADMIN-26"),
    post("/admin/api/jobs/{name}/run", ActServer, "WP-ADMIN-26"),
    get("/admin/api/backups", ActServer, "WP-ADMIN-26"),
    post("/admin/api/backups", ActServer, "WP-ADMIN-26"),
    post("/admin/api/backups/{name}/verify", ActServer, "WP-ADMIN-26"),
    get(
        "/admin/api/backups/{name}/download",
        ActServer,
        "WP-ADMIN-26",
    ),
    get("/admin/api/maintenance", ActServer, "WP-ADMIN-26"),
    put("/admin/api/maintenance", ActServer, "WP-AUTH-18"),
    // ---- audit (WP-ADMIN-29) ----
    get("/admin/api/audit", ViewAuditOwn, "WP-ADMIN-29"),
    get("/admin/api/audit/export", ViewAuditOwn, "WP-ADMIN-29"),
    // ---- the routes of 10 §13.2, 12 §5.5 and 23 §8.3 that the console reuses (WP-OPS-09, WP-AUTH-19) ----
    get("/admin/api/status", ViewDashboard, "WP-OPS-09"),
    post("/admin/api/console", ActServer, "WP-OPS-09"),
    get("/admin/api/sessions", ViewPlayers, "WP-OPS-09"),
    get("/admin/api/netstats", ActServer, "WP-OPS-18"),
    post("/admin/api/reload", ActServer, "WP-OPS-09"),
    get("/admin/api/reload", ActServer, "WP-OPS-16"),
    post("/admin/api/shutdown", ActServer, "WP-OPS-09"),
    post("/admin/api/kick", ActKick, "WP-OPS-09"),
    post("/admin/api/notice", ActBroadcast, "WP-OPS-09"),
    get("/admin/api/rates", ViewDashboard, "WP-OPS-09"),
    put("/admin/api/rates/{key}", ActWorld, "WP-OPS-09"),
    get("/admin/api/events", ViewDashboard, "WP-OPS-09"),
    put("/admin/api/events/{id}", ActWorld, "WP-OPS-09"),
    get("/admin/api/accounts", ViewPlayers, "WP-AUTH-19"),
    post("/admin/api/accounts", ActAccounts, "WP-AUTH-19"),
    put(
        "/admin/api/accounts/{name}/password",
        ActAccounts,
        "WP-AUTH-19",
    ),
    put(
        "/admin/api/accounts/{name}/authority",
        ActAccounts,
        "WP-AUTH-19",
    ),
    post("/admin/api/accounts/{name}/ban", ActBan, "WP-AUTH-19"),
    delete("/admin/api/accounts/{name}/ban", ActBan, "WP-AUTH-19"),
    delete(
        "/admin/api/accounts/{name}/totp",
        ActAccounts,
        "WP-ADMIN-28",
    ),
];

/// The catalogue entry of `(method, path pattern)`.
pub fn find(method: &str, path: &str) -> Option<&'static RouteSpec> {
    CATALOG
        .iter()
        .find(|r| r.method == method && r.path == path)
}

/// `true` when some method of `path` is in the catalogue.
pub fn has_path(path: &str) -> bool {
    CATALOG.iter().any(|r| r.path == path)
}
