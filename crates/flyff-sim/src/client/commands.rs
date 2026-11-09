//! The command-buffer decoder (Plan/31 §17.4): packed records to typed [`Cmd`]s.
//!
//! Unknown ops are skipped by their `len` (forward compatible); a record whose `len` differs from the fixed
//! size of its op, a non-finite float or an out-of-range enumeration value is an error
//! ([`SimError::BadCommand`]). The whole buffer is framed and checked BEFORE anything is applied, so a
//! malformed buffer changes nothing.

use crate::act::ActMsg;
use crate::control::LocalOp;
use crate::num::Vec3;
use crate::pose::AttachPoint;
use crate::projectile::ProjectileKind;
use crate::sync::{FlyFields, IntentOp, IntentStampData, StateUpdate};
use crate::types::{ObjState, SimError, StateFlags};
use crate::wire::{self, Reader};
use flyff_common::ObjId;

/// `ADD_MOVER` (op 0x01).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct AddMover {
    /// Object id.
    pub id: ObjId,
    /// `wire::KIND_*`.
    pub kind: u8,
    /// The own player.
    pub is_active: bool,
    /// `m_dwIndex`.
    pub mover_index: u32,
    /// `(OT_*, index)` of the model.
    pub model: (u32, u32),
    /// Skeleton id (`0` = none).
    pub skel_id: u32,
    /// Position.
    pub pos: Vec3,
    /// Yaw.
    pub angle: f32,
    /// Pitch.
    pub angle_x: f32,
    /// Initial state word (0 = fresh).
    pub state: u32,
    /// Initial state flags.
    pub state_flags: u32,
    /// Initial motion (used with a non-zero `state`).
    pub motion: u32,
    /// Scale.
    pub scale: f32,
}

/// `PROJECTILE_SPAWN` (op 0x14): a projectile stands at `origin` at the start of `frame`.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ProjectileSpawn {
    /// `projectile_id`.
    pub id: u32,
    /// The behaviour class.
    pub kind: ProjectileKind,
    /// The shooter.
    pub attacker: ObjId,
    /// The homing target (`ObjId::NONE` for fixed destinations).
    pub target: ObjId,
    /// The server frame the projectile is at `origin`.
    pub frame: u64,
    /// Origin.
    pub origin: Vec3,
    /// The initial destination.
    pub dest: Vec3,
    /// Per-frame speed (`0` = the table value of the kind with `faster_projectiles`).
    pub speed: f32,
    /// Index within a multi-shot burst (informational).
    pub shot_index: u32,
}

/// `CTRL_ADD` (op 0x16).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct CtrlAdd {
    /// The ctrl's `ObjId`.
    pub id: ObjId,
    /// `mdlDyna` ctrl index (the `FLYK` key and the `MotionTable` `(OT_CTRL, index)` key).
    pub model_key: u32,
    /// The LH world matrix (`Mat43`).
    pub world: [f32; 12],
    /// The model box `[min, max]`; all zeros = the box of the `FLYK` model ([`crate::wire::ctrl_aabb`]).
    pub aabb: [f32; 6],
    /// Spawn order (the candidate order of the dynamic link walk).
    pub seq: u32,
}

/// `DECOR_ADD` (op 0x1D).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct DecorAdd {
    /// The decor id (its own namespace).
    pub id: u32,
    /// `wire::decor::KIND_STATIC` or `KIND_ATTACHED`.
    pub kind: u8,
    /// `wire::decor::{LOOP, VISIBLE, SOUNDS, SOUND_POSITIONAL}` bits.
    pub flags: u8,
    /// Attachment point code (`AttachPoint::from_code`).
    pub attach: u8,
    /// `(OT_*, index)` of the model.
    pub model: (u32, u32),
    /// Clip id, `wire::decor::CLIP_BUILTIN` = the built-in clip of the model.
    pub clip_id: u32,
    /// Skeleton id, `0` = rigid node animation.
    pub skel_id: u32,
    /// The parent mover (attached entries).
    pub parent_mover: ObjId,
    /// The frame the clock starts at.
    pub start_frame: f32,
    /// The LH `Mat43`: the world matrix of a static entry, the offset of an attached one.
    pub local: [f32; 12],
}

