//! DDS fixes (`dds` kind, Plan/24 §10.4, WP-WEB-09): complete the mip chains of the textures that ship
//! without one and convert the uncompressed layouts WebGL2 cannot upload directly.
//!
//! * **Scan** (`on_start`, §10.4.1): the 128-byte header of every DDS of the five 3D texture directories is
//!   classified ([`DdsClass`]) and the result stored in `index/dds-scan.<fp>.json`; `FixMips` and
//!   `FixLayout` file names go to `ManifestParts::set_dds_fix` (the client manifest's `dds_fix` and the
//!   gzip prewarm's exclusion list).
//! * **Job** (one per fix file): read the file, build the output of §10.4.2 with [`fix_dds`] and store it at
//!   `gen/dds/<partfp>/<vfs path>` (`Compress::IfBeneficial{9}`).
//!
//! ## Output layouts (§10.4.2)
//!
//! A standard DDS (`DDS ` + 124-byte header, no DX10 header) with `DDSD_MIPMAPCOUNT` and the full chain
//! down to 1x1:
//!
//! | source | output | `ddspf` |
//! |---|---|---|
//! | DXT1/DXT3/DXT5 (sides multiple of 4) | same FourCC; level 0 = the source blocks, levels >= 1 re-encoded by [`super::bc_encode`] | unchanged |
//! | R5G6B5 | 16-bit, masks `F800 07E0 001F`, no alpha | `DDPF_RGB` |
//! | A1R5G5B5, X1R5G5B5 | 16-bit RGBA5551 `r5 << 11 \| g5 << 6 \| b5 << 1 \| a1` (X1: `a1 = 1`), masks `F800 07C0 003E 0001` | `DDPF_RGB \| DDPF_ALPHAPIXELS` |
//! | A8R8G8B8, X8R8G8B8, A4R4G4B4, anything else (also DXT with a side that is not a multiple of 4) | 32-bit RGBA8 in byte order R, G, B, A, masks `000000FF 0000FF00 00FF0000 FF000000` (X8: `A = 255`) | `DDPF_RGB \| DDPF_ALPHAPIXELS` |
//!
//! Mips are generated in the 8-bit domain: the previous level's RGBA8 image is halved with the 2x2 box
//! filter `(a + b + c + d + 2) >> 2` ([`flyff_formats::texture::downsample_box`]; a side of 1 duplicates its
//! texel) and then quantised or block-encoded into the output format (5-bit `(v * 31 + 127) / 255`, 6-bit
//! `(v * 63 + 127) / 255`, 1-bit alpha `a >= 128`). Level 0 of an uncompressed source is converted
//! texel for texel, without filtering ([`flyff_formats::texture::to_gl_upload`]).

use super::bc_encode::{self, BC_ENCODE_VERSION, BcFormat};
use crate::cache::Cache;
use crate::error::AssetError;
use crate::fp::{Fp, FpCtx, group_fp};
use crate::kinds::{
    AssetBuilder, AssetKind, BuildCtx, BuiltFile, JobKey, KeyRoute, content_type_by_ext,
};
use crate::res;
use flyff_formats::dds::{
    self, DATA_OFFSET, DDSCAPS2_CUBEMAP_ALLFACES, DDSD_MIPMAPCOUNT, DdsFormat, DdsHeader,
    PixelFormat,
};
use flyff_formats::limits::{MAX_DDS_DIM, max_mip_levels};
use flyff_formats::texture::{self, GlUpload, Rgba8Image};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::sync::{Arc, Mutex};
use xxhash_rust::xxh3::Xxh3;

/// Bump on any change that alters the produced bytes (see also [`BC_ENCODE_VERSION`], an input of the
/// partition fingerprint).
pub const BUILDER_VERSION: u32 = 1;
/// Bump when the byte layout of the outputs (§10.4.2) changes.
pub const FORMAT_VERSION: u32 = 1;
/// Bump when the classification rules of [`classify_header`] change (the scan index is recomputed).
pub const SCAN_VERSION: u32 = 1;

