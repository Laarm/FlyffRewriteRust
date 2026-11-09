//! [`ServerDerived`]: the server-side values decoded from the start-up kinds (Plan/01 §5.5, 24 §11.1
//! step 6).
//!
//! `AssetPrep::load_server_derived` runs the jobs of the start-up kinds and then calls
//! `AssetBuilder::load_server` of each registered kind, which fills its field here. Until the builder
//! WPs register their kinds (WP-WEB-11/12/14/18/19) the value is empty (Plan/10 §3.1, M1 has no world
//! collision consumers). Fields are added by the WPs that own the types: `collision`
//! (`CollisionSet`, WP-WEB-14), `ctrl_models` (WP-WEB-19) and `rigs` (`Arc<BossRigs>`, WP-WEB-18).

use crate::build::collision::CollisionSet;
use crate::build::rigs::BossRigs;
use crate::kinds::AssetKind;
use flyff_sim::MotionTable;
use flyff_sim::bounds::ModelBounds;
use flyff_sim::collision::CtrlModelSet;
use std::collections::BTreeSet;
use std::sync::Arc;

/// Shared read-mostly server data derived from `Resource/` (cloned cheaply: every field is an `Arc`).
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct ServerDerived {
    /// The binary motion table (`MotionTable::decode(all.bin)`, WP-WEB-11).
    pub motions: Option<Arc<MotionTable>>,
    /// The model-bounds table (`ModelBounds::decode(all.bin)`, WP-WEB-12).
    pub bounds: Option<Arc<ModelBounds>>,
    /// One collision world per world folder, shared by the world ids of the folder (WP-WEB-14).
    pub collision: Option<Arc<CollisionSet>>,
    /// The `FLYK` ctrl collision models (`decode_ctrl_models(all.bin)`, WP-WEB-19).
    pub ctrl_models: Option<Arc<CtrlModelSet>>,
    /// The boss rig index and the `SimSkeleton` / `SimClip`s it names, materialised at start-up (WP-WEB-18).
    pub rigs: Option<Arc<BossRigs>>,
    /// The kinds whose `load_server` hook ran successfully, for diagnostics and tests.
    pub loaded: BTreeSet<AssetKind>,
}

impl ServerDerived {
    /// `true` when no kind contributed anything.
    pub fn is_empty(&self) -> bool {
        self.motions.is_none()
            && self.bounds.is_none()
            && self.collision.is_none()
            && self.ctrl_models.is_none()
            && self.rigs.is_none()
            && self.loaded.is_empty()
    }
}
