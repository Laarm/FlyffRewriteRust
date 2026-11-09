//! The browser wire contract of `SimWorld` (Plan/31 §17.4-§17.7, §23; WP-SIM-13/14/15).
//!
//! Everything the TypeScript `SimBridge` and the wasm `SimWorld` must agree on is defined **once** here as
//! tables: the command-buffer ops with their packed little-endian payload layouts ([`OPS`]), the
//! `SET_PARAMS` word layout ([`PARAMS_LAYOUT`]), the event kinds ([`EVENT_KINDS`]), the render view header,
//! record layouts and flags ([`view`]), the local report ([`report`]) and the local input record
//! ([`input`]). `tests/wire_export.rs` renders these tables into `web/src/gen/sim/wire.ts` (committed,
//! `UPDATE_GOLDEN=1` rewrites it), so the TS side never hand-types a number.
//!
//! The command buffer is a stream of records `{op: u16, len: u16, payload}`; every payload is packed
//! (no implicit alignment) little endian, 64-bit frames are written as two `u32` (lo, hi) which is the
//! same bytes as one LE `u64`. [`CmdWriter`] is the reference encoder (tests, vectors, recording).

use crate::collision::geom::Aabb;
use crate::mover::{RideParams, SimMoverParams, WeaponStance};
use crate::num::{Vec3, dtoi_x86};
use crate::step::SimFlags;
use crate::types::{MoverKind, SimError};
use flyff_common::config::HitAttrRule;

/// Version of this wire contract (command layouts, params layout, view layout, event kinds). Bumped on any
/// incompatible change; client and wasm ship together, so it is informational (the view header carries
/// its own [`view::VERSION`]).
pub const WIRE_VERSION: u32 = 1;

// ---------------------------------------------------------------------------------------------------
// field types and op table
// ---------------------------------------------------------------------------------------------------

/// A field type of a command payload.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ty {
    /// `u8`.
    U8,
    /// `u16`.
    U16,
    /// `u32`.
    U32,
    /// `i32`.
    I32,
    /// `u64` (the frames: two `u32`, lo then hi).
    U64,
    /// `i64` (the clock offset).
    I64,
    /// `f32`.
    F32,
}

impl Ty {
    /// Size in bytes.
    pub const fn size(self) -> usize {
        match self {
            Ty::U8 => 1,
            Ty::U16 => 2,
            Ty::U32 | Ty::I32 | Ty::F32 => 4,
            Ty::U64 | Ty::I64 => 8,
        }
    }

    /// The name used in the generated TypeScript.
    pub const fn name(self) -> &'static str {
        match self {
            Ty::U8 => "u8",
            Ty::U16 => "u16",
            Ty::U32 => "u32",
            Ty::I32 => "i32",
            Ty::U64 => "u64",
            Ty::I64 => "i64",
            Ty::F32 => "f32",
        }
    }
}

/// One field of a payload: `count` consecutive elements of `ty`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Field {
    /// Field name (`pad` fields are explicit padding).
    pub name: &'static str,
    /// Element type.
    pub ty: Ty,
    /// Number of elements (1 for scalars).
    pub count: u16,
}

const fn f(name: &'static str, ty: Ty) -> Field {
    Field { name, ty, count: 1 }
}

const fn fa(name: &'static str, ty: Ty, count: u16) -> Field {
    Field { name, ty, count }
}

/// One command op (Plan/31 §17.4).
#[derive(Clone, Copy, Debug)]
pub struct OpDesc {
    /// The op number.
    pub code: u16,
    /// `SCREAMING_SNAKE` name.
    pub name: &'static str,
    /// The packed payload fields in order.
    pub fields: &'static [Field],
    /// The record carries a server frame (`frame u64`): it is applied at local frame `frame - offset`.
    pub framed: bool,
}

impl OpDesc {
    /// The payload length in bytes.
    pub const fn payload_len(&self) -> usize {
        let mut n = 0;
        let mut rest = self.fields;
        while let [first, tail @ ..] = rest {
            n += first.ty.size() * first.count as usize;
            rest = tail;
        }
        n
    }

    /// Byte offset of the field `name` inside the payload.
    pub fn offset_of(&self, name: &str) -> Option<usize> {
        let mut off = 0;
        for fld in self.fields {
            if fld.name == name {
                return Some(off);
            }
            off += fld.ty.size() * usize::from(fld.count);
        }
        None
    }
}

/// `ADD_MOVER` op.
pub const OP_ADD_MOVER: u16 = 0x01;
/// `REMOVE_MOVER` op.
pub const OP_REMOVE_MOVER: u16 = 0x02;
/// `SET_PARAMS` op.
pub const OP_SET_PARAMS: u16 = 0x03;
/// `STATE` op.
pub const OP_STATE: u16 = 0x04;
/// `INTENT` op.
pub const OP_INTENT: u16 = 0x05;
/// `KEYFRAME` op.
pub const OP_KEYFRAME: u16 = 0x06;
/// `ACT` op.
pub const OP_ACT: u16 = 0x07;
/// `DAMAGE_FX` op.
pub const OP_DAMAGE_FX: u16 = 0x08;
/// `PUSH_POWER` op.
pub const OP_PUSH_POWER: u16 = 0x09;
/// `DIE` op.
pub const OP_DIE: u16 = 0x0A;
/// `MOTION_ARRIVE` op.
pub const OP_MOTION_ARRIVE: u16 = 0x0B;
/// `SET_FUEL` op.
pub const OP_SET_FUEL: u16 = 0x0C;
/// `SET_ACTIVE` op.
pub const OP_SET_ACTIVE: u16 = 0x0D;
/// `SET_CLOCK` op.
pub const OP_SET_CLOCK: u16 = 0x0E;
/// `SET_VISIBLE` op.
pub const OP_SET_VISIBLE: u16 = 0x0F;
/// `SET_SKILL` op.
pub const OP_SET_SKILL: u16 = 0x10;
/// `SET_SKELETON` op.
pub const OP_SET_SKELETON: u16 = 0x11;
/// `SET_TRACKING` op.
pub const OP_SET_TRACKING: u16 = 0x12;
/// `SET_POS` op.
pub const OP_SET_POS: u16 = 0x13;
/// `PROJECTILE_SPAWN` op (WP-SIM-22).
pub const OP_PROJECTILE_SPAWN: u16 = 0x14;
/// `PROJECTILE_END` op (WP-SIM-22).
pub const OP_PROJECTILE_END: u16 = 0x15;
/// `CTRL_ADD` op (WP-SIM-24).
pub const OP_CTRL_ADD: u16 = 0x16;
/// `CTRL_STATE` op (WP-SIM-24).
pub const OP_CTRL_STATE: u16 = 0x17;
/// `CTRL_REMOVE` op (WP-SIM-24).
pub const OP_CTRL_REMOVE: u16 = 0x18;
/// `LOCAL_CMD` op.
pub const OP_LOCAL_CMD: u16 = 0x19;
/// `LOCAL_DEST_POS` op.
pub const OP_LOCAL_DEST_POS: u16 = 0x1A;
/// `LOCAL_CLEAR` op.
pub const OP_LOCAL_CLEAR: u16 = 0x1B;
/// `LOCAL_FACE` op.
pub const OP_LOCAL_FACE: u16 = 0x1C;
/// `DECOR_ADD` op (WP-SIM-24).
pub const OP_DECOR_ADD: u16 = 0x1D;
/// `DECOR_REMOVE` op (WP-SIM-24).
pub const OP_DECOR_REMOVE: u16 = 0x1E;
/// `DECOR_VISIBLE` op (WP-SIM-24).
pub const OP_DECOR_VISIBLE: u16 = 0x1F;

/// `DAMAGE_FX.has_tail` bit 0: the legacy `AF_FLYING` tail (`def_pos`, `def_angle`) is valid.
pub const DAMAGE_FX_TAIL: u8 = 1;
/// `DAMAGE_FX.has_tail` bit 1: `pad[0..2]` holds the attacker's yaw (`Damage.attacker_angle`, Plan/31 Q10) as a
/// `u16` in 1/65536 of a turn ([`yaw_to_q16`]); the byte layout of the record is unchanged.
pub const DAMAGE_FX_ATTACKER_ANGLE: u8 = 2;

/// The `u16` (1/65536 turn) of a yaw in degrees: exact for a yaw produced by `set_angle_quantized`.
pub fn yaw_to_q16(deg: f32) -> u16 {
    (dtoi_x86(f64::from(deg) * (65536.0_f64 / 360.0_f64)) & 0xFFFF) as u16 // BOUNDS: masked
}

/// The yaw in degrees of a 1/65536-turn value (the inverse of [`yaw_to_q16`] on the quantised domain).
pub fn yaw_from_q16(q: u16) -> f32 {
    ((360.0_f64 / 65536.0_f64) * f64::from(q)) as f32
}

/// Number of 32-bit words of the `SET_PARAMS` parameter block ([`PARAMS_LAYOUT`]).
pub const PARAMS_WORDS: u16 = 54;

const U8: Ty = Ty::U8;
const U16: Ty = Ty::U16;
const U32: Ty = Ty::U32;
const I32: Ty = Ty::I32;
const U64: Ty = Ty::U64;
const I64: Ty = Ty::I64;
const F32: Ty = Ty::F32;

