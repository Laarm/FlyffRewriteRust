//! The boss rig index (kind `boss-rigs`, server only; Plan/24 §10.9, WP-WEB-18; Plan/31 §13.5).
//!
//! The boss AIs (ClockWorks, BigMuscle, Krrr, Bear, Meteonyker) need bone and event positions at arbitrary motion
//! frames (the ClockWorks laser every frame; ref/12 §10.3, ref/18 §7.7). The server evaluates them with
//! `flyff_sim::pose::event_point` / `bone_matrix` on [`SimSkeleton`] / [`SimClip`] values built from the `.chr` /
//! `.ani` files. This kind stores **no keyframes and no pose math**: `server/boss-rigs.<fp>.json` is only the index
//! that tells the server which files to load, and [`BossRigs::materialise`] builds the `SimSkeleton` / `SimClip`s
//! the index names at start-up (`ServerDerived.rigs`, no file I/O on the game thread afterwards).
//!
//! # Rules
//!
//! * **Bosses** are the `GameData.movers` whose `ai` is one of [`BOSS_AIS`] (corpus: `MI_CLOCKWORK1`,
//!   `MI_LOADCLOCKWORK`, `MI_EVENT04`, `MI_BIGMUSCLE`, `MI_KRRR`, `MI_MUSHMOOT`, `MI_DU_METEONYKER2/3/4`). Every one
//!   gets an entry, also when its model is missing or broken: the entry is kept and the problem is reported, so that
//!   doc 18 can log at use instead of finding no row.
//! * **`chr`** is the lower-case VFS path of `ModelRegistry::bone_file(OT_MOVER, id)`; `bone_count` and `events` (the
//!   event slots with a parent bone >= 0, ascending) come from the parsed `.chr`.
//! * **`clips`** are the motion ids whose `MotionTable::resolve` (Plan/24 §10.5) yields a clip, mapped to that clip's
//!   lower-case `.ani` path: every listed slot whose file exists and parses, plus, for every id in `0..motion_max`
//!   without a listed slot, the clip of the `MakeMotionName` EMPTY-slot quirk (`mvr_<name>_<STAND's text>.ani`) when
//!   that file exists. A listed slot whose file is missing or unparsable is reported and absent (`Slot::Missing`:
//!   `resolve` gives no clip, the hits of that motion never fire, KQ-02). An id at or above `motion_max` resolves
//!   like `MTI_STAND` ([`BossRigs::clip_path`] applies that rule). `tests/rigs.rs` pins the equality with the real
//!   motion table.
//! * Every referenced `.chr` / `.ani` is parsed once with the key arrays skipped (doc 30); parse failures and
//!   bone-count mismatches are reported, never fatal.

use crate::cache::Cache;
use crate::derived::ServerDerived;
use crate::error::AssetError;
use crate::fp::{Fp, FpCtx};
use crate::kinds::{
    AssetBuilder, AssetKind, BuildCtx, BuiltFile, Compress, JobKey, KeyRoute, NO_PARTITION,
};
use crate::prep::AssetPrep;
use crate::util::lock;
use flyff_common::defs::define::OT_MOVER;
use flyff_common::defs::neuz::{
    AII_BEAR, AII_BIGMUSCLE, AII_CLOCKWORKS, AII_KRRR, AII_METEONYKER, MTI_STAND,
};
use flyff_data::GameData;
use flyff_data::models::type_root;
use flyff_formats::ani::{self, AniOptions, KeyMode};
use flyff_formats::chr;
use flyff_sim::pose::{SimClip, SimSkeleton};
use flyff_vfs::Vfs;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

/// Bump on any change that alters the produced bytes (boss selection, clip rules, JSON layout).
pub const BUILDER_VERSION: u32 = 1;
/// The version of the JSON layout (`"version"` of the file).
pub const FORMAT_VERSION: u32 = 1;
/// The AI kinds that need a rig (`AII_CLOCKWORKS`, `AII_BIGMUSCLE`, `AII_KRRR`, `AII_BEAR`, `AII_METEONYKER`).
pub const BOSS_AIS: [u32; 5] = [
    AII_CLOCKWORKS,
    AII_BIGMUSCLE,
    AII_KRRR,
    AII_BEAR,
    AII_METEONYKER,
];
/// The compression level of the stored artifact.
pub const GZIP_LEVEL: u32 = 9;

