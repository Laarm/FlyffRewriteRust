//! `.sfx` effect prototypes (Plan/30 §10; ref/20 §3; `CSfxBase::Load`, src:_Common/sfxbase.cpp:1719-1800 and the
//! part loaders `OldLoad`/`Load`/`Load2`/`Load3` of `CSfxPartBill/Particle/Mesh/CustomMesh`, :421-1300).
//!
//! # Layout
//!
//! ```text
//! [char magic[8]]            "SFX0.1  " | "SFX0.2  " | "SFX0.3  " (the first 6 chars decide); absent = "old" format
//! i32 nPart
//! nPart x { i32 partType (1 BILL, 2 PARTICLE, 3 MESH, 4 CUSTOMMESH); part body }
//! ```
//!
//! The part body is `name` (LSTR) -> [`texture` (LSTR), 0.2/0.3 only] -> `texFrame`, `texLoop` (u16, from 0.1) ->
//! `bUse` (i32, 0.2/0.3) -> `billType`, `alphaType` (i32) -> particle block (partType 2) -> keys (`i32 nKey`, 54
//! bytes each) -> `i32 nPoints` (partType 4). Which loader reads which part is the table of Plan/30 §10.1:
//!
//! | header | BILL / MESH / CUSTOMMESH | PARTICLE |
//! |---|---|---|
//! | none | `OldLoad` | `OldLoad` |
//! | `SFX0.1` | `Load` | `Load` |
//! | `SFX0.2` | `Load2` | `Load2` |
//! | `SFX0.3` | `Load2` | `Load3` |
//!
//! The parser is a faithful image of the file ([`SfxFile`]); it never sorts or clamps (R4). The helpers
//! [`SfxPart::effective_keys`] and [`SfxPart::frame_textures`] reproduce what the original loader derived from it.

use crate::FormatKind;
use crate::error::{ErrorKind, FormatError, FormatWarning, Parsed, WarningCode};
use crate::limits::{MAX_SFX_KEYS, MAX_SFX_PARTS, MAX_SFX_STRING_LEN};
use crate::raw::{RawStr, Vec3};
use crate::reader::ByteReader;

/// `SFXPARTTYPE_BILL` (sfxbase.h:17).
pub const PART_BILL: i32 = 1;
/// `SFXPARTTYPE_PARTICLE`.
pub const PART_PARTICLE: i32 = 2;
/// `SFXPARTTYPE_MESH`.
pub const PART_MESH: i32 = 3;
/// `SFXPARTTYPE_CUSTOMMESH`.
pub const PART_CUSTOMMESH: i32 = 4;

/// `SFXPARTBILLTYPE_BILL`: faces the camera.
pub const BILL_TYPE_BILL: i32 = 1;
/// `SFXPARTBILLTYPE_BOTTOM`: lies flat on XZ.
pub const BILL_TYPE_BOTTOM: i32 = 2;
/// `SFXPARTBILLTYPE_POLE` (unfinished in the original: identical to BILL).
pub const BILL_TYPE_POLE: i32 = 3;
/// `SFXPARTBILLTYPE_NORMAL`: free 3D orientation.
pub const BILL_TYPE_NORMAL: i32 = 4;

/// `SFXPARTALPHATYPE_BLEND`: destination factor `INVSRCALPHA`.
pub const ALPHA_TYPE_BLEND: i32 = 1;
/// `SFXPARTALPHATYPE_GLOW`: destination factor `ONE` (additive).
pub const ALPHA_TYPE_GLOW: i32 = 2;

/// `CustomMesh::m_nPoints` constructor value, kept when the file ends right before the field.
pub const DEFAULT_CUSTOM_MESH_POINTS: i32 = 20;

/// Size of one keyframe on disk: `u16 frame` + 4 x VEC3 + `i32 alpha`.
const KEY_SIZE: usize = 2 + 4 * 12 + 4;
/// Smallest possible part: `partType`, `name` length, `billType`, `alphaType`, `nKey` (old format, empty name).
const MIN_PART_SIZE: usize = 5 * 4;