/// Every op of Plan/31 §17.4 with its packed payload layout, ordered by op number.
pub static OPS: [OpDesc; 31] = [
    OpDesc {
        code: OP_ADD_MOVER,
        name: "ADD_MOVER",
        fields: &[
            f("id", U32),
            f("kind", U8),
            f("is_active", U8),
            f("pad", U16),
            f("mover_index", U32),
            f("model_type", U32),
            f("model_index", U32),
            f("skel_id", U32),
            fa("pos", F32, 3),
            f("angle", F32),
            f("angle_x", F32),
            f("state", U32),
            f("state_flags", U32),
            f("motion", U32),
            f("scale", F32),
        ],
        framed: false,
    },
    OpDesc {
        code: OP_REMOVE_MOVER,
        name: "REMOVE_MOVER",
        fields: &[f("id", U32)],
        framed: false,
    },
    OpDesc {
        code: OP_SET_PARAMS,
        name: "SET_PARAMS",
        fields: &[f("id", U32), fa("params", U32, PARAMS_WORDS)],
        framed: false,
    },
    OpDesc {
        code: OP_STATE,
        name: "STATE",
        fields: &[
            f("id", U32),
            f("frame", U64),
            f("reason", U8),
            f("has_fly", U8),
            f("pad", U16),
            fa("pos", F32, 3),
            fa("delta", F32, 3),
            f("angle", F32),
            f("state", U32),
            f("flags", U32),
            f("motion", U32),
            f("motion_ex", I32),
            f("loop", I32),
            f("option", U32),
            f("motion_frame", F32),
            f("angle_x", F32),
            f("acc_power", F32),
            f("turn_angle", F32),
        ],
        framed: true,
    },
    OpDesc {
        code: OP_INTENT,
        name: "INTENT",
        fields: &[
            f("id", U32),
            f("frame", U64),
            f("kind", U8),
            fa("pad", U8, 3),
            fa("stamp_pos", F32, 3),
            f("stamp_angle", F32),
            f("stamp_state", U32),
            f("stamp_flags", U32),
            f("stamp_angle_x", F32),
            fa("args", F32, 6),
        ],
        framed: true,
    },
    OpDesc {
        code: OP_KEYFRAME,
        name: "KEYFRAME",
        fields: &[
            f("id", U32),
            f("frame", U64),
            fa("pos", F32, 3),
            f("angle", F32),
            f("state", U32),
        ],
        framed: true,
    },
    OpDesc {
        code: OP_ACT,
        name: "ACT",
        fields: &[
            f("id", U32),
            f("frame", U64),
            f("msg", U32),
            fa("p", I32, 5),
            f("queue_if_busy", U8),
        ],
        framed: true,
    },
    OpDesc {
        code: OP_DAMAGE_FX,
        name: "DAMAGE_FX",
        fields: &[
            f("defender", U32),
            f("frame", U64),
            f("atk_flags", U32),
            f("attacker", U32),
            f("has_tail", U8),
            fa("pad", U8, 3),
            fa("def_pos", F32, 3),
            f("def_angle", F32),
        ],
        framed: true,
    },
    OpDesc {
        code: OP_PUSH_POWER,
        name: "PUSH_POWER",
        fields: &[
            f("id", U32),
            f("frame", U64),
            f("angle", F32),
            f("push_angle", F32),
            f("power", F32),
        ],
        framed: true,
    },
    OpDesc {
        code: OP_DIE,
        name: "DIE",
        fields: &[
            f("id", U32),
            f("frame", U64),
            f("cause_msg", U32),
            f("attacker", U32),
        ],
        framed: true,
    },
    OpDesc {
        code: OP_MOTION_ARRIVE,
        name: "MOTION_ARRIVE",
        fields: &[f("id", U32), f("obj_msg", U32)],
        framed: false,
    },
    OpDesc {
        code: OP_SET_FUEL,
        name: "SET_FUEL",
        fields: &[f("id", U32), f("fuel", I32), f("acc_fuel", I32)],
        framed: false,
    },
    OpDesc {
        code: OP_SET_ACTIVE,
        name: "SET_ACTIVE",
        fields: &[f("id", U32)],
        framed: false,
    },
    OpDesc {
        code: OP_SET_CLOCK,
        name: "SET_CLOCK",
        fields: &[f("offset", I64)],
        framed: false,
    },
    OpDesc {
        code: OP_SET_VISIBLE,
        name: "SET_VISIBLE",
        fields: &[f("id", U32), f("visible", U8), f("want_attach", U8)],
        framed: false,
    },
    OpDesc {
        code: OP_SET_SKILL,
        name: "SET_SKILL",
        fields: &[
            f("id", U32),
            f("skill", U32),
            f("level", U32),
            f("target", U32),
            f("skill_count", I32),
            f("act", U32),
        ],
        framed: false,
    },
    OpDesc {
        code: OP_SET_SKELETON,
        name: "SET_SKELETON",
        fields: &[
            f("id", U32),
            f("skel_id", U32),
            f("model_type", U32),
            f("model_index", U32),
        ],
        framed: false,
    },
    OpDesc {
        code: OP_SET_TRACKING,
        name: "SET_TRACKING",
        fields: &[f("target", U32)],
        framed: false,
    },
    OpDesc {
        code: OP_SET_POS,
        name: "SET_POS",
        fields: &[f("id", U32), fa("pos", F32, 3)],
        framed: false,
    },
    OpDesc {
        code: OP_PROJECTILE_SPAWN,
        name: "PROJECTILE_SPAWN",
        fields: &[
            f("projectile_id", U32),
            f("kind", U8),
            fa("pad", U8, 3),
            f("attacker", U32),
            f("target", U32),
            f("frame", U64),
            fa("origin", F32, 3),
            fa("dest", F32, 3),
            f("speed", F32),
            f("shot_index", U32),
        ],
        framed: true,
    },
    OpDesc {
        code: OP_PROJECTILE_END,
        name: "PROJECTILE_END",
        fields: &[
            f("projectile_id", U32),
            f("hit", U8),
            fa("pad", U8, 3),
            fa("pos", F32, 3),
        ],
        framed: false,
    },
    OpDesc {
        code: OP_CTRL_ADD,
        name: "CTRL_ADD",
        fields: &[
            f("id", U32),
            f("model_key", U32),
            fa("world", F32, 12),
            fa("aabb", F32, 6),
            f("seq", U32),
        ],
        framed: false,
    },
    OpDesc {
        code: OP_CTRL_STATE,
        name: "CTRL_STATE",
        fields: &[
            f("id", U32),
            f("frame", U64),
            f("anim_frame", F32),
            f("loop", U32),
        ],
        framed: true,
    },
    OpDesc {
        code: OP_CTRL_REMOVE,
        name: "CTRL_REMOVE",
        fields: &[f("id", U32)],
        framed: false,
    },
    OpDesc {
        code: OP_LOCAL_CMD,
        name: "LOCAL_CMD",
        fields: &[
            f("act", U32),
            f("target", U32),
            f("range", F32),
            f("ar", U32),
            fa("p", I32, 3),
            f("flags", U8),
            fa("pad", U8, 3),
        ],
        framed: false,
    },
    OpDesc {
        code: OP_LOCAL_DEST_POS,
        name: "LOCAL_DEST_POS",
        fields: &[fa("pos", F32, 3), f("forward", U8), fa("pad", U8, 3)],
        framed: false,
    },
    OpDesc {
        code: OP_LOCAL_CLEAR,
        name: "LOCAL_CLEAR",
        fields: &[f("what", U8), fa("pad", U8, 3)],
        framed: false,
    },
    OpDesc {
        code: OP_LOCAL_FACE,
        name: "LOCAL_FACE",
        fields: &[f("angle", F32)],
        framed: false,
    },
    OpDesc {
        code: OP_DECOR_ADD,
        name: "DECOR_ADD",
        fields: &[
            f("id", U32),
            f("kind", U8),
            f("flags", U8),
            f("attach", U8),
            f("pad", U8),
            f("model_type", U32),
            f("model_index", U32),
            f("clip_id", U32),
            f("skel_id", U32),
            f("parent_mover", U32),
            f("start_frame", F32),
            fa("local", F32, 12),
        ],
        framed: false,
    },
    OpDesc {
        code: OP_DECOR_REMOVE,
        name: "DECOR_REMOVE",
        fields: &[f("id", U32)],
        framed: false,
    },
    OpDesc {
        code: OP_DECOR_VISIBLE,
        name: "DECOR_VISIBLE",
        fields: &[f("id", U32), f("visible", U8), fa("pad", U8, 3)],
        framed: false,
    },
];

/// The descriptor of op `code`.
pub fn op_desc(code: u16) -> Option<&'static OpDesc> {
    OPS.iter().find(|o| o.code == code)
}

/// `ADD_MOVER.kind`: player.
pub const KIND_PLAYER: u8 = 0;
/// `ADD_MOVER.kind`: monster.
pub const KIND_MONSTER: u8 = 1;
/// `ADD_MOVER.kind`: NPC.
pub const KIND_NPC: u8 = 2;
/// `ADD_MOVER.kind`: pet.
pub const KIND_PET: u8 = 3;
/// `ADD_MOVER.kind`: preview mover (Plan/31 §15.1).
pub const KIND_PREVIEW: u8 = 4;