/// The cache key of the index for the partition fingerprint `fp` (`boss-rigs.<fp>.json`).
pub fn index_key(fp: Fp) -> String {
    format!("boss-rigs.{fp}.json")
}

/// The cache key of the build report for `fp` (`boss-rigs.<fp>.report.json`).
pub fn report_key(fp: Fp) -> String {
    format!("boss-rigs.{fp}.report.json")
}

/// One boss: which files to load (Plan/24 §10.9).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BossRig {
    /// Lower-case VFS path of the skeleton (`ModelRegistry::bone_file`); empty when the model has none.
    pub chr: String,
    /// `.chr` bone count (validation; 31 §13.1 tolerates clip/skeleton mismatch); 0 when the `.chr` is missing.
    pub bone_count: u16,
    /// Event slots defined in the `.chr` (parent >= 0), ascending; at most 8 (`.chr` v >= 6).
    pub events: Vec<u8>,
    /// `m_nMax` of the model's motion list (0 = no list): ids at or above it resolve like `MTI_STAND`.
    #[serde(default)]
    pub motion_max: u32,
    /// Motion id (the offset `mti` of `mdlDyna.inc`) to lower-case `.ani` path, for every id that resolves to a clip.
    pub clips: BTreeMap<u32, String>,
}

/// The boss rig index plus, after [`BossRigs::materialise`], the pose data it names
/// (`ServerDerived.rigs: Option<Arc<BossRigs>>`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BossRigs {
    /// The index: mover id (`MI_*`) to rig.
    pub rigs: BTreeMap<u32, BossRig>,
    skeletons: BTreeMap<String, SimSkeleton>,
    clips: BTreeMap<String, SimClip>,
}

/// What [`BossRigs::materialise`] could not load (each entry is kept in the index).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MaterialiseReport {
    /// `(path, message)` of skeletons that failed to read or convert.
    pub skeletons_failed: Vec<(String, String)>,
    /// `(path, message)` of clips that failed to read or convert.
    pub clips_failed: Vec<(String, String)>,
}

#[derive(Serialize, Deserialize)]
struct IndexFile {
    version: u32,
    rigs: BTreeMap<u32, BossRig>,
}

impl BossRigs {
    /// An index without materialised pose data.
    pub fn from_index(rigs: BTreeMap<u32, BossRig>) -> BossRigs {
        BossRigs {
            rigs,
            skeletons: BTreeMap::new(),
            clips: BTreeMap::new(),
        }
    }

    /// The canonical JSON of the index (compact, ascending ids, no timestamps).
    pub fn to_json(&self) -> Result<Vec<u8>, AssetError> {
        serde_json::to_vec(&IndexFile {
            version: FORMAT_VERSION,
            rigs: self.rigs.clone(),
        })
        .map_err(|e| AssetError::build(format!("boss-rigs: {e}")))
    }

    /// Parses [`BossRigs::to_json`] output (an index without pose data).
    pub fn from_json(bytes: &[u8]) -> Result<BossRigs, AssetError> {
        let f: IndexFile = serde_json::from_slice(bytes)
            .map_err(|e| AssetError::build(format!("boss-rigs: bad index: {e}")))?;
        if f.version != FORMAT_VERSION {
            return Err(AssetError::build(format!(
                "boss-rigs: index version {} (expected {FORMAT_VERSION})",
                f.version
            )));
        }
        Ok(BossRigs::from_index(f.rigs))
    }

    /// The rig of mover `id` (`MI_*`).
    pub fn rig(&self, id: u32) -> Option<&BossRig> {
        self.rigs.get(&id)
    }

