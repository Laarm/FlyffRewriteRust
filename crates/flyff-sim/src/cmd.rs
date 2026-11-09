//! Active-mover command helpers (Plan/31 §11.7; WP-SIM-15 / WP-SIM-23).
//!
//! `local_cmd` (the `LOCAL_CMD` op) derives the arrival range of a command with [`approach_range`], and the
//! client host runs [`on_arrival`] when the active mover reached its destination object (the
//! `SimEvent::CmdArrived` raised by `steer::process_move_arrival`): the original `ProcessMoveArrival`
//! client branch (`MoverMove.cpp:73-287`) with the checks that need the UI (line of sight, skill timers)
//! left to doc 43/45, which answer the resulting [`Arrival`].

use crate::act::{ActMsg, send_act_msg_cx};
use crate::bounds::{get_attack_range, is_range_obj, radius};
use crate::mover::{SimMover, WeaponStance};
use crate::num::{get_degree, set_angle_quantized};
use crate::step::{Cx, SimFlags};
use crate::types::*;
use flyff_common::ObjId;

/// The kind of a command (`OBJACT_*`) as far as the arrival range is concerned.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CmdKind {
    /// No command: a plain `SetDestObj` (follow).
    None,
    /// `OBJACT_MELEE_ATTACK` (and `OBJACT_SP_ATTACK`).
    Melee,
    /// `OBJACT_RANGE_ATTACK` (bow).
    Range,
    /// `OBJACT_MAGIC_ATTACK` (wand auto attack).
    Magic,
    /// `OBJACT_USESKILL`.
    UseSkill,
    /// `OBJACT_USEITEM`.
    UseItem,
    /// `OBJACT_COLLECT`.
    Collect,
}

impl CmdKind {
    /// The kind of an `OBJACT_*` value (unknown values are [`CmdKind::None`]).
    pub const fn from_act(act: u32) -> CmdKind {
        match act {
            OBJACT_MELEE_ATTACK | OBJACT_SP_ATTACK => CmdKind::Melee,
            OBJACT_RANGE_ATTACK => CmdKind::Range,
            OBJACT_MAGIC_ATTACK => CmdKind::Magic,
            OBJACT_USESKILL => CmdKind::UseSkill,
            OBJACT_USEITEM => CmdKind::UseItem,
            OBJACT_COLLECT => CmdKind::Collect,
            _ => CmdKind::None,
        }
    }
}

/// The arrival range of a command (`CMD_Set*`, `MoverMsg.cpp:427-533`):
///
/// * melee: the caller's range (`0.0` = up to the target's radius), a yoyo uses its item range;
/// * range (bow): with `features.attack_range_code` `AR_WAND` (15 m), else the item range;
/// * magic (wand auto attack): `AR_WAND`;
/// * skills: the skill's `attack_range` code (`item_attack_range` is the `AR_*` of the hand item for
///   attacks and of the skill prop for skills);
/// * use item: 20 m while flying, else 0.
///
/// Every range includes the hawkeye bonus of [`get_attack_range`].
pub fn approach_range(
    kind: CmdKind,
    stance: Option<&WeaponStance>,
    item_attack_range: u32,
    flags: &SimFlags,
    hawkeye: i32,
    fly: bool,
) -> f32 {
    match kind {
        CmdKind::None | CmdKind::Collect => 0.0,
        CmdKind::Melee => {
            if stance.is_some_and(|s| s.kind3 == defs::IK3_YOYO) {
                get_attack_range(item_attack_range, hawkeye)
            } else {
                0.0
            }
        }
        CmdKind::Range => {
            if flags.attack_range_code {
                get_attack_range(AR_WAND, hawkeye)
            } else {
                get_attack_range(item_attack_range, hawkeye)
            }
        }
        CmdKind::Magic => get_attack_range(AR_WAND, hawkeye),
        CmdKind::UseSkill => get_attack_range(item_attack_range, hawkeye),
        CmdKind::UseItem => {
            if fly {
                20.0
            } else {
                0.0
            }
        }
    }
}

/// What [`on_arrival`] decided; the host turns it into events.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Arrival {
    /// Nothing to report (plain follow, collect).
    Quiet,
    /// The command target was reached: doc 43/45 issue the attack/skill/item use.
    Arrived {
        /// The `OBJACT_*` that arrived.
        cmd: u32,
        /// The target.
        target: ObjId,
    },
    /// A ranged attack was refused because the target is out of the item range (`fix_ranger_roller`).
    Aborted,
}

/// The active mover reached its destination object (Plan/31 §11.7; the client branch of
/// `CMover::ProcessMoveArrival`). `ar` is the `AR_*` of the hand item (the range check of a bow attack under
/// `features.fix_ranger_roller`); `target` is the destination object's snapshot (`None` when it vanished).
pub fn on_arrival(m: &mut SimMover, cx: &mut Cx<'_>, target_id: ObjId, ar: u32) -> Arrival {
    let cmd = m.cmd.act;
    let snap = cx.env.lookup.snapshot(target_id);
    let flags = cx.env.flags;
    let out = match CmdKind::from_act(cmd) {
        CmdKind::Melee | CmdKind::Magic => {
            send_act_msg_cx(m, cx, ActMsg::new(OBJMSG_STAND));
            send_act_msg_cx(m, cx, ActMsg::new(OBJMSG_STOP_TURN));
            m.clear_dest_obj();
            Arrival::Arrived {
                cmd,
                target: target_id,
            }
        }
        CmdKind::Range => {
            if flags.fix_ranger_roller {
                send_act_msg_cx(m, cx, ActMsg::new(OBJMSG_STOP));
            }
            send_act_msg_cx(m, cx, ActMsg::new(OBJMSG_STAND));
            send_act_msg_cx(m, cx, ActMsg::new(OBJMSG_STOP_TURN));
            m.clear_dest_obj();
            let out_of_range = flags.fix_ranger_roller
                && snap.is_some_and(|t| {
                    !is_range_obj(
                        t.pos,
                        t.radius,
                        m.pos,
                        radius(&m.params.bounds, m.params.scale),
                        get_attack_range(ar, m.params.hawkeye_rate),
                    )
                });
            if out_of_range {
                send_act_msg_cx(m, cx, ActMsg::new(OBJMSG_STOP));
                // KEEP-QUIRK: the original `return`s here, before `SetCmd(OBJACT_NONE)` (MoverMove.cpp:243):
                // the command stays, only the chase is over
                return Arrival::Aborted;
            } else {
                Arrival::Arrived {
                    cmd,
                    target: target_id,
                }
            }
        }
        CmdKind::UseSkill => {
            m.clear_dest_obj();
            send_act_msg_cx(m, cx, ActMsg::new(OBJMSG_STOP));
            Arrival::Arrived {
                cmd,
                target: target_id,
            }
        }
        CmdKind::UseItem => {
            m.clear_dest_obj();
            send_act_msg_cx(m, cx, ActMsg::new(OBJMSG_STAND));
            if let Some(t) = snap {
                m.angle = set_angle_quantized(get_degree(t.pos, m.pos));
            }
            Arrival::Arrived {
                cmd,
                target: target_id,
            }
        }
        CmdKind::Collect => Arrival::Quiet,
        CmdKind::None => {
            m.clear_dest_obj();
            send_act_msg_cx(m, cx, ActMsg::new(OBJMSG_STOP));
            Arrival::Quiet
        }
    };
    m.cmd = crate::mover::ActiveCmd::default();
    out
}