/// `INTENT.kind`: `DestPosNtf` (`args` x, y, z, forward).
pub const INTENT_DEST_POS: u8 = 1;
/// `INTENT.kind`: `SetDestObjNtf` (`args` target bits, range).
pub const INTENT_DEST_OBJ: u8 = 2;
/// `INTENT.kind`: `DestAngle` (`args` angle, left).
pub const INTENT_DEST_ANGLE: u8 = 3;
/// `INTENT.kind`: `SetMovePattern` (`args` pattern bits, x, y, z, angle, angle_x).
pub const INTENT_MOVE_PATTERN: u8 = 4;
/// `INTENT.kind`: `SetSpeedFactor` (`args` factor).
pub const INTENT_SPEED_FACTOR: u8 = 5;
/// `INTENT.kind`: `SetPos` (`args` x, y, z).
pub const INTENT_SET_POS: u8 = 6;

/// `MoveReason` of the `STATE` record (`proto` `MoveReason`).
pub mod reason {
    /// `MOVE_REASON_MOVED`.
    pub const MOVED: u8 = 1;
    /// `MOVE_REASON_BEHAVIOR`.
    pub const BEHAVIOR: u8 = 2;
    /// `MOVE_REASON_CORR`: the receiver clears the destination.
    pub const CORR: u8 = 3;
    /// `MOVE_REASON_ANGLE`.
    pub const ANGLE: u8 = 4;
    /// `MOVE_REASON_PERIODIC`.
    pub const PERIODIC: u8 = 5;
}

/// `LOCAL_CMD.flags`: set the report bit `BEHAVIOR` before the destination change.
pub const LOCAL_CMD_BEHAVIOR_FIRST: u8 = 1;
/// `LOCAL_CLEAR.what`: clear the pending command.
pub const CLEAR_CMD: u8 = 1;
/// `LOCAL_CLEAR.what`: clear the destination position and object.
pub const CLEAR_DEST: u8 = 2;
/// `LOCAL_CLEAR.what`: clear the destination object.
pub const CLEAR_DEST_OBJ: u8 = 4;
/// `LOCAL_CLEAR.what`: end casting and clear the state.
pub const CLEAR_CASTING: u8 = 8;
/// `LOCAL_CLEAR.what`: clear `MVRF_SKILL`.
pub const CLEAR_SKILL_FLAG: u8 = 16;

// ---------------------------------------------------------------------------------------------------
// SimWorld::new configuration words and SimFlags bits
// ---------------------------------------------------------------------------------------------------

/// `SimFlags` bit: `features.better_movement`.
pub const FLAG_BETTER_MOVEMENT: u32 = 1;
/// `SimFlags` bit: `features.fix_ranger_roller`.
pub const FLAG_FIX_RANGER_ROLLER: u32 = 2;
/// `SimFlags` bit: `features.attack_range_code`.
pub const FLAG_ATTACK_RANGE_CODE: u32 = 4;
/// `SimFlags` bit: `features.speed_cast`.
pub const FLAG_SPEED_CAST: u32 = 8;
/// `SimFlags` bit: `features.improved_pets`.
pub const FLAG_IMPROVED_PETS: u32 = 16;

/// The `SimFlags` of the configuration words `[bits, hit_attr_rule]` (`SimWorld::new`).
#[allow(clippy::field_reassign_with_default)] // keys added to `SimFlags` later keep their defaults
pub fn sim_flags_from_words(bits: u32, hit_attr_rule: u32) -> SimFlags {
    let mut f = SimFlags::default();
    f.better_movement = bits & FLAG_BETTER_MOVEMENT != 0;
    f.fix_ranger_roller = bits & FLAG_FIX_RANGER_ROLLER != 0;
    f.attack_range_code = bits & FLAG_ATTACK_RANGE_CODE != 0;
    f.speed_cast = bits & FLAG_SPEED_CAST != 0;
    f.improved_pets = bits & FLAG_IMPROVED_PETS != 0;
    f.hit_attr_rule = if hit_attr_rule == 0 {
        HitAttrRule::Legacy
    } else {
        HitAttrRule::Window
    };
    f
}

/// The inverse of [`sim_flags_from_words`]: `[bits, hit_attr_rule]`.
pub fn sim_flags_to_words(f: &SimFlags) -> [u32; 2] {
    let mut bits = 0;
    if f.better_movement {
        bits |= FLAG_BETTER_MOVEMENT;
    }
    if f.fix_ranger_roller {
        bits |= FLAG_FIX_RANGER_ROLLER;
    }
    if f.attack_range_code {
        bits |= FLAG_ATTACK_RANGE_CODE;
    }
    if f.speed_cast {
        bits |= FLAG_SPEED_CAST;
    }
    if f.improved_pets {
        bits |= FLAG_IMPROVED_PETS;
    }
    [
        bits,
        match f.hit_attr_rule {
            HitAttrRule::Legacy => 0,
            HitAttrRule::Window => 1,
        },
    ]
}

// ---------------------------------------------------------------------------------------------------
// SET_PARAMS layout
// ---------------------------------------------------------------------------------------------------

/// How a `SET_PARAMS` word is read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PTy {
    /// Unsigned integer.
    U32,
    /// Signed integer.
    I32,
    /// `f32` bits.
    F32,
    /// `0` or `1`.
    Bool,
}

impl PTy {
    /// The name used in the generated TypeScript.
    pub const fn name(self) -> &'static str {
        match self {
            PTy::U32 => "u32",
            PTy::I32 => "i32",
            PTy::F32 => "f32",
            PTy::Bool => "bool",
        }
    }
}

/// One field of the `SET_PARAMS` parameter block.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ParamField {
    /// Field name.
    pub name: &'static str,
    /// Word index inside the block.
    pub word: u16,
    /// Number of words (16 for `bounds_entries`, otherwise 1).
    pub words: u16,
    /// Reading rule.
    pub ty: PTy,
}

const fn pf(name: &'static str, word: u16, ty: PTy) -> ParamField {
    ParamField {
        name,
        word,
        words: 1,
        ty,
    }
}

/// The `SET_PARAMS` parameter block (Plan/31 §17.4 op 0x03): [`PARAMS_WORDS`] 32-bit words after the mover
/// id, in this order. `kind` is the [`MoverKind`] (0 player, 1 monster, 2 NPC, 3 pet); `stance_flags` has
/// `TWO_HANDED 1, DUAL 2, MASTER_MODEL 4, PSY_JOB 8`; the bounds are `bounds_count` (1..=16) `FLMB` entry
/// indices in `bounds_entries` (unused slots 0): the wasm side computes the union, TS never does.
pub const PARAMS_LAYOUT: [ParamField; 39] = [
    pf("kind", 0, PTy::U32),
    pf("mover_index", 1, PTy::U32),
    pf("ai", 2, PTy::U32),
    pf("flying_npc", 3, PTy::Bool),
    pf("rank", 4, PTy::U32),
    pf("killable", 5, PTy::Bool),
    pf("base_speed", 6, PTy::F32),
    pf("speed_factor", 7, PTy::F32),
    pf("speed_adj_pct", 8, PTy::I32),
    pf("speed_chg_set", 9, PTy::Bool),
    pf("jumping", 10, PTy::I32),
    pf("chr_state", 11, PTy::U32),
    pf("mode", 12, PTy::U32),
    pf("base_motion_mode", 13, PTy::Bool),
    pf("attack_speed", 14, PTy::F32),
    pf("casting_ani_speed", 15, PTy::F32),
    pf("frame_factor", 16, PTy::F32),
    pf("hp_positive", 17, PTy::Bool),
    pf("level", 18, PTy::U32),
    pf("has_stance", 19, PTy::Bool),
    pf("stance_kind3", 20, PTy::U32),
    pf("stance_flags", 21, PTy::U32),
    pf("has_ride", 22, PTy::Bool),
    pf("ride_item", 23, PTy::U32),
    pf("ride_flight_speed", 24, PTy::F32),
    pf("ride_lr_angle", 25, PTy::F32),
    pf("ride_tb_angle", 26, PTy::F32),
    pf("ride_use_motion", 27, PTy::U32),
    pf("ride_f_fuel_re_max", 28, PTy::I32),
    pf("model_ot", 29, PTy::U32),
    pf("model_index", 30, PTy::U32),
    pf("bounds_count", 31, PTy::U32),
    ParamField {
        name: "bounds_entries",
        word: 32,
        words: 16,
        ty: PTy::U32,
    },
    pf("scale", 48, PTy::F32),
    pf("hawkeye_rate", 49, PTy::I32),
    pf("disguised", 50, PTy::Bool),
    pf("prop_flying", 51, PTy::U32),
    pf("is_pet", 52, PTy::Bool),
    pf("pet_owner", 53, PTy::U32),
];

/// `stance_flags` bit: `dwHanded == HD_TWO`.
pub const STANCE_TWO_HANDED: u32 = 1;
/// `stance_flags` bit: dual wielding.
pub const STANCE_DUAL: u32 = 2;
/// `stance_flags` bit: master model.
pub const STANCE_MASTER_MODEL: u32 = 4;
/// `stance_flags` bit: psykeeper job.
pub const STANCE_PSY_JOB: u32 = 8;

/// The largest number of `FLMB` entries a mover may union.
pub const MAX_BOUNDS_ENTRIES: usize = 16;

fn w_f32(v: f32) -> u32 {
    v.to_bits()
}

fn r_f32(w: u32) -> f32 {
    f32::from_bits(w)
}

