//! The checked little-endian cursor every parser reads through (Plan/30 §2.1, §3).
//!
//! Rules (Plan/30 §2.1): every read is bounds-checked and returns [`FormatError`]; floats are read as raw
//! bits (NaN payloads survive); every multi-element read goes through [`ByteReader::count`] (or an
//! equivalent remaining-bytes check) *before* anything is allocated.

use crate::FormatKind;
use crate::error::{ErrorKind, FormatError};
use crate::raw::{D3dMatrix, Quat, RawStr, Vec3};

/// A checked cursor over a byte slice. After an `Err` the position is unspecified (except for plain
/// end-of-data errors, which leave it unchanged); callers abandon the parse on error.
#[derive(Debug, Clone)]
pub struct ByteReader<'a> {
    buf: &'a [u8],
    pos: usize,
    kind: FormatKind,
}

impl<'a> ByteReader<'a> {
    /// A reader at offset 0; `kind` labels the errors.
    pub fn new(buf: &'a [u8], kind: FormatKind) -> Self {
        ByteReader { buf, pos: 0, kind }
    }

    /// The format this reader labels its errors with.
    pub fn kind(&self) -> FormatKind {
        self.kind
    }

    /// Current byte offset from the start of the buffer.
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Bytes left.
    pub fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }

    /// Total length of the underlying buffer.
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// `true` when the underlying buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// `true` when every byte has been consumed.
    pub fn at_end(&self) -> bool {
        self.remaining() == 0
    }

    /// The unread bytes (does not advance).
    pub fn rest(&self) -> &'a [u8] {
        self.buf.get(self.pos..).unwrap_or(&[])
    }

    /// Moves the cursor to the absolute offset `pos` (`pos == len` is allowed).
    pub fn seek(&mut self, pos: usize) -> Result<(), FormatError> {
        if pos > self.buf.len() {
            return Err(self.err_at(
                pos,
                ErrorKind::UnexpectedEof {
                    needed: pos as u64,
                    remaining: self.buf.len() as u64,
                },
            ));
        }
        self.pos = pos;
        Ok(())
    }

    /// An error of this reader's format at the current position.
    pub fn err(&self, kind: ErrorKind) -> FormatError {
        FormatError::new(self.kind, self.pos, kind)
    }

    /// An error of this reader's format at `offset`.
    pub fn err_at(&self, offset: usize, kind: ErrorKind) -> FormatError {
        FormatError::new(self.kind, offset, kind)
    }

    fn eof(&self, needed: usize) -> FormatError {
        self.err(ErrorKind::UnexpectedEof {
            needed: needed as u64,
            remaining: self.remaining() as u64,
        })
    }

    /// Takes `n` bytes, advancing. The cursor is unchanged on error.
    fn take(&mut self, n: usize) -> Result<&'a [u8], FormatError> {
        let start = self.pos;
        let slice = start
            .checked_add(n)
            .and_then(|end| self.buf.get(start..end).map(|s| (end, s)));
        match slice {
            Some((end, s)) => {
                self.pos = end;
                Ok(s)
            }
            None => Err(self.eof(n)),
        }
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], FormatError> {
        let bytes = self.take(N)?;
        <[u8; N]>::try_from(bytes).map_err(|_| self.eof(N))
    }

    /// One byte.
    pub fn u8(&mut self) -> Result<u8, FormatError> {
        Ok(u8::from_le_bytes(self.array::<1>()?))
    }

    /// `u16`, little-endian.
    pub fn u16(&mut self) -> Result<u16, FormatError> {
        Ok(u16::from_le_bytes(self.array::<2>()?))
    }

    /// `i16`, little-endian.
    pub fn i16(&mut self) -> Result<i16, FormatError> {
        Ok(i16::from_le_bytes(self.array::<2>()?))
    }

    /// `u32`, little-endian.
    pub fn u32(&mut self) -> Result<u32, FormatError> {
        Ok(u32::from_le_bytes(self.array::<4>()?))
    }

    /// `i32`, little-endian.
    pub fn i32(&mut self) -> Result<i32, FormatError> {
        Ok(i32::from_le_bytes(self.array::<4>()?))
    }

    /// `f32` from the exact little-endian bits (NaN payloads included).
    pub fn f32(&mut self) -> Result<f32, FormatError> {
        Ok(f32::from_bits(self.u32()?))
    }

    /// The next `u32` without advancing.
    pub fn peek_u32(&self) -> Result<u32, FormatError> {
        self.clone().u32()
    }

    /// Three `f32` (12 bytes).
    pub fn vec3(&mut self) -> Result<Vec3, FormatError> {
        let [a, b, c] = self.take(12).map(f32x::<3>)?;
        Ok([a, b, c])
    }

    /// Four `f32` (16 bytes): `x, y, z, w`.
    pub fn quat(&mut self) -> Result<Quat, FormatError> {
        let [a, b, c, d] = self.take(16).map(f32x::<4>)?;
        Ok([a, b, c, d])
    }

    /// Sixteen `f32` (64 bytes), row-major `D3DXMATRIX`.
    pub fn mat(&mut self) -> Result<D3dMatrix, FormatError> {
        Ok(D3dMatrix(self.take(64).map(f32x::<16>)?))
    }

    /// `n` raw bytes (borrowed from the input).
    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8], FormatError> {
        self.take(n)
    }

    /// Skips `n` bytes.
    pub fn skip(&mut self, n: usize) -> Result<(), FormatError> {
        self.take(n).map(|_| ())
    }

    /// Reads `n` little-endian `f32` into a vector. The size check happens before the allocation.
    pub fn f32_vec(&mut self, n: usize) -> Result<Vec<f32>, FormatError> {
        let bytes = self.take_array_bytes(n, 4)?;
        let (chunks, _) = bytes.as_chunks::<4>();
        Ok(chunks
            .iter()
            .map(|c| f32::from_bits(u32::from_le_bytes(*c)))
            .collect())
    }

    /// Reads `n` little-endian `u32` into a vector (size check before the allocation).
    pub fn u32_vec(&mut self, n: usize) -> Result<Vec<u32>, FormatError> {
        let bytes = self.take_array_bytes(n, 4)?;
        let (chunks, _) = bytes.as_chunks::<4>();
        Ok(chunks.iter().map(|c| u32::from_le_bytes(*c)).collect())
    }

    /// Reads `n` little-endian `i32` into a vector (size check before the allocation).
    pub fn i32_vec(&mut self, n: usize) -> Result<Vec<i32>, FormatError> {
        let bytes = self.take_array_bytes(n, 4)?;
        let (chunks, _) = bytes.as_chunks::<4>();
        Ok(chunks.iter().map(|c| i32::from_le_bytes(*c)).collect())
    }

    fn take_array_bytes(&mut self, n: usize, elem_size: usize) -> Result<&'a [u8], FormatError> {
        match n.checked_mul(elem_size) {
            Some(total) => self.take(total),
            None => Err(self.eof(usize::MAX)),
        }
    }

    /// Reads an `i32` count and checks `0 <= count <= max` and `count * elem_size <= remaining` (with
    /// `checked_mul`) BEFORE the caller allocates anything (R1, R3).
    ///
    /// `CountOutOfRange` is reported at the offset of the count field; a count whose elements cannot fit
    /// in the remaining input is `UnexpectedEof` at the offset where the elements would start.
    pub fn count(
        &mut self,
        field: &'static str,
        elem_size: usize,
        max: u32,
    ) -> Result<usize, FormatError> {
        let at = self.pos;
        let value = self.i32()?;
        let value64 = i64::from(value);
        let max64 = i64::from(max);
        if !(0..=max64).contains(&value64) {
            return Err(self.err_at(
                at,
                ErrorKind::CountOutOfRange {
                    field,
                    value: value64,
                    max: max64,
                },
            ));
        }
        let n = usize::try_from(value).map_err(|_| {
            self.err_at(
                at,
                ErrorKind::CountOutOfRange {
                    field,
                    value: value64,
                    max: max64,
                },
            )
        })?;
        match n.checked_mul(elem_size) {
            Some(total) if total <= self.remaining() => Ok(n),
            Some(total) => Err(self.eof(total)),
            None => Err(self.eof(usize::MAX)),
        }
    }

    /// `NSTR`: an `i32` length INCLUDING the terminating NUL, then that many bytes; the result is cut at
    /// the first NUL. `0 <= len <= max_len`.
    pub fn nstr(&mut self, field: &'static str, max_len: usize) -> Result<RawStr, FormatError> {
        let raw = self.read_len_prefixed(field, max_len)?;
        let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        Ok(RawStr::from(raw.get(..end).unwrap_or(raw)))
    }

    /// `LSTR`: an `i32` length, then that many bytes, no NUL (`.sfx`). `0 <= len <= max_len`.
    pub fn lstr(&mut self, field: &'static str, max_len: usize) -> Result<RawStr, FormatError> {
        Ok(RawStr::from(self.read_len_prefixed(field, max_len)?))
    }

    fn read_len_prefixed(
        &mut self,
        field: &'static str,
        max_len: usize,
    ) -> Result<&'a [u8], FormatError> {
        let at = self.pos;
        let len = self.i32()?;
        let len64 = i64::from(len);
        let max64 = max_len as i64;
        if len < 0 {
            return Err(self.err_at(
                at,
                ErrorKind::CountOutOfRange {
                    field,
                    value: len64,
                    max: max64,
                },
            ));
        }
        if len64 > max64 {
            return Err(self.err_at(
                at,
                ErrorKind::StringTooLong {
                    field,
                    len: len64,
                    max: max64,
                },
            ));
        }
        let n = usize::try_from(len).map_err(|_| {
            self.err_at(
                at,
                ErrorKind::StringTooLong {
                    field,
                    len: len64,
                    max: max64,
                },
            )
        })?;
        self.take(n)
    }

    /// `char[n]` cut at the first NUL.
    pub fn fixed_str(&mut self, n: usize) -> Result<RawStr, FormatError> {
        let raw = self.take(n)?;
        let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        Ok(RawStr::from(raw.get(..end).unwrap_or(raw)))
    }
}

/// Converts `4 * N` little-endian bytes into `N` floats (exact bits). Used through `Result::map`.
fn f32x<const N: usize>(bytes: &[u8]) -> [f32; N] {
    let mut out = [0.0f32; N];
    let (chunks, _) = bytes.as_chunks::<4>();
    for (dst, c) in out.iter_mut().zip(chunks) {
        *dst = f32::from_bits(u32::from_le_bytes(*c));
    }
    out
}
