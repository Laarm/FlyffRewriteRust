//! Asset kinds and the builder contract (Plan/24 §7.1, §7.2).
//!
//! The contract in one paragraph: a builder is registered for exactly one [`AssetKind`]. It names its
//! work as [`JobKey`]s (`jobs()`), parses request keys into jobs (`route()`), knows the fingerprint of
//! the partition a job belongs to (`partition_fp()`), and turns a job into files (`build()`), a pure
//! deterministic function of VFS bytes and `GameData`. Everything else (queues, de-duplication,
//! compression, atomic writes, markers, progress, budget, reports) is the framework's job.

use crate::cache::Cache;
use crate::derived::ServerDerived;
use crate::error::AssetError;
use crate::fp::{FingerprintMode, Fp, FpCtx};
use crate::manifest::ManifestParts;
use crate::prep::AssetPrep;
use flyff_data::GameData;
use flyff_vfs::Vfs;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};

/// The partition name of kinds without partitions (Plan/24 §8.1): one partition directory, `_` in the
/// fingerprint input.
pub const NO_PARTITION: &str = "_";

/// Job priority; the smaller value is served first (Plan/24 §7.4, 01 §4.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Priority {
    /// Start-up builds (temporary full-core pool).
    Startup,
    /// A browser is waiting for the result.
    Interactive,
    /// Background prewarm.
    Prewarm,
}

impl Priority {
    /// `0`, `1`, `2` in priority order (queue index).
    pub const fn index(self) -> usize {
        match self {
            Priority::Startup => 0,
            Priority::Interactive => 1,
            Priority::Prewarm => 2,
        }
    }

    /// Lower-case name (`startup`, `interactive`, `prewarm`).
    pub const fn name(self) -> &'static str {
        match self {
            Priority::Startup => "startup",
            Priority::Interactive => "interactive",
            Priority::Prewarm => "prewarm",
        }
    }
}

/// Where the artifacts of a kind live inside `cache/` (Plan/24 §8.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// `gen/<slug>/<partition fp>/<key>[.gz]`.
    Gen,
    /// `data/<key>[.gz]`: the bundle exporter's content-hash names (owned by 14, never swept here).
    Data,
    /// `server/<key>[.gz]`: server-only artifacts whose names embed their fingerprint.
    Server,
    /// `res-gz/<key>`: the gzip variants of raw files (`<vfs path>.<file fp>.gz|nogz`).
    ResGz,
}

/// Kinds of derived assets (Plan/24 §7.1; Plan/01 §5.5 named the first eight).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AssetKind {
    /// Client data bundles (`data-bundles`, one job, builder registered by 14).
    DataBundle,
    /// Binary motion table (`motion-table`, writes the `motions` bundle projection too).
    MotionTable,
    /// Binary model bounds (`model-bounds`, writes the `model-bounds` bundle projection too).
    ModelBounds,
    /// Per-land collision chunks (`collision`, one job per world folder).
    CollisionChunk,
    /// Ctrl collision models (`ctrl-models`, M4).
    CtrlModels,
    /// Boss rig index (`boss-rigs`, server only).
    BossRigs,
    /// PNG conversions of UI / 3D TGA, BMP, DDS (`png`).
    UiPng,
    /// Cursor PNG + hotspot (`cursor`).
    CursorPng,
    /// Repaired DDS (`dds`).
    DdsFix,
    /// Pre-compressed gzip variant of a raw asset (`gz`).
    Gzip,
    /// Per-land pack (`pack-land`, M8).
    LandPack,
    /// Per-skeleton pack (`pack-skel`, M8).
    SkeletonPack,
    /// The overview pyramid and coverage document of a world's minimap for the admin console's map
    /// (`admin-map`, Plan/25 §11.7; built lazily, never prepared ahead).
    AdminMap,
}

impl AssetKind {
    /// Every kind, in the order reports and prepare use.
    pub const ALL: [AssetKind; 13] = [
        AssetKind::DataBundle,
        AssetKind::MotionTable,
        AssetKind::ModelBounds,
        AssetKind::CollisionChunk,
        AssetKind::CtrlModels,
        AssetKind::BossRigs,
        AssetKind::UiPng,
        AssetKind::CursorPng,
        AssetKind::DdsFix,
        AssetKind::Gzip,
        AssetKind::LandPack,
        AssetKind::SkeletonPack,
        AssetKind::AdminMap,
    ];

    /// Number of kinds (array sizes of counters).
    pub const COUNT: usize = Self::ALL.len();