/// Encodes `p` into the parameter block. `bounds_entries` are the `FLMB` entry indices (1..=16) the
/// mover's bounds are the union of; `p.bounds` itself is not carried.
pub fn params_to_words(
    p: &SimMoverParams,
    bounds_entries: &[u32],
) -> Result<[u32; PARAMS_WORDS as usize], SimError> {
    if bounds_entries.is_empty() || bounds_entries.len() > MAX_BOUNDS_ENTRIES {
        return Err(SimError::BadCommand("bounds_count must be 1..=16"));
    }
    let mut w = [0u32; PARAMS_WORDS as usize];
    let mut set = |i: usize, v: u32| {
        if let Some(s) = w.get_mut(i) {
            *s = v;
        }
    };
    set(
        0,
        match p.kind {
            MoverKind::Player => 0,
            MoverKind::Monster => 1,
            MoverKind::Npc => 2,
            MoverKind::Pet => 3,
        },
    );
    set(1, p.mover_index);
    set(2, p.ai);
    set(3, u32::from(p.flying_npc));
    set(4, p.rank);
    set(5, u32::from(p.killable));
    set(6, w_f32(p.base_speed));
    set(7, w_f32(p.speed_factor));
    set(8, p.speed_adj_pct as u32); // BOUNDS: bit pattern of an i32
    set(9, u32::from(p.speed_chg_set));
    set(10, p.jumping as u32); // BOUNDS: bit pattern of an i32
    set(11, p.chr_state);
    set(12, p.mode);
    set(13, u32::from(p.base_motion_mode));
    set(14, w_f32(p.attack_speed));
    set(15, w_f32(p.casting_ani_speed));
    set(16, w_f32(p.frame_factor));
    set(17, u32::from(p.hp_positive));
    set(18, p.level);
    if let Some(s) = p.stance {
        set(19, 1);
        set(20, s.kind3);
        set(
            21,
            (if s.two_handed { STANCE_TWO_HANDED } else { 0 })
                | (if s.dual { STANCE_DUAL } else { 0 })
                | (if s.master_model {
                    STANCE_MASTER_MODEL
                } else {
                    0
                })
                | (if s.psy_job { STANCE_PSY_JOB } else { 0 }),
        );
    }
    if let Some(r) = p.ride {
        set(22, 1);
        set(23, r.item);
        set(24, w_f32(r.flight_speed));
        set(25, w_f32(r.lr_angle));
        set(26, w_f32(r.tb_angle));
        set(27, r.use_motion);
        set(28, r.f_fuel_re_max as u32); // BOUNDS: bit pattern of an i32
    }
    set(29, p.model_key.ot);
    set(30, p.model_key.index);
    set(31, bounds_entries.len() as u32); // BOUNDS: at most 16
    for (i, e) in bounds_entries.iter().enumerate() {
        set(32 + i, *e);
    }
    set(48, w_f32(p.scale));
    set(49, p.hawkeye_rate as u32); // BOUNDS: bit pattern of an i32
    set(50, u32::from(p.disguised));
    set(51, p.prop_flying);
    set(52, u32::from(p.is_pet));
    set(53, p.pet_owner.0);
    Ok(w)
}

/// A decoded parameter block: the params (with an empty-box placeholder for `bounds`) and the `FLMB` entry
/// indices the caller resolves into `params.bounds`.
#[derive(Clone, Debug)]
pub struct DecodedParams {
    /// The parameters; `bounds` still holds the default box.
    pub params: SimMoverParams,
    /// The `FLMB` entry indices (`1..=16`).
    pub bounds_entries: Vec<u32>,
}

/// Decodes and validates a parameter block (`words.len() == PARAMS_WORDS`): unknown `kind`, non-finite
/// floats, booleans other than 0/1 and a `bounds_count` outside `1..=16` are errors.
pub fn params_from_words(words: &[u32]) -> Result<DecodedParams, SimError> {
    if words.len() != usize::from(PARAMS_WORDS) {
        return Err(SimError::BadCommand("SET_PARAMS block has the wrong size"));
    }
    let g = |i: usize| words.get(i).copied().unwrap_or(0);
    let boolean = |i: usize| -> Result<bool, SimError> {
        match g(i) {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(SimError::BadCommand("SET_PARAMS boolean is not 0 or 1")),
        }
    };
    let float = |i: usize| -> Result<f32, SimError> {
        let v = r_f32(g(i));
        if v.is_finite() {
            Ok(v)
        } else {
            Err(SimError::BadCommand("SET_PARAMS float is not finite"))
        }
    };
    let kind = match g(0) {
        0 => MoverKind::Player,
        1 => MoverKind::Monster,
        2 => MoverKind::Npc,
        3 => MoverKind::Pet,
        _ => return Err(SimError::BadCommand("SET_PARAMS kind is unknown")),
    };
    let mut p = SimMoverParams::default_for(kind);
    p.mover_index = g(1);
    p.ai = g(2);
    p.flying_npc = boolean(3)?;
    p.rank = g(4);
    p.killable = boolean(5)?;
    p.base_speed = float(6)?;
    p.speed_factor = float(7)?;
    p.speed_adj_pct = g(8) as i32; // BOUNDS: bit pattern of an i32
    p.speed_chg_set = boolean(9)?;
    p.jumping = g(10) as i32; // BOUNDS: bit pattern of an i32
    p.chr_state = g(11);
    p.mode = g(12);
    p.base_motion_mode = boolean(13)?;
    p.attack_speed = float(14)?;
    p.casting_ani_speed = float(15)?;
    p.frame_factor = float(16)?;
    p.hp_positive = boolean(17)?;
    p.level = g(18);
    p.stance = if boolean(19)? {
        let fl = g(21);
        Some(WeaponStance {
            kind3: g(20),
            two_handed: fl & STANCE_TWO_HANDED != 0,
            dual: fl & STANCE_DUAL != 0,
            master_model: fl & STANCE_MASTER_MODEL != 0,
            psy_job: fl & STANCE_PSY_JOB != 0,
        })
    } else {
        None
    };
    p.ride = if boolean(22)? {
        Some(RideParams {
            item: g(23),
            flight_speed: float(24)?,
            lr_angle: float(25)?,
            tb_angle: float(26)?,
            use_motion: g(27),
            f_fuel_re_max: g(28) as i32, // BOUNDS: bit pattern of an i32
        })
    } else {
        None
    };
    p.model_key = crate::anim::motion_table::ModelKey {
        ot: g(29),
        index: g(30),
    };
    let count = g(31) as usize; // BOUNDS: u32 -> usize on every target (checked below)
    if count == 0 || count > MAX_BOUNDS_ENTRIES {
        return Err(SimError::BadCommand("bounds_count must be 1..=16"));
    }
    let bounds_entries: Vec<u32> = (0..count).map(|i| g(32 + i)).collect();
    p.scale = float(48)?;
    p.hawkeye_rate = g(49) as i32; // BOUNDS: bit pattern of an i32
    p.disguised = boolean(50)?;
    p.prop_flying = g(51);
    p.is_pet = boolean(52)?;
    p.pet_owner = flyff_common::ObjId(g(53));
    Ok(DecodedParams {
        params: p,
        bounds_entries,
    })
}

// ---------------------------------------------------------------------------------------------------
// events
// ---------------------------------------------------------------------------------------------------

/// One exported event kind (`take_events`, Plan/31 §17.6).
#[derive(Clone, Copy, Debug)]
pub struct EventKind {
    /// The kind number (stable, append-only).
    pub id: u32,
    /// The `SimEvent` name.
    pub name: &'static str,
    /// Meaning of the payload words `a..e` (unused words are not listed).
    pub payload: &'static [&'static str],
    /// Cosmetic events are dropped first when a frame call overflows the buffer.
    pub cosmetic: bool,
}

/// Capacity of the event buffer per `step`/`take_events` cycle (Plan/31 §17.6, B3).
pub const EVENT_CAPACITY: usize = 256;
/// Words of one event record: `[kind, obj_id, frame_lo, a, b, c, d, e]`.
pub const EVENT_WORDS: usize = 8;
/// Words of the event buffer header: `[count, dropped]`.
pub const EVENT_HEADER_WORDS: usize = 2;

const fn ek(
    id: u32,
    name: &'static str,
    payload: &'static [&'static str],
    cosmetic: bool,
) -> EventKind {
    EventKind {
        id,
        name,
        payload,
        cosmetic,
    }
}

/// The event kinds of the "JS export: yes" rows of Plan/31 §16.4 (floats are bit-cast, `obj_id` is the
/// mover id unless noted).
pub static EVENT_KINDS: [EventKind; 28] = [
    ek(1, "AttrSound", &["sound", "x", "y", "z", "flags"], true),
    ek(2, "Footstep", &["zero", "x", "y", "z"], true),
    ek(3, "Landed", &[], true),
    ek(4, "JumpCompleted", &[], false),
    ek(5, "AttrQuake", &[], true),
    ek(6, "CameraQuake", &["frames", "scale"], true),
    ek(7, "DustFx", &[], true),
    ek(8, "DriftFx", &[], true),
    ek(9, "SwordTrail", &["hit_slow"], true),
    ek(10, "SwordTrailClear", &[], true),
    ek(11, "BossChargeFx", &["state"], true),
    ek(12, "YoyoThrowFx", &[], true),
    ek(13, "CastSpark", &[], true),
    ek(14, "SatDown", &[], true),
    ek(15, "RideChanged", &["has_item", "item"], false),
    ek(16, "RideMissing", &[], false),
    ek(17, "RideFrameReset", &[], true),
    ek(
        18,
        "AttrHit",
        &["hit_kind", "state", "target", "hit_index"],
        false,
    ),
    ek(19, "LocalAttack", &["msg", "target", "flags"], false),
    ek(20, "CmdArrived", &["cmd", "target"], false),
    ek(21, "CmdAborted", &[], false),
    ek(22, "Notice", &["tid"], false),
    ek(23, "CastCancel", &[], false),
    ek(24, "LmbRelease", &[], false),
    ek(25, "DestPosChosen", &["forward", "x", "y", "z"], false),
    ek(26, "ChunkMissing", &["lx", "lz"], false),
    ek(27, "ProjectileHit", &["id", "x", "y", "z"], false),
    ek(28, "ProjectileExpired", &["id"], false),
];

