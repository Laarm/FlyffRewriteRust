//! The projections `GameData`/`ClientData` -> bundle DTOs (Plan/14 §10.4-10.6), one function per bundle.
//!
//! Every function is pure and deterministic. The client-variant transforms of §10.6 (ultimate names,
//! teleport16 levels, the help-topic join, which live in the loaders) are applied here; paths are made
//! canonical with [`canon`] and resolved through the [`PathResolver`].

mod items;
mod quests;
mod tables;
mod texts;
mod ui;
mod worlds;

pub use items::{
    client_item_name, project_item_ext, project_items, project_movers, project_npcs, project_skills,
};
pub use quests::{project_quest, project_quests};
pub use tables::{project_honor, project_tables};
pub use texts::{project_defines, project_help, project_names, project_texts};
pub use ui::{project_atlases, project_music, project_resdata, project_sounds, project_terrain};
pub use worlds::{WORLD_MAPS, project_models, project_sfx_table, project_worlds};

use crate::client_data::ClientData;
use crate::game_data::GameData;
use std::collections::BTreeSet;

/// What a projection reads.
#[derive(Clone, Copy)]
pub struct Sources<'a> {
    /// The server data.
    pub game: &'a GameData,
    /// The client-only data.
    pub client: &'a ClientData,
    /// Which canonical VFS paths exist.
    pub files: &'a dyn PathResolver,
}

/// Answers whether a canonical VFS path (`normalize_resource_path` form) exists.
pub trait PathResolver {
    /// `true` when the file exists.
    fn exists(&self, canonical_path: &str) -> bool;
}

impl PathResolver for BTreeSet<String> {
    fn exists(&self, canonical_path: &str) -> bool {
        self.contains(canonical_path)
    }
}

/// Every path exists (fixtures that do not care about the `null` rules).
#[derive(Clone, Copy, Debug, Default)]
pub struct AllFiles;

impl PathResolver for AllFiles {
    fn exists(&self, _: &str) -> bool {
        true
    }
}

/// No path exists.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoFiles;

impl PathResolver for NoFiles {
    fn exists(&self, _: &str) -> bool {
        false
    }
}

#[cfg(feature = "load")]
impl PathResolver for flyff_vfs::Vfs {
    fn exists(&self, canonical_path: &str) -> bool {
        flyff_vfs::Vfs::exists(self, canonical_path)
    }
}

/// The canonical form of `path` (14 §10.1): `normalize_resource_path`, or the lower-cased `/` form when the
/// path cannot be normalised.
pub fn canon(path: &str) -> String {
    flyff_common::normalize_resource_path(path)
        .unwrap_or_else(|_| path.replace('\\', "/").to_ascii_lowercase())
}

/// The canonical path of `dir/file` when the file exists, else `None` (the `null` rule).
pub fn resolve(files: &dyn PathResolver, dir: &str, file: &str) -> Option<String> {
    if file.is_empty() {
        return None;
    }
    let p = if dir.is_empty() {
        canon(file)
    } else {
        canon(&format!("{dir}/{file}"))
    };
    files.exists(&p).then_some(p)
}
