//! The motion table (`motion-table` kind, Plan/24 §10.5, WP-WEB-11): one `flyff_sim::MotionTable` value
//! built from the model registry (`GameData.models`) and the `.ani`/`.o3d` headers, written as `FLMT` v1
//! by the sim's own encoder.
//!
//! One job (no partition) builds the value and stores two artifacts under `gen/motion-table/<fp>/`:
//!
//! * `all.bin`: `MotionTable::encode()` (Plan/31 §12.1): what the server decodes into
//!   `ServerDerived.motions` and the browser passes to `SimWorld.load_motion_table`;
//! * `motions.json`: the envelope of the `motions` bundle (`flyff_data::bundle::types::MotionsBundle`),
//!   a projection of the same value with the same fingerprint, published to `cache/data/` by
//!   `load_server`.
//!
//! # What the table holds
//!
//! * **Clips** (`MotionTable.clips`, `ClipId` = index = row of the JSON `clips` table): every parsable
//!   `.ani` of `model/` (source 0; key arrays skipped) and the built-in animation of every registry model
//!   whose own `.o3d` has `maxFrame > 0` (source 1). `per_slerp_raw` is the **raw** file `f32`, copied
//!   bit-exactly: the sim applies `trunc3` exactly once (`trunc3` is not idempotent in `f32`). `attrs`
//!   keeps the `MOTION_ATTR` entries with `flags != 0` as `(array slot, flags, sound, exact frame)`.
//!   *Row order:* the `FLMT` decoder requires the clips sorted by unique path bytes, so source 0 and
//!   source 1 rows are interleaved in path order (Plan/24 §10.5 says "source 0 first"; that cannot hold
//!   with the decoder's invariant, and the row index is the `ClipId` either way).
//! * **Models** (`MotionTable.models`, `(ot, index)`): the `OT_MOVER` registry entries with a motion list
//!   (`max = m_nMax`, `slots` = every non-empty slot resolved with `MakeMotionName`, including borrowed
//!   clips `"BangBig_walk"`; a referenced `.ani` that does not exist or does not parse is `Slot::Missing`:
//!   the original's `LoadMotion` fails and leaves no clip), the wings (`OT_ITEM` entries of model type
//!   `ANIMATED_MESH` with their own `.chr`: synthetic slots `STAND -> item_<name>_stand.ani`,
//!   `WALK -> item_<name>_walk.ani`, `max = MTI_WALK + 1`, ref/18 §11.3) and every model whose `.o3d`
//!   has a built-in animation (`builtin`). `empty` is the clip an EMPTY slot resolves to: the file
//!   `mvr_<name>_<STAND's motion string>.ani` even when STAND's string contains `_` (KEEP-QUIRK,
//!   `MakeMotionName`, src:_Common/ModelMng.cpp:124-155).
//!
//! The weapon-stance offsets are code in `flyff_sim` (31 §12.3), not table data: the motion id passed to
//! `MotionTable::resolve` already includes them, and the table has every registered id because
//! `mdlDyna.inc` lists them.

use super::bounds::read_artifact;
use super::external;
use crate::error::AssetError;
use crate::fp::{Fp, FpCtx};
use crate::kinds::{AssetBuilder, AssetKind, BuildCtx, BuiltFile, JobKey, KeyRoute, NO_PARTITION};
use crate::prep::AssetPrep;
use crate::util::{job_threads, par_map};
use flyff_common::defs::define::{
    MODELTYPE_ANIMATED_MESH, MODELTYPE_BILLBOARD, MODELTYPE_SFX, OT_ITEM, OT_MOVER, OT_SFX,
};
use flyff_common::defs::neuz::{MTI_STAND, MTI_WALK};
use flyff_data::bundle::envelope::encode_envelope;
use flyff_data::bundle::types::{MotionClipRow, MotionModelRow, MotionsBundle};
use flyff_data::bundle::{BundleData, RowTable};
use flyff_data::models::{ModelRegistry, type_root};
use flyff_formats::ani::{self, AniOptions, KeyMode};
use flyff_formats::chr;
use flyff_formats::o3d::{self, AttrBlock, O3dOptions};
use flyff_sim::MotionTable;
use flyff_sim::anim::motion_table::{
    ClipId, ClipMeta, ClipSource, MOTION_TABLE_VERSION, ModelKey, ModelMotions, MotionAttr, Slot,
};
use flyff_vfs::Vfs;
use std::collections::{BTreeMap, BTreeSet};

