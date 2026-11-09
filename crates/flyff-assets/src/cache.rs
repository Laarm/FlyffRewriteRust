//! The `cache/` directory (Plan/24 §8): layout, atomic writes, lookup, stale sweep, budget counters.
//!
//! ```text
//! cache/
//!   CACHEDIR.TAG  VERSION                     identity files; a VERSION mismatch wipes the layout below
//!   data/   gen/<slug>/<partfp>/<key>[.gz]   res-gz/<vfs path>.<filefp>.gz|nogz
//!   server/ index/   jobs/<slug>/<partfp>/<job name>   tmp/   prepare-report.json
//! ```
//!
//! `jobs/` is this crate's addition to the plan's layout: one empty marker per finished job, written
//! after the job's files, so `prepare` and `flyff check` can tell "job done" without knowing the keys a
//! job produces (a collision job writes every land of a world).
//!
//! Writes are two-phase: [`Cache::stage_artifact`] writes `tmp/<pid>-<n>.part` (the compression rule of
//! W8 applied), [`Cache::commit_all`] makes it durable (`sync_data`) and renames it into place. A crash
//! between the phases leaves only a `.part` file, which the next start deletes (a temp file whose
//! owner process is gone). Prewarm commits are batched: up to 64 files or 1 s of work are synced in one
//! pass and renamed afterwards (Plan/24 §8.3).

use crate::clock::Clock;
use crate::error::AssetError;
use crate::fp::Fp;
use crate::kinds::{AssetKind, BuiltFile, Compress, Layout};
use crate::util::lock;
use bytes::Bytes;
use flyff_common::det::DetMap;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

/// Content of `cache/VERSION`; any other content wipes the cache layout at start-up.
pub const CACHE_VERSION: &str = "flyff-cache 1\n";

/// Content of `cache/CACHEDIR.TAG` (the standard tag, so backup tools skip the directory).
pub const CACHEDIR_TAG: &str = "Signature: 8a477f597d28d172789f06886806bc55\n\
# This file is a cache directory tag created by flyff.\n\
# For information about cache directory tags, see:\n#\thttps://bford.info/cachedir/\n";

/// Entries this crate creates inside `cache/`; everything else is foreign and never deleted.
const KNOWN: &[&str] = &[
    "CACHEDIR.TAG",
    "VERSION",
    "data",
    "gen",
    "res-gz",
    "server",
    "index",
    "tmp",
    "jobs",
    "prepare-report.json",
];

/// Minimum size for the W8 rule (smaller artifacts are stored as is).
pub const GZ_MIN_RAW: u64 = 1024;
const POSITIVE_CAP: usize = 100_000;
const PREWARM_BATCH_FILES: usize = 64;
const PREWARM_BATCH_MS: u64 = 1_000;
const WRITE_WARN_EVERY_MS: u64 = 60_000;
const COMMIT_STRIPES: usize = 64;

/// How an artifact is stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoredEncoding {
    /// The file holds the artifact bytes.
    Identity,
    /// The file holds the gzip form (served with `Content-Encoding: gzip`, inflated for readers).
    Gzip,
}

/// A cached artifact (Plan/24 §7.3).
#[derive(Clone, Debug)]
pub struct CachedFile {
    /// The file (the `.gz` one for gzip artifacts). With `mem` set it may not exist.
    pub path: PathBuf,
    /// Stored length in bytes.
    pub len: u64,
    /// Length of the identity form (from the gzip trailer for `.gz` files).
    pub raw_len: u64,
    /// How the file is encoded.
    pub encoding: StoredEncoding,
    /// The partition fingerprint (`ETag "g-<fp>"`); the file fingerprint for gzip variants of raw files.
    pub fp: Fp,
    /// MIME type of the identity form.
    pub content_type: &'static str,
    /// Set when the artifact could not be written to disk: the stored bytes, so the request can still
    /// be answered (Plan/24 §7.6).
    pub mem: Option<Bytes>,
}

impl CachedFile {
    /// The stored bytes (the gzip form for gzip artifacts).
    pub fn read_stored(&self) -> io::Result<Bytes> {
        match &self.mem {
            Some(b) => Ok(b.clone()),
            None => fs::read(&self.path).map(Bytes::from),
        }
    }

    /// The identity bytes: the stored bytes inflated when the artifact is gzip.
    pub fn read_decoded(&self) -> io::Result<Vec<u8>> {
        let stored = self.read_stored()?;
        match self.encoding {
            StoredEncoding::Identity => Ok(stored.to_vec()),
            StoredEncoding::Gzip => {
                let mut out = Vec::with_capacity(usize::try_from(self.raw_len).unwrap_or(0));
                flate2::read::GzDecoder::new(&stored[..]).read_to_end(&mut out)?;
                Ok(out)
            }
        }
    }
}

/// Result of looking up the gzip variant of a raw file.
#[derive(Clone, Debug)]
pub enum GzLookup {
    /// A `.gz` variant exists.
    Present(CachedFile),
    /// A `.nogz` marker exists: the ratio was >= 0.9.
    NotBeneficial,
    /// Neither exists yet.
    Unknown,
}

/// How a committed file reaches durable storage (Plan/24 §8.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Durability {
    /// `sync_data` then rename now (Startup and Interactive jobs).
    Sync,
    /// Queue for the next batch flush (Prewarm jobs nobody waits for). The returned [`CachedFile`]
    /// points to a path that exists only after [`Cache::flush_pending`]; `lookup` flushes on a miss.
    Deferred,
}

/// A file written to `tmp/` and not yet renamed into place.
#[derive(Debug)]
pub struct Staged {
    dest: PathBuf,
    tmp: Option<PathBuf>,
    other_variant: Option<PathBuf>,
    usage_idx: usize,
    bytes: Option<Bytes>,
    template: CachedFile,
    /// Writing the temp file failed (already counted and logged).
    stage_failed: bool,
}

