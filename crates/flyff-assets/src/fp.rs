//! Fingerprints (Plan/24 §6.1-§6.3).
//!
//! All fingerprints are `xxh3_64` rendered as 16 lower-hex characters ([`Fp`]). Integers are hashed
//! little-endian, strings as their UTF-8 bytes followed by one `0x00`. The formulas:
//!
//! | name | bytes hashed |
//! |---|---|
//! | [`file_fp`] | `"F1"`, path, `0`, layer (`0` Resource / `1` Override), size `u64`, mtime_ns `i64` |
//! | [`set_fp`] | `"S1"`, then for each direct child of the directory in sorted order whose extension is listed: path, `0`, `file_fp` as `u64` |
//! | [`group_fp`] | [`set_fp`] over the whitelisted extensions of the directory ([`crate::res`]) |
//! | [`registry_fp`] | `"G1"`, `file_fp(mdlobj.inc)`, `file_fp(mdldyna.inc)`, `set_fp("", ["h"])` (a missing file counts as `0`) |
//! | [`kind_fp`] | `"K1"`, slug, `0`, `BUILDER_VERSION` `u32`, `FORMAT_VERSION` `u32`, every input as `u64` |
//! | [`manifest_fp`] | the manifest JSON bytes |
//!
//! The literal tags (`F1`, `S1`, `G1`, `K1`) carry no terminator: the plan's formulas write the
//! terminating `0` explicitly after the variable-length strings only.
//!
//! Everything is computed from the VFS index (stat data), never from file contents; `mtime_ns` is the
//! one the index recorded. [`FingerprintMode::Content`] (Plan/24 §6.6, WP-WEB-23) is accepted but
//! behaves like `Stat` for now.

use crate::kinds::AssetKind;
use crate::res;
use flyff_vfs::{Layer, Vfs, VfsEntry};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;
use xxhash_rust::xxh3::Xxh3;

/// A 64-bit fingerprint, displayed as 16 lower-case hex digits.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(type = "string"))]
pub struct Fp(pub u64);

impl Fp {
    /// The fingerprint of "nothing" (a missing input file).
    pub const ZERO: Fp = Fp(0);

    /// The 16-digit lower-hex form.
    pub fn to_hex(self) -> String {
        format!("{:016x}", self.0)
    }
}

impl fmt::Display for Fp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

/// `Fp::from_str` failed: the text is not exactly 16 hex digits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("a fingerprint is exactly 16 hex digits")]
pub struct ParseFpError;

impl FromStr for Fp {
    type Err = ParseFpError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 16 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(ParseFpError);
        }
        u64::from_str_radix(s, 16).map(Fp).map_err(|_| ParseFpError)
    }
}

impl Serialize for Fp {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Fp {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// `assets.fingerprint_mode` (Plan/24 §6.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FingerprintMode {
    /// `(path, layer, size, mtime_ns)` per file (the default, W5).
    #[default]
    Stat,
    /// Content hashes for portable prepared caches (WP-WEB-23; currently identical to `Stat`).
    Content,
}

/// Incremental hasher with the encoding rules of §6.1.
struct Hasher(Xxh3);

impl Hasher {
    fn new(tag: &[u8]) -> Self {
        let mut h = Xxh3::new();
        h.update(tag);
        Hasher(h)
    }

    /// A string: UTF-8 bytes followed by one `0x00`.
    fn str0(&mut self, s: &str) {
        self.0.update(s.as_bytes());
        self.0.update(&[0]);
    }

    fn u8(&mut self, v: u8) {
        self.0.update(&[v]);
    }

    fn u32(&mut self, v: u32) {
        self.0.update(&v.to_le_bytes());
    }

    fn u64(&mut self, v: u64) {
        self.0.update(&v.to_le_bytes());
    }

    fn i64(&mut self, v: i64) {
        self.0.update(&v.to_le_bytes());
    }

    fn fp(&mut self, f: Fp) {
        self.u64(f.0);
    }

