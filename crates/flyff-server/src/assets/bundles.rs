//! The `data-bundles` asset kind (Plan/24 §7.1, §11.1 step 5a; Plan/14 §10.3; the adapter that completes WP-DATA-18).
//!
//! [`DataBundleBuilder`] registers with `AssetPrep` (`AssetKind::DataBundle`, layout `data/`) and exports the 20
//! client bundles of `flyff_data::bundle` into `cache/data/` **synchronously at start-up**, from its `on_start`
//! hook, before `/data` is served:
//!
//! * the cache fingerprint is `flyff_data::bundle::store::fingerprint(GameData.meta.fingerprint,
//!   vfs_tree_digest(vfs))` (it embeds `BUNDLE_BUILDER_VERSION`); a hit loads nothing (`BundleStore::ensure`);
//! * a miss runs `export_all` and writes the files, `manifest.json` and the stamp atomically, sweeping the stale
//!   files of the 20 own bundles;
//! * the two projections (`motions`, `model-bounds`) come from their own builders and are added at the manifest
//!   finalisation ([`super::AssetsState::new`]).
//!
//! The bundles are not artifacts of `AssetPrep::get`: the builder has no jobs and no `/gen` keys (`/data` is served
//! by `http::data` from `cache/data/`). A failure is a fatal start-up error naming the cause (the bundles are
//! required, §7.6). The builder needs the loaded `ClientData`, which `BuildCtx` does not carry, so the server
//! passes it to [`DataBundleBuilder::new`]; register the builder only once the real `GameData` and `ClientData`
//! exist.

use flyff_assets::Fp;
use flyff_assets::cache::Cache;
use flyff_assets::{
    AssetBuilder, AssetError, AssetKind, BuildCtx, BuiltFile, FpCtx, JobKey, KeyRoute, NO_PARTITION,
};
use flyff_data::ClientData;
use flyff_data::bundle::store::{BundleStore, StoreOutcome, fingerprint, vfs_tree_digest};
use flyff_data::bundle::{BUNDLE_BUILDER_VERSION, ExportOptions, export_all};
use parking_lot::Mutex;
use std::sync::Arc;

/// What the last `on_start` did (for the start-up report and tests).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BundleOutcome {
    /// The cached files were current.
    Hit,
    /// The bundles were exported and written.
    Built,
}

/// The `data-bundles` builder.
pub struct DataBundleBuilder {
    client: Arc<ClientData>,
    options: ExportOptions,
    last: Mutex<Option<(BundleOutcome, u64)>>,
}

impl DataBundleBuilder {
    /// A builder over the loaded client data.
    pub fn new(client: Arc<ClientData>) -> DataBundleBuilder {
        DataBundleBuilder {
            client,
            options: ExportOptions::default(),
            last: Mutex::new(None),
        }
    }

    /// What the start-up hook did and how long it took (ms); `None` before it ran.
    pub fn last_outcome(&self) -> Option<(BundleOutcome, u64)> {
        *self.last.lock()
    }
}

impl AssetBuilder for DataBundleBuilder {
    fn kind(&self) -> AssetKind {
        AssetKind::DataBundle
    }

    fn builder_version(&self) -> u32 {
        BUNDLE_BUILDER_VERSION
    }

    fn route(&self, _ctx: &FpCtx<'_>, key: &str) -> Result<KeyRoute, AssetError> {
        Err(AssetError::not_found(format!(
            "data bundles are served under /data, not through the builder keys ({key:?})"
        )))
    }

    fn partition_fp(&self, ctx: &FpCtx<'_>, partition: &str) -> Option<Fp> {
        (partition == NO_PARTITION).then(|| {
            let [lo, hi] = ctx.data_fp_parts();
            ctx.kind_fp(
                AssetKind::DataBundle,
                BUNDLE_BUILDER_VERSION,
                0,
                &[lo, hi, Fp(vfs_tree_digest(ctx.vfs))],
            )
        })
    }

    fn jobs(&self, _ctx: &FpCtx<'_>) -> Vec<JobKey> {
        Vec::new()
    }

    fn build(&self, _ctx: &BuildCtx<'_>, job: &JobKey) -> Result<Vec<BuiltFile>, AssetError> {
        Err(AssetError::not_found(format!(
            "the data bundles are exported at start-up, there is no job {job}"
        )))
    }

    fn on_start(&self, ctx: &BuildCtx<'_>, cache: &Cache) -> Result<(), AssetError> {
        let started = std::time::Instant::now();
        let store = BundleStore::new(cache.path_of("data"));
        let fp = fingerprint(ctx.data.meta.fingerprint, vfs_tree_digest(ctx.vfs));
        let outcome = store
            .ensure(&fp, || {
                export_all(ctx.data, &self.client, ctx.vfs, &self.options)
            })
            .map_err(|e| AssetError::build(format!("exporting the data bundles: {e}")))?;
        let ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let kind = match outcome {
            StoreOutcome::Hit(_) => BundleOutcome::Hit,
            StoreOutcome::Built(_) => BundleOutcome::Built,
        };
        tracing::info!(
            outcome = ?kind,
            bundles = outcome.manifest().bundles.len(),
            data_version = %outcome.manifest().data_version,
            ms,
            "client data bundles ready"
        );
        *self.last.lock() = Some((kind, ms));
        Ok(())
    }
}
