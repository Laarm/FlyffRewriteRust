//! The flying action message table (`ProcessActMsg2`, `src:_AIInterface/ActionMoverMsg2.cpp:30-357`;
//! Plan/31 §6.3; ref/12 §4.7; WP-SIM-25).
//!
//! A mover with the `FLY` state flag answers its messages here (`CActionMover::SendActMsg`). Flying motions
//! are the ride's `use_motion + MTA_*`, always `MOP_FIXED`. Like the original the table ends with `return 1`:
//! a message without a case (sit, walk, combat mode, ...) is "accepted" and does nothing.
//!
//! Without ride parameters (`params.ride` absent, the original logged an error and dereferenced NULL) the
//! messages that need the ride (`STAND`, `FORWARD`, `LTURN`, `RTURN`, `STOP_TURN`, `ATK1`, `ATK_MAGIC1`)
//! answer `0` and raise [`SimEvent::RideMissing`] once; the messages that do not (`ACC_*`, `MODE_*`, the look
//! states, `STOP`, `DIE`) work, so a flyer can always be dismounted.

use super::msg_ground::mode_ground;
use super::{ActMsg, send_act_msg_cx};
use crate::anim::select::set_motion;
use crate::bounds::{is_range_obj, radius};
use crate::mover::{RideParams, SimMover};
use crate::step::{Cx, SimEvent};
use crate::types::*;
use flyff_common::ObjId;

/// The ride of `m`, or the `RideMissing` refusal (latched: the event is raised once per mount).
fn need_ride(m: &mut SimMover, cx: &mut Cx<'_>) -> Option<RideParams> {
    match m.params.ride {
        Some(r) => Some(r),
        None => {
            report_missing(m, cx);
            None
        }
    }
}

/// Raises `RideMissing` the first time a flyer without ride parameters is processed.
pub(crate) fn report_missing(m: &mut SimMover, cx: &mut Cx<'_>) {
    if !m.act.ride_missing_sent {
        m.act.ride_missing_sent = true;
        cx.emit(SimEvent::RideMissing);
    }
}

/// `SetMotion(ride.use_motion + mta, lp, MOP_FIXED)`; when the motion changed the ride model restarts its
/// animation (`m_pRide->m_fFrameCurrent = 0`, client event).
pub(super) fn ride_motion(
    m: &mut SimMover,
    cx: &mut Cx<'_>,
    ride: &RideParams,
    mta: u32,
    lp: u32,
) -> bool {
    let changed = set_motion(m, cx, ride.use_motion.wrapping_add(mta), lp, MOP_FIXED);
    if changed {
        cx.emit(SimEvent::RideFrameReset);
    }
    changed
}

