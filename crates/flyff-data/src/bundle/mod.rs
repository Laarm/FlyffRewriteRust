//! The client data bundles (Plan/14 §10): versioned JSON projections of `GameData` and `ClientData` that the
//! browser loads instead of the Flyff text files (D6).
//!
//! * [`types`]: the DTOs of every bundle (always compiled and wasm-safe, `flyff-wasm` decodes with them),
//!   the envelope `{bundle, schema, data}` and the manifest types; [`rowtable`]: the columnar `RowTable<T>`.
//! * feature `bundle`: [`export_all`] builds every bundle of this crate deterministically (the
//!   projections of [`project`], the registry [`catalog`](fn@catalog), the envelope, content hashes and gzip of
//!   [`envelope`], the manifest and `data_version` of [`manifest`]); [`store`] writes and caches them as
//!   `cache/data/<bundle>.<h16>.json(.gz)` + `manifest.json`.
//! * feature `schema`: `schema` derives the JSON Schemas and `tsgen` the TypeScript types from the DTOs.
//!
//! The 22 bundle names are [`BUNDLE_NAMES`]. 20 are exported here; `motions` and `model-bounds` are JSON
//! projections of binary derived assets written by `flyff-assets` (doc 24 §10.5-10.6) and inserted into the
//! manifest at its finalisation (`manifest::BundleManifest::with_external`).

pub mod rowtable;
pub mod types;

#[cfg(all(feature = "bundle", feature = "schema"))]
pub mod artifacts;
#[cfg(feature = "bundle")]
pub mod catalog;
#[cfg(feature = "bundle")]
pub mod envelope;
#[cfg(feature = "bundle")]
pub mod manifest;
#[cfg(feature = "bundle")]
pub mod project;
#[cfg(feature = "schema")]
pub mod schema;
#[cfg(feature = "bundle")]
pub mod store;
#[cfg(all(feature = "bundle", feature = "schema"))]
pub mod tsgen;

pub use rowtable::RowTable;
pub use types::{BUNDLE_FORMAT, BundleData, BundleManifest, Envelope, LoadClass, ManifestEntry};

#[cfg(feature = "bundle")]
pub use catalog::{BundleSpec, catalog};
#[cfg(feature = "bundle")]
pub use envelope::{Bundle, BundleError, BundleSet, ExportOptions, export_all};
#[cfg(feature = "bundle")]
pub use project::{PathResolver, Sources, WORLD_MAPS};

/// Bump whenever the export output can change for the same `GameData` (a projection rule, a field, a
/// transform): it invalidates the `data-bundles` cache (14 §10.8).
pub const BUNDLE_BUILDER_VERSION: u32 = 4;

/// The 22 bundle names in catalogue order (14 §10.4); the same list as `BUNDLES` of
/// `web/src/gen/bundles/bundles.ts`.
pub const BUNDLE_NAMES: [&str; 22] = [
    "items",
    "item-ext",
    "skills",
    "movers",
    "npcs",
    "texts",
    "help",
    "worlds",
    "models",
    "sfx-table",
    "motions",
    "model-bounds",
    "terrain",
    "resdata-layouts",
    "ui-atlases",
    "sounds",
    "music",
    "honor",
    "tables",
    "names",
    "quests",
    "defines",
];

/// The two bundles that `flyff-assets` produces (projections of the binary motion table and model bounds).
pub const EXTERNAL_BUNDLES: [&str; 2] = ["motions", "model-bounds"];