    /// Position in [`Self::ALL`].
    pub const fn index(self) -> usize {
        match self {
            AssetKind::DataBundle => 0,
            AssetKind::MotionTable => 1,
            AssetKind::ModelBounds => 2,
            AssetKind::CollisionChunk => 3,
            AssetKind::CtrlModels => 4,
            AssetKind::BossRigs => 5,
            AssetKind::UiPng => 6,
            AssetKind::CursorPng => 7,
            AssetKind::DdsFix => 8,
            AssetKind::Gzip => 9,
            AssetKind::LandPack => 10,
            AssetKind::SkeletonPack => 11,
            AssetKind::AdminMap => 12,
        }
    }

    /// The URL / cache segment (`motion-table`, `png`, `gz`, ...).
    pub const fn slug(self) -> &'static str {
        match self {
            AssetKind::DataBundle => "data-bundles",
            AssetKind::MotionTable => "motion-table",
            AssetKind::ModelBounds => "model-bounds",
            AssetKind::CollisionChunk => "collision",
            AssetKind::CtrlModels => "ctrl-models",
            AssetKind::BossRigs => "boss-rigs",
            AssetKind::UiPng => "png",
            AssetKind::CursorPng => "cursor",
            AssetKind::DdsFix => "dds",
            AssetKind::Gzip => "gz",
            AssetKind::LandPack => "pack-land",
            AssetKind::SkeletonPack => "pack-skel",
            AssetKind::AdminMap => "admin-map",
        }
    }

    /// The kind with this slug.
    pub fn from_slug(slug: &str) -> Option<AssetKind> {
        Self::ALL.iter().copied().find(|k| k.slug() == slug)
    }

    /// Where artifacts of this kind are stored.
    pub const fn layout(self) -> Layout {
        match self {
            AssetKind::DataBundle => Layout::Data,
            AssetKind::BossRigs => Layout::Server,
            AssetKind::Gzip => Layout::ResGz,
            _ => Layout::Gen,
        }
    }

    /// `true` for kinds served at `/gen/<slug>/<fp>/<key>` (Plan/24 §9.1).
    pub const fn served(self) -> bool {
        matches!(
            self,
            AssetKind::MotionTable
                | AssetKind::ModelBounds
                | AssetKind::CollisionChunk
                | AssetKind::CtrlModels
                | AssetKind::UiPng
                | AssetKind::CursorPng
                | AssetKind::DdsFix
                | AssetKind::LandPack
                | AssetKind::SkeletonPack
                | AssetKind::AdminMap
        )
    }

    /// The start-up stage of kinds the server needs before the game thread starts (W7, §11.1 step 5:
    /// bundles, then motion table + bounds, then collision, then ctrl models + rigs); `None` for
    /// client-only kinds.
    pub const fn startup_stage(self) -> Option<u8> {
        match self {
            AssetKind::DataBundle => Some(0),
            AssetKind::MotionTable | AssetKind::ModelBounds => Some(1),
            AssetKind::CollisionChunk => Some(2),
            AssetKind::CtrlModels | AssetKind::BossRigs => Some(3),
            _ => None,
        }
    }

    /// `true` for kinds built at start-up.
    pub const fn startup(self) -> bool {
        self.startup_stage().is_some()
    }

    /// `true` for kinds the budget never refuses (Plan/24 §8.7); only `gz` and the packs are optional.
    pub const fn required(self) -> bool {
        !matches!(
            self,
            AssetKind::Gzip | AssetKind::LandPack | AssetKind::SkeletonPack
        )
    }
}

impl fmt::Display for AssetKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.slug())
    }
}

/// Which kinds are registered (milestone gating, Plan/24 §7.3 `AssetConfig.features`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssetFeatures(u16);

impl AssetFeatures {
    /// Every kind enabled.
    pub const fn all() -> Self {
        Self((1 << AssetKind::COUNT) - 1)
    }

    /// No kind enabled.
    pub const fn none() -> Self {
        Self(0)
    }

    /// `self` plus `kind`.
    #[must_use]
    pub const fn with(self, kind: AssetKind) -> Self {
        Self(self.0 | (1 << kind.index()))
    }

    /// `self` minus `kind`.
    #[must_use]
    pub const fn without(self, kind: AssetKind) -> Self {
        Self(self.0 & !(1 << kind.index()))
    }

    /// Is `kind` enabled?
    pub const fn contains(self, kind: AssetKind) -> bool {
        self.0 & (1 << kind.index()) != 0
    }
}

impl Default for AssetFeatures {
    fn default() -> Self {
        Self::all()
    }
}

/// One unit of work: `(kind, partition, name)`. Ordered and hashable so it can key a `DetMap`.
///
/// * `partition`: the fingerprint partition (a source directory for `png`/`dds`, a world folder for
///   `collision`, [`NO_PARTITION`] for single-partition kinds).
/// * `name`: the job's own name inside the kind (a source file path for `png`; may be empty when the
///   partition is the job, as for a collision world). It is also the marker name, so it must be a valid
///   relative path (no `..`, no empty segments).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JobKey {
    /// The kind.
    pub kind: AssetKind,
    /// The fingerprint partition.
    pub partition: String,
    /// The name inside the kind.
    pub name: String,
}

