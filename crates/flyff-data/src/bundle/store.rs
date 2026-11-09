//! The on-disk bundle cache (`cache/data/`, Plan/14 §10.3): `<bundle>.<h16>.json`, `<bundle>.<h16>.json.gz` and
//! `manifest.json`, written atomically, with a fingerprint stamp that decides cache hits and a sweep of stale files.
//!
//! This module uses `std::fs`; it is listed in `[io_exempt]` of `xtask/lint/deps-allow.toml` next to `load/`.
//!
//! ```text
//! ensure(fingerprint, build):  stamp == fingerprint and manifest.json readable and every listed file present
//!                              with its size  ->  hit: nothing is rebuilt
//!                              otherwise      ->  build(), write the files, then the manifest, then the stamp,
//!                                                 then sweep the files of the 20 own bundles that the new
//!                                                 manifest no longer lists (`motions.*` and `model-bounds.*`
//!                                                 belong to `flyff-assets` and are never touched)
//! ```
//!
//! The fingerprint of the `data-bundles` derived kind is `GameData.meta.fingerprint` + `BUNDLE_BUILDER_VERSION`
//! (+ a digest of the file tree for the `null` rules that depend on which files exist): [`fingerprint`].

use super::BUNDLE_BUILDER_VERSION;
use super::catalog::catalog;
use super::envelope::{BundleError, BundleSet};
use super::manifest::h16_of_url;
use super::types::BundleManifest;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// The file that holds the fingerprint the files were built for.
const STAMP: &str = ".fingerprint";
/// The manifest file name.
pub const MANIFEST_FILE: &str = "manifest.json";

/// The cache fingerprint: the game-data fingerprint, the builder version and `tree` (a digest of the set of files
/// the `null` rules looked at; 0 when it does not matter).
pub fn fingerprint(game_fingerprint: u128, tree: u64) -> String {
    format!("{game_fingerprint:032x}-b{BUNDLE_BUILDER_VERSION}-{tree:016x}")
}

/// `xxh3_64` over `(path, size, mtime)` of every file of the VFS: the part of the fingerprint that tracks which
/// files exist (14 §10.1 `null` rule).
#[cfg(feature = "load")]
pub fn vfs_tree_digest(vfs: &flyff_vfs::Vfs) -> u64 {
    use std::hash::Hasher;
    let mut h = xxhash_rust::xxh3::Xxh3::new();
    for e in vfs.iter() {
        h.write(e.path.as_str().as_bytes());
        h.write(&e.size.to_le_bytes());
        h.write(&e.mtime_ns.to_le_bytes());
    }
    h.finish()
}

/// What [`BundleStore::ensure`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreOutcome {
    /// The files were current; nothing was exported.
    Hit(BundleManifest),
    /// The bundles were exported and written.
    Built(BundleManifest),
}

impl StoreOutcome {
    /// The manifest on disk.
    pub fn manifest(&self) -> &BundleManifest {
        match self {
            Self::Hit(m) | Self::Built(m) => m,
        }
    }

    /// `true` when nothing had to be rebuilt.
    pub fn is_hit(&self) -> bool {
        matches!(self, Self::Hit(_))
    }
}

/// A `cache/data/` directory.
#[derive(Clone, Debug)]
pub struct BundleStore {
    dir: PathBuf,
}

fn io(path: &Path, e: &std::io::Error) -> BundleError {
    BundleError::Io {
        path: path.display().to_string(),
        message: e.to_string(),
    }
}

/// Writes `bytes` to `path` through a temporary file in the same directory (atomic on the same file system).
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), BundleError> {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp = path.with_file_name(format!(".{name}.tmp{}", std::process::id()));
    fs::write(&tmp, bytes).map_err(|e| io(&tmp, &e))?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        io(path, &e)
    })
}

