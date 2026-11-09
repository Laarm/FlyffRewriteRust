//! `.chr` skeleton files, versions 4-7 (Plan/30 §4; ref/18 §5; `CBones::LoadBone`, src:_Common/Bone.cpp:76-175).
//!
//! Layout (little-endian, no padding): `i32 ver (>= 4)`, `i32 id`, `i32 nBone (1..=255)`, per bone
//! `NSTR name (<= 32)`, `MAT tm` (bind pose, model space), `MAT inverseTM`, `MAT localTM`, `i32 parentIdx`;
//! then `i32 bSendVS`, `MAT localRH` (right-hand grip, relative to R Hand), `MAT localShield` (L ForeArm),
//! `MAT localKnuckle` (R ForeArm); events `VEC3[4] + i32[4]` for ver == 5, `VEC3[8] + i32[8]` for
//! ver >= 6 (unused slots have parent -1); `MAT localLH` for ver == 7 only.
//!
//! The parser keeps the file exactly as stored and does not enforce the invariants that hold for all 301
//! corpus files (exactly one root, `parentIdx < index`, `TM = localTM * TM[parent]`); the skeleton pack
//! (`pack::skeleton`) validates what its consumers rely on.

use crate::FormatKind;
use crate::error::{ErrorKind, FormatError, FormatWarning, Parsed, WarningCode};
use crate::limits::{MAX_BONE_NAME_LEN, MAX_BONES, MAX_MDL_EVENTS};
use crate::motion::{checked_bytes, f32s};
use crate::raw::{D3dMatrix, RawStr, Vec3};
use crate::reader::ByteReader;

/// `VER_BONE`: the oldest accepted version.
pub const VER_BONE: i32 = 4;
/// Highest version the original understands; anything above is parsed like v6 with a warning.
pub const MAX_KNOWN_VERSION: i32 = 7;

/// Smallest per-bone record: name length word + 3 matrices + parent.
const MIN_BONE_BYTES: usize = 4 + 3 * 64 + 4;

/// One bone (Plan/30 §4.2).
#[derive(Debug, Clone, PartialEq)]
pub struct ChrBone {
    /// Name, CP949 or ASCII bytes (at most 32).
    pub name: RawStr,
    /// Bind-pose matrix in model space.
    pub tm: D3dMatrix,
    /// Inverse bind matrix (used by skinning).
    pub inverse_tm: D3dMatrix,
    /// Local bind matrix (`tm = local_tm * tm[parent]`).
    pub local_tm: D3dMatrix,
    /// Parent index, `-1` for the root.
    pub parent: i32,
}

/// One event point: a position in the parent bone's space (Plan/30 §4.2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChrEvent {
    /// Position relative to `parent`.
    pub pos: Vec3,
    /// Parent bone index, `-1` for an unused slot.
    pub parent: i32,
}

/// The four special bone indices (`RHandIdx` etc.), found by name (Plan/30 §4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SpecialBones {
    /// `"r hand"`.
    pub r_hand: i32,
    /// `"l hand"`.
    pub l_hand: i32,
    /// `"l forearm"`.
    pub l_arm: i32,
    /// `"r forearm"`.
    pub r_arm: i32,
}

/// A parsed `.chr` (Plan/30 §4.2).
#[derive(Debug, Clone, PartialEq)]
pub struct ChrFile {
    /// `ver` (corpus 4-7).
    pub version: i32,
    /// Serial id, unused.
    pub id: i32,
    /// The bones in file order.
    pub bones: Vec<ChrBone>,
    /// `bSendVS`: every bone fits one vertex-shader palette (nBone <= 28).
    pub send_vs: i32,
    /// Right-hand grip relative to the R Hand bone.
    pub local_rh: D3dMatrix,
    /// Shield grip relative to the L ForeArm bone.
    pub local_shield: D3dMatrix,
    /// Knuckle-weapon grip relative to the R ForeArm bone.
    pub local_knuckle: D3dMatrix,
    /// Raw event slots: none for v4, 4 for v5, 8 for v >= 6 (unused slots keep parent -1).
    pub events: Vec<ChrEvent>,
    /// Left-hand grip relative to the L Hand bone; `Some` only for v7.
    pub local_lh: Option<D3dMatrix>,
}