/// The five directories whose DDS files go through the scan, in prewarm order (§7.5: P4 `model/texture`,
/// then P6 the rest).
pub const SCAN_DIRS: [&str; 5] = [
    "model/texture",
    "sfx/texture",
    "sfx",
    "world/texture",
    "weather",
];

/// The class of one DDS file (§10.4.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DdsClass {
    /// DXT1/3/5 with a mip chain and sides that are multiples of 4: served raw.
    Ok,
    /// 16-bit R5G6B5 with a mip chain: served raw (already the WebGL2 `RGB565` layout).
    Ok565,
    /// DXT1/3/5 without a mip chain: `/gen/dds`.
    FixMips,
    /// Any other uncompressed layout, R5G6B5 without mips, or DXT with odd sides: `/gen/dds`.
    FixLayout,
    /// Bad magic, header or size: no client form (placeholder), listed in the report.
    Invalid,
}

impl DdsClass {
    /// `true` for the two classes whose client form is `/gen/dds`.
    pub const fn is_fix(self) -> bool {
        matches!(self, DdsClass::FixMips | DdsClass::FixLayout)
    }
}

/// What the header says about a file (for [`classify_header`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DdsHead {
    /// Width in texels.
    pub width: u32,
    /// Height in texels.
    pub height: u32,
    /// Declared mip levels per face (`1` without `DDSD_MIPMAPCOUNT`).
    pub levels: u32,
    /// The pixel format.
    pub format: DdsFormat,
    /// `true` for a cube map.
    pub cube: bool,
}

fn le32(b: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(off..off + 4)?.try_into().ok()?))
}

/// Parses the 128-byte header (`DDS ` + `DDS_HEADER`). The errors are one-line descriptions for the
/// report; they match the rejections of `flyff_formats::dds::view` for everything the header decides.
pub fn read_head(b: &[u8]) -> Result<DdsHead, String> {
    if b.get(..4) != Some(b"DDS ") {
        return Err("bad magic".to_owned());
    }
    let field = |off: usize| le32(b, off).ok_or_else(|| "truncated header".to_owned());
    if field(4)? != 124 {
        return Err("bad dwSize".to_owned());
    }
    if field(76)? != 32 {
        return Err("bad pixel-format size".to_owned());
    }
    let (flags, height, width, mips) = (field(8)?, field(12)?, field(16)?, field(28)?);
    if width == 0 || height == 0 || width > MAX_DDS_DIM || height > MAX_DDS_DIM {
        return Err(format!("bad size {width}x{height}"));
    }
    let pf = PixelFormat {
        size: 32,
        flags: field(80)?,
        four_cc: field(84)?.to_le_bytes(),
        rgb_bit_count: field(88)?,
        masks: [field(92)?, field(96)?, field(100)?, field(104)?],
    };
    let format = DdsFormat::identify(&pf)?;
    let caps2 = field(112)?;
    let cube = caps2 & dds::DDSCAPS2_CUBEMAP != 0;
    if caps2 & dds::DDSCAPS2_VOLUME != 0 {
        return Err("volume texture".to_owned());
    }
    if cube && caps2 & DDSCAPS2_CUBEMAP_ALLFACES != DDSCAPS2_CUBEMAP_ALLFACES {
        return Err("incomplete cube map".to_owned());
    }
    let levels = if flags & DDSD_MIPMAPCOUNT != 0 && mips > 0 {
        mips
    } else {
        1
    };
    if levels > max_mip_levels(width, height) {
        return Err(format!("{levels} mip levels for {width}x{height}"));
    }
    Ok(DdsHead {
        width,
        height,
        levels,
        format,
        cube,
    })
}