/// The four on-disk versions (Plan/30 §10.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum SfxVersion {
    /// No header: the file starts with `nPart` (`OldLoad`).
    #[cfg_attr(feature = "wasm", serde(rename = "old"))]
    Old,
    /// `"SFX0.1  "` (`Load`).
    #[cfg_attr(feature = "wasm", serde(rename = "0.1"))]
    V01,
    /// `"SFX0.2  "` (`Load2`).
    #[cfg_attr(feature = "wasm", serde(rename = "0.2"))]
    V02,
    /// `"SFX0.3  "` (`Load2`, particles `Load3`).
    #[cfg_attr(feature = "wasm", serde(rename = "0.3"))]
    V03,
}

impl SfxVersion {
    /// The 8-byte header of this version, `None` for [`SfxVersion::Old`].
    pub fn magic(self) -> Option<&'static [u8; 8]> {
        match self {
            SfxVersion::Old => None,
            SfxVersion::V01 => Some(b"SFX0.1  "),
            SfxVersion::V02 => Some(b"SFX0.2  "),
            SfxVersion::V03 => Some(b"SFX0.3  "),
        }
    }

    /// Recognises the header at the start of `b`: the original compares the first six characters only.
    pub fn from_header(b: &[u8]) -> SfxVersion {
        match b.get(..6) {
            Some(b"SFX0.1") => SfxVersion::V01,
            Some(b"SFX0.2") => SfxVersion::V02,
            Some(b"SFX0.3") => SfxVersion::V03,
            _ => SfxVersion::Old,
        }
    }

    /// `true` for the 0.2/0.3 layout: own `texture` string, `bUse`, extended particle block.
    fn is_v2_layout(self) -> bool {
        matches!(self, SfxVersion::V02 | SfxVersion::V03)
    }
}

/// A parsed `.sfx` file (Plan/30 §10.3).
#[derive(Debug, Clone, PartialEq)]
pub struct SfxFile {
    /// Which of the four formats the file is in.
    pub version: SfxVersion,
    /// The parts in file order (draw order).
    pub parts: Vec<SfxPart>,
}

/// One layer of an effect (`CSfxPart`).
#[derive(Debug, Clone, PartialEq)]
pub struct SfxPart {
    /// Part type and its type-specific data.
    pub kind: SfxPartKind,
    /// `m_strName`: the layer name (CP949 in 140 corpus parts).
    pub name: RawStr,
    /// `m_strTex`: file in `SFX/Texture/` (for [`SfxPartKind::Mesh`] a `Model/` `.o3d`). Equal to `name` in the
    /// old and 0.1 formats, which store one string.
    pub texture: RawStr,
    /// `m_nTexFrame`: frames of the texture animation (1 = none; old format: 1).
    pub tex_frame: u16,
    /// `m_nTexLoop`: frames per full texture-animation cycle (old format: 1).
    pub tex_loop: u16,
    /// `m_bUseing` (`BOOL`): 0 = hidden editor layer (old and 0.1 formats: 1).
    pub in_use: i32,
    /// `billType` (raw `i32`; see the `BILL_TYPE_*` constants).
    pub bill_type: i32,
    /// `alphaType` (raw `i32`; see the `ALPHA_TYPE_*` constants).
    pub alpha_type: i32,
    /// The keyframes in STORED order. The original inserted them sorted and merged duplicates; use
    /// [`SfxPart::effective_keys`] for that view.
    pub keys: Vec<SfxKey>,
}

/// The type of a part with its type-specific data (`SFXPARTTYPE`).
#[derive(Debug, Clone, PartialEq)]
pub enum SfxPartKind {
    /// Camera-facing / flat / free quad.
    Bill,
    /// Particle emitter.
    Particle(ParticleParams),
    /// A `Model/` mesh (`texture` names the `.o3d`).
    Mesh,
    /// Procedural cylinder/cone.
    CustomMesh {
        /// `m_nPoints`: ring resolution (default 20 when the file ends before the field).
        points: i32,
    },
}

impl SfxPartKind {
    /// The on-disk `partType` word.
    pub fn type_id(&self) -> i32 {
        match self {
            SfxPartKind::Bill => PART_BILL,
            SfxPartKind::Particle(_) => PART_PARTICLE,
            SfxPartKind::Mesh => PART_MESH,
            SfxPartKind::CustomMesh { .. } => PART_CUSTOMMESH,
        }
    }
}

