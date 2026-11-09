//! Packed skeleton `FSK1` v1 (Plan/30 §13.3): the input of `SimWorld.add_skeleton` (Plan/31 §17.3).
//!
//! Left-handed, little-endian, 4-byte fields; the layout is **frozen** with `FSK1` v1 (31 and 41 read it):
//!
//! | off | type | field |
//! |---|---|---|
//! | 0 | `u8[4]` | magic `FSK1` |
//! | 4 | `u32` | pack version (1) |
//! | 8 | `u32` | bone count N (1..=255) |
//! | 12 | `i32 x4` | `r_hand`, `l_hand`, `l_arm`, `r_arm` (`ChrFile::special_bones`, default 0) |
//! | 28 | `u32` | event count E (0, 4 or 8) |
//! | 32 | `u32` | flags: bit 0 `send_vs`, bit 1 has `local_lh`, bit 2 embedded (from an `.o3d`) |
//! | 36 | `u32` | source version (`.chr` ver or `.o3d` ver) |
//! | 40 | `f32[16] x4` | `local_rh`, `local_lh` (identity when absent), `local_shield`, `local_knuckle` |
//! | 296 | N x 196 B | per bone: `i32 parent`, `f32[16] tm`, `f32[16] inverse_tm`, `f32[16] local_tm` |
//! | 296 + 196 N | E x 16 B | per event: `f32[3] pos`, `i32 parent` |
//!
//! The 16 floats of bone `b`'s `tm` start at byte [`bone_tm_offset`]`(b) = 296 + 196 b + 4` (4-byte
//! aligned), which TypeScript may read through a read-only `Float32Array` (41 §12.4).
//!
//! Validation (build and [`SkeletonView::parse`], error `Inconsistent`): 1..=255 bones, every parent `-1` or
//! smaller than its own index, exactly one root (several roots are allowed for embedded `.o3d` skeletons
//! without a motion, whose hierarchy is unknown), event parents `-1` or `< N`. The special bone indices are
//! passed through unchecked: Plan/31 range-checks them.

use crate::FormatKind;
use crate::chr::{ChrEvent, ChrFile, SpecialBones};
use crate::error::{ErrorKind, FormatError};
use crate::limits::MAX_BONES;
use crate::motion::f32s;
use crate::o3d::O3dFile;
use crate::raw::D3dMatrix;
use crate::space::{mirror_mat, mirror_pos};

use super::{BlobView, ElemType, Packed, Space};

/// The magic of a packed skeleton.
pub const MAGIC: [u8; 4] = *b"FSK1";
/// The pack version of `FSK1` (frozen; independent of [`crate::version::PACK`]).
pub const VERSION: u32 = 1;
/// Bytes before the first bone.
pub const HEADER_SIZE: usize = 296;
/// Bytes per bone.
pub const BONE_SIZE: usize = 196;
/// Bytes per event.
pub const EVENT_SIZE: usize = 16;
/// Flag bit 0: `bSendVS`.
pub const FLAG_SEND_VS: u32 = 1;
/// Flag bit 1: the left-hand grip is real (not the identity filler).
pub const FLAG_LOCAL_LH: u32 = 2;
/// Flag bit 2: embedded skeleton of an `.o3d`.
pub const FLAG_EMBEDDED: u32 = 4;
/// All defined flag bits.
const FLAG_MASK: u32 = FLAG_SEND_VS | FLAG_LOCAL_LH | FLAG_EMBEDDED;

/// Byte offset of bone `b`'s `tm` (16 floats) inside the pack (Plan/30 §13.4 rule 2).
pub const fn bone_tm_offset(b: usize) -> usize {
    HEADER_SIZE + BONE_SIZE * b + 4
}

fn err(offset: usize, what: &'static str) -> FormatError {
    FormatError::new(
        FormatKind::PackedSkeleton,
        offset,
        ErrorKind::Inconsistent { what },
    )
}