impl JobKey {
    /// A job key.
    pub fn new(kind: AssetKind, partition: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            kind,
            partition: partition.into(),
            name: name.into(),
        }
    }

    /// A job of a kind without partitions.
    pub fn single(kind: AssetKind, name: impl Into<String>) -> Self {
        Self::new(kind, NO_PARTITION, name)
    }
}

impl fmt::Display for JobKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.kind, self.partition)?;
        if !self.name.is_empty() {
            write!(f, "/{}", self.name)?;
        }
        Ok(())
    }
}

/// The result of `AssetBuilder::route`: the job that produces a key and the partition it belongs to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyRoute {
    /// The job whose output contains the key.
    pub job: JobKey,
    /// The partition (equal to `job.partition`).
    pub partition: String,
}

/// Storage rule of one built file (Plan/24 §8.2, W8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compress {
    /// gzip at `level`; store `<key>.gz` iff `gz_len < 0.9 × raw_len` and `raw_len >= 1024`, else the
    /// identity file. Level 9 is the default, 6 for start-up collision jobs.
    IfBeneficial {
        /// The deflate level (1-9).
        level: u32,
    },
    /// Store as is (already compressed formats such as PNG, or tiny files).
    Never,
}

impl Compress {
    /// Level 9, the default for derived artifacts.
    pub const DEFAULT: Compress = Compress::IfBeneficial { level: 9 };
}

/// One file produced by a job.
#[derive(Clone, Debug)]
pub struct BuiltFile {
    /// Key relative to the partition directory (`13-6.bin`, `theme/wndtile00.tga.png`, `all.bin`);
    /// `/` creates sub-directories. Must not end in `.gz`.
    pub key: String,
    /// The bytes (identity form).
    pub bytes: Vec<u8>,
    /// MIME type; must equal `AssetBuilder::content_type(key)`.
    pub content_type: &'static str,
    /// Storage rule.
    pub compress: Compress,
}

impl BuiltFile {
    /// A file with the default gzip rule.
    pub fn new(key: impl Into<String>, bytes: Vec<u8>, content_type: &'static str) -> Self {
        Self {
            key: key.into(),
            bytes,
            content_type,
            compress: Compress::DEFAULT,
        }
    }

    /// The same file stored as is.
    #[must_use]
    pub fn uncompressed(mut self) -> Self {
        self.compress = Compress::Never;
        self
    }
}

static NEVER_CANCELLED: AtomicBool = AtomicBool::new(false);

/// What a builder may touch while building (Plan/24 §7.2): the VFS (B1: read source bytes only through
/// it), the game data and the shared manifest parts. Long jobs poll [`BuildCtx::cancelled`] between
/// files so shutdown does not wait for them.
#[derive(Clone, Copy)]
pub struct BuildCtx<'a> {
    /// The resource index (`Vfs::read` / `open_file`, overrides apply).
    pub vfs: &'a Vfs,
    /// The loaded game data.
    pub data: &'a GameData,
    /// Parts of the client manifest that builders contribute (DDS fix list, cursor hotspots, worlds).
    pub manifest_parts: &'a ManifestParts,
    cancel: &'a AtomicBool,
    mode: FingerprintMode,
}

impl<'a> BuildCtx<'a> {
    /// The context `AssetPrep` passes to jobs.
    pub(crate) fn new(
        vfs: &'a Vfs,
        data: &'a GameData,
        manifest_parts: &'a ManifestParts,
        cancel: &'a AtomicBool,
        mode: FingerprintMode,
    ) -> Self {
        Self {
            vfs,
            data,
            manifest_parts,
            cancel,
            mode,
        }
    }

    /// A context that is never cancelled, for calling a builder directly (unit tests, tools, the
    /// `det_chunks` example).
    pub fn standalone(vfs: &'a Vfs, data: &'a GameData, manifest_parts: &'a ManifestParts) -> Self {
        Self::new(
            vfs,
            data,
            manifest_parts,
            &NEVER_CANCELLED,
            FingerprintMode::Stat,
        )
    }

    /// `true` once the pool is shutting down: stop between files and return an error.
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// The fingerprint helpers over the same VFS.
    pub fn fp(&self) -> FpCtx<'a> {
        FpCtx {
            vfs: self.vfs,
            data: self.data,
            mode: self.mode,
        }
    }
}