/// The particle block of a partType-2 part (Plan/30 §10.2); fields that the file version does not store are
/// zero (the loaders zeroed them).
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ParticleParams {
    /// `m_nParticleCreate`: spawn interval in frames (0 = a single burst at the first key).
    pub create: u16,
    /// `m_nParticleCreateNum`: particles per spawn.
    pub create_num: u16,
    /// `m_nParticleFrameAppear`: age at which the fade-in ends.
    pub appear: u16,
    /// `m_nParticleFrameKeep`: age at which the fade-out starts.
    pub keep: u16,
    /// `m_nParticleFrameDisappear`: age at which the particle dies.
    pub disappear: u16,
    /// `m_fParticleStartPosVar`: spawn radius on the XZ circle.
    pub pos_var: f32,
    /// `m_fParticleStartPosVarY`: spawn Y range.
    pub pos_var_y: f32,
    /// `m_fParticleYLow`: initial vertical speed, low.
    pub y_low: f32,
    /// `m_fParticleYHigh`: initial vertical speed, high.
    pub y_high: f32,
    /// `m_fParticleXZLow`: initial radial speed, low.
    pub xz_low: f32,
    /// `m_fParticleXZHigh`: initial radial speed, high.
    pub xz_high: f32,
    /// `m_vParticleAccel`: added to the speed every frame.
    pub accel: Vec3,
    /// `m_vScale`: initial scale.
    pub scale: Vec3,
    /// `m_vScaleSpeed`: added to the scale every frame.
    pub scale_speed: Vec3,
    /// `m_vRotation` (unused at runtime; 0.2, 0.3).
    pub rotation: Vec3,
    /// `m_vRotationLow`: random per-particle rotation in degrees, low (0.2, 0.3).
    pub rotation_low: Vec3,
    /// `m_vRotationHigh`: random per-particle rotation in degrees, high (0.2, 0.3).
    pub rotation_high: Vec3,
    /// `m_bRepeatScal`: ping-pong scale mode (0.2, 0.3).
    pub repeat_scale: i32,
    /// `m_bRepeat`: loop the whole effect (0.3 only).
    pub repeat: i32,
    /// `m_fScalSpeedXLow, XHigh, YLow, YHigh, ZLow, ZHigh` (0.2, 0.3).
    pub scale_speed_range: [f32; 6],
    /// `m_vScaleSpeed2` (unused at runtime; 0.2, 0.3).
    pub scale_speed2: Vec3,
    /// `m_vScaleEnd`: ping-pong upper bound (0.2, 0.3).
    pub scale_end: Vec3,
}

/// One 54-byte keyframe (`SfxKeyFrame`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SfxKey {
    /// Position in 60 Hz frames.
    pub frame: u16,
    /// Local offset.
    pub pos: Vec3,
    /// Rotation in degrees applied to `pos` (CUSTOMMESH: UV parameters).
    pub pos_rotate: Vec3,
    /// Scale (CUSTOMMESH: top radius, height, bottom radius).
    pub scale: Vec3,
    /// Part rotation in degrees.
    pub rotate: Vec3,
    /// Raw alpha. Values like 2555 occur and the renderer wraps `& 0xFF` (ref/20 §4.7): never clamped here.
    pub alpha: i32,
}

/// Parses a `.sfx` (Plan/30 §10; WP-FMT-06).
///
/// Errors: truncated data, a negative or oversized part/key count, a string longer than 254 bytes (the
/// original's 255-byte read buffer), and an unknown part type (the original would crash). A custom-mesh
/// part whose `nPoints` field is missing because the file ends exactly there keeps the constructor value
/// 20 and reports [`WarningCode::SfxPointsMissing`]; bytes after the last part are a
/// [`WarningCode::TrailingBytes`] warning.
pub fn parse(b: &[u8]) -> Result<Parsed<SfxFile>, FormatError> {
    let mut r = ByteReader::new(b, FormatKind::Sfx);
    let mut warnings = Vec::new();
    let version = SfxVersion::from_header(b);
    if version != SfxVersion::Old {
        r.skip(8)?;
    }
    let n_parts = r.count("nPart", MIN_PART_SIZE - 4, MAX_SFX_PARTS)?;
    let mut parts = Vec::with_capacity(n_parts);
    for _ in 0..n_parts {
        parts.push(read_part(&mut r, version, &mut warnings)?);
    }
    if r.remaining() > 0 {
        warnings.push(FormatWarning::new(
            r.pos(),
            WarningCode::TrailingBytes,
            format!("{} byte(s) after the last part", r.remaining()),
        ));
    }
    Ok(Parsed {
        value: SfxFile { version, parts },
        warnings,
    })
}