/// One bone of an [`OwnedSkeleton`].
#[derive(Debug, Clone, PartialEq)]
pub struct PackedBone {
    /// Parent index, `-1` for a root.
    pub parent: i32,
    /// Bind-pose model-space matrix.
    pub tm: D3dMatrix,
    /// Inverse bind matrix.
    pub inverse_tm: D3dMatrix,
    /// Local bind matrix.
    pub local_tm: D3dMatrix,
}

/// An owned, decoded `FSK1` (the result of [`SkeletonView::to_owned`], also the build intermediate).
#[derive(Debug, Clone, PartialEq)]
pub struct OwnedSkeleton {
    /// Source `.chr` / `.o3d` version.
    pub source_version: u32,
    /// Flag bits (`FLAG_*`).
    pub flags: u32,
    /// Special bone indices.
    pub special: SpecialBones,
    /// Right-hand grip.
    pub local_rh: D3dMatrix,
    /// Left-hand grip (identity when `FLAG_LOCAL_LH` is clear).
    pub local_lh: D3dMatrix,
    /// Shield grip.
    pub local_shield: D3dMatrix,
    /// Knuckle grip.
    pub local_knuckle: D3dMatrix,
    /// Bones in file order.
    pub bones: Vec<PackedBone>,
    /// Event slots (0, 4 or 8).
    pub events: Vec<ChrEvent>,
}

/// Validates the hierarchy shared by the builders and the view.
fn check_hierarchy(
    n: usize,
    parent_of: impl Fn(usize) -> i32,
    event_count: usize,
    event_parent_of: impl Fn(usize) -> i32,
    embedded: bool,
    offset_of_bone: impl Fn(usize) -> usize,
) -> Result<(), FormatError> {
    if n == 0 {
        return Err(err(0, "skeleton has no bone"));
    }
    if n > MAX_BONES as usize {
        return Err(err(0, "skeleton has more than 255 bones"));
    }
    let mut roots = 0usize;
    for i in 0..n {
        let p = parent_of(i);
        if p == -1 {
            roots += 1;
        } else if !(0..i64::try_from(i).unwrap_or(0)).contains(&i64::from(p)) {
            return Err(err(
                offset_of_bone(i),
                "skeleton bone parent does not precede the bone",
            ));
        }
    }
    if roots != 1 && !(embedded && roots >= 1) {
        return Err(err(0, "skeleton needs exactly one root bone"));
    }
    if !matches!(event_count, 0 | 4 | 8) {
        return Err(err(0, "skeleton event count must be 0, 4 or 8"));
    }
    for e in 0..event_count {
        let p = event_parent_of(e);
        if p != -1 && !(0..n as i64).contains(&i64::from(p)) {
            return Err(err(
                HEADER_SIZE + BONE_SIZE * n + EVENT_SIZE * e + 12,
                "skeleton event parent out of range",
            ));
        }
    }
    Ok(())
}

impl OwnedSkeleton {
    /// From a parsed `.chr` (Plan/30 §13.3): `tm`, `inverse_tm`, `local_tm`, parents and grips are copied
    /// bit for bit; the special bones come from [`ChrFile::special_bones`].
    pub fn from_chr(chr: &ChrFile) -> OwnedSkeleton {
        let mut flags = 0;
        if chr.send_vs != 0 {
            flags |= FLAG_SEND_VS;
        }
        if chr.local_lh.is_some() {
            flags |= FLAG_LOCAL_LH;
        }
        OwnedSkeleton {
            source_version: chr.version as u32,
            flags,
            special: chr.special_bones(),
            local_rh: chr.local_rh,
            local_lh: chr.local_lh.unwrap_or(D3dMatrix::IDENTITY),
            local_shield: chr.local_shield,
            local_knuckle: chr.local_knuckle,
            bones: chr
                .bones
                .iter()
                .map(|b| PackedBone {
                    parent: b.parent,
                    tm: b.tm,
                    inverse_tm: b.inverse_tm,
                    local_tm: b.local_tm,
                })
                .collect(),
            events: chr.events.clone(),
        }
    }