/// A decoded command.
#[derive(Clone, PartialEq, Debug)]
pub enum Cmd {
    /// `ADD_MOVER`.
    AddMover(AddMover),
    /// `REMOVE_MOVER`.
    RemoveMover(ObjId),
    /// `SET_PARAMS` (the block is validated by [`wire::params_from_words`] when applied).
    SetParams {
        /// The mover.
        id: ObjId,
        /// The parameter block.
        words: Vec<u32>,
    },
    /// `STATE`.
    State {
        /// The mover.
        id: ObjId,
        /// Server frame.
        frame: u64,
        /// `wire::reason::*`.
        reason: u8,
        /// The state.
        s: StateUpdate,
    },
    /// `INTENT`.
    Intent {
        /// The mover.
        id: ObjId,
        /// Server frame of the stamp.
        frame: u64,
        /// The start state.
        stamp: IntentStampData,
        /// The operation.
        op: IntentOp,
    },
    /// `KEYFRAME`.
    Keyframe {
        /// The mover.
        id: ObjId,
        /// Server frame.
        frame: u64,
        /// Position.
        pos: Vec3,
        /// Yaw.
        angle: f32,
        /// State word.
        state: u32,
    },
    /// `ACT`.
    Act {
        /// The mover.
        id: ObjId,
        /// Server frame.
        frame: u64,
        /// The message.
        msg: ActMsg,
        /// Queue a refused attack message.
        queue_if_busy: bool,
    },
    /// `DAMAGE_FX`.
    DamageFx {
        /// The defender.
        defender: ObjId,
        /// Server frame.
        frame: u64,
        /// `AF_*`.
        atk_flags: u32,
        /// The attacker.
        attacker: ObjId,
        /// The legacy `AF_FLYING` tail: `(defender_pos, defender_angle)`.
        tail: Option<(Vec3, f32)>,
        /// The attacker's yaw exactly as the server used it for the knock-back (`Damage.attacker_angle`,
        /// Plan/31 Q10); `None` falls back to the yaw of the client's copy of the attacker.
        attacker_angle: Option<f32>,
    },
    /// `PUSH_POWER`.
    PushPower {
        /// The mover.
        id: ObjId,
        /// Server frame.
        frame: u64,
        /// Yaw.
        angle: f32,
        /// Push direction.
        push_angle: f32,
        /// Power.
        power: f32,
    },
    /// `DIE`.
    Die {
        /// The mover.
        id: ObjId,
        /// Server frame.
        frame: u64,
        /// The cause message.
        cause_msg: u32,
        /// The attacker.
        attacker: ObjId,
    },
    /// `MOTION_ARRIVE`.
    MotionArrive {
        /// The mover.
        id: ObjId,
        /// The message.
        obj_msg: u32,
    },
    /// `SET_FUEL`.
    SetFuel {
        /// The mover.
        id: ObjId,
        /// `m_nFuel`.
        fuel: i32,
        /// `m_tmAccFuel`.
        acc_fuel: i32,
    },
    /// `SET_ACTIVE`.
    SetActive(ObjId),
    /// `SET_CLOCK`.
    SetClock(i64),
    /// `SET_VISIBLE`.
    SetVisible {
        /// The mover.
        id: ObjId,
        /// Evaluate the pose.
        visible: bool,
        /// Write the attachment record.
        want_attach: bool,
    },
    /// `SET_SKILL`.
    SetSkill {
        /// The mover.
        id: ObjId,
        /// Skill id.
        skill: u32,
        /// Level.
        level: u32,
        /// Target.
        target: ObjId,
        /// `nSkillCount`.
        skill_count: i32,
        /// `OBJACT_*`.
        act: u32,
    },
    /// `SET_SKELETON`.
    SetSkeleton {
        /// The mover.
        id: ObjId,
        /// New skeleton id.
        skel_id: u32,
        /// New model.
        model: (u32, u32),
    },
    /// `SET_POS`.
    SetPos {
        /// The mover.
        id: ObjId,
        /// The new position.
        pos: Vec3,
    },
    /// `SET_TRACKING` (active mover; `ObjId::NONE` = off).
    SetTracking(ObjId),
    /// `PROJECTILE_SPAWN`.
    ProjectileSpawn(ProjectileSpawn),
    /// `PROJECTILE_END`: the server ended a projectile.
    ProjectileEnd {
        /// `projectile_id`.
        id: u32,
        /// Ended with a contact (else expired).
        hit: bool,
        /// The impact position.
        pos: Vec3,
    },
    /// `LOCAL_CMD`/`LOCAL_DEST_POS`/`LOCAL_CLEAR`/`LOCAL_FACE` (active mover, no frame).
    Local(LocalOp),
    /// `CTRL_ADD`.
    CtrlAdd(CtrlAdd),
    /// `CTRL_STATE`: ctrl `id` starts playing at server frame `frame`.
    CtrlState {
        /// The ctrl.
        id: ObjId,
        /// Server frame.
        frame: u64,
        /// The frame to start from, negative = where the ctrl holds.
        anim_frame: f32,
        /// `wire::decor::STATE_*` bits.
        loop_bits: u32,
    },
    /// `CTRL_REMOVE`.
    CtrlRemove(ObjId),
    /// `DECOR_ADD`.
    DecorAdd(DecorAdd),
    /// `DECOR_REMOVE`.
    DecorRemove(u32),
    /// `DECOR_VISIBLE`.
    DecorVisible {
        /// The decor id.
        id: u32,
        /// Evaluate the pose.
        visible: bool,
    },
    /// An op of this contract that this version does not execute: the record is validated and ignored.
    Unsupported(u16),
}

