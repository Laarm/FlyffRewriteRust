//! Error and warning types of the format parsers (Plan/30 §2.1).
//!
//! Parsers return [`FormatError`] for input that cannot be represented and [`FormatWarning`]s (through
//! [`Parsed`]) for conditions the original loaders silently tolerated (F-076: warn, never crash).

use crate::FormatKind;

/// A parse failure: which format, the byte offset of the failing field, and what went wrong.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{format:?} at offset {offset}: {kind}")]
pub struct FormatError {
    /// Format being parsed.
    pub format: FormatKind,
    /// Byte offset (from the start of the input) of the field that failed.
    pub offset: u64,
    /// What went wrong.
    pub kind: ErrorKind,
}

impl FormatError {
    /// Builds an error at `offset` (clamped into `u64`).
    pub fn new(format: FormatKind, offset: usize, kind: ErrorKind) -> FormatError {
        FormatError {
            format,
            offset: offset as u64,
            kind,
        }
    }
}

/// The reason of a [`FormatError`] (Plan/30 §2.1).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ErrorKind {
    /// The input ended before a field could be read.
    #[error("unexpected end of data: {needed} byte(s) needed, {remaining} remaining")]
    UnexpectedEof {
        /// Bytes the read needed.
        needed: u64,
        /// Bytes that were left.
        remaining: u64,
    },
    /// A magic number did not match.
    #[error("bad magic: expected {expected:?}, found {found:02x?}")]
    BadMagic {
        /// The expected magic, as text.
        expected: &'static str,
        /// The four bytes found.
        found: [u8; 4],
    },
    /// A version field with a value the parser does not know.
    #[error("unsupported version {found}")]
    UnsupportedVersion {
        /// The version found.
        found: i64,
    },
    /// A pre-v20 `.o3d` head (Plan/30 §6.6).
    #[error("legacy (pre-v20) layout is not supported")]
    LegacyLayout,
    /// A count field was negative, above its cap, or larger than the input could hold.
    #[error("count `{field}` = {value} is out of range (max {max})")]
    CountOutOfRange {
        /// Field name.
        field: &'static str,
        /// The value found.
        value: i64,
        /// The cap that applies.
        max: i64,
    },
    /// A field with a value the format does not allow.
    #[error("invalid value {value} for `{field}`")]
    InvalidValue {
        /// Field name.
        field: &'static str,
        /// The value found.
        value: i64,
    },
    /// A length-prefixed string above its cap.
    #[error("string `{field}` of length {len} exceeds the maximum of {max}")]
    StringTooLong {
        /// Field name.
        field: &'static str,
        /// The length found.
        len: i64,
        /// The cap that applies.
        max: i64,
    },
    /// A texture pixel format the decoders do not handle.
    #[error("unsupported pixel format: {detail}")]
    UnsupportedPixelFormat {
        /// Description of the format.
        detail: String,
    },
    /// An image larger than the decode caps.
    #[error("image dimensions {width}x{height} are too large")]
    DimensionTooLarge {
        /// Width in pixels.
        width: u32,
        /// Height in pixels.
        height: u32,
    },
    /// Pack-layer validation failure (for example a bone parent after its child).
    #[error("inconsistent data: {what}")]
    Inconsistent {
        /// What is inconsistent.
        what: &'static str,
    },
}

/// A tolerated irregularity found while parsing (Plan/30 §2.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatWarning {
    /// Byte offset the warning refers to.
    pub offset: u64,
    /// Machine-readable code.
    pub code: WarningCode,
    /// Human-readable detail (for logs and `flyff check`).
    pub detail: String,
}

impl FormatWarning {
    /// Builds a warning at `offset`.
    pub fn new(offset: usize, code: WarningCode, detail: impl Into<String>) -> FormatWarning {
        FormatWarning {
            offset: offset as u64,
            code,
            detail: detail.into(),
        }
    }
}

/// Machine-readable warning codes (Plan/30 §2.1; the corpus tests assert exact counts per code).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WarningCode {
    /// Bytes remain after the last field the original loader reads.
    TrailingBytes,
    /// `.o3d` header name differs from the file's own name (model is not loaded by the original).
    O3dNameMismatch,
    /// `.o3d` attribute count does not match the data.
    O3dAttrCountMismatch,
    /// `.o3d` vertex pool size mismatch.
    O3dPoolSizeMismatch,
    /// `.o3d` index count mismatch.
    O3dIndexCountMismatch,
    /// A texture name longer than the original's 64-byte buffer.
    TextureNameLong,
    /// MOTIONCORE pool size mismatch.
    MotionPoolMismatch,
    /// `.chr` with a version the original does not know.
    ChrUnknownVersion,
    /// `.lnd` header coordinates differ from the file name.
    LandHeaderMismatch,
    /// `.dyo` ended by an unknown object type instead of `0xFFFFFFFF` (the original stops reading there).
    DyoUnknownTypeTerminates,
    /// `.sfx` part without its points.
    SfxPointsMissing,
    /// DDS with a partial mip chain.
    DdsPartialMipChain,
    /// Palette conflict in an indexed image.
    PaletteConflict,
    /// Palette index count not a multiple of 3.
    PaletteIndexNotMultipleOf3,
    /// Palette slot out of range.
    PaletteSlotOutOfRange,
    /// Texture id out of range.
    TextureIdOutOfRange,
    /// A block range reaching outside its container.
    BlockRangeOutOfBounds,
    /// Physique data inconsistent with the vertices.
    PhysiqueMismatch,
    /// Cursor with an inverting pixel.
    CursorInvertPixel,
    /// Image with trailing data.
    ImageTrailingData,
}

/// A parse result together with the warnings collected on the way (Plan/30 §2.1).
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed<T> {
    /// The parsed value.
    pub value: T,
    /// Tolerated irregularities (empty for a clean file).
    pub warnings: Vec<FormatWarning>,
}

impl<T> Parsed<T> {
    /// Wraps a value without warnings.
    pub fn clean(value: T) -> Parsed<T> {
        Parsed {
            value,
            warnings: Vec::new(),
        }
    }

    /// `true` when no warning carries `code`.
    pub fn lacks_warning(&self, code: WarningCode) -> bool {
        !self.warnings.iter().any(|w| w.code == code)
    }
}
