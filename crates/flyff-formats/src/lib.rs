//! `flyff-formats` — tier S (Plan/30): byte-exact parsers for the legacy binary formats
//! (`.o3d .chr .ani .lnd .dyo .sfx .dds .tga .bmp ...`). Pure `&[u8] -> T`, wasm-safe, panic-free.
//! Features: `writers` (test writers), `wasm` (serde descriptors), `ts` (ts-rs derives).
//!
//! Layout (Plan/30 §1.2): [`reader`] is the checked little-endian cursor, [`error`] the error/warning
//! types, [`raw`] the shared value types, [`limits`] the caps table, [`version`] the version constants,
//! [`corpus`] path/magic classification, [`objects`] the object records shared by `.lnd` and `.dyo`,
//! [`dyo`] and [`lnd`] the dynamic-object and landscape parsers. Each further format adds its own module
//! (one `pub mod` line below) and never touches another format's file.

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented
)]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]
#![deny(clippy::iter_over_hash_type)]
#![warn(missing_docs)]

// ---- modules: add one `pub mod <format>;` line per format, keep alphabetical ----
pub mod ani;
pub mod chr;
pub mod corpus;
pub mod dds;
pub mod dyo;
pub mod error;
// bc, bmp, cur, texture and tga (Plan/30 §11) live in `image/` and are re-exported below under their plan names.
mod image;
pub mod limits;
pub mod lnd;
pub mod motion;
pub mod o3d;
pub mod objects;
pub mod pack;
pub mod raw;
pub mod reader;
pub mod sfx;
pub mod space;
pub mod version;
pub mod world_text;

pub use ani::{AniFile, AniOptions, KeyMode};
pub use chr::{ChrFile, SpecialBones};
pub use corpus::sniff;
pub use dyo::DyoFile;
pub use error::{ErrorKind, FormatError, FormatWarning, Parsed, WarningCode};
pub use image::{bc, bmp, cur, texture, tga};
pub use lnd::{
    HeightAttr, LandFile, LandLayer, LandOptions, LandView, LayerMode, Layers, WaterPatch,
    check_header, strip_height_client, strip_height_server,
};
pub use motion::{MA_HIT, MA_QUAKE, MA_SOUND, MotionAttr, MotionCore, TmKey};
pub use o3d::{GeometryMode, O3dFile, O3dOptions};
pub use objects::{CtrlElemDef, MoverExt, ObjBase, ObjExt, ObjRecord, Placement, read_record};
pub use raw::{D3dMatrix, Quat, RawStr, Vec3};
pub use reader::ByteReader;
pub use sfx::{SfxFile, SfxKey, SfxPart, SfxPartKind, SfxVersion};
pub use world_text::{
    CntFile, ContinentDef, PatrolPath, PatrolPoint, RegionDef, RegionReq, RespawnDef, RgnFile,
    WldSettings, parse_24l, parse_cnt, parse_pat, parse_rgn, parse_wld,
};

/// Crate marker used by the workspace smoke test and by `cargo xtask doctor`.
pub const CRATE_NAME: &str = "flyff-formats";

/// The binary and text formats this crate handles (Plan/30 §2.1). Labels errors and drives
/// [`FormatKind::from_path`] / [`sniff`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FormatKind {
    /// `.o3d` mesh.
    O3d,
    /// `.chr` skeleton.
    Chr,
    /// `.ani` motion.
    Ani,
    /// `.lnd` landscape.
    Lnd,
    /// `.dyo` dynamic objects.
    Dyo,
    /// `.sfx` effect prototype.
    Sfx,
    /// DDS texture.
    Dds,
    /// TGA image.
    Tga,
    /// BMP image.
    Bmp,
    /// `.cur` cursor.
    Cur,
    /// `.wld` world script (text).
    Wld,
    /// `.rgn` regions and respawns (text).
    Rgn,
    /// `.wld.cnt` continents (text).
    Cnt,
    /// `.pat` patrol paths (text).
    Pat,
    /// `.24l` light table (text).
    L24,
    /// Packed skeleton blob `FSK1`.
    PackedSkeleton,
    /// Packed motion blob `FMO1`.
    PackedMotion,
}

// Unit-test byte builders (no unwrap/expect/panic so the determinism scanner stays clean).
