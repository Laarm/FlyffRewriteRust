//! Packed motion `FMO1` v1 (Plan/30 §13.4): the input of `SimWorld.add_clip` (Plan/31 §17.3) and of the
//! server's pose evaluation. Left-handed, little-endian, lossless: unpacking yields the same `f32` bits as
//! the parsed [`AniFile`] (test `pack_motion_bit_exact`). The layout is **frozen** with `FMO1` v1:
//!
//! | off | type | field |
//! |---|---|---|
//! | 0 | `u8[4]` | magic `FMO1` |
//! | 4 | `u32` | pack version (1) |
//! | 8 | `u32` | bone count B |
//! | 12 | `u32` | frame count F (> 0) |
//! | 16 | `f32` | `per_slerp` (raw; `trunc3` is applied by the motion clock) |
//! | 20 | `u32` | animated track count A |
//! | 24 | `u32` | flags: bit 0 embedded, bit 1 has attrs, bit 2 `nodes` (node clip built from `.o3d` GMOBJECTs) |
//! | 28 | `u32` | reserved (0) |
//! | 32 | B x 72 B | per bone: `i32 parent` (MOTIONCORE hierarchy; node clips: object index), `u32 track` (0..A-1, or `0xFFFFFFFF` = static), `f32[16] static_tm` (static bones: the stored matrix; animated: zeros) |
//! | 32 + 72 B | A x F x 28 B | keys per animated track: F x (`f32 qx, qy, qz, qw, px, py, pz`) |
//! | ... | F x 12 B | `MOTION_ATTR` array (only if flags bit 1) |
//!
//! Node clips (flag bit 2, [`build_nodes_from_o3d`]) animate the GMOBJECTs of an `.o3d`: bone `i` = LOD-0
//! object `i`, see the function docs.
//!
//! Validation (build and [`MotionView::parse`], error `Inconsistent`): the exact blob size, `F` in
//! `1..=65535`, every parent `-1` or smaller than the bone's own index (parents precede children, ref/18
//! §4.8), every track index below `A` or `0xFFFFFFFF`, undefined flag bits and a non-zero reserved word.

use crate::FormatKind;
use crate::ani::AniFile;
use crate::error::{ErrorKind, FormatError};
use crate::limits::MAX_ANI_FRAMES;
use crate::motion::{
    BoneTrack, MOTION_ATTR_SIZE, MotionAttr, MotionCore, TM_KEY_SIZE, TmKey, f32s,
};
use crate::o3d::{GmType, O3dFile};
use crate::raw::D3dMatrix;

use super::o3d::{AttrMeta, attr_meta};
use super::{BlobView, ElemType, Packed};

/// The magic of a packed motion.
pub const MAGIC: [u8; 4] = *b"FMO1";
/// The pack version of `FMO1` (frozen; independent of [`crate::version::PACK`]).
pub const VERSION: u32 = 1;
/// Bytes before the first bone.
pub const HEADER_SIZE: usize = 32;
/// Bytes per bone record.
pub const BONE_SIZE: usize = 72;
/// Bytes per key.
pub const KEY_SIZE: usize = TM_KEY_SIZE;
/// `track` value of a static bone.
pub const NO_TRACK: u32 = 0xFFFF_FFFF;
/// Flag bit 0: the clip is the embedded motion of an `.o3d`.
pub const FLAG_EMBEDDED: u32 = 1;
/// Flag bit 1: the MOTION_ATTR array follows the keys.
pub const FLAG_ATTRS: u32 = 2;
/// Flag bit 2: node clip (bone `i` = GMOBJECT `i` of LOD 0).
pub const FLAG_NODES: u32 = 4;
const FLAG_MASK: u32 = FLAG_EMBEDDED | FLAG_ATTRS | FLAG_NODES;
/// Most bones a motion pack may declare (node clips can exceed the 255 of a skeleton).
pub const MAX_BONES: usize = 65_535;

fn err(offset: usize, what: &'static str) -> FormatError {
    FormatError::new(
        FormatKind::PackedMotion,
        offset,
        ErrorKind::Inconsistent { what },
    )
}

/// One bone of an [`OwnedMotion`].
#[derive(Debug, Clone, PartialEq)]
pub struct PackedMotionBone {
    /// Parent bone (or object) index, `-1` for a root.
    pub parent: i32,
    /// Index into [`OwnedMotion::tracks`], `None` for a static bone.
    pub track: Option<u32>,
    /// The stored matrix of a static bone (zeros for an animated one).
    pub static_tm: D3dMatrix,
}