/// Bump on any change that alters the produced bytes (row selection, resolution rules, projection).
pub const BUILDER_VERSION: u32 = 1;

/// The key of the binary table.
pub const KEY_BIN: &str = "all.bin";
/// The key of the JSON projection (the `motions` bundle envelope).
pub const KEY_JSON: &str = "motions.json";

/// What the build found (logged by the builder, asserted by the corpus test).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MotionReport {
    /// `.ani` files in `model/`.
    pub ani_files: usize,
    /// `.ani` files that became clip rows.
    pub ani_clips: usize,
    /// `.ani` files that did not parse, with the message (their references are `Missing`).
    pub unparsable: Vec<(String, String)>,
    /// `.o3d` built-in animation clips.
    pub builtin_clips: usize,
    /// `.o3d` files that did not parse (no built-in clip, no bounds).
    pub o3d_failed: Vec<(String, String)>,
    /// Model rows.
    pub models: usize,
    /// Wing models with synthetic slots.
    pub wings: usize,
    /// Non-empty slots over all models.
    pub slots: usize,
    /// Slot references of `OT_MOVER` registry entries whose clip is missing or unparsable.
    pub missing_refs: usize,
    /// The distinct VFS paths of those references, sorted.
    pub missing_files: Vec<String>,
    /// The synthetic wing clips that are missing (not part of the mover counts above), sorted.
    pub wing_missing: Vec<String>,
    /// Models whose EMPTY-slot quirk file exists / does not exist.
    pub empty_present: usize,
    /// See `empty_present`.
    pub empty_missing: usize,
    /// `(clip, clip bone count, skeleton bone count)` of every slot clip whose `.ani` bone count differs
    /// from the model's `.chr` (1 known: `mvr_infopeng_getup.ani`).
    pub bone_mismatch: Vec<(String, u32, u32)>,
}

/// The value of one build.
#[derive(Clone, Debug)]
pub struct MotionBuild {
    /// The table (`FLMT`).
    pub table: MotionTable,
    /// What was found.
    pub report: MotionReport,
}

/// Parses one `.ani` header: frame count, raw `perSlerp`, bone count and the flagged attributes.
fn read_ani(vfs: &Vfs, path: &str) -> Result<ClipMeta, String> {
    let bytes = vfs.read(path).map_err(|e| e.to_string())?;
    let f = ani::parse(
        &bytes,
        &AniOptions {
            keys: KeyMode::Skip,
        },
    )
    .map_err(|e| e.to_string())?
    .value;
    let frames = u32::try_from(f.frame_count).map_err(|_| "negative frame count".to_owned())?;
    let bones = u16::try_from(f.bone_count).map_err(|_| "bone count out of range".to_owned())?;
    if !f.per_slerp.is_finite() {
        return Err("non-finite perSlerp".to_owned());
    }
    let mut attrs = Vec::new();
    for (i, a) in f.attrs.iter().enumerate() {
        if a.flags != 0 {
            if !a.frame.is_finite() {
                return Err(format!("attribute {i} has a non-finite frame"));
            }
            attrs.push(MotionAttr {
                index: u32::try_from(i).map_err(|_| "attribute index".to_owned())?,
                flags: a.flags,
                sound: a.sound_id,
                frame: a.frame,
            });
        }
    }
    Ok(ClipMeta {
        file: path.to_owned(),
        source: ClipSource::Ani,
        frames,
        per_slerp_raw: f.per_slerp,
        bones,
        attrs,
    })
}

