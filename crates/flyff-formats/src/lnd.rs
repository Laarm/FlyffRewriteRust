//! `.lnd` landscape files (Plan/30 §8; ref/19 §6, §15; client `CLandscape::LoadLandscape`
//! src:_Common/lod.cpp:1266-1425, server `CWorld::ReadWorld` src:_Common/WorldFile.cpp:823-955).
//!
//! Layout (little-endian, no padding): `u32 version` (>= 1: `i32 xLand, i32 yLand`), `f32[129*129]` heights
//! (attribute flags encoded, see [`HeightAttr`]), `{u8 height, u8 tex}[16*16]` water, `u8[16*16]` location
//! map (version >= 2, unused by the game), `u8 nLayer`, per layer `u16 texture id + i32[16*16] patch enable +
//! u32[128*128] ARGB light/alpha map`, `u32 nObj` records, and an optional `u32 nSfx` with records (the client
//! reads it only if bytes remain; the server never reads it).
//!
//! Two entry points share one structural scan: [`parse`] builds the owned [`LandFile`] model (with
//! [`LayerMode::Skip`] the 66,562 bytes per layer are never copied), and [`LandView`] is the zero-copy
//! view used by the browser decoders and the collision-chunk builder.
//!
//! Consumer facts (not parser behaviour): neighbouring lands share edge vertices; the server keeps only
//! columns/rows 0..127 of each land, so the neighbour's column 0 wins (KEEP-QUIRK Q11, applied by the chunk
//! builder). The light map RGB is identical across all layers of a land and texels 113..127 are filler
//! (ref/19 §16.3).

use crate::FormatKind;
use crate::error::{ErrorKind, FormatError, FormatWarning, Parsed, WarningCode};
use crate::limits::MAX_LAND_OBJECTS;
use crate::objects::{MIN_RECORD_SIZE, ObjRecord, RecordIter, Slot, skip_record};
use crate::reader::ByteReader;

/// Cells per land side (`MAP_SIZE`, landscape.h:6).
pub const MAP_SIZE: usize = 128;
/// Height vertices per land side (`MAP_SIZE + 1`).
pub const HEIGHT_DIM: usize = MAP_SIZE + 1;
/// Height vertices per land (129 x 129 = 16,641).
pub const HEIGHT_COUNT: usize = HEIGHT_DIM * HEIGHT_DIM;
/// Patches per land side (`NUM_PATCHES_PER_SIDE`, landscape.h:7).
pub const PATCHES_PER_SIDE: usize = 16;
/// Patches per land (256).
pub const PATCH_COUNT: usize = PATCHES_PER_SIDE * PATCHES_PER_SIDE;
/// Light/alpha map texels per side (the texture is 128 x 128; only 112 are effective).
pub const LIGHTMAP_DIM: usize = MAP_SIZE;
/// Light/alpha map texels per layer (16,384).
pub const LIGHTMAP_TEXELS: usize = LIGHTMAP_DIM * LIGHTMAP_DIM;
/// Bytes of one layer: `u16` texture id + 256 `i32` patch flags + 16,384 `u32` texels = 66,562.
pub const LAYER_SIZE: usize = 2 + PATCH_COUNT * 4 + LIGHTMAP_TEXELS * 4;
/// Highest `version` the parser accepts (corpus: 2 and 3).
pub const MAX_VERSION: u32 = 3;

/// `HGT_NOWALK`: first raw height with the no-walk attribute (landscape.h:33).
pub const HGT_NOWALK: f32 = 1000.0;
/// `HGT_NOFLY`.
pub const HGT_NOFLY: f32 = 2000.0;
/// `HGT_NOMOVE`.
pub const HGT_NOMOVE: f32 = 3000.0;
/// `HGT_DIE`.
pub const HGT_DIE: f32 = 4000.0;

/// `WTYPE_NONE`.
pub const WTYPE_NONE: u8 = 0x00;
/// `WTYPE_CLOUD` (compared with `==`, unmasked, everywhere in the original: KEEP-QUIRK Q10).
pub const WTYPE_CLOUD: u8 = 0x01;
/// `WTYPE_WATER`.
pub const WTYPE_WATER: u8 = 0x02;