/// One part: `partType`, the common header, the particle block, the keys and the custom-mesh trailer.
fn read_part(
    r: &mut ByteReader<'_>,
    version: SfxVersion,
    warnings: &mut Vec<FormatWarning>,
) -> Result<SfxPart, FormatError> {
    let type_at = r.pos();
    let part_type = r.i32()?;
    if !(PART_BILL..=PART_CUSTOMMESH).contains(&part_type) {
        return Err(r.err_at(
            type_at,
            ErrorKind::InvalidValue {
                field: "partType",
                value: i64::from(part_type),
            },
        ));
    }

    let name = r.lstr("name", MAX_SFX_STRING_LEN)?;
    let (texture, tex_frame, tex_loop, in_use) = match version {
        SfxVersion::Old => (name.clone(), 1, 1, 1),
        SfxVersion::V01 => {
            let frame = r.u16()?;
            let tex_loop = r.u16()?;
            (name.clone(), frame, tex_loop, 1)
        }
        SfxVersion::V02 | SfxVersion::V03 => {
            let texture = r.lstr("texture", MAX_SFX_STRING_LEN)?;
            let frame = r.u16()?;
            let tex_loop = r.u16()?;
            let in_use = r.i32()?;
            (texture, frame, tex_loop, in_use)
        }
    };
    let bill_type = r.i32()?;
    let alpha_type = r.i32()?;

    let mut particle = None;
    if part_type == PART_PARTICLE {
        particle = Some(read_particle_block(r, version)?);
    }

    let n_keys = r.count("nKey", KEY_SIZE, MAX_SFX_KEYS)?;
    let mut keys = Vec::with_capacity(n_keys);
    for _ in 0..n_keys {
        keys.push(read_key(r)?);
    }

    let kind = match part_type {
        PART_PARTICLE => SfxPartKind::Particle(particle.unwrap_or_default()),
        PART_MESH => SfxPartKind::Mesh,
        PART_CUSTOMMESH => {
            let points = if r.remaining() == 0 {
                warnings.push(FormatWarning::new(
                    r.pos(),
                    WarningCode::SfxPointsMissing,
                    format!(
                        "custom mesh `{}` ends the file before nPoints; the default {DEFAULT_CUSTOM_MESH_POINTS} is kept",
                        name.to_string_lossy_ascii()
                    ),
                ));
                DEFAULT_CUSTOM_MESH_POINTS
            } else {
                r.i32()?
            };
            SfxPartKind::CustomMesh { points }
        }
        _ => SfxPartKind::Bill,
    };

    Ok(SfxPart {
        kind,
        name,
        texture,
        tex_frame,
        tex_loop,
        in_use,
        bill_type,
        alpha_type,
        keys,
    })
}

/// The particle block: 70 bytes (old, 0.1), 158 (0.2) or 162 (0.3).
fn read_particle_block(
    r: &mut ByteReader<'_>,
    version: SfxVersion,
) -> Result<ParticleParams, FormatError> {
    let mut p = ParticleParams {
        create: r.u16()?,
        create_num: r.u16()?,
        appear: r.u16()?,
        keep: r.u16()?,
        disappear: r.u16()?,
        pos_var: r.f32()?,
        pos_var_y: r.f32()?,
        y_low: r.f32()?,
        y_high: r.f32()?,
        xz_low: r.f32()?,
        xz_high: r.f32()?,
        accel: r.vec3()?,
        scale: r.vec3()?,
        scale_speed: r.vec3()?,
        ..ParticleParams::default()
    };
    if version.is_v2_layout() {
        p.rotation = r.vec3()?;
        p.rotation_low = r.vec3()?;
        p.rotation_high = r.vec3()?;
        p.repeat_scale = r.i32()?;
        if version == SfxVersion::V03 {
            p.repeat = r.i32()?;
        }
        for v in &mut p.scale_speed_range {
            *v = r.f32()?;
        }
        p.scale_speed2 = r.vec3()?;
        p.scale_end = r.vec3()?;
    }
    Ok(p)
}

