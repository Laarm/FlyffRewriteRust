//! DDS textures (Plan/30 §11.1; ref/18 §9): header parsing, format identification, level layout, cube maps and
//! partial mip chains.
//!
//! [`view`] is the zero-copy parser (level data borrows the input); [`parse`] copies it into an owned [`Dds`].
//! Pixel decoding lives in [`crate::texture`] (`decode_level`, `to_gl_upload`) and [`crate::bc`].
//!
//! Corpus facts (measured over all 11,903 `.dds`): 11,902 are valid (the odd one out is an `SFX0.` file and
//! fails with `BadMagic`), `dwSize` is always 124 and `pf.dwSize` 32, rows are tightly packed, there is exactly
//! one cube map (`Model/Texture/Env.dds`) and three partial mip chains (`DdsPartialMipChain`). The
//! `pf.dwRGBBitCount` field of FourCC files is garbage in 660 files (`0x100`, `0x200`) and is ignored there.

use crate::FormatKind;
use crate::error::{ErrorKind, FormatError, FormatWarning, Parsed, WarningCode};
use crate::limits::{MAX_DDS_DIM, max_mip_levels};
use crate::reader::ByteReader;

/// The file magic `"DDS "`.
pub const MAGIC: [u8; 4] = *b"DDS ";
/// `DDS_HEADER.dwSize`.
pub const HEADER_SIZE: u32 = 124;
/// `DDS_PIXELFORMAT.dwSize`.
pub const PIXELFORMAT_SIZE: u32 = 32;
/// Offset of the first pixel byte (magic + header).
pub const DATA_OFFSET: usize = 128;

/// `dwFlags`: `DDSD_CAPS`.
pub const DDSD_CAPS: u32 = 0x1;
/// `dwFlags`: `DDSD_HEIGHT`.
pub const DDSD_HEIGHT: u32 = 0x2;
/// `dwFlags`: `DDSD_WIDTH`.
pub const DDSD_WIDTH: u32 = 0x4;
/// `dwFlags`: `DDSD_PITCH`.
pub const DDSD_PITCH: u32 = 0x8;
/// `dwFlags`: `DDSD_PIXELFORMAT`.
pub const DDSD_PIXELFORMAT: u32 = 0x1000;
/// `dwFlags`: `DDSD_MIPMAPCOUNT`.
pub const DDSD_MIPMAPCOUNT: u32 = 0x2_0000;
/// `dwFlags`: `DDSD_LINEARSIZE`.
pub const DDSD_LINEARSIZE: u32 = 0x8_0000;
/// `pf.dwFlags`: `DDPF_ALPHAPIXELS`.
pub const DDPF_ALPHAPIXELS: u32 = 0x1;
/// `pf.dwFlags`: `DDPF_FOURCC`.
pub const DDPF_FOURCC: u32 = 0x4;
/// `pf.dwFlags`: `DDPF_RGB`.
pub const DDPF_RGB: u32 = 0x40;
/// `dwCaps`: `DDSCAPS_COMPLEX`.
pub const DDSCAPS_COMPLEX: u32 = 0x8;
/// `dwCaps`: `DDSCAPS_TEXTURE`.
pub const DDSCAPS_TEXTURE: u32 = 0x1000;
/// `dwCaps`: `DDSCAPS_MIPMAP`.
pub const DDSCAPS_MIPMAP: u32 = 0x40_0000;
/// `dwCaps2`: `DDSCAPS2_CUBEMAP`.
pub const DDSCAPS2_CUBEMAP: u32 = 0x200;
/// `dwCaps2`: the six cube-face bits (`POSITIVEX` .. `NEGATIVEZ`).
pub const DDSCAPS2_CUBEMAP_ALLFACES: u32 = 0xFC00;
/// `dwCaps2`: `DDSCAPS2_VOLUME`.
pub const DDSCAPS2_VOLUME: u32 = 0x20_0000;

/// Offset of `dwMipMapCount` in the file (for diagnostics).
const OFF_MIP_COUNT: usize = 28;
/// Offset of `DDS_PIXELFORMAT` in the file (for diagnostics).
const OFF_PIXEL_FORMAT: usize = 76;