/// An owned, decoded `FMO1` (the result of [`MotionView::to_owned`], also the serialisation input).
#[derive(Debug, Clone, PartialEq)]
pub struct OwnedMotion {
    /// Flag bits (`FLAG_*`).
    pub flags: u32,
    /// Raw `perSlerp`.
    pub per_slerp: f32,
    /// Frame count F.
    pub frame_count: u32,
    /// Bones in file order.
    pub bones: Vec<PackedMotionBone>,
    /// The animated tracks, F keys each.
    pub tracks: Vec<Vec<TmKey>>,
    /// The attribute array (F entries) when `FLAG_ATTRS` is set, else empty.
    pub attrs: Vec<MotionAttr>,
}

/// Borrowed serialisation input (so the builders do not copy the keys twice).
struct Parts<'a> {
    flags: u32,
    per_slerp: f32,
    frames: usize,
    bones: Vec<PackedMotionBone>,
    tracks: Vec<&'a [TmKey]>,
    attrs: &'a [MotionAttr],
}

fn write(p: &Parts<'_>) -> Result<Vec<u8>, FormatError> {
    if p.flags & !FLAG_MASK != 0 {
        return Err(err(24, "motion flags use undefined bits"));
    }
    if p.frames == 0 || p.frames > MAX_ANI_FRAMES as usize {
        return Err(err(12, "motion frame count must be 1..=65535"));
    }
    if p.bones.len() > MAX_BONES {
        return Err(err(8, "motion has too many bones"));
    }
    for (i, b) in p.bones.iter().enumerate() {
        let off = HEADER_SIZE + BONE_SIZE * i;
        if b.parent != -1 && !(0..i as i64).contains(&i64::from(b.parent)) {
            return Err(err(off, "motion bone parent does not precede the bone"));
        }
        if let Some(t) = b.track
            && t as usize >= p.tracks.len()
        {
            return Err(err(off + 4, "motion bone track index out of range"));
        }
    }
    if p.tracks.iter().any(|t| t.len() != p.frames) {
        return Err(err(
            HEADER_SIZE,
            "motion track does not hold one key per frame",
        ));
    }
    let has_attrs = p.flags & FLAG_ATTRS != 0;
    if has_attrs && p.attrs.len() != p.frames {
        return Err(err(
            HEADER_SIZE,
            "motion attribute array does not hold one entry per frame",
        ));
    }
    let size = HEADER_SIZE
        + BONE_SIZE * p.bones.len()
        + KEY_SIZE * p.frames * p.tracks.len()
        + if has_attrs {
            MOTION_ATTR_SIZE * p.frames
        } else {
            0
        };
    let mut out = Vec::with_capacity(size);
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&(p.bones.len() as u32).to_le_bytes());
    out.extend_from_slice(&(p.frames as u32).to_le_bytes());
    out.extend_from_slice(&p.per_slerp.to_bits().to_le_bytes());
    out.extend_from_slice(&(p.tracks.len() as u32).to_le_bytes());
    out.extend_from_slice(&p.flags.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    for b in &p.bones {
        out.extend_from_slice(&b.parent.to_le_bytes());
        out.extend_from_slice(&b.track.unwrap_or(NO_TRACK).to_le_bytes());
        for v in b.static_tm.0 {
            out.extend_from_slice(&v.to_bits().to_le_bytes());
        }
    }
    for t in &p.tracks {
        for k in *t {
            for v in k.to_array() {
                out.extend_from_slice(&v.to_bits().to_le_bytes());
            }
        }
    }
    if has_attrs {
        for a in p.attrs {
            out.extend_from_slice(&a.to_le_bytes());
        }
    }
    Ok(out)
}

const ZERO_MAT: D3dMatrix = D3dMatrix([0.0; 16]);

/// Bones and borrowed tracks of a MOTIONCORE. Errors when the keys were skipped.
fn core_parts(core: &MotionCore) -> Result<(Vec<PackedMotionBone>, Vec<&[TmKey]>), FormatError> {
    let mut bones = Vec::with_capacity(core.bones.len());
    let mut tracks: Vec<&[TmKey]> = Vec::new();
    for (info, track) in core.bones.iter().zip(&core.tracks) {
        let bone = match track {
            BoneTrack::Animated { keys } => {
                tracks.push(keys);
                PackedMotionBone {
                    parent: info.parent,
                    track: Some((tracks.len() - 1) as u32),
                    static_tm: ZERO_MAT,
                }
            }
            BoneTrack::Static { local_tm, .. } => PackedMotionBone {
                parent: info.parent,
                track: None,
                static_tm: *local_tm,
            },
            BoneTrack::Skipped { .. } => {
                return Err(err(
                    0,
                    "motion keys were skipped (parse with KeyMode::Load)",
                ));
            }
        };
        bones.push(bone);
    }
    if core.tracks.len() != core.bones.len() {
        return Err(err(
            0,
            "motion core has a different number of tracks and bones",
        ));
    }
    Ok((bones, tracks))
}

