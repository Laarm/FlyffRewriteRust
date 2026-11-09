//! The motion table (`FLMT` v1) and motion resolution (Plan/31 §12.1; WP-SIM-05).
//!
//! The table is built by the asset builder of Plan/24 (`AssetKind::MotionTable`) from the model registry
//! and the `.ani`/`.o3d` headers; `encode` is the serialiser it calls. The server decodes the same bytes
//! the browser fetches (`/gen/motion-table/<fp>/all.bin`). The sim never parses JSON.
//!
//! Layout (little endian, 4-byte aligned): a 32-byte header (`FLMT`, version, flags, `file_len`, counts of
//! clips/models/slots/attrs, string length), then `ClipDesc[C]` (28 B), `ModelDesc[M]` (28 B),
//! `SlotDesc[S]` (8 B), `AttrDesc[A]` (16 B) and the UTF-8 path string area padded to 4 bytes.

use crate::types::SimError;
use std::collections::BTreeMap;

/// `FLMT` format version (`MOTION_TABLE_VERSION`).
pub const MOTION_TABLE_VERSION: u16 = 1;
const HEADER_LEN: usize = 32;
const CLIP_DESC_LEN: usize = 28;
const MODEL_DESC_LEN: usize = 28;
const SLOT_DESC_LEN: usize = 8;
const ATTR_DESC_LEN: usize = 16;

/// Index of a clip in [`MotionTable::clips`] (the row of the JSON `motions` bundle `clips` table).
pub type ClipId = u32;

/// Identifies the model whose motions are listed: `(obj_type: OT_*, index)` (movers, items with skeletons,
/// ctrls/objs with built-in animations).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ModelKey {
    /// `OT_*` object type.
    pub ot: u32,
    /// Model index within the type (`m_dwIndex`).
    pub index: u32,
}

impl ModelKey {
    /// A key.
    pub const fn new(ot: u32, index: u32) -> ModelKey {
        ModelKey { ot, index }
    }
}

/// Where a clip's frames come from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClipSource {
    /// A `.ani` motion file.
    Ani = 0,
    /// The `.o3d` file's own built-in animation.
    O3dBuiltin = 1,
}

/// One frame attribute (`MOTION_ATTR`): hit / sound / quake at an (integer array slot, exact frame).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct MotionAttr {
    /// Array slot = `(int)frame`.
    pub index: u32,
    /// `MA_HIT 1 | MA_SOUND 2 | MA_QUAKE 4`.
    pub flags: u32,
    /// Sound table id (used when `MA_SOUND`).
    pub sound: i32,
    /// The exact (possibly fractional) frame.
    pub frame: f32,
}

/// Header-level facts about one `.ani` or `.o3d` animation.
#[derive(Clone, PartialEq, Debug)]
pub struct ClipMeta {
    /// Lower-case VFS path (`.ani`, or the `.o3d` of a built-in).
    pub file: String,
    /// Source kind.
    pub source: ClipSource,
    /// Frame count (`nFrame`, `> 0`).
    pub frames: u32,
    /// The file's `perSlerp` (raw; `trunc3` is applied by the clock exactly once).
    pub per_slerp_raw: f32,
    /// Bone count (`0` for `.o3d` built-ins).
    pub bones: u16,
    /// Only entries with `flags != 0`, ascending `index`.
    pub attrs: Vec<MotionAttr>,
}

impl ClipMeta {
    /// The attribute stored at array slot `index`.
    pub fn attr_at(&self, index: u32) -> Option<&MotionAttr> {
        self.attrs
            .binary_search_by_key(&index, |a| a.index)
            .ok()
            .and_then(|i| self.attrs.get(i))
    }
}

/// A motion slot of a model.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slot {
    /// The clip to play.
    Clip(ClipId),
    /// The referenced `.ani` is absent or unparsable (449 files): the original `LoadMotion` fails and
    /// leaves no clip; hits never fire.
    Missing,
}