/// `DDS_PIXELFORMAT` (32 bytes), stored as in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PixelFormat {
    /// `dwSize` (32).
    pub size: u32,
    /// `dwFlags` (`DDPF_*`).
    pub flags: u32,
    /// `dwFourCC` as the four file bytes (`b"DXT1"`).
    pub four_cc: [u8; 4],
    /// `dwRGBBitCount` (meaningless for FourCC files).
    pub rgb_bit_count: u32,
    /// `dwRBitMask`, `dwGBitMask`, `dwBBitMask`, `dwABitMask`.
    pub masks: [u32; 4],
}

/// `DDS_HEADER` (124 bytes after the magic), stored as in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DdsHeader {
    /// `dwSize` (124).
    pub size: u32,
    /// `dwFlags` (`DDSD_*`).
    pub flags: u32,
    /// `dwHeight`.
    pub height: u32,
    /// `dwWidth`.
    pub width: u32,
    /// `dwPitchOrLinearSize`.
    pub pitch_or_linear_size: u32,
    /// `dwDepth`.
    pub depth: u32,
    /// `dwMipMapCount`.
    pub mip_map_count: u32,
    /// `dwReserved1[11]`.
    pub reserved1: [u32; 11],
    /// `ddspf`.
    pub pf: PixelFormat,
    /// `dwCaps`.
    pub caps: u32,
    /// `dwCaps2`.
    pub caps2: u32,
    /// `dwCaps3`.
    pub caps3: u32,
    /// `dwCaps4`.
    pub caps4: u32,
    /// `dwReserved2`.
    pub reserved2: u32,
}

impl DdsHeader {
    /// A header like the ones D3DX writes for a 2-D texture of `format` with `levels` mip levels
    /// (compressed: `LINEARSIZE`, uncompressed: `PITCH`; `COMPLEX | MIPMAP` when `levels > 1`).
    pub fn new(width: u32, height: u32, format: DdsFormat, levels: u32) -> DdsHeader {
        let mut flags = DDSD_CAPS | DDSD_HEIGHT | DDSD_WIDTH | DDSD_PIXELFORMAT;
        let mut caps = DDSCAPS_TEXTURE;
        let level0 = u32::try_from(format.level_size(width, height)).unwrap_or(u32::MAX);
        let pitch_or_linear_size = match format.bytes_per_pixel() {
            Some(bpp) => {
                flags |= DDSD_PITCH;
                width.saturating_mul(u32::try_from(bpp).unwrap_or(4))
            }
            None => {
                flags |= DDSD_LINEARSIZE;
                level0
            }
        };
        if levels > 1 {
            flags |= DDSD_MIPMAPCOUNT;
            caps |= DDSCAPS_COMPLEX | DDSCAPS_MIPMAP;
        }
        DdsHeader {
            size: HEADER_SIZE,
            flags,
            height,
            width,
            pitch_or_linear_size,
            depth: 0,
            mip_map_count: if levels > 1 { levels } else { 0 },
            reserved1: [0; 11],
            pf: format.pixel_format(),
            caps,
            caps2: 0,
            caps3: 0,
            caps4: 0,
            reserved2: 0,
        }
    }

    /// Reads the 124 header bytes that follow the magic.
    fn read(r: &mut ByteReader<'_>) -> Result<DdsHeader, FormatError> {
        let size = r.u32()?;
        if size != HEADER_SIZE {
            return Err(r.err_at(
                4,
                ErrorKind::InvalidValue {
                    field: "dwSize",
                    value: i64::from(size),
                },
            ));
        }
        let flags = r.u32()?;
        let height = r.u32()?;
        let width = r.u32()?;
        let pitch_or_linear_size = r.u32()?;
        let depth = r.u32()?;
        let mip_map_count = r.u32()?;
        let mut reserved1 = [0u32; 11];
        for slot in &mut reserved1 {
            *slot = r.u32()?;
        }
        let pf_size = r.u32()?;
        if pf_size != PIXELFORMAT_SIZE {
            return Err(r.err_at(
                OFF_PIXEL_FORMAT,
                ErrorKind::InvalidValue {
                    field: "pf.dwSize",
                    value: i64::from(pf_size),
                },
            ));
        }
        let pf_flags = r.u32()?;
        let four_cc = r.u32()?.to_le_bytes();
        let rgb_bit_count = r.u32()?;
        let masks = [r.u32()?, r.u32()?, r.u32()?, r.u32()?];
        let caps = r.u32()?;
        let caps2 = r.u32()?;
        let caps3 = r.u32()?;
        let caps4 = r.u32()?;
        let reserved2 = r.u32()?;
        Ok(DdsHeader {
            size,
            flags,
            height,
            width,
            pitch_or_linear_size,
            depth,
            mip_map_count,
            reserved1,
            pf: PixelFormat {
                size: pf_size,
                flags: pf_flags,
                four_cc,
                rgb_bit_count,
                masks,
            },
            caps,
            caps2,
            caps3,
            caps4,
            reserved2,
        })
    }

