//! Object records shared by `.lnd` and `.dyo` (Plan/30 §7; ref/19 §7; src:_Common/CreateObj.cpp:718-747,
//! Obj.cpp:471-530, Mover.cpp:2960-2980, CommonCtrl.cpp:84-115).
//!
//! A record is a `u32` object type, the 60-byte base (`CObj::Read`) and a type extension: movers carry 136
//! more bytes, controls a `u32` version plus a [`CtrlElemDef`] in one of three on-disk variants; every other
//! known type is the base only. The parser never normalises (R4): positions stay as stored (x and z divided
//! by [`OLD_MPU`]); [`ObjBase::absolute_position`] and [`ObjBase::yaw_pitch_roll_deg`] are the helpers that do.

use crate::FormatKind;
use crate::error::{ErrorKind, FormatError};
use crate::raw::{RawStr, Vec3};
use crate::reader::ByteReader;

/// `OT_OBJ`: static background object (all `.lnd` objects).
pub const OT_OBJ: u32 = 0;
/// `OT_ANI`: never a record type (the client re-buckets animated `OT_OBJ` at load time).
pub const OT_ANI: u32 = 1;
/// `OT_CTRL`: chests, traps, switches.
pub const OT_CTRL: u32 = 2;
/// `OT_SFX`: effect placement (client only; all `.lnd` static sfx).
pub const OT_SFX: u32 = 3;
/// `OT_ITEM`: dropped item (base only).
pub const OT_ITEM: u32 = 4;
/// `OT_MOVER`: NPC or monster (all `.dyo` movers).
pub const OT_MOVER: u32 = 5;
/// `OT_SHIP`: ship (base only).
pub const OT_SHIP: u32 = 7;
/// The `.dyo` list terminator.
pub const END_OF_LIST: u32 = 0xFFFF_FFFF;

/// `OLD_MPU`: x and z of every stored position are divided by this, independent of the world MPU
/// (`_Common/DefineCommon.cpp:10`, ref/19 §7.1).
pub const OLD_MPU: f32 = 4.0;
/// Cells per land side (`MAP_SIZE`, landscape.h:6).
pub const LAND_CELLS: i64 = 128;

/// Size of the base after the type word (`CObj::Read`).
pub const BASE_SIZE: usize = 60;
/// Size of the mover extension (`CMover::Read`).
pub const MOVER_EXT_SIZE: usize = 136;
/// `sizeof(CCtrlElem)`.
pub const CTRL_ELEM_SIZE: usize = 432;
/// Smallest possible record: type word plus base (64 bytes).
pub const MIN_RECORD_SIZE: usize = 4 + BASE_SIZE;
/// `ctrl_version` of the plain 432-byte layout (the only one in the data).
pub const CTRL_VERSION_FULL: u32 = 0x8000_0000;
/// `ctrl_version` of the layout with `set_job[16..32]` absent (368 bytes).
pub const CTRL_VERSION_JOBS16: u32 = 0x9000_0000;

/// `true` for the record types a parser reads: `OT_OBJ`, `OT_CTRL`, `OT_SFX`, `OT_ITEM`, `OT_MOVER`,
/// `OT_SHIP`. Everything else (including [`END_OF_LIST`]) ends a `.dyo` list.
pub fn is_known_type(obj_type: u32) -> bool {
    matches!(
        obj_type,
        OT_OBJ | OT_CTRL | OT_SFX | OT_ITEM | OT_MOVER | OT_SHIP
    )
}

/// One object record (Plan/30 §7.3).
#[derive(Debug, Clone, PartialEq)]
pub struct ObjRecord {
    /// The record's type word (`OT_*`).
    pub obj_type: u32,
    /// The 60-byte base.
    pub base: ObjBase,
    /// The type extension.
    pub ext: ObjExt,
}

impl ObjRecord {
    /// The mover extension, for `OT_MOVER` records.
    pub fn mover(&self) -> Option<&MoverExt> {
        match &self.ext {
            ObjExt::Mover(m) => Some(m),
            _ => None,
        }
    }

