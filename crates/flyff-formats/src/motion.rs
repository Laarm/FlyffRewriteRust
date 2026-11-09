//! MOTIONCORE and MOTION_ATTR: the animation payload shared by `.ani` and the embedded motion of `.o3d`
//! (Plan/30 §5.2-§5.4; ref/18 §6.2-§6.3; `CMotion::ReadTM`, src:_Common/Bone.cpp:576-651).
//!
//! MOTIONCORE layout (little-endian, no padding), for `nBone` bones and `nFrame` frames given by the caller:
//!
//! 1. per bone: `NSTR name` (<= 32), `MAT inverseTM`, `MAT localTM`, `i32 parentIdx` — this hierarchy
//!    drives the animation (`AnimateBone`), not the `.chr`'s; `inverseTM` is unused at runtime;
//! 2. `i32 poolSize`, which must equal the sum of `nFrame` over the animated bones (a mismatch is only
//!    logged by the original: warning [`WarningCode::MotionPoolMismatch`]);
//! 3. per bone: `i32 animated`; `== 1` is followed by `TM_ANIMATION[nFrame]` (`QUAT rot`, `VEC3 pos`, 28
//!    bytes each, no scale), any other value by one `MAT localTM` used for every frame (it may carry scale).
//!
//! [`MotionAttr`] is the 12-byte `MOTION_ATTR` (flags, sound id, exact frame) stored densely at the
//! array index `(int)frame`.

use crate::ani::KeyMode;
use crate::error::{ErrorKind, FormatError, FormatWarning, WarningCode};
use crate::limits::{MAX_BONE_NAME_LEN, MAX_BONES};
use crate::raw::{D3dMatrix, Quat, RawStr, Vec3};
use crate::reader::ByteReader;

/// `MA_HIT`: hit / impact frame (`Bone.h:105`).
pub const MA_HIT: u32 = 0x1;
/// `MA_SOUND`: play `sound_id` (`Bone.h:106`).
pub const MA_SOUND: u32 = 0x2;
/// `MA_QUAKE`: camera quake (`Bone.h:107`; compiled out of the original client by `__NO_QUAKE`).
pub const MA_QUAKE: u32 = 0x4;

/// Size in bytes of one `TM_ANIMATION` key (`QUAT` + `VEC3`).
pub const TM_KEY_SIZE: usize = 28;
/// Size in bytes of one `MOTION_ATTR`.
pub const MOTION_ATTR_SIZE: usize = 12;
/// Smallest possible per-bone record of a MOTIONCORE: name length word + 2 matrices + parent + the
/// `animated` word (a bone with an empty name, a static track would add 64 more bytes).
const MIN_CORE_BONE_BYTES: usize = 4 + 64 + 64 + 4 + 4;

/// One `MOTION_ATTR` (12 bytes, Plan/30 §5.3): `flags` (`MA_*`), `sound_id`, and the exact `frame`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionAttr {
    /// `MA_HIT | MA_SOUND | MA_QUAKE` bits (no other bit occurs in the corpus).
    pub flags: u32,
    /// Sound table id, meaningful with `MA_SOUND`.
    pub sound_id: i32,
    /// Exact frame (fractional values occur twice in the corpus); the entry lives at index `(int)frame`.
    pub frame: f32,
}

impl MotionAttr {
    /// `true` when the entry carries `MA_HIT`.
    pub fn is_hit(&self) -> bool {
        self.flags & MA_HIT != 0
    }

    /// `true` when the entry carries `MA_SOUND`.
    pub fn is_sound(&self) -> bool {
        self.flags & MA_SOUND != 0
    }

    /// `true` when the entry carries `MA_QUAKE`.
    pub fn is_quake(&self) -> bool {
        self.flags & MA_QUAKE != 0
    }

    /// `true` for the all-zero filler entries of the dense array (no flag, no sound, frame bits 0).
    pub fn is_empty(&self) -> bool {
        self.flags == 0 && self.sound_id == 0 && self.frame.to_bits() == 0
    }

    /// The 12 on-disk bytes.
    pub fn to_le_bytes(&self) -> [u8; MOTION_ATTR_SIZE] {
        let [a, b, c, d] = self.flags.to_le_bytes();
        let [e, f, g, h] = self.sound_id.to_le_bytes();
        let [i, j, k, l] = self.frame.to_bits().to_le_bytes();
        [a, b, c, d, e, f, g, h, i, j, k, l]
    }
}

/// One `TM_ANIMATION` key (28 bytes): rotation quaternion `x, y, z, w` and translation (Plan/30 §5.2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TmKey {
    /// Rotation `x, y, z, w`.
    pub rot: Quat,
    /// Translation.
    pub pos: Vec3,
}

