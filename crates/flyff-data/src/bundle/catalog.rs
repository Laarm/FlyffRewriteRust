//! The bundle registry (Plan/14 §10.4): [`BundleSpec`] per bundle with its schema version, load class and
//! projection. Adding a bundle adds one row (WP-DATA-15 replaces the `quests` stub the same way).

use super::BundleError;
use super::envelope::encode_envelope;
use super::project::{self, Sources};
use super::types::{
    BundleData, DefinesBundle, HelpBundle, HonorBundle, ItemExtBundle, ItemsBundle, LoadClass,
    ModelBoundsBundle, ModelsBundle, MotionsBundle, MoversBundle, MusicBundle, NamesBundle,
    NpcsBundle, QuestsBundle, ResdataBundle, SfxTableBundle, SkillsBundle, SoundsBundle,
    TablesBundle, TerrainBundle, TextsBundle, UiAtlasesBundle, WorldsBundle,
};

/// A projection: reads the sources and gives the compact JSON of the envelope.
pub type ProjectFn = fn(&Sources<'_>) -> Result<Vec<u8>, BundleError>;

/// One catalogue row.
#[derive(Clone, Copy)]
pub struct BundleSpec {
    /// The bundle name.
    pub name: &'static str,
    /// The schema version.
    pub schema: u32,
    /// Boot or lazy.
    pub load: LoadClass,
    /// The projection, `None` for the two bundles `flyff-assets` produces (`motions`, `model-bounds`).
    pub project: Option<ProjectFn>,
}

macro_rules! spec {
    ($t:ty, $f:path) => {
        BundleSpec {
            name: <$t as BundleData>::NAME,
            schema: <$t as BundleData>::SCHEMA,
            load: <$t as BundleData>::LOAD,
            project: Some(|s| encode_envelope(&$f(s)?)),
        }
    };
}

macro_rules! external {
    ($t:ty) => {
        BundleSpec {
            name: <$t as BundleData>::NAME,
            schema: <$t as BundleData>::SCHEMA,
            load: <$t as BundleData>::LOAD,
            project: None,
        }
    };
}

static CATALOG: [BundleSpec; 22] = [
    spec!(ItemsBundle, project::project_items),
    spec!(ItemExtBundle, project::project_item_ext),
    spec!(SkillsBundle, project::project_skills),
    spec!(MoversBundle, project::project_movers),
    spec!(NpcsBundle, project::project_npcs),
    spec!(TextsBundle, project::project_texts),
    spec!(HelpBundle, project::project_help),
    spec!(WorldsBundle, project::project_worlds),
    spec!(ModelsBundle, project::project_models),
    spec!(SfxTableBundle, project::project_sfx_table),
    external!(MotionsBundle),
    external!(ModelBoundsBundle),
    spec!(TerrainBundle, project::project_terrain),
    spec!(ResdataBundle, project::project_resdata),
    spec!(UiAtlasesBundle, project::project_atlases),
    spec!(SoundsBundle, project::project_sounds),
    spec!(MusicBundle, project::project_music),
    spec!(HonorBundle, project::project_honor),
    spec!(TablesBundle, project::project_tables),
    spec!(NamesBundle, project::project_names),
    spec!(QuestsBundle, project::project_quests),
    spec!(DefinesBundle, project::project_defines),
];

/// The 22 catalogue rows in the order of `BUNDLE_NAMES`.
pub fn catalog() -> &'static [BundleSpec] {
    &CATALOG
}