/// `ProcessActMsg2(msg, p1..p5)`: the flying table (Plan/31 §6.3).
pub fn fly(m: &mut SimMover, cx: &mut Cx<'_>, am: ActMsg) -> ActResult {
    let [p1, p2, p3, _p4, _p5] = am.p;
    match am.msg {
        OBJMSG_STAND => {
            let mv = m.act.state.move_();
            if mv == OBJSTA_FMOVE || mv == OBJSTA_BMOVE {
                m.reset_state(cx, OBJSTA_MOVE_ALL);
            }
            if m.act.state.move_() == OBJSTA_STAND {
                return ACT_IGNORED;
            }
            if m.act.state.is_act_attack() {
                return ACT_DROP;
            }
            if m.act.state.is_act_turn() {
                return ACT_IGNORED;
            }
            let Some(ride) = need_ride(m, cx) else {
                return ACT_IGNORED;
            };
            m.set_move_state(cx, OBJSTA_STAND);
            ride_motion(m, cx, &ride, MTA_FSTAND1, ANILOOP_LOOP);
            ACT_OK
        }
        OBJMSG_STOP | OBJMSG_ASTOP => {
            let mv = m.act.state.move_();
            if mv == OBJSTA_FMOVE || mv == OBJSTA_BMOVE {
                m.clear_dest();
                send_act_msg_cx(m, cx, ActMsg::new(OBJMSG_ACC_STOP));
                m.reset_state(cx, OBJSTA_MOVE_ALL);
            }
            ACT_OK
        }
        OBJMSG_FORWARD => {
            if m.act.state.move_() == OBJSTA_FMOVE {
                return ACT_IGNORED;
            }
            let Some(ride) = need_ride(m, cx) else {
                return ACT_IGNORED;
            };
            m.set_move_state(cx, OBJSTA_FMOVE);
            if m.act.state.is_act_turn() || m.act.state.is_act_attack() {
                return ACT_OK;
            }
            set_motion(
                m,
                cx,
                ride.use_motion.wrapping_add(MTA_FRUNNING1),
                ANILOOP_LOOP,
                MOP_FIXED,
            );
            ACT_OK
        }
        OBJMSG_LTURN | OBJMSG_RTURN => {
            let left = am.msg == OBJMSG_LTURN;
            let (want, mta) = if left {
                (OBJSTA_LTURN, MTA_FLTURN1)
            } else {
                (OBJSTA_RTURN, MTA_FRTURN1)
            };
            let turn = p1 as f32 / 100.0; // BOUNDS: turn rates are hundredths of a degree
            if m.act.state.turn() == want && m.act.turn_angle == turn {
                return ACT_IGNORED;
            }
            let Some(ride) = need_ride(m, cx) else {
                return ACT_IGNORED;
            };
            m.act.turn_angle = turn;
            m.set_turn_state(cx, want);
            if m.act.state.is_act_attack() {
                return ACT_IGNORED;
            }
            if turn > 1.0 {
                set_motion(
                    m,
                    cx,
                    ride.use_motion.wrapping_add(mta),
                    ANILOOP_LOOP,
                    MOP_FIXED,
                );
            }
            ACT_OK
        }
        OBJMSG_STOP_TURN => {
            if m.act.state.turn() == 0 {
                return ACT_IGNORED;
            }
            let Some(ride) = need_ride(m, cx) else {
                return ACT_IGNORED;
            };
            m.act.turn_angle = 0.0;
            m.set_turn_state(cx, 0);
            if m.act.state.is_act_attack() {
                return ACT_IGNORED;
            }
            let mta = if m.act.state.move_() == OBJSTA_FMOVE {
                MTA_FRUNNING1
            } else {
                MTA_FSTAND1
            };
            set_motion(
                m,
                cx,
                ride.use_motion.wrapping_add(mta),
                ANILOOP_LOOP,
                MOP_FIXED,
            );
            ACT_OK
        }
        OBJMSG_LOOKUP => {
            if m.act.state.look() == OBJSTA_LOOKUP {
                return ACT_IGNORED;
            }
            // KEEP-QUIRK (the original `SetTurnState(OBJSTA_LOOKUP)`): the TURN group is reset and the LOOKUP
            // bit is OR-ed in next to it, so a turn in progress stops and `IsActTurn` stays false
            m.set_turn_state(cx, OBJSTA_LOOKUP);
            ACT_OK
        }
        OBJMSG_LOOKDOWN => {
            if m.act.state.look() == OBJSTA_LOOKDOWN {
                return ACT_IGNORED;
            }
            m.set_look_state(cx, OBJSTA_LOOKDOWN);
            ACT_OK
        }
        OBJMSG_STOP_LOOK => {
            if m.act.state.look() == 0 {
                return ACT_IGNORED;
            }
            m.set_look_state(cx, 0);
            ACT_OK
        }
        OBJMSG_ATK1 => atk1(m, cx, p1),
        OBJMSG_ATK_MAGIC1 => atk_magic1(m, cx, p1),
        OBJMSG_JUMP => ACT_OK,                              // no-op
        OBJMSG_DAMAGE | OBJMSG_DAMAGE_FORCE => ACT_IGNORED, // doc 17 owns the damage rules (§6.2)
        OBJMSG_DIE => {
            // `SendActMsg(STAND)`: the flying table (the position is where the mover died)
            send_act_msg_cx(m, cx, ActMsg::new(OBJMSG_STAND));
            m.reset_state(cx, OBJSTA_TURN_ALL);
            m.set_state(cx, OBJSTA_DMG_ALL, OBJSTA_DEAD);
            m.act.delta.x = 0.0;
            m.act.delta.z = 0.0;
            set_motion(m, cx, MTI_DIE1, ANILOOP_CONT, 0);
            let aid = ObjId(p2 as u32); // BOUNDS: ids travel as i32 bit patterns
            let attacker = if cx.env.lookup.snapshot(aid).is_some() {
                aid
            } else {
                ObjId::NONE
            };
            cx.emit(SimEvent::Died { attacker });
            if cx.server() && !m.is_player() {
                m.act.dead_cnt = 60 * 3;
            }
            ACT_OK
        }
        OBJMSG_ACC_START => {
            // the fuel check of the original is commented out: thrust is never refused here (the server
            // answers `SetFuel` and the client stops at 0 fuel)
            m.act.flags.add(OBJSTAF_ACC);
            ACT_OK
        }
        OBJMSG_ACC_STOP => {
            m.act.flags.remove(OBJSTAF_ACC);
            ACT_OK
        }
        OBJMSG_MODE_GROUND => mode_ground(m, cx, p3),
        OBJMSG_MODE_TURBO_ON => {
            if m.act.flags.has(OBJSTAF_TURBO) || m.tm_acc_fuel <= 0 {
                return ACT_IGNORED;
            }
            m.act.flags.add(OBJSTAF_TURBO);
            ACT_OK
        }
        OBJMSG_MODE_TURBO_OFF => {
            if !m.act.flags.has(OBJSTAF_TURBO) {
                return ACT_IGNORED;
            }
            m.act.flags.remove(OBJSTAF_TURBO);
            ACT_OK
        }
        // DROP: only the dropped flying ghost replay used it
        OBJMSG_TURNMOVE => ACT_IGNORED,
        // every other message falls through the switch to `return 1`
        _ => ACT_OK,
    }
}

