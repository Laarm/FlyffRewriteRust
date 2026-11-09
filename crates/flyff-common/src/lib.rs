//! `flyff-common` — tier S (Plan/02 §11.3): ids, time types, deterministic collections, RNG wrappers,
//! path normalisation and (feature `rt`) the cross-thread contract plumbing of Plan/01 §4.4.
//!
//! The crate compiles for `wasm32-unknown-unknown` **without** the `rt` feature. Everything that is not
//! wasm-safe (channels, atomics-based telemetry, clocks) lives under [`rt`].

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented
)]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]
#![deny(clippy::iter_over_hash_type)]
#![warn(missing_docs)]
#![recursion_limit = "512"]

pub mod config;
pub mod defs;
pub mod det;
pub mod gm;
pub mod ids;
pub mod kick;
pub mod log_budget;
pub mod math;
pub mod path;
pub mod rng;
pub mod time;

#[cfg(feature = "rt")]
pub mod rt;

pub use ids::{
    AccountId, ChannelId, CharacterId, GuildId, GuildWarId, ItemSerial, LayerId, MailId, ObjId,
    PartyId, SaveGen, SessionId, WorldId,
};
pub use path::{PathError, normalize_resource_path};
pub use time::{GameMs, SimFrame, Tick, UnixMs, UnixTime};