impl TmKey {
    /// The seven floats in file order (`qx qy qz qw px py pz`).
    pub fn to_array(&self) -> [f32; 7] {
        [
            self.rot[0],
            self.rot[1],
            self.rot[2],
            self.rot[3],
            self.pos[0],
            self.pos[1],
            self.pos[2],
        ]
    }

    /// Builds a key from seven floats in file order.
    pub fn from_array(a: [f32; 7]) -> TmKey {
        TmKey {
            rot: [a[0], a[1], a[2], a[3]],
            pos: [a[4], a[5], a[6]],
        }
    }

    /// `true` when all seven floats have the same bits as `other`'s (R2: bitwise comparison).
    pub fn bits_eq(&self, other: &TmKey) -> bool {
        self.to_array()
            .iter()
            .zip(other.to_array())
            .all(|(a, b)| a.to_bits() == b.to_bits())
    }
}

/// Per-bone hierarchy record of a MOTIONCORE.
#[derive(Debug, Clone, PartialEq)]
pub struct MotionBoneInfo {
    /// Bone name (CP949 or ASCII bytes, at most 32).
    pub name: RawStr,
    /// Inverse bind matrix of the clip's own bind pose (unused at runtime).
    pub inverse_tm: D3dMatrix,
    /// Local bind matrix.
    pub local_tm: D3dMatrix,
    /// Parent bone index, `-1` for a root.
    pub parent: i32,
}

/// The animation data of one bone track.
#[derive(Debug, Clone, PartialEq)]
pub enum BoneTrack {
    /// `animated == 1`: one key per frame.
    Animated {
        /// `nFrame` keys.
        keys: Vec<TmKey>,
    },
    /// `animated != 1`: one matrix used for every frame (may contain scale). `flag` keeps the raw
    /// `animated` word so writers reproduce the file.
    Static {
        /// Raw `animated` word (anything but 1).
        flag: i32,
        /// The matrix used for all frames.
        local_tm: D3dMatrix,
    },
    /// Parsed with [`KeyMode::Skip`]: the payload was skipped without allocating. `animated` tells which
    /// kind it was.
    Skipped {
        /// `true` when the track had keys.
        animated: bool,
    },
}

/// A parsed MOTIONCORE (Plan/30 §5.4).
#[derive(Debug, Clone, PartialEq)]
pub struct MotionCore {
    /// Per-bone hierarchy records (`nBone`).
    pub bones: Vec<MotionBoneInfo>,
    /// The stored `poolSize` word.
    pub pool_size: i32,
    /// One track per bone, same order as `bones`.
    pub tracks: Vec<BoneTrack>,
}

impl MotionCore {
    /// Number of animated tracks (tracks with keys).
    pub fn animated_count(&self) -> usize {
        self.tracks
            .iter()
            .filter(|t| matches!(t, BoneTrack::Animated { .. }))
            .count()
    }
}

/// Converts `4 * N` little-endian bytes into `N` floats (exact bits). Missing bytes read as 0.
pub(crate) fn f32s<const N: usize>(bytes: &[u8]) -> [f32; N] {
    let mut out = [0.0f32; N];
    let (chunks, _) = bytes.as_chunks::<4>();
    for (dst, c) in out.iter_mut().zip(chunks) {
        *dst = f32::from_bits(u32::from_le_bytes(*c));
    }
    out
}

/// Reads `n` `VEC3` into a vector; the size is checked before the allocation (R1).
pub(crate) fn read_vec3s(r: &mut ByteReader<'_>, n: usize) -> Result<Vec<Vec3>, FormatError> {
    let total = checked_bytes(r, n, 12)?;
    let bytes = r.bytes(total)?;
    let (chunks, _) = bytes.as_chunks::<12>();
    Ok(chunks.iter().map(|c| f32s::<3>(c)).collect())
}

/// Reads `n` `MOTION_ATTR`; the size is checked before the allocation (R1).
pub(crate) fn read_attrs(r: &mut ByteReader<'_>, n: usize) -> Result<Vec<MotionAttr>, FormatError> {
    let total = checked_bytes(r, n, MOTION_ATTR_SIZE)?;
    let bytes = r.bytes(total)?;
    let (chunks, _) = bytes.as_chunks::<MOTION_ATTR_SIZE>();
    Ok(chunks
        .iter()
        .map(|c| {
            let (words, _) = c.as_chunks::<4>();
            let mut it = words.iter().map(|w| u32::from_le_bytes(*w));
            let flags = it.next().unwrap_or(0);
            let sound = it.next().unwrap_or(0);
            let frame = it.next().unwrap_or(0);
            MotionAttr {
                flags,
                sound_id: sound as i32,
                frame: f32::from_bits(frame),
            }
        })
        .collect())
}