    /// The `.ani` path `MotionTable::resolve` would pick for motion id `mti` of boss `id`: the listed (or
    /// empty-slot) clip, `MTI_STAND`'s for an id at or above `motion_max` or `u32::MAX`; `None` for a missing
    /// clip and for an unknown boss.
    pub fn clip_path(&self, id: u32, mti: u32) -> Option<&str> {
        let rig = self.rigs.get(&id)?;
        let m = if mti == u32::MAX || mti >= rig.motion_max {
            MTI_STAND
        } else {
            mti
        };
        rig.clips.get(&m).map(String::as_str)
    }

    /// The skeleton of boss `id` (after [`materialise`](Self::materialise); `None` when its `.chr` is missing or
    /// failed to load).
    pub fn skeleton(&self, id: u32) -> Option<&SimSkeleton> {
        self.skeletons.get(&self.rigs.get(&id)?.chr)
    }

    /// The clip boss `id` plays for motion id `mti` ([`clip_path`](Self::clip_path) resolved to the loaded clip).
    pub fn clip(&self, id: u32, mti: u32) -> Option<&SimClip> {
        self.clips.get(self.clip_path(id, mti)?)
    }

    /// Number of materialised skeletons.
    pub fn skeleton_count(&self) -> usize {
        self.skeletons.len()
    }

    /// Number of materialised clips.
    pub fn clip_count(&self) -> usize {
        self.clips.len()
    }

    /// Approximate heap bytes of the pose data (start-up log line).
    pub fn heap_bytes(&self) -> usize {
        self.skeletons
            .values()
            .map(SimSkeleton::heap_bytes)
            .chain(self.clips.values().map(SimClip::heap_bytes))
            .sum()
    }

    /// Builds the `SimSkeleton` of every distinct `.chr` and the `SimClip` of every distinct `.ani` the index names,
    /// through `SimSkeleton::from_chr_bytes` / `SimClip::from_ani_bytes` (the same `FSK1` / `FMO1` conversion path as
    /// the browser, Plan/31 §13.1). A file that cannot be read or converted is left out and listed in the report;
    /// its index entry stays.
    pub fn materialise(
        &mut self,
        vfs: &Vfs,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<MaterialiseReport, AssetError> {
        let mut report = MaterialiseReport::default();
        let chrs: BTreeSet<&str> = self
            .rigs
            .values()
            .map(|r| r.chr.as_str())
            .filter(|p| !p.is_empty())
            .collect();
        let anis: BTreeSet<&str> = self
            .rigs
            .values()
            .flat_map(|r| r.clips.values().map(String::as_str))
            .collect();
        let mut skeletons = BTreeMap::new();
        let mut clips = BTreeMap::new();
        for p in chrs {
            if cancelled() {
                return Err(AssetError::shutting_down());
            }
            match vfs
                .read(p)
                .map_err(|e| e.to_string())
                .and_then(|b| SimSkeleton::from_chr_bytes(&b).map_err(|e| e.to_string()))
            {
                Ok(s) => {
                    skeletons.insert(p.to_owned(), s);
                }
                Err(e) => report.skeletons_failed.push((p.to_owned(), e)),
            }
        }
        for p in anis {
            if cancelled() {
                return Err(AssetError::shutting_down());
            }
            match vfs
                .read(p)
                .map_err(|e| e.to_string())
                .and_then(|b| SimClip::from_ani_bytes(&b).map_err(|e| e.to_string()))
            {
                Ok(c) => {
                    clips.insert(p.to_owned(), c);
                }
                Err(e) => report.clips_failed.push((p.to_owned(), e)),
            }
        }
        self.skeletons = skeletons;
        self.clips = clips;
        Ok(report)
    }
}

/// What the build saw (logged by the builder, asserted by the corpus test; `boss-rigs.<fp>.report.json`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RigReport {
    /// Movers with a boss AI.
    pub bosses: usize,
    /// Bosses without a `mdlDyna.inc` model entry or without a bone file name.
    pub no_model: Vec<u32>,
    /// Skeleton paths that do not exist.
    pub missing_chr: Vec<String>,
    /// `(path, message)` of skeletons that failed to parse.
    pub unparsable_chr: Vec<(String, String)>,
    /// Listed `.ani` paths that do not exist (their motion ids have no clip).
    pub missing_clips: Vec<String>,
    /// `(path, message)` of `.ani` files that failed to parse.
    pub unparsable_clips: Vec<(String, String)>,
    /// `(ani path, ani bones, chr bones)` where a clip disagrees with its skeleton (31 §13.1 tolerates it).
    pub bone_mismatch: Vec<(String, u32, u32)>,
    /// Bosses whose EMPTY-slot quirk file is absent (their unlisted motion ids have no clip).
    pub empty_slot_missing: Vec<u32>,
    /// Entries of all `clips` maps.
    pub clip_entries: usize,
    /// Distinct `.ani` files named.
    pub clip_files: usize,
}