/// One 54-byte key.
fn read_key(r: &mut ByteReader<'_>) -> Result<SfxKey, FormatError> {
    Ok(SfxKey {
        frame: r.u16()?,
        pos: r.vec3()?,
        pos_rotate: r.vec3()?,
        scale: r.vec3()?,
        rotate: r.vec3()?,
        alpha: r.i32()?,
    })
}

impl SfxPart {
    /// The key list the original held after loading (`AddKeyFrame` + `AdjustKeyFrame`, sfxbase.cpp:44-87,
    /// 145): sorted by frame, one key per frame. A duplicate frame keeps the FIRST position in the list and
    /// the LAST value read (the later record overwrites the earlier one in place).
    pub fn effective_keys(&self) -> Vec<SfxKey> {
        let mut sorted = self.keys.clone();
        // Stable: equal frames stay in file order, so the last of a run is the last one read.
        sorted.sort_by_key(|k| k.frame);
        let mut out: Vec<SfxKey> = Vec::with_capacity(sorted.len());
        for k in sorted {
            match out.last_mut() {
                Some(last) if last.frame == k.frame => *last = k,
                _ => out.push(k),
            }
        }
        out
    }

    /// The texture file names of the texture animation (`GetTextureName(tex, i)`, sfxbase.cpp:21-33).
    ///
    /// The extension is assumed to be exactly 3 characters; the two characters before `.ext` are `atoi`'d to a
    /// start number `n`, and frame `i` is `tex[..len-6] + "%02d"(n + i) + tex[len-4..]`. Empty when
    /// `tex_frame <= 1`, and for [`SfxPartKind::Mesh`] parts (the original does not animate mesh textures).
    pub fn frame_textures(&self) -> Vec<RawStr> {
        if self.tex_frame <= 1 || matches!(self.kind, SfxPartKind::Mesh) {
            return Vec::new();
        }
        let tex = self.texture.as_bytes();
        let len = tex.len();
        // CString::Left(len - 4).Right(2)
        let stem = tex.get(..len.saturating_sub(4)).unwrap_or(&[]);
        let digits = stem.get(stem.len().saturating_sub(2)..).unwrap_or(&[]);
        let start = c_atoi_bytes(digits);
        // CString::Left(len - 6) and Right(4)
        let prefix = tex.get(..len.saturating_sub(6)).unwrap_or(&[]);
        let ext = tex.get(len.saturating_sub(4)..).unwrap_or(&[]);
        (0..i32::from(self.tex_frame))
            .map(|i| {
                let number = format!("{:02}", start.wrapping_add(i));
                let mut name = Vec::with_capacity(prefix.len() + number.len() + ext.len());
                name.extend_from_slice(prefix);
                name.extend_from_slice(number.as_bytes());
                name.extend_from_slice(ext);
                RawStr(name.into_boxed_slice())
            })
            .collect()
    }
}

/// C `atoi` for a short byte string: leading white space, an optional sign, then decimal digits; anything
/// else stops the scan (an empty or non-numeric string is 0). Saturates instead of overflowing.
fn c_atoi_bytes(s: &[u8]) -> i32 {
    let mut i = 0usize;
    while s
        .get(i)
        .is_some_and(|c| matches!(c, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r'))
    {
        i += 1;
    }
    let mut negative = false;
    match s.get(i) {
        Some(b'-') => {
            negative = true;
            i += 1;
        }
        Some(b'+') => i += 1,
        _ => {}
    }
    let mut value: i32 = 0;
    while let Some(c) = s.get(i).filter(|c| c.is_ascii_digit()) {
        value = value.saturating_mul(10).saturating_add(i32::from(c - b'0'));
        i += 1;
    }
    if negative { -value } else { value }
}