    /// The control payload `(ctrl_version, element)`, for `OT_CTRL` records.
    pub fn ctrl(&self) -> Option<(u32, &CtrlElemDef)> {
        match &self.ext {
            ObjExt::Ctrl { version, elem } => Some((*version, elem)),
            _ => None,
        }
    }
}

/// `CObj::Read`: the 60 bytes after the type word, exactly as stored (Plan/30 §7.1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ObjBase {
    /// Yaw in degrees (`m_fAngle`).
    pub angle: f32,
    /// `vAxis`: `.x` is pitch in degrees (`m_fAngX`), `.y` is unused, `.z` is roll in degrees (`m_fAngZ`).
    pub axis: Vec3,
    /// Position as stored: x and z divided by [`OLD_MPU`], y in world units.
    pub pos_stored: Vec3,
    /// Scale (non-uniform allowed).
    pub scale: Vec3,
    /// Duplicate of the record type (0 for `.lnd` objects, 2/5 for `.dyo`). The `0x10000/0x20000`
    /// invisible flags exist only under the inactive `__BS_ADDOBJATTR_INVISIBLE` (DROP).
    pub type_dup: u32,
    /// `m_dwIndex`: mdlObj index (`OT_OBJ`), MoverProp id (`OT_MOVER`), control prop id, SFX id.
    pub index: u32,
    /// `dwMotion`: initial motion for movers (`0xFFFFFFFF` in the data).
    pub motion: u32,
    /// `dwAIInterface` (server: `SetAIInterface(ai, ai2)` for movers).
    pub ai_interface: u32,
    /// `dwAI2`.
    pub ai2: u32,
}

impl ObjBase {
    /// Reads the 60-byte base.
    pub fn read(r: &mut ByteReader<'_>) -> Result<ObjBase, FormatError> {
        Ok(ObjBase {
            angle: r.f32()?,
            axis: r.vec3()?,
            pos_stored: r.vec3()?,
            scale: r.vec3()?,
            type_dup: r.u32()?,
            index: r.u32()?,
            motion: r.u32()?,
            ai_interface: r.u32()?,
            ai2: r.u32()?,
        })
    }

    /// World position: x and z times [`OLD_MPU`] (4, independent of the world MPU), plus for `.lnd`
    /// objects the land offset `land · 128 · mpu` (client `lod.cpp:1356`, server `WorldFile.cpp:897`).
    /// `placement == None` is the `.dyo` rule: absolute after the multiplication.
    pub fn absolute_position(&self, placement: Option<&Placement>) -> Vec3 {
        let [px, y, pz] = self.pos_stored;
        let (x, z) = (px * OLD_MPU, pz * OLD_MPU);
        match placement {
            None => [x, y, z],
            Some(p) => {
                let span = i64::from(p.mpu).saturating_mul(LAND_CELLS);
                let off_x = i64::from(p.land_x).saturating_mul(span) as f32;
                let off_z = i64::from(p.land_z).saturating_mul(span) as f32;
                [x + off_x, y, z + off_z]
            }
        }
    }

    /// The arguments of `D3DXMatrixRotationYawPitchRoll` in degrees, `(yaw, pitch, roll) = (-angle,
    /// -axis.x, +axis.z)` (src:_Common/Obj.cpp:126-147, `__VER >= 11`).
    pub fn yaw_pitch_roll_deg(&self) -> (f32, f32, f32) {
        (-self.angle, -self.axis[0], self.axis[2])
    }
}

/// Where a `.lnd` object's land sits: the indices come from the FILE NAME (`Name%02d-%02d.lnd`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    /// Land column (the first number of the file name).
    pub land_x: i32,
    /// Land row (the second number of the file name).
    pub land_z: i32,
    /// World units per heightmap cell (`.wld` key `MPU`, default 4).
    pub mpu: u32,
}

/// The type-specific tail of a record (Plan/30 §7.3).
#[derive(Debug, Clone, PartialEq)]
pub enum ObjExt {
    /// `OT_OBJ`, `OT_SFX`, `OT_ITEM`, `OT_SHIP`: base only.
    None,
    /// `OT_MOVER`: 136 bytes.
    Mover(MoverExt),
    /// `OT_CTRL`: the on-disk version word and the decoded 432-byte element (boxed: it is large).
    Ctrl {
        /// `ctrl_version` as stored (`0x80000000`, `0x90000000`, or the legacy `set` value).
        version: u32,
        /// The control element.
        elem: Box<CtrlElemDef>,
    },
}

