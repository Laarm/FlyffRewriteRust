//! The HTTP layer of the single `flyff` listener (Plan/24 §1-§5, §9, §13, WP-WEB-01/03/04/05/06/17).
//!
//! * [`router`](fn@router) composes the route table of §2 with the routers other documents own ([`Mounts`]) and the middleware
//!   stack of §3.3 ([`stack`]); [`serve`](fn@serve) runs the listener (accept caps, timeouts, graceful shutdown).
//! * [`files::respond`] serves every byte source with conditional GET, Range and gzip negotiation.
//! * Routes: [`web`] (R1-R4, the embedded client), [`res`] (`/res`), [`data`] (`/data`), `r#gen` (`/gen`; `gen` is reserved in edition 2024),
//!   [`prepare`] (`/api/prepare`), [`health`] (`/healthz`).
//! * Shared rules: [`client_ip`](mod@client_ip) (the one client-IP rule), [`origin`] (the one Origin rule), [`cidr`],
//!   [`ratelimit`], [`headers`], [`mime`], [`error`], [`asset_auth`] (login-gated assets), [`telemetry`].
//! * State: [`HttpState`], [`HttpConfig`] (see `state`); the assets side lives in [`crate::assets`].

pub mod asset_auth;
pub mod cidr;
pub mod client_ip;
pub mod data;
pub mod error;
pub mod files;
#[path = "gen.rs"]
pub mod r#gen;
pub mod headers;
pub mod health;
pub mod live;
pub mod mime;
pub mod origin;
pub mod pathcodec;
pub mod prepare;
pub mod ratelimit;
pub mod res;
pub mod router;
pub mod serve;
pub mod stack;
pub mod state;
pub mod telemetry;

pub mod web;

pub use client_ip::{ClientInfo, client_ip};
pub use router::{Mounts, router};
pub use serve::{ServeOptions, TlsConnection, serve};
pub use state::{CoreHandles, HttpConfig, HttpState};
pub use telemetry::HttpTelemetry;
