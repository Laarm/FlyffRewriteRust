//! Blob conventions of the packs (Plan/30 §13.1).
//!
//! A pack is [`Packed<M>`]`{ meta, blob }`. The blob is a concatenation of sections, each starting at a
//! multiple of 4 bytes; the meta refers to them with [`BlobView`]. No 64-bit types, little-endian (wasm
//! and x86 are LE), so the browser reads sections with `TypedArray` views at `off`.

use crate::FormatKind;
use crate::error::{ErrorKind, FormatError};

/// A pack: a small structured `meta` plus one blob holding every bulk section.
#[derive(Debug, Clone, PartialEq)]
pub struct Packed<M> {
    /// The structured description (what the browser receives as a plain object).
    pub meta: M,
    /// All bulk sections, each 4-byte aligned.
    pub blob: Vec<u8>,
}

/// Element type of a section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", serde(rename_all = "lowercase"))]
pub enum ElemType {
    /// Bytes.
    U8,
    /// Little-endian `u16`.
    U16,
    /// Little-endian `u32`.
    U32,
    /// Little-endian `i32`.
    I32,
    /// Little-endian `f32`.
    F32,
}

impl ElemType {
    /// Size of one component in bytes.
    pub fn size(self) -> u32 {
        match self {
            ElemType::U8 => 1,
            ElemType::U16 => 2,
            ElemType::U32 | ElemType::I32 | ElemType::F32 => 4,
        }
    }
}

/// A typed window onto the blob (Plan/30 §13.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct BlobView {
    /// Byte offset of the section start (a multiple of 4).
    pub off: u32,
    /// Section length in bytes (`count * comps * size`).
    pub len: u32,
    /// Component type.
    pub ty: ElemType,
    /// Components per element (3 for positions, 2 for UVs, 1 for scalars and raw bytes).
    pub comps: u32,
    /// Number of elements.
    pub count: u32,
}

impl BlobView {
    /// The section's bytes within `blob`, or `None` when the view does not fit.
    pub fn bytes<'a>(&self, blob: &'a [u8]) -> Option<&'a [u8]> {
        let start = usize::try_from(self.off).ok()?;
        let end = start.checked_add(usize::try_from(self.len).ok()?)?;
        blob.get(start..end)
    }

    /// Decodes an `F32` section (or `None` when the type differs or the view does not fit).
    pub fn read_f32(&self, blob: &[u8]) -> Option<Vec<f32>> {
        if self.ty != ElemType::F32 {
            return None;
        }
        let bytes = self.bytes(blob)?;
        let (chunks, _) = bytes.as_chunks::<4>();
        Some(
            chunks
                .iter()
                .map(|c| f32::from_bits(u32::from_le_bytes(*c)))
                .collect(),
        )
    }

    /// Decodes a `U16` section.
    pub fn read_u16(&self, blob: &[u8]) -> Option<Vec<u16>> {
        if self.ty != ElemType::U16 {
            return None;
        }
        let bytes = self.bytes(blob)?;
        let (chunks, _) = bytes.as_chunks::<2>();
        Some(chunks.iter().map(|c| u16::from_le_bytes(*c)).collect())
    }

    /// Decodes a `U32` section.
    pub fn read_u32(&self, blob: &[u8]) -> Option<Vec<u32>> {
        if self.ty != ElemType::U32 {
            return None;
        }
        let bytes = self.bytes(blob)?;
        let (chunks, _) = bytes.as_chunks::<4>();
        Some(chunks.iter().map(|c| u32::from_le_bytes(*c)).collect())
    }
}

/// Builds a blob: every `push_*` pads the buffer to a multiple of 4 first and returns the [`BlobView`].
#[derive(Debug, Default, Clone)]
pub struct BlobBuilder {
    buf: Vec<u8>,
    format: Option<FormatKind>,
}

impl BlobBuilder {
    /// An empty builder (errors are labelled [`FormatKind::O3d`]).
    pub fn new() -> BlobBuilder {
        BlobBuilder::default()
    }

    /// An empty builder whose size errors carry `format`.
    pub fn for_format(format: FormatKind) -> BlobBuilder {
        BlobBuilder {
            buf: Vec::new(),
            format: Some(format),
        }
    }

    /// Bytes written so far (including padding).
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// `true` when nothing was written.
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    fn err(&self, what: &'static str) -> FormatError {
        FormatError::new(
            self.format.unwrap_or(FormatKind::O3d),
            self.buf.len(),
            ErrorKind::Inconsistent { what },
        )
    }

