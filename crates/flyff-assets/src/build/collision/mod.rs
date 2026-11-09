//! Collision chunks `FLYC` v1 (Plan/24 §10.7; WP-WEB-13, WP-WEB-24, WP-WEB-14).
//!
//! | module | contents |
//! |---|---|
//! | [`assemble`] | the content-assembly API (WP-WEB-13): `CollisionInputs`/`LandInput`/`ObjectInput`, [`instance_flags`], [`assemble_world`], [`WorldAssembly`], [`CollisionReport`]; pure, no I/O |
//! | [`world`] | [`build_world_chunks`] (WP-WEB-24): a world folder from the VFS + formats + game data to encoded chunks, no asset framework |
//! | [`builder`] | the `collision` [`AssetBuilder`](crate::AssetBuilder) (WP-WEB-14): cache keys, fingerprints, the start-up hook |
//! | [`set`] | [`CollisionSet`]: one `Arc<CollisionWorld>` per folder for `ServerDerived.collision` |
//!
//! The bytes come only from `flyff_sim::collision::{encode_chunk, decode_chunk}` (W9): this module has no
//! writer of its own.

pub mod assemble;
pub mod builder;
pub mod set;
pub mod world;

pub use assemble::{
    CollisionInputs, CollisionMesh, CollisionReport, LandInput, ObjectInput, WorldAssembly,
    assemble_world, instance_flags, instance_matrices, invert_f64, link_list_order,
};
pub use builder::{CollisionBuilder, chunk_key, report_json, report_key};
pub use set::{
    CollisionSet, LoadFailure, WorldStats, is_housing_world, load_collision_set, world_from_chunks,
};
pub use world::{
    BuildOptions, WorldDims, build_world_chunks, build_world_chunks_with, lnd_path, present_lands,
    world_dims,
};

/// Bump on any change that can alter output bytes for the same inputs (Plan/24 §6.3).
pub const BUILDER_VERSION: u32 = 1;

/// The byte layout version, `CHUNK_FORMAT_VERSION` of the codec (Plan/24 §10.7.1).
pub const FORMAT_VERSION: u32 = flyff_sim::collision::CHUNK_FORMAT_VERSION as u32;