/// `CMover::Read` extension, 136 bytes (Plan/30 §7.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoverExt {
    /// `char[64]` editor name (CP949); the display name comes from `character.inc` via the key.
    pub name: RawStr,
    /// `char[32]` dialog file name (ignored by the original).
    pub dialog: RawStr,
    /// `char[32]` character key (`MaFl_Juria`) into `character.inc`.
    pub character_key: RawStr,
    /// `m_dwBelligerence`.
    pub belligerence: u32,
    /// `dwExtraFlag` (read, ignored by the original).
    pub extra_flag: u32,
}

impl MoverExt {
    /// Reads the 136-byte mover tail.
    pub fn read(r: &mut ByteReader<'_>) -> Result<MoverExt, FormatError> {
        Ok(MoverExt {
            name: r.fixed_str(64)?,
            dialog: r.fixed_str(32)?,
            character_key: r.fixed_str(32)?,
            belligerence: r.u32()?,
            extra_flag: r.u32()?,
        })
    }
}

/// `CCtrlElem` (432 bytes, src:_Common/CommonCtrl.h:17-57); also the payload of `.rgn` control respawns
/// (Plan/30 §7.2, §9.4). Field offsets in the comments.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CtrlElemDef {
    /// 0: `dwSet`.
    pub set: u32,
    /// 4: `dwSetItem`.
    pub set_item: u32,
    /// 8: `dwSetLevel`.
    pub set_level: u32,
    /// 12: `dwSetQuestNum`.
    pub set_quest_num: u32,
    /// 16: `dwSetFlagNum`.
    pub set_flag_num: u32,
    /// 20: `dwSetGender`.
    pub set_gender: u32,
    /// 24: `bSetJob[32]` (BOOL per job).
    pub set_job: [u32; 32],
    /// 152: `dwSetEndu`.
    pub set_endu: u32,
    /// 156: `dwMinItemNum`.
    pub min_item_num: u32,
    /// 160: `dwMaxiItemNum`.
    pub max_item_num: u32,
    /// 164: `dwInsideItemKind[4]`.
    pub inside_item_kind: [u32; 4],
    /// 180: `dwInsideItemPer[4]`.
    pub inside_item_per: [u32; 4],
    /// 196: `dwMonResKind[3]`.
    pub mon_res_kind: [u32; 3],
    /// 208: `dwMonResNum[3]`.
    pub mon_res_num: [u32; 3],
    /// 220: `dwMonActAttack[3]`.
    pub mon_act_attack: [u32; 3],
    /// 232: `dwTrapOperType`.
    pub trap_oper_type: u32,
    /// 236: `dwTrapRandomPer`.
    pub trap_random_per: u32,
    /// 240: `dwTrapDelay`.
    pub trap_delay: u32,
    /// 244: `dwTrapKind[3]`.
    pub trap_kind: [u32; 3],
    /// 256: `dwTrapLevel[3]`.
    pub trap_level: [u32; 3],
    /// 268: `strLinkCtrlKey` `char[64]`, cut at the first NUL.
    pub link_ctrl_key: RawStr,
    /// 332: `strCtrlKey` `char[64]`, cut at the first NUL.
    pub ctrl_key: RawStr,
    /// 396: `dwSetQuestNum1`.
    pub set_quest_num1: u32,
    /// 400: `dwSetFlagNum1`.
    pub set_flag_num1: u32,
    /// 404: `dwSetQuestNum2`.
    pub set_quest_num2: u32,
    /// 408: `dwSetFlagNum2`.
    pub set_flag_num2: u32,
    /// 412: `dwSetItemCount`.
    pub set_item_count: u32,
    /// 416: `dwTeleWorldId`.
    pub tele_world_id: u32,
    /// 420: `dwTeleX`.
    pub tele_x: u32,
    /// 424: `dwTeleY`.
    pub tele_y: u32,
    /// 428: `dwTeleZ`.
    pub tele_z: u32,
}