/// The kind number of the event named `name`.
pub fn event_kind(name: &str) -> Option<u32> {
    EVENT_KINDS.iter().find(|k| k.name == name).map(|k| k.id)
}

/// `AttrSound` payload word `e`: bit 0 `positional`.
pub const SOUND_POSITIONAL: u32 = 1;
/// `AttrSound` payload word `e`: bit 1, `obj_id` is a decor id (WP-SIM-24).
pub const SOUND_DECOR: u32 = 2;

/// Constants of the decor and ctrl commands (`DECOR_ADD`, `CTRL_STATE`; Plan/31 §13.7, §17.4; WP-SIM-24).
pub mod decor {
    /// `DECOR_ADD.flags`: the clock loops (else `1PLAY`).
    pub const LOOP: u8 = 1;
    /// `DECOR_ADD.flags`: the pose is evaluated and written to the render view.
    pub const VISIBLE: u8 = 2;
    /// `DECOR_ADD.flags`: the clock emits `AttrSound` events.
    pub const SOUNDS: u8 = 4;
    /// `DECOR_ADD.flags`: the sounds are positional.
    pub const SOUND_POSITIONAL: u8 = 8;
    /// Mask of the defined `DECOR_ADD.flags` bits.
    pub const FLAGS_MASK: u8 = 15;
    /// `DECOR_ADD.kind`: a static animated object.
    pub const KIND_STATIC: u8 = 0;
    /// `DECOR_ADD.kind`: attached to a mover.
    pub const KIND_ATTACHED: u8 = 1;
    /// Render-view kind of a ctrl entry (created by `CTRL_ADD`, never by `DECOR_ADD`).
    pub const KIND_CTRL: u8 = 2;
    /// `DECOR_ADD.clip_id`: the built-in `.o3d` clip of the model (`MotionTable::builtin`).
    pub const CLIP_BUILTIN: u32 = 0xFFFF_FFFF;
    /// `CTRL_STATE.loop` bit 0: the clock loops (else it plays on to the last frame and holds, `CONT`).
    pub const STATE_LOOP: u32 = 1;
    /// `CTRL_STATE.loop` bit 1: `CK3_HALF` (pause at `max_frame / 2` for 20 s).
    pub const STATE_HALF: u32 = 2;
    /// `CTRL_STATE.loop` bit 2: `CK2_KEEP` (re-arm at the end of the animation).
    pub const STATE_KEEP: u32 = 4;
    /// Mask of the defined `CTRL_STATE.loop` bits.
    pub const STATE_MASK: u32 = 7;
}

// ---------------------------------------------------------------------------------------------------
// render view, report, input
// ---------------------------------------------------------------------------------------------------

/// The render view layout (Plan/31 §17.7). All offsets are in `f32` units from the header pointer.
pub mod view {
    /// `'SIMV'`.
    pub const MAGIC: u32 = 0x5349_4D56;
    /// Layout version (`header[1]`).
    pub const VERSION: u32 = 1;
    /// Header size in words.
    pub const HEADER_WORDS: usize = 32;
    /// Header slot: magic.
    pub const H_MAGIC: usize = 0;
    /// Header slot: version.
    pub const H_VERSION: usize = 1;
    /// Header slot: frame, low word.
    pub const H_FRAME_LO: usize = 2;
    /// Header slot: frame, high word.
    pub const H_FRAME_HI: usize = 3;
    /// Header slot: mover record count.
    pub const H_MOVER_COUNT: usize = 4;
    /// Header slot: mover stride (32).
    pub const H_MOVER_STRIDE: usize = 5;
    /// Header slot: offset of the mover area.
    pub const H_MOVERS_OFF: usize = 6;
    /// Header slot: offset of the palette area (multiple of 4).
    pub const H_PALETTE_OFF: usize = 7;
    /// Header slot: floats of palette data.
    pub const H_PALETTE_FLOATS: usize = 8;
    /// Header slot: offset of the attachment area.
    pub const H_ATTACH_OFF: usize = 9;
    /// Header slot: attachment stride (88).
    pub const H_ATTACH_STRIDE: usize = 10;
    /// Header slot: projectile record count.
    pub const H_PROJ_COUNT: usize = 11;
    /// Header slot: projectile stride (16).
    pub const H_PROJ_STRIDE: usize = 12;
    /// Header slot: offset of the projectile area.
    pub const H_PROJ_OFF: usize = 13;
    /// Header slot: offset of the report area.
    pub const H_REPORT_OFF: usize = 14;
    /// Header slot: report floats (32).
    pub const H_REPORT_FLOATS: usize = 15;
    /// Header slot: decor record count.
    pub const H_DECOR_COUNT: usize = 16;
    /// Header slot: decor stride (20).
    pub const H_DECOR_STRIDE: usize = 17;
    /// Header slot: offset of the decor area.
    pub const H_DECOR_OFF: usize = 18;
    /// Header slot: palette texture rows (`ceil(palette_floats / 6144)`).
    pub const H_PALETTE_ROWS: usize = 19;

    /// Floats per mover record.
    pub const MOVER_STRIDE: usize = 32;
    /// Floats per attachment record.
    pub const ATTACH_STRIDE: usize = 88;
    /// Floats per projectile record.
    pub const PROJ_STRIDE: usize = 16;
    /// Floats per decor record.
    pub const DECOR_STRIDE: usize = 20;
    /// Floats per report.
    pub const REPORT_FLOATS: usize = 32;
    /// Floats per palette texture row (512 matrices of 12 floats).
    pub const PALETTE_ROW_FLOATS: usize = 6144;
    /// "None" for `palette_index` and `attach_index`.
    pub const NO_INDEX: u32 = 0xFFFF_FFFF;

    /// Mover record slot: object id.
    pub const M_ID: usize = 0;
    /// Mover record slot: flags.
    pub const M_FLAGS: usize = 1;
    /// Mover record slot: first float of the world matrix (3 vec4 rows = 12 floats).
    pub const M_WORLD: usize = 2;
    /// Mover record slot: first matrix index in the palette area.
    pub const M_PALETTE_INDEX: usize = 14;
    /// Mover record slot: number of palette matrices.
    pub const M_BONE_COUNT: usize = 15;
    /// Mover record slot: attachment record index.
    pub const M_ATTACH_INDEX: usize = 16;
    /// Mover record slot: un-offset motion id.
    pub const M_MOTION: usize = 17;
    /// Mover record slot: the resolved (stance-offset) motion id.
    pub const M_MOTION_RESOLVED: usize = 18;
    /// Mover record slot: `frame_cur`.
    pub const M_FRAME_CUR: usize = 19;
    /// Mover record slot: state word.
    pub const M_STATE: usize = 20;
    /// Mover record slot: state flags.
    pub const M_STATE_FLAGS: usize = 21;
    /// Mover record slot: first float of the logical position (LH, 3 floats).
    pub const M_POS: usize = 22;
    /// Mover record slot: yaw in degrees (LH).
    pub const M_ANGLE: usize = 25;
    /// Mover record slot: visual roll in degrees.
    pub const M_ROLL: usize = 26;
    /// Mover record slot: `GetRadius()`.
    pub const M_RADIUS: usize = 27;
    /// Mover record slot: the clip id of the current motion (`0xFFFFFFFF` = none).
    pub const M_CLIP_ID: usize = 28;
    /// Mover record slot: `act.current_height`.
    pub const M_CURRENT_HEIGHT: usize = 29;

    /// Mover flag: the renderer asked for the mover (`SET_VISIBLE`).
    pub const F_VISIBLE: u32 = 1;
    /// Mover flag: the palette holds an evaluated pose.
    pub const F_HAS_POSE: u32 = 2;
    /// Mover flag: flying.
    pub const F_FLY: u32 = 4;
    /// Mover flag: dead.
    pub const F_DEAD: u32 = 8;
    /// Mover flag: the active mover.
    pub const F_ACTIVE: u32 = 16;
    /// Mover flag: the land under the mover is still pending.
    pub const F_PENDING_LAND: u32 = 32;
    /// Mover flag: an intent older than the fast-forward cap is waiting for a keyframe.
    pub const F_LAGGING: u32 = 64;
    /// Mover flag: the attachment record is valid.
    pub const F_HAS_ATTACH: u32 = 128;
    /// Mover flag: a preview mover.
    pub const F_PREVIEW: u32 = 256;

    /// Decor record slot: id (the decor id, or the ctrl's `ObjId` for ctrl entries).
    pub const D_ID: usize = 0;
    /// Decor record slot: flags.
    pub const D_FLAGS: usize = 1;
    /// Decor record slot: first matrix index in the palette area.
    pub const D_PALETTE_INDEX: usize = 2;
    /// Decor record slot: matrix count (nodes of a rigid entry, bones of a skinned one).
    pub const D_MATRIX_COUNT: usize = 3;
    /// Decor record slot: `frame_cur` of the entry's clock.
    pub const D_FRAME_CUR: usize = 4;
    /// Decor record slot: first float of the world matrix (3 vec4 rows = 12 floats).
    pub const D_WORLD: usize = 5;

