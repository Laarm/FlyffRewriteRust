//! Persistence mapping of game state (Plan/13 §17, 16 §3.6): `flyff_game::persist`.
//!
//! WP-MOVER-03 provides the snapshot helpers that turn a player [`Mover`](crate::mover::Mover) into the
//! snapshot-owned columns and documents of `flyff-db` ([`snapshot`]); WP-DB-07's `snapshot::build` calls them
//! and adds what the session knows (generation, reason, play time, banks, `last_logout_at`).
//!
//! WP-DB-07 (task world-flow): [`save`] (generations, the staggered 90 s periodic, urgent and world-change
//! snapshots), `snapshot::build`, [`load`] (`apply_character_load`) and [`grants`] (the poll plumbing).
//!
//! WP-DB-16 (task db-economy): [`maintenance`] (the daily retention trigger, the `maintenance` wall job).
//!
//! WP-DB-06 (task lobby-chars): [`new_character`] (the template and the builder of 13 §24), [`serial`] (the
//! `SerialAllocator`) and [`container_port`] (the minimal `Add`/`DoEquip` port the kit is built with until doc 19's
//! inventory exists).

pub mod container_port;
pub mod grants;
pub mod load;
pub mod maintenance;
pub mod new_character;
pub mod save;
pub mod serial;
pub mod snapshot;

pub use load::{LoadHook, LoadOptions, LoadReport, apply_character_load};
pub use save::{SaveGenCounter, SaveScheduler, run_pass, save_player};
pub use snapshot::{Location, SnapshotCtx, build};
