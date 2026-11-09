//! Model bounds (`model-bounds` kind, Plan/24 §10.6, WP-WEB-12): the header bounding box of every
//! `.o3d` of `model/`, as one `flyff_sim::bounds::ModelBounds` value (the `FLMB` v1 file both sides load).
//!
//! One job (no partition) builds the value and stores two artifacts under `gen/model-bounds/<fp>/`:
//!
//! * `all.bin`: `ModelBounds::encode()` (Plan/31 §11.5), the bytes the server decodes into
//!   `ServerDerived.bounds` and the browser passes to `SimWorld.load_model_bounds`;
//! * `model-bounds.json`: the envelope of the `model-bounds` bundle (`flyff_data::bundle::types::
//!   ModelBoundsBundle`, one row per table entry in entry order), published to `cache/data/` by
//!   `load_server` so `/data` serves it like any other bundle.
//!
//! Rows are the parsable `.o3d` files: the two legacy heads (`Part_maleHead.o3d`, `Part_femaleHead.o3d`,
//! pre-v20 layout, unreferenced) are skipped, a header-name mismatch (6 files) loads with a warning
//! (ref/18 §4.10: load and warn). `min`/`max` are the header `bbMin`/`bbMax` (ref/18 §4.1 fields 12-13),
//! bit-exact. The per-file box is all this table stores: the model AABB rule of the original (the union of
//! the loaded parts, starting from the *empty* box `+65535 / -65535`, §10.6) is
//! `flyff_sim::bounds::union_parts`, and the instance scale is applied by `radius`/`IsRangeObj`.

use super::external;
use crate::error::AssetError;
use crate::fp::{Fp, FpCtx};
use crate::kinds::{AssetBuilder, AssetKind, BuildCtx, BuiltFile, JobKey, KeyRoute, NO_PARTITION};
use crate::prep::AssetPrep;
use crate::util::{job_threads, par_map};
use flyff_data::bundle::envelope::encode_envelope;
use flyff_data::bundle::types::{ModelBoundsBundle, ModelBoundsRow};
use flyff_data::bundle::{BundleData, RowTable};
use flyff_formats::error::{ErrorKind, WarningCode};
use flyff_formats::o3d::{self, O3dOptions};
use flyff_sim::bounds::{MODEL_BOUNDS_VERSION, ModelBounds};
use flyff_sim::collision::geom::Aabb;
use flyff_sim::num::Vec3;
use flyff_vfs::Vfs;
use std::sync::Arc;

/// Bump on any change that alters the produced bytes (row selection, JSON projection).
pub const BUILDER_VERSION: u32 = 1;

/// The key of the binary table.
pub const KEY_BIN: &str = "all.bin";
/// The key of the JSON projection (the `model-bounds` bundle envelope).
pub const KEY_JSON: &str = "model-bounds.json";

/// What the build skipped or noted (logged by the builder, asserted by the corpus test).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BoundsReport {
    /// `.o3d` files found in `model/`.
    pub files: usize,
    /// Rows of the table.
    pub rows: usize,
    /// Pre-v20 files without the name header (skipped).
    pub legacy: Vec<String>,
    /// Files with a header-name mismatch (loaded anyway).
    pub name_mismatch: Vec<String>,
    /// Files skipped for another reason, with the parser's message.
    pub failed: Vec<(String, String)>,
}

/// The value of one build: the table, the rows it was made from (entry order) and the report.
#[derive(Clone, Debug)]
pub struct BoundsBuild {
    /// The table (`FLMB`).
    pub bounds: ModelBounds,
    /// `(path, box)` in entry order: row `i` of the JSON projection is `entries[i]`.
    pub entries: Vec<(String, Aabb)>,
    /// What was skipped.
    pub report: BoundsReport,
}

enum Row {
    Ok(String, Aabb, bool),
    Legacy(String),
    Failed(String, String),
}

fn read_row(vfs: &Vfs, path: &str) -> Row {
    let bytes = match vfs.read(path) {
        Ok(b) => b,
        Err(e) => return Row::Failed(path.to_owned(), e.to_string()),
    };
    let opts = O3dOptions::skip().with_file_name(path.rsplit('/').next().unwrap_or(path));
    match o3d::parse(&bytes, &opts) {
        Ok(p) => {
            let f = &p.value;
            let mismatch = p
                .warnings
                .iter()
                .any(|w| w.code == WarningCode::O3dNameMismatch);
            let b = Aabb {
                min: Vec3::new(f.bb_min[0], f.bb_min[1], f.bb_min[2]),
                max: Vec3::new(f.bb_max[0], f.bb_max[1], f.bb_max[2]),
            };
            if ![b.min.x, b.min.y, b.min.z, b.max.x, b.max.y, b.max.z]
                .iter()
                .all(|v| v.is_finite())
            {
                return Row::Failed(path.to_owned(), "non-finite bounding box".to_owned());
            }
            Row::Ok(path.to_owned(), b, mismatch)
        }
        Err(e) if e.kind == ErrorKind::LegacyLayout => Row::Legacy(path.to_owned()),
        Err(e) => Row::Failed(path.to_owned(), e.to_string()),
    }
}