    fn finish(self) -> Fp {
        Fp(self.0.digest())
    }
}

/// `file_fp(e)`: the stat fingerprint of one VFS entry (override layer included).
pub fn file_fp(e: &VfsEntry) -> Fp {
    let mut h = Hasher::new(b"F1");
    h.str0(e.path.as_str());
    h.u8(match e.layer {
        Layer::Resource => 0,
        Layer::Override => 1,
    });
    h.u64(e.size);
    h.i64(e.mtime_ns);
    h.finish()
}

fn ext_listed(path: &str, exts: &[&str]) -> bool {
    if exts.is_empty() {
        return true;
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.')
        .is_some_and(|(_, ext)| exts.iter().any(|x| x.eq_ignore_ascii_case(ext)))
}

/// `set_fp(dir, exts)`: the fingerprint of the files directly inside `dir` whose extension is in `exts`
/// (all files when `exts` is empty). The directory name and the extension list are not hashed, only
/// the matching files, so adding a file of another extension or another directory changes nothing.
pub fn set_fp(vfs: &Vfs, dir: &str, exts: &[&str]) -> Fp {
    let mut h = Hasher::new(b"S1");
    for e in vfs.list_dir(dir) {
        if ext_listed(e.path.as_str(), exts) {
            h.str0(e.path.as_str());
            h.fp(file_fp(e));
        }
    }
    h.finish()
}

/// `group_fp(dir)`: [`set_fp`] over the client-visible extensions of `dir` (the `/res/<fp>/` URL
/// segment). `None` when `dir` is not a client-visible directory ([`res::CLIENT_DIRS`]).
pub fn group_fp(vfs: &Vfs, dir: &str) -> Option<Fp> {
    let exts = res::exts_of_dir(dir);
    if exts.is_empty() {
        return None;
    }
    Some(set_fp(vfs, dir, &exts))
}

/// `registry_fp`: the model-registry inputs (`mdlObj.inc`, `mdlDyna.inc` and every `*.h` in the root).
pub fn registry_fp(vfs: &Vfs) -> Fp {
    let of = |p: &str| vfs.resolve(p).map_or(Fp::ZERO, file_fp);
    let mut h = Hasher::new(b"G1");
    h.fp(of("mdlobj.inc"));
    h.fp(of("mdldyna.inc"));
    h.fp(set_fp(vfs, "", &["h"]));
    h.finish()
}

/// `kind_fp(kind, inputs…)`: the `/gen` URL fingerprint and cache partition directory name.
pub fn kind_fp(kind: AssetKind, builder_version: u32, format_version: u32, inputs: &[Fp]) -> Fp {
    let mut h = Hasher::new(b"K1");
    h.str0(kind.slug());
    h.u32(builder_version);
    h.u32(format_version);
    for f in inputs {
        h.fp(*f);
    }
    h.finish()
}

/// `manifest_fp`: the fingerprint of the serialised client manifest.
pub fn manifest_fp(json: &[u8]) -> Fp {
    Fp(xxhash_rust::xxh3::xxh3_64(json))
}

/// What a builder needs to compute fingerprints: the VFS, the game data and the mode (Plan/24 §7.2).
/// The methods are the free functions of this module bound to `vfs` and `mode`.
#[derive(Clone, Copy)]
pub struct FpCtx<'a> {
    /// The resource index.
    pub vfs: &'a Vfs,
    /// The loaded game data (`GameData.meta.fingerprint` feeds `boss-rigs` and `data-bundles`).
    pub data: &'a flyff_data::GameData,
    /// The fingerprint mode.
    pub mode: FingerprintMode,
}

impl<'a> FpCtx<'a> {
    /// A context over `vfs` and `data` in `Stat` mode.
    pub fn new(vfs: &'a Vfs, data: &'a flyff_data::GameData) -> Self {
        Self {
            vfs,
            data,
            mode: FingerprintMode::Stat,
        }
    }

    /// [`file_fp`] of an entry.
    pub fn file_fp(&self, e: &VfsEntry) -> Fp {
        file_fp(e)
    }

    /// [`file_fp`] of the file at `path` (`Fp::ZERO` when it does not exist).
    pub fn file_fp_of(&self, path: &str) -> Fp {
        self.vfs.resolve(path).map_or(Fp::ZERO, file_fp)
    }

    /// [`set_fp`].
    pub fn set_fp(&self, dir: &str, exts: &[&str]) -> Fp {
        set_fp(self.vfs, dir, exts)
    }

    /// [`group_fp`].
    pub fn group_fp(&self, dir: &str) -> Option<Fp> {
        group_fp(self.vfs, dir)
    }

    /// [`registry_fp`].
    pub fn registry_fp(&self) -> Fp {
        registry_fp(self.vfs)
    }

    /// [`kind_fp`].
    pub fn kind_fp(
        &self,
        kind: AssetKind,
        builder_version: u32,
        format_version: u32,
        inputs: &[Fp],
    ) -> Fp {
        kind_fp(kind, builder_version, format_version, inputs)
    }

    /// The low 64 bits of `GameData.meta.fingerprint` (`boss-rigs` input, Plan/24 §6.3).
    pub fn data_fp_low64(&self) -> Fp {
        Fp(self.data.meta.fingerprint as u64) // truncation to the low 64 bits is the point
    }

    /// Both halves of the 128-bit `GameData.meta.fingerprint` (the `data_fingerprint` of
    /// `flyff-vfs`), low half first: the `kind_fp` inputs of the `data-bundles` kind (Plan/14 §10.3).
    pub fn data_fp_parts(&self) -> [Fp; 2] {
        let f = self.data.meta.fingerprint;
        [Fp(f as u64), Fp((f >> 64) as u64)] // the two truncations are the point
    }
}