/// The value of one build.
#[derive(Clone, Debug)]
pub struct RigBuild {
    /// The index (no pose data).
    pub rigs: BossRigs,
    /// What was skipped or noted.
    pub report: RigReport,
}

fn model_path(file: &str) -> Option<String> {
    Vfs::normalize(&format!("model/{file}"))
        .ok()
        .map(|p| p.as_str().to_owned())
}

/// What a parsed `.chr` contributes.
struct ChrInfo {
    bones: u32,
    events: Vec<u8>,
}

fn read_chr(vfs: &Vfs, path: &str) -> Result<ChrInfo, String> {
    let bytes = vfs.read(path).map_err(|e| e.to_string())?;
    let f = chr::parse(&bytes).map_err(|e| e.to_string())?.value;
    let events = f
        .events
        .iter()
        .enumerate()
        .filter(|(_, e)| e.parent >= 0)
        .filter_map(|(i, _)| u8::try_from(i).ok())
        .collect();
    Ok(ChrInfo {
        bones: u32::try_from(f.bones.len()).unwrap_or(u32::MAX),
        events,
    })
}

/// The `nBone` of a parsed `.ani` (key arrays skipped).
fn read_ani_bones(vfs: &Vfs, path: &str) -> Result<u32, String> {
    let bytes = vfs.read(path).map_err(|e| e.to_string())?;
    let f = ani::parse(
        &bytes,
        &AniOptions {
            keys: KeyMode::Skip,
        },
    )
    .map_err(|e| e.to_string())?
    .value;
    u32::try_from(f.bone_count).map_err(|_| "negative bone count".to_owned())
}