// ---------------------------------------------------------------------------------------------------
// Height attributes (§8.2)
// ---------------------------------------------------------------------------------------------------

/// The terrain attribute carried by a raw height (`HATTR_*`, landscape.h:29-38).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum HeightAttr {
    /// `HATTR_NONE`: normal terrain (raw `< 1000`).
    None = 0,
    /// `HATTR_NOWALK`: no walking or jumping, flying allowed (`[1000, 2000)`).
    NoWalk = 1,
    /// `HATTR_NOFLY`: no flying, dismounts flyers (`[2000, 3000)`).
    NoFly = 2,
    /// `HATTR_NOMOVE`: neither walking nor flying (`[3000, 4000)`).
    NoMove = 3,
    /// `HATTR_DIE`: entering kills (`>= 4000`).
    Die = 4,
}

impl HeightAttr {
    /// Classifies a raw height: `>= 4000` Die, `>= 3000` NoMove, `>= 2000` NoFly, `>= 1000` NoWalk, else
    /// None (NaN is None). Mirrors `CWorld::GetHeightAttribute` (World.cpp:1732-1777).
    pub fn classify(raw: f32) -> HeightAttr {
        if raw >= HGT_DIE {
            HeightAttr::Die
        } else if raw >= HGT_NOMOVE {
            HeightAttr::NoMove
        } else if raw >= HGT_NOFLY {
            HeightAttr::NoFly
        } else if raw >= HGT_NOWALK {
            HeightAttr::NoWalk
        } else {
            HeightAttr::None
        }
    }

    /// The attribute from its `HATTR_*` number (0..=4).
    pub fn from_u8(v: u8) -> Option<HeightAttr> {
        match v {
            0 => Some(HeightAttr::None),
            1 => Some(HeightAttr::NoWalk),
            2 => Some(HeightAttr::NoFly),
            3 => Some(HeightAttr::NoMove),
            4 => Some(HeightAttr::Die),
            _ => None,
        }
    }
}

/// Server `GetLandHeight` stripping, bit-exact (World.cpp:1008-1027):
/// `if raw >= 1000.0 { let n = (raw / 1000.0) as i32; raw - 1000.0 * n as f32 } else { raw }`.
///
/// `flyff-sim` and the collision-chunk builder MUST use this rule (server authority; both sides run the same
/// simulation). Identical to [`strip_height_client`] for `raw < 5000`.
pub fn strip_height_server(raw: f32) -> f32 {
    if raw >= HGT_NOWALK {
        let n = (raw / HGT_NOWALK) as i32;
        raw - HGT_NOWALK * n as f32
    } else {
        raw
    }
}

/// Client `CLandscape::GetHeightMap`: raw minus the constant of its range (lod.cpp:1657-1670).
pub fn strip_height_client(raw: f32) -> f32 {
    if raw >= HGT_NOWALK {
        if raw >= HGT_DIE {
            return raw - HGT_DIE;
        }
        if raw >= HGT_NOMOVE {
            return raw - HGT_NOMOVE;
        }
        if raw >= HGT_NOFLY {
            return raw - HGT_NOFLY;
        }
        return raw - HGT_NOWALK;
    }
    raw
}

/// `(attribute, real height)` of a raw height: [`HeightAttr::classify`] and [`strip_height_server`].
pub fn decode_height(raw: f32) -> (HeightAttr, f32) {
    (HeightAttr::classify(raw), strip_height_server(raw))
}

// ---------------------------------------------------------------------------------------------------
// Water (§8.3)
// ---------------------------------------------------------------------------------------------------

/// `WATERHEIGHT` of one patch (landscape.h:88-93).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct WaterPatch {
    /// `byWaterHeight`: water surface Y in integer world units.
    pub height: u8,
    /// `byWaterTexture`: bits 0-1 type, bits 2-7 animation set.
    pub tex: u8,
}

impl WaterPatch {
    /// `tex == WTYPE_CLOUD`, compared unmasked like the original (KEEP-QUIRK Q10).
    pub fn is_cloud(&self) -> bool {
        is_cloud(self.tex)
    }

    /// Bits 0-1 of `tex`: [`WTYPE_NONE`], [`WTYPE_CLOUD`] or [`WTYPE_WATER`] (3 is invalid but kept).
    pub fn water_type(&self) -> u8 {
        water_type(self.tex)
    }

