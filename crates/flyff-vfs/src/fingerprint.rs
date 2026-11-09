//! Source records and the data fingerprint (Plan/14 §2.9).
//!
//! Every loader opens files through the VFS and records a [`SourceRecord`] per file (missing files
//! too, with `layer = None`). The data fingerprint is `xxh3_128` over the records sorted by path
//! (`path`, `layer`, `size`, `mtime_ns`) plus the loader version, so any edit of a source file or an
//! `overrides/` change moves it. `flyff-assets` keys the `data-bundles` derived kind on it.

use crate::index::{Layer, Vfs};
use crate::text::SourceEncoding;
use std::fmt;
use xxhash_rust::xxh3::Xxh3;

/// What was read from one file.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SourceRecord {
    /// The normalised path that was requested.
    pub path: String,
    /// The layer the file came from; `None` when it does not exist.
    pub layer: Option<Layer>,
    /// Size in bytes (0 when missing).
    pub size: u64,
    /// Modification time in ns since the Unix epoch (0 when missing).
    pub mtime_ns: i64,
    /// The detected text encoding, when the file was decoded as text.
    pub encoding: Option<SourceEncoding>,
}

impl SourceRecord {
    /// The record of a file that does not exist.
    pub fn missing(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            layer: None,
            size: 0,
            mtime_ns: 0,
            encoding: None,
        }
    }
}

/// A 128-bit data fingerprint, displayed as 32 lower-case hex digits.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Fingerprint(pub u128);

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:032x}", self.0)
    }
}

/// `xxh3_128` over the sorted `(path, layer, size, mtime_ns)` tuples plus `loader_version`. The result
/// does not depend on the order of `records`.
pub fn data_fingerprint<'a>(
    records: impl IntoIterator<Item = &'a SourceRecord>,
    loader_version: u32,
) -> Fingerprint {
    let mut sorted: Vec<&SourceRecord> = records.into_iter().collect();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    let mut h = Xxh3::new();
    for r in sorted {
        h.update(r.path.as_bytes());
        h.update(&[0]);
        h.update(&[match r.layer {
            None => 0,
            Some(Layer::Resource) => 1,
            Some(Layer::Override) => 2,
        }]);
        h.update(&r.size.to_le_bytes());
        h.update(&r.mtime_ns.to_le_bytes());
    }
    h.update(&loader_version.to_le_bytes());
    Fingerprint(h.digest128())
}

impl Vfs {
    /// The [`SourceRecord`] of `p` as it resolves now (missing files give `layer = None`). The path
    /// in the record is the normalised key (or the input when it cannot be normalised).
    pub fn source_record(&self, p: &str, encoding: Option<SourceEncoding>) -> SourceRecord {
        match Vfs::normalize(p) {
            Err(_) => SourceRecord::missing(p),
            Ok(key) => match self.resolve_key(key.as_str()) {
                None => SourceRecord::missing(key.to_string()),
                Some(e) => SourceRecord {
                    path: key.to_string(),
                    layer: Some(e.layer),
                    size: e.size,
                    mtime_ns: e.mtime_ns,
                    encoding,
                },
            },
        }
    }
}
