//! The layered case-insensitive file index (Plan/14 §2.1-2.5, ref/17 §3.1).
//!
//! Two roots are indexed once at start-up: the original `Resource/` folder and the optional
//! `overrides/` folder. Keys are normalised paths ([`VfsPath`]); a lookup tries `overrides/` first
//! (whole-file shadowing) and then `Resource/`. Both roots are only ever read.

use crate::path::{VfsPath, normalize};
use crate::text::{DecodedText, EncodingHint, decode_text};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use walkdir::WalkDir;

/// Where the roots are (config keys `paths.resource_dir` and `paths.overrides_dir`, doc 10).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VfsConfig {
    /// The original `Resource/` folder. Must exist and contain `Masquerade.prj` (see [`Vfs::open`]).
    pub resource_dir: PathBuf,
    /// Admin-supplied replacement and added files; a missing directory means no overrides.
    pub overrides_dir: Option<PathBuf>,
}

impl VfsConfig {
    /// A configuration with only the Resource root.
    pub fn new(resource_dir: impl Into<PathBuf>) -> Self {
        Self {
            resource_dir: resource_dir.into(),
            overrides_dir: None,
        }
    }

    /// The same configuration with an `overrides/` directory.
    #[must_use]
    pub fn with_overrides(mut self, overrides_dir: impl Into<PathBuf>) -> Self {
        self.overrides_dir = Some(overrides_dir.into());
        self
    }
}

/// Which root a file comes from.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Layer {
    /// The original `Resource/` folder.
    Resource,
    /// The `overrides/` folder (wins over `Resource`).
    Override,
}

/// One indexed file.
#[derive(Clone, Debug)]
pub struct VfsEntry {
    /// The normalised key.
    pub path: VfsPath,
    /// The real path on disk.
    pub real: PathBuf,
    /// Size in bytes at index time.
    pub size: u64,
    /// Modification time (ns since the Unix epoch, negative before it) at index time.
    pub mtime_ns: i64,
    /// The layer the file lives in.
    pub layer: Layer,
}

/// A VFS problem (Plan/14 §2.5).
#[derive(Debug, thiserror::Error)]
pub enum VfsError {
    /// The Resource root is missing, not a directory or has no `Masquerade.prj` (Fatal
    /// `VfsMissingRoot`).
    #[error("Resource root is missing or invalid (no Masquerade.prj): {}", .0.display())]
    MissingRoot(PathBuf),
    /// No such file (the payload is the normalised path).
    #[error("file not found: {0}")]
    NotFound(String),
    /// The path cannot be normalised (empty or escaping the root).
    #[error("invalid resource path: {0}")]
    InvalidPath(String),
    /// An I/O error.
    #[error("i/o error on {}: {source}", .path.display())]
    Io {
        /// The real path that failed.
        path: PathBuf,
        /// The cause.
        #[source]
        source: std::io::Error,
    },
}

/// What an index [`VfsNote`] is about.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VfsNoteKind {
    /// Two real files of one layer normalise to the same key (`VfsCaseCollision`, Warning).
    CaseCollision,
    /// An `overrides/` file shadows or adds a file (`VfsOverride`, Info).
    Override,
    /// An entry could not be indexed (non-UTF-8 name, unreadable metadata, symlink loop): a Warning.
    Unreadable,
}

/// A note produced while indexing. `flyff-vfs` has no dependency on the diagnostics crate; the loader
/// maps these to `Diagnostic`s (`VfsCaseCollision`, `VfsOverride`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VfsNote {
    /// The kind.
    pub kind: VfsNoteKind,
    /// The normalised path, when there is one.
    pub path: Option<VfsPath>,
    /// Human-readable text.
    pub message: String,
}

type FileMap = BTreeMap<VfsPath, VfsEntry>;

/// The layered case-insensitive index. All maps are sorted, so every iteration is deterministic.
#[derive(Debug)]
pub struct Vfs {
    cfg: VfsConfig,
    resource: FileMap,
    overrides: FileMap,
    dirs: BTreeSet<VfsPath>,
    notes: Vec<VfsNote>,
    cp1252_lossy: bool,
}

impl Vfs {
    /// Indexes both roots. The Resource root must be a directory containing `Masquerade.prj` (any
    /// letter case), otherwise [`VfsError::MissingRoot`].
    pub fn open(cfg: &VfsConfig) -> Result<Vfs, VfsError> {
        let vfs = Self::open_lenient(cfg)?;
        if !vfs.resource.contains_key("masquerade.prj")
            && !vfs.overrides.contains_key("masquerade.prj")
        {
            return Err(VfsError::MissingRoot(cfg.resource_dir.clone()));
        }
        Ok(vfs)
    }