    /// Appends the 128 bytes `"DDS "` + header to `out`, field for field as stored.
    pub fn write_to(&self, out: &mut Vec<u8>) {
        out.reserve(DATA_OFFSET);
        out.extend_from_slice(&MAGIC);
        let words = [
            self.size,
            self.flags,
            self.height,
            self.width,
            self.pitch_or_linear_size,
            self.depth,
            self.mip_map_count,
        ];
        for w in words.iter().chain(self.reserved1.iter()) {
            out.extend_from_slice(&w.to_le_bytes());
        }
        out.extend_from_slice(&self.pf.size.to_le_bytes());
        out.extend_from_slice(&self.pf.flags.to_le_bytes());
        out.extend_from_slice(&self.pf.four_cc);
        out.extend_from_slice(&self.pf.rgb_bit_count.to_le_bytes());
        for m in &self.pf.masks {
            out.extend_from_slice(&m.to_le_bytes());
        }
        let tail = [
            self.caps,
            self.caps2,
            self.caps3,
            self.caps4,
            self.reserved2,
        ];
        for w in &tail {
            out.extend_from_slice(&w.to_le_bytes());
        }
    }

    /// The number of mip levels the header declares per face: `dwMipMapCount` when `MIPMAPCOUNT` is set and
    /// the count is positive, else 1 (Plan/30 §11.1).
    pub fn declared_levels(&self) -> u32 {
        if self.flags & DDSD_MIPMAPCOUNT != 0 && self.mip_map_count > 0 {
            self.mip_map_count
        } else {
            1
        }
    }

    /// `true` for a cube map (`DDSCAPS2_CUBEMAP`).
    pub fn is_cube(&self) -> bool {
        self.caps2 & DDSCAPS2_CUBEMAP != 0
    }
}

/// The pixel formats the decoders know (Plan/30 §11.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DdsFormat {
    /// FourCC `DXT1`, 8-byte blocks.
    Bc1,
    /// FourCC `DXT3`, 16-byte blocks.
    Bc2,
    /// FourCC `DXT5`, 16-byte blocks.
    Bc3,
    /// 16-bit `ARRRRRGGGGGBBBBB` (masks 7C00/03E0/001F/8000).
    A1R5G5B5,
    /// 16-bit `xRRRRRGGGGGBBBBB` (masks 7C00/03E0/001F/0).
    X1R5G5B5,
    /// 16-bit `RRRRRGGGGGGBBBBB` (masks F800/07E0/001F/0).
    R5G6B5,
    /// 32-bit `A8R8G8B8` (memory order B, G, R, A).
    A8R8G8B8,
    /// Any other RGB(A) layout of 8, 16, 24 or 32 bits with contiguous channel masks (`R, G, B, A`; the alpha
    /// mask is zero when the file does not set `DDPF_ALPHAPIXELS`).
    OtherRgb {
        /// Bits per pixel (8, 16, 24 or 32).
        bits: u32,
        /// Channel masks `R, G, B, A`.
        masks: [u32; 4],
    },
}

impl DdsFormat {
    /// `true` for BC1/2/3.
    pub fn is_compressed(self) -> bool {
        matches!(self, DdsFormat::Bc1 | DdsFormat::Bc2 | DdsFormat::Bc3)
    }

    /// Bytes per 4x4 block of a compressed format (`None` for uncompressed ones).
    pub fn block_bytes(self) -> Option<usize> {
        match self {
            DdsFormat::Bc1 => Some(8),
            DdsFormat::Bc2 | DdsFormat::Bc3 => Some(16),
            _ => None,
        }
    }

    /// Bytes per pixel of an uncompressed format (`None` for BC formats).
    pub fn bytes_per_pixel(self) -> Option<usize> {
        match self {
            DdsFormat::A1R5G5B5 | DdsFormat::X1R5G5B5 | DdsFormat::R5G6B5 => Some(2),
            DdsFormat::A8R8G8B8 => Some(4),
            DdsFormat::OtherRgb { bits, .. } => Some((bits / 8) as usize),
            _ => None,
        }
    }

