//! The client asset manifest skeleton and the parts builders contribute (Plan/24 §6.4).
//!
//! **Scope note.** WP-WEB-02 only needs the *types* (`AssetPrep::manifest()` returns one) and the
//! small shared bag [`ManifestParts`] through which builders hand start-up knowledge (the DDS fix list,
//! cursor hotspots, loadable worlds) to the manifest and to each other (the gzip prewarm skips
//! `dds_fix` files). The manifest *content* (`res.groups`, `rules`, the golden sample, `asset_url`,
//! ts-rs/TS generation) belongs to WP-WEB-04/06; they extend this module.

use crate::fp::{Fp, manifest_fp};
pub use crate::manifest_ops::{
    PUBLIC_BUNDLES, asset_url, conversion_rules, encode_path, encode_segment,
};
use flyff_data::bundle::ManifestEntry;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::RwLock;

/// Hotspot and size of one parsed `.cur` (`ClientManifest.cursors`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CursorInfo {
    /// Hotspot `[x, y]`.
    pub hotspot: [u32; 2],
    /// Size `[w, h]`.
    pub size: [u32; 2],
}

/// One loadable world (`ClientManifest.worlds`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct WorldInfo {
    /// Lands `[width, height]`.
    pub lands: [u32; 2],
    /// Meters per unit.
    pub mpu: u32,
    /// `x + z * width` of every existing `.lnd`.
    pub present: Vec<u32>,
}

#[derive(Default)]
struct Parts {
    dds_fix: BTreeMap<String, BTreeSet<String>>,
    cursors: BTreeMap<String, CursorInfo>,
    worlds: BTreeMap<String, WorldInfo>,
    external_bundles: BTreeMap<String, ManifestEntry>,
    keep_fps: BTreeSet<Fp>,
}

/// Start-up knowledge shared between builders and the manifest. Thread-safe; written by
/// `AssetBuilder::on_start`, read by jobs and by manifest assembly.
#[derive(Default)]
pub struct ManifestParts {
    inner: RwLock<Parts>,
}

impl std::fmt::Debug for ManifestParts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ManifestParts").finish_non_exhaustive()
    }
}

impl ManifestParts {
    /// An empty bag.
    pub fn new() -> Self {
        Self::default()
    }

    fn read<T>(&self, f: impl FnOnce(&Parts) -> T) -> T {
        match self.inner.read() {
            Ok(g) => f(&g),
            Err(p) => f(&p.into_inner()),
        }
    }

    fn write(&self, f: impl FnOnce(&mut Parts)) {
        match self.inner.write() {
            Ok(mut g) => f(&mut g),
            Err(p) => f(&mut p.into_inner()),
        }
    }

    /// Replaces the `dds_fix` file list of source directory `dir` (lower-case file names).
    pub fn set_dds_fix(&self, dir: &str, files: impl IntoIterator<Item = String>) {
        let set: BTreeSet<String> = files.into_iter().collect();
        self.write(|p| {
            p.dds_fix.insert(dir.to_owned(), set);
        });
    }

    /// `true` when the normalised path `dir/file` is a DDS whose client form is the fixed one.
    pub fn is_dds_fix(&self, path: &str) -> bool {
        let dir = crate::res::dir_of(path);
        let file = path.rsplit('/').next().unwrap_or(path);
        self.read(|p| p.dds_fix.get(dir).is_some_and(|s| s.contains(file)))
    }

    /// A copy of the `dds_fix` lists.
    pub fn dds_fix(&self) -> BTreeMap<String, BTreeSet<String>> {
        self.read(|p| p.dds_fix.clone())
    }

    /// Records the hotspot of cursor `stem`.
    pub fn set_cursor(&self, stem: &str, info: CursorInfo) {
        self.write(|p| {
            p.cursors.insert(stem.to_owned(), info);
        });
    }

    /// A copy of the cursor table.
    pub fn cursors(&self) -> BTreeMap<String, CursorInfo> {
        self.read(|p| p.cursors.clone())
    }

    /// Records a loadable world folder.
    pub fn set_world(&self, folder: &str, info: WorldInfo) {
        self.write(|p| {
            p.worlds.insert(folder.to_owned(), info);
        });
    }

    /// A copy of the world table.
    pub fn worlds(&self) -> BTreeMap<String, WorldInfo> {
        self.read(|p| p.worlds.clone())
    }

    /// Records a JSON bundle that a derived-asset builder produced (`motions`, `model-bounds`; Plan/24
    /// §10.5, §10.6): the file `cache/data/<name>.<h16>.json(.gz)` exists and `entry` is its line of
    /// `manifest.json`. The manifest finalisation of WP-WEB-05 adds these to the bundle manifest with
    /// `BundleManifest::with_external`.
    pub fn set_external_bundle(&self, name: &str, entry: ManifestEntry) {
        self.write(|p| {
            p.external_bundles.insert(name.to_owned(), entry);
        });
    }

    /// A copy of the external bundle entries by bundle name.
    pub fn external_bundles(&self) -> BTreeMap<String, ManifestEntry> {
        self.read(|p| p.external_bundles.clone())
    }