impl Staged {
    /// The final path.
    pub fn dest(&self) -> &Path {
        &self.dest
    }

    /// Overrides the `raw_len` the committed [`CachedFile`] reports (gzip variants of raw files know
    /// the source size, the framework does not).
    pub fn set_raw_len(&mut self, raw_len: u64) {
        self.template.raw_len = raw_len;
    }

    /// `true` when writing the temp file failed (the artifact will be served from memory).
    pub fn write_failed(&self) -> bool {
        self.stage_failed
    }

    /// The stored length (the gzip form's length for gzip artifacts).
    pub fn stored_len(&self) -> u64 {
        self.template.len
    }
}

struct StageSpec {
    rel: String,
    usage_idx: usize,
    encoding: StoredEncoding,
    raw_len: u64,
    fp: Fp,
    content_type: &'static str,
    other_variant: Option<String>,
}

/// `f(vfs path, file fp)`: is the `res-gz` variant of that file current?
pub type GzCurrent<'a> = &'a dyn Fn(&str, Fp) -> bool;

/// What the sweep keeps (Plan/24 §8.5). Built by `AssetPrep` from the registered builders.
pub struct SweepKeep<'a> {
    /// Current partition fingerprints per registered kind. Kinds absent here are not swept
    /// (registered at another milestone); directories of unknown slugs are removed.
    pub partitions: BTreeMap<AssetKind, BTreeSet<Fp>>,
    /// `Some(f)`: `f(vfs path, file fp)` says whether a `res-gz` variant is current; `None`: leave
    /// `res-gz/` alone (the `gz` kind is not registered).
    pub gz_current: Option<GzCurrent<'a>>,
    /// Fingerprints that keep the files of `server/` and `index/` whose names embed `.<fp>.`.
    pub keep_fps: BTreeSet<Fp>,
}

/// Progress of one sweep batch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SweepStats {
    /// Files deleted (a removed directory counts its files).
    pub deleted_files: u64,
    /// Bytes freed.
    pub deleted_bytes: u64,
    /// `false` when the batch limit was hit and another call is needed.
    pub done: bool,
}

impl SweepStats {
    /// Adds the counters of a later batch.
    pub fn absorb(&mut self, other: SweepStats) {
        self.deleted_files += other.deleted_files;
        self.deleted_bytes += other.deleted_bytes;
        self.done = other.done;
    }
}

/// The cache directory handle.
pub struct Cache {
    root: PathBuf,
    tmp_dir: PathBuf,
    pid: u32,
    next_tmp: AtomicU64,
    clock: Arc<dyn Clock>,
    budget_bytes: u64,
    usage: [AtomicU64; AssetKind::COUNT + 1],
    positive: Mutex<DetMap<String, CachedFile>>,
    pending: Mutex<Vec<Staged>>,
    pending_len: AtomicUsize,
    pending_since_ms: AtomicU64,
    /// Bytes of deferred files that are written but not renamed yet (counted against the budget).
    pending_bytes: AtomicU64,
    /// Held for the whole of a flush: a second flusher waits for the first instead of seeing an
    /// empty queue while the first one is still renaming.
    flush_lock: Mutex<()>,
    /// `true` while a flush runs (set before its batch is taken, cleared after the last rename), so a
    /// lookup that misses can tell "not built" from "being made visible right now".
    flush_active: AtomicBool,
    /// Serialises "read the old length, rename" per destination so the byte counters stay exact when
    /// two writers replace the same file (striped by path hash).
    commit_locks: [Mutex<()>; COMMIT_STRIPES],
    write_errors: AtomicU64,
    last_write_warn_ms: AtomicU64,
}

impl std::fmt::Debug for Cache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cache")
            .field("root", &self.root)
            .field("used_bytes", &self.used_bytes())
            .field("budget_bytes", &self.budget_bytes)
            .finish_non_exhaustive()
    }
}

// ---- key and path rules -------------------------------------------------------------------------

/// A relative key: non-empty, no `\`, no NUL, no empty / `.` / `..` segments, and not absurdly long.
pub fn validate_key(key: &str) -> Result<(), AssetError> {
    let bad = |why: &str| Err(AssetError::bad_key(format!("{why}: {key:?}")));
    if key.is_empty() || key.len() > 480 {
        return bad("key length");
    }
    if key.contains('\0') || key.contains('\\') {
        return bad("forbidden character");
    }
    if key
        .split('/')
        .any(|seg| seg.is_empty() || seg == "." || seg == "..")
    {
        return bad("empty, '.' or '..' segment");
    }
    Ok(())
}

/// The cache-relative path of an artifact.
pub fn artifact_rel(kind: AssetKind, partfp: Fp, key: &str) -> Result<String, AssetError> {
    validate_key(key)?;
    match kind.layout() {
        Layout::Gen => {
            if key.ends_with(".gz") {
                return Err(AssetError::bad_key(format!("key ends in .gz: {key:?}")));
            }
            Ok(format!("gen/{}/{partfp}/{key}", kind.slug()))
        }
        Layout::Data => Ok(format!("data/{key}")),
        Layout::Server => Ok(format!("server/{key}")),
        Layout::ResGz => Ok(format!("res-gz/{key}")),
    }
}

/// The cache-relative path of a job marker.
pub fn marker_rel(kind: AssetKind, partfp: Fp, name: &str) -> Result<String, AssetError> {
    let name = if name.is_empty() { "_" } else { name };
    validate_key(name)?;
    Ok(format!("jobs/{}/{partfp}/{name}", kind.slug()))
}

/// `true` iff storing `gz_len` bytes instead of `raw_len` is worth it: `gz_len / raw_len < 0.9`
/// (exact integer arithmetic) and the artifact is at least [`GZ_MIN_RAW`] long (Plan/24 §8.2, 02 §4.4).
pub fn gzip_beneficial(raw_len: u64, gz_len: u64) -> bool {
    raw_len >= GZ_MIN_RAW && u128::from(gz_len) * 10 < u128::from(raw_len) * 9
}

