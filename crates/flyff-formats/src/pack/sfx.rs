//! The `.sfx` pack (`pack::sfx::build`, Plan/30 §13.6): effect prototypes for the renderer, no blob.
//!
//! Every part carries the key list the original held after loading ([`SfxPart::effective_keys`]: stable
//! sort by frame, one key per frame), the expanded texture-animation names ([`SfxPart::frame_textures`]),
//! the particle block and the custom-mesh ring resolution. Strings are ASCII-lossy (`to_string_lossy_ascii`:
//! non-ASCII CP949 bytes appear as U+FFFD; texture and model names are plain ASCII). Alpha values are raw:
//! the renderer wraps them with `& 0xFF` (ref/20 §4.7, Q17).

use crate::error::{FormatError, Parsed};
use crate::sfx::{ParticleParams, SfxFile, SfxKey, SfxPart, SfxPartKind, SfxVersion};

/// The part type (`SFXPARTTYPE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", serde(rename_all = "snake_case"))]
pub enum PartKind {
    /// Camera-facing / flat / free quad.
    Bill,
    /// Particle emitter (`particle` is set).
    Particle,
    /// A `Model/` mesh (`texture` names the `.o3d`).
    Mesh,
    /// Procedural cylinder/cone (`points` is set).
    CustomMesh,
}

/// One part (layer) of an effect.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SfxPartMeta {
    /// Part type.
    pub kind: PartKind,
    /// Layer name.
    pub name: String,
    /// `SFX/Texture/` file, or for [`PartKind::Mesh`] a `Model/` `.o3d`.
    pub texture: String,
    /// Frames of the texture animation (1 = none).
    pub tex_frame: u16,
    /// Frames per full texture-animation cycle.
    pub tex_loop: u16,
    /// `false` = hidden editor layer.
    pub in_use: bool,
    /// `billType` (1 billboard, 2 bottom, 3 pole, 4 normal).
    pub bill_type: i32,
    /// `alphaType` (1 blend, 2 glow).
    pub alpha_type: i32,
    /// Ring resolution of a custom mesh.
    pub points: Option<i32>,
    /// The particle block of a particle part.
    pub particle: Option<ParticleParams>,
    /// The effective key list (sorted by frame, one key per frame).
    pub keys: Vec<SfxKey>,
    /// The texture names of the texture animation, empty when `tex_frame <= 1` or for mesh parts.
    pub frame_textures: Vec<String>,
}

/// The structured description of an effect (Plan/30 §13.6).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SfxMeta {
    /// Which of the four file formats this was.
    pub version: SfxVersion,
    /// The parts in file order (draw order).
    pub parts: Vec<SfxPartMeta>,
    /// Parser warnings, `"<WarningCode>: <detail>"`.
    pub warnings: Vec<String>,
}

fn part_meta(part: &SfxPart) -> SfxPartMeta {
    let (kind, points, particle) = match &part.kind {
        SfxPartKind::Bill => (PartKind::Bill, None, None),
        SfxPartKind::Particle(p) => (PartKind::Particle, None, Some(p.clone())),
        SfxPartKind::Mesh => (PartKind::Mesh, None, None),
        SfxPartKind::CustomMesh { points } => (PartKind::CustomMesh, Some(*points), None),
    };
    SfxPartMeta {
        kind,
        name: part.name.to_string_lossy_ascii(),
        texture: part.texture.to_string_lossy_ascii(),
        tex_frame: part.tex_frame,
        tex_loop: part.tex_loop,
        in_use: part.in_use != 0,
        bill_type: part.bill_type,
        alpha_type: part.alpha_type,
        points,
        particle,
        keys: part.effective_keys(),
        frame_textures: part
            .frame_textures()
            .iter()
            .map(|t| t.to_string_lossy_ascii())
            .collect(),
    }
}

/// Builds the effect description of a parsed `.sfx`. It cannot fail today (the `Result` keeps the signature
/// uniform with the other packs).
pub fn build(parsed: &Parsed<SfxFile>) -> Result<SfxMeta, FormatError> {
    Ok(SfxMeta {
        version: parsed.value.version,
        parts: parsed.value.parts.iter().map(part_meta).collect(),
        warnings: parsed
            .warnings
            .iter()
            .map(|w| format!("{:?}: {}", w.code, w.detail))
            .collect(),
    })
}