/// Builds the `FMO1` of a `.ani` (flags: bit 1 attrs; `per_slerp` raw). Requires `KeyMode::Load` data.
pub fn build_from_ani(ani: &AniFile) -> Result<Vec<u8>, FormatError> {
    let (bones, tracks) = core_parts(&ani.core)?;
    write(&Parts {
        flags: FLAG_ATTRS,
        per_slerp: ani.per_slerp,
        frames: usize::try_from(ani.frame_count).unwrap_or(0),
        bones,
        tracks,
        attrs: &ani.attrs,
    })
}

/// Builds the `FMO1` of the embedded MOTIONCORE of an `.o3d` (flags: bit 0 embedded, bit 1 when the file
/// stores attributes); `Ok(None)` when the file has no embedded skeleton or no embedded motion.
pub fn build_from_o3d(o3d: &O3dFile) -> Result<Option<Vec<u8>>, FormatError> {
    let Some(core) = o3d.bones.as_ref().and_then(|b| b.motion.as_ref()) else {
        return Ok(None);
    };
    let (bones, tracks) = core_parts(core)?;
    let (attr_flag, attrs): (u32, &[MotionAttr]) = match &o3d.attrs {
        crate::o3d::AttrBlock::Attrs(a) => (FLAG_ATTRS, a),
        _ => (0, &[]),
    };
    write(&Parts {
        flags: FLAG_EMBEDDED | attr_flag,
        per_slerp: o3d.per_slerp,
        frames: usize::try_from(o3d.max_frame).unwrap_or(0),
        bones,
        tracks,
        attrs,
    })
    .map(Some)
}

/// Builds the node clip of an `.o3d` (flag bit 2, `31 Q23(b)-(c)`); `Ok(None)` for a file without node
/// animation (`max_frame == 0`, or no LOD-0 object with stored node keys).
///
/// Bone `i` = LOD-0 GMOBJECT `i` (B = the LOD-0 object count; LOD groups above 0 are not included):
///
/// * `parent` = the parent object index (`parentType` 0) or `-1`;
/// * `track` = the object's node keys (F = `max_frame`, the parsed bits, left-handed) when present, else
///   static with `static_tm = local_tm`;
/// * an object parented to a **bone** (`parentType` 2) is frozen: static, `parent = -1`,
///   `static_tm = local_tm * baseBone[parentIdx]` (the bind pose of the embedded skeleton; its node keys, if
///   any, are ignored); without an embedded skeleton (a bone of an external `.chr`) `static_tm = local_tm`
///   and the renderer composes the bone matrix itself (Plan/30 §18 Q23);
/// * `per_slerp` = the header value; bit 1 (attrs) when the `.o3d` stores attributes; bit 0 clear.
pub fn build_nodes_from_o3d(o3d: &O3dFile) -> Result<Option<Vec<u8>>, FormatError> {
    if !o3d.has_node_animation() {
        return Ok(None);
    }
    let Some(group) = o3d.lod0() else {
        return Ok(None);
    };
    let frames = usize::try_from(o3d.max_frame).unwrap_or(0);
    let mut bones = Vec::with_capacity(group.objects.len());
    let mut tracks: Vec<&[TmKey]> = Vec::new();
    for obj in &group.objects {
        let keys = obj
            .node_anim
            .as_ref()
            .filter(|n| n.has_frames != 0 && matches!(obj.kind(), GmType::Normal))
            .map(|n| n.keys.as_slice());
        let bone_parented = obj.parent_idx != -1 && obj.parent_type == Some(2);
        let bone = if bone_parented {
            let base = o3d
                .bones
                .as_ref()
                .map(|b| {
                    usize::try_from(obj.parent_idx)
                        .ok()
                        .and_then(|p| b.base.get(p))
                        .ok_or_else(|| err(0, "o3d object parent bone is out of range"))
                })
                .transpose()?;
            PackedMotionBone {
                parent: -1,
                track: None,
                static_tm: match base {
                    Some(m) => obj.local_tm.mul(m),
                    None => obj.local_tm,
                },
            }
        } else {
            let parent = obj.parent_idx;
            match keys {
                Some(k) => {
                    tracks.push(k);
                    PackedMotionBone {
                        parent,
                        track: Some((tracks.len() - 1) as u32),
                        static_tm: ZERO_MAT,
                    }
                }
                None => PackedMotionBone {
                    parent,
                    track: None,
                    static_tm: obj.local_tm,
                },
            }
        };
        bones.push(bone);
    }
    let (attr_flag, attrs): (u32, &[MotionAttr]) = match &o3d.attrs {
        crate::o3d::AttrBlock::Attrs(a) => (FLAG_ATTRS, a),
        _ => (0, &[]),
    };
    write(&Parts {
        flags: FLAG_NODES | attr_flag,
        per_slerp: o3d.per_slerp,
        frames,
        bones,
        tracks,
        attrs,
    })
    .map(Some)
}