    /// Decor flag: the renderer asked for the entry (`DECOR_VISIBLE` / `SET_VISIBLE`).
    pub const DF_VISIBLE: u32 = 1;
    /// Decor flag: the palette holds an evaluated pose.
    pub const DF_HAS_POSE: u32 = 2;
    /// Decor flag: skinned (palette = `inverse_tm * bone`), else rigid node matrices.
    pub const DF_SKINNED: u32 = 4;
    /// Decor flag: a ctrl entry (the id is an `ObjId`).
    pub const DF_CTRL: u32 = 8;
    /// Decor flag: the clock reached the end of a non-looping play.
    pub const DF_ENDED: u32 = 16;

    /// Projectile flag: alive.
    pub const P_ALIVE: u32 = 1;
    /// Projectile flag: hit.
    pub const P_HIT: u32 = 2;
    /// Projectile flag: expired.
    pub const P_EXPIRED: u32 = 4;

    /// The height rule of players: the render matrix is `0.07` above the position ("heel").
    pub const HEEL: f32 = 0.07;
}

/// The local report (Plan/31 §15.3, §17.7 `report` area).
pub mod report {
    /// Report bit: a key-driven state change.
    pub const MOVED: u32 = 1;
    /// Report bit: a discrete action (jump, walk toggle).
    pub const BEHAVIOR: u32 = 2;
    /// Report bit: a collision/drop correction.
    pub const CORR: u32 = 4;
    /// Report bit: a throttled angle change.
    pub const ANGLE: u32 = 8;
    /// Report bit: the 30-frame refresh.
    pub const PERIODIC: u32 = 16;
    /// Report bit: click-to-move destination chosen.
    pub const DEST_POS: u32 = 32;
    /// Report bit: follow target chosen.
    pub const DEST_OBJ: u32 = 64;
    /// Report bit: a collision kept the destination.
    pub const KEPT_DEST: u32 = 128;

    /// Slot: bits (as `u32`).
    pub const S_BITS: usize = 0;
    /// Slot: `client_frame` of the last stepped frame (low 32 bits).
    pub const S_CLIENT_FRAME: usize = 1;
    /// Slot: first float of `pos`.
    pub const S_POS: usize = 2;
    /// Slot: first float of `delta`.
    pub const S_DELTA: usize = 5;
    /// Slot: yaw.
    pub const S_ANGLE: usize = 8;
    /// Slot: state word.
    pub const S_STATE: usize = 9;
    /// Slot: state flags.
    pub const S_STATE_FLAGS: usize = 10;
    /// Slot: un-offset motion id.
    pub const S_MOTION: usize = 11;
    /// Slot: `motion_ex` (`i32` bits).
    pub const S_MOTION_EX: usize = 12;
    /// Slot: loop mode (`i32` bits).
    pub const S_LOOP: usize = 13;
    /// Slot: motion option.
    pub const S_MOTION_OPTION: usize = 14;
    /// Slot: motion frame.
    pub const S_MOTION_FRAME: usize = 15;
    /// Slot: pitch.
    pub const S_ANGLE_X: usize = 16;
    /// Slot: flight acceleration power.
    pub const S_ACC_POWER: usize = 17;
    /// Slot: flight turn angle.
    pub const S_TURN_ANGLE: usize = 18;
    /// Slot: first float of the chosen destination position.
    pub const S_DEST_POS: usize = 19;
    /// Slot: `forward` of the destination (1.0 / 0.0).
    pub const S_DEST_FORWARD: usize = 22;
    /// Slot: destination object id (`u32` bits).
    pub const S_DEST_OBJ: usize = 23;
    /// Slot: destination object range.
    pub const S_DEST_RANGE: usize = 24;
}

/// The local input record `local_input(&[i32])` (Plan/31 §17.5).
pub mod input {
    /// Button: forward (W).
    pub const UP: u32 = 1;
    /// Button: backward (S).
    pub const DOWN: u32 = 2;
    /// Button: turn left (A).
    pub const LEFT: u32 = 4;
    /// Button: turn right (D).
    pub const RIGHT: u32 = 8;
    /// Button: jump.
    pub const JUMP: u32 = 16;
    /// Edge: walk/run toggle.
    pub const WALK_TOGGLE: u32 = 32;
    /// Edge: flight acceleration toggle.
    pub const ACC_TOGGLE: u32 = 64;
    /// Edge: turbo toggle.
    pub const TURBO_TOGGLE: u32 = 128;
    /// Left mouse button pressed (decided by doc 43; the sim only latches it).
    pub const LMB_DOWN: u32 = 256;
    /// Left mouse button held (combo chaining).
    pub const LMB_HELD: u32 = 512;
    /// Auto attack on.
    pub const AUTO_ATTACK: u32 = 1024;
    /// No skill is queued on the taskbar.
    pub const NEXT_SKILL_NONE: u32 = 2048;
    /// Control blocked (dead, vendor, ...).
    pub const BLOCKED: u32 = 4096;
    /// The mouse moved (flight steering).
    pub const MOUSE_MOVED: u32 = 8192;
    /// Bits consumed by the next stepped frame.
    pub const EDGE_MASK: u32 = WALK_TOGGLE | ACC_TOGGLE | TURBO_TOGGLE;
    /// Slot: buttons.
    pub const S_BUTTONS: usize = 0;
    /// Slot: mouse dx.
    pub const S_MOUSE_DX: usize = 1;
    /// Slot: mouse dy.
    pub const S_MOUSE_DY: usize = 2;
    /// Slot: level.
    pub const S_LEVEL: usize = 3;
    /// Slot: `req_flags`.
    pub const S_REQ_FLAGS: usize = 4;
}

/// `ray_cast` / `target_check` / query constants shared with TS.
pub mod query {
    /// `target_check` bit: in range.
    pub const IN_RANGE: u32 = 1;
    /// `target_check` bit: inside the cone.
    pub const IN_CONE: u32 = 2;
    /// `target_check` bit: back attack allowed.
    pub const AS_BACK_OK: u32 = 4;
}

// ---------------------------------------------------------------------------------------------------
// the reference command encoder
// ---------------------------------------------------------------------------------------------------

/// The reference encoder of the command buffer: tests, vectors and the recorder build streams with it, and
/// the TS golden of `SimCmdWriter` compares against its bytes.
#[derive(Clone, Debug, Default)]
pub struct CmdWriter {
    buf: Vec<u8>,
    records: u32,
}

/// A payload under construction (fields are appended in layout order).
#[derive(Debug, Default)]
pub struct Payload(Vec<u8>);