    /// From the embedded skeleton of an `.o3d`: `tm = baseBone`, `inverse_tm = baseBoneInv`,
    /// `parent`/`local_tm` from the MOTIONCORE bone info when present (else `-1` / `baseBone`), no events,
    /// identity grips. `None` when the file has no embedded skeleton.
    pub fn from_o3d(o3d: &O3dFile) -> Option<OwnedSkeleton> {
        let b = o3d.bones.as_ref()?;
        let bones = b
            .base
            .iter()
            .zip(&b.base_inv)
            .enumerate()
            .map(|(i, (tm, inv))| {
                let info = b.motion.as_ref().and_then(|m| m.bones.get(i));
                PackedBone {
                    parent: info.map_or(-1, |bi| bi.parent),
                    tm: *tm,
                    inverse_tm: *inv,
                    local_tm: info.map_or(*tm, |bi| bi.local_tm),
                }
            })
            .collect();
        Some(OwnedSkeleton {
            source_version: o3d.version as u32,
            flags: FLAG_EMBEDDED | if b.send_vs != 0 { FLAG_SEND_VS } else { 0 },
            special: SpecialBones::default(),
            local_rh: D3dMatrix::IDENTITY,
            local_lh: D3dMatrix::IDENTITY,
            local_shield: D3dMatrix::IDENTITY,
            local_knuckle: D3dMatrix::IDENTITY,
            bones,
            events: Vec::new(),
        })
    }

    /// Validates and serialises (see the module docs for the layout).
    pub fn to_bytes(&self) -> Result<Vec<u8>, FormatError> {
        if self.flags & !FLAG_MASK != 0 {
            return Err(err(32, "skeleton flags use undefined bits"));
        }
        let n = self.bones.len();
        check_hierarchy(
            n,
            |i| self.bones.get(i).map_or(-1, |b| b.parent),
            self.events.len(),
            |e| self.events.get(e).map_or(-1, |ev| ev.parent),
            self.flags & FLAG_EMBEDDED != 0,
            |i| HEADER_SIZE + BONE_SIZE * i,
        )?;
        let mut out =
            Vec::with_capacity(HEADER_SIZE + BONE_SIZE * n + EVENT_SIZE * self.events.len());
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&(n as u32).to_le_bytes());
        for v in [
            self.special.r_hand,
            self.special.l_hand,
            self.special.l_arm,
            self.special.r_arm,
        ] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&(self.events.len() as u32).to_le_bytes());
        out.extend_from_slice(&self.flags.to_le_bytes());
        out.extend_from_slice(&self.source_version.to_le_bytes());
        for m in [
            &self.local_rh,
            &self.local_lh,
            &self.local_shield,
            &self.local_knuckle,
        ] {
            push_mat(&mut out, m);
        }
        for b in &self.bones {
            out.extend_from_slice(&b.parent.to_le_bytes());
            push_mat(&mut out, &b.tm);
            push_mat(&mut out, &b.inverse_tm);
            push_mat(&mut out, &b.local_tm);
        }
        for e in &self.events {
            for c in e.pos {
                out.extend_from_slice(&c.to_bits().to_le_bytes());
            }
            out.extend_from_slice(&e.parent.to_le_bytes());
        }
        Ok(out)
    }
}

fn push_mat(out: &mut Vec<u8>, m: &D3dMatrix) {
    for v in m.0 {
        out.extend_from_slice(&v.to_bits().to_le_bytes());
    }
}

/// Builds the `FSK1` of a `.chr` (Plan/30 §13.3). Errors with `Inconsistent` when the skeleton has no bone,
/// more than one root, a parent that does not precede its bone, or an event parent out of range.
pub fn build_from_chr(chr: &ChrFile) -> Result<Vec<u8>, FormatError> {
    OwnedSkeleton::from_chr(chr).to_bytes()
}

/// Builds the `FSK1` of the embedded skeleton of an `.o3d`; `Ok(None)` when the file has none.
pub fn build_from_o3d(o3d: &O3dFile) -> Result<Option<Vec<u8>>, FormatError> {
    OwnedSkeleton::from_o3d(o3d)
        .map(|s| s.to_bytes())
        .transpose()
}