/// gzip with no file name, mtime 0 and OS byte 255 (deterministic, Plan/24 B3).
pub fn gzip_bytes(data: &[u8], level: u32) -> Vec<u8> {
    let mut enc = flate2::GzBuilder::new()
        .mtime(0)
        .operating_system(255)
        .write(
            Vec::with_capacity(data.len() / 2 + 32),
            flate2::Compression::new(level.min(9)),
        );
    // Writing to a Vec cannot fail.
    let _ = enc.write_all(data);
    enc.finish().unwrap_or_default()
}

fn gz_trailer_len(path: &Path, len: u64) -> u64 {
    if len < 18 {
        return 0;
    }
    let read = || -> io::Result<u32> {
        let mut f = fs::File::open(path)?;
        f.seek(SeekFrom::End(-4))?;
        let mut b = [0u8; 4];
        f.read_exact(&mut b)?;
        Ok(u32::from_le_bytes(b))
    };
    read().map_or(0, u64::from)
}

fn remove_any(path: &Path) {
    let _ = match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(_) => Ok(()),
    };
}

fn count_tree(path: &Path) -> (u64, u64) {
    let Ok(md) = fs::symlink_metadata(path) else {
        return (0, 0);
    };
    if !md.is_dir() {
        return (1, md.len());
    }
    let (mut files, mut bytes) = (0, 0);
    if let Ok(rd) = fs::read_dir(path) {
        for ent in rd.flatten() {
            let (f, b) = count_tree(&ent.path());
            files += f;
            bytes += b;
        }
    }
    (files, bytes)
}

fn proc_alive(pid: u32) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}

/// Parses `<anything>.<16 hex>.<anything>` fingerprints out of a file name.
fn embedded_fp(name: &str) -> Option<Fp> {
    name.split('.').find_map(|part| part.parse::<Fp>().ok())
}

impl Cache {
    /// Opens (creating) the cache at `root` with a budget of `budget_bytes` (Plan/24 §11.1 step 1):
    /// creates the sub-directories, checks `VERSION` (a mismatch wipes the layout, foreign files are
    /// never touched), removes the temp files of dead processes and walks the cache once for the
    /// per-kind byte counters.
    ///
    /// A non-empty directory with unknown content and no `VERSION` is refused: pointing `paths.cache_dir`
    /// at a directory that is not a cache must never delete anything.
    pub fn open(
        root: impl Into<PathBuf>,
        budget_bytes: u64,
        clock: Arc<dyn Clock>,
    ) -> Result<Cache, AssetError> {
        let root = root.into();
        let io_err =
            |what: &str, e: &io::Error| AssetError::io(format!("{what} {}", root.display()), e);
        fs::create_dir_all(&root).map_err(|e| io_err("create", &e))?;

        let version_path = root.join("VERSION");
        match fs::read_to_string(&version_path) {
            Ok(s) if s == CACHE_VERSION => {}
            Ok(_) => {
                tracing::info!(dir = %root.display(), "cache VERSION changed, wiping derived assets");
                wipe_known(&root);
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let foreign: Vec<String> = fs::read_dir(&root)
                    .map_err(|e| io_err("read", &e))?
                    .flatten()
                    .map(|ent| ent.file_name().to_string_lossy().into_owned())
                    .filter(|n| !KNOWN.contains(&n.as_str()))
                    .collect();
                if !foreign.is_empty() {
                    return Err(AssetError::new(
                        crate::AssetErrorKind::Io,
                        format!(
                            "{} is not empty and has no cache VERSION file (found {:?}); refusing to use it as the asset cache",
                            root.display(),
                            foreign.iter().take(5).collect::<Vec<_>>()
                        ),
                    ));
                }
                wipe_known(&root);
            }
            Err(e) => return Err(io_err("read VERSION in", &e)),
        }

        for sub in ["data", "gen", "res-gz", "server", "index", "tmp", "jobs"] {
            fs::create_dir_all(root.join(sub)).map_err(|e| io_err("create", &e))?;
        }
        if !root.join("CACHEDIR.TAG").exists() {
            fs::write(root.join("CACHEDIR.TAG"), CACHEDIR_TAG).map_err(|e| io_err("write", &e))?;
        }
        if !version_path.exists() {
            fs::write(&version_path, CACHE_VERSION).map_err(|e| io_err("write", &e))?;
        }

        let cache = Cache {
            tmp_dir: root.join("tmp"),
            root,
            pid: std::process::id(),
            next_tmp: AtomicU64::new(0),
            clock,
            budget_bytes,
            usage: std::array::from_fn(|_| AtomicU64::new(0)),
            positive: Mutex::new(DetMap::default()),
            pending: Mutex::new(Vec::new()),
            pending_len: AtomicUsize::new(0),
            pending_since_ms: AtomicU64::new(0),
            pending_bytes: AtomicU64::new(0),
            flush_lock: Mutex::new(()),
            flush_active: AtomicBool::new(false),
            commit_locks: std::array::from_fn(|_| Mutex::new(())),
            write_errors: AtomicU64::new(0),
            last_write_warn_ms: AtomicU64::new(0),
        };
        cache.clean_tmp();
        cache.walk_usage();
        Ok(cache)
    }