impl CtrlElemDef {
    /// Reads the element that follows `version` (already consumed), reproducing the three on-disk
    /// variants of `CCommonCtrl::Read` by filling a zeroed 432-byte image like the original struct
    /// (Plan/30 §7.2):
    ///
    /// * `0x80000000`: 432 bytes straight.
    /// * `0x90000000`: 88 bytes into offsets 0..88, then 280 bytes into 152..432 (`set_job[16..32]` stay 0).
    /// * any other value: the value is `set`; 392 bytes go into offsets 4..396; offsets 396..432 stay 0.
    pub fn read(r: &mut ByteReader<'_>, version: u32) -> Result<CtrlElemDef, FormatError> {
        let mut image = [0u8; CTRL_ELEM_SIZE];
        match version {
            CTRL_VERSION_FULL => put(&mut image, 0, r.bytes(CTRL_ELEM_SIZE)?),
            CTRL_VERSION_JOBS16 => {
                put(&mut image, 0, r.bytes(88)?);
                put(&mut image, 152, r.bytes(CTRL_ELEM_SIZE - 152)?);
            }
            other => {
                put(&mut image, 0, &other.to_le_bytes());
                put(&mut image, 4, r.bytes(CTRL_ELEM_SIZE - 40)?);
            }
        }
        CtrlElemDef::from_bytes(&image)
    }

    /// Decodes a 432-byte image (offsets of the field docs). Errors only when `bytes` is shorter.
    pub fn from_bytes(bytes: &[u8]) -> Result<CtrlElemDef, FormatError> {
        let mut r = ByteReader::new(bytes, FormatKind::Dyo);
        Ok(CtrlElemDef {
            set: r.u32()?,
            set_item: r.u32()?,
            set_level: r.u32()?,
            set_quest_num: r.u32()?,
            set_flag_num: r.u32()?,
            set_gender: r.u32()?,
            set_job: u32s(&mut r)?,
            set_endu: r.u32()?,
            min_item_num: r.u32()?,
            max_item_num: r.u32()?,
            inside_item_kind: u32s(&mut r)?,
            inside_item_per: u32s(&mut r)?,
            mon_res_kind: u32s(&mut r)?,
            mon_res_num: u32s(&mut r)?,
            mon_act_attack: u32s(&mut r)?,
            trap_oper_type: r.u32()?,
            trap_random_per: r.u32()?,
            trap_delay: r.u32()?,
            trap_kind: u32s(&mut r)?,
            trap_level: u32s(&mut r)?,
            link_ctrl_key: r.fixed_str(64)?,
            ctrl_key: r.fixed_str(64)?,
            set_quest_num1: r.u32()?,
            set_flag_num1: r.u32()?,
            set_quest_num2: r.u32()?,
            set_flag_num2: r.u32()?,
            set_item_count: r.u32()?,
            tele_world_id: r.u32()?,
            tele_x: r.u32()?,
            tele_y: r.u32()?,
            tele_z: r.u32()?,
        })
    }
}

fn u32s<const N: usize>(r: &mut ByteReader<'_>) -> Result<[u32; N], FormatError> {
    let mut out = [0u32; N];
    for slot in &mut out {
        *slot = r.u32()?;
    }
    Ok(out)
}

/// Copies `src` into `dst` at `at` (a no-op if it would not fit; the callers' sizes make that impossible).
fn put(dst: &mut [u8], at: usize, src: &[u8]) {
    if let Some(slot) = at
        .checked_add(src.len())
        .and_then(|end| dst.get_mut(at..end))
    {
        slot.copy_from_slice(src);
    }
}

/// Outcome of reading one record slot: a record, or the end of the list.
#[derive(Debug, Clone, PartialEq)]
pub enum RecordRead {
    /// A complete record.
    Record(ObjRecord),
    /// The list ended: `0xFFFFFFFF` or an unknown object type (the original's `ReadObj` returned NULL).
    /// The type word has been consumed.
    End(u32),
}

