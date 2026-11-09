//! `flyff-server` — tier N (Plan/10, 12, 24): the library behind the single static `flyff`
//! executable. `main` is a thin wrapper over [`cli::run_from_args`]; the in-process test harness
//! (`flyff-e2e`) uses [`runtime::start`] directly.
//!
//! Modules: [`cli`] (clap surface), [`config`] (`flyff.toml`), [`runtime`] (start-up, handles),
//! [`http`] (router, middleware, asset routes), [`assets`] (VFS, `AssetPrep`, manifests), [`gateway`] (WebSocket), [`shutdown`] (coordinator), [`console`] (stdin),
//! [`logging`], [`web_dist`] (embedded client), [`build_info`], [`exit`], [`data_load`] (VFS + game data at start-up),
//! [`metrics`] and [`admin`] (`/metrics` and its authorization), [`exposure`] (the S12 warning), [`wiring`] (the hooks the
//! lobby, world and CLI tasks install).

pub mod admin;
pub mod assets;
pub mod audit;
pub mod build_info;
pub mod cli;
pub mod config;
pub mod console;
pub mod data_load;
pub mod exit;
pub mod exposure;
pub mod gateway;
pub mod http;
pub mod lobby;
pub mod logging;
pub mod metrics;
pub mod runtime;
pub mod shutdown;
pub mod sys;
pub mod wall_store;
pub mod web_dist;
pub mod wiring;