    /// Bits 2-7 of `tex`: the water animation set (0 = river set, 1 = Harmonin set).
    pub fn water_set(&self) -> u8 {
        water_set(self.tex)
    }
}

/// `tex == WTYPE_CLOUD` (unmasked: `5` is not a cloud although its type bits are 1).
pub fn is_cloud(tex: u8) -> bool {
    tex == WTYPE_CLOUD
}

/// `tex & 0x03`.
pub fn water_type(tex: u8) -> u8 {
    tex & 0x03
}

/// `tex >> 2`.
pub fn water_set(tex: u8) -> u8 {
    tex >> 2
}

// ---------------------------------------------------------------------------------------------------
// Owned model (§8.4)
// ---------------------------------------------------------------------------------------------------

/// Whether the per-layer data is read or seeked over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LayerMode {
    /// Copy the layers into [`Layers::Loaded`].
    #[default]
    Load,
    /// Seek over 66,562 bytes per layer (the server does this); [`Layers::Skipped`] keeps the count.
    Skip,
}

/// Options of [`parse`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LandOptions {
    /// Layer handling.
    pub layers: LayerMode,
}

/// A parsed `.lnd`, a faithful image of the file (Plan/30 §8.4). `Debug` prints a summary (the bulk
/// vectors are thousands of values long).
#[derive(Clone, PartialEq)]
pub struct LandFile {
    /// `version` word (0..=3).
    pub version: u32,
    /// `(xLand, yLand)` from the header; `None` for version 0.
    pub land_xz: Option<(i32, i32)>,
    /// 16,641 raw heights `h[z * 129 + x]`, attribute flags included.
    pub heights: Vec<f32>,
    /// 256 patches, index `px + pz * 16`.
    pub water: Vec<WaterPatch>,
    /// The unused location map (version >= 2), kept raw (often `0xCD` heap garbage).
    pub location: Option<[u8; PATCH_COUNT]>,
    /// The splat layers.
    pub layers: Layers,
    /// `nObj` static objects (`OT_OBJ` in all data).
    pub objects: Vec<ObjRecord>,
    /// Static sfx (`OT_SFX` in all data); `None` when the optional count field is absent.
    pub sfx: Option<Vec<ObjRecord>>,
}

impl core::fmt::Debug for LandFile {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LandFile")
            .field("version", &self.version)
            .field("land_xz", &self.land_xz)
            .field("heights", &self.heights.len())
            .field("water", &self.water.len())
            .field("location", &self.location.is_some())
            .field("layers", &self.layers.count())
            .field("objects", &self.objects.len())
            .field("sfx", &self.sfx.as_ref().map(Vec::len))
            .finish()
    }
}

impl core::fmt::Debug for LandLayer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LandLayer")
            .field("texture_id", &self.texture_id)
            .finish_non_exhaustive()
    }
}

impl LandFile {
    /// Raw height at vertex `(x, z)`, both in `0..=128`.
    pub fn height_raw(&self, x: usize, z: usize) -> Option<f32> {
        if x >= HEIGHT_DIM || z >= HEIGHT_DIM {
            return None;
        }
        self.heights.get(z * HEIGHT_DIM + x).copied()
    }

    /// Water patch `(px, pz)`, both in `0..=15`.
    pub fn water_at(&self, px: usize, pz: usize) -> Option<WaterPatch> {
        if px >= PATCHES_PER_SIDE || pz >= PATCHES_PER_SIDE {
            return None;
        }
        self.water.get(px + pz * PATCHES_PER_SIDE).copied()
    }

    /// Number of layers in the file (also when skipped).
    pub fn layer_count(&self) -> usize {
        self.layers.count()
    }
}

/// The layers of a land: loaded, or only counted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Layers {
    /// All layers, in file order.
    Loaded(Vec<LandLayer>),
    /// [`LayerMode::Skip`]: the bytes were seeked over.
    Skipped {
        /// `nLayer` from the file.
        count: u8,
    },
}

impl Layers {
    /// Number of layers.
    pub fn count(&self) -> usize {
        match self {
            Layers::Loaded(v) => v.len(),
            Layers::Skipped { count } => usize::from(*count),
        }
    }
}

