//! Normalised VFS paths (Plan/14 §2.2).
//!
//! The rules themselves live in [`flyff_common::normalize_resource_path`] (wasm-safe, shared with the
//! browser); this module wraps the result in [`VfsPath`] and adds [`join`] (the port of
//! `MakePath(DIR_X, file)`).

use crate::VfsError;
use flyff_common::PathError;
use std::borrow::Borrow;
use std::fmt;
use std::sync::Arc;

/// A normalised resource path: ASCII lower-case, `/`-separated, no leading or trailing slash, no `.`
/// or empty segments (`world/dudreadfulcave/dudreadfulcave.wld`). Ordering is by the string, so a
/// `BTreeMap<VfsPath, _>` iterates in sorted path order and can be queried with `&str` keys.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VfsPath(Arc<str>);

impl VfsPath {
    /// Normalises `p` (rules of Plan/14 §2.2).
    pub fn new(p: &str) -> Result<Self, VfsError> {
        normalize(p)
    }

    /// The normalised text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The last segment (the file name), already lower-cased.
    pub fn file_name(&self) -> &str {
        self.0.rsplit('/').next().unwrap_or(&self.0)
    }

    /// The directory part (`""` for a top-level file).
    pub fn parent(&self) -> &str {
        self.0.rsplit_once('/').map_or("", |(dir, _)| dir)
    }

    /// The extension of the file name without the dot (`"wld"` for `a.wld`, `"cnt"` for `a.wld.cnt`).
    pub fn extension(&self) -> Option<&str> {
        let name = self.file_name();
        name.rsplit_once('.')
            .map(|(_, ext)| ext)
            .filter(|ext| !ext.is_empty())
    }

    pub(crate) fn from_normalised(s: String) -> Self {
        Self(Arc::from(s))
    }
}

impl fmt::Display for VfsPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for VfsPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&*self.0, f)
    }
}

impl AsRef<str> for VfsPath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for VfsPath {
    fn borrow(&self) -> &str {
        &self.0
    }
}

/// `Vfs::normalize`: normalises a path found in data or code.
pub fn normalize(p: &str) -> Result<VfsPath, VfsError> {
    match flyff_common::normalize_resource_path(p) {
        Ok(s) => Ok(VfsPath::from_normalised(s)),
        Err(PathError::Empty) => Err(VfsError::InvalidPath(format!("empty path {p:?}"))),
        Err(e) => Err(VfsError::InvalidPath(e.to_string())),
    }
}

/// `Vfs::join`: the port of `MakePath(DIR_X, file)`: concatenates with `/` and normalises
/// (`join("Sound", "VocMale-logout.wav")` is `sound/vocmale-logout.wav`). When the result cannot be
/// normalised (empty, escaping the root) the plain concatenation is returned.
pub fn join(dir: &str, file: &str) -> String {
    let raw = format!("{dir}/{file}");
    flyff_common::normalize_resource_path(&raw).unwrap_or(raw)
}