/// The expected file size of the stored levels (header included), `None` on overflow.
fn stored_size(h: &DdsHead) -> Option<u64> {
    let faces = if h.cube { 6u64 } else { 1 };
    let mut total = 0u64;
    for k in 0..h.levels {
        let (w, hh) = ((h.width >> k).max(1), (h.height >> k).max(1));
        total = total.checked_add(h.format.level_size(w, hh))?;
    }
    total.checked_mul(faces)?.checked_add(DATA_OFFSET as u64)
}

/// Classifies a file from its first 128 bytes and its total size (§10.4.1). A file shorter than its
/// header promises is `Invalid`.
pub fn classify_header(head: &[u8], file_size: u64) -> DdsClass {
    let Ok(h) = read_head(head) else {
        return DdsClass::Invalid;
    };
    if stored_size(&h).is_none_or(|need| file_size < need) {
        return DdsClass::Invalid;
    }
    let sides_ok = h.width % 4 == 0 && h.height % 4 == 0;
    match h.format {
        DdsFormat::Bc1 | DdsFormat::Bc2 | DdsFormat::Bc3 if sides_ok => {
            if h.levels >= 2 {
                DdsClass::Ok
            } else {
                DdsClass::FixMips
            }
        }
        DdsFormat::R5G6B5 if h.levels >= 2 => DdsClass::Ok565,
        _ => DdsClass::FixLayout,
    }
}

/// The scan result of one directory.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirScan {
    /// Files served raw (`Ok`).
    pub ok: u32,
    /// Files served raw (`Ok565`).
    pub ok565: u32,
    /// File names (lower case, without the directory) of class `FixMips`.
    pub fix_mips: Vec<String>,
    /// File names of class `FixLayout`.
    pub fix_layout: Vec<String>,
    /// File names of class `Invalid`.
    pub invalid: Vec<String>,
}

impl DirScan {
    /// The `dds_fix` list of the directory: `FixMips` and `FixLayout` names, sorted.
    pub fn fix_names(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .fix_mips
            .iter()
            .chain(&self.fix_layout)
            .cloned()
            .collect();
        v.sort();
        v
    }

    /// Number of scanned files.
    pub fn total(&self) -> usize {
        self.ok as usize
            + self.ok565 as usize
            + self.fix_mips.len()
            + self.fix_layout.len()
            + self.invalid.len()
    }
}

/// The scan of all five directories (`index/dds-scan.<fp>.json`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DdsScan {
    /// File format version of the index (`1`).
    pub format: u32,
    /// The scan fingerprint (also in the file name).
    pub fp: Fp,
    /// Per directory.
    pub dirs: BTreeMap<String, DirScan>,
}

impl DdsScan {
    /// Total of one class over all directories.
    pub fn count(&self, class: DdsClass) -> usize {
        self.dirs
            .values()
            .map(|d| match class {
                DdsClass::Ok => d.ok as usize,
                DdsClass::Ok565 => d.ok565 as usize,
                DdsClass::FixMips => d.fix_mips.len(),
                DdsClass::FixLayout => d.fix_layout.len(),
                DdsClass::Invalid => d.invalid.len(),
            })
            .sum()
    }
}

/// The scan fingerprint: the group fingerprints of the five directories (§10.4.1) plus the scan version.
pub fn scan_fp(vfs: &flyff_vfs::Vfs) -> Fp {
    let mut h = Xxh3::new();
    h.update(b"D1");
    h.update(&SCAN_VERSION.to_le_bytes());
    for dir in SCAN_DIRS {
        h.update(dir.as_bytes());
        h.update(&[0]);
        h.update(&group_fp(vfs, dir).unwrap_or(Fp::ZERO).0.to_le_bytes());
    }
    Fp(h.digest())
}

