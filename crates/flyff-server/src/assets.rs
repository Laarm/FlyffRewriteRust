//! The asset side of the server state (Plan/24 §6.4, §11.1 step 5e-f, WP-WEB-04/05): [`AssetsState`] holds the
//! VFS, the loaded game data, `AssetPrep` and the manifests the HTTP routes serve.
//!
//! **Construction** ([`AssetsState::new`]) is the *finalisation* step of the start-up sequence. Call it after
//! `AssetPrep::load_server_derived()` (the start-up kinds exist) and before the listener serves:
//!
//! 1. the bundle manifest `cache/data/manifest.json` (written by the `data-bundles` builder) is loaded, the
//!    bundles of the derived kinds (`motions`, `model-bounds`, announced through
//!    `ManifestParts::set_external_bundle`) are inserted with `BundleManifest::with_external` (which recomputes
//!    `data_version` over all entries, Plan/14 §10.3) and the result is written back atomically;
//! 2. the [`BundleIndex`] (`/data/<bundle>.<h16>.json` lookup, public flags) is built and handed to
//!    `AssetPrep::set_bundles`;
//! 3. the [`ClientManifest`] is assembled (`res.groups`, `rules`, `gen` fingerprints of every registered kind,
//!    builder parts) and handed to `AssetPrep::set_manifest`; its JSON and fingerprint are kept for
//!    `/data/asset-manifest.<fp>.json`;
//! 4. the public file set of §13 (title music and click sound) is resolved from `ClientData`.
//!
//! Constructors: [`AssetsState::new`] for a started `AssetPrep`, [`AssetsState::open`] to open the VFS and start
//! `AssetPrep` from the configuration, and `AssetsState::fixture` (feature `test-hooks`) for tests.

mod bundles;

pub use bundles::{BundleOutcome, DataBundleBuilder};

use crate::config::{Config, ResolvedPaths};
use bytes::Bytes;
use flyff_assets::cache::{gzip_beneficial, gzip_bytes};
use flyff_assets::{
    AssetBuilder, AssetConfig, AssetError, AssetFeatures, AssetKind, AssetPrep, AssetTelemetry,
    BundleIndex, ClientManifest, Clock, FingerprintMode, Fp, GzipRawMode, ManifestParts,
};
use flyff_data::bundle::manifest::data_version_of;
use flyff_data::bundle::store::BundleStore;
use flyff_data::bundle::types::{BUNDLE_FORMAT, BundleManifest};
use flyff_data::{ClientData, GameData};
use flyff_vfs::{Vfs, VfsConfig};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

/// A JSON document served from memory in both encodings.
#[derive(Clone, Debug)]
pub struct StoredJson {
    /// The identity bytes.
    pub identity: Bytes,
    /// The gzip form, when it is smaller than 90 % of the identity form (W8).
    pub gz: Option<Bytes>,
}

impl StoredJson {
    /// Stores `identity` (and its gzip form when beneficial).
    pub fn new(identity: Vec<u8>) -> StoredJson {
        let gz = gzip_bytes(&identity, 9);
        let gz = gzip_beneficial(identity.len() as u64, gz.len() as u64).then(|| Bytes::from(gz));
        StoredJson {
            identity: Bytes::from(identity),
            gz,
        }
    }
}

/// The monotonic + wall clock of the asset framework (`flyff_common::rt::MonoClock` plus `unix_s`).
#[derive(Debug)]
pub struct ServerClock {
    mono: flyff_common::rt::MonoClock,
}

impl Default for ServerClock {
    fn default() -> Self {
        Self {
            mono: flyff_common::rt::MonoClock::start(),
        }
    }
}

impl Clock for ServerClock {
    fn now_ms(&self) -> u64 {
        self.mono.now_ns() / 1_000_000
    }

    fn unix_s(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }
}