/// Motion data of one model.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ModelMotions {
    /// `m_nMax`: highest listed motion id + 1, `0` = no list.
    pub max: u32,
    /// Non-empty slots only, by `MTI_*`.
    pub slots: BTreeMap<u32, Slot>,
    /// The clip of an EMPTY slot (the `MakeMotionName` quirk), `None` when that file is absent.
    pub empty: Option<ClipId>,
    /// The model's own `.o3d` built-in animation (ctrls, objs, 35 embedded-skeleton files).
    pub builtin: Option<ClipId>,
}

/// The motion table: clips plus per-model slots.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct MotionTable {
    /// `ClipId` = index.
    pub clips: Vec<ClipMeta>,
    /// Models by `(ot, index)`.
    pub models: BTreeMap<ModelKey, ModelMotions>,
}

impl MotionTable {
    /// `resolve(model, mti)` (`src:_Common/ModelMng.cpp:124-155`): an out-of-range or `NULL_ID` motion id
    /// maps to `MTI_STAND`'s slot; a `Missing` slot gives `None`; an empty slot gives the model's `empty`
    /// clip (KEEP-QUIRK: its file name is built from STAND's string, which can differ from STAND's own
    /// clip); an unknown model gives `None`.
    pub fn resolve(&self, model: ModelKey, mti: u32) -> Option<ClipId> {
        let mm = self.models.get(&model)?;
        let m = if mti == u32::MAX || mti >= mm.max {
            crate::types::MTI_STAND
        } else {
            mti
        };
        match mm.slots.get(&m) {
            Some(Slot::Clip(c)) => Some(*c),
            Some(Slot::Missing) => None,
            None => mm.empty,
        }
    }

    /// The model's built-in `.o3d` animation clip.
    pub fn builtin(&self, model: ModelKey) -> Option<ClipId> {
        self.models.get(&model)?.builtin
    }

    /// The clip metadata of `id`.
    #[inline]
    pub fn clip(&self, id: ClipId) -> Option<&ClipMeta> {
        self.clips.get(id as usize)
    }