/// A validated, zero-copy view of an `FSK1` blob with accessor methods.
#[derive(Debug, Clone, Copy)]
pub struct SkeletonView<'a> {
    buf: &'a [u8],
    bones: usize,
    events: usize,
    flags: u32,
    source_version: u32,
}

impl<'a> SkeletonView<'a> {
    /// Validates `buf` (magic, version, exact size, flags, hierarchy) and returns the view. Errors are
    /// labelled [`FormatKind::PackedSkeleton`].
    pub fn parse(buf: &'a [u8]) -> Result<SkeletonView<'a>, FormatError> {
        let e = |offset: usize, kind: ErrorKind| {
            FormatError::new(FormatKind::PackedSkeleton, offset, kind)
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
        let magic = <[u8; 4]>::try_from(buf.get(..4).unwrap_or(&[])).unwrap_or([0; 4]);
        if magic != MAGIC {
            return Err(e(
                0,
                ErrorKind::BadMagic {
                    expected: "FSK1",
                    found: magic,
                },
            ));
        }
        let word = |off: usize| -> u32 {
            buf.get(off..off + 4)
                .and_then(|s| <[u8; 4]>::try_from(s).ok())
                .map_or(0, u32::from_le_bytes)
        };
        let version = word(4);
        if version != VERSION {
            return Err(e(
                4,
                ErrorKind::UnsupportedVersion {
                    found: i64::from(version),
                },
            ));
        }
        let n = word(8) as usize;
        if n == 0 || n > MAX_BONES as usize {
            return Err(e(
                8,
                ErrorKind::CountOutOfRange {
                    field: "bone count",
                    value: i64::from(word(8)),
                    max: i64::from(MAX_BONES),
                },
            ));
        }
        let events = word(28) as usize;
        if !matches!(events, 0 | 4 | 8) {
            return Err(e(
                28,
                ErrorKind::InvalidValue {
                    field: "event count",
                    value: i64::from(word(28)),
                },
            ));
        }
        let flags = word(32);
        if flags & !FLAG_MASK != 0 {
            return Err(e(
                32,
                ErrorKind::InvalidValue {
                    field: "flags",
                    value: i64::from(flags),
                },
            ));
        }
        let expected = HEADER_SIZE + BONE_SIZE * n + EVENT_SIZE * events;
        if buf.len() < expected {
            return Err(e(
                buf.len(),
                ErrorKind::UnexpectedEof {
                    needed: expected as u64,
                    remaining: buf.len() as u64,
                },
            ));
        }
        if buf.len() > expected {
            return Err(e(
                expected,
                ErrorKind::Inconsistent {
                    what: "skeleton blob has trailing bytes",
                },
            ));
        }
        let view = SkeletonView {
            buf,
            bones: n,
            events,
            flags,
            source_version: word(36),
        };
        check_hierarchy(
            n,
            |i| view.parent(i).unwrap_or(-1),
            events,
            |k| view.event(k).map_or(-1, |ev| ev.parent),
            flags & FLAG_EMBEDDED != 0,
            |i| HEADER_SIZE + BONE_SIZE * i,
        )?;
        Ok(view)
    }

    /// The validated bytes.
    pub fn bytes(&self) -> &'a [u8] {
        self.buf
    }

    /// Bone count N.
    pub fn bone_count(&self) -> usize {
        self.bones
    }

    /// Event count E (0, 4 or 8).
    pub fn event_count(&self) -> usize {
        self.events
    }

    /// The flag bits (`FLAG_*`).
    pub fn flags(&self) -> u32 {
        self.flags
    }

    /// `bSendVS`.
    pub fn send_vs(&self) -> bool {
        self.flags & FLAG_SEND_VS != 0
    }

    /// `true` when the left-hand grip is real.
    pub fn has_local_lh(&self) -> bool {
        self.flags & FLAG_LOCAL_LH != 0
    }

    /// `true` for the embedded skeleton of an `.o3d`.
    pub fn is_embedded(&self) -> bool {
        self.flags & FLAG_EMBEDDED != 0
    }

    /// The `.chr` / `.o3d` version the pack was built from.
    pub fn source_version(&self) -> u32 {
        self.source_version
    }

    fn word(&self, off: usize) -> u32 {
        self.buf
            .get(off..off + 4)
            .and_then(|s| <[u8; 4]>::try_from(s).ok())
            .map_or(0, u32::from_le_bytes)
    }

    fn mat(&self, off: usize) -> D3dMatrix {
        D3dMatrix(f32s::<16>(self.buf.get(off..off + 64).unwrap_or(&[])))
    }

    /// The special bone indices as stored (not range-checked).
    pub fn special_bones(&self) -> SpecialBones {
        SpecialBones {
            r_hand: self.word(12) as i32,
            l_hand: self.word(16) as i32,
            l_arm: self.word(20) as i32,
            r_arm: self.word(24) as i32,
        }
    }

    /// Right-hand grip.
    pub fn local_rh(&self) -> D3dMatrix {
        self.mat(40)
    }

    /// Left-hand grip (identity when [`Self::has_local_lh`] is false).
    pub fn local_lh(&self) -> D3dMatrix {
        self.mat(104)
    }

    /// Shield grip.
    pub fn local_shield(&self) -> D3dMatrix {
        self.mat(168)
    }

    /// Knuckle grip.
    pub fn local_knuckle(&self) -> D3dMatrix {
        self.mat(232)
    }

    /// Parent of bone `i` (`-1` for a root).
    pub fn parent(&self, i: usize) -> Option<i32> {
        (i < self.bones).then(|| self.word(HEADER_SIZE + BONE_SIZE * i) as i32)
    }

    /// Bind-pose `tm` of bone `i`.
    pub fn tm(&self, i: usize) -> Option<D3dMatrix> {
        (i < self.bones).then(|| self.mat(bone_tm_offset(i)))
    }

    /// Inverse bind matrix of bone `i`.
    pub fn inverse_tm(&self, i: usize) -> Option<D3dMatrix> {
        (i < self.bones).then(|| self.mat(bone_tm_offset(i) + 64))
    }

    /// Local bind matrix of bone `i`.
    pub fn local_tm(&self, i: usize) -> Option<D3dMatrix> {
        (i < self.bones).then(|| self.mat(bone_tm_offset(i) + 128))
    }

    /// Event slot `k` (`parent == -1` for an unused slot).
    pub fn event(&self, k: usize) -> Option<ChrEvent> {
        if k >= self.events {
            return None;
        }
        let off = HEADER_SIZE + BONE_SIZE * self.bones + EVENT_SIZE * k;
        let pos = f32s::<3>(self.buf.get(off..off + 12).unwrap_or(&[]));
        Some(ChrEvent {
            pos,
            parent: self.word(off + 12) as i32,
        })
    }

    /// Copies the view into owned structures.
    pub fn to_owned(&self) -> OwnedSkeleton {
        OwnedSkeleton {
            source_version: self.source_version,
            flags: self.flags,
            special: self.special_bones(),
            local_rh: self.local_rh(),
            local_lh: self.local_lh(),
            local_shield: self.local_shield(),
            local_knuckle: self.local_knuckle(),
            bones: (0..self.bones)
                .map(|i| PackedBone {
                    parent: self.parent(i).unwrap_or(-1),
                    tm: self.tm(i).unwrap_or_default(),
                    inverse_tm: self.inverse_tm(i).unwrap_or_default(),
                    local_tm: self.local_tm(i).unwrap_or_default(),
                })
                .collect(),
            events: (0..self.events).filter_map(|k| self.event(k)).collect(),
        }
    }
}

// ---------------------------------------------------------------------------------------------------------
// `decode_chr` meta (Plan/30 §14.1)
// ---------------------------------------------------------------------------------------------------------

/// Options of [`pack_chr`] (`decode_chr`'s `opts`, Plan/30 §14.1): the space of the attachment info.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", serde(default))]
pub struct ChrPackOptions {
    /// Space of the grips and event points (default [`Space::Three`]); the `FSK1` blob is always left-handed.
    pub space: Space,
}

/// One bone of [`ChrMeta`]: the name and the parent index (matrices stay inside the `FSK1` blob).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChrBoneMeta {
    /// Bone name (`to_string_lossy_ascii`: non-ASCII bytes appear as U+FFFD).
    pub name: String,
    /// Parent index, `-1` for the root.
    pub parent: i32,
}

