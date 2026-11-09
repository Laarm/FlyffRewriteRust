//! The client-visible directory whitelist (Plan/24 §5.2).
//!
//! A file under `/res` is servable iff its directory matches a [`DirRule`] row and its extension is
//! listed. Everything else (root text data, `client/`, `luafunc/`, `theme/default/`, `*.inc *.lua *.dll`
//! and so on) is not. The table is data: `flyff-server`'s `http::res` serves it, the client manifest
//! copies the conversion rows into `ClientManifest.rules`, and this crate needs it for
//! [`crate::fp::group_fp`] and for the eligibility rule of the gzip variants ([`crate::build::gz`]).
//!
//! Directory patterns are matched on the *normalised parent directory of the file* (lower-case,
//! `/`-separated, see [`flyff_common::normalize_resource_path`]); a pattern never matches
//! sub-directories (`theme` does not cover `theme/default`).

use flyff_vfs::Vfs;

/// How the client obtains a file of a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    /// Served raw (gzip variants apply to the eligible extensions).
    Raw,
    /// A DDS served raw unless it is listed in `dds_fix` (then `/gen/dds/...`).
    DdsRaw,
    /// Converted to PNG by the `png` kind, magenta key `exact`.
    PngExact,
    /// Converted to PNG by the `png` kind, no colour key.
    PngNone,
    /// A cursor: `/gen/cursor` PNG + hotspot (raw access also allowed).
    Cursor,
}

/// Which directories a row covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirPattern {
    /// Exactly this directory (`"model/texture"`).
    Exact(&'static str),
    /// `world/<folder>`: any direct sub-directory of `world` except `texture`.
    WorldFolder,
}

/// One row of the §5.2 table.
#[derive(Clone, Copy, Debug)]
pub struct DirRule {
    /// The directories.
    pub dir: DirPattern,
    /// Lower-case extensions without the dot.
    pub exts: &'static [&'static str],
    /// The client form.
    pub form: Form,
}

const fn rule(dir: &'static str, exts: &'static [&'static str], form: Form) -> DirRule {
    DirRule {
        dir: DirPattern::Exact(dir),
        exts,
        form,
    }
}

/// `http::res::CLIENT_DIRS` (Plan/24 §5.2 as data).
pub const CLIENT_DIRS: &[DirRule] = &[
    rule("model", &["o3d", "chr", "ani"], Form::Raw),
    rule("model/texture", &["dds"], Form::DdsRaw),
    rule("model/texture", &["tga", "bmp"], Form::PngNone),
    rule("sfx", &["sfx"], Form::Raw),
    rule("sfx", &["dds"], Form::DdsRaw),
    rule("sfx", &["tga", "bmp"], Form::PngNone),
    rule("sfx/texture", &["dds"], Form::DdsRaw),
    rule("sfx/texture", &["bmp", "tga"], Form::PngNone),
    rule("sfx/texture", &["sfx"], Form::Raw),
    rule("world/texture", &["dds"], Form::DdsRaw),
    DirRule {
        dir: DirPattern::WorldFolder,
        exts: &["lnd", "dds"],
        form: Form::Raw,
    },
    rule("weather", &["dds"], Form::DdsRaw),
    rule("weather", &["tga", "bmp"], Form::PngNone),
    rule("sound", &["wav"], Form::Raw),
    rule("music", &["ogg", "mp3", "wav"], Form::Raw),
    rule("theme", &["tga", "bmp", "dds"], Form::PngExact),
    rule("theme", &["png", "jpg"], Form::Raw),
    rule("icon", &["tga", "bmp", "dds"], Form::PngExact),
    rule("icon", &["jpg"], Form::Raw),
    rule("item", &["dds", "tga"], Form::PngExact),
    rule("char", &["tga"], Form::PngExact),
    rule("cursor", &["cur"], Form::Cursor),
];

impl DirPattern {
    /// Does the pattern cover the normalised directory `dir`?
    pub fn matches(self, dir: &str) -> bool {
        match self {
            DirPattern::Exact(d) => d == dir,
            DirPattern::WorldFolder => dir
                .strip_prefix("world/")
                .is_some_and(|rest| !rest.is_empty() && !rest.contains('/') && rest != "texture"),
        }
    }
}

/// The extension of a normalised path's file name, lower-case, without the dot.
pub fn ext_of(path: &str) -> Option<&str> {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.')
        .map(|(_, e)| e)
        .filter(|e| !e.is_empty())
}

/// The directory of a normalised path (`""` for a top-level file).
pub fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(d, _)| d)
}

/// The row that makes the file `dir/<name>.<ext>` client-visible (`dir` normalised).
pub fn rule_for(dir: &str, ext: &str) -> Option<&'static DirRule> {
    CLIENT_DIRS
        .iter()
        .find(|r| r.dir.matches(dir) && r.exts.contains(&ext))
}

/// `true` when the normalised file path is client-visible (directory and extension on the whitelist).
pub fn is_client_visible(path: &str) -> bool {
    ext_of(path).is_some_and(|ext| rule_for(dir_of(path), ext).is_some())
}

/// The union of the whitelisted extensions of `dir` (empty when `dir` is not whitelisted).
pub fn exts_of_dir(dir: &str) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for r in CLIENT_DIRS.iter().filter(|r| r.dir.matches(dir)) {
        for e in r.exts {
            if !out.contains(e) {
                out.push(e);
            }
        }
    }
    out
}

/// Every whitelisted directory that exists in `vfs`, sorted: the exact directories of the table plus
/// each `world/<folder>`. These are the keys of `ClientManifest.res.groups`.
pub fn client_dirs(vfs: &Vfs) -> Vec<String> {
    let mut dirs: Vec<String> = Vec::new();
    for r in CLIENT_DIRS {
        if let DirPattern::Exact(d) = r.dir
            && vfs.is_dir(d)
            && !dirs.iter().any(|x| x == d)
        {
            dirs.push(d.to_owned());
        }
    }
    for sub in vfs.list_subdirs("world") {
        if DirPattern::WorldFolder.matches(sub.as_str()) {
            dirs.push(sub.to_string());
        }
    }
    dirs.sort();
    dirs
}