/// Parses one `.o3d`: `Some(clip)` when it has a built-in animation (`maxFrame > 0`).
fn read_builtin(vfs: &Vfs, path: &str) -> Result<Option<ClipMeta>, String> {
    let bytes = vfs.read(path).map_err(|e| e.to_string())?;
    let f = o3d::parse(&bytes, &O3dOptions::skip())
        .map_err(|e| e.to_string())?
        .value;
    let Ok(frames) = u32::try_from(f.max_frame) else {
        return Ok(None);
    };
    if frames == 0 {
        return Ok(None);
    }
    if !f.per_slerp.is_finite() {
        return Err("non-finite perSlerp".to_owned());
    }
    let mut attrs = Vec::new();
    if let AttrBlock::Attrs(list) = &f.attrs {
        for (i, a) in list.iter().enumerate() {
            if a.flags != 0 {
                if !a.frame.is_finite() {
                    return Err(format!("attribute {i} has a non-finite frame"));
                }
                attrs.push(MotionAttr {
                    index: u32::try_from(i).map_err(|_| "attribute index".to_owned())?,
                    flags: a.flags,
                    sound: a.sound_id,
                    frame: a.frame,
                });
            }
        }
    }
    Ok(Some(ClipMeta {
        file: path.to_owned(),
        source: ClipSource::O3dBuiltin,
        frames,
        per_slerp_raw: f.per_slerp,
        bones: 0,
        attrs,
    }))
}

/// The canonical VFS path of `model/<file>` for a file name built by the registry (mixed case).
fn model_path(file: &str) -> Option<String> {
    Vfs::normalize(&format!("model/{file}"))
        .ok()
        .map(|p| p.as_str().to_owned())
}

/// The `.o3d` a registry entry loads (`MakeModelName`), `None` for billboards, sfx and entries that do
/// not name an `.o3d`.
fn o3d_path_of(models: &ModelRegistry, ot: u32, index: u32) -> Option<String> {
    let e = models.get(ot, index)?;
    if ot == OT_SFX || e.model_type == MODELTYPE_SFX || e.model_type == MODELTYPE_BILLBOARD {
        return None;
    }
    let file = models.model_file(ot, index)?;
    file.to_ascii_lowercase()
        .ends_with(".o3d")
        .then(|| model_path(&file))
        .flatten()
}