    /// Byte size of one `width x height` level: `max(1, (w+3)/4) * max(1, (h+3)/4) * block` for BC, `w * h *
    /// bytes_per_pixel` otherwise (rows tightly packed). Saturates instead of overflowing.
    pub fn level_size(self, width: u32, height: u32) -> u64 {
        let (w, h) = (u64::from(width), u64::from(height));
        match self.block_bytes() {
            Some(block) => w
                .div_ceil(4)
                .max(1)
                .saturating_mul(h.div_ceil(4).max(1))
                .saturating_mul(block as u64),
            None => w
                .saturating_mul(h)
                .saturating_mul(self.bytes_per_pixel().unwrap_or(0) as u64),
        }
    }

    /// The `DDS_PIXELFORMAT` D3DX writes for this format.
    pub fn pixel_format(self) -> PixelFormat {
        let rgb = |flags: u32, bits: u32, masks: [u32; 4]| PixelFormat {
            size: PIXELFORMAT_SIZE,
            flags,
            four_cc: [0; 4],
            rgb_bit_count: bits,
            masks,
        };
        let fourcc = |cc: [u8; 4]| PixelFormat {
            size: PIXELFORMAT_SIZE,
            flags: DDPF_FOURCC,
            four_cc: cc,
            rgb_bit_count: 0,
            masks: [0; 4],
        };
        match self {
            DdsFormat::Bc1 => fourcc(*b"DXT1"),
            DdsFormat::Bc2 => fourcc(*b"DXT3"),
            DdsFormat::Bc3 => fourcc(*b"DXT5"),
            DdsFormat::A1R5G5B5 => rgb(
                DDPF_RGB | DDPF_ALPHAPIXELS,
                16,
                [0x7C00, 0x03E0, 0x001F, 0x8000],
            ),
            DdsFormat::X1R5G5B5 => rgb(DDPF_RGB, 16, [0x7C00, 0x03E0, 0x001F, 0]),
            DdsFormat::R5G6B5 => rgb(DDPF_RGB, 16, [0xF800, 0x07E0, 0x001F, 0]),
            DdsFormat::A8R8G8B8 => rgb(
                DDPF_RGB | DDPF_ALPHAPIXELS,
                32,
                [0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0xFF00_0000],
            ),
            DdsFormat::OtherRgb { bits, masks } => {
                let flags = if masks[3] != 0 {
                    DDPF_RGB | DDPF_ALPHAPIXELS
                } else {
                    DDPF_RGB
                };
                rgb(flags, bits, masks)
            }
        }
    }

    /// Identifies the format of a pixel-format block (Plan/30 §11.1 table). `Err` carries the
    /// `UnsupportedPixelFormat` description.
    pub fn identify(pf: &PixelFormat) -> Result<DdsFormat, String> {
        if pf.flags & DDPF_FOURCC != 0 {
            return match &pf.four_cc {
                b"DXT1" => Ok(DdsFormat::Bc1),
                b"DXT3" => Ok(DdsFormat::Bc2),
                b"DXT5" => Ok(DdsFormat::Bc3),
                other => Err(format!(
                    "FourCC {:?} (only DXT1, DXT3 and DXT5 are supported)",
                    String::from_utf8_lossy(other)
                )),
            };
        }
        if pf.flags & DDPF_RGB == 0 {
            return Err(format!(
                "pixel format flags {:#x} (neither FourCC nor RGB)",
                pf.flags
            ));
        }
        let bits = pf.rgb_bit_count;
        let alpha = if pf.flags & DDPF_ALPHAPIXELS != 0 {
            pf.masks[3]
        } else {
            0
        };
        let masks = [pf.masks[0], pf.masks[1], pf.masks[2], alpha];
        match (bits, masks) {
            (16, [0x7C00, 0x03E0, 0x001F, 0x8000]) => return Ok(DdsFormat::A1R5G5B5),
            (16, [0x7C00, 0x03E0, 0x001F, 0]) => return Ok(DdsFormat::X1R5G5B5),
            (16, [0xF800, 0x07E0, 0x001F, 0]) => return Ok(DdsFormat::R5G6B5),
            (32, [0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0xFF00_0000]) => {
                return Ok(DdsFormat::A8R8G8B8);
            }
            _ => {}
        }
        if !matches!(bits, 8 | 16 | 24 | 32) {
            return Err(format!(
                "{bits}-bit RGB (only 8, 16, 24 and 32 are supported)"
            ));
        }
        if masks[0] | masks[1] | masks[2] == 0 {
            return Err("RGB format without colour masks".to_owned());
        }
        for m in masks {
            let fits = bits == 32 || m >> bits == 0;
            let shifted = if m == 0 { 0 } else { m >> m.trailing_zeros() };
            let contiguous = shifted & shifted.wrapping_add(1) == 0;
            if !fits || !contiguous {
                return Err(format!(
                    "RGB masks {:#x}/{:#x}/{:#x}/{:#x} are not contiguous bit fields of {bits} bits",
                    masks[0], masks[1], masks[2], masks[3]
                ));
            }
        }
        Ok(DdsFormat::OtherRgb { bits, masks })
    }
}