/// Everything the asset routes need.
pub struct AssetsState {
    /// The layered case-insensitive file index.
    pub vfs: Arc<Vfs>,
    /// The loaded game data (an empty `GameData` until the real start-up loads it).
    pub data: Arc<GameData>,
    /// The client-only data (`None` until loaded).
    pub client: Option<Arc<ClientData>>,
    /// The derived-asset framework.
    pub prep: AssetPrep,
    /// The assembled client manifest (immutable for the process lifetime).
    pub manifest: Arc<ClientManifest>,
    /// Its fingerprint (`/data/asset-manifest.<fp>.json`).
    pub manifest_fp: Fp,
    /// The manifest JSON in both encodings.
    pub manifest_json: StoredJson,
    /// The finalised bundle manifest.
    pub bundle_manifest: Arc<BundleManifest>,
    /// Its JSON in both encodings (`/data/manifest.json`).
    pub bundle_manifest_json: StoredJson,
    /// The `/data` lookup table.
    pub bundles: Arc<BundleIndex>,
    /// `cache/data/`.
    pub data_dir: PathBuf,
    /// The canonical paths that stay public while assets are gated (§13): title music, click sound.
    pub public_files: BTreeSet<String>,
}

impl std::fmt::Debug for AssetsState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AssetsState")
            .field("manifest_fp", &self.manifest_fp)
            .field("data_version", &self.bundles.data_version)
            .finish_non_exhaustive()
    }
}

impl AssetsState {
    /// Finalises the bundle manifest, assembles the client manifest and wraps everything (see the module
    /// documentation). Errors only when `manifest.json` cannot be written back.
    pub fn new(
        vfs: Arc<Vfs>,
        data: Arc<GameData>,
        client: Option<Arc<ClientData>>,
        prep: AssetPrep,
    ) -> Result<AssetsState, AssetError> {
        let data_dir = prep.cache().root().join("data");
        let bundle_manifest = finalize_bundle_manifest(&prep, &data_dir)?;
        let bundles = Arc::new(BundleIndex::from_manifest(&bundle_manifest));
        prep.set_bundles(Arc::clone(&bundles));
        let bundle_manifest_json = StoredJson::new(
            serde_json::to_vec(&bundle_manifest)
                .map_err(|e| AssetError::build(format!("bundle manifest: {e}")))?,
        );
        let manifest = Arc::new(ClientManifest::assemble(&prep));
        prep.set_manifest(Arc::clone(&manifest));
        let manifest_fp = manifest.fp();
        let manifest_json = StoredJson::new(manifest.to_json());
        let public_files = public_files(&data, client.as_deref());
        Ok(AssetsState {
            vfs,
            data,
            client,
            prep,
            manifest,
            manifest_fp,
            manifest_json,
            bundle_manifest: Arc::new(bundle_manifest),
            bundle_manifest_json,
            bundles,
            data_dir,
            public_files,
        })
    }

    /// Opens the VFS and starts `AssetPrep` from the configuration (`make_builders` registers the builders
    /// other crates own, e.g. the data-bundle exporter; the ones compiled into `flyff-assets` are always
    /// there), then finalises. The real start-up sequence passes the loaded `GameData`; until it exists
    /// pass `Arc::new(GameData::default())`.
    pub fn open(
        cfg: &Config,
        paths: &ResolvedPaths,
        data: Arc<GameData>,
        client: Option<Arc<ClientData>>,
        clock: Arc<dyn Clock>,
        telemetry: Arc<AssetTelemetry>,
        make_builders: impl FnOnce(&Arc<ManifestParts>) -> Vec<Arc<dyn AssetBuilder>>,
    ) -> Result<AssetsState, AssetError> {
        let vfs_cfg = VfsConfig::new(&paths.resource).with_overrides(&paths.overrides);
        let mut vfs = Vfs::open_lenient(&vfs_cfg)
            .map_err(|e| AssetError::new(flyff_assets::AssetErrorKind::Io, format!("vfs: {e}")))?;
        vfs.set_cp1252_lossy(cfg.data.cp1252_lossy);
        let vfs = Arc::new(vfs);
        let prep = start_prep(
            Arc::clone(&vfs),
            Arc::clone(&data),
            prep_config(cfg, paths),
            clock,
            telemetry,
            make_builders,
        )?;
        AssetsState::new(vfs, data, client, prep)
    }

    /// `true` when `fp` is the current group fingerprint of the `/res` directory `dir` (`/res` answers any
    /// other fingerprint with the current bytes and `Cache-Control: no-store`).
    pub fn group_is_current(&self, dir: &str, fp: Fp) -> bool {
        self.manifest.res.groups.get(dir) == Some(&fp)
    }
}