/// The attachment grips of a `.chr` (`local_rh` ... `local_knuckle`), mapped to the requested space.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct GripsMeta {
    /// Right-hand grip matrix.
    pub rh: [f32; 16],
    /// Left-hand grip matrix (`None` when the file has no `local_lh`).
    pub lh: Option<[f32; 16]>,
    /// Shield grip matrix.
    pub shield: [f32; 16],
    /// Knuckle grip matrix.
    pub knuckle: [f32; 16],
}

/// An event point of a `.chr` (boss effect anchors), mapped to the requested space.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct EventMeta {
    /// Position (mapped).
    pub pos: [f32; 3],
    /// Parent bone, `-1` for none.
    pub parent: i32,
}

/// The structured description of a `.chr` pack (Plan/30 §14.1 `ChrMeta`). The whole blob is one `FSK1`
/// (`skeleton = { off: 0, len: blob.byteLength }`), passed unchanged to `SimWorld.add_skeleton`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChrMeta {
    /// `.chr` file version.
    pub version: i32,
    /// Number of bones.
    pub bone_count: u32,
    /// Bone names and parents in file order.
    pub bones: Vec<ChrBoneMeta>,
    /// The four special bone indices (`r hand`, `l hand`, `l forearm`, `r forearm`; 0 when missing).
    pub special: SpecialBones,
    /// The attachment grips (mapped).
    pub grips: GripsMeta,
    /// Event points (0, 4 or 8; mapped).
    pub events: Vec<EventMeta>,
    /// `bSendVS`.
    pub send_vs: bool,
    /// The `FSK1` section: always the whole blob.
    pub skeleton: BlobView,
}

