//! `flyff-data` — tier S for the types, tier G for feature `load` (Plan/14): the immutable `GameData`
//! model and the typed loaders that parse the original `Resource/` text data. Default features give the
//! wasm-safe type library used by `flyff-rules` and `flyff-wasm`; the loaders (the original load order,
//! fix-ups F1-F13, diagnostics) live behind `load` (native only); `mlua` appears only under `load` (rule X6,
//! Plan/01 §5.3).
//!
//! * [`types`]: id aliases, [`NULL_ID`] and [`IdTable`] (the `CFixedArray` replacement),
//! * [`options`]: [`DataOptions`] (the config values that change loading),
//! * [`game_data`]: [`GameData`], [`DataMeta`], [`DataCounts`],
//! * [`texts`]: `textClient.inc` and the `GETTEXT` semantics,
//! * [`props`]: `ItemProp` (items, skills, party skills), the item index, `AddSkillProp`, `MoverProp`,
//!   `CtrlProp`, `MotionProp`, `JobProp`,
//! * [`mover_ex`]: the `propMoverEx.inc` block types (the loader is WP-DATA-07),
//! * [`exp_table`], [`job_item`], [`etc`], [`rules`], [`social`]: the experience, job-item, etc and rules
//!   tables,
//! * [`items_ext`]: `except.txt` (more item tables come with WP-DATA-11),
//! * [`character`] and [`vendor`]: the NPC definitions (`character*.inc`) and the vendor stock generation
//!   (WP-DATA-08),
//! * [`world_list`], [`worlds`] and [`models`]: `World.inc`, the per-folder text data of every world, the
//!   revival tables, and the model registry with its file-name builders (WP-DATA-09),
//! * [`names`]: the character-name rules (`InvalidName_ENG.inc`, `Letter_ENG.inc`),
//! * [`client`] and [`client_data`]: the client-only data (window layouts, sound and music tables, terrain
//!   textures, atlases, the help family and panels, emoticons, word filter, world-map records; WP-DATA-16),
//! * [`check`]: the define-drift check and the header renderer,
//! * `load` (feature `load`): `load_all`, `LoadCtx`, the `Masquerade.prj` driver, the step glue and the
//!   `data-load.jsonl` writer.
//!
//! Diagnostics are `flyff-script`'s and re-exported here: every loader reports problems instead of
//! panicking, and only the conditions of Plan/14 §8.2 are Fatal.

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

pub mod bundle;
pub mod character;
pub mod check;
pub mod client;
pub mod client_data;
pub mod dialog_text;
pub mod etc;
pub mod events;
pub mod exp_table;
pub mod game_data;
pub mod guide;
pub mod items_ext;
pub mod job_item;
#[cfg(feature = "load")]
pub mod load;
pub mod lua_config;
pub mod models;
pub mod mover_ex;
pub mod names;
pub mod options;
pub mod props;
pub mod quest;

pub mod rules;
pub mod social;
pub mod teleport16;
pub mod texts;
pub mod types;
pub mod vendor;
pub mod world_list;
pub mod worlds;

pub use client_data::ClientData;
pub use flyff_script::{DiagCode, DiagSummary, Diagnostic, Diagnostics, Severity};
pub use game_data::{DataCounts, DataMeta, GAMEDATA_LOADER_VERSION, GameData, SourceInfo};
pub use options::DataOptions;
pub use texts::{TextEntry, TextTable};
pub use types::{IdTable, NULL_ID};

#[cfg(feature = "load")]
pub use load::{LoadCtx, LoadError, LoadOutput, load_all};

/// Crate marker used by the workspace smoke test and by `cargo xtask doctor`.
pub const CRATE_NAME: &str = "flyff-data";