    /// The cache root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The absolute path of a cache-relative path.
    pub fn path_of(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    // ---- accounting -----------------------------------------------------------------------------

    /// The configured budget (`assets.disk_budget_mb`).
    pub fn budget_bytes(&self) -> u64 {
        self.budget_bytes
    }

    /// Bytes stored for `kind`.
    pub fn kind_bytes(&self, kind: AssetKind) -> u64 {
        self.usage
            .get(kind.index())
            .map_or(0, |a| a.load(Ordering::Relaxed))
    }

    /// Bytes of `index/`, `jobs/`, the report and other non-artifact files.
    pub fn misc_bytes(&self) -> u64 {
        self.usage
            .get(AssetKind::COUNT)
            .map_or(0, |a| a.load(Ordering::Relaxed))
    }

    /// Total cache bytes, deferred files that are still waiting for their rename included.
    pub fn used_bytes(&self) -> u64 {
        let stored: u64 = self.usage.iter().map(|a| a.load(Ordering::Relaxed)).sum();
        stored + self.pending_bytes.load(Ordering::Relaxed)
    }

    /// `used * 100 / budget` as a percentage (saturating; a zero budget reads as 100 %).
    pub fn used_percent(&self) -> u64 {
        if self.budget_bytes == 0 {
            return 100;
        }
        u64::try_from(u128::from(self.used_bytes()) * 100 / u128::from(self.budget_bytes))
            .unwrap_or(u64::MAX)
    }

    /// Optional kinds (`gz`, packs) may be built by Prewarm/Startup jobs while usage is below 95 %.
    pub fn below_prewarm_limit(&self) -> bool {
        u128::from(self.used_bytes()) * 100 < u128::from(self.budget_bytes) * 95
    }

    /// Optional kinds may be built on request while usage is below 100 %.
    pub fn below_budget(&self) -> bool {
        self.used_bytes() < self.budget_bytes
    }

    /// Number of artifact writes that failed since start (`flyff_asset_write_errors_total`).
    pub fn write_errors(&self) -> u64 {
        self.write_errors.load(Ordering::Relaxed)
    }

    fn add_usage(&self, idx: usize, bytes: u64) {
        if let Some(a) = self.usage.get(idx) {
            a.fetch_add(bytes, Ordering::Relaxed);
        }
    }

    fn sub_usage(&self, idx: usize, bytes: u64) {
        if let Some(a) = self.usage.get(idx) {
            let _ = a.try_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
                Some(v.saturating_sub(bytes))
            });
        }
    }

    /// Recomputes the byte counters with one walk of the cache. The incremental counters are exact for
    /// one process; this corrects drift caused by other processes sharing the directory (`prepare`
    /// next to `serve`). Called after a finished sweep and at the end of `prepare`.
    pub fn recount_usage(&self) {
        self.walk_usage();
    }

    fn walk_usage(&self) {
        for a in &self.usage {
            a.store(0, Ordering::Relaxed);
        }
        for (name, idx) in [
            ("data", AssetKind::DataBundle.index()),
            ("server", AssetKind::BossRigs.index()),
            ("res-gz", AssetKind::Gzip.index()),
            ("index", AssetKind::COUNT),
            ("jobs", AssetKind::COUNT),
            ("prepare-report.json", AssetKind::COUNT),
        ] {
            self.add_usage(idx, count_tree(&self.root.join(name)).1);
        }
        if let Ok(rd) = fs::read_dir(self.root.join("gen")) {
            for ent in rd.flatten() {
                let slug = ent.file_name().to_string_lossy().into_owned();
                let idx = AssetKind::from_slug(&slug).map_or(AssetKind::COUNT, AssetKind::index);
                self.add_usage(idx, count_tree(&ent.path()).1);
            }
        }
    }

    // ---- tmp ------------------------------------------------------------------------------------

    fn clean_tmp(&self) {
        let Ok(rd) = fs::read_dir(&self.tmp_dir) else {
            return;
        };
        for ent in rd.flatten() {
            let name = ent.file_name().to_string_lossy().into_owned();
            let owner = name.split('-').next().and_then(|p| p.parse::<u32>().ok());
            let stale = match owner {
                None => true,
                Some(pid) => pid == self.pid || !proc_alive(pid),
            };
            if stale {
                remove_any(&ent.path());
            }
        }
    }

    fn tmp_path(&self, ext: &str) -> PathBuf {
        let n = self.next_tmp.fetch_add(1, Ordering::Relaxed);
        self.tmp_dir.join(format!("{}-{n}.{ext}", self.pid))
    }

    // ---- staging and committing -----------------------------------------------------------------

    fn note_write_error(&self, what: &str, err: &dyn std::fmt::Display) {
        self.write_errors.fetch_add(1, Ordering::Relaxed);
        let now = self.clock.now_ms().saturating_add(1);
        let last = self.last_write_warn_ms.load(Ordering::Relaxed);
        if (last == 0 || now.saturating_sub(last) >= WRITE_WARN_EVERY_MS)
            && self
                .last_write_warn_ms
                .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
        {
            tracing::warn!(error = %err, "asset cache write failed ({what}); serving from memory");
        }
    }

    fn stage_bytes(&self, stored: &[u8]) -> Result<PathBuf, io::Error> {
        let tmp = self.tmp_path("part");
        let write = || -> io::Result<()> {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(stored)?;
            Ok(())
        };
        match write() {
            Ok(()) => Ok(tmp),
            Err(e) => {
                let _ = fs::remove_file(&tmp);
                Err(e)
            }
        }
    }

    fn stage_at(&self, spec: StageSpec, stored: Bytes) -> Staged {
        let StageSpec {
            rel,
            usage_idx,
            encoding,
            raw_len,
            fp,
            content_type,
            other_variant,
        } = spec;
        let dest = self.root.join(&rel);
        let len = stored.len() as u64;
        let tmp = match self.stage_bytes(&stored) {
            Ok(p) => Some(p),
            Err(e) => {
                self.note_write_error(&rel, &e);
                None
            }
        };
        let tmp_failed = tmp.is_none();
        Staged {
            template: CachedFile {
                path: dest.clone(),
                len,
                raw_len,
                encoding,
                fp,
                content_type,
                mem: None,
            },
            dest,
            stage_failed: tmp_failed,
            tmp,
            other_variant: other_variant.map(|r| self.root.join(r)),
            usage_idx,
            // The bytes live in the temp file; they are kept only when it could not be written.
            bytes: tmp_failed.then_some(stored),
        }
    }

    /// Applies the storage rule of W8 to `file` and writes the result to `tmp/`. A temp-file write
    /// error does not fail the call: the [`Staged`] then reports [`Staged::write_failed`] and commits
    /// to a memory-backed [`CachedFile`] (Plan/24 §7.6). Errors are only key violations. This variant
    /// copies the bytes; the framework uses [`Self::stage_built`].
    pub fn stage_artifact(
        &self,
        kind: AssetKind,
        partfp: Fp,
        file: &BuiltFile,
    ) -> Result<Staged, AssetError> {
        self.stage_built(kind, partfp, file.clone())
    }

    /// [`Self::stage_artifact`] consuming the file (no copy of the bytes).
    pub fn stage_built(
        &self,
        kind: AssetKind,
        partfp: Fp,
        file: BuiltFile,
    ) -> Result<Staged, AssetError> {
        let base = artifact_rel(kind, partfp, &file.key)?;
        let raw_len = file.bytes.len() as u64;
        let usage_idx = kind.index();
        let BuiltFile {
            key,
            bytes,
            content_type,
            compress,
        } = file;

        if kind.layout() == Layout::ResGz {
            // `<path>.<fp>.gz` holds gzip bytes already, `.nogz` is a marker; stored as is.
            let encoding = if key.ends_with(".gz") {
                StoredEncoding::Gzip
            } else {
                StoredEncoding::Identity
            };
            let spec = StageSpec {
                rel: base,
                usage_idx,
                encoding,
                raw_len,
                fp: partfp,
                content_type,
                other_variant: None,
            };
            return Ok(self.stage_at(spec, Bytes::from(bytes)));
        }

        let gz = match compress {
            Compress::IfBeneficial { level } if raw_len >= GZ_MIN_RAW => {
                let gz = gzip_bytes(&bytes, level);
                gzip_beneficial(raw_len, gz.len() as u64).then_some(gz)
            }
            _ => None,
        };
        let spec = |rel: String, encoding, other: String| StageSpec {
            rel,
            usage_idx,
            encoding,
            raw_len,
            fp: partfp,
            content_type,
            other_variant: Some(other),
        };
        Ok(match gz {
            Some(gz) => self.stage_at(
                spec(format!("{base}.gz"), StoredEncoding::Gzip, base),
                Bytes::from(gz),
            ),
            None => self.stage_at(
                spec(base.clone(), StoredEncoding::Identity, format!("{base}.gz")),
                Bytes::from(bytes),
            ),
        })
    }

    /// Stages the empty marker that says job `name` of `kind` is done for partition `partfp`.
    pub fn stage_marker(
        &self,
        kind: AssetKind,
        partfp: Fp,
        name: &str,
    ) -> Result<Staged, AssetError> {
        let rel = marker_rel(kind, partfp, name)?;
        let spec = StageSpec {
            rel,
            usage_idx: AssetKind::COUNT,
            encoding: StoredEncoding::Identity,
            raw_len: 0,
            fp: partfp,
            content_type: "application/octet-stream",
            other_variant: None,
        };
        Ok(self.stage_at(spec, Bytes::new()))
    }

    /// The lock that serialises "read the old length, rename" for `dest`.
    fn stripe_lock(&self, dest: &Path) -> Option<std::sync::MutexGuard<'_, ()>> {
        let h = xxhash_rust::xxh3::xxh3_64(dest.as_os_str().as_encoded_bytes());
        let stripe = usize::try_from(h % COMMIT_STRIPES as u64).unwrap_or(0);
        self.commit_locks.get(stripe).map(lock)
    }

    /// Takes the temp file out of `s`, keeping its bytes for a memory-backed answer, and deletes it.
    fn discard_tmp(s: &mut Staged) {
        if let Some(tmp) = s.tmp.take() {
            if s.bytes.is_none() {
                s.bytes = fs::read(&tmp).ok().map(Bytes::from);
            }
            let _ = fs::remove_file(tmp);
        }
    }

    /// Renames the (already synced) temp file into place and updates the byte counters. On failure the
    /// temp file is deleted and `s.bytes` holds its content.
    fn rename_into_place(&self, s: &mut Staged) -> io::Result<()> {
        let Some(tmp) = s.tmp.clone() else {
            return Err(io::Error::other("no staged file"));
        };
        let _guard = self.stripe_lock(&s.dest);
        let old_len = fs::metadata(&s.dest).map(|m| m.len()).ok();
        let moved = (|| -> io::Result<()> {
            if let Some(parent) = s.dest.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::rename(&tmp, &s.dest)
        })();
        if let Err(e) = moved {
            Self::discard_tmp(s);
            return Err(e);
        }
        s.tmp = None;
        if let Some(old) = old_len {
            self.sub_usage(s.usage_idx, old);
        }
        self.add_usage(s.usage_idx, s.template.len);
        if let Some(other) = s.other_variant.take() {
            // Exactly one of `<key>` and `<key>.gz` exists.
            if let Ok(md) = fs::metadata(&other)
                && fs::remove_file(&other).is_ok()
            {
                self.sub_usage(s.usage_idx, md.len());
            }
        }
        Ok(())
    }

    fn commit_sync(&self, mut s: Staged) -> CachedFile {
        let res = match s.tmp.clone() {
            None => Err(io::Error::other("temp file was not written")),
            Some(tmp) => match fs::File::open(&tmp).and_then(|f| f.sync_data()) {
                Ok(()) => self.rename_into_place(&mut s),
                Err(e) => {
                    Self::discard_tmp(&mut s);
                    Err(e)
                }
            },
        };
        let mut cf = s.template.clone();
        if let Err(e) = res {
            if !s.stage_failed {
                self.note_write_error(&s.dest.display().to_string(), &e);
            }
            cf.mem = s.bytes.take();
        }
        cf
    }

    /// Commits `staged` in order and returns one [`CachedFile`] per entry. With [`Durability::Sync`]
    /// the files are at their final path when this returns; a failed write yields a memory-backed
    /// file instead of an error. [`Durability::Deferred`] queues the whole group (see
    /// [`Self::flush_pending`]); order inside a group is preserved, so a job's marker is renamed after
    /// its artifacts.
    pub fn commit_all(&self, staged: Vec<Staged>, mode: Durability) -> Vec<CachedFile> {
        match mode {
            Durability::Sync => staged.into_iter().map(|s| self.commit_sync(s)).collect(),
            Durability::Deferred => {
                let out: Vec<CachedFile> = staged.iter().map(|s| s.template.clone()).collect();
                let bytes: u64 = staged.iter().map(|s| s.template.len).sum();
                self.pending_bytes.fetch_add(bytes, Ordering::Relaxed);
                let now = self.clock.now_ms().saturating_add(1);
                let (len, since) = {
                    let mut p = lock(&self.pending);
                    for mut s in staged {
                        s.bytes = None; // the temp file holds the bytes now
                        p.push(s);
                    }
                    // The counters change under the queue lock: `pending_len == pending.len()`
                    // whenever the lock is free, so a lookup can trust `flush_in_flight()`.
                    self.pending_len.store(p.len(), Ordering::SeqCst);
                    let since = self.pending_since_ms.load(Ordering::Relaxed);
                    if since == 0 {
                        self.pending_since_ms.store(now, Ordering::Relaxed);
                    }
                    (p.len(), since)
                };
                if len >= PREWARM_BATCH_FILES
                    || (since != 0 && now.saturating_sub(since) >= PREWARM_BATCH_MS)
                {
                    self.flush_pending();
                }
                out
            }
        }
    }

    /// Syncs and renames every deferred file (one pass, then the renames). Called when a batch is
    /// full, when it is older than 1 s, at the end of a wave, before `prepare` returns and on drop.
    pub fn flush_pending(&self) {
        let _serial = lock(&self.flush_lock);
        self.flush_active.store(true, Ordering::SeqCst);
        let batch = {
            let mut p = lock(&self.pending);
            self.pending_len.store(0, Ordering::SeqCst);
            self.pending_since_ms.store(0, Ordering::Relaxed);
            std::mem::take(&mut *p)
        };
        self.flush_batch(batch);
        self.flush_active.store(false, Ordering::SeqCst);
    }

    /// `true` while deferred files are queued or being made visible: a lookup that missed must
    /// [`Self::flush_pending`] (which waits for a running flush) and look again.
    fn flush_in_flight(&self) -> bool {
        self.pending_len.load(Ordering::SeqCst) > 0 || self.flush_active.load(Ordering::SeqCst)
    }

    fn flush_batch(&self, batch: Vec<Staged>) {
        if batch.is_empty() {
            return;
        }
        // One pass of syncs first, then the renames, so a crash never exposes an empty file.
        let synced: Vec<bool> = batch
            .iter()
            .map(|s| {
                s.tmp
                    .as_ref()
                    .is_some_and(|t| fs::File::open(t).and_then(|f| f.sync_data()).is_ok())
            })
            .collect();
        for (mut s, ok) in batch.into_iter().zip(synced) {
            let res = if ok {
                self.rename_into_place(&mut s)
            } else {
                Err(io::Error::other("sync failed"))
            };
            // The bytes are now either in `usage` (renamed) or gone (failed).
            let len = s.template.len;
            let _ = self
                .pending_bytes
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
                    Some(v.saturating_sub(len))
                });
            if let Err(e) = res {
                self.note_write_error(&s.dest.display().to_string(), &e);
                Self::discard_tmp(&mut s);
            }
        }
    }

    /// Number of files waiting for [`Self::flush_pending`].
    pub fn pending_files(&self) -> usize {
        self.pending_len.load(Ordering::Relaxed)
    }

    /// Writes `bytes` atomically (temp file, optional `sync_data`, rename) to the cache-relative path
    /// `rel` (`index/dds-scan.<fp>.json`, `prepare-report.json`). Unlike artifact writes this reports
    /// failures.
    pub fn write_atomic(&self, rel: &str, bytes: &[u8], durable: bool) -> Result<(), AssetError> {
        validate_key(rel)?;
        let dest = self.root.join(rel);
        let tmp = self.stage_bytes(bytes).map_err(|e| {
            self.write_errors.fetch_add(1, Ordering::Relaxed);
            AssetError::io(rel, &e)
        })?;
        let fin = || -> io::Result<()> {
            if durable {
                fs::File::open(&tmp)?.sync_data()?;
            }
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            let _guard = self.stripe_lock(&dest);
            let old = fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
            fs::rename(&tmp, &dest)?;
            self.sub_usage(AssetKind::COUNT, old);
            self.add_usage(AssetKind::COUNT, bytes.len() as u64);
            Ok(())
        };
        fin().map_err(|e| {
            let _ = fs::remove_file(&tmp);
            self.write_errors.fetch_add(1, Ordering::Relaxed);
            AssetError::io(rel, &e)
        })
    }

    // ---- lookups --------------------------------------------------------------------------------

    fn stat_artifact(
        &self,
        rel: &str,
        kind: AssetKind,
        partfp: Fp,
        content_type: &'static str,
    ) -> Option<CachedFile> {
        let one = |rel: String, encoding: StoredEncoding| -> Option<CachedFile> {
            let path = self.root.join(&rel);
            let md = fs::metadata(&path).ok().filter(fs::Metadata::is_file)?;
            let len = md.len();
            let raw_len = match encoding {
                StoredEncoding::Identity => len,
                StoredEncoding::Gzip => gz_trailer_len(&path, len),
            };
            Some(CachedFile {
                path,
                len,
                raw_len,
                encoding,
                fp: partfp,
                content_type,
                mem: None,
            })
        };
        if kind.layout() == Layout::ResGz {
            let enc = if rel.ends_with(".gz") {
                StoredEncoding::Gzip
            } else {
                StoredEncoding::Identity
            };
            return one(rel.to_owned(), enc);
        }
        one(format!("{rel}.gz"), StoredEncoding::Gzip)
            .or_else(|| one(rel.to_owned(), StoredEncoding::Identity))
    }

    fn remember(&self, rel: String, cf: &CachedFile) {
        let mut map = lock(&self.positive);
        if map.len() >= POSITIVE_CAP {
            map.clear();
        }
        map.insert(rel, cf.clone());
    }

    /// Looks `key` up with at most two `stat` calls (`<key>.gz`, then `<key>`), through the positive
    /// cache. Flushes deferred writes on a miss so a just-built artifact is found.
    pub fn lookup(
        &self,
        kind: AssetKind,
        partfp: Fp,
        key: &str,
        content_type: &'static str,
    ) -> Result<Option<CachedFile>, AssetError> {
        let rel = artifact_rel(kind, partfp, key)?;
        if let Some(cf) = lock(&self.positive).get(&rel) {
            return Ok(Some(cf.clone()));
        }
        let mut found = self.stat_artifact(&rel, kind, partfp, content_type);
        if found.is_none() && self.flush_in_flight() {
            self.flush_pending();
            found = self.stat_artifact(&rel, kind, partfp, content_type);
        }
        if let Some(cf) = &found {
            self.remember(rel, cf);
        }
        Ok(found)
    }

    /// The state of the gzip variant of the raw file `path` whose `file_fp` is `fp`.
    pub fn lookup_res_gz(&self, path: &str, fp: Fp, content_type: &'static str) -> GzLookup {
        let gz_rel = format!("res-gz/{path}.{fp}.gz");
        let no_rel = format!("res-gz/{path}.{fp}.nogz");
        {
            let map = lock(&self.positive);
            if let Some(cf) = map.get(&gz_rel) {
                return GzLookup::Present(cf.clone());
            }
            if map.contains_key(&no_rel) {
                return GzLookup::NotBeneficial;
            }
        }
        let probe = |rel: &str| self.stat_artifact(rel, AssetKind::Gzip, fp, content_type);
        let mut present = probe(&gz_rel);
        let mut marker = if present.is_none() {
            probe(&no_rel)
        } else {
            None
        };
        if present.is_none() && marker.is_none() && self.flush_in_flight() {
            self.flush_pending();
            present = probe(&gz_rel);
            marker = if present.is_none() {
                probe(&no_rel)
            } else {
                None
            };
        }
        if let Some(cf) = present {
            self.remember(gz_rel, &cf);
            return GzLookup::Present(cf);
        }
        if let Some(cf) = marker {
            self.remember(no_rel, &cf);
            return GzLookup::NotBeneficial;
        }
        GzLookup::Unknown
    }

    /// Is the job marker present (the job finished for this partition fingerprint)?
    pub fn job_done(&self, kind: AssetKind, partfp: Fp, name: &str) -> bool {
        let Ok(rel) = marker_rel(kind, partfp, name) else {
            return false;
        };
        if self.root.join(&rel).is_file() {
            return true;
        }
        if self.flush_in_flight() {
            self.flush_pending();
            return self.root.join(&rel).is_file();
        }
        false
    }

    /// Number of job markers of one partition.
    pub fn count_markers(&self, kind: AssetKind, partfp: Fp) -> u64 {
        count_tree(&self.root.join(format!("jobs/{}/{partfp}", kind.slug()))).0
    }

    /// Number of `.gz` and `.nogz` files under `res-gz/` whose fingerprint passes `current`.
    pub fn count_res_gz(&self, current: &dyn Fn(&str, Fp) -> bool) -> (u64, u64) {
        let mut gz = 0;
        let mut nogz = 0;
        let base = self.root.join("res-gz");
        walk_files(&base, &mut |path| {
            let Ok(rel) = path.strip_prefix(&base) else {
                return;
            };
            let rel = rel.to_string_lossy();
            if let Some((p, fp, is_gz)) = parse_res_gz(&rel)
                && current(p, fp)
            {
                if is_gz {
                    gz += 1;
                } else {
                    nogz += 1;
                }
            }
        });
        (gz, nogz)
    }

    /// Forgets every positive lookup (after a sweep or a forced removal).
    pub fn clear_positive(&self) {
        lock(&self.positive).clear();
    }

    // ---- removal and sweep ----------------------------------------------------------------------

    /// Removes `path` by renaming it into `tmp/` first (so lookups never see a half-deleted
    /// directory) and then deleting it. Returns the files and bytes removed.
    fn delete_via_tmp(&self, path: &Path) -> (u64, u64) {
        let (files, bytes) = count_tree(path);
        let parked = self.tmp_path("del");
        match fs::rename(path, &parked) {
            Ok(()) => remove_any(&parked),
            Err(_) => remove_any(path),
        }
        (files.max(1), bytes)
    }

    /// Deletes the artifacts and markers of one partition (`flyff prepare --force`). For kinds
    /// outside the `gen/` layout only the markers go (the files are rewritten atomically); for `gz`
    /// the whole `res-gz/` tree goes.
    pub fn remove_partition(&self, kind: AssetKind, partfp: Fp) {
        let slug = kind.slug();
        match kind.layout() {
            Layout::Gen => {
                self.drop_dir(&format!("gen/{slug}/{partfp}"), kind.index());
            }
            Layout::Data | Layout::Server => {}
            Layout::ResGz => {
                self.drop_dir("res-gz", kind.index());
                let _ = fs::create_dir_all(self.root.join("res-gz"));
            }
        }
        self.drop_dir(&format!("jobs/{slug}/{partfp}"), AssetKind::COUNT);
        self.clear_positive();
    }

    fn drop_dir(&self, rel: &str, usage_idx: usize) {
        let p = self.root.join(rel);
        if p.exists() {
            let (_, bytes) = self.delete_via_tmp(&p);
            self.sub_usage(usage_idx, bytes);
        }
    }

    /// One bounded batch of the stale sweep (Plan/24 §8.5): deletes at most about `max_deletions`
    /// files that are not current according to `keep`, never touching `data/` or foreign files.
    pub fn sweep_step(&self, keep: &SweepKeep<'_>, max_deletions: u64) -> SweepStats {
        let mut st = SweepStats::default();
        let limit_hit = |st: &SweepStats| st.deleted_files >= max_deletions;

        // gen/<slug>/<partfp> and jobs/<slug>/<partfp>
        for (top, idx_of) in [("gen", true), ("jobs", false)] {
            let Ok(rd) = fs::read_dir(self.root.join(top)) else {
                continue;
            };
            let mut slugs: Vec<_> = rd.flatten().collect();
            slugs.sort_by_key(fs::DirEntry::file_name);
            for ent in slugs {
                if limit_hit(&st) {
                    return st;
                }
                let name = ent.file_name().to_string_lossy().into_owned();
                let path = ent.path();
                let usage_idx = |kind: Option<AssetKind>| {
                    if idx_of {
                        kind.map_or(AssetKind::COUNT, AssetKind::index)
                    } else {
                        AssetKind::COUNT
                    }
                };
                match AssetKind::from_slug(&name) {
                    None => {
                        let (f, b) = self.delete_via_tmp(&path);
                        self.sub_usage(usage_idx(None), b);
                        st.deleted_files += f;
                        st.deleted_bytes += b;
                    }
                    Some(kind) => {
                        let Some(current) = keep.partitions.get(&kind) else {
                            continue;
                        };
                        let Ok(parts) = fs::read_dir(&path) else {
                            continue;
                        };
                        let mut parts: Vec<_> = parts.flatten().collect();
                        parts.sort_by_key(fs::DirEntry::file_name);
                        for part in parts {
                            if limit_hit(&st) {
                                return st;
                            }
                            let pname = part.file_name().to_string_lossy().into_owned();
                            let is_current =
                                pname.parse::<Fp>().is_ok_and(|fp| current.contains(&fp));
                            if !is_current {
                                let (f, b) = self.delete_via_tmp(&part.path());
                                self.sub_usage(usage_idx(Some(kind)), b);
                                st.deleted_files += f;
                                st.deleted_bytes += b;
                            }
                        }
                    }
                }
            }
        }

        // res-gz
        if let Some(current) = keep.gz_current {
            let base = self.root.join("res-gz");
            let mut stale: Vec<PathBuf> = Vec::new();
            walk_files(&base, &mut |path| {
                if stale.len() as u64 + st.deleted_files >= max_deletions {
                    return;
                }
                let Ok(rel) = path.strip_prefix(&base) else {
                    return;
                };
                let keep_it =
                    parse_res_gz(&rel.to_string_lossy()).is_some_and(|(p, fp, _)| current(p, fp));
                if !keep_it {
                    stale.push(path.to_path_buf());
                }
            });
            for path in stale {
                let (f, b) = self.delete_via_tmp(&path);
                self.sub_usage(AssetKind::Gzip.index(), b);
                st.deleted_files += f;
                st.deleted_bytes += b;
            }
            prune_empty_dirs(&base);
            if limit_hit(&st) {
                self.clear_positive();
                return st;
            }
        }

        // server/ and index/: files whose names embed a fingerprint that is no longer current.
        for (top, idx) in [
            ("server", AssetKind::BossRigs.index()),
            ("index", AssetKind::COUNT),
        ] {
            let Ok(rd) = fs::read_dir(self.root.join(top)) else {
                continue;
            };
            let mut files: Vec<_> = rd.flatten().collect();
            files.sort_by_key(fs::DirEntry::file_name);
            for ent in files {
                if limit_hit(&st) {
                    self.clear_positive();
                    return st;
                }
                let name = ent.file_name().to_string_lossy().into_owned();
                if embedded_fp(&name).is_some_and(|fp| !keep.keep_fps.contains(&fp)) {
                    let (f, b) = self.delete_via_tmp(&ent.path());
                    self.sub_usage(idx, b);
                    st.deleted_files += f;
                    st.deleted_bytes += b;
                }
            }
        }

        if st.deleted_files > 0 {
            self.clear_positive();
        }
        st.done = true;
        st
    }
}

