//! Area 1 of the module: the browser decoders (Plan/30 §14; WP-FMT-16, WP-FMT-17).
//!
//! Seven synchronous exports, one per [`DecodeKind`](#kinds) of the worker protocol (kind `x` calls
//! `decode_x`), all with the same shape:
//!
//! ```text
//! decode_x(bytes: Uint8Array, opts?: object) -> { meta: <XMeta>, blob: Uint8Array | null }
//! ```
//!
//! * `meta` is a plain JS object built with `serde-wasm-bindgen` from the `flyff_formats::pack` meta type
//!   (`None` becomes `null`, no `Map`s, no 64-bit integers), typed by the generated
//!   `web/src/gen/formats/*.ts`.
//! * `blob` owns a fresh, exactly sized `ArrayBuffer` (`byteOffset 0`), so a worker can post
//!   `blob.buffer` as a transferable; the meta refers to its sections with `BlobView`s (4-byte aligned).
//!   `decode_sfx` has no blob (`null`).
//! * Every failure is a `JsError` with the message `"<format>: <kind> at offset <n>"`; nothing traps. Inputs
//!   above 32 MiB are rejected from the `Uint8Array` length, before any copy into wasm memory.
//!
//! The decoding itself is plain Rust in the `core` functions (natively testable, no `JsValue`); the
//! `#[wasm_bindgen]` functions only convert arguments and results.
//!
//! Kinds: `o3d` (mesh pack), `chr` (`FSK1` skeleton + attachment info), `ani` (`FMO1` motion + hit frames),
//! `lnd` (land grids and placements), `sfx` (effect prototype), `dds` (S3TC passthrough / GL layouts),
//! `dds_bc` (everything decoded to RGBA8 for GPUs without S3TC). There is no TGA/BMP/CUR decoder: those
//! formats arrive as PNG from `/gen/png` (Plan/30 §14.1).

use flyff_formats::ani::{self, AniOptions};
use flyff_formats::dds;
use flyff_formats::limits::MAX_DECODER_INPUT;
use flyff_formats::lnd::LandView;
use flyff_formats::o3d::{self, O3dOptions};
use flyff_formats::pack::dds::{self as pdds, BcMode, DdsPackOptions};
use flyff_formats::pack::lnd::{self as plnd, LndPackOptions};
use flyff_formats::pack::o3d::{self as po3d, O3dPackOptions};
use flyff_formats::pack::sfx as psfx;
use flyff_formats::pack::skeleton::{self as pskel, ChrPackOptions};
use flyff_formats::pack::{AniMeta, ChrMeta, DdsMeta, LndMeta, O3dMeta, Packed, SfxMeta, motion};
use flyff_formats::{FormatError, FormatKind, FormatWarning, chr, sfx, texture, version};
use js_sys::{Object, Reflect, Uint8Array};
use serde::Serialize;
use serde::de::DeserializeOwned;
use wasm_bindgen::prelude::*;

/// Largest accepted input of every decoder: 32 MiB (Plan/30 §2.2, worker budget 64 MB).
pub const MAX_INPUT_BYTES: usize = MAX_DECODER_INPUT;

// ---------------------------------------------------------------------------------------------------------
// Native core: bytes in, typed result out
// ---------------------------------------------------------------------------------------------------------

/// A decode failure: the message that becomes the `JsError`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeError(pub String);

impl core::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The short lower-case name of a format for error messages (`"o3d"`, `"fsk1"`, ...).
pub fn format_name(kind: FormatKind) -> &'static str {
    match kind {
        FormatKind::O3d => "o3d",
        FormatKind::Chr => "chr",
        FormatKind::Ani => "ani",
        FormatKind::Lnd => "lnd",
        FormatKind::Dyo => "dyo",
        FormatKind::Sfx => "sfx",
        FormatKind::Dds => "dds",
        FormatKind::Tga => "tga",
        FormatKind::Bmp => "bmp",
        FormatKind::Cur => "cur",
        FormatKind::Wld => "wld",
        FormatKind::Rgn => "rgn",
        FormatKind::Cnt => "cnt",
        FormatKind::Pat => "pat",
        FormatKind::L24 => "24l",
        FormatKind::PackedSkeleton => "fsk1",
        FormatKind::PackedMotion => "fmo1",
    }
}