/// Reads up to 128 bytes of `path`.
fn read_head_bytes(vfs: &flyff_vfs::Vfs, path: &str) -> Option<Vec<u8>> {
    let f = vfs.open_file(path).ok()?;
    let mut buf = Vec::with_capacity(DATA_OFFSET);
    f.take(DATA_OFFSET as u64).read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// Scans every DDS of the five directories (reads 128 bytes per file).
pub fn scan_vfs(vfs: &flyff_vfs::Vfs, cancelled: &dyn Fn() -> bool) -> Option<DdsScan> {
    let mut dirs = BTreeMap::new();
    for dir in SCAN_DIRS {
        let mut d = DirScan::default();
        for e in vfs.list_dir(dir) {
            if cancelled() {
                return None;
            }
            let path = e.path.as_str();
            if res::ext_of(path) != Some("dds") {
                continue;
            }
            let name = path.rsplit('/').next().unwrap_or(path).to_owned();
            let class = read_head_bytes(vfs, path)
                .map_or(DdsClass::Invalid, |h| classify_header(&h, e.size));
            match class {
                DdsClass::Ok => d.ok += 1,
                DdsClass::Ok565 => d.ok565 += 1,
                DdsClass::FixMips => d.fix_mips.push(name),
                DdsClass::FixLayout => d.fix_layout.push(name),
                DdsClass::Invalid => d.invalid.push(name),
            }
        }
        d.fix_mips.sort();
        d.fix_layout.sort();
        d.invalid.sort();
        dirs.insert(dir.to_owned(), d);
    }
    Some(DdsScan {
        format: 1,
        fp: scan_fp(vfs),
        dirs,
    })
}

// ---- the fix -------------------------------------------------------------------------------------------------

/// The output pixel format of a fix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Out {
    Bc(BcFormat),
    Rgb565,
    Rgba5551,
    Rgba8,
}

impl Out {
    fn dds_format(self, src: DdsFormat) -> DdsFormat {
        match self {
            Out::Bc(_) => src,
            Out::Rgb565 => DdsFormat::R5G6B5,
            Out::Rgba5551 => DdsFormat::OtherRgb {
                bits: 16,
                masks: [0xF800, 0x07C0, 0x003E, 0x0001],
            },
            Out::Rgba8 => DdsFormat::OtherRgb {
                bits: 32,
                masks: [0x0000_00FF, 0x0000_FF00, 0x00FF_0000, 0xFF00_0000],
            },
        }
    }

    /// Quantises or block-encodes one level of RGBA8 pixels.
    fn encode(self, img: &Rgba8Image) -> Vec<u8> {
        match self {
            Out::Bc(f) => bc_encode::encode(f, &img.pixels, img.width, img.height),
            Out::Rgb565 => {
                let mut out = Vec::with_capacity(img.pixels.len() / 2);
                for &[r, g, b, _] in img.pixels.as_chunks::<4>().0 {
                    let v = (q(r, 31) << 11) | (q(g, 63) << 5) | q(b, 31);
                    out.extend_from_slice(&(v as u16).to_le_bytes());
                }
                out
            }
            Out::Rgba5551 => {
                let mut out = Vec::with_capacity(img.pixels.len() / 2);
                for &[r, g, b, a] in img.pixels.as_chunks::<4>().0 {
                    let v =
                        (q(r, 31) << 11) | (q(g, 31) << 6) | (q(b, 31) << 1) | u32::from(a >= 128);
                    out.extend_from_slice(&(v as u16).to_le_bytes());
                }
                out
            }
            Out::Rgba8 => img.pixels.clone(),
        }
    }
}

/// `(v * max + 127) / 255`.
fn q(v: u8, max: u32) -> u32 {
    (u32::from(v) * max + 127) / 255
}

fn build_err(file: &str, what: impl std::fmt::Display) -> AssetError {
    AssetError::build(format!("{file}: {what}"))
}

