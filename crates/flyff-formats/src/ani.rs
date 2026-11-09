//! `.ani` motion files, version 10 (Plan/30 §5.1; ref/18 §6; `CMotion::LoadMotion`, src:_Common/Bone.cpp:491-571).
//!
//! Layout (little-endian, no padding): `i32 ver (== 10)`, `i32 id`, `f32 perSlerp`, `u8[32]` reserved,
//! `i32 nBone`, `i32 nFrame (> 0)`, `i32 hasPath`, `VEC3[nFrame] path` (only when `hasPath != 0`),
//! MOTIONCORE (see [`crate::motion`]), `MOTION_ATTR[nFrame]`, `i32 nEvent (0..=4)`, `VEC3[nEvent]`.
//!
//! [`KeyMode::Skip`] is what the server's motion-table builder uses: the key arrays are skipped by
//! offset arithmetic (2,925 files parse in well under a second) while `perSlerp`, the frame count and
//! the attributes are kept.

use crate::FormatKind;
use crate::error::{ErrorKind, FormatError, FormatWarning, Parsed, WarningCode};
use crate::limits::{MAX_ANI_EVENTS, MAX_ANI_FRAMES};
use crate::motion::{MotionAttr, MotionCore, check_bone_count, read_attrs, read_core, read_vec3s};
use crate::raw::Vec3;
use crate::reader::ByteReader;

/// `VER_MOTION`: the only accepted `.ani` version (all 2,925 corpus files).
pub const VER_MOTION: i32 = 10;

/// Whether the key arrays of the MOTIONCORE are loaded or skipped (Plan/30 §5.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyMode {
    /// Load every key and static matrix.
    #[default]
    Load,
    /// Skip the key arrays and static matrices: tracks become `BoneTrack::Skipped` (no allocation).
    Skip,
}

/// Options of [`parse`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AniOptions {
    /// Key loading mode.
    pub keys: KeyMode,
}

/// A parsed `.ani` file (Plan/30 §5.4).
#[derive(Debug, Clone, PartialEq)]
pub struct AniFile {
    /// `ver`, always 10.
    pub version: i32,
    /// Serial id, unused.
    pub id: i32,
    /// Frame advance per tick, raw (the motion clock applies `trunc3`, Plan/31).
    pub per_slerp: f32,
    /// The 32 reserved bytes (kept for round trips).
    pub reserved: [u8; 32],
    /// `nBone` as stored; should equal the skeleton's bone count (consumer check).
    pub bone_count: i32,
    /// `nFrame`, `1..=65535`.
    pub frame_count: i32,
    /// The raw `hasPath` word.
    pub has_path: i32,
    /// Root path, one point per frame; empty unless `has_path != 0` (1 corpus file, never used).
    pub path: Vec<Vec3>,
    /// Bone hierarchy and tracks.
    pub core: MotionCore,
    /// Dense attribute array, one entry per frame (index = `(int)frame`).
    pub attrs: Vec<MotionAttr>,
    /// Event points (0..=4; 6 pet files have one; unused at runtime).
    pub events: Vec<Vec3>,
}

impl AniFile {
    /// `(array index, frame)` of every entry carrying `MA_HIT`.
    pub fn hit_frames(&self) -> impl Iterator<Item = (usize, f32)> + '_ {
        self.attrs
            .iter()
            .enumerate()
            .filter(|(_, a)| a.is_hit())
            .map(|(i, a)| (i, a.frame))
    }
}

/// Parses a `.ani`. A pool-size mismatch is a `MotionPoolMismatch` warning, bytes after the events a
/// `TrailingBytes` warning; a version other than 10, `nFrame <= 0` and any over-long count are errors.
pub fn parse(b: &[u8], opts: &AniOptions) -> Result<Parsed<AniFile>, FormatError> {
    let mut r = ByteReader::new(b, FormatKind::Ani);
    let mut warnings = Vec::new();

    let version = r.i32()?;
    if version != VER_MOTION {
        return Err(r.err_at(
            0,
            ErrorKind::UnsupportedVersion {
                found: i64::from(version),
            },
        ));
    }
    let id = r.i32()?;
    let per_slerp = r.f32()?;
    let mut reserved = [0u8; 32];
    reserved.copy_from_slice(r.bytes(32)?);

    let bone_at = r.pos();
    let bone_count = r.i32()?;
    let bones = check_bone_count(&r, bone_at, bone_count)?;
    let frame_at = r.pos();
    let frame_count = r.i32()?;
    let frames = match usize::try_from(frame_count) {
        Ok(n) if (1..=MAX_ANI_FRAMES as usize).contains(&n) => n,
        _ if frame_count <= 0 => {
            return Err(r.err_at(
                frame_at,
                ErrorKind::InvalidValue {
                    field: "nFrame",
                    value: i64::from(frame_count),
                },
            ));
        }
        _ => {
            return Err(r.err_at(
                frame_at,
                ErrorKind::CountOutOfRange {
                    field: "nFrame",
                    value: i64::from(frame_count),
                    max: i64::from(MAX_ANI_FRAMES),
                },
            ));
        }
    };
    let has_path = r.i32()?;
    let path = if has_path != 0 {
        read_vec3s(&mut r, frames)?
    } else {
        Vec::new()
    };

    let core = read_core(&mut r, bones, frames, opts.keys, &mut warnings)?;
    let attrs = read_attrs(&mut r, frames)?;

    let events_at = r.pos();
    let n_event = r.i32()?;
    let events = match usize::try_from(n_event) {
        Ok(n) if n <= MAX_ANI_EVENTS as usize => read_vec3s(&mut r, n)?,
        _ => {
            return Err(r.err_at(
                events_at,
                ErrorKind::CountOutOfRange {
                    field: "nEvent",
                    value: i64::from(n_event),
                    max: i64::from(MAX_ANI_EVENTS),
                },
            ));
        }
    };

    if !r.at_end() {
        warnings.push(FormatWarning::new(
            r.pos(),
            WarningCode::TrailingBytes,
            format!("{} byte(s) after the last field", r.remaining()),
        ));
    }
    Ok(Parsed {
        value: AniFile {
            version,
            id,
            per_slerp,
            reserved,
            bone_count,
            frame_count,
            has_path,
            path,
            core,
            attrs,
            events,
        },
        warnings,
    })
}