impl From<FormatError> for DecodeError {
    /// `"<format>: <kind> at offset <n>"` (Plan/30 §14.2); the worker takes the text before the first
    /// colon as the `format` of its error response.
    fn from(e: FormatError) -> DecodeError {
        DecodeError(format!(
            "{}: {} at offset {}",
            format_name(e.format),
            e.kind,
            e.offset
        ))
    }
}

/// Rejects inputs above [`MAX_INPUT_BYTES`] (`len` is the byte length of the JS array).
pub fn check_input_len(kind: &str, len: usize) -> Result<(), DecodeError> {
    if len > MAX_INPUT_BYTES {
        return Err(DecodeError(format!(
            "{kind}: input of {len} bytes exceeds the decoder limit of {MAX_INPUT_BYTES} bytes"
        )));
    }
    Ok(())
}

fn warning_lines(warnings: &[FormatWarning]) -> impl Iterator<Item = String> + '_ {
    warnings
        .iter()
        .map(|w| format!("{:?}: {}", w.code, w.detail))
}

/// `decode_o3d` core: parses the file and builds the mesh pack. Parser warnings are appended to
/// `meta.warnings` after the pack warnings.
pub fn o3d_pack(bytes: &[u8], opts: &O3dPackOptions) -> Result<Packed<O3dMeta>, DecodeError> {
    check_input_len("o3d", bytes.len())?;
    let parsed = o3d::parse(
        bytes,
        &O3dOptions {
            file_name: opts.requested_name.clone(),
            ..O3dOptions::default()
        },
    )?;
    let mut packed = po3d::build(&parsed.value, opts)?;
    packed.meta.warnings.extend(warning_lines(&parsed.warnings));
    Ok(packed)
}

/// `decode_chr` core: the `FSK1` blob plus names, special bones and mapped attachment info.
pub fn chr_pack(bytes: &[u8], opts: &ChrPackOptions) -> Result<Packed<ChrMeta>, DecodeError> {
    check_input_len("chr", bytes.len())?;
    let parsed = chr::parse(bytes)?;
    Ok(pskel::pack_chr(&parsed.value, opts)?)
}

/// `decode_ani` core: the `FMO1` blob plus frames and hit-frame attributes.
pub fn ani_pack(bytes: &[u8]) -> Result<Packed<AniMeta>, DecodeError> {
    check_input_len("ani", bytes.len())?;
    let parsed = ani::parse(bytes, &AniOptions::default())?;
    Ok(motion::pack_ani(&parsed.value)?)
}

/// `decode_lnd` core: land grids, layer maps and placements as typed sections.
pub fn lnd_pack(bytes: &[u8], opts: &LndPackOptions) -> Result<Packed<LndMeta>, DecodeError> {
    check_input_len("lnd", bytes.len())?;
    let view = LandView::parse(bytes)?;
    Ok(plnd::build(&view, opts)?)
}

/// `decode_sfx` core: the effect description (no blob).
pub fn sfx_meta(bytes: &[u8]) -> Result<SfxMeta, DecodeError> {
    check_input_len("sfx", bytes.len())?;
    let parsed = sfx::parse(bytes)?;
    Ok(psfx::build(&parsed)?)
}

/// `decode_dds` / `decode_dds_bc` core: `mode` overrides `opts.bc`.
pub fn dds_pack(
    bytes: &[u8],
    opts: &DdsPackOptions,
    mode: BcMode,
) -> Result<Packed<DdsMeta>, DecodeError> {
    check_input_len("dds", bytes.len())?;
    let parsed = dds::view(bytes)?;
    Ok(pdds::build(&parsed, &DdsPackOptions { bc: mode, ..*opts })?)
}

/// The result of `formats_info()` (Plan/30 §14.1), camel-cased for JavaScript.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatsInfo {
    /// `flyff_formats::version::PARSERS`.
    pub parsers: u32,
    /// `flyff_formats::version::PACK`.
    pub pack: u32,
    /// `flyff_formats::version::BC_DECODE`.
    pub bc_decode: u32,
    /// `flyff_formats::version::BC_ENCODE`.
    pub bc_encode: u32,
    /// `flyff_formats::version::MIPS`.
    pub mips: u32,
    /// The size caps.
    pub limits: FormatsLimits,
}