/// Reads one record slot. An unknown type word (including `0xFFFFFFFF`) is [`RecordRead::End`] with the
/// word consumed and nothing else read.
pub fn read_record_or_end(r: &mut ByteReader<'_>) -> Result<RecordRead, FormatError> {
    let obj_type = r.u32()?;
    if !is_known_type(obj_type) {
        return Ok(RecordRead::End(obj_type));
    }
    let base = ObjBase::read(r)?;
    let ext = match obj_type {
        OT_MOVER => ObjExt::Mover(MoverExt::read(r)?),
        OT_CTRL => {
            let version = r.u32()?;
            ObjExt::Ctrl {
                version,
                elem: Box::new(CtrlElemDef::read(r, version)?),
            }
        }
        _ => ObjExt::None,
    };
    Ok(RecordRead::Record(ObjRecord {
        obj_type,
        base,
        ext,
    }))
}

/// Reads one record; `None` on `0xFFFFFFFF` or any unknown type (Plan/30 §7.3 `read_record`). Use
/// [`read_record_or_end`] to learn the terminating word.
pub fn read_record(r: &mut ByteReader<'_>) -> Result<Option<ObjRecord>, FormatError> {
    Ok(match read_record_or_end(r)? {
        RecordRead::Record(rec) => Some(rec),
        RecordRead::End(_) => None,
    })
}

/// Bytes of the control element that follow a `ctrl_version` word.
fn ctrl_payload_len(version: u32) -> usize {
    match version {
        CTRL_VERSION_FULL => CTRL_ELEM_SIZE,
        CTRL_VERSION_JOBS16 => 88 + (CTRL_ELEM_SIZE - 152),
        _ => CTRL_ELEM_SIZE - 40,
    }
}

/// What [`skip_record`] found in a record slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Slot {
    /// A record of this type was skipped.
    Record(u32),
    /// End of list (`0xFFFFFFFF` or an unknown type word, consumed).
    End(u32),
}

/// Like [`read_record_or_end`] but advances without building the record (no allocation). Used by
/// `LandView` to find the extent of the object sections.
pub(crate) fn skip_record(r: &mut ByteReader<'_>) -> Result<Slot, FormatError> {
    let obj_type = r.u32()?;
    match obj_type {
        OT_MOVER => r.skip(BASE_SIZE + MOVER_EXT_SIZE)?,
        OT_CTRL => {
            r.skip(BASE_SIZE)?;
            let version = r.u32()?;
            r.skip(ctrl_payload_len(version))?;
        }
        t if is_known_type(t) => r.skip(BASE_SIZE)?,
        _ => return Ok(Slot::End(obj_type)),
    }
    Ok(Slot::Record(obj_type))
}

/// Iterator over `count` consecutive records starting at a byte offset (used by `LandView`). Stops after
/// the first error; an end-of-list word before `count` records is reported as an error because the
/// containing format promised `count` records.
#[derive(Debug, Clone)]
pub struct RecordIter<'a> {
    reader: ByteReader<'a>,
    left: usize,
    /// A failure of the initial seek, yielded by the first `next()`.
    pending: Option<FormatError>,
}

impl<'a> RecordIter<'a> {
    /// Iterates `count` records of `buf` starting at `offset`; `kind` labels errors. An `offset` beyond the
    /// buffer is reported by the first item.
    pub fn new(buf: &'a [u8], kind: FormatKind, offset: usize, count: usize) -> RecordIter<'a> {
        let mut reader = ByteReader::new(buf, kind);
        let pending = reader.seek(offset).err();
        RecordIter {
            reader,
            left: count,
            pending,
        }
    }

    /// Records not yet yielded.
    pub fn remaining(&self) -> usize {
        self.left
    }
}

impl Iterator for RecordIter<'_> {
    type Item = Result<ObjRecord, FormatError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.left == 0 {
            return None;
        }
        if let Some(e) = self.pending.take() {
            self.left = 0;
            return Some(Err(e));
        }
        self.left -= 1;
        let at = self.reader.pos();
        match read_record_or_end(&mut self.reader) {
            Ok(RecordRead::Record(rec)) => Some(Ok(rec)),
            Ok(RecordRead::End(t)) => {
                self.left = 0;
                Some(Err(self.reader.err_at(
                    at,
                    ErrorKind::InvalidValue {
                        field: "object type",
                        value: i64::from(t),
                    },
                )))
            }
            Err(e) => {
                self.left = 0;
                Some(Err(e))
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.left))
    }
}
