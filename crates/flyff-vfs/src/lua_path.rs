//! The Lua `dofile` / `require` path mapping (Plan/14 §2.8).
//!
//! Every top-level Lua script starts with `dofile(".\\LuaFunc\\XxxFunc.lua")`, a Windows path relative
//! to the original working directory, which was `Resource/`. The path is therefore resolved relative
//! to the Resource root, never relative to the calling script, and case-insensitively. The source
//! handed to Lua is the decoded UTF-8 text, so CP949 notice strings of `Event.lua` reach the clients
//! as proper UTF-8. `flyff-lua` and the static extraction VM of `flyff-data` install a `dofile`, a
//! `loadfile` and one `package.searchers` entry built on [`Vfs::read_lua_source`].

use crate::VfsError;
use crate::index::{Vfs, VfsEntry};
use crate::path::{VfsPath, normalize};
use crate::text::EncodingHint;

/// Strips leading `.\` / `./` (repeatedly) and normalises: `".\\LuaFunc\\EventFunc.lua"` is
/// `luafunc/eventfunc.lua`.
pub fn lua_path_key(arg: &str) -> Result<VfsPath, VfsError> {
    let mut s = arg.trim();
    while let Some(rest) = s.strip_prefix(".\\").or_else(|| s.strip_prefix("./")) {
        s = rest.trim_start();
    }
    normalize(s)
}

/// The candidate paths of `require(name)`: the module name with `.` replaced by `/` plus `.lua`, then
/// the same below `luafunc/`. The originals never use `require`.
pub fn require_candidates(name: &str) -> Vec<VfsPath> {
    let rel = format!("{}.lua", name.trim().replace('.', "/"));
    let mut out = Vec::new();
    if let Ok(p) = normalize(&rel) {
        out.push(p);
    }
    if let Ok(p) = normalize(&format!("luafunc/{rel}")) {
        out.push(p);
    }
    out
}

impl Vfs {
    /// Resolves the argument of `dofile`/`loadfile` (Plan/14 §2.8).
    pub fn resolve_lua(&self, arg: &str) -> Option<&VfsEntry> {
        let key = lua_path_key(arg).ok()?;
        self.resolve_key(key.as_str())
    }

    /// Reads a Lua file as UTF-8 text; returns the resolved normalised path (for chunk names and error
    /// messages) and the source.
    pub fn read_lua_source(&self, arg: &str) -> Result<(VfsPath, String), VfsError> {
        let key = lua_path_key(arg)?;
        let text = self.read_text(key.as_str(), EncodingHint::Auto)?.text;
        Ok((key, text))
    }

    /// Resolves `require(name)`: the first existing candidate of [`require_candidates`].
    pub fn resolve_require(&self, name: &str) -> Option<&VfsEntry> {
        require_candidates(name)
            .iter()
            .find_map(|k| self.resolve_key(k.as_str()))
    }

    /// Reads the source of `require(name)`.
    pub fn read_require_source(&self, name: &str) -> Result<(VfsPath, String), VfsError> {
        let entry = self
            .resolve_require(name)
            .ok_or_else(|| VfsError::NotFound(name.to_owned()))?;
        let key = entry.path.clone();
        let text = self.read_text(key.as_str(), EncodingHint::Auto)?.text;
        Ok((key, text))
    }
}