/// Builds the `decode_chr` result: `meta` (names, special bones, mapped grips and events) plus the `FSK1`
/// blob of [`build_from_chr`] (left-handed, byte for byte the `add_skeleton` input).
///
/// Errors: the validation errors of [`build_from_chr`] (`Inconsistent`).
pub fn pack_chr(chr: &ChrFile, opts: &ChrPackOptions) -> Result<Packed<ChrMeta>, FormatError> {
    let blob = build_from_chr(chr)?;
    let len = u32::try_from(blob.len()).map_err(|_| err(0, "skeleton pack exceeds 4 GiB"))?;
    let three = opts.space == Space::Three;
    let mat = |m: &D3dMatrix| if three { mirror_mat(m).0 } else { m.0 };
    let pos = |p: [f32; 3]| if three { mirror_pos(p) } else { p };
    let meta = ChrMeta {
        version: chr.version,
        bone_count: u32::try_from(chr.bones.len()).unwrap_or(u32::MAX),
        bones: chr
            .bones
            .iter()
            .map(|b| ChrBoneMeta {
                name: b.name.to_string_lossy_ascii(),
                parent: b.parent,
            })
            .collect(),
        special: chr.special_bones(),
        grips: GripsMeta {
            rh: mat(&chr.local_rh),
            lh: chr.local_lh.as_ref().map(mat),
            shield: mat(&chr.local_shield),
            knuckle: mat(&chr.local_knuckle),
        },
        events: chr
            .events
            .iter()
            .map(|e| EventMeta {
                pos: pos(e.pos),
                parent: e.parent,
            })
            .collect(),
        send_vs: chr.send_vs != 0,
        skeleton: BlobView {
            off: 0,
            len,
            ty: ElemType::U8,
            comps: 1,
            count: len,
        },
    };
    Ok(Packed { meta, blob })
}