impl Drop for Cache {
    fn drop(&mut self) {
        self.flush_pending();
    }
}

fn wipe_known(root: &Path) {
    for name in KNOWN {
        if *name != "CACHEDIR.TAG" {
            remove_any(&root.join(name));
        }
    }
}

/// Calls `f` for every regular file below `dir`, in sorted order.
fn walk_files(dir: &Path, f: &mut dyn FnMut(&Path)) {
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(fs::DirEntry::file_name);
    for ent in entries {
        let path = ent.path();
        match ent.file_type() {
            Ok(t) if t.is_dir() => walk_files(&path, f),
            Ok(_) => f(&path),
            Err(_) => {}
        }
    }
}

fn prune_empty_dirs(dir: &Path) -> bool {
    let Ok(rd) = fs::read_dir(dir) else {
        return false;
    };
    let mut empty = true;
    for ent in rd.flatten() {
        let path = ent.path();
        let is_dir = ent.file_type().is_ok_and(|t| t.is_dir());
        if is_dir && prune_empty_dirs(&path) {
            let _ = fs::remove_dir(&path);
            continue;
        }
        empty = false;
    }
    empty
}

/// Splits `<vfs path>.<fp>.gz|nogz` (cache-relative below `res-gz/`) into `(path, fp, is_gz)`.
pub fn parse_res_gz(rel: &str) -> Option<(&str, Fp, bool)> {
    let (rest, is_gz) = if let Some(r) = rel.strip_suffix(".nogz") {
        (r, false)
    } else {
        (rel.strip_suffix(".gz")?, true)
    };
    let (path, fp) = rest.rsplit_once('.')?;
    let fp = fp.parse::<Fp>().ok()?;
    (!path.is_empty()).then_some((path, fp, is_gz))
}