    /// Keeps the `index/` and `server/` files whose names embed `fp` when sweeping: for builders whose
    /// index file is named after a fingerprint that is not a partition fingerprint (the DDS scan).
    /// Usable from `AssetBuilder::on_start`, which has no `AssetPrep` yet.
    pub fn keep_fingerprint(&self, fp: Fp) {
        self.write(|p| {
            p.keep_fps.insert(fp);
        });
    }

    /// The fingerprints registered with [`Self::keep_fingerprint`].
    pub fn kept_fingerprints(&self) -> BTreeSet<Fp> {
        self.read(|p| p.keep_fps.clone())
    }
}

/// `res` part: the raw-asset URL prefix and the per-directory group fingerprints.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ResManifest {
    /// `/res`.
    pub prefix: String,
    /// Directory to group fingerprint.
    pub groups: BTreeMap<String, Fp>,
}

impl Default for ResManifest {
    fn default() -> Self {
        Self {
            prefix: "/res".to_owned(),
            groups: BTreeMap::new(),
        }
    }
}

/// One entry of the `gen` map: a single fingerprint or one per partition.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(untagged)]
pub enum GenEntry {
    /// Kinds without partitions (`motion-table`, `cursor`, ...).
    Single(Fp),
    /// Kinds with partitions (`png`: source dir, `collision`: world folder).
    Partitioned(BTreeMap<String, Fp>),
}

/// The `gen` part: the URL prefix plus one entry per served kind slug.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct GenManifest {
    /// `/gen`.
    pub prefix: String,
    /// Slug to entry.
    #[serde(flatten)]
    pub kinds: BTreeMap<String, GenEntry>,
}

impl Default for GenManifest {
    fn default() -> Self {
        Self {
            prefix: "/gen".to_owned(),
            kinds: BTreeMap::new(),
        }
    }
}

/// One conversion rule copied from the §5.2 table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ManifestRule {
    /// Directory pattern.
    pub dir: String,
    /// Extensions.
    pub exts: Vec<String>,
    /// `"png"` or `"dds"`.
    pub form: String,
    /// `"exact"` / `"none"` for PNG rules.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}

/// `gzip` part.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct GzipManifest {
    /// `assets.gzip_raw` (`all` / `lazy` / `off`).
    pub raw: String,
}

impl Default for GzipManifest {
    fn default() -> Self {
        Self {
            raw: "all".to_owned(),
        }
    }
}

/// The client asset manifest (`/data/asset-manifest.<fp>.json`, Plan/24 §6.4). Immutable for the
/// process lifetime once assembled.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ClientManifest {
    /// Format version, `1`.
    pub format: u32,
    /// `flyff <version>+<commit>`.
    pub server_build: String,
    /// Raw assets.
    pub res: ResManifest,
    /// Conversion rules.
    pub rules: Vec<ManifestRule>,
    /// Derived assets (`gen` is a reserved word in edition 2024, hence the field name).
    #[serde(rename = "gen")]
    pub gen_urls: GenManifest,
    /// DDS files whose client form is `/gen/dds`, per source directory.
    pub dds_fix: BTreeMap<String, BTreeSet<String>>,
    /// Parsed cursors by lower-case file stem.
    pub cursors: BTreeMap<String, CursorInfo>,
    /// Loadable worlds by folder.
    pub worlds: BTreeMap<String, WorldInfo>,
    /// Gzip policy.
    pub gzip: GzipManifest,
}

impl ClientManifest {
    /// A valid empty manifest (what `AssetPrep::manifest()` returns before assembly).
    pub fn empty(server_build: &str) -> Self {
        Self {
            format: 1,
            server_build: server_build.to_owned(),
            ..Self::default()
        }
    }

    /// Deterministic JSON bytes (`BTreeMap` order, no whitespace).
    pub fn to_json(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    /// `manifest_fp` of [`Self::to_json`].
    pub fn fp(&self) -> Fp {
        manifest_fp(&self.to_json())
    }
}

/// One bundle of the `/data` lookup table (WP-WEB-05; built by [`BundleIndex::from_manifest`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleEntry {
    /// The 16-hex content hash in the file name.
    pub h16: String,
    /// Uncompressed file name inside `cache/data/` (`<bundle>.<h16>.json`; the gzip file is `<file>.gz`).
    pub file: String,
    /// `true` for lazily fetched bundles (help, quests, defines).
    pub lazy: bool,
    /// `true` for bundles reachable without login when assets are gated.
    pub public: bool,
    /// Size of the uncompressed file.
    #[serde(default)]
    pub bytes: u64,
    /// Size of the gzip file.
    #[serde(default)]
    pub gz_bytes: u64,
}

/// The `/data` lookup table, loaded from `cache/data/manifest.json` (WP-WEB-05).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleIndex {
    /// The bundle manifest's `data_version`.
    pub data_version: String,
    /// Bundle name to entry.
    pub bundles: BTreeMap<String, BundleEntry>,
}