/// Builds the fixed DDS of `src` (§10.4.2): the same image with a complete mip chain in a layout WebGL2
/// uploads directly. `name` is the VFS path, used in error messages. Pure and deterministic.
pub fn fix_dds(src: &[u8], name: &str) -> Result<Vec<u8>, AssetError> {
    let parsed = dds::view(src).map_err(|e| build_err(name, e))?;
    let view = parsed.value;
    let (w, h) = (view.header.width, view.header.height);
    let levels = max_mip_levels(w, h);
    let compressed = view.format.is_compressed();
    let sides_ok = w % 4 == 0 && h % 4 == 0;

    // The output format follows the level-0 conversion of `to_gl_upload`.
    let mut scratch = Vec::new();
    let probe = texture::to_gl_upload(&view, 0, 0, &mut scratch).map_err(|e| build_err(name, e))?;
    let out_fmt = match probe {
        GlUpload::Bc1 | GlUpload::Bc2 | GlUpload::Bc3 if !sides_ok => Out::Rgba8,
        GlUpload::Bc1 => Out::Bc(BcFormat::Bc1),
        GlUpload::Bc2 => Out::Bc(BcFormat::Bc2),
        GlUpload::Bc3 => Out::Bc(BcFormat::Bc3),
        GlUpload::Rgb565 => Out::Rgb565,
        GlUpload::Rgba5551 => Out::Rgba5551,
        GlUpload::Rgba8 => Out::Rgba8,
    };

    let mut header = DdsHeader::new(w, h, out_fmt.dds_format(view.format), levels);
    header.caps2 = view.header.caps2;
    let mut out = Vec::new();
    header.write_to(&mut out);

    for face in 0..view.face_count() {
        // Level 0: the source blocks / texel-for-texel conversion.
        let mut base = texture::decode_level(&view, face, 0).map_err(|e| build_err(name, e))?;
        if compressed && !sides_ok {
            out.extend_from_slice(&out_fmt.encode(&base));
        } else {
            texture::to_gl_upload(&view, face, 0, &mut scratch).map_err(|e| build_err(name, e))?;
            out.extend_from_slice(&scratch);
        }
        for _ in 1..levels {
            base = texture::downsample_box(&base);
            out.extend_from_slice(&out_fmt.encode(&base));
        }
    }
    Ok(out)
}

// ---- the builder -------------------------------------------------------------------------------------------

/// The scan plus the set of fix paths for O(log n) routing.
struct Loaded {
    scan: Arc<DdsScan>,
    fix_paths: BTreeSet<String>,
}

/// The builder of kind [`AssetKind::DdsFix`]: one job per fix file; partition = source directory.
pub struct DdsBuilder {
    loaded: Mutex<Option<Arc<Loaded>>>,
}

impl Default for DdsBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl DdsBuilder {
    /// A builder that has not scanned yet (`on_start` does).
    pub fn new() -> Self {
        Self {
            loaded: Mutex::new(None),
        }
    }

    fn loaded(&self) -> Option<Arc<Loaded>> {
        crate::util::lock(&self.loaded).clone()
    }

    /// The scan of this process, once `on_start` ran.
    pub fn scan(&self) -> Option<Arc<DdsScan>> {
        self.loaded().map(|l| Arc::clone(&l.scan))
    }

    fn install(&self, scan: DdsScan) {
        let mut fix_paths = BTreeSet::new();
        for (dir, d) in &scan.dirs {
            for name in d.fix_names() {
                fix_paths.insert(format!("{dir}/{name}"));
            }
        }
        *crate::util::lock(&self.loaded) = Some(Arc::new(Loaded {
            scan: Arc::new(scan),
            fix_paths,
        }));
    }
}

fn scan_rel(fp: Fp) -> String {
    format!("index/dds-scan.{fp}.json")
}

impl AssetBuilder for DdsBuilder {
    fn kind(&self) -> AssetKind {
        AssetKind::DdsFix
    }

    fn builder_version(&self) -> u32 {
        BUILDER_VERSION
    }

    fn format_version(&self) -> u32 {
        FORMAT_VERSION
    }