impl Cmd {
    /// The server frame of a frame-stamped command.
    pub fn server_frame(&self) -> Option<u64> {
        match self {
            Cmd::State { frame, .. }
            | Cmd::Intent { frame, .. }
            | Cmd::Keyframe { frame, .. }
            | Cmd::Act { frame, .. }
            | Cmd::DamageFx { frame, .. }
            | Cmd::PushPower { frame, .. }
            | Cmd::Die { frame, .. }
            | Cmd::CtrlState { frame, .. } => Some(*frame),
            Cmd::ProjectileSpawn(p) => Some(p.frame),
            _ => None,
        }
    }
}

fn bad(msg: &'static str) -> SimError {
    SimError::BadCommand(msg)
}

fn finite(v: f32) -> Result<f32, SimError> {
    if v.is_finite() {
        Ok(v)
    } else {
        Err(bad("non-finite float"))
    }
}

fn finite3(v: Vec3) -> Result<Vec3, SimError> {
    finite(v.x)?;
    finite(v.y)?;
    finite(v.z)?;
    Ok(v)
}

fn flag(b: u8) -> Result<bool, SimError> {
    match b {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(bad("boolean byte is not 0 or 1")),
    }
}

/// Decodes one record. `Ok(None)` for an op this version does not know (skipped).
pub fn decode_record(op: u16, payload: &[u8]) -> Result<Option<Cmd>, SimError> {
    let Some(desc) = wire::op_desc(op) else {
        return Ok(None);
    };
    if payload.len() != desc.payload_len() {
        return Err(bad("record length differs from the op layout"));
    }
    let mut r = Reader::new(payload);
    let cmd = match op {
        wire::OP_ADD_MOVER => {
            let id = ObjId(r.u32());
            let kind = r.u8();
            let is_active = flag(r.u8())?;
            r.skip(2);
            if kind > wire::KIND_PREVIEW {
                return Err(bad("ADD_MOVER kind is unknown"));
            }
            Cmd::AddMover(AddMover {
                id,
                kind,
                is_active,
                mover_index: r.u32(),
                model: (r.u32(), r.u32()),
                skel_id: r.u32(),
                pos: finite3(r.v3())?,
                angle: finite(r.f32())?,
                angle_x: finite(r.f32())?,
                state: r.u32(),
                state_flags: r.u32(),
                motion: r.u32(),
                scale: finite(r.f32())?,
            })
        }
        wire::OP_REMOVE_MOVER => Cmd::RemoveMover(ObjId(r.u32())),
        wire::OP_SET_PARAMS => {
            let id = ObjId(r.u32());
            let words = (0..wire::PARAMS_WORDS).map(|_| r.u32()).collect();
            Cmd::SetParams { id, words }
        }
        wire::OP_STATE => {
            let id = ObjId(r.u32());
            let frame = r.u64();
            let reason = r.u8();
            let has_fly = flag(r.u8())?;
            r.skip(2);
            let pos = finite3(r.v3())?;
            let delta = finite3(r.v3())?;
            let angle = finite(r.f32())?;
            let state = ObjState(r.u32());
            let flags = StateFlags(r.u32());
            let motion = r.u32();
            let motion_ex = r.i32();
            let loop_ = r.i32() as u32; // BOUNDS: ANILOOP_* bit pattern
            let motion_option = r.u32();
            let motion_frame = finite(r.f32())?;
            let angle_x = finite(r.f32())?;
            let acc_power = finite(r.f32())?;
            let turn_angle = finite(r.f32())?;
            Cmd::State {
                id,
                frame,
                reason,
                s: StateUpdate {
                    pos,
                    delta,
                    angle,
                    state,
                    flags,
                    motion,
                    motion_ex,
                    loop_,
                    motion_option,
                    motion_frame: if motion_frame < 0.0 {
                        None
                    } else {
                        Some(motion_frame)
                    },
                    fly: has_fly.then_some(FlyFields {
                        angle_x,
                        acc_power,
                        turn_angle,
                    }),
                },
            }
        }
        wire::OP_INTENT => {
            let id = ObjId(r.u32());
            let frame = r.u64();
            let kind = r.u8();
            r.skip(3);
            let stamp = IntentStampData {
                pos: finite3(r.v3())?,
                angle: finite(r.f32())?,
                state: ObjState(r.u32()),
                flags: StateFlags(r.u32()),
                angle_x: finite(r.f32())?,
            };
            let [a0, a1, a2, a3, a4, a5] = [r.f32(), r.f32(), r.f32(), r.f32(), r.f32(), r.f32()];
            let op = match kind {
                wire::INTENT_DEST_POS => IntentOp::DestPos {
                    pos: finite3(Vec3::new(a0, a1, a2))?,
                    forward: a3 != 0.0,
                },
                wire::INTENT_DEST_OBJ => IntentOp::DestObj {
                    target: ObjId(a0.to_bits()),
                    range: finite(a1)?,
                },
                wire::INTENT_DEST_ANGLE => IntentOp::DestAngle {
                    angle: finite(a0)?,
                    left: a1 != 0.0,
                },
                wire::INTENT_MOVE_PATTERN => IntentOp::SetMovePattern {
                    pattern: a0.to_bits() as i32, // BOUNDS: bit pattern
                    pos: finite3(Vec3::new(a1, a2, a3))?,
                    angle: finite(a4)?,
                    angle_x: finite(a5)?,
                },
                wire::INTENT_SPEED_FACTOR => IntentOp::SpeedFactor {
                    factor: finite(a0)?,
                },
                wire::INTENT_SET_POS => IntentOp::SetPos {
                    pos: finite3(Vec3::new(a0, a1, a2))?,
                },
                _ => return Err(bad("INTENT kind is unknown")),
            };
            Cmd::Intent {
                id,
                frame,
                stamp,
                op,
            }
        }
        wire::OP_KEYFRAME => Cmd::Keyframe {
            id: ObjId(r.u32()),
            frame: r.u64(),
            pos: finite3(r.v3())?,
            angle: finite(r.f32())?,
            state: r.u32(),
        },
        wire::OP_ACT => {
            let id = ObjId(r.u32());
            let frame = r.u64();
            let msg = r.u32();
            let p = [r.i32(), r.i32(), r.i32(), r.i32(), r.i32()];
            let queue_if_busy = flag(r.u8())?;
            Cmd::Act {
                id,
                frame,
                msg: ActMsg::with(msg, p),
                queue_if_busy,
            }
        }
        wire::OP_DAMAGE_FX => {
            let defender = ObjId(r.u32());
            let frame = r.u64();
            let atk_flags = r.u32();
            let attacker = ObjId(r.u32());
            // `has_tail`: bit 0 = the legacy tail follows, bit 1 = `pad[0..2]` carries the attacker's yaw as a
            // `u16` in 1/65536 of a turn (the `SetAngle` domain, so an exact copy of the server's value)
            let bits = r.u8();
            if bits > wire::DAMAGE_FX_TAIL | wire::DAMAGE_FX_ATTACKER_ANGLE {
                return Err(bad("DAMAGE_FX flag byte has unknown bits"));
            }
            let q = r.u16();
            r.skip(1);
            let pos = finite3(r.v3())?;
            let ang = finite(r.f32())?;
            Cmd::DamageFx {
                defender,
                frame,
                atk_flags,
                attacker,
                tail: (bits & wire::DAMAGE_FX_TAIL != 0).then_some((pos, ang)),
                attacker_angle: (bits & wire::DAMAGE_FX_ATTACKER_ANGLE != 0)
                    .then(|| wire::yaw_from_q16(q)),
            }
        }
        wire::OP_PUSH_POWER => Cmd::PushPower {
            id: ObjId(r.u32()),
            frame: r.u64(),
            angle: finite(r.f32())?,
            push_angle: finite(r.f32())?,
            power: finite(r.f32())?,
        },
        wire::OP_DIE => Cmd::Die {
            id: ObjId(r.u32()),
            frame: r.u64(),
            cause_msg: r.u32(),
            attacker: ObjId(r.u32()),
        },
        wire::OP_MOTION_ARRIVE => Cmd::MotionArrive {
            id: ObjId(r.u32()),
            obj_msg: r.u32(),
        },
        wire::OP_SET_FUEL => Cmd::SetFuel {
            id: ObjId(r.u32()),
            fuel: r.i32(),
            acc_fuel: r.i32(),
        },
        wire::OP_SET_ACTIVE => Cmd::SetActive(ObjId(r.u32())),
        wire::OP_SET_CLOCK => Cmd::SetClock(r.i64()),
        wire::OP_SET_VISIBLE => Cmd::SetVisible {
            id: ObjId(r.u32()),
            visible: flag(r.u8())?,
            want_attach: flag(r.u8())?,
        },
        wire::OP_SET_SKILL => Cmd::SetSkill {
            id: ObjId(r.u32()),
            skill: r.u32(),
            level: r.u32(),
            target: ObjId(r.u32()),
            skill_count: r.i32(),
            act: r.u32(),
        },
        wire::OP_SET_SKELETON => Cmd::SetSkeleton {
            id: ObjId(r.u32()),
            skel_id: r.u32(),
            model: (r.u32(), r.u32()),
        },
        wire::OP_SET_POS => Cmd::SetPos {
            id: ObjId(r.u32()),
            pos: finite3(r.v3())?,
        },
        wire::OP_SET_TRACKING => Cmd::SetTracking(ObjId(r.u32())),
        wire::OP_PROJECTILE_SPAWN => {
            let id = r.u32();
            let kind = ProjectileKind::from_u8(r.u8())
                .ok_or_else(|| bad("PROJECTILE_SPAWN kind is unknown"))?;
            r.skip(3);
            let attacker = ObjId(r.u32());
            let target = ObjId(r.u32());
            let frame = r.u64();
            let origin = finite3(r.v3())?;
            let dest = finite3(r.v3())?;
            let speed = finite(r.f32())?;
            if speed < 0.0 {
                return Err(bad("PROJECTILE_SPAWN speed is negative"));
            }
            Cmd::ProjectileSpawn(ProjectileSpawn {
                id,
                kind,
                attacker,
                target,
                frame,
                origin,
                dest,
                speed,
                shot_index: r.u32(),
            })
        }
        wire::OP_PROJECTILE_END => {
            let id = r.u32();
            let hit = flag(r.u8())?;
            r.skip(3);
            Cmd::ProjectileEnd {
                id,
                hit,
                pos: finite3(r.v3())?,
            }
        }
        wire::OP_LOCAL_CMD => {
            let act = r.u32();
            let target = ObjId(r.u32());
            let range = finite(r.f32())?;
            let ar = r.u32();
            let p = [r.i32(), r.i32(), r.i32()];
            let flags = r.u8();
            Cmd::Local(LocalOp::Cmd {
                act,
                target,
                range,
                ar,
                p,
                flags,
            })
        }
        wire::OP_LOCAL_DEST_POS => {
            let pos = finite3(r.v3())?;
            Cmd::Local(LocalOp::DestPos {
                pos,
                forward: flag(r.u8())?,
            })
        }
        wire::OP_LOCAL_CLEAR => Cmd::Local(LocalOp::Clear(r.u8())),
        wire::OP_LOCAL_FACE => Cmd::Local(LocalOp::Face(finite(r.f32())?)),
        wire::OP_CTRL_ADD => {
            let id = ObjId(r.u32());
            let model_key = r.u32();
            let mut world = [0.0_f32; 12];
            for w in &mut world {
                *w = finite(r.f32())?;
            }
            let mut aabb = [0.0_f32; 6];
            for a in &mut aabb {
                *a = finite(r.f32())?;
            }
            Cmd::CtrlAdd(CtrlAdd {
                id,
                model_key,
                world,
                aabb,
                seq: r.u32(),
            })
        }
        wire::OP_CTRL_STATE => {
            let id = ObjId(r.u32());
            let frame = r.u64();
            let anim_frame = finite(r.f32())?;
            let loop_bits = r.u32();
            if loop_bits & !wire::decor::STATE_MASK != 0 {
                return Err(bad("CTRL_STATE loop bits are unknown"));
            }
            Cmd::CtrlState {
                id,
                frame,
                anim_frame,
                loop_bits,
            }
        }
        wire::OP_CTRL_REMOVE => Cmd::CtrlRemove(ObjId(r.u32())),
        wire::OP_DECOR_ADD => {
            let id = r.u32();
            let kind = r.u8();
            let flags = r.u8();
            let attach = r.u8();
            r.skip(1);
            if kind != wire::decor::KIND_STATIC && kind != wire::decor::KIND_ATTACHED {
                return Err(bad("DECOR_ADD kind is unknown"));
            }
            if flags & !wire::decor::FLAGS_MASK != 0 {
                return Err(bad("DECOR_ADD flags are unknown"));
            }
            if AttachPoint::from_code(attach).is_none() {
                return Err(bad("DECOR_ADD attach point is unknown"));
            }
            let model = (r.u32(), r.u32());
            let clip_id = r.u32();
            let skel_id = r.u32();
            let parent_mover = ObjId(r.u32());
            let start_frame = finite(r.f32())?;
            let mut local = [0.0_f32; 12];
            for l in &mut local {
                *l = finite(r.f32())?;
            }
            Cmd::DecorAdd(DecorAdd {
                id,
                kind,
                flags,
                attach,
                model,
                clip_id,
                skel_id,
                parent_mover,
                start_frame,
                local,
            })
        }
        wire::OP_DECOR_REMOVE => Cmd::DecorRemove(r.u32()),
        wire::OP_DECOR_VISIBLE => {
            let id = r.u32();
            let visible = flag(r.u8())?;
            Cmd::DecorVisible { id, visible }
        }
        // validated by length above; not executed by this version
        other => Cmd::Unsupported(other),
    };
    Ok(Some(cmd))
}

/// Frames and decodes a whole command buffer: `(op, Some(cmd))` for known ops, `(op, None)` for skipped
/// unknown ones. Nothing is applied by this function.
pub fn decode_buffer(buf: &[u8]) -> Result<Vec<(u16, Option<Cmd>)>, SimError> {
    let records = wire::split_records(buf)?;
    let mut out = Vec::with_capacity(records.len());
    for (op, payload) in records {
        out.push((op, decode_record(op, payload)?));
    }
    Ok(out)
}