/// Builds the table from every `.o3d` of `model/` (a pure function of the VFS bytes). Rows are sorted
/// by path bytes, which is the entry order of `FLMB` and of the JSON projection. `cancelled` is polled
/// between files; a cancelled build is `ShuttingDown`.
pub fn build_model_bounds(
    vfs: &Vfs,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<BoundsBuild, AssetError> {
    let mut paths: Vec<&str> = vfs
        .list_dir("model")
        .into_iter()
        .map(|e| e.path.as_str())
        .filter(|p| crate::res::ext_of(p) == Some("o3d"))
        .collect();
    paths.sort_unstable();
    let rows = par_map(&paths, job_threads(), |p| {
        if cancelled() {
            Row::Failed((*p).to_owned(), "cancelled".to_owned())
        } else {
            read_row(vfs, p)
        }
    });
    if cancelled() {
        return Err(AssetError::shutting_down());
    }
    let mut report = BoundsReport {
        files: paths.len(),
        ..BoundsReport::default()
    };
    let mut entries = Vec::with_capacity(rows.len());
    for r in rows {
        match r {
            Row::Ok(path, b, mismatch) => {
                if mismatch {
                    report.name_mismatch.push(path.clone());
                }
                entries.push((path, b));
            }
            Row::Legacy(p) => report.legacy.push(p),
            Row::Failed(p, e) => report.failed.push((p, e)),
        }
    }
    report.rows = entries.len();
    let bounds = ModelBounds::new(entries.clone())
        .map_err(|e| AssetError::build(format!("model bounds table: {e}")))?;
    Ok(BoundsBuild {
        bounds,
        entries,
        report,
    })
}

/// The JSON projection of `entries` (`(path, box)` in table order).
pub fn bundle_of(entries: &[(String, Aabb)]) -> ModelBoundsBundle {
    ModelBoundsBundle {
        files: RowTable::new(
            entries
                .iter()
                .map(|(file, b)| ModelBoundsRow {
                    file: file.clone(),
                    min: [b.min.x, b.min.y, b.min.z],
                    max: [b.max.x, b.max.y, b.max.z],
                })
                .collect(),
        ),
    }
}

/// The builder of kind [`AssetKind::ModelBounds`]: a single job, no partitions.
#[derive(Clone, Copy, Debug, Default)]
pub struct BoundsBuilder;

impl AssetBuilder for BoundsBuilder {
    fn kind(&self) -> AssetKind {
        AssetKind::ModelBounds
    }

    fn builder_version(&self) -> u32 {
        BUILDER_VERSION
    }

    fn format_version(&self) -> u32 {
        u32::from(MODEL_BOUNDS_VERSION)
    }

    /// `all.bin` or `model-bounds.json`.
    fn route(&self, _ctx: &FpCtx<'_>, key: &str) -> Result<KeyRoute, AssetError> {
        if key != KEY_BIN && key != KEY_JSON {
            return Err(AssetError::bad_key(format!(
                "{key:?}: model-bounds has the keys {KEY_BIN:?} and {KEY_JSON:?}"
            )));
        }
        Ok(KeyRoute {
            job: JobKey::single(AssetKind::ModelBounds, ""),
            partition: NO_PARTITION.to_owned(),
        })
    }

    fn partition_fp(&self, ctx: &FpCtx<'_>, partition: &str) -> Option<Fp> {
        (partition == NO_PARTITION).then(|| {
            ctx.kind_fp(
                AssetKind::ModelBounds,
                BUILDER_VERSION,
                u32::from(MODEL_BOUNDS_VERSION),
                &[ctx.set_fp("model", &["o3d"])],
            )
        })
    }

    fn jobs(&self, _ctx: &FpCtx<'_>) -> Vec<JobKey> {
        vec![JobKey::single(AssetKind::ModelBounds, "")]
    }

    fn build(&self, ctx: &BuildCtx<'_>, _job: &JobKey) -> Result<Vec<BuiltFile>, AssetError> {
        let built = build_model_bounds(ctx.vfs, &|| ctx.cancelled())?;
        for (file, err) in &built.report.failed {
            tracing::warn!(%file, error = %err, "model bounds: unparsable .o3d skipped");
        }
        for file in &built.report.name_mismatch {
            tracing::warn!(%file, "model bounds: the .o3d header name differs from the file name");
        }
        let json = encode_envelope(&bundle_of(&built.entries))
            .map_err(|e| AssetError::build(format!("model-bounds bundle: {e}")))?;
        Ok(vec![
            BuiltFile::new(KEY_BIN, built.bounds.encode(), "application/octet-stream"),
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

    /// `ModelBounds::decode(all.bin)` into `ServerDerived.bounds`; the bundle files go to `cache/data/`.
    fn load_server(
        &self,
        prep: &AssetPrep,
        out: &mut crate::ServerDerived,
    ) -> Result<(), AssetError> {
        let bin = read_artifact(prep, AssetKind::ModelBounds, KEY_BIN)?;
        let bounds = ModelBounds::decode(&bin)
            .map_err(|e| AssetError::build(format!("model-bounds/{KEY_BIN}: {e}")))?;
        out.bounds = Some(Arc::new(bounds));
        let json = read_artifact(prep, AssetKind::ModelBounds, KEY_JSON)?;
        external::publish(
            prep,
            ModelBoundsBundle::NAME,
            ModelBoundsBundle::SCHEMA,
            ModelBoundsBundle::LOAD,
            json,
        )?;
        Ok(())
    }
}

/// Reads and inflates the cached artifact `key` of `kind`; a missing artifact is a build error (the
/// start-up job ran before the hook).
pub(crate) fn read_artifact(
    prep: &AssetPrep,
    kind: AssetKind,
    key: &str,
) -> Result<Vec<u8>, AssetError> {
    let file = prep.lookup(kind, key)?.ok_or_else(|| {
        AssetError::build(format!("{kind}/{key} is missing after its start-up job"))
    })?;
    file.read_decoded()
        .map_err(|e| AssetError::build(format!("{kind}/{key}: {e}")))
}