/// One terrain layer (`CLandLayer`). `Debug` prints the texture id only.
#[derive(Clone, PartialEq, Eq)]
pub struct LandLayer {
    /// Terrain texture id (key into `Terrain.inc`).
    pub texture_id: u16,
    /// 256 `BOOL` patch-enable flags as stored (`px + pz * 16`).
    pub patch_enable: Vec<i32>,
    /// 16,384 `A8R8G8B8` texels (`z * 128 + x`); the `u32` value is `A << 24 | R << 16 | G << 8 | B`
    /// (memory order B, G, R, A).
    pub light_map: Vec<u32>,
}

/// Parses a `.lnd` into the owned model. Warnings: `TrailingBytes` when bytes remain after the last record
/// (1 to 3 bytes after the objects are the "sfx count absent" case plus trailing bytes).
pub fn parse(b: &[u8], opts: &LandOptions) -> Result<Parsed<LandFile>, FormatError> {
    let view = LandView::parse(b)?;
    let value = view.to_owned_with(opts)?;
    let mut warnings = Vec::new();
    if view.trailing_bytes() > 0 {
        warnings.push(FormatWarning::new(
            view.end,
            WarningCode::TrailingBytes,
            format!("{} byte(s) after the last record", view.trailing_bytes()),
        ));
    }
    Ok(Parsed { value, warnings })
}

/// `LandHeaderMismatch` when the header coordinates differ from the file name's (`Name%02d-%02d.lnd`,
/// first number X, second Z; ref/19 §6). `None` for version 0 (no header) and for matching names.
pub fn check_header(file: &LandFile, name_x: i32, name_z: i32) -> Option<FormatWarning> {
    let (x, z) = file.land_xz?;
    if (x, z) == (name_x, name_z) {
        return None;
    }
    Some(FormatWarning::new(
        4,
        WarningCode::LandHeaderMismatch,
        format!("header says land ({x}, {z}) but the file name says ({name_x}, {name_z})"),
    ))
}

/// Extracts `(x, z)` from a land file name `<world_stem>XX-ZZ.lnd` (any directory prefix; `world_stem` and
/// the extension are matched ASCII case-insensitively). The world stem is a parameter because world names
/// may themselves end in digits (`DuOminous_1` + `00-00` is `DuOminous_100-00.lnd`). `None` when the name
/// does not have exactly that shape.
pub fn coords_from_file_name(path: &str, world_stem: &str) -> Option<(i32, i32)> {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let stem_len = name.len().checked_sub(4)?;
    if !name.get(stem_len..)?.eq_ignore_ascii_case(".lnd") {
        return None;
    }
    let stem = name.get(..stem_len)?;
    let prefix = stem.get(..world_stem.len())?;
    if !prefix.eq_ignore_ascii_case(world_stem) {
        return None;
    }
    let (x_digits, z_digits) = stem.get(world_stem.len()..)?.split_once('-')?;
    let all_digits = |s: &str| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit());
    if !all_digits(x_digits) || !all_digits(z_digits) {
        return None;
    }
    Some((x_digits.parse().ok()?, z_digits.parse().ok()?))
}

// ---------------------------------------------------------------------------------------------------
// Zero-copy view (§8.4)
// ---------------------------------------------------------------------------------------------------

/// A validated, zero-copy view of a `.lnd`: the structure was scanned once (including the extent of every
/// object record), the bulk data stays in the borrowed input. Cheap to copy.
#[derive(Clone)]
pub struct LandView<'a> {
    buf: &'a [u8],
    version: u32,
    land_xz: Option<(i32, i32)>,
    heights_off: usize,
    water_off: usize,
    location_off: Option<usize>,
    layer_count: usize,
    layers_off: usize,
    object_count: usize,
    objects_off: usize,
    /// `(count, offset of the first record)` of the optional sfx section.
    sfx: Option<(usize, usize)>,
    end: usize,
}

impl core::fmt::Debug for LandView<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LandView")
            .field("len", &self.buf.len())
            .field("version", &self.version)
            .field("land_xz", &self.land_xz)
            .field("layers", &self.layer_count)
            .field("objects", &self.object_count)
            .field("sfx", &self.sfx.map(|(n, _)| n))
            .field("trailing_bytes", &self.trailing_bytes())
            .finish()
    }
}