/// Starts `AssetPrep` over an already opened VFS (Plan/24 §11.1 steps 1-4): the builders compiled into
/// `flyff-assets` plus `make_builders` (the data-bundle exporter and other crates' builders). The real
/// start-up then calls `AssetPrep::load_server_derived` and finalises with [`AssetsState::new`].
pub fn start_prep(
    vfs: Arc<Vfs>,
    data: Arc<GameData>,
    cfg: AssetConfig,
    clock: Arc<dyn Clock>,
    telemetry: Arc<AssetTelemetry>,
    make_builders: impl FnOnce(&Arc<ManifestParts>) -> Vec<Arc<dyn AssetBuilder>>,
) -> Result<AssetPrep, AssetError> {
    AssetPrep::start_with(vfs, data, cfg, clock, telemetry, |parts| {
        let mut all = flyff_assets::build::default_builders(parts);
        all.extend(make_builders(parts));
        all
    })
}

/// Maps the server configuration to the framework's [`AssetConfig`] (the notes of `flyff-assets`).
pub fn prep_config(cfg: &Config, paths: &ResolvedPaths) -> AssetConfig {
    let mut c = AssetConfig::new(&paths.cache);
    c.threads = cfg.runtime.asset_threads as usize;
    c.nice = i32::try_from(cfg.runtime.asset_nice).unwrap_or(10);
    c.startup_threads = cfg.runtime.startup_threads as usize;
    c.prewarm = cfg.assets.prewarm;
    c.gzip_raw = match cfg.assets.gzip_raw.as_str() {
        "lazy" => GzipRawMode::Lazy,
        "off" => GzipRawMode::Off,
        _ => GzipRawMode::All,
    };
    c.disk_budget_bytes = u64::from(cfg.assets.disk_budget_mb) << 20;
    c.fingerprint_mode = if cfg.assets.fingerprint_mode == "content" {
        FingerprintMode::Content
    } else {
        FingerprintMode::Stat
    };
    c.packs = cfg.assets.packs == "on";
    c.features = AssetFeatures::all();
    c.server_build = format!("flyff {}", crate::build_info::BUILD_ID);
    c
}

/// Step 1 of the finalisation: the bundle manifest with the derived kinds' bundles inserted.
fn finalize_bundle_manifest(
    prep: &AssetPrep,
    data_dir: &std::path::Path,
) -> Result<BundleManifest, AssetError> {
    let store = BundleStore::new(data_dir);
    let base = store.load_manifest();
    let existed = base.is_some();
    let mut manifest = base.unwrap_or_else(|| BundleManifest {
        format: BUNDLE_FORMAT,
        data_version: data_version_of(std::iter::empty()),
        server_build: prep.config().server_build.clone(),
        bundles: std::collections::BTreeMap::new(),
    });
    let before = manifest.clone();
    for (name, entry) in prep.manifest_parts().external_bundles() {
        manifest = manifest.with_external(&name, entry);
    }
    if manifest != before || (!existed && !manifest.bundles.is_empty()) {
        store
            .write_manifest(&manifest)
            .map_err(|e| AssetError::build(format!("writing manifest.json: {e}")))?;
    }
    Ok(manifest)
}

/// The paths that stay public while assets are gated (§13): the title music (`BGM_TITLE`) and the UI click
/// sound (`SND_INF_CLICK`), resolved through the client tables.
fn public_files(data: &GameData, client: Option<&ClientData>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let Some(client) = client else {
        return out;
    };
    if let Some(file) = data
        .defines
        .value_u32("BGM_TITLE")
        .and_then(|id| client.music.get(&id))
        && let Ok(p) = Vfs::normalize(file)
    {
        out.insert(p.to_string());
    }
    if let Some(file) = data
        .defines
        .value_u32("SND_INF_CLICK")
        .and_then(|id| client.sounds.get(&id))
        && !file.is_empty()
        && let Ok(p) = Vfs::normalize(&Vfs::join("sound", file))
    {
        out.insert(p.to_string());
    }
    out
}

/// The kinds `/gen` serves (re-exported for the handler's slug check).
pub fn served_kind(slug: &str) -> Option<AssetKind> {
    AssetKind::from_slug(slug).filter(|k| k.served())
}