/// The caps of [`FormatsInfo`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatsLimits {
    /// Largest accepted decoder input in bytes (32 MiB).
    pub max_input_bytes: u32,
    /// Largest accepted `width * height` of a decoded texture (16 Mi texels).
    pub max_texels: u32,
    /// Largest accepted texture side (8192).
    pub max_dim: u32,
}

/// The values `formats_info()` returns.
pub fn formats_info_value() -> FormatsInfo {
    FormatsInfo {
        parsers: version::PARSERS,
        pack: version::PACK,
        bc_decode: version::BC_DECODE,
        bc_encode: version::BC_ENCODE,
        mips: version::MIPS,
        limits: FormatsLimits {
            max_input_bytes: u32::try_from(MAX_INPUT_BYTES).unwrap_or(u32::MAX),
            max_texels: u32::try_from(texture::MAX_DECODE_TEXELS).unwrap_or(u32::MAX),
            max_dim: texture::MAX_DECODE_DIM,
        },
    }
}

// ---------------------------------------------------------------------------------------------------------
// JavaScript boundary
// ---------------------------------------------------------------------------------------------------------

fn js_error(e: DecodeError) -> JsError {
    JsError::new(&e.0)
}

/// Serialises `value` for JavaScript: `None` as `null`, maps as plain objects, no `BigInt`.
fn to_js<T: Serialize>(value: &T) -> Result<JsValue, JsError> {
    let serializer = serde_wasm_bindgen::Serializer::new()
        .serialize_missing_as_null(true)
        .serialize_maps_as_objects(true);
    value
        .serialize(&serializer)
        .map_err(|e| JsError::new(&format!("meta serialisation failed: {e}")))
}

/// `{ meta, blob }` with `blob` a fresh `Uint8Array` (or `null`).
fn result_object<M: Serialize>(meta: &M, blob: Option<&[u8]>) -> Result<JsValue, JsError> {
    let out = Object::new();
    let set = |key: &str, value: &JsValue| -> Result<(), JsError> {
        Reflect::set(&out, &JsValue::from_str(key), value)
            .map(|_| ())
            .map_err(|_| JsError::new("could not build the result object"))
    };
    set("meta", &to_js(meta)?)?;
    set(
        "blob",
        &blob.map_or(JsValue::NULL, |b| Uint8Array::from(b).into()),
    )?;
    Ok(out.into())
}

/// Options argument: `undefined` / `null` mean the defaults, an object fills the struct (missing fields keep
/// their defaults, unknown fields are ignored).
fn parse_opts<T: DeserializeOwned + Default>(opts: JsValue) -> Result<T, JsError> {
    if opts.is_undefined() || opts.is_null() {
        return Ok(T::default());
    }
    serde_wasm_bindgen::from_value(opts)
        .map_err(|e| JsError::new(&format!("invalid decoder options: {e}")))
}

/// Copies the JS array into wasm memory after the length check (so an oversized input never allocates).
fn input_bytes(kind: &str, bytes: &Uint8Array) -> Result<Vec<u8>, JsError> {
    check_input_len(kind, bytes.length() as usize).map_err(js_error)?;
    Ok(bytes.to_vec())
}

/// Decodes an `.o3d` mesh. `opts`: `{ space?: 'three' | 'lh', lods?: 'all' | 'lod0', collision?: boolean,
/// reject_name_mismatch?: boolean, requested_name?: string | null }` (Plan/30 §13.2).
///
/// Returns `{ meta: O3dMeta, blob }` (per object position/normal/uv/joints/weights/index/anim sections plus
/// the embedded `FSK1`/`FMO1` and the node-clip `FMO1` sections).
#[wasm_bindgen]
pub fn decode_o3d(bytes: &Uint8Array, opts: JsValue) -> Result<JsValue, JsError> {
    let opts: O3dPackOptions = parse_opts(opts)?;
    let input = input_bytes("o3d", bytes)?;
    let packed = o3d_pack(&input, &opts).map_err(js_error)?;
    result_object(&packed.meta, Some(&packed.blob))
}