impl<'a> LandView<'a> {
    /// Scans `b`: header, fixed sections, `nLayer` layers (seeked over), the object records and the optional
    /// sfx records. Bytes after the last record are tolerated (see [`LandView::trailing_bytes`]).
    pub fn parse(b: &'a [u8]) -> Result<LandView<'a>, FormatError> {
        let mut r = ByteReader::new(b, FormatKind::Lnd);
        let version = r.u32()?;
        if version > MAX_VERSION {
            return Err(r.err_at(
                0,
                ErrorKind::UnsupportedVersion {
                    found: i64::from(version),
                },
            ));
        }
        let land_xz = if version >= 1 {
            let x = r.i32()?;
            let z = r.i32()?;
            Some((x, z))
        } else {
            None
        };
        let heights_off = r.pos();
        r.skip(HEIGHT_COUNT * 4)?;
        let water_off = r.pos();
        r.skip(PATCH_COUNT * 2)?;
        let location_off = if version >= 2 {
            let off = r.pos();
            r.skip(PATCH_COUNT)?;
            Some(off)
        } else {
            None
        };
        let layer_count = usize::from(r.u8()?);
        let layers_off = r.pos();
        r.skip(layer_count * LAYER_SIZE)?;

        let object_count = r.count("nObj", MIN_RECORD_SIZE, MAX_LAND_OBJECTS)?;
        let objects_off = r.pos();
        skip_records(&mut r, object_count)?;

        // The client reads the sfx count only if bytes remain; fewer than 4 bytes are not a count.
        let sfx = if r.remaining() >= 4 {
            let n = r.count("nSfx", MIN_RECORD_SIZE, MAX_LAND_OBJECTS)?;
            let off = r.pos();
            skip_records(&mut r, n)?;
            Some((n, off))
        } else {
            None
        };
        Ok(LandView {
            buf: b,
            version,
            land_xz,
            heights_off,
            water_off,
            location_off,
            layer_count,
            layers_off,
            object_count,
            objects_off,
            sfx,
            end: r.pos(),
        })
    }

    /// The `version` word.
    pub fn version(&self) -> u32 {
        self.version
    }

    /// `(xLand, yLand)` from the header (`None` for version 0).
    pub fn land_xz(&self) -> Option<(i32, i32)> {
        self.land_xz
    }

    /// Bytes after the last parsed record (0 for every real file).
    pub fn trailing_bytes(&self) -> usize {
        self.buf.len().saturating_sub(self.end)
    }

    fn slice(&self, off: usize, len: usize) -> Option<&'a [u8]> {
        self.buf.get(off..off.checked_add(len)?)
    }