/// The builder contract (Plan/24 §7.2). Implementations are `Send + Sync` and stateless apart from
/// configuration: the pool calls `build` from several threads at once for different jobs.
///
/// * B1: read source bytes only through `Vfs::read`/`open_file`.
/// * B2: never panic on bad input; return `AssetError::build(..)` naming the file (the pool also wraps
///   every job in `catch_unwind`).
/// * B3: deterministic output: no timestamps, no hash-order iteration, gzip is the framework's job.
pub trait AssetBuilder: Send + Sync + 'static {
    /// The one kind this builder serves.
    fn kind(&self) -> AssetKind;

    /// Bump on any change that can alter output bytes for the same inputs (Plan/24 §6.3).
    fn builder_version(&self) -> u32;

    /// Bump when the byte layout of the artifacts changes.
    fn format_version(&self) -> u32 {
        0
    }

    /// Parses and validates a request key (§9.2 grammar) and names the job that produces it.
    /// `BadKey` for a grammar error (HTTP 400), `NotFound`/`NotConvertible` for a missing or unusable
    /// source (404). Must be cheap (stat data only): it runs on the caller's thread.
    fn route(&self, ctx: &FpCtx<'_>, key: &str) -> Result<KeyRoute, AssetError>;

    /// The current fingerprint of a partition (§6.3, normally [`FpCtx::kind_fp`] over the inputs);
    /// `None` = unknown partition (404). Cheap, stat data only.
    fn partition_fp(&self, ctx: &FpCtx<'_>, partition: &str) -> Option<Fp>;

    /// Every job, in the deterministic prewarm order of §7.5 (prepare and prewarm walk this list).
    fn jobs(&self, ctx: &FpCtx<'_>) -> Vec<JobKey>;

    /// Builds one job. Pure and deterministic; called on a pool thread.
    fn build(&self, ctx: &BuildCtx<'_>, job: &JobKey) -> Result<Vec<BuiltFile>, AssetError>;

    /// MIME type of a cached key (used by `lookup`, which does not build). Default: by extension.
    fn content_type(&self, key: &str) -> &'static str {
        content_type_by_ext(key)
    }

    /// Start-up hook, run on the calling thread by `AssetPrep::start` after the cache is open and
    /// before any job runs (§11.1 step 4: load or build `index/dds-scan.<fp>.json`, parse the cursor
    /// headers, fill `ctx.manifest_parts`). An error is fatal for required kinds and a warning otherwise.
    fn on_start(&self, _ctx: &BuildCtx<'_>, _cache: &Cache) -> Result<(), AssetError> {
        Ok(())
    }

    /// Server-side decode hook, run by `AssetPrep::load_server_derived` after all jobs of this kind
    /// are done: read the artifacts through `prep.lookup`/`prep.get` and fill `out` (for example
    /// `MotionTable::decode(all.bin)` into `out.motions`).
    fn load_server(&self, _prep: &AssetPrep, _out: &mut ServerDerived) -> Result<(), AssetError> {
        Ok(())
    }
}

/// MIME type by the extension of `name` (the keys of the derived kinds and the raw gz sources).
pub fn content_type_by_ext(name: &str) -> &'static str {
    let ext = crate::res::ext_of(name).unwrap_or("");
    match ext {
        "json" => "application/json",
        "png" => "image/png",
        "dds" => "image/vnd.ms-dds",
        "tga" => "image/x-tga",
        "bmp" => "image/bmp",
        "cur" => "image/x-icon",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "mp3" => "audio/mpeg",
        _ => "application/octet-stream",
    }
}

/// The prewarm steps after the sweep (Plan/24 §7.5; P1 is the sweep itself).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PrewarmStep {
    /// `png` for `theme/**` and the cursor PNGs.
    P2,
    /// `gz` of the `.lnd` files.
    P3,
    /// `dds` fixes of `model/texture`, `png` of `icon`, `item`, `char`.
    P4,
    /// `gz` of models, model textures, sfx, world/texture, weather.
    P5,
    /// `dds` fixes and `png` of the remaining 3D texture dirs.
    P6,
    /// `gz` of `sound/*.wav`.
    P7,
    /// `pack-land`, `pack-skel`.
    P8,
}

/// The prewarm step of a job, `None` for kinds that are not prewarmed (the start-up kinds).
pub fn prewarm_step(job: &JobKey) -> Option<PrewarmStep> {
    match job.kind {
        AssetKind::UiPng => Some(match job.partition.as_str() {
            "theme" => PrewarmStep::P2,
            "icon" | "item" | "char" => PrewarmStep::P4,
            _ => PrewarmStep::P6,
        }),
        AssetKind::CursorPng => Some(PrewarmStep::P2),
        AssetKind::DdsFix => Some(if job.partition == "model/texture" {
            PrewarmStep::P4
        } else {
            PrewarmStep::P6
        }),
        AssetKind::Gzip => crate::build::gz::prewarm_step(&job.name),
        AssetKind::LandPack | AssetKind::SkeletonPack => Some(PrewarmStep::P8),
        _ => None,
    }
}
