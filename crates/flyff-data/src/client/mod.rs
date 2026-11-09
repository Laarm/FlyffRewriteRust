//! The client-only loaders (P7, L95; Plan/14 §6.19, WP-DATA-16): window layouts, sound and music tables,
//! terrain textures, texture atlases and the world-map records, the help family and text panels, chat
//! emoticons and the word filter, quest destinations.
//!
//! Every module has always-compiled data types (wasm-safe: `flyff-rules` and `flyff-wasm` decode the
//! bundles with them) and, behind feature `load`, a pure parser `parse_*(text, file, ...)` that knows
//! nothing about the VFS. The glue that reads the files and fills [`crate::ClientData`] is
//! `load::steps::client`.

pub mod atlas;
pub mod filter;
pub mod help;
pub mod map_info;
pub mod panels;
pub mod quest_dest;
pub mod resdata;
pub mod sound;
pub mod terrain;
pub mod text_emotion;

pub use atlas::{Atlas, AtlasSprite};
pub use help::{GuideStep, HelpEntry, HelpTexts, HelpTreeNode};
pub use map_info::{MapMonsterEntry, MapMonsterInfo, RainbowMarker};
pub use resdata::{Applet, Control};
pub use sound::IdFileTable;
pub use terrain::TerrainTex;