/// One mip level of an owned [`Dds`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DdsLevel {
    /// Level width in texels.
    pub width: u32,
    /// Level height in texels.
    pub height: u32,
    /// The level's bytes (BC blocks or tightly packed rows).
    pub data: Vec<u8>,
}

/// An owned DDS texture: the header as stored, the identified format and `faces[face][level]` (1 face, or 6
/// for a cube map).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dds {
    /// The header as stored.
    pub header: DdsHeader,
    /// The identified pixel format.
    pub format: DdsFormat,
    /// `faces[face][level]`.
    pub faces: Vec<Vec<DdsLevel>>,
}

/// A zero-copy DDS texture: `levels[face][level] = (width, height, bytes)` borrowing the input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DdsView<'a> {
    /// The header as stored.
    pub header: DdsHeader,
    /// The identified pixel format.
    pub format: DdsFormat,
    /// `levels[face][level]`.
    pub levels: Vec<Vec<(u32, u32, &'a [u8])>>,
}

/// Summary facts about a texture (Plan/30 §11.1; the corpus tallies and the pack builders use them).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DdsFacts {
    /// BC1/2/3.
    pub compressed: bool,
    /// Mip levels per face.
    pub levels: u32,
    /// The chain reaches 1x1 (`levels == floor(log2(max(w, h))) + 1`).
    pub full_chain: bool,
    /// More than one level but not down to 1x1.
    pub partial_chain: bool,
    /// Width or height is not a power of two.
    pub npot: bool,
    /// Six-face cube map.
    pub cube: bool,
}

impl<'a> DdsView<'a> {
    /// Number of faces (1 or 6).
    pub fn face_count(&self) -> usize {
        self.levels.len()
    }

    /// Mip levels per face.
    pub fn level_count(&self) -> usize {
        self.levels.first().map_or(0, Vec::len)
    }

    /// Copies the data into an owned [`Dds`].
    pub fn to_owned_dds(&self) -> Dds {
        Dds {
            header: self.header,
            format: self.format,
            faces: self
                .levels
                .iter()
                .map(|face| {
                    face.iter()
                        .map(|&(width, height, data)| DdsLevel {
                            width,
                            height,
                            data: data.to_vec(),
                        })
                        .collect()
                })
                .collect(),
        }
    }
}

impl Dds {
    /// Borrows the levels as a [`DdsView`] (so `decode_level`, `to_gl_upload` and `complete_mips` work on
    /// owned textures too).
    pub fn as_view(&self) -> DdsView<'_> {
        DdsView {
            header: self.header,
            format: self.format,
            levels: self
                .faces
                .iter()
                .map(|face| {
                    face.iter()
                        .map(|l| (l.width, l.height, l.data.as_slice()))
                        .collect()
                })
                .collect(),
        }
    }

    /// Serialises the texture: the header exactly as stored, then every face's levels in order.
    pub fn to_bytes(&self) -> Vec<u8> {
        let payload: usize = self
            .faces
            .iter()
            .flat_map(|f| f.iter())
            .map(|l| l.data.len())
            .sum();
        let mut out = Vec::with_capacity(DATA_OFFSET + payload);
        self.header.write_to(&mut out);
        for level in self.faces.iter().flat_map(|f| f.iter()) {
            out.extend_from_slice(&level.data);
        }
        out
    }
}

/// Parses a DDS file into an owned [`Dds`] (Plan/30 §11.1).
///
/// Errors: `BadMagic`, `UnexpectedEof` (header or level data), `InvalidValue` (`dwSize`, `pf.dwSize`, a zero
/// side), `DimensionTooLarge` (> 8192), `CountOutOfRange` (`dwMipMapCount` above the legal chain length),
/// `UnsupportedPixelFormat` (volume textures, incomplete cube maps, DXT2/DXT4, luminance, YUV ...).
/// Warnings: `DdsPartialMipChain`, `TrailingBytes`.
pub fn parse(b: &[u8]) -> Result<Parsed<Dds>, FormatError> {
    let v = view(b)?;
    Ok(Parsed {
        value: v.value.to_owned_dds(),
        warnings: v.warnings,
    })
}