    /// The key is the VFS path of a DDS listed in `dds_fix` (e.g. `model/texture/obj_mafltree01.dds`).
    fn route(&self, ctx: &FpCtx<'_>, key: &str) -> Result<KeyRoute, AssetError> {
        let entry = ctx
            .vfs
            .resolve(key)
            .ok_or_else(|| AssetError::not_found(format!("no such file: {key:?}")))?;
        if entry.path.as_str() != key {
            return Err(AssetError::bad_key(format!(
                "not a canonical path: {key:?}"
            )));
        }
        let dir = res::dir_of(key);
        if res::ext_of(key) != Some("dds") || !SCAN_DIRS.contains(&dir) {
            return Err(AssetError::not_convertible(format!(
                "{key}: not a DDS of a fixable directory"
            )));
        }
        if !self.loaded().is_some_and(|l| l.fix_paths.contains(key)) {
            return Err(AssetError::not_convertible(format!(
                "{key}: not listed in dds_fix (served raw or invalid)"
            )));
        }
        Ok(KeyRoute {
            job: JobKey::new(AssetKind::DdsFix, dir, key),
            partition: dir.to_owned(),
        })
    }

    fn partition_fp(&self, ctx: &FpCtx<'_>, partition: &str) -> Option<Fp> {
        SCAN_DIRS.contains(&partition).then(|| {
            ctx.kind_fp(
                AssetKind::DdsFix,
                BUILDER_VERSION,
                FORMAT_VERSION,
                &[
                    ctx.set_fp(partition, &["dds"]),
                    Fp(u64::from(BC_ENCODE_VERSION)),
                ],
            )
        })
    }

    /// Every fix file: `model/texture` first (P4), then `sfx/texture`, `sfx`, `world/texture`, `weather`
    /// (P6), names sorted.
    fn jobs(&self, _ctx: &FpCtx<'_>) -> Vec<JobKey> {
        let Some(scan) = self.scan() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for dir in SCAN_DIRS {
            if let Some(d) = scan.dirs.get(dir) {
                for name in d.fix_names() {
                    out.push(JobKey::new(AssetKind::DdsFix, dir, format!("{dir}/{name}")));
                }
            }
        }
        out
    }

    fn build(&self, ctx: &BuildCtx<'_>, job: &JobKey) -> Result<Vec<BuiltFile>, AssetError> {
        let src = ctx
            .vfs
            .read(&job.name)
            .map_err(|e| build_err(&job.name, e))?;
        let bytes = fix_dds(&src, &job.name)?;
        Ok(vec![BuiltFile::new(
            job.name.clone(),
            bytes,
            content_type_by_ext(&job.name),
        )])
    }

    /// Loads or builds `index/dds-scan.<fp>.json`, publishes the `dds_fix` lists and keeps the index
    /// file alive for the sweep.
    fn on_start(&self, ctx: &BuildCtx<'_>, cache: &Cache) -> Result<(), AssetError> {
        let fp = scan_fp(ctx.vfs);
        let rel = scan_rel(fp);
        let cached = std::fs::read(cache.path_of(&rel))
            .ok()
            .and_then(|b| serde_json::from_slice::<DdsScan>(&b).ok())
            .filter(|s| s.format == 1 && s.fp == fp);
        let scan = match cached {
            Some(s) => s,
            None => {
                let s =
                    scan_vfs(ctx.vfs, &|| ctx.cancelled()).ok_or_else(AssetError::shutting_down)?;
                let json = serde_json::to_vec(&s)
                    .map_err(|e| AssetError::build(format!("dds scan index: {e}")))?;
                if let Err(e) = cache.write_atomic(&rel, &json, false) {
                    tracing::warn!(error = %e, "could not write the dds scan index");
                }
                s
            }
        };
        for dir in SCAN_DIRS {
            let names = scan
                .dirs
                .get(dir)
                .map(DirScan::fix_names)
                .unwrap_or_default();
            ctx.manifest_parts.set_dds_fix(dir, names);
        }
        ctx.manifest_parts.keep_fingerprint(fp);
        for (dir, d) in &scan.dirs {
            for bad in &d.invalid {
                tracing::warn!(file = %format!("{dir}/{bad}"), "invalid DDS: no client form");
            }
        }
        self.install(scan);
        Ok(())
    }
}