impl BundleStore {
    /// A store over `dir` (created on the first write).
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The path of `name` in the store.
    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    /// The manifest on disk, `None` when it is missing or unreadable.
    pub fn load_manifest(&self) -> Option<BundleManifest> {
        let bytes = fs::read(self.path(MANIFEST_FILE)).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// `true` when the stamp equals `fp` and the manifest and every file it lists are present with their sizes.
    fn is_current(&self, fp: &str) -> Option<BundleManifest> {
        let stamp = fs::read_to_string(self.path(STAMP)).ok()?;
        if stamp.trim() != fp {
            return None;
        }
        let m = self.load_manifest()?;
        for (name, e) in &m.bundles {
            let h = h16_of_url(&e.url)?;
            let json = fs::metadata(self.path(&format!("{name}.{h}.json"))).ok()?;
            let gz = fs::metadata(self.path(&format!("{name}.{h}.json.gz"))).ok()?;
            if json.len() != e.bytes || gz.len() != e.gz_bytes {
                return None;
            }
        }
        Some(m)
    }

    /// The cache hit of [`Self::ensure`]: the manifest when the files built for `fp` are all there.
    pub fn lookup(&self, fp: &str) -> Option<BundleManifest> {
        self.is_current(fp)
    }

    /// Serves the files of `fp` when they are current, otherwise runs `build`, writes the result and sweeps
    /// the stale files.
    pub fn ensure(
        &self,
        fp: &str,
        build: impl FnOnce() -> Result<BundleSet, BundleError>,
    ) -> Result<StoreOutcome, BundleError> {
        if let Some(m) = self.is_current(fp) {
            return Ok(StoreOutcome::Hit(m));
        }
        let set = build()?;
        self.write(&set, fp)?;
        Ok(StoreOutcome::Built(set.manifest))
    }

    /// Writes `set` (the files, then `manifest.json`, then the stamp) and sweeps the stale files of the own
    /// bundles.
    pub fn write(&self, set: &BundleSet, fp: &str) -> Result<(), BundleError> {
        fs::create_dir_all(&self.dir).map_err(|e| io(&self.dir, &e))?;
        // the stamp goes first-away: a crash between the files and the stamp must not look current
        let _ = fs::remove_file(self.path(STAMP));
        for b in &set.bundles {
            let json = self.path(&b.file_name());
            if fs::metadata(&json).ok().map(|m| m.len()) != Some(b.json.len() as u64) {
                write_atomic(&json, &b.json)?;
            }
            let gz = self.path(&format!("{}.gz", b.file_name()));
            if fs::metadata(&gz).ok().map(|m| m.len()) != Some(b.gz.len() as u64) {
                write_atomic(&gz, &b.gz)?;
            }
        }
        self.write_manifest(&set.manifest)?;
        write_atomic(&self.path(STAMP), fp.as_bytes())?;
        self.sweep(&set.manifest);
        Ok(())
    }

    /// Writes `manifest.json` atomically (also used by the finalisation that adds the two projections).
    pub fn write_manifest(&self, m: &BundleManifest) -> Result<(), BundleError> {
        let bytes = serde_json::to_vec(m).map_err(|e| BundleError::Serialize {
            bundle: "manifest".to_owned(),
            message: e.to_string(),
        })?;
        write_atomic(&self.path(MANIFEST_FILE), &bytes)
    }

    /// Deletes `<bundle>.<h16>.json(.gz)` of the 20 own bundles whose `h16` the manifest no longer lists.
    fn sweep(&self, m: &BundleManifest) {
        let own: BTreeSet<&str> = catalog()
            .iter()
            .filter(|s| s.project.is_some())
            .map(|s| s.name)
            .collect();
        let live: BTreeSet<String> = m
            .bundles
            .iter()
            .filter_map(|(n, e)| h16_of_url(&e.url).map(|h| format!("{n}.{h}.json")))
            .collect();
        let Ok(rd) = fs::read_dir(&self.dir) else {
            return;
        };
        for entry in rd.flatten() {
            let file = entry.file_name().to_string_lossy().into_owned();
            let base = file.strip_suffix(".gz").unwrap_or(&file);
            let Some(stem) = base.strip_suffix(".json") else {
                continue;
            };
            let Some((name, h)) = stem.rsplit_once('.') else {
                continue;
            };
            if own.contains(name) && h.len() == 16 && !live.contains(base) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}