/// Parses a DDS file without copying the pixel data. See [`parse`] for the error and warning list.
pub fn view(b: &[u8]) -> Result<Parsed<DdsView<'_>>, FormatError> {
    let mut r = ByteReader::new(b, FormatKind::Dds);
    let magic = r.u32()?.to_le_bytes();
    if magic != MAGIC {
        return Err(r.err_at(
            0,
            ErrorKind::BadMagic {
                expected: "DDS ",
                found: magic,
            },
        ));
    }
    let header = DdsHeader::read(&mut r)?;
    let (width, height) = (header.width, header.height);
    if width == 0 || height == 0 {
        let (field, offset) = if width == 0 {
            ("dwWidth", 16)
        } else {
            ("dwHeight", 12)
        };
        return Err(r.err_at(offset, ErrorKind::InvalidValue { field, value: 0 }));
    }
    if width > MAX_DDS_DIM || height > MAX_DDS_DIM {
        return Err(r.err_at(12, ErrorKind::DimensionTooLarge { width, height }));
    }
    if header.caps2 & DDSCAPS2_VOLUME != 0 {
        return Err(r.err_at(
            112,
            ErrorKind::UnsupportedPixelFormat {
                detail: "volume texture".to_owned(),
            },
        ));
    }
    let cube = header.is_cube();
    if cube && header.caps2 & DDSCAPS2_CUBEMAP_ALLFACES != DDSCAPS2_CUBEMAP_ALLFACES {
        return Err(r.err_at(
            112,
            ErrorKind::UnsupportedPixelFormat {
                detail: format!("incomplete cube map (caps2 {:#x})", header.caps2),
            },
        ));
    }
    let format = DdsFormat::identify(&header.pf).map_err(|detail| {
        r.err_at(
            OFF_PIXEL_FORMAT,
            ErrorKind::UnsupportedPixelFormat { detail },
        )
    })?;

    let levels_per_face = header.declared_levels();
    let max_levels = max_mip_levels(width, height);
    if levels_per_face > max_levels {
        return Err(r.err_at(
            OFF_MIP_COUNT,
            ErrorKind::CountOutOfRange {
                field: "dwMipMapCount",
                value: i64::from(levels_per_face),
                max: i64::from(max_levels),
            },
        ));
    }

    let face_count = if cube { 6 } else { 1 };
    let mut faces = Vec::with_capacity(face_count);
    for _ in 0..face_count {
        let mut face = Vec::with_capacity(levels_per_face as usize);
        for k in 0..levels_per_face {
            let lw = (width >> k).max(1);
            let lh = (height >> k).max(1);
            let size = usize::try_from(format.level_size(lw, lh)).map_err(|_| {
                r.err(ErrorKind::DimensionTooLarge {
                    width: lw,
                    height: lh,
                })
            })?;
            face.push((lw, lh, r.bytes(size)?));
        }
        faces.push(face);
    }

    let mut warnings = Vec::new();
    if levels_per_face > 1 && levels_per_face < max_levels {
        warnings.push(FormatWarning::new(
            OFF_MIP_COUNT,
            WarningCode::DdsPartialMipChain,
            format!(
                "{levels_per_face} of {max_levels} mip levels for {width}x{height}: the chain stops early"
            ),
        ));
    }
    if r.remaining() > 0 {
        warnings.push(FormatWarning::new(
            r.pos(),
            WarningCode::TrailingBytes,
            format!("{} byte(s) after the last mip level", r.remaining()),
        ));
    }
    Ok(Parsed {
        value: DdsView {
            header,
            format,
            levels: faces,
        },
        warnings,
    })
}

/// Summarises a view (Plan/30 §11.1).
pub fn facts(d: &DdsView<'_>) -> DdsFacts {
    let levels = d.level_count() as u32;
    let (w, h) = (d.header.width, d.header.height);
    let max = max_mip_levels(w, h);
    DdsFacts {
        compressed: d.format.is_compressed(),
        levels,
        full_chain: levels == max,
        partial_chain: levels > 1 && levels < max,
        npot: !w.is_power_of_two() || !h.is_power_of_two(),
        cube: d.face_count() == 6,
    }
}
