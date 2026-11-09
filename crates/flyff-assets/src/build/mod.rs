//! The builders of `flyff-assets` (Plan/24 §10). Each module owns one kind, declares
//! `BUILDER_VERSION` (and `FORMAT_VERSION` where it has a byte format) and implements
//! [`crate::AssetBuilder`]; [`default_builders`] is the list `AssetPrep::start` registers (the kinds
//! enabled by `AssetConfig::features` are kept).
//!
//! Adding a builder: create `build/<name>.rs`, add the `pub mod` line and one `Arc::new(..)` entry in
//! [`default_builders`]. Builders that need start-up knowledge shared with others (the DDS fix list, the
//! cursor hotspots) read and write the [`ManifestParts`] they are given here.

use crate::kinds::AssetBuilder;
use crate::manifest::ManifestParts;
use std::sync::Arc;

pub mod admin_map;
pub mod bc_encode;
pub mod bounds;
pub mod collision;
pub mod ctrl;
pub mod cursor;
pub mod dds;
pub mod external;

pub mod gz;
pub mod motion;
pub mod png;
pub mod rigs;

/// Every builder compiled into this crate (WP-WEB-10: `gz`; the other WPs append their own).
pub fn default_builders(parts: &Arc<ManifestParts>) -> Vec<Arc<dyn AssetBuilder>> {
    vec![
        Arc::new(gz::GzBuilder::new(Arc::clone(parts))),
        Arc::new(collision::CollisionBuilder::new()),
        Arc::new(ctrl::CtrlBuilder::new()),
        Arc::new(rigs::BossRigsBuilder::new()),
        Arc::new(motion::MotionBuilder),
        Arc::new(bounds::BoundsBuilder),
        Arc::new(png::PngBuilder),
        Arc::new(cursor::CursorBuilder),
        Arc::new(dds::DdsBuilder::new()),
        Arc::new(admin_map::AdminMapBuilder),
    ]
}