    /// Serialises the table as `FLMT` v1. The table must satisfy the invariants of
    /// [`MotionTable::decode`] (clips sorted by unique path, attrs ascending, ids in range); a table that
    /// does not encodes to bytes the decoder rejects.
    pub fn encode(&self) -> Vec<u8> {
        let mut strings = Vec::new();
        let mut clip_descs = Vec::with_capacity(self.clips.len() * CLIP_DESC_LEN);
        let mut attr_descs = Vec::new();
        let mut attr_count = 0u32;
        for c in &self.clips {
            let off = strings.len() as u32; // BOUNDS: path area far below 4 GiB
            strings.extend_from_slice(c.file.as_bytes());
            put_u32(&mut clip_descs, c.frames);
            put_f32(&mut clip_descs, c.per_slerp_raw);
            clip_descs.extend_from_slice(&c.bones.to_le_bytes());
            clip_descs.push(c.source as u8);
            clip_descs.push(0);
            put_u32(&mut clip_descs, attr_count);
            put_u32(&mut clip_descs, c.attrs.len() as u32); // BOUNDS: attr counts are tiny
            put_u32(&mut clip_descs, off);
            put_u32(&mut clip_descs, c.file.len() as u32); // BOUNDS: path lengths are tiny
            for a in &c.attrs {
                put_u32(&mut attr_descs, a.index);
                put_u32(&mut attr_descs, a.flags);
                put_u32(&mut attr_descs, a.sound as u32); // BOUNDS: bit reinterpretation of i32
                put_f32(&mut attr_descs, a.frame);
            }
            attr_count += c.attrs.len() as u32; // BOUNDS: see above
        }
        let mut model_descs = Vec::with_capacity(self.models.len() * MODEL_DESC_LEN);
        let mut slot_descs = Vec::new();
        let mut slot_count = 0u32;
        for (k, m) in &self.models {
            put_u32(&mut model_descs, k.ot);
            put_u32(&mut model_descs, k.index);
            put_u32(&mut model_descs, m.max);
            put_i32(&mut model_descs, m.empty.map_or(-1, |c| c as i32)); // BOUNDS: ClipIds are < 2^31
            put_i32(&mut model_descs, m.builtin.map_or(-1, |c| c as i32)); // BOUNDS: see above
            put_u32(&mut model_descs, slot_count);
            put_u32(&mut model_descs, m.slots.len() as u32); // BOUNDS: slot counts are tiny
            for (mti, s) in &m.slots {
                put_u32(&mut slot_descs, *mti);
                put_i32(
                    &mut slot_descs,
                    match s {
                        Slot::Clip(c) => *c as i32, // BOUNDS: ClipIds are < 2^31
                        Slot::Missing => -1,
                    },
                );
            }
            slot_count += m.slots.len() as u32; // BOUNDS: see above
        }
        let pad = (4 - strings.len() % 4) % 4;
        let strings_len = strings.len() as u32; // BOUNDS: far below 4 GiB
        strings.extend(std::iter::repeat_n(0u8, pad));
        let file_len = HEADER_LEN
            + clip_descs.len()
            + model_descs.len()
            + slot_descs.len()
            + attr_descs.len()
            + strings.len();
        let mut out = Vec::with_capacity(file_len);
        out.extend_from_slice(b"FLMT");
        out.extend_from_slice(&MOTION_TABLE_VERSION.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        put_u32(&mut out, file_len as u32); // BOUNDS: far below 4 GiB
        put_u32(&mut out, self.clips.len() as u32); // BOUNDS: far below 2^32
        put_u32(&mut out, self.models.len() as u32); // BOUNDS: far below 2^32
        put_u32(&mut out, slot_count);
        put_u32(&mut out, attr_count);
        put_u32(&mut out, strings_len);
        out.extend_from_slice(&clip_descs);
        out.extend_from_slice(&model_descs);
        out.extend_from_slice(&slot_descs);
        out.extend_from_slice(&attr_descs);
        out.extend_from_slice(&strings);
        out
    }

    /// Decodes and validates a `FLMT` v1 table (Plan/31 §12.1). Invariants: clips sorted by unique path
    /// bytes with `frames > 0` (and `<= i32::MAX`), `source` in `{0, 1}`; models sorted by unique
    /// `(ot, index)`; each model's slots a contiguous run sorted by unique `mti`; each clip's attrs a
    /// contiguous run with `flags != 0`, strictly ascending `index < frames`; every ClipId `< C` or `-1`
    /// where allowed; paths consecutive in the string area, UTF-8; floats finite. Violations are
    /// [`SimError::BadMotionTable`]; the function never panics.
    pub fn decode(b: &[u8]) -> Result<MotionTable, SimError> {
        let mut r = Rd { b, pos: 0 };
        if r.take(4)? != b"FLMT" {
            return Err(bad("wrong magic"));
        }
        if r.u16()? != MOTION_TABLE_VERSION {
            return Err(bad("unsupported version"));
        }
        if r.u16()? != 0 {
            return Err(bad("flags must be 0"));
        }
        let file_len = r.u32()? as usize;
        let c = r.u32()? as usize;
        let m = r.u32()? as usize;
        let s = r.u32()? as usize;
        let a = r.u32()? as usize;
        let l = r.u32()? as usize;
        let expect = c
            .checked_mul(CLIP_DESC_LEN)
            .and_then(|n| n.checked_add(m.checked_mul(MODEL_DESC_LEN)?))
            .and_then(|n| n.checked_add(s.checked_mul(SLOT_DESC_LEN)?))
            .and_then(|n| n.checked_add(a.checked_mul(ATTR_DESC_LEN)?))
            .and_then(|n| n.checked_add(l.checked_add(3)? & !3))
            .and_then(|n| n.checked_add(HEADER_LEN))
            .ok_or_else(|| bad("overflow"))?;
        if file_len != b.len() || expect != b.len() {
            return Err(bad("length mismatch"));
        }
        let clip_off = HEADER_LEN;
        let model_off = clip_off + c * CLIP_DESC_LEN;
        let slot_off = model_off + m * MODEL_DESC_LEN;
        let attr_off = slot_off + s * SLOT_DESC_LEN;
        let str_off = attr_off + a * ATTR_DESC_LEN;
        let strings = b
            .get(str_off..str_off + l)
            .ok_or_else(|| bad("string area"))?;
        if b.get(str_off + l..)
            .is_some_and(|pad| pad.iter().any(|&x| x != 0))
        {
            return Err(bad("string padding is not zero"));
        }

        // attributes
        let mut attrs = Vec::with_capacity(a);
        let mut ar = Rd { b, pos: attr_off };
        for _ in 0..a {
            attrs.push(MotionAttr {
                index: ar.u32()?,
                flags: ar.u32()?,
                sound: ar.u32()? as i32, // BOUNDS: bit reinterpretation
                frame: ar.finite_f32()?,
            });
        }
        // clips
        let mut clips = Vec::with_capacity(c);
        let mut cr = Rd { b, pos: clip_off };
        let mut next_attr = 0usize;
        let mut next_str = 0usize;
        let mut prev_path: Option<&[u8]> = None;
        for _ in 0..c {
            let frames = cr.u32()?;
            let per_slerp_raw = cr.finite_f32()?;
            let bones = cr.u16()?;
            let source = match cr.u8()? {
                0 => ClipSource::Ani,
                1 => ClipSource::O3dBuiltin,
                _ => return Err(bad("unknown clip source")),
            };
            if cr.u8()? != 0 {
                return Err(bad("clip reserved byte"));
            }
            let first_attr = cr.u32()? as usize;
            let attr_count = cr.u32()? as usize;
            let path_off = cr.u32()? as usize;
            let path_len = cr.u32()? as usize;
            if frames == 0 || i32::try_from(frames).is_err() {
                return Err(bad("clip frame count"));
            }
            if first_attr != next_attr {
                return Err(bad("clip attrs are not a contiguous run"));
            }
            let end_attr = first_attr
                .checked_add(attr_count)
                .ok_or_else(|| bad("overflow"))?;
            let list = attrs
                .get(first_attr..end_attr)
                .ok_or_else(|| bad("clip attrs out of range"))?;
            next_attr = end_attr;
            let mut prev_idx: Option<u32> = None;
            for at in list {
                if at.flags == 0 || at.index >= frames || prev_idx.is_some_and(|p| at.index <= p) {
                    return Err(bad("clip attr order or flags"));
                }
                prev_idx = Some(at.index);
            }
            if path_off != next_str {
                return Err(bad("paths are not consecutive"));
            }
            let end_str = path_off
                .checked_add(path_len)
                .ok_or_else(|| bad("overflow"))?;
            let raw = strings
                .get(path_off..end_str)
                .ok_or_else(|| bad("path out of range"))?;
            next_str = end_str;
            let file = core::str::from_utf8(raw).map_err(|_| bad("path is not UTF-8"))?;
            if prev_path.is_some_and(|p| raw <= p) {
                return Err(bad("clips are not sorted by unique path"));
            }
            prev_path = Some(raw);
            clips.push(ClipMeta {
                file: file.to_owned(),
                source,
                frames,
                per_slerp_raw,
                bones,
                attrs: list.to_vec(),
            });
        }
        if next_attr != a || next_str != l {
            return Err(bad("unused attrs or string bytes"));
        }
        // slots
        let mut slots = Vec::with_capacity(s);
        let mut sr = Rd { b, pos: slot_off };
        for _ in 0..s {
            let mti = sr.u32()?;
            let clip = sr.i32()?;
            let slot = if clip == -1 {
                Slot::Missing
            } else {
                let id = u32::try_from(clip).map_err(|_| bad("slot clip id"))?;
                if (id as usize) >= c {
                    return Err(bad("slot clip id out of range"));
                }
                Slot::Clip(id)
            };
            slots.push((mti, slot));
        }
        // models
        let mut models = BTreeMap::new();
        let mut mr = Rd { b, pos: model_off };
        let mut next_slot = 0usize;
        let mut prev_key: Option<ModelKey> = None;
        for _ in 0..m {
            let key = ModelKey {
                ot: mr.u32()?,
                index: mr.u32()?,
            };
            let max = mr.u32()?;
            let empty = opt_clip(mr.i32()?, c)?;
            let builtin = opt_clip(mr.i32()?, c)?;
            let first_slot = mr.u32()? as usize;
            let slot_count = mr.u32()? as usize;
            if prev_key.is_some_and(|p| key <= p) {
                return Err(bad("models are not sorted by unique (ot, index)"));
            }
            prev_key = Some(key);
            if first_slot != next_slot {
                return Err(bad("model slots are not a contiguous run"));
            }
            let end = first_slot
                .checked_add(slot_count)
                .ok_or_else(|| bad("overflow"))?;
            let run = slots
                .get(first_slot..end)
                .ok_or_else(|| bad("model slots out of range"))?;
            next_slot = end;
            let mut map = BTreeMap::new();
            let mut prev_mti: Option<u32> = None;
            for (mti, slot) in run {
                if prev_mti.is_some_and(|p| *mti <= p) {
                    return Err(bad("slots are not sorted by unique mti"));
                }
                prev_mti = Some(*mti);
                map.insert(*mti, *slot);
            }
            models.insert(
                key,
                ModelMotions {
                    max,
                    slots: map,
                    empty,
                    builtin,
                },
            );
        }
        if next_slot != s {
            return Err(bad("unused slots"));
        }
        Ok(MotionTable { clips, models })
    }
}

fn opt_clip(v: i32, clip_count: usize) -> Result<Option<ClipId>, SimError> {
    if v == -1 {
        return Ok(None);
    }
    let id = u32::try_from(v).map_err(|_| bad("clip id"))?;
    if (id as usize) >= clip_count {
        return Err(bad("clip id out of range"));
    }
    Ok(Some(id))
}

fn bad(msg: &'static str) -> SimError {
    SimError::BadMotionTable(msg)
}

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn put_i32(out: &mut Vec<u8>, v: i32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn put_f32(out: &mut Vec<u8>, v: f32) {
    out.extend_from_slice(&v.to_le_bytes());
}

struct Rd<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Rd<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], SimError> {
        let end = self.pos.checked_add(n).ok_or_else(|| bad("overflow"))?;
        let s = self.b.get(self.pos..end).ok_or_else(|| bad("truncated"))?;
        self.pos = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, SimError> {
        self.take(1)?
            .first()
            .copied()
            .ok_or_else(|| bad("truncated"))
    }
    fn u16(&mut self) -> Result<u16, SimError> {
        Ok(u16::from_le_bytes(
            self.take(2)?.try_into().map_err(|_| bad("truncated"))?,
        ))
    }
    fn u32(&mut self) -> Result<u32, SimError> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().map_err(|_| bad("truncated"))?,
        ))
    }
    fn i32(&mut self) -> Result<i32, SimError> {
        Ok(i32::from_le_bytes(
            self.take(4)?.try_into().map_err(|_| bad("truncated"))?,
        ))
    }
    fn finite_f32(&mut self) -> Result<f32, SimError> {
        let v = f32::from_le_bytes(self.take(4)?.try_into().map_err(|_| bad("truncated"))?);
        if v.is_finite() {
            Ok(v)
        } else {
            Err(bad("non-finite float"))
        }
    }
}

/// The byte length an `FLMT` file of the given counts has (`32 + 28 C + 28 M + 8 S + 16 A + pad4(L)`).
pub fn flmt_len(clips: usize, models: usize, slots: usize, attrs: usize, strings: usize) -> usize {
    HEADER_LEN
        + CLIP_DESC_LEN * clips
        + MODEL_DESC_LEN * models
        + SLOT_DESC_LEN * slots
        + ATTR_DESC_LEN * attrs
        + ((strings + 3) & !3)
}