impl ChrFile {
    /// The special bone indices, reproducing src:_Common/Bone.cpp:122-129 exactly: for each bone in
    /// order the name **after its first 6 bytes** (`"Bip02 "` / `"Bip01 "`) is compared ASCII
    /// case-insensitively with `"r hand"`, `"l hand"`, `"l forearm"`, `"r forearm"`; the last match wins;
    /// the default is 0. A name shorter than 7 bytes never matches (the original compared into its zeroed
    /// buffer).
    pub fn special_bones(&self) -> SpecialBones {
        let mut out = SpecialBones::default();
        for (i, bone) in self.bones.iter().enumerate() {
            let rest = bone.name.as_bytes().get(6..).unwrap_or(&[]);
            let idx = i as i32; // BOUNDS: at most 255 bones (parser cap)
            if rest.eq_ignore_ascii_case(b"r hand") {
                out.r_hand = idx;
            }
            if rest.eq_ignore_ascii_case(b"l hand") {
                out.l_hand = idx;
            }
            if rest.eq_ignore_ascii_case(b"l forearm") {
                out.l_arm = idx;
            }
            if rest.eq_ignore_ascii_case(b"r forearm") {
                out.r_arm = idx;
            }
        }
        out
    }
}

/// Parses a `.chr`. Versions above 7 produce `ChrUnknownVersion` (parsed like v6); bytes after the last
/// field produce `TrailingBytes`.
pub fn parse(b: &[u8]) -> Result<Parsed<ChrFile>, FormatError> {
    let mut r = ByteReader::new(b, FormatKind::Chr);
    let mut warnings = Vec::new();

    let version = r.i32()?;
    if version < VER_BONE {
        return Err(r.err_at(
            0,
            ErrorKind::UnsupportedVersion {
                found: i64::from(version),
            },
        ));
    }
    if version > MAX_KNOWN_VERSION {
        warnings.push(FormatWarning::new(
            0,
            WarningCode::ChrUnknownVersion,
            format!("version {version} is newer than {MAX_KNOWN_VERSION}; parsed like v6"),
        ));
    }
    let id = r.i32()?;

    let count_at = r.pos();
    let bone_count = r.count("nBone", MIN_BONE_BYTES, MAX_BONES)?;
    if bone_count == 0 {
        return Err(r.err_at(
            count_at,
            ErrorKind::InvalidValue {
                field: "nBone",
                value: 0,
            },
        ));
    }
    let mut bones = Vec::with_capacity(bone_count);
    for _ in 0..bone_count {
        let name = r.nstr("bone name", MAX_BONE_NAME_LEN)?;
        let tm = r.mat()?;
        let inverse_tm = r.mat()?;
        let local_tm = r.mat()?;
        let parent = r.i32()?;
        bones.push(ChrBone {
            name,
            tm,
            inverse_tm,
            local_tm,
            parent,
        });
    }

    let send_vs = r.i32()?;
    let local_rh = r.mat()?;
    let local_shield = r.mat()?;
    let local_knuckle = r.mat()?;

    let event_slots = match version {
        5 => 4,
        v if v >= 6 => MAX_MDL_EVENTS as usize,
        _ => 0,
    };
    let mut events = Vec::with_capacity(event_slots);
    if event_slots > 0 {
        // All positions first, then all parents (two separate `Read` calls in the original).
        let pos_bytes = checked_bytes(&r, event_slots, 12)?;
        let positions = r.bytes(pos_bytes)?;
        let (pos_chunks, _) = positions.as_chunks::<12>();
        let parent_bytes = checked_bytes(&r, event_slots, 4)?;
        let parents = r.bytes(parent_bytes)?;
        let (parent_chunks, _) = parents.as_chunks::<4>();
        for (p, par) in pos_chunks.iter().zip(parent_chunks) {
            events.push(ChrEvent {
                pos: f32s::<3>(p),
                parent: i32::from_le_bytes(*par),
            });
        }
    }
    let local_lh = if version == 7 { Some(r.mat()?) } else { None };

    if !r.at_end() {
        warnings.push(FormatWarning::new(
            r.pos(),
            WarningCode::TrailingBytes,
            format!("{} byte(s) after the last field", r.remaining()),
        ));
    }
    Ok(Parsed {
        value: ChrFile {
            version,
            id,
            bones,
            send_vs,
            local_rh,
            local_shield,
            local_knuckle,
            events,
            local_lh,
        },
        warnings,
    })
}