impl Payload {
    /// Appends a `u8`.
    pub fn u8(mut self, v: u8) -> Payload {
        self.0.push(v);
        self
    }
    /// Appends `n` zero bytes.
    pub fn pad(mut self, n: usize) -> Payload {
        self.0.resize(self.0.len() + n, 0);
        self
    }
    /// Appends a `u16`.
    pub fn u16(mut self, v: u16) -> Payload {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    /// Appends a `u32`.
    pub fn u32(mut self, v: u32) -> Payload {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    /// Appends an `i32`.
    pub fn i32(mut self, v: i32) -> Payload {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    /// Appends a `u64` (two LE words, lo then hi).
    pub fn u64(mut self, v: u64) -> Payload {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    /// Appends an `i64`.
    pub fn i64(mut self, v: i64) -> Payload {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    /// Appends an `f32`.
    pub fn f32(mut self, v: f32) -> Payload {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    /// Appends three `f32`.
    pub fn v3(self, v: Vec3) -> Payload {
        self.f32(v.x).f32(v.y).f32(v.z)
    }
}

/// Starts a payload.
pub fn payload() -> Payload {
    Payload::default()
}

impl CmdWriter {
    /// An empty buffer.
    pub fn new() -> CmdWriter {
        CmdWriter::default()
    }

    /// The bytes so far.
    pub fn bytes(&self) -> &[u8] {
        &self.buf
    }

    /// Takes the bytes and resets the writer.
    pub fn take(&mut self) -> Vec<u8> {
        self.records = 0;
        std::mem::take(&mut self.buf)
    }

    /// Number of records written since the last [`CmdWriter::take`].
    pub fn records(&self) -> u32 {
        self.records
    }

    /// Appends one record. The payload length must equal the layout size of `op` (checked against
    /// [`OPS`]; a mismatch is a programming error and is reported, nothing is written).
    pub fn record(&mut self, op: u16, p: Payload) -> Result<&mut CmdWriter, SimError> {
        let want = op_desc(op).map(OpDesc::payload_len);
        if want != Some(p.0.len()) {
            return Err(SimError::BadCommand(
                "payload size differs from the op layout",
            ));
        }
        let len = u16::try_from(p.0.len()).map_err(|_| SimError::BadCommand("payload too long"))?;
        self.buf.extend_from_slice(&op.to_le_bytes());
        self.buf.extend_from_slice(&len.to_le_bytes());
        self.buf.extend_from_slice(&p.0);
        self.records += 1;
        Ok(self)
    }

    /// Appends a record of an op unknown to this version (forward-compatibility tests).
    pub fn raw(&mut self, op: u16, bytes: &[u8]) -> &mut CmdWriter {
        self.buf.extend_from_slice(&op.to_le_bytes());
        self.buf
            .extend_from_slice(&u16::try_from(bytes.len()).unwrap_or(u16::MAX).to_le_bytes());
        self.buf.extend_from_slice(bytes);
        self.records += 1;
        self
    }

    /// `ADD_MOVER`.
    #[allow(clippy::too_many_arguments)]
    pub fn add_mover(
        &mut self,
        id: u32,
        kind: u8,
        is_active: bool,
        mover_index: u32,
        model: (u32, u32),
        skel_id: u32,
        pos: Vec3,
        angle: f32,
        scale: f32,
    ) -> Result<&mut CmdWriter, SimError> {
        self.record(
            OP_ADD_MOVER,
            payload()
                .u32(id)
                .u8(kind)
                .u8(u8::from(is_active))
                .u16(0)
                .u32(mover_index)
                .u32(model.0)
                .u32(model.1)
                .u32(skel_id)
                .v3(pos)
                .f32(angle)
                .f32(0.0)
                .u32(0)
                .u32(0)
                .u32(u32::MAX)
                .f32(scale),
        )
    }

    /// `REMOVE_MOVER`.
    pub fn remove_mover(&mut self, id: u32) -> Result<&mut CmdWriter, SimError> {
        self.record(OP_REMOVE_MOVER, payload().u32(id))
    }

    /// `SET_PARAMS`.
    pub fn set_params(
        &mut self,
        id: u32,
        p: &SimMoverParams,
        bounds_entries: &[u32],
    ) -> Result<&mut CmdWriter, SimError> {
        let words = params_to_words(p, bounds_entries)?;
        let mut pl = payload().u32(id);
        for w in words {
            pl = pl.u32(w);
        }
        self.record(OP_SET_PARAMS, pl)
    }

    /// `SET_ACTIVE`.
    pub fn set_active(&mut self, id: u32) -> Result<&mut CmdWriter, SimError> {
        self.record(OP_SET_ACTIVE, payload().u32(id))
    }

    /// `SET_CLOCK`.
    pub fn set_clock(&mut self, offset: i64) -> Result<&mut CmdWriter, SimError> {
        self.record(OP_SET_CLOCK, payload().i64(offset))
    }

    /// `SET_VISIBLE`.
    pub fn set_visible(
        &mut self,
        id: u32,
        visible: bool,
        want_attach: bool,
    ) -> Result<&mut CmdWriter, SimError> {
        self.record(
            OP_SET_VISIBLE,
            payload()
                .u32(id)
                .u8(u8::from(visible))
                .u8(u8::from(want_attach)),
        )
    }

    /// `SET_SKELETON`.
    pub fn set_skeleton(
        &mut self,
        id: u32,
        skel_id: u32,
        model: (u32, u32),
    ) -> Result<&mut CmdWriter, SimError> {
        self.record(
            OP_SET_SKELETON,
            payload().u32(id).u32(skel_id).u32(model.0).u32(model.1),
        )
    }

    /// `SET_POS`.
    pub fn set_pos(&mut self, id: u32, pos: Vec3) -> Result<&mut CmdWriter, SimError> {
        self.record(OP_SET_POS, payload().u32(id).v3(pos))
    }

    /// `MOTION_ARRIVE`.
    pub fn motion_arrive(&mut self, id: u32, obj_msg: u32) -> Result<&mut CmdWriter, SimError> {
        self.record(OP_MOTION_ARRIVE, payload().u32(id).u32(obj_msg))
    }

    /// `ACT`.
    pub fn act(
        &mut self,
        id: u32,
        frame: u64,
        msg: u32,
        p: [i32; 5],
        queue_if_busy: bool,
    ) -> Result<&mut CmdWriter, SimError> {
        let mut pl = payload().u32(id).u64(frame).u32(msg);
        for v in p {
            pl = pl.i32(v);
        }
        self.record(OP_ACT, pl.u8(u8::from(queue_if_busy)))
    }

    /// `KEYFRAME`.
    pub fn keyframe(
        &mut self,
        id: u32,
        frame: u64,
        pos: Vec3,
        angle: f32,
        state: u32,
    ) -> Result<&mut CmdWriter, SimError> {
        self.record(
            OP_KEYFRAME,
            payload().u32(id).u64(frame).v3(pos).f32(angle).u32(state),
        )
    }

    /// `STATE` (ground state: `has_fly = 0`; the fly fields are written as zeros).
    #[allow(clippy::too_many_arguments)]
    pub fn state(
        &mut self,
        id: u32,
        frame: u64,
        reason: u8,
        s: &crate::sync::StateUpdate,
    ) -> Result<&mut CmdWriter, SimError> {
        let fly = s.fly.unwrap_or_default();
        self.record(
            OP_STATE,
            payload()
                .u32(id)
                .u64(frame)
                .u8(reason)
                .u8(u8::from(s.fly.is_some()))
                .u16(0)
                .v3(s.pos)
                .v3(s.delta)
                .f32(s.angle)
                .u32(s.state.bits())
                .u32(s.flags.bits())
                .u32(s.motion)
                .i32(s.motion_ex)
                .i32(s.loop_ as i32) // BOUNDS: ANILOOP_* are small
                .u32(s.motion_option)
                .f32(s.motion_frame.unwrap_or(-1.0))
                .f32(fly.angle_x)
                .f32(fly.acc_power)
                .f32(fly.turn_angle),
        )
    }

    /// `INTENT`.
    pub fn intent(
        &mut self,
        id: u32,
        frame: u64,
        kind: u8,
        stamp: &crate::sync::IntentStampData,
        args: [f32; 6],
    ) -> Result<&mut CmdWriter, SimError> {
        let mut pl = payload()
            .u32(id)
            .u64(frame)
            .u8(kind)
            .pad(3)
            .v3(stamp.pos)
            .f32(stamp.angle)
            .u32(stamp.state.bits())
            .u32(stamp.flags.bits())
            .f32(stamp.angle_x);
        for a in args {
            pl = pl.f32(a);
        }
        self.record(OP_INTENT, pl)
    }

    /// `LOCAL_DEST_POS`.
    pub fn local_dest_pos(&mut self, pos: Vec3, forward: bool) -> Result<&mut CmdWriter, SimError> {
        self.record(
            OP_LOCAL_DEST_POS,
            payload().v3(pos).u8(u8::from(forward)).pad(3),
        )
    }

    /// `LOCAL_CMD`.
    #[allow(clippy::too_many_arguments)]
    pub fn local_cmd(
        &mut self,
        act: u32,
        target: u32,
        range: f32,
        ar: u32,
        p: [i32; 3],
        flags: u8,
    ) -> Result<&mut CmdWriter, SimError> {
        self.record(
            OP_LOCAL_CMD,
            payload()
                .u32(act)
                .u32(target)
                .f32(range)
                .u32(ar)
                .i32(p[0])
                .i32(p[1])
                .i32(p[2])
                .u8(flags)
                .pad(3),
        )
    }

    /// `LOCAL_CLEAR`.
    pub fn local_clear(&mut self, what: u8) -> Result<&mut CmdWriter, SimError> {
        self.record(OP_LOCAL_CLEAR, payload().u8(what).pad(3))
    }

    /// `LOCAL_FACE`.
    pub fn local_face(&mut self, angle: f32) -> Result<&mut CmdWriter, SimError> {
        self.record(OP_LOCAL_FACE, payload().f32(angle))
    }

    /// `DAMAGE_FX` (`Damage`): the defender's damage motion and knock-back at `frame`; `tail` is the legacy
    /// `AF_FLYING` tail `(defender position, defender yaw)`.
    pub fn damage_fx(
        &mut self,
        defender: u32,
        frame: u64,
        atk_flags: u32,
        attacker: u32,
        tail: Option<(Vec3, f32)>,
    ) -> Result<&mut CmdWriter, SimError> {
        let (pos, angle) = tail.unwrap_or((Vec3::ZERO, 0.0));
        self.record(
            OP_DAMAGE_FX,
            payload()
                .u32(defender)
                .u64(frame)
                .u32(atk_flags)
                .u32(attacker)
                .u8(if tail.is_some() { DAMAGE_FX_TAIL } else { 0 })
                .pad(3)
                .v3(pos)
                .f32(angle),
        )
    }

    /// `DAMAGE_FX` with the attacker's yaw exactly as the server used it (`Damage.attacker_angle`): the
    /// knock-back no longer depends on the client's copy of the attacker.
    #[allow(clippy::too_many_arguments)]
    pub fn damage_fx_exact(
        &mut self,
        defender: u32,
        frame: u64,
        atk_flags: u32,
        attacker: u32,
        tail: Option<(Vec3, f32)>,
        attacker_angle: f32,
    ) -> Result<&mut CmdWriter, SimError> {
        let (pos, angle) = tail.unwrap_or((Vec3::ZERO, 0.0));
        let bits = if tail.is_some() { DAMAGE_FX_TAIL } else { 0 } | DAMAGE_FX_ATTACKER_ANGLE;
        self.record(
            OP_DAMAGE_FX,
            payload()
                .u32(defender)
                .u64(frame)
                .u32(atk_flags)
                .u32(attacker)
                .u8(bits)
                .u16(yaw_to_q16(attacker_angle))
                .pad(1)
                .v3(pos)
                .f32(angle),
        )
    }

    /// `PUSH_POWER`.
    pub fn push_power(
        &mut self,
        id: u32,
        frame: u64,
        angle: f32,
        push_angle: f32,
        power: f32,
    ) -> Result<&mut CmdWriter, SimError> {
        self.record(
            OP_PUSH_POWER,
            payload()
                .u32(id)
                .u64(frame)
                .f32(angle)
                .f32(push_angle)
                .f32(power),
        )
    }

    /// `DIE` (`MoverDeath`): `cause_msg` is `OBJMSG_DAMAGE` or `OBJMSG_DAMAGE_FORCE`.
    pub fn die(
        &mut self,
        id: u32,
        frame: u64,
        cause_msg: u32,
        attacker: u32,
    ) -> Result<&mut CmdWriter, SimError> {
        self.record(
            OP_DIE,
            payload().u32(id).u64(frame).u32(cause_msg).u32(attacker),
        )
    }

    /// `SET_SKILL`.
    pub fn set_skill(
        &mut self,
        id: u32,
        skill: u32,
        level: u32,
        target: u32,
        skill_count: i32,
        act: u32,
    ) -> Result<&mut CmdWriter, SimError> {
        self.record(
            OP_SET_SKILL,
            payload()
                .u32(id)
                .u32(skill)
                .u32(level)
                .u32(target)
                .i32(skill_count)
                .u32(act),
        )
    }

    /// `SET_FUEL`: `m_nFuel` and the turbo fuel `m_tmAccFuel` (ms); `fuel == 0` stops the thrust.
    pub fn set_fuel(
        &mut self,
        id: u32,
        fuel: i32,
        acc_fuel: i32,
    ) -> Result<&mut CmdWriter, SimError> {
        self.record(OP_SET_FUEL, payload().u32(id).i32(fuel).i32(acc_fuel))
    }

    /// `SET_TRACKING` (`0` = off).
    pub fn set_tracking(&mut self, target: u32) -> Result<&mut CmdWriter, SimError> {
        self.record(OP_SET_TRACKING, payload().u32(target))
    }

    /// `PROJECTILE_SPAWN`: the projectile stands at `origin` at the start of `frame`.
    #[allow(clippy::too_many_arguments)]
    pub fn projectile_spawn(
        &mut self,
        projectile_id: u32,
        kind: u8,
        attacker: u32,
        target: u32,
        frame: u64,
        origin: Vec3,
        dest: Vec3,
        speed: f32,
        shot_index: u32,
    ) -> Result<&mut CmdWriter, SimError> {
        self.record(
            OP_PROJECTILE_SPAWN,
            payload()
                .u32(projectile_id)
                .u8(kind)
                .pad(3)
                .u32(attacker)
                .u32(target)
                .u64(frame)
                .v3(origin)
                .v3(dest)
                .f32(speed)
                .u32(shot_index),
        )
    }

    /// `CTRL_ADD`: a ctrl with the LH world matrix `world` (`Mat43`, 12 floats), the model box
    /// `aabb = [min, max]` and its spawn order `seq`.
    pub fn ctrl_add(
        &mut self,
        id: u32,
        model_key: u32,
        world: [f32; 12],
        aabb: [f32; 6],
        seq: u32,
    ) -> Result<&mut CmdWriter, SimError> {
        let mut p = payload().u32(id).u32(model_key);
        for f in world.into_iter().chain(aabb) {
            p = p.f32(f);
        }
        self.record(OP_CTRL_ADD, p.u32(seq))
    }

    /// `CTRL_STATE`: ctrl `id` plays (`anim_frame >= 0` from that frame, `< 0` from where it holds) from
    /// server frame `frame`; `loop_bits` are the [`decor`] `STATE_*` bits.
    pub fn ctrl_state(
        &mut self,
        id: u32,
        frame: u64,
        anim_frame: f32,
        loop_bits: u32,
    ) -> Result<&mut CmdWriter, SimError> {
        self.record(
            OP_CTRL_STATE,
            payload().u32(id).u64(frame).f32(anim_frame).u32(loop_bits),
        )
    }

    /// `CTRL_REMOVE`.
    pub fn ctrl_remove(&mut self, id: u32) -> Result<&mut CmdWriter, SimError> {
        self.record(OP_CTRL_REMOVE, payload().u32(id))
    }

    /// `DECOR_ADD` (an existing id is replaced). `local` is the LH `Mat43` (world matrix for a static
    /// entry, offset for an attached one).
    #[allow(clippy::too_many_arguments)]
    pub fn decor_add(
        &mut self,
        id: u32,
        kind: u8,
        flags: u8,
        attach: u8,
        model: (u32, u32),
        clip_id: u32,
        skel_id: u32,
        parent_mover: u32,
        start_frame: f32,
        local: [f32; 12],
    ) -> Result<&mut CmdWriter, SimError> {
        let mut p = payload()
            .u32(id)
            .u8(kind)
            .u8(flags)
            .u8(attach)
            .pad(1)
            .u32(model.0)
            .u32(model.1)
            .u32(clip_id)
            .u32(skel_id)
            .u32(parent_mover)
            .f32(start_frame);
        for f in local {
            p = p.f32(f);
        }
        self.record(OP_DECOR_ADD, p)
    }

    /// `DECOR_REMOVE`.
    pub fn decor_remove(&mut self, id: u32) -> Result<&mut CmdWriter, SimError> {
        self.record(OP_DECOR_REMOVE, payload().u32(id))
    }

    /// `DECOR_VISIBLE`.
    pub fn decor_visible(&mut self, id: u32, visible: bool) -> Result<&mut CmdWriter, SimError> {
        self.record(
            OP_DECOR_VISIBLE,
            payload().u32(id).u8(u8::from(visible)).pad(3),
        )
    }

    /// `PROJECTILE_END`: the server ended the projectile (`hit` at `pos`, or expired).
    pub fn projectile_end(
        &mut self,
        projectile_id: u32,
        hit: bool,
        pos: Vec3,
    ) -> Result<&mut CmdWriter, SimError> {
        self.record(
            OP_PROJECTILE_END,
            payload()
                .u32(projectile_id)
                .u8(u8::from(hit))
                .pad(3)
                .v3(pos),
        )
    }
}

/// A little-endian cursor over a payload (panic-free: a short read yields zero and the caller has
/// validated the length against [`OPS`] beforehand).
#[derive(Clone, Copy, Debug)]
pub struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    /// A reader over `b`.
    pub fn new(b: &'a [u8]) -> Reader<'a> {
        Reader { b, at: 0 }
    }

    fn take<const N: usize>(&mut self) -> [u8; N] {
        let mut out = [0u8; N];
        if let Some(s) = self.b.get(self.at..self.at + N) {
            out.copy_from_slice(s);
        }
        self.at += N;
        out
    }

    /// Skips `n` bytes.
    pub fn skip(&mut self, n: usize) {
        self.at += n;
    }
    /// Reads a `u8`.
    pub fn u8(&mut self) -> u8 {
        u8::from_le_bytes(self.take())
    }
    /// Reads a `u16`.
    pub fn u16(&mut self) -> u16 {
        u16::from_le_bytes(self.take())
    }
    /// Reads a `u32`.
    pub fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.take())
    }
    /// Reads an `i32`.
    pub fn i32(&mut self) -> i32 {
        i32::from_le_bytes(self.take())
    }
    /// Reads a `u64`.
    pub fn u64(&mut self) -> u64 {
        u64::from_le_bytes(self.take())
    }
    /// Reads an `i64`.
    pub fn i64(&mut self) -> i64 {
        i64::from_le_bytes(self.take())
    }
    /// Reads an `f32`.
    pub fn f32(&mut self) -> f32 {
        f32::from_le_bytes(self.take())
    }
    /// Reads three `f32`.
    pub fn v3(&mut self) -> Vec3 {
        Vec3::new(self.f32(), self.f32(), self.f32())
    }
}

/// The bounds of an AABB as six floats `[min.xyz, max.xyz]`.
pub fn aabb_from_floats(f: [f32; 6]) -> Aabb {
    Aabb {
        min: Vec3::new(f[0], f[1], f[2]),
        max: Vec3::new(f[3], f[4], f[5]),
    }
}

/// The box a `CTRL_ADD` asks for: an all-zero `aabb` means "the model's own box" (`None`: the `FLYK` entry of
/// the model decides, exactly like the server, which registers its colliders without a box of their own); any
/// other value is the box to use (31 §17.4 lets the renderer pass the box of the loaded model).
pub fn ctrl_aabb(f: [f32; 6]) -> Option<Aabb> {
    if f.iter().all(|&v| v == 0.0) {
        None
    } else {
        Some(aabb_from_floats(f))
    }
}

/// Splits a command buffer into `(op, payload)` records without interpreting them.
///
/// Fails when a record header or payload runs past the end of the buffer.
pub fn split_records(buf: &[u8]) -> Result<Vec<(u16, &[u8])>, SimError> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at < buf.len() {
        let head = buf
            .get(at..at + 4)
            .ok_or(SimError::BadCommand("truncated record header"))?;
        let [o0, o1, l0, l1] = *head
            .first_chunk::<4>()
            .ok_or(SimError::BadCommand("truncated record header"))?;
        let op = u16::from_le_bytes([o0, o1]);
        let len = usize::from(u16::from_le_bytes([l0, l1]));
        let body = buf
            .get(at + 4..at + 4 + len)
            .ok_or(SimError::BadCommand("record payload runs past the buffer"))?;
        out.push((op, body));
        at += 4 + len;
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------------
// TypeScript export (tests/wire_export.rs)
// ---------------------------------------------------------------------------------------------------