/// A validated, zero-copy view of an `FMO1` blob.
#[derive(Debug, Clone, Copy)]
pub struct MotionView<'a> {
    buf: &'a [u8],
    bones: usize,
    frames: usize,
    tracks: usize,
    flags: u32,
    per_slerp: f32,
}

impl<'a> MotionView<'a> {
    /// Validates `buf` (magic, version, exact size, flags, parents, track indices) and returns the view.
    /// Errors are labelled [`FormatKind::PackedMotion`].
    pub fn parse(buf: &'a [u8]) -> Result<MotionView<'a>, FormatError> {
        let e = |offset: usize, kind: ErrorKind| {
            FormatError::new(FormatKind::PackedMotion, offset, kind)
        };
        if buf.len() < HEADER_SIZE {
            return Err(e(
                0,
                ErrorKind::UnexpectedEof {
                    needed: HEADER_SIZE as u64,
                    remaining: buf.len() as u64,
                },
            ));
        }
        let word = |off: usize| -> u32 {
            buf.get(off..off + 4)
                .and_then(|s| <[u8; 4]>::try_from(s).ok())
                .map_or(0, u32::from_le_bytes)
        };
        let magic = <[u8; 4]>::try_from(buf.get(..4).unwrap_or(&[])).unwrap_or([0; 4]);
        if magic != MAGIC {
            return Err(e(
                0,
                ErrorKind::BadMagic {
                    expected: "FMO1",
                    found: magic,
                },
            ));
        }
        if word(4) != VERSION {
            return Err(e(
                4,
                ErrorKind::UnsupportedVersion {
                    found: i64::from(word(4)),
                },
            ));
        }
        let (bones, frames, tracks, flags) = (
            word(8) as usize,
            word(12) as usize,
            word(20) as usize,
            word(24),
        );
        if bones > MAX_BONES {
            return Err(e(
                8,
                ErrorKind::CountOutOfRange {
                    field: "bone count",
                    value: i64::from(word(8)),
                    max: MAX_BONES as i64,
                },
            ));
        }
        if frames == 0 || frames > MAX_ANI_FRAMES as usize {
            return Err(e(
                12,
                ErrorKind::CountOutOfRange {
                    field: "frame count",
                    value: i64::from(word(12)),
                    max: i64::from(MAX_ANI_FRAMES),
                },
            ));
        }
        if tracks > bones {
            return Err(e(
                20,
                ErrorKind::Inconsistent {
                    what: "motion has more tracks than bones",
                },
            ));
        }
        if flags & !FLAG_MASK != 0 {
            return Err(e(
                24,
                ErrorKind::InvalidValue {
                    field: "flags",
                    value: i64::from(flags),
                },
            ));
        }
        if word(28) != 0 {
            return Err(e(
                28,
                ErrorKind::InvalidValue {
                    field: "reserved",
                    value: i64::from(word(28)),
                },
            ));
        }
        // All factors are bounded (65535 x 65535 x 28 fits u64), so plain u64 arithmetic cannot overflow.
        let has_attrs = flags & FLAG_ATTRS != 0;
        let expected = HEADER_SIZE as u64
            + BONE_SIZE as u64 * bones as u64
            + KEY_SIZE as u64 * frames as u64 * tracks as u64
            + if has_attrs {
                MOTION_ATTR_SIZE as u64 * frames as u64
            } else {
                0
            };
        let len = buf.len() as u64;
        if len < expected {
            return Err(e(
                buf.len(),
                ErrorKind::UnexpectedEof {
                    needed: expected,
                    remaining: len,
                },
            ));
        }
        if len > expected {
            return Err(e(
                expected as usize,
                ErrorKind::Inconsistent {
                    what: "motion blob has trailing bytes",
                },
            ));
        }
        let view = MotionView {
            buf,
            bones,
            frames,
            tracks,
            flags,
            per_slerp: f32::from_bits(word(16)),
        };
        for i in 0..bones {
            let off = HEADER_SIZE + BONE_SIZE * i;
            let parent = word(off) as i32;
            if parent != -1 && !(0..i as i64).contains(&i64::from(parent)) {
                return Err(err(off, "motion bone parent does not precede the bone"));
            }
            let track = word(off + 4);
            if track != NO_TRACK && track as usize >= tracks {
                return Err(err(off + 4, "motion bone track index out of range"));
            }
        }
        Ok(view)
    }

    /// The validated bytes.
    pub fn bytes(&self) -> &'a [u8] {
        self.buf
    }