/// Builds the motion table from `models` and the files of `model/` in `vfs` (a pure function of both).
/// `cancelled` is polled between files; a cancelled build is `ShuttingDown`.
pub fn build_motion_table(
    vfs: &Vfs,
    models: &ModelRegistry,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<MotionBuild, AssetError> {
    let mut report = MotionReport::default();

    // 1. Source 0: every .ani of model/.
    let mut ani_paths: Vec<&str> = Vec::new();
    let mut chr_paths: BTreeSet<&str> = BTreeSet::new();
    for e in vfs.list_dir("model") {
        let p = e.path.as_str();
        match crate::res::ext_of(p) {
            Some("ani") => ani_paths.push(p),
            Some("chr") => {
                chr_paths.insert(p);
            }
            _ => {}
        }
    }
    ani_paths.sort_unstable();
    report.ani_files = ani_paths.len();
    let parsed = par_map(&ani_paths, job_threads(), |p| {
        if cancelled() {
            Err("cancelled".to_owned())
        } else {
            read_ani(vfs, p)
        }
    });
    if cancelled() {
        return Err(AssetError::shutting_down());
    }
    let mut clips: Vec<ClipMeta> = Vec::with_capacity(parsed.len());
    for (p, r) in ani_paths.iter().zip(parsed) {
        match r {
            Ok(c) => clips.push(c),
            Err(e) => report.unparsable.push(((*p).to_owned(), e)),
        }
    }
    report.ani_clips = clips.len();

    // 2. Source 1: the built-in animation of the .o3d of every registry model.
    let mut o3d_of: BTreeMap<(u32, u32), String> = BTreeMap::new();
    for (ot, table) in &models.by_type {
        for (_, e) in table.iter() {
            if let Some(p) = o3d_path_of(models, *ot, e.index) {
                o3d_of.insert((*ot, e.index), p);
            }
        }
    }
    let unique_o3d: Vec<&str> = o3d_of
        .values()
        .map(String::as_str)
        .collect::<BTreeSet<&str>>()
        .into_iter()
        .filter(|p| vfs.exists(p))
        .collect();
    let built = par_map(&unique_o3d, job_threads(), |p| {
        if cancelled() {
            Err("cancelled".to_owned())
        } else {
            read_builtin(vfs, p)
        }
    });
    if cancelled() {
        return Err(AssetError::shutting_down());
    }
    for (p, r) in unique_o3d.iter().zip(built) {
        match r {
            Ok(Some(c)) => clips.push(c),
            Ok(None) => {}
            Err(e) => report.o3d_failed.push(((*p).to_owned(), e)),
        }
    }
    report.builtin_clips = clips.len() - report.ani_clips;

    // 3. ClipId = index in path-byte order (the decoder's invariant).
    clips.sort_by(|a, b| a.file.as_bytes().cmp(b.file.as_bytes()));
    let clip_id: BTreeMap<&str, ClipId> = clips
        .iter()
        .enumerate()
        .filter_map(|(i, c)| Some((c.file.as_str(), u32::try_from(i).ok()?)))
        .collect();

    // 4. The models.
    let mut skeleton_bones: BTreeMap<String, Option<u32>> = BTreeMap::new();
    let mut missing: BTreeSet<String> = BTreeSet::new();
    let mut wing_missing: BTreeSet<String> = BTreeSet::new();
    let mut rows: BTreeMap<ModelKey, ModelMotions> = BTreeMap::new();
    for (ot, table) in &models.by_type {
        for (_, e) in table.iter() {
            let key = ModelKey::new(*ot, e.index);
            let builtin = o3d_of
                .get(&(*ot, e.index))
                .and_then(|p| clip_id.get(p.as_str()))
                .copied();
            let mut mm = ModelMotions {
                max: 0,
                slots: BTreeMap::new(),
                empty: None,
                builtin,
            };
            let mut has_row = builtin.is_some();
            let root = type_root(*ot).unwrap_or("mvr");

            if *ot == OT_MOVER && e.motion_max > 0 {
                has_row = true;
                mm.max = e.motion_max;
                for (mti, _) in &e.motions {
                    let Some(file) = models.motion_file(*ot, e.index, *mti) else {
                        continue;
                    };
                    let path = model_path(&file);
                    let id = path.as_deref().and_then(|p| clip_id.get(p)).copied();
                    report.slots += 1;
                    mm.slots.insert(
                        *mti,
                        match id {
                            Some(c) => Slot::Clip(c),
                            None => {
                                report.missing_refs += 1;
                                missing.insert(path.unwrap_or(file));
                                Slot::Missing
                            }
                        },
                    );
                }
                // The EMPTY-slot quirk: prefix decided on the empty string (`name_` is added), text of STAND.
                let stand = e.motion(MTI_STAND).unwrap_or("");
                let quirk = format!("{root}_{}_{stand}.ani", e.name);
                mm.empty = model_path(&quirk).and_then(|p| clip_id.get(p.as_str()).copied());
                if mm.empty.is_some() {
                    report.empty_present += 1;
                } else {
                    report.empty_missing += 1;
                }
            } else if *ot == OT_ITEM && e.model_type == MODELTYPE_ANIMATED_MESH {
                let has_chr = models
                    .bone_file(*ot, e.index)
                    .and_then(|f| model_path(&f))
                    .is_some_and(|p| chr_paths.contains(p.as_str()));
                if has_chr {
                    has_row = true;
                    report.wings += 1;
                    mm.max = MTI_WALK + 1;
                    for (mti, motion) in [(MTI_STAND, "stand"), (MTI_WALK, "walk")] {
                        let file = format!("{root}_{}_{motion}.ani", e.name);
                        let path = model_path(&file);
                        report.slots += 1;
                        let slot = match path.as_deref().and_then(|p| clip_id.get(p)) {
                            Some(c) => Slot::Clip(*c),
                            None => {
                                wing_missing.insert(path.unwrap_or(file));
                                Slot::Missing
                            }
                        };
                        mm.slots.insert(mti, slot);
                    }
                }
            }
            if !has_row {
                continue;
            }

            // Bone count of the skeleton against the clips (reported, never fatal).
            if let Some(chr_file) = models
                .bone_file(*ot, e.index)
                .and_then(|f| model_path(&f))
                .filter(|p| chr_paths.contains(p.as_str()))
            {
                let count = *skeleton_bones.entry(chr_file.clone()).or_insert_with(|| {
                    let bytes = vfs.read(&chr_file).ok()?;
                    let f = chr::parse(&bytes).ok()?.value;
                    u32::try_from(f.bones.len()).ok()
                });
                if let Some(skel) = count {
                    for slot in mm.slots.values() {
                        if let Slot::Clip(c) = slot
                            && let Some(clip) = clips.get(*c as usize)
                            && clip.source == ClipSource::Ani
                            && u32::from(clip.bones) != skel
                        {
                            let entry = (clip.file.clone(), u32::from(clip.bones), skel);
                            if !report.bone_mismatch.contains(&entry) {
                                report.bone_mismatch.push(entry);
                            }
                        }
                    }
                }
            }
            rows.insert(key, mm);
        }
    }
    report.models = rows.len();
    report.missing_files = missing.into_iter().collect();
    report.wing_missing = wing_missing.into_iter().collect();
    report.bone_mismatch.sort();
    Ok(MotionBuild {
        table: MotionTable {
            clips,
            models: rows,
        },
        report,
    })
}

/// The JSON projection of `table` (the `motions` bundle): `clips` in `ClipId` order, `models` in
/// `(ot, index)` order.
pub fn motions_bundle(table: &MotionTable) -> MotionsBundle {
    let clip_rows = table
        .clips
        .iter()
        .map(|c| MotionClipRow {
            file: c.file.clone(),
            source: c.source as u32,
            frames: c.frames,
            per_slerp_raw: c.per_slerp_raw,
            bones: u32::from(c.bones),
            attrs: c
                .attrs
                .iter()
                .map(|a| {
                    [
                        f64::from(a.index),
                        f64::from(a.frame),
                        f64::from(a.flags),
                        f64::from(a.sound),
                    ]
                })
                .collect(),
        })
        .collect();
    let id = |c: Option<ClipId>| c.map_or(-1, |c| i32::try_from(c).unwrap_or(-1));
    let model_rows = table
        .models
        .iter()
        .map(|(k, m)| MotionModelRow {
            ot: k.ot,
            index: k.index,
            max: m.max,
            empty: id(m.empty),
            builtin: id(m.builtin),
            slots: m
                .slots
                .iter()
                .map(|(mti, s)| {
                    [
                        i32::try_from(*mti).unwrap_or(-1),
                        match s {
                            Slot::Clip(c) => id(Some(*c)),
                            Slot::Missing => -1,
                        },
                    ]
                })
                .collect(),
        })
        .collect();
    MotionsBundle {
        clips: RowTable::new(clip_rows),
        models: RowTable::new(model_rows),
    }
}

/// The builder of kind [`AssetKind::MotionTable`]: a single job, no partitions.
#[derive(Clone, Copy, Debug, Default)]
pub struct MotionBuilder;

impl AssetBuilder for MotionBuilder {
    fn kind(&self) -> AssetKind {
        AssetKind::MotionTable
    }

    fn builder_version(&self) -> u32 {
        BUILDER_VERSION
    }

    fn format_version(&self) -> u32 {
        u32::from(MOTION_TABLE_VERSION)
    }

    /// `all.bin` or `motions.json`.
    fn route(&self, _ctx: &FpCtx<'_>, key: &str) -> Result<KeyRoute, AssetError> {
        if key != KEY_BIN && key != KEY_JSON {
            return Err(AssetError::bad_key(format!(
                "{key:?}: motion-table has the keys {KEY_BIN:?} and {KEY_JSON:?}"
            )));
        }
        Ok(KeyRoute {
            job: JobKey::single(AssetKind::MotionTable, ""),
            partition: NO_PARTITION.to_owned(),
        })
    }

    /// `set_fp("model", ["ani", "o3d"])` and the registry fingerprint (§6.3); the `.chr` bone counts
    /// only feed the report, not the output.
    fn partition_fp(&self, ctx: &FpCtx<'_>, partition: &str) -> Option<Fp> {
        (partition == NO_PARTITION).then(|| {
            ctx.kind_fp(
                AssetKind::MotionTable,
                BUILDER_VERSION,
                u32::from(MOTION_TABLE_VERSION),
                &[ctx.set_fp("model", &["ani", "o3d"]), ctx.registry_fp()],
            )
        })
    }

    fn jobs(&self, _ctx: &FpCtx<'_>) -> Vec<JobKey> {
        vec![JobKey::single(AssetKind::MotionTable, "")]
    }

    fn build(&self, ctx: &BuildCtx<'_>, _job: &JobKey) -> Result<Vec<BuiltFile>, AssetError> {
        let built = build_motion_table(ctx.vfs, &ctx.data.models, &|| ctx.cancelled())?;
        let r = &built.report;
        for (file, err) in &r.unparsable {
            tracing::warn!(%file, error = %err, "motion table: unparsable .ani, its references are missing");
        }
        for (file, err) in &r.o3d_failed {
            tracing::warn!(%file, error = %err, "motion table: unparsable .o3d, no built-in clip");
        }
        for (file, ani, chr) in &r.bone_mismatch {
            tracing::warn!(%file, ani_bones = ani, skeleton_bones = chr, "motion table: .ani bone count differs from the skeleton");
        }
        tracing::debug!(
            clips = built.table.clips.len(),
            models = r.models,
            slots = r.slots,
            missing_refs = r.missing_refs,
            missing_files = r.missing_files.len(),
            "motion table built"
        );
        let json = encode_envelope(&motions_bundle(&built.table))
            .map_err(|e| AssetError::build(format!("motions bundle: {e}")))?;
        Ok(vec![
            BuiltFile::new(KEY_BIN, built.table.encode(), "application/octet-stream"),
            BuiltFile::new(KEY_JSON, json, "application/json"),
        ])
    }

    fn content_type(&self, key: &str) -> &'static str {
        if key == KEY_JSON {
            "application/json"
        } else {
            "application/octet-stream"
        }
    }

    /// `MotionTable::decode(all.bin)` into `ServerDerived.motions`; the bundle files go to `cache/data/`.
    fn load_server(
        &self,
        prep: &AssetPrep,
        out: &mut crate::ServerDerived,
    ) -> Result<(), AssetError> {
        let bin = read_artifact(prep, AssetKind::MotionTable, KEY_BIN)?;
        let table = MotionTable::decode(&bin)
            .map_err(|e| AssetError::build(format!("motion-table/{KEY_BIN}: {e}")))?;
        out.motions = Some(std::sync::Arc::new(table));
        let json = read_artifact(prep, AssetKind::MotionTable, KEY_JSON)?;
        external::publish(
            prep,
            MotionsBundle::NAME,
            MotionsBundle::SCHEMA,
            MotionsBundle::LOAD,
            json,
        )?;
        Ok(())
    }
}
