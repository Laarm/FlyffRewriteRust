//! `flyff-assets` — tier R (Plan/24, D6): the derived-asset framework, the `cache/` directory and the
//! bounded, niced asset-preparation pool. **Never** the unbounded tokio blocking pool (D2), and no
//! tokio at all in this crate (01 X1): callers wait on [`flyff_common::rt::ReplyRx`] values, which work
//! under any executor.
//!
//! # Map
//!
//! | module | contents (Plan/24) |
//! |---|---|
//! | [`kinds`] | [`AssetKind`], [`AssetBuilder`] and its job/file types, [`Priority`], prewarm steps (§7.1, §7.2, §7.5) |
//! | [`fp`] | [`Fp`] and the fingerprint formulas (§6.1-§6.3) |
//! | [`cache`] | the `cache/` directory: atomic writes, W8 storage rule, lookup, sweep, budget (§8) |
//! | [`check`] | [`check::binary_corpus`]: the threaded binary-format corpus runner of `flyff check` (30 §17.1, WP-FMT-12) |
//! | [`pool`] | [`AssetPool`]: niced workers, three priority queues, promotion, start-up workers (§7.4) |
//! | [`prep`] | [`AssetPrep`]: `get`, in-flight de-duplication, prewarm, prepare, start-up loading (§7.3) |
//! | [`progress`] | [`PrepareProgress`] waves and `progress_version` (§12) |
//! | [`report`] | `prepare-report.json`, [`CacheReport`] (§8.9, §11.4) |
//! | [`telemetry`] | [`AssetTelemetry`] counters (§8.10) |
//! | [`res`] | the client-visible directory whitelist (§5.2) |
//! | [`build::gz`] | gzip variants of raw files (§10.1, WP-WEB-10) |
//! | [`manifest`], [`derived`] | `ClientManifest` skeleton, `ManifestParts`, [`ServerDerived`] |
//! | `testkit` (feature) | `EchoBuilder`, `ResourceTree`, clocks |
//!
//! Read `NOTES.md` for the builder registration contract the other asset WPs code against.

// `sys.rs` holds the one `unsafe` block (the `ioprio_set` syscall); everything else is safe.
#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod build;
pub mod cache;
pub mod check;
pub mod clock;
pub mod derived;
pub mod error;
pub mod fp;
pub mod kinds;
pub mod manifest;
mod manifest_ops;
pub mod pool;
pub mod prep;
pub mod progress;
pub mod report;
pub mod res;
mod sys;
pub mod telemetry;

mod util;

pub use cache::{
    Cache, CachedFile, Durability, GzLookup, Staged, StoredEncoding, SweepKeep, SweepStats,
};
pub use clock::Clock;
pub use derived::ServerDerived;
pub use error::{AssetError, AssetErrorKind};
pub use fp::{FingerprintMode, Fp, FpCtx};
pub use kinds::{
    AssetBuilder, AssetFeatures, AssetKind, BuildCtx, BuiltFile, Compress, JobKey, KeyRoute,
    Layout, NO_PARTITION, PrewarmStep, Priority,
};
pub use manifest::{BundleIndex, ClientManifest, ManifestParts};
pub use pool::{AssetPool, PoolError, PoolHandle};
pub use prep::{AssetConfig, AssetPrep, GzState, GzipRawMode, JobSummary};
pub use progress::{Phase, PrepareProgress};
pub use report::{CacheReport, KindCacheReport, KindReport, PrepareOptions, PrepareReport};
pub use telemetry::{AssetTelemetry, JobResult};
