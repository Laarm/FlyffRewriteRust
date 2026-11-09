//! `.dyo` dynamic objects (Plan/30 §7.3; ref/19 §8; `CWorld::LoadObject`, src:_Common/WorldFile.cpp:297-381).
//!
//! A `.dyo` is a sequence of object records ended by `u32 0xFFFFFFFF`; an empty file is the 4-byte
//! terminator. **KEEP-QUIRK Q6:** the original loops `ReadObj` until it returns NULL, so any other unknown
//! object type ends the list too; the parser keeps the value in [`DyoFile::terminator`], reports
//! [`WarningCode::DyoUnknownTypeTerminates`] and treats everything after it as trailing bytes. A file that
//! ends before any terminator is an `UnexpectedEof` error (the original would read uninitialised memory).

use crate::FormatKind;
use crate::error::{FormatError, FormatWarning, Parsed, WarningCode};
use crate::objects::{END_OF_LIST, ObjRecord, RecordRead, read_record_or_end};
use crate::reader::ByteReader;

/// A parsed `.dyo` (Plan/30 §7.3).
#[derive(Debug, Clone, PartialEq)]
pub struct DyoFile {
    /// The records in file order (movers, controls, ...).
    pub records: Vec<ObjRecord>,
    /// The word that ended the list: `0xFFFFFFFF`, or the unknown object type that did.
    pub terminator: u32,
}

impl DyoFile {
    /// `true` when the list ended with the regular `0xFFFFFFFF`.
    pub fn is_regular_end(&self) -> bool {
        self.terminator == END_OF_LIST
    }
}

/// Parses a `.dyo`. Each record is read through `objects::read_record_or_end`; bytes after the terminator
/// are a `TrailingBytes` warning.
pub fn parse(b: &[u8]) -> Result<Parsed<DyoFile>, FormatError> {
    let mut r = ByteReader::new(b, FormatKind::Dyo);
    let mut warnings = Vec::new();
    let mut records = Vec::new();
    let terminator = loop {
        let at = r.pos();
        match read_record_or_end(&mut r)? {
            RecordRead::Record(rec) => records.push(rec),
            RecordRead::End(word) => {
                if word != END_OF_LIST {
                    warnings.push(FormatWarning::new(
                        at,
                        WarningCode::DyoUnknownTypeTerminates,
                        format!(
                            "unknown object type {word:#010x} ends the list after {} record(s)",
                            records.len()
                        ),
                    ));
                }
                break word;
            }
        }
    };
    if r.remaining() > 0 {
        warnings.push(FormatWarning::new(
            r.pos(),
            WarningCode::TrailingBytes,
            format!("{} byte(s) after the list terminator", r.remaining()),
        ));
    }
    Ok(Parsed {
        value: DyoFile {
            records,
            terminator,
        },
        warnings,
    })
}