    /// Bone count B.
    pub fn bone_count(&self) -> usize {
        self.bones
    }

    /// Frame count F.
    pub fn frame_count(&self) -> usize {
        self.frames
    }

    /// Animated track count A.
    pub fn track_count(&self) -> usize {
        self.tracks
    }

    /// The flag bits (`FLAG_*`).
    pub fn flags(&self) -> u32 {
        self.flags
    }

    /// Raw `perSlerp`.
    pub fn per_slerp(&self) -> f32 {
        self.per_slerp
    }

    /// `true` when the clip is the embedded motion of an `.o3d`.
    pub fn is_embedded(&self) -> bool {
        self.flags & FLAG_EMBEDDED != 0
    }

    /// `true` when the attribute array is present.
    pub fn has_attrs(&self) -> bool {
        self.flags & FLAG_ATTRS != 0
    }

    /// `true` for a node clip (bone `i` = GMOBJECT `i`).
    pub fn is_node_clip(&self) -> bool {
        self.flags & FLAG_NODES != 0
    }

    fn word(&self, off: usize) -> u32 {
        self.buf
            .get(off..off + 4)
            .and_then(|s| <[u8; 4]>::try_from(s).ok())
            .map_or(0, u32::from_le_bytes)
    }

    /// Parent of bone `i` (`-1` for a root).
    pub fn parent(&self, i: usize) -> Option<i32> {
        (i < self.bones).then(|| self.word(HEADER_SIZE + BONE_SIZE * i) as i32)
    }

    /// Track index of bone `i`: `Some(t)` for an animated bone, `None` for a static one (also out of range).
    pub fn track(&self, i: usize) -> Option<u32> {
        if i >= self.bones {
            return None;
        }
        match self.word(HEADER_SIZE + BONE_SIZE * i + 4) {
            NO_TRACK => None,
            t => Some(t),
        }
    }

    /// The stored matrix of bone `i` (zeros for an animated bone).
    pub fn static_tm(&self, i: usize) -> Option<D3dMatrix> {
        (i < self.bones).then(|| {
            let off = HEADER_SIZE + BONE_SIZE * i + 8;
            D3dMatrix(f32s::<16>(self.buf.get(off..off + 64).unwrap_or(&[])))
        })
    }

    fn keys_start(&self) -> usize {
        HEADER_SIZE + BONE_SIZE * self.bones
    }

