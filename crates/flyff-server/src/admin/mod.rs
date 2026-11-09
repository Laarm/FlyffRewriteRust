//! Admin surfaces (Plan/10 §13, 23 §8, 25 §1, §11; WP-OPS-09, WP-ADMIN-01, WP-ADMIN-02).
//!
//! | module | content |
//! |---|---|
//! | [`audit_index`] | the admin audit viewer: per-day postings (`logs/audit/index/*.idx`), the query engine, the line scanner (WP-ADMIN-29) |
//! | [`auth`] | the authentication layer shared by `/metrics`, `/admin`, `/admin/ws` and `/admin/api/*`: the identity of a request ([`auth::Caller`]), the CSRF and `428` rules |
//! | [`identity`] | [`identity::Identity`] (staff, owner token, owner local), [`identity::CapTable`], [`identity::StaffResolver`] |
//! | [`session`] | [`session::AdminSessions`]: the in-memory table behind the `flyff_admin` cookie |
//! | [`alerts`], [`history`] | the dashboard: alert thresholds on a fake-able clock, the 24 h metrics history, the activity feed and the sampler (WP-ADMIN-25) |
//! | [`logs`] | the log ring buffer and its `tracing` layer (WP-ADMIN-26) |
//! | [`account_events`] | [`account_events::AccountEvents`]: bans, authority and password changes end the sessions of the account |
//! | [`caps`] | the generated [`caps::Cap`] enum (`caps.toml`, `cargo xtask admin-caps`) |
//! | [`dto`] | the DTO catalogue (`web/src/admin/gen/dto.ts`, `cargo xtask admin-dto`) |
//! | [`error`] | [`error::ApiError`]: the `{error: {code, message, details?}}` envelope |
//! | [`guard`], `guard_idempotency`, `guard_limits` | rank protection, reason, typed confirmation, group cap, the `Idempotency-Key` guard, the token bucket, the streaming permit |
//! | [`api`] | the route catalogue, the gate, the area mounts, the session routes |
//! | [`shared`] | [`shared::AdminShared`]: the state of the console that is not a request |
//! | [`metrics`] | the `flyff_admin_*` counters |
//! | [`totp`] | staff TOTP: HMAC/HOTP/TOTP, base32, the key URI, [`totp::TotpGuard`] (setups, replay rule, account lock; WP-ADMIN-28) |
//! | [`ws`], [`ws_map`] | the admin WebSocket gateway `GET /admin/ws`: subscriptions, latest-wins slots, bounded queues, backpressure, session end; the per-subscription map encoder |
//! | [`ops`] | [`AdminOps`]: the one object behind the console, the routes and the page (game commands, sessions, kick, shutdown) |
//! | [`gm_reload`] | the forwarder of the GM commands' server requests: `/loadscript` (the dialog scripts of `scripts/npc/`) and `/reload config` (WP-GM-25) |
//! | [`reload`] | `AdminOps::reload` / `reload_preview`: the file reload of 10 §8.6 (`ReloadBy`, `summary_line`; WP-OPS-16) |
//! | [`lines`] | the console grammar: parser and executor (runtime words, doc 23's console words, GM lines) |
//! | [`routes`] | `GET /admin` (the page) and the routes of the table of 10 §13.2 |
//! | [`json`] | JSON and text renderings of `AdminOutput` and its payloads |
//!
//! The stdin console thread itself is [`crate::console`]; the account routes are `lobby::admin_routes`.

pub mod account_events;
pub mod alerts;
pub mod api;
pub mod audit_index;
pub mod auth;
pub mod caps;
pub mod dto;
pub mod dto_guilds;
pub mod dto_items;
pub mod dto_players;
pub mod error;
pub mod gm_reload;
pub mod grants;
pub mod guard;
pub mod guard_idempotency;
pub mod guard_limits;
pub mod history;
pub mod identity;
pub mod ip_blocks;
pub mod json;
pub mod lines;
pub mod logs;
pub mod metrics;
pub mod ops;
pub mod players;
pub mod reload;
pub mod routes;
pub mod session;
pub mod shared;
pub mod totp;
pub mod ws;
pub mod ws_map;

pub use auth::{ADMIN_TOKEN_FILE, AdminAuth, ensure_admin_token};

/// `data/admin.addr`: the address the running server listens on (written at start; read by `flyff account`).
pub const ADMIN_ADDR_FILE: &str = "admin.addr";
pub use ops::{AdminOps, GatewaySessionInfo, OpsError, ReloadReport, ReloadSource, ShutdownReq};