/// Builds the index from `data` (mover props and the model registry) and the files of `model/` in `vfs` (a pure
/// function of both). `cancelled` is polled between bosses; a cancelled build is `ShuttingDown`.
pub fn build_boss_rigs(
    vfs: &Vfs,
    data: &GameData,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<RigBuild, AssetError> {
    let models = &data.models;
    let mut report = RigReport::default();
    let mut rigs: BTreeMap<u32, BossRig> = BTreeMap::new();
    // `.ani` path -> bone count or the parse error, each file once
    let mut ani_cache: BTreeMap<String, Result<u32, String>> = BTreeMap::new();
    let mut missing_clips: BTreeSet<String> = BTreeSet::new();
    let mut mismatch: BTreeSet<(String, u32, u32)> = BTreeSet::new();
    let mut chr_seen: BTreeSet<String> = BTreeSet::new();

    let mut bosses: Vec<u32> = data
        .movers
        .iter()
        .filter(|(_, p)| BOSS_AIS.contains(&p.ai))
        .map(|(id, _)| id)
        .collect();
    bosses.sort_unstable();
    report.bosses = bosses.len();

    for id in bosses {
        if cancelled() {
            return Err(AssetError::shutting_down());
        }
        let mut rig = BossRig::default();
        let Some(elem) = models.get(OT_MOVER, id) else {
            report.no_model.push(id);
            rigs.insert(id, rig);
            continue;
        };

        // the skeleton
        let mut chr_bones = None;
        match models.bone_file(OT_MOVER, id).and_then(|f| model_path(&f)) {
            None => report.no_model.push(id),
            Some(path) => {
                rig.chr.clone_from(&path);
                if !vfs.exists(&path) {
                    if chr_seen.insert(path.clone()) {
                        report.missing_chr.push(path);
                    }
                } else {
                    match read_chr(vfs, &path) {
                        Ok(info) => {
                            rig.bone_count = u16::try_from(info.bones).unwrap_or(u16::MAX);
                            rig.events = info.events;
                            chr_bones = Some(info.bones);
                        }
                        Err(e) => {
                            if chr_seen.insert(path.clone()) {
                                report.unparsable_chr.push((path, e));
                            }
                        }
                    }
                }
            }
        }

        // the clips: listed slots, then the EMPTY-slot quirk for the unlisted ids below `motion_max`
        rig.motion_max = elem.motion_max;
        let mut usable = |path: &str, cache: &mut BTreeMap<String, Result<u32, String>>| -> bool {
            let r = cache.entry(path.to_owned()).or_insert_with(|| {
                if vfs.exists(path) {
                    read_ani_bones(vfs, path)
                } else {
                    Err(String::new())
                }
            });
            match r {
                Ok(b) => {
                    if let Some(c) = chr_bones
                        && *b != c
                    {
                        mismatch.insert((path.to_owned(), *b, c));
                    }
                    true
                }
                Err(e) if e.is_empty() => {
                    missing_clips.insert(path.to_owned());
                    false
                }
                Err(_) => false,
            }
        };
        if elem.motion_max > 0 {
            let mut listed: BTreeSet<u32> = BTreeSet::new();
            for (mti, _) in &elem.motions {
                listed.insert(*mti);
                let Some(path) = models
                    .motion_file(OT_MOVER, id, *mti)
                    .and_then(|f| model_path(&f))
                else {
                    continue;
                };
                if usable(&path, &mut ani_cache) {
                    rig.clips.insert(*mti, path);
                }
            }
            let root = type_root(OT_MOVER).unwrap_or("mvr");
            let stand = elem.motion(MTI_STAND).unwrap_or("");
            let quirk = model_path(&format!("{root}_{}_{stand}.ani", elem.name));
            let empty = quirk.filter(|p| {
                // the quirk file is not a reference of the data: absent is quiet, only counted
                if !vfs.exists(p) {
                    return false;
                }
                usable(p, &mut ani_cache)
            });
            match empty {
                Some(path) => {
                    for mti in 0..elem.motion_max {
                        if !listed.contains(&mti) {
                            rig.clips.insert(mti, path.clone());
                        }
                    }
                }
                None => report.empty_slot_missing.push(id),
            }
        }
        report.clip_entries += rig.clips.len();
        rigs.insert(id, rig);
    }

    report.clip_files = rigs
        .values()
        .flat_map(|r| r.clips.values())
        .collect::<BTreeSet<_>>()
        .len();
    report.missing_clips = missing_clips.into_iter().collect();
    for (path, r) in &ani_cache {
        if let Err(e) = r
            && !e.is_empty()
        {
            report.unparsable_clips.push((path.clone(), e.clone()));
        }
    }
    report.bone_mismatch = mismatch.into_iter().collect();
    Ok(RigBuild {
        rigs: BossRigs::from_index(rigs),
        report,
    })
}

/// The `report.json` bytes of a build (compact, no timings).
pub fn report_json(r: &RigReport) -> Vec<u8> {
    let list = |v: &[String]| -> String {
        let items: Vec<String> = v.iter().map(|s| format!("{s:?}")).collect();
        format!("[{}]", items.join(","))
    };
    let pairs = |v: &[(String, String)]| -> String {
        let items: Vec<String> = v.iter().map(|(a, b)| format!("[{a:?},{b:?}]")).collect();
        format!("[{}]", items.join(","))
    };
    let ids = |v: &[u32]| -> String {
        let items: Vec<String> = v.iter().map(u32::to_string).collect();
        format!("[{}]", items.join(","))
    };
    let mismatch: Vec<String> = r
        .bone_mismatch
        .iter()
        .map(|(p, a, c)| format!("[{p:?},{a},{c}]"))
        .collect();
    format!(
        "{{\"bosses\":{},\"no_model\":{},\"missing_chr\":{},\"unparsable_chr\":{},\"missing_clips\":{},\
\"unparsable_clips\":{},\"bone_mismatch\":[{}],\"empty_slot_missing\":{},\"clip_entries\":{},\"clip_files\":{}}}",
        r.bosses,
        ids(&r.no_model),
        list(&r.missing_chr),
        pairs(&r.unparsable_chr),
        list(&r.missing_clips),
        pairs(&r.unparsable_clips),
        mismatch.join(","),
        ids(&r.empty_slot_missing),
        r.clip_entries,
        r.clip_files,
    )
    .into_bytes()
}

// ---------------------------------------------------------------------------------------------------
// the builder
// ---------------------------------------------------------------------------------------------------

/// Fingerprint inputs shared by every call, memoised per VFS (a `set_fp` of `model/` costs ~0.5 s).
#[derive(Default)]
struct FpCache {
    vfs: (usize, usize),
    model: Option<Fp>,
    registry: Option<Fp>,
}

/// The builder of kind [`AssetKind::BossRigs`]: one job, no partitions, artifacts under `server/`.
#[derive(Default)]
pub struct BossRigsBuilder {
    cache: Mutex<FpCache>,
}

impl BossRigsBuilder {
    /// A builder.
    pub fn new() -> BossRigsBuilder {
        BossRigsBuilder::default()
    }
}

impl AssetBuilder for BossRigsBuilder {
    fn kind(&self) -> AssetKind {
        AssetKind::BossRigs
    }

    fn builder_version(&self) -> u32 {
        BUILDER_VERSION
    }

    fn format_version(&self) -> u32 {
        FORMAT_VERSION
    }

    /// `boss-rigs.<fp>.json` or `boss-rigs.<fp>.report.json`.
    fn route(&self, _ctx: &FpCtx<'_>, key: &str) -> Result<KeyRoute, AssetError> {
        let ok = key
            .strip_prefix("boss-rigs.")
            .and_then(|r| r.strip_suffix(".json"))
            .is_some_and(|mid| {
                let fp = mid.strip_suffix(".report").unwrap_or(mid);
                fp.parse::<Fp>().is_ok()
            });
        if !ok {
            return Err(AssetError::bad_key(format!(
                "{key:?}: boss-rigs has the keys boss-rigs.<fp>.json and boss-rigs.<fp>.report.json"
            )));
        }
        Ok(KeyRoute {
            job: JobKey::single(AssetKind::BossRigs, ""),
            partition: NO_PARTITION.to_owned(),
        })
    }

    /// `set_fp("model", ["chr", "ani"])`, `registry_fp`, `GameData.meta.fingerprint` low 64 bits (Plan/24 §6.3).
    fn partition_fp(&self, ctx: &FpCtx<'_>, partition: &str) -> Option<Fp> {
        if partition != NO_PARTITION {
            return None;
        }
        let (model, registry) = {
            let mut c = lock(&self.cache);
            let id = (std::ptr::from_ref(ctx.vfs) as usize, ctx.vfs.len());
            if c.vfs != id {
                *c = FpCache {
                    vfs: id,
                    ..FpCache::default()
                };
            }
            let model = *c
                .model
                .get_or_insert_with(|| ctx.set_fp("model", &["chr", "ani"]));
            let registry = *c.registry.get_or_insert_with(|| ctx.registry_fp());
            (model, registry)
        };
        Some(ctx.kind_fp(
            AssetKind::BossRigs,
            BUILDER_VERSION,
            FORMAT_VERSION,
            &[model, registry, ctx.data_fp_low64()],
        ))
    }

    fn jobs(&self, _ctx: &FpCtx<'_>) -> Vec<JobKey> {
        vec![JobKey::single(AssetKind::BossRigs, "")]
    }

    fn build(&self, ctx: &BuildCtx<'_>, _job: &JobKey) -> Result<Vec<BuiltFile>, AssetError> {
        let fp = self
            .partition_fp(&ctx.fp(), NO_PARTITION)
            .ok_or_else(|| AssetError::build("boss-rigs: no fingerprint"))?;
        let built = build_boss_rigs(ctx.vfs, ctx.data, &|| ctx.cancelled())?;
        let r = &built.report;
        for id in &r.no_model {
            tracing::warn!(
                mover = id,
                "boss-rigs: the boss has no model entry or bone file"
            );
        }
        for p in &r.missing_chr {
            tracing::warn!(path = %p, "boss-rigs: the skeleton of a boss does not exist");
        }
        for (p, e) in &r.unparsable_chr {
            tracing::warn!(path = %p, error = %e, "boss-rigs: unparsable skeleton");
        }
        for p in &r.missing_clips {
            tracing::warn!(path = %p, "boss-rigs: a motion of a boss has no .ani, its hits never fire");
        }
        for (p, e) in &r.unparsable_clips {
            tracing::warn!(path = %p, error = %e, "boss-rigs: unparsable .ani");
        }
        for (p, a, c) in &r.bone_mismatch {
            tracing::warn!(path = %p, ani_bones = a, skeleton_bones = c, "boss-rigs: .ani bone count differs from the skeleton");
        }
        tracing::debug!(
            bosses = r.bosses,
            clip_entries = r.clip_entries,
            clip_files = r.clip_files,
            "boss rig index built"
        );
        let compress = Compress::IfBeneficial { level: GZIP_LEVEL };
        Ok(vec![
            BuiltFile {
                key: index_key(fp),
                bytes: built.rigs.to_json()?,
                content_type: "application/json",
                compress,
            },
            BuiltFile {
                key: report_key(fp),
                bytes: report_json(&built.report),
                content_type: "application/json",
                compress,
            },
        ])
    }

    fn content_type(&self, _key: &str) -> &'static str {
        "application/json"
    }

    fn on_start(&self, _ctx: &BuildCtx<'_>, _cache: &Cache) -> Result<(), AssetError> {
        Ok(())
    }

    /// Reads the index and materialises the `SimSkeleton` / `SimClip`s it names into `ServerDerived.rigs`
    /// (Plan/24 §10.9: at start-up, no lazy loading on the game thread).
    fn load_server(&self, prep: &AssetPrep, out: &mut ServerDerived) -> Result<(), AssetError> {
        let fp = prep
            .partition_fp(AssetKind::BossRigs, NO_PARTITION)
            .ok_or_else(|| AssetError::build("boss-rigs: no fingerprint"))?;
        let key = index_key(fp);
        let file = prep
            .lookup(AssetKind::BossRigs, &key)?
            .ok_or_else(|| AssetError::build(format!("boss-rigs/{key} is not in the cache")))?;
        let bytes = file
            .read_decoded()
            .map_err(|e| AssetError::build(format!("boss-rigs/{key}: {e}")))?;
        let mut rigs = BossRigs::from_json(&bytes)?;
        let m = rigs.materialise(prep.vfs(), &|| false)?;
        for (p, e) in &m.skeletons_failed {
            tracing::warn!(path = %p, error = %e, "boss-rigs: skeleton not loaded");
        }
        for (p, e) in &m.clips_failed {
            tracing::warn!(path = %p, error = %e, "boss-rigs: clip not loaded");
        }
        tracing::info!(
            bosses = rigs.rigs.len(),
            skeletons = rigs.skeleton_count(),
            clips = rigs.clip_count(),
            heap_kib = rigs.heap_bytes() / 1024,
            "boss rigs loaded"
        );
        out.rigs = Some(Arc::new(rigs));
        Ok(())
    }
}