    /// Like [`Self::open`] but without the `Masquerade.prj` requirement (tools, tests, partial trees).
    pub fn open_lenient(cfg: &VfsConfig) -> Result<Vfs, VfsError> {
        if !cfg.resource_dir.is_dir() {
            return Err(VfsError::MissingRoot(cfg.resource_dir.clone()));
        }
        let mut notes = Vec::new();
        let mut dirs = BTreeSet::new();
        let resource = index_layer(&cfg.resource_dir, Layer::Resource, &mut notes, &mut dirs)?;
        let overrides = match &cfg.overrides_dir {
            Some(dir) if dir.is_dir() => index_layer(dir, Layer::Override, &mut notes, &mut dirs)?,
            _ => FileMap::new(),
        };
        for e in overrides.values() {
            let what = if resource.contains_key(e.path.as_str()) {
                "shadows the Resource file"
            } else {
                "is added (no such Resource file)"
            };
            notes.push(VfsNote {
                kind: VfsNoteKind::Override,
                path: Some(e.path.clone()),
                message: format!("override {} {what}", e.real.display()),
            });
        }
        // Directories that hold files are implied by the file keys; empty ones were collected by the walk.
        for map in [&resource, &overrides] {
            for key in map.keys() {
                add_ancestors(&mut dirs, key.as_str());
            }
        }
        Ok(Vfs {
            cfg: cfg.clone(),
            resource,
            overrides,
            dirs,
            notes,
            cp1252_lossy: false,
        })
    }

    /// Sets the `data.cp1252_lossy` switch used by [`Self::read_text`] (default `false`, Plan/14 §2.6).
    pub fn set_cp1252_lossy(&mut self, on: bool) {
        self.cp1252_lossy = on;
    }

    /// The configuration the index was built from.
    pub fn config(&self) -> &VfsConfig {
        &self.cfg
    }

    /// Notes produced while indexing (case collisions, overrides, unreadable entries).
    pub fn notes(&self) -> &[VfsNote] {
        &self.notes
    }

    // ---- paths -----------------------------------------------------------------------------

    /// `Vfs::normalize`: Plan/14 §2.2.
    pub fn normalize(p: &str) -> Result<VfsPath, VfsError> {
        normalize(p)
    }

    /// `Vfs::join`: the port of `MakePath(DIR_X, file)`.
    pub fn join(dir: &str, file: &str) -> String {
        crate::path::join(dir, file)
    }

    // ---- lookups ---------------------------------------------------------------------------

    /// The entry for a normalised key: `overrides/` first, then `Resource/`.
    pub fn resolve_key(&self, key: &str) -> Option<&VfsEntry> {
        self.overrides.get(key).or_else(|| self.resource.get(key))
    }

    /// Resolves any path spelling (`"World\\DuDreadfulCave\\DuDreadfulCave.wld"`); `None` when the
    /// path is invalid or the file does not exist.
    pub fn resolve(&self, p: &str) -> Option<&VfsEntry> {
        let key = normalize(p).ok()?;
        self.resolve_key(key.as_str())
    }

    /// `true` when the file exists.
    pub fn exists(&self, p: &str) -> bool {
        self.resolve(p).is_some()
    }

    /// The entry of one specific layer (no shadowing), for tools that compare layers.
    pub fn resolve_in(&self, layer: Layer, p: &str) -> Option<&VfsEntry> {
        let key = normalize(p).ok()?;
        match layer {
            Layer::Resource => self.resource.get(key.as_str()),
            Layer::Override => self.overrides.get(key.as_str()),
        }
    }

    fn require(&self, p: &str) -> Result<&VfsEntry, VfsError> {
        let key = normalize(p)?;
        self.resolve_key(key.as_str())
            .ok_or_else(|| VfsError::NotFound(key.to_string()))
    }

    /// Reads a whole file.
    pub fn read(&self, p: &str) -> Result<Vec<u8>, VfsError> {
        let e = self.require(p)?;
        std::fs::read(&e.real).map_err(|source| VfsError::Io {
            path: e.real.clone(),
            source,
        })
    }

    /// Opens a file for streaming (doc 24).
    pub fn open_file(&self, p: &str) -> Result<std::fs::File, VfsError> {
        let e = self.require(p)?;
        std::fs::File::open(&e.real).map_err(|source| VfsError::Io {
            path: e.real.clone(),
            source,
        })
    }

    /// Reads and decodes a text file (Plan/14 §2.6) with the `cp1252_lossy` switch of this VFS.
    pub fn read_text(&self, p: &str, hint: EncodingHint) -> Result<DecodedText, VfsError> {
        self.read_text_with(p, hint, self.cp1252_lossy)
    }

    /// Reads and decodes a text file with an explicit `cp1252_lossy`.
    pub fn read_text_with(
        &self,
        p: &str,
        hint: EncodingHint,
        cp1252_lossy: bool,
    ) -> Result<DecodedText, VfsError> {
        let bytes = self.read(p)?;
        Ok(decode_text(&bytes, hint, cp1252_lossy))
    }