/// `OBJMSG_ATK1` of a flyer (p1 = target): the flying melee swing. The target must exist and live; the
/// active client mover must be within 3 m (`IsRangeObj`), the server refuses a steal. Starts `ATK1` with the
/// ride's `FATK1` motion and, for the active client mover, asks the host to send the melee request
/// (`SendMeleeAttack2`, event `LocalAttack`).
fn atk1(m: &mut SimMover, cx: &mut Cx<'_>, target: i32) -> ActResult {
    if m.act.state.atk() == OBJSTA_ATK1 {
        return ACT_IGNORED;
    }
    let id = ObjId(target as u32); // BOUNDS: ids travel as i32 bit patterns
    m.act.objid_hit = id;
    let Some(t) = cx.env.lookup.snapshot(id).filter(|t| t.alive) else {
        return ACT_DROP;
    };
    if cx.client()
        && m.is_active
        && !is_range_obj(
            m.pos,
            radius(&m.params.bounds, m.params.scale),
            t.pos,
            t.radius,
            3.0,
        )
    {
        return ACT_IGNORED;
    }
    if cx.server() && cx.rules.is_steal(m.id, id) {
        return ACT_DROP;
    }
    let Some(ride) = need_ride(m, cx) else {
        return ACT_IGNORED;
    };
    m.act.flags.add(OBJSTAF_COMBAT);
    m.set_state(cx, OBJSTA_ATK_ALL, OBJSTA_ATK1);
    set_motion(
        m,
        cx,
        ride.use_motion.wrapping_add(MTA_FATK1),
        ANILOOP_1PLAY,
        MOP_NO_TRANS | MOP_SWDFORCE | MOP_FIXED,
    );
    cx.emit(SimEvent::JJim { target: id });
    cx.emit(SimEvent::RemoveInvisible);
    if cx.client() && m.is_active {
        cx.emit(SimEvent::LocalAttack {
            msg: OBJMSG_ATK1,
            target: id,
            flags: 0,
        });
    }
    ACT_OK
}

/// `OBJMSG_ATK_MAGIC1` of a flyer (p1 = target): the wand swing.
fn atk_magic1(m: &mut SimMover, cx: &mut Cx<'_>, target: i32) -> ActResult {
    if m.act.state.is_action() {
        return ACT_IGNORED;
    }
    if m.act.state.is_act_attack() {
        return ACT_IGNORED;
    }
    let Some(ride) = need_ride(m, cx) else {
        return ACT_IGNORED;
    };
    let id = ObjId(target as u32); // BOUNDS: ids travel as i32 bit patterns
    if cx.env.lookup.snapshot(id).is_none() {
        return ACT_BAD_TARGET;
    }
    if cx.server() && cx.rules.is_steal(m.id, id) {
        return ACT_DROP;
    }
    if m.act.state.is_act_damage() {
        m.reset_state(cx, OBJSTA_DMG_ALL);
    }
    m.act.flags.add(OBJSTAF_COMBAT);
    send_act_msg_cx(m, cx, ActMsg::new(OBJMSG_STOP_TURN));
    send_act_msg_cx(m, cx, ActMsg::new(OBJMSG_STAND));
    m.set_state(cx, OBJSTA_ATK_ALL, OBJSTA_ATK_MAGIC1);
    set_motion(
        m,
        cx,
        ride.use_motion.wrapping_add(MTA_FATK1),
        ANILOOP_1PLAY,
        MOP_NO_TRANS | MOP_SWDFORCE | MOP_FIXED,
    );
    cx.emit(SimEvent::JJim { target: id });
    cx.emit(SimEvent::RemoveInvisible);
    ACT_OK
}