/// Decodes a `.chr` skeleton. `opts`: `{ space?: 'three' | 'lh' }` (the space of the grips and events).
///
/// Returns `{ meta: ChrMeta, blob }`; the whole blob is one `FSK1` (`meta.skeleton = { off: 0, len:
/// blob.byteLength, .. }`), passed unchanged to `SimWorld.add_skeleton` (Plan/30 §13.4).
#[wasm_bindgen]
pub fn decode_chr(bytes: &Uint8Array, opts: JsValue) -> Result<JsValue, JsError> {
    let opts: ChrPackOptions = parse_opts(opts)?;
    let input = input_bytes("chr", bytes)?;
    let packed = chr_pack(&input, &opts).map_err(js_error)?;
    result_object(&packed.meta, Some(&packed.blob))
}

/// Decodes an `.ani` motion.
///
/// Returns `{ meta: AniMeta, blob }`; the whole blob is one `FMO1` (`meta.motion = { off: 0, len:
/// blob.byteLength, .. }`), passed unchanged to `SimWorld.add_clip` (Plan/30 §13.4).
#[wasm_bindgen]
pub fn decode_ani(bytes: &Uint8Array) -> Result<JsValue, JsError> {
    let input = input_bytes("ani", bytes)?;
    let packed = ani_pack(&input).map_err(js_error)?;
    result_object(&packed.meta, Some(&packed.blob))
}

/// Decodes a `.lnd` landscape. `opts`: `{ land_x?, land_z?, mpu?, normals? }` (the land indices of the file
/// name, world units per cell, per-vertex normals; Plan/30 §13.5).
///
/// Returns `{ meta: LndMeta, blob }` (heights, attributes, water, layer alpha, light RGB, optional normals
/// and the object/sfx placements as typed sections).
#[wasm_bindgen]
pub fn decode_lnd(bytes: &Uint8Array, opts: JsValue) -> Result<JsValue, JsError> {
    let opts: LndPackOptions = parse_opts(opts)?;
    let input = input_bytes("lnd", bytes)?;
    let packed = lnd_pack(&input, &opts).map_err(js_error)?;
    result_object(&packed.meta, Some(&packed.blob))
}

/// Decodes a `.sfx` effect prototype. Returns `{ meta: SfxMeta, blob: null }`.
#[wasm_bindgen]
pub fn decode_sfx(bytes: &Uint8Array) -> Result<JsValue, JsError> {
    let input = input_bytes("sfx", bytes)?;
    let meta = sfx_meta(&input).map_err(js_error)?;
    result_object(&meta, None)
}

/// Decodes a DDS texture for a GPU with `WEBGL_compressed_texture_s3tc`: BC blocks pass through, the
/// uncompressed formats are converted to their WebGL2 layouts. `opts`: `{ all_faces?: boolean }`.
///
/// Returns `{ meta: DdsMeta, blob }`.
#[wasm_bindgen]
pub fn decode_dds(bytes: &Uint8Array, opts: JsValue) -> Result<JsValue, JsError> {
    let opts: DdsPackOptions = parse_opts(opts)?;
    let input = input_bytes("dds", bytes)?;
    let packed = dds_pack(&input, &opts, BcMode::Passthrough).map_err(js_error)?;
    result_object(&packed.meta, Some(&packed.blob))
}

/// Decodes a DDS texture to RGBA8 (the fallback without S3TC). `opts`: `{ all_faces?: boolean }`.
///
/// Returns `{ meta: DdsMeta, blob }`.
#[wasm_bindgen]
pub fn decode_dds_bc(bytes: &Uint8Array, opts: JsValue) -> Result<JsValue, JsError> {
    let opts: DdsPackOptions = parse_opts(opts)?;
    let input = input_bytes("dds", bytes)?;
    let packed = dds_pack(&input, &opts, BcMode::Rgba8).map_err(js_error)?;
    result_object(&packed.meta, Some(&packed.blob))
}

/// `{ parsers, pack, bcDecode, bcEncode, mips, limits: { maxInputBytes, maxTexels, maxDim } }`: the version
/// constants of the format layer (folded into asset cache keys) and the decoder caps.
#[wasm_bindgen]
pub fn formats_info() -> JsValue {
    to_js(&formats_info_value()).unwrap_or(JsValue::NULL)
}