    /// [`Self::read_text`] with the hint of the hint table (`worlddialog.txt` is windows-1252).
    pub fn read_text_auto(&self, p: &str) -> Result<DecodedText, VfsError> {
        let key = normalize(p)?;
        self.read_text(p, crate::text::hint_for(key.as_str()))
    }

    // ---- iteration -------------------------------------------------------------------------

    /// All files, sorted by path, `overrides/` winning on equal keys.
    pub fn iter(&self) -> impl Iterator<Item = &VfsEntry> {
        MergedIter {
            base: self.resource.values().peekable(),
            over: self.overrides.values().peekable(),
        }
    }

    /// Number of distinct visible files.
    pub fn len(&self) -> usize {
        self.iter().count()
    }

    /// `true` when no file is indexed.
    pub fn is_empty(&self) -> bool {
        self.resource.is_empty() && self.overrides.is_empty()
    }

    /// The files of `overrides/` that replace a Resource file.
    pub fn overridden(&self) -> impl Iterator<Item = &VfsEntry> {
        self.overrides
            .values()
            .filter(|e| self.resource.contains_key(e.path.as_str()))
    }

    /// Every file of `overrides/` (replacing or added), sorted.
    pub fn override_entries(&self) -> impl Iterator<Item = &VfsEntry> {
        self.overrides.values()
    }

    /// The files directly inside `dir` (not recursive), sorted, merged over both layers with the
    /// override winning. `""` or `"."` lists the root.
    pub fn list_dir(&self, dir: &str) -> Vec<&VfsEntry> {
        let Some(prefix) = dir_prefix(dir) else {
            return Vec::new();
        };
        let mut merged: BTreeMap<&str, &VfsEntry> = BTreeMap::new();
        for map in [&self.resource, &self.overrides] {
            for (key, entry) in range_of(map, &prefix) {
                let rest = key.as_str().get(prefix.len()..).unwrap_or_default();
                if !rest.is_empty() && !rest.contains('/') {
                    merged.insert(key.as_str(), entry);
                }
            }
        }
        merged.into_values().collect()
    }

    /// The sub-directories directly inside `dir`, sorted (normalised keys, e.g. `world/wdmadrigal`).
    pub fn list_subdirs(&self, dir: &str) -> Vec<&VfsPath> {
        let Some(prefix) = dir_prefix(dir) else {
            return Vec::new();
        };
        range_set(&self.dirs, &prefix)
            .filter(|k| {
                let rest = k.as_str().get(prefix.len()..).unwrap_or_default();
                !rest.is_empty() && !rest.contains('/')
            })
            .collect()
    }

    /// `true` when `dir` is a directory of either layer.
    pub fn is_dir(&self, dir: &str) -> bool {
        match normalize(dir) {
            Ok(k) => self.dirs.contains(k.as_str()),
            Err(_) => dir_prefix(dir).is_some_and(|p| p.is_empty()),
        }
    }
}

/// The key prefix (`"world/wdmadrigal/"`, `""` for the root) that lists the children of `dir`.
fn dir_prefix(dir: &str) -> Option<String> {
    match flyff_common::normalize_resource_path(dir) {
        Ok(k) => Some(format!("{k}/")),
        Err(flyff_common::PathError::Empty) => Some(String::new()),
        Err(_) => None,
    }
}

/// All entries whose key starts with `prefix` (`""`: everything).
fn range_of<'a, V>(
    map: &'a BTreeMap<VfsPath, V>,
    prefix: &str,
) -> Box<dyn Iterator<Item = (&'a VfsPath, &'a V)> + 'a> {
    if prefix.is_empty() {
        return Box::new(map.iter());
    }
    // `/` + 1 == `0`: the exclusive upper bound of every key that starts with `prefix`.
    let mut hi = prefix.to_owned();
    hi.pop();
    hi.push('0');
    Box::new(map.range::<str, _>((Bound::Included(prefix), Bound::Excluded(hi.as_str()))))
}

fn range_set<'a>(
    set: &'a BTreeSet<VfsPath>,
    prefix: &str,
) -> Box<dyn Iterator<Item = &'a VfsPath> + 'a> {
    if prefix.is_empty() {
        return Box::new(set.iter());
    }
    let mut hi = prefix.to_owned();
    hi.pop();
    hi.push('0');
    Box::new(set.range::<str, _>((Bound::Included(prefix), Bound::Excluded(hi.as_str()))))
}

/// Sorted merge of the two layers; on equal keys the override wins.
struct MergedIter<'a> {
    base: std::iter::Peekable<std::collections::btree_map::Values<'a, VfsPath, VfsEntry>>,
    over: std::iter::Peekable<std::collections::btree_map::Values<'a, VfsPath, VfsEntry>>,
}