    /// The raw height section: 66,564 little-endian bytes (`h[z * 129 + x]`). The slice may be unaligned
    /// within the input, so typed-array consumers copy it (or use [`LandView::heights`]).
    pub fn heights_bytes(&self) -> &'a [u8] {
        self.slice(self.heights_off, HEIGHT_COUNT * 4)
            .unwrap_or(&[])
    }

    /// The 16,641 raw heights in file order (`h[z * 129 + x]`, attribute flags included), without
    /// copying the section.
    pub fn heights(&self) -> impl ExactSizeIterator<Item = f32> + 'a {
        let (chunks, _) = self.heights_bytes().as_chunks::<4>();
        chunks
            .iter()
            .map(|c| f32::from_bits(u32::from_le_bytes(*c)))
    }

    /// Raw height at vertex `(x, z)`, both in `0..=128` (attribute flags included).
    pub fn height_raw(&self, x: usize, z: usize) -> Option<f32> {
        if x >= HEIGHT_DIM || z >= HEIGHT_DIM {
            return None;
        }
        let at = (z * HEIGHT_DIM + x) * 4;
        let bytes = <[u8; 4]>::try_from(self.heights_bytes().get(at..at + 4)?).ok()?;
        Some(f32::from_bits(u32::from_le_bytes(bytes)))
    }

    /// The raw water section: 512 bytes of `{height, tex}` pairs.
    pub fn water_bytes(&self) -> &'a [u8] {
        self.slice(self.water_off, PATCH_COUNT * 2).unwrap_or(&[])
    }

    /// Water patch `(px, pz)`, both in `0..=15`.
    pub fn water(&self, px: usize, pz: usize) -> Option<WaterPatch> {
        if px >= PATCHES_PER_SIDE || pz >= PATCHES_PER_SIDE {
            return None;
        }
        let at = (px + pz * PATCHES_PER_SIDE) * 2;
        match self.water_bytes().get(at..at + 2)? {
            [height, tex] => Some(WaterPatch {
                height: *height,
                tex: *tex,
            }),
            _ => None,
        }
    }

    /// The 256-byte location map (version >= 2).
    pub fn location(&self) -> Option<&'a [u8]> {
        self.slice(self.location_off?, PATCH_COUNT)
    }

    /// Number of layers.
    pub fn layer_count(&self) -> usize {
        self.layer_count
    }

    fn layer_bytes(&self, layer: usize) -> Option<&'a [u8]> {
        if layer >= self.layer_count {
            return None;
        }
        self.slice(self.layers_off + layer * LAYER_SIZE, LAYER_SIZE)
    }

    /// Terrain texture id of `layer`.
    pub fn layer_texture_id(&self, layer: usize) -> Option<u16> {
        match self.layer_bytes(layer)?.get(..2)? {
            [lo, hi] => Some(u16::from_le_bytes([*lo, *hi])),
            _ => None,
        }
    }

    /// The 1,024 raw bytes of `layer`'s 256 `i32` patch-enable flags.
    pub fn layer_patch_enable_bytes(&self, layer: usize) -> Option<&'a [u8]> {
        self.layer_bytes(layer)?.get(2..2 + PATCH_COUNT * 4)
    }

    /// Patch-enable flag of `layer` at patch `(px, pz)` (`BOOL` as stored).
    pub fn patch_enable(&self, layer: usize, px: usize, pz: usize) -> Option<i32> {
        if px >= PATCHES_PER_SIDE || pz >= PATCHES_PER_SIDE {
            return None;
        }
        let at = (px + pz * PATCHES_PER_SIDE) * 4;
        let bytes =
            <[u8; 4]>::try_from(self.layer_patch_enable_bytes(layer)?.get(at..at + 4)?).ok()?;
        Some(i32::from_le_bytes(bytes))
    }

    /// The 65,536 raw light-map bytes of `layer` (B, G, R, A per texel, `z * 128 + x`).
    pub fn layer_light_bytes(&self, layer: usize) -> Option<&'a [u8]> {
        self.layer_bytes(layer)?.get(2 + PATCH_COUNT * 4..)
    }

    fn layer_error(&self, layer: usize) -> FormatError {
        FormatError::new(
            FormatKind::Lnd,
            self.layers_off,
            ErrorKind::InvalidValue {
                field: "layer",
                value: layer as i64,
            },
        )
    }

    fn out_error(&self, got: usize) -> FormatError {
        FormatError::new(
            FormatKind::Lnd,
            self.layers_off,
            ErrorKind::InvalidValue {
                field: "output length",
                value: got as i64,
            },
        )
    }

    /// Writes the A channel of `layer`'s light map (the splat alpha) into `out` (exactly 16,384 bytes).
    pub fn layer_alpha(&self, layer: usize, out: &mut [u8]) -> Result<(), FormatError> {
        let light = self
            .layer_light_bytes(layer)
            .ok_or_else(|| self.layer_error(layer))?;
        if out.len() != LIGHTMAP_TEXELS {
            return Err(self.out_error(out.len()));
        }
        let (texels, _) = light.as_chunks::<4>();
        for (dst, &[_b, _g, _r, a]) in out.iter_mut().zip(texels) {
            *dst = a;
        }
        Ok(())
    }

    /// Writes the R, G, B channels of `layer`'s light map into `out` (exactly 49,152 bytes, 3 per texel).
    pub fn light_rgb(&self, layer: usize, out: &mut [u8]) -> Result<(), FormatError> {
        let light = self
            .layer_light_bytes(layer)
            .ok_or_else(|| self.layer_error(layer))?;
        if out.len() != LIGHTMAP_TEXELS * 3 {
            return Err(self.out_error(out.len()));
        }
        let (texels, _) = light.as_chunks::<4>();
        let (dst_texels, _) = out.as_chunks_mut::<3>();
        for (dst, &[b, g, r, _a]) in dst_texels.iter_mut().zip(texels) {
            *dst = [r, g, b];
        }
        Ok(())
    }

    /// Number of object records.
    pub fn object_count(&self) -> usize {
        self.object_count
    }

    /// Iterates the object records (parsed lazily; the structure was validated by [`LandView::parse`]).
    pub fn objects(&self) -> RecordIter<'a> {
        RecordIter::new(
            self.buf,
            FormatKind::Lnd,
            self.objects_off,
            self.object_count,
        )
    }

    /// Number of static sfx records; `None` when the count field is absent.
    pub fn sfx_count(&self) -> Option<usize> {
        self.sfx.map(|(n, _)| n)
    }

    /// Iterates the static sfx records (empty when the section is absent).
    pub fn sfx(&self) -> RecordIter<'a> {
        let (n, off) = self.sfx.unwrap_or((0, 0));
        RecordIter::new(self.buf, FormatKind::Lnd, off, n)
    }

    /// Builds the owned model with all layers loaded.
    pub fn to_owned(&self) -> Result<LandFile, FormatError> {
        self.to_owned_with(&LandOptions::default())
    }

    /// Builds the owned model; [`LayerMode::Skip`] leaves the layer bytes untouched.
    pub fn to_owned_with(&self, opts: &LandOptions) -> Result<LandFile, FormatError> {
        let mut r = ByteReader::new(self.buf, FormatKind::Lnd);
        r.seek(self.heights_off)?;
        let heights = r.f32_vec(HEIGHT_COUNT)?;
        let (water_pairs, _) = r.bytes(PATCH_COUNT * 2)?.as_chunks::<2>();
        let water = water_pairs
            .iter()
            .map(|&[height, tex]| WaterPatch { height, tex })
            .collect();
        let location = match self.location_off {
            Some(off) => {
                r.seek(off)?;
                let bytes = r.bytes(PATCH_COUNT)?;
                let loc = <[u8; PATCH_COUNT]>::try_from(bytes).map_err(|_| {
                    r.err(ErrorKind::Inconsistent {
                        what: "location map length",
                    })
                })?;
                Some(loc)
            }
            None => None,
        };
        let layers = match opts.layers {
            LayerMode::Skip => Layers::Skipped {
                count: u8::try_from(self.layer_count).unwrap_or(u8::MAX),
            },
            LayerMode::Load => {
                let mut layers = Vec::with_capacity(self.layer_count);
                for i in 0..self.layer_count {
                    r.seek(self.layers_off + i * LAYER_SIZE)?;
                    layers.push(LandLayer {
                        texture_id: r.u16()?,
                        patch_enable: r.i32_vec(PATCH_COUNT)?,
                        light_map: r.u32_vec(LIGHTMAP_TEXELS)?,
                    });
                }
                Layers::Loaded(layers)
            }
        };
        let objects = collect_records(self.objects(), self.object_count)?;
        let sfx = match self.sfx {
            Some((n, _)) => Some(collect_records(self.sfx(), n)?),
            None => None,
        };
        Ok(LandFile {
            version: self.version,
            land_xz: self.land_xz,
            heights,
            water,
            location,
            layers,
            objects,
            sfx,
        })
    }
}

/// Collects `count` records. `count` was validated against the remaining input by the scan, so the
/// reservation is bounded by the input size.
fn collect_records(iter: RecordIter<'_>, count: usize) -> Result<Vec<ObjRecord>, FormatError> {
    let mut out = Vec::with_capacity(count);
    for rec in iter {
        out.push(rec?);
    }
    Ok(out)
}

fn skip_records(r: &mut ByteReader<'_>, count: usize) -> Result<(), FormatError> {
    for _ in 0..count {
        let at = r.pos();
        match skip_record(r)? {
            Slot::Record(_) => {}
            Slot::End(word) => {
                return Err(r.err_at(
                    at,
                    ErrorKind::InvalidValue {
                        field: "object type",
                        value: i64::from(word),
                    },
                ));
            }
        }
    }
    Ok(())
}
