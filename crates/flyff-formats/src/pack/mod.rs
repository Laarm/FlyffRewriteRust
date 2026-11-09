//! Packs: render-ready and sim-ready outputs built from the model layer (Plan/30 §13).
//!
//! A pack is a small structured `meta` plus one little-endian `blob` of 4-byte aligned sections
//! ([`blob`]). Parsing is lenient about semantics; packing validates what its consumer relies on and
//! returns [`FormatError`](crate::FormatError)s.
//!
//! * [`o3d`]: the browser mesh pack (`pack::o3d::build`): per-material-block geometry with skin palettes
//!   resolved to global `.chr` bone indices, mapped to three.js space by default.
//! * [`skeleton`]: `FSK1`, the packed skeleton passed unchanged to `SimWorld.add_skeleton`.
//! * [`motion`]: `FMO1`, the packed motion (and node clip) passed unchanged to `SimWorld.add_clip`;
//!   [`AniMeta`] / [`skeleton::ChrMeta`] are the `decode_ani` / `decode_chr` descriptions around them.
//! * [`lnd`]: the land pack (heights, attributes, water, layer alpha, light, placements as typed sections).
//! * [`dds`]: texture levels laid out for WebGL2 upload (BC passthrough or RGBA8).
//! * [`sfx`]: effect prototypes as plain descriptions (no blob).
//!
//! Skeleton and motion packs stay left-handed; mesh packs are emitted already mapped (`space`).

pub mod blob;
pub mod dds;
pub mod lnd;
pub mod motion;
pub mod o3d;
pub mod sfx;
pub mod skeleton;

pub use blob::{BlobBuilder, BlobView, ElemType, Packed};
pub use dds::{BcMode, DdsMeta, DdsPackOptions, LevelMeta};
pub use lnd::{LayerMeta, LndMeta, LndPackOptions, Placements};
pub use motion::{AniMeta, MotionView, OwnedMotion};
pub use o3d::{O3dMeta, O3dPackOptions};
pub use sfx::{SfxMeta, SfxPartMeta};
pub use skeleton::{ChrMeta, ChrPackOptions, OwnedSkeleton, SkeletonView};

/// The coordinate space of a mesh pack (Plan/30 §13.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", serde(rename_all = "lowercase"))]
pub enum Space {
    /// three.js space: `z` negated, winding reversed (default for the browser).
    #[default]
    Three,
    /// The original left-handed Direct3D space (native consumers, the collision builder).
    Lh,
}

/// Which LOD groups a mesh pack contains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", serde(rename_all = "lowercase"))]
pub enum LodSelect {
    /// Every group of the file (1 or 3).
    #[default]
    All,
    /// Only the highest-detail group.
    Lod0,
}