    /// The raw bytes of one track (`F * 28`), `None` when `track` is out of range.
    pub fn track_bytes(&self, track: usize) -> Option<&'a [u8]> {
        if track >= self.tracks {
            return None;
        }
        let len = KEY_SIZE * self.frames;
        let start = self.keys_start() + len * track;
        self.buf.get(start..start + len)
    }

    /// Key `frame` of `track`.
    pub fn key(&self, track: usize, frame: usize) -> Option<TmKey> {
        if frame >= self.frames {
            return None;
        }
        let bytes = self.track_bytes(track)?;
        let start = KEY_SIZE * frame;
        bytes
            .get(start..start + KEY_SIZE)
            .map(|b| TmKey::from_array(f32s::<7>(b)))
    }

    /// All keys of `track` in frame order.
    pub fn keys(&self, track: usize) -> impl Iterator<Item = TmKey> + 'a {
        let (chunks, _) = self
            .track_bytes(track)
            .unwrap_or(&[])
            .as_chunks::<KEY_SIZE>();
        chunks.iter().map(|c| TmKey::from_array(f32s::<7>(c)))
    }

    /// Attribute entry `i` (`None` without the attrs flag or out of range).
    pub fn attr(&self, i: usize) -> Option<MotionAttr> {
        if !self.has_attrs() || i >= self.frames {
            return None;
        }
        let start = self.keys_start() + KEY_SIZE * self.frames * self.tracks + MOTION_ATTR_SIZE * i;
        let b = self.buf.get(start..start + MOTION_ATTR_SIZE)?;
        let (words, _) = b.as_chunks::<4>();
        let mut it = words.iter().map(|w| u32::from_le_bytes(*w));
        Some(MotionAttr {
            flags: it.next()?,
            sound_id: it.next()? as i32,
            frame: f32::from_bits(it.next()?),
        })
    }

    /// The attribute array (empty without the attrs flag).
    pub fn attrs(&self) -> Vec<MotionAttr> {
        (0..self.frames).map_while(|i| self.attr(i)).collect()
    }

    /// Copies the view into owned structures.
    pub fn to_owned(&self) -> OwnedMotion {
        OwnedMotion {
            flags: self.flags,
            per_slerp: self.per_slerp,
            frame_count: self.frames as u32,
            bones: (0..self.bones)
                .map(|i| PackedMotionBone {
                    parent: self.parent(i).unwrap_or(-1),
                    track: self.track(i),
                    static_tm: self.static_tm(i).unwrap_or_default(),
                })
                .collect(),
            tracks: (0..self.tracks).map(|t| self.keys(t).collect()).collect(),
            attrs: self.attrs(),
        }
    }
}

impl OwnedMotion {
    /// Validates and serialises (the inverse of [`MotionView::to_owned`]).
    pub fn to_bytes(&self) -> Result<Vec<u8>, FormatError> {
        write(&Parts {
            flags: self.flags,
            per_slerp: self.per_slerp,
            frames: self.frame_count as usize,
            bones: self.bones.clone(),
            tracks: self.tracks.iter().map(Vec::as_slice).collect(),
            attrs: &self.attrs,
        })
    }
}

// ---------------------------------------------------------------------------------------------------------
// `decode_ani` meta (Plan/30 §14.1)
// ---------------------------------------------------------------------------------------------------------

/// The structured description of a `.ani` pack (Plan/30 §14.1 `AniMeta`). The whole blob is one `FMO1`
/// (`motion = { off: 0, len: blob.byteLength }`), passed unchanged to `SimWorld.add_clip`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct AniMeta {
    /// Number of frames (`1..=65535`).
    pub frames: u32,
    /// Raw per-tick frame advance (the motion clock applies `trunc3`).
    pub per_slerp: f32,
    /// Bones of the clip (the skeleton's bone count).
    pub bone_count: u32,
    /// Bones that carry keys.
    pub animated_count: u32,
    /// Frame attributes (hit, sound, quake) of the frames that have any, in frame order.
    pub attrs: Vec<AttrMeta>,
    /// `true` when the file stores a root path (1 corpus file; never used).
    pub has_path: bool,
    /// The `FMO1` section: always the whole blob.
    pub motion: BlobView,
}

/// Builds the `decode_ani` result: `meta` (frames, attributes for hit-frame markers) plus the `FMO1` blob of
/// [`build_from_ani`] (left-handed, byte for byte the `add_clip` input). Requires keys parsed with
/// `KeyMode::Load`.
///
/// Errors: the validation errors of [`build_from_ani`] (`Inconsistent`).
pub fn pack_ani(ani: &AniFile) -> Result<Packed<AniMeta>, FormatError> {
    let blob = build_from_ani(ani)?;
    let len = u32::try_from(blob.len()).map_err(|_| err(0, "motion pack exceeds 4 GiB"))?;
    let meta = AniMeta {
        frames: u32::try_from(ani.frame_count).unwrap_or(0),
        per_slerp: ani.per_slerp,
        bone_count: u32::try_from(ani.bone_count).unwrap_or(0),
        animated_count: u32::try_from(ani.core.animated_count()).unwrap_or(u32::MAX),
        attrs: ani
            .attrs
            .iter()
            .enumerate()
            .filter(|(_, a)| a.flags != 0)
            .map(|(i, a)| attr_meta(i, a))
            .collect(),
        has_path: ani.has_path != 0,
        motion: BlobView {
            off: 0,
            len,
            ty: ElemType::U8,
            comps: 1,
            count: len,
        },
    };
    Ok(Packed { meta, blob })
}
