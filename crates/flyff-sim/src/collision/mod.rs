//! The collision world (Plan/31 §10): the `FLYC` chunk codec, terrain queries, static-object index and
//! world queries, ground collision.

pub mod chunk;
pub mod client_query;
pub mod ctrl_models;
pub mod dynamic;
pub mod geom;
pub mod ground;
pub mod linkmap;
pub mod query;
pub mod terrain;
pub mod world;

pub use chunk::{
    Attrs, CHUNK_FORMAT_VERSION, CollisionMesh, Heights, LandChunk, LinkCell, Obb, StaticInstance,
    decode_chunk, encode_chunk,
};
pub use ctrl_models::{
    CTRL_MODELS_FORMAT_VERSION, CtrlModel, CtrlModelSet, RigidObject, decode_ctrl_models,
};
pub use dynamic::{CtrlCollider, DynamicColliders};
pub use linkmap::link_placement;
pub use terrain::QueryCtx;
pub use world::{CollisionWorld, InitialLands, LandSlot, WorldDesc};
