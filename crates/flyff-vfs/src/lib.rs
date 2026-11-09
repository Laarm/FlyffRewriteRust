//! `flyff-vfs` — tier N (Plan/14 §2): the case-insensitive index over `Resource/` with `overrides/`
//! shadowing, text decoding (BOM / UTF-16LE / CP949 / CP1252) and the Lua `dofile` path mapping.
//! Path normalisation itself lives in `flyff_common::normalize_resource_path`.
//!
//! * [`Vfs`] / [`VfsConfig`] / [`VfsEntry`] / [`Layer`] — the layered index ([`index`]), built once at
//!   start-up; lookups are by any path spelling (`"World\\DuDreadfulCave\\DuDreadfulCave.wld"`),
//! * [`VfsPath`] — a normalised key ([`path`]),
//! * [`decode_text`] / [`EncodingHint`] / [`SourceEncoding`] / [`DecodedText`] — decoding ([`text`]),
//! * [`Vfs::resolve_lua`] / [`Vfs::read_lua_source`] — the Lua `dofile`/`require` mapping
//!   ([`lua_path`]),
//! * [`SourceRecord`] / [`data_fingerprint`] — source tracking ([`fingerprint`]).
//!
//! Nothing here ever writes below `Resource/` or `overrides/`.

#![warn(missing_docs)]

pub mod fingerprint;
pub mod index;
pub mod lua_path;
pub mod path;
pub mod text;

pub use fingerprint::{Fingerprint, SourceRecord, data_fingerprint};
pub use index::{Layer, Vfs, VfsConfig, VfsEntry, VfsError, VfsNote, VfsNoteKind};
pub use lua_path::{lua_path_key, require_candidates};
pub use path::{VfsPath, join, normalize};
pub use text::{DecodedText, EncodingHint, SourceEncoding, decode_text, hint_for};

/// Crate marker used by the workspace smoke test and by `cargo xtask doctor`.
pub const CRATE_NAME: &str = "flyff-vfs";