impl<'a> Iterator for MergedIter<'a> {
    type Item = &'a VfsEntry;

    fn next(&mut self) -> Option<&'a VfsEntry> {
        match (self.base.peek(), self.over.peek()) {
            (None, None) => None,
            (Some(_), None) => self.base.next(),
            (None, Some(_)) => self.over.next(),
            (Some(b), Some(o)) => match b.path.cmp(&o.path) {
                std::cmp::Ordering::Less => self.base.next(),
                std::cmp::Ordering::Greater => self.over.next(),
                std::cmp::Ordering::Equal => {
                    self.base.next();
                    self.over.next()
                }
            },
        }
    }
}

fn add_ancestors(dirs: &mut BTreeSet<VfsPath>, key: &str) {
    for (i, _) in key.match_indices('/') {
        let Some(dir) = key.get(..i) else { continue };
        if !dirs.contains(dir) {
            dirs.insert(VfsPath::from_normalised(dir.to_owned()));
        }
    }
}

fn is_hidden(e: &walkdir::DirEntry) -> bool {
    e.depth() > 0 && e.file_name().to_string_lossy().starts_with('.')
}

fn mtime_ns(meta: &std::fs::Metadata) -> i64 {
    match meta.modified() {
        Ok(t) => match t.duration_since(UNIX_EPOCH) {
            Ok(d) => i64::try_from(d.as_nanos()).unwrap_or(i64::MAX),
            Err(e) => i64::try_from(e.duration().as_nanos()).map_or(i64::MIN, |n| -n),
        },
        Err(_) => 0,
    }
}

/// Walks one root (symlinks are followed, loops detected; names that start with `.` are skipped).
fn index_layer(
    root: &Path,
    layer: Layer,
    notes: &mut Vec<VfsNote>,
    dirs: &mut BTreeSet<VfsPath>,
) -> Result<FileMap, VfsError> {
    // key -> (entry, real path relative to the root, used for the collision rule)
    let mut files: BTreeMap<VfsPath, (VfsEntry, String)> = BTreeMap::new();
    let walker = WalkDir::new(root)
        .follow_links(true)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|e| !is_hidden(e));
    for item in walker {
        let entry = match item {
            Ok(e) => e,
            Err(err) => {
                if err.depth() == 0 {
                    return Err(VfsError::Io {
                        path: root.to_owned(),
                        source: std::io::Error::other(err),
                    });
                }
                notes.push(VfsNote {
                    kind: VfsNoteKind::Unreadable,
                    path: None,
                    message: format!("skipped: {err}"),
                });
                continue;
            }
        };
        if entry.depth() == 0 {
            continue;
        }
        let is_dir = entry.file_type().is_dir();
        if !is_dir && !entry.file_type().is_file() {
            continue;
        }
        let Some(rel) = entry
            .path()
            .strip_prefix(root)
            .ok()
            .and_then(Path::to_str)
            .map(|s| s.replace(std::path::MAIN_SEPARATOR, "/"))
        else {
            notes.push(VfsNote {
                kind: VfsNoteKind::Unreadable,
                path: None,
                message: format!("skipped: non-UTF-8 file name {:?}", entry.path()),
            });
            continue;
        };
        let key = match normalize(&rel) {
            Ok(k) => k,
            Err(e) => {
                notes.push(VfsNote {
                    kind: VfsNoteKind::Unreadable,
                    path: None,
                    message: format!("skipped {rel}: {e}"),
                });
                continue;
            }
        };
        if is_dir {
            if !dirs.contains(key.as_str()) {
                dirs.insert(key);
            }
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(e) => {
                notes.push(VfsNote {
                    kind: VfsNoteKind::Unreadable,
                    path: Some(key),
                    message: format!("skipped {rel}: {e}"),
                });
                continue;
            }
        };
        let new = VfsEntry {
            path: key.clone(),
            real: entry.path().to_owned(),
            size: meta.len(),
            mtime_ns: mtime_ns(&meta),
            layer,
        };
        match files.get(&key) {
            None => {
                files.insert(key, (new, rel));
            }
            Some((old, old_rel)) => {
                // The winner is the one whose real relative path is byte-wise smallest.
                let new_wins = rel.as_bytes() < old_rel.as_bytes();
                let (win, lose) = if new_wins {
                    (rel.as_str(), old_rel.as_str())
                } else {
                    (old_rel.as_str(), rel.as_str())
                };
                notes.push(VfsNote {
                    kind: VfsNoteKind::CaseCollision,
                    path: Some(old.path.clone()),
                    message: format!("{lose} and {win} are the same path; using {win}"),
                });
                if new_wins {
                    files.insert(key, (new, rel));
                }
            }
        }
    }
    Ok(files.into_iter().map(|(k, (e, _))| (k, e)).collect())
}