/// `n * elem_size` with the overflow reported as an end-of-data error.
pub(crate) fn checked_bytes(
    r: &ByteReader<'_>,
    n: usize,
    elem_size: usize,
) -> Result<usize, FormatError> {
    n.checked_mul(elem_size).ok_or_else(|| {
        r.err(ErrorKind::UnexpectedEof {
            needed: u64::MAX,
            remaining: r.remaining() as u64,
        })
    })
}

/// Reads `frame_count` keys.
pub(crate) fn read_keys(
    r: &mut ByteReader<'_>,
    frame_count: usize,
) -> Result<Vec<TmKey>, FormatError> {
    let total = checked_bytes(r, frame_count, TM_KEY_SIZE)?;
    let bytes = r.bytes(total)?;
    let (chunks, _) = bytes.as_chunks::<TM_KEY_SIZE>();
    Ok(chunks
        .iter()
        .map(|c| TmKey::from_array(f32s::<7>(c)))
        .collect())
}

/// Reads a MOTIONCORE for `bone_count` bones and `frame_count` frames (the caller validated both; the
/// byte budget is re-checked here so a hostile count never allocates more than the input holds).
///
/// With [`KeyMode::Skip`] the key arrays and static matrices are skipped by offset arithmetic and the
/// tracks become [`BoneTrack::Skipped`]. A pool-size mismatch is reported in `warnings`.
pub(crate) fn read_core(
    r: &mut ByteReader<'_>,
    bone_count: usize,
    frame_count: usize,
    mode: KeyMode,
    warnings: &mut Vec<FormatWarning>,
) -> Result<MotionCore, FormatError> {
    let min = checked_bytes(r, bone_count, MIN_CORE_BONE_BYTES)?;
    if min > r.remaining() {
        return Err(r.err(ErrorKind::UnexpectedEof {
            needed: min as u64,
            remaining: r.remaining() as u64,
        }));
    }
    let mut bones = Vec::with_capacity(bone_count);
    for _ in 0..bone_count {
        let name = r.nstr("bone name", MAX_BONE_NAME_LEN)?;
        let inverse_tm = r.mat()?;
        let local_tm = r.mat()?;
        let parent = r.i32()?;
        bones.push(MotionBoneInfo {
            name,
            inverse_tm,
            local_tm,
            parent,
        });
    }

    let pool_at = r.pos();
    let pool_size = r.i32()?;
    let mut tracks = Vec::with_capacity(bone_count);
    let mut animated_frames: u64 = 0;
    for _ in 0..bone_count {
        let flag = r.i32()?;
        let animated = flag == 1;
        if animated {
            animated_frames = animated_frames.saturating_add(frame_count as u64);
        }
        let track = match (mode, animated) {
            (KeyMode::Load, true) => BoneTrack::Animated {
                keys: read_keys(r, frame_count)?,
            },
            (KeyMode::Load, false) => BoneTrack::Static {
                flag,
                local_tm: r.mat()?,
            },
            (KeyMode::Skip, true) => {
                let total = checked_bytes(r, frame_count, TM_KEY_SIZE)?;
                r.skip(total)?;
                BoneTrack::Skipped { animated: true }
            }
            (KeyMode::Skip, false) => {
                r.skip(64)?;
                BoneTrack::Skipped { animated: false }
            }
        };
        tracks.push(track);
    }
    if i64::from(pool_size) != animated_frames as i64 {
        warnings.push(FormatWarning::new(
            pool_at,
            WarningCode::MotionPoolMismatch,
            format!("poolSize {pool_size} != {animated_frames} frames of animated bones"),
        ));
    }
    Ok(MotionCore {
        bones,
        pool_size,
        tracks,
    })
}

/// Validates the bone count of a MOTIONCORE host (`.ani`, `.o3d`) read as a plain `i32`.
pub(crate) fn check_bone_count(
    r: &ByteReader<'_>,
    at: usize,
    value: i32,
) -> Result<usize, FormatError> {
    let value64 = i64::from(value);
    match usize::try_from(value) {
        Ok(n) if n <= MAX_BONES as usize => Ok(n),
        _ => Err(FormatError::new(
            r.kind(),
            at,
            ErrorKind::CountOutOfRange {
                field: "nBone",
                value: value64,
                max: i64::from(MAX_BONES),
            },
        )),
    }
}