    fn align(&mut self) {
        let pad = (4 - self.buf.len() % 4) % 4;
        self.buf.resize(self.buf.len() + pad, 0);
    }

    fn push_raw(
        &mut self,
        ty: ElemType,
        comps: u32,
        count: usize,
        bytes: &[u8],
    ) -> Result<BlobView, FormatError> {
        self.align();
        let off = u32::try_from(self.buf.len()).map_err(|_| self.err("blob exceeds 4 GiB"))?;
        let len = u32::try_from(bytes.len()).map_err(|_| self.err("blob section exceeds 4 GiB"))?;
        let count = u32::try_from(count).map_err(|_| self.err("blob element count exceeds u32"))?;
        let end = self
            .buf
            .len()
            .checked_add(bytes.len())
            .ok_or_else(|| self.err("blob exceeds 4 GiB"))?;
        if u32::try_from(end).is_err() {
            return Err(self.err("blob exceeds 4 GiB"));
        }
        self.buf.extend_from_slice(bytes);
        Ok(BlobView {
            off,
            len,
            ty,
            comps,
            count,
        })
    }

    /// Appends raw bytes as a `U8` section (`comps = 1`, `count = len`).
    pub fn push_bytes(&mut self, bytes: &[u8]) -> Result<BlobView, FormatError> {
        self.push_raw(ElemType::U8, 1, bytes.len(), bytes)
    }

    /// Appends `bytes` (already little-endian) as elements of type `ty` with `comps` components each.
    /// `bytes.len()` must be a multiple of `comps * ty.size()` (`comps >= 1`). Used where the data already
    /// has the target layout (DDS levels, `U8` sections with `comps > 1`).
    pub fn push_typed(
        &mut self,
        ty: ElemType,
        comps: u32,
        bytes: &[u8],
    ) -> Result<BlobView, FormatError> {
        let stride = usize::try_from(comps.saturating_mul(ty.size())).unwrap_or(0);
        if comps == 0 || stride == 0 || !bytes.len().is_multiple_of(stride) {
            return Err(self.err("blob section size"));
        }
        self.push_raw(ty, comps, bytes.len() / stride, bytes)
    }

    /// Appends `data` as `F32` elements of `comps` components (`data.len()` must be a multiple of
    /// `comps`, `comps >= 1`).
    pub fn push_f32(&mut self, comps: u32, data: &[f32]) -> Result<BlobView, FormatError> {
        let n = element_count(comps, data.len()).ok_or_else(|| self.err("blob section size"))?;
        let mut bytes = Vec::with_capacity(data.len() * 4);
        for v in data {
            bytes.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        self.push_raw(ElemType::F32, comps, n, &bytes)
    }

    /// Appends `data` as `U16` elements of `comps` components.
    pub fn push_u16(&mut self, comps: u32, data: &[u16]) -> Result<BlobView, FormatError> {
        let n = element_count(comps, data.len()).ok_or_else(|| self.err("blob section size"))?;
        let mut bytes = Vec::with_capacity(data.len() * 2);
        for v in data {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        self.push_raw(ElemType::U16, comps, n, &bytes)
    }

    /// Appends `data` as `U32` elements of `comps` components.
    pub fn push_u32(&mut self, comps: u32, data: &[u32]) -> Result<BlobView, FormatError> {
        let n = element_count(comps, data.len()).ok_or_else(|| self.err("blob section size"))?;
        let mut bytes = Vec::with_capacity(data.len() * 4);
        for v in data {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        self.push_raw(ElemType::U32, comps, n, &bytes)
    }

    /// Appends `data` as `I32` elements of `comps` components.
    pub fn push_i32(&mut self, comps: u32, data: &[i32]) -> Result<BlobView, FormatError> {
        let n = element_count(comps, data.len()).ok_or_else(|| self.err("blob section size"))?;
        let mut bytes = Vec::with_capacity(data.len() * 4);
        for v in data {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        self.push_raw(ElemType::I32, comps, n, &bytes)
    }

    /// Finishes the blob (padded to a multiple of 4 bytes).
    pub fn finish(mut self) -> Vec<u8> {
        self.align();
        self.buf
    }
}

/// `len / comps` when `comps >= 1` and `comps` divides `len`.
fn element_count(comps: u32, len: usize) -> Option<usize> {
    let comps = usize::try_from(comps).ok().filter(|c| *c >= 1)?;
    len.is_multiple_of(comps).then_some(len / comps)
}
