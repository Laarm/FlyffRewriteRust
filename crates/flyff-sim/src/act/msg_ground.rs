//! The ground action message table (`ProcessActMsg1`, `src:_AIInterface/ActionMoverMsg.cpp:101-1170`;
//! Plan/31 §6.1-6.2; WP-SIM-06/08/19).
//!
//! `send_act_msg` returns `1` when the message executed, `0` when it was ignored or the mover is busy
//! (the server then QUEUES attack messages), and the negative refusal codes of §6.1. ATK1..ATK4 return
//! their attack flags (`AF_GENERIC`/`AF_MISS`) instead of `1`.
//!
//! A mover with the `FLY` state flag answers its messages in the flying table (`act::msg_fly`, WP-SIM-25);
//! the two messages that cross the boundary live here: `MODE_FLY` (a row of the ground table: boards a
//! ride) and `MODE_GROUND` (a row of the flying table: dismounts).

use super::ActMsg;
use crate::anim::select::{set_blending, set_motion};
use crate::mover::SimMover;
use crate::num::{Vec3, ftoi_x86, get_degree, set_angle_quantized};
use crate::step::{Cx, MoverSnapshot, SimEvent};
use crate::types::*;
use flyff_common::ObjId;

fn target(cx: &Cx<'_>, id: i32) -> Option<MoverSnapshot> {
    cx.env.lookup.snapshot(ObjId(id as u32)) // BOUNDS: ids travel as i32 bit patterns
}

/// Client active mover awaiting a server answer (`REQ_USESKILL`/`REQ_MOTION`): actions are refused.
fn active_req(m: &SimMover, cx: &Cx<'_>, flags: u32) -> bool {
    cx.client() && m.is_active && m.req_flags & flags != 0
}

fn root(m: &SimMover) -> bool {
    m.params.chr_state & defs::CHS_LOOT != 0
}

fn no_move(m: &SimMover) -> bool {
    m.mvrf.has(MVRF_NOMOVE)
}

/// "DmgClr": a plain `DMG` state is cleared; any other damage state refuses with `r`.
fn dmg_clr(m: &mut SimMover, cx: &mut Cx<'_>, r: ActResult) -> Option<ActResult> {
    if m.act.state.dmg() == OBJSTA_DMG {
        m.reset_state(cx, OBJSTA_DMG_ALL);
        None
    } else if m.act.state.is_act_damage() {
        Some(r)
    } else {
        None
    }
}

/// `CActionMover::SendActMsg`: dispatches on the FLY flag to the flying table (`ProcessActMsg2`) or the
/// ground table (`ProcessActMsg1`).
pub fn send_act_msg_cx(m: &mut SimMover, cx: &mut Cx<'_>, am: ActMsg) -> ActResult {
    if m.act.flags.fly() {
        return super::msg_fly::fly(m, cx, am);
    }
    ground(m, cx, am)
}

/// `OBJMSG_MODE_FLY` (ground table, Plan/31 §6.2): boards the ride the owner put into `params.ride`.
/// Without `p3` (force) the message is refused while sitting, acting, attacking, rooted or standing in a
/// `HATTR_NOFLY` cell; otherwise the FLY flag is added, the fuel is initialised once from the ride
/// (`m_nFuel == -1`), every state is cleared and `RideChanged` tells the renderer to load the model.
fn mode_fly(m: &mut SimMover, cx: &mut Cx<'_>, ride_item: i32, force: i32) -> ActResult {
    if force == 0 {
        if m.act.flags.sit()
            || m.act.state.is_action()
            || m.act.state.is_act_attack()
            || no_move(m)
            || root(m)
        {
            return ACT_IGNORED;
        }
        if cx.env.world.height_attr(m.pos.x, m.pos.z) == HATTR_NOFLY {
            return ACT_IGNORED;
        }
    }
    m.act.flags.add(OBJSTAF_FLY);
    m.act.ride_missing_sent = false;
    if let Some(r) = m.params.ride
        && m.fuel == -1
    {
        m.fuel = r.f_fuel_re_max;
    }
    m.clear_state(cx);
    cx.emit(SimEvent::RideChanged {
        item: Some(ride_item as u32), // BOUNDS: item ids travel as bit patterns
    });
    ACT_OK
}

/// `OBJMSG_MODE_GROUND` (flying table, Plan/31 §6.3): dismounts. Without `p3` (force) it is refused over a
/// `HATTR_NOWALK` cell; otherwise FLY and ACC are removed, every state is cleared, the pitch is levelled
/// and `RideChanged(None)` tells the renderer to drop the ride model.
pub(super) fn mode_ground(m: &mut SimMover, cx: &mut Cx<'_>, force: i32) -> ActResult {
    if force == 0 && cx.env.world.height_attr(m.pos.x, m.pos.z) == HATTR_NOWALK {
        return ACT_IGNORED;
    }
    m.act.ride_missing_sent = false;
    m.act.flags.remove(OBJSTAF_FLY);
    m.act.flags.remove(OBJSTAF_ACC);
    m.clear_state(cx);
    m.angle_x = 0.0;
    cx.emit(SimEvent::RideChanged { item: None });
    ACT_OK
}

/// Pre-step shared by `STAND`, `STOP` and `ASTOP`: moving forward/backward is reset and the horizontal
/// velocity cleared (flying NPCs also lose pitch and vertical velocity).
fn stop_moving(m: &mut SimMover, cx: &mut Cx<'_>) {
    let mv = m.act.state.move_();
    if mv == OBJSTA_FMOVE || mv == OBJSTA_BMOVE {
        m.reset_state(cx, OBJSTA_MOVE_ALL);
        m.act.delta.x = 0.0;
        m.act.delta.z = 0.0;
        if m.params.flying_npc {
            m.act.delta.y = 0.0;
            m.angle_x = 0.0;
        }
    }
}

fn ground(m: &mut SimMover, cx: &mut Cx<'_>, am: ActMsg) -> ActResult {
    let [p1, p2, p3, p4, _p5] = am.p;
    match am.msg {
        OBJMSG_STAND => {
            stop_moving(m, cx);
            let st = m.act.state;
            if st.move_() == OBJSTA_STAND || st.move_() == OBJSTA_PICKUP {
                return ACT_IGNORED;
            }
            if st.is_act_jump() {
                return ACT_BAD_TARGET;
            }
            if st.is_act_attack() {
                return ACT_DROP;
            }
            if st.is_act_damage() {
                return ACT_DAMAGED;
            }
            if st.is_die() {
                return ACT_DEAD;
            }
            if st.is_action() {
                return ACT_IGNORED;
            }
            m.set_move_state(cx, OBJSTA_STAND);
            set_motion(m, cx, MTI_STAND, ANILOOP_LOOP, 0);
            m.act.flags.remove(OBJSTAF_ETC);
            ACT_OK
        }
        OBJMSG_STOP | OBJMSG_ASTOP => {
            stop_moving(m, cx);
            m.clear_dest();
            ACT_IGNORED // always 0: the key-up path of ControlGround relies on it
        }
        OBJMSG_SITDOWN => {
            if m.act.flags.sit() {
                return ACT_IGNORED;
            }
            if p3 == 0 {
                let st = m.act.state;
                if st.is_act_jump() || st.is_act_attack() || st.is_act_damage() || st.is_action() {
                    return ACT_IGNORED;
                }
                if active_req(m, cx, REQ_USESKILL) {
                    return ACT_IGNORED;
                }
            }
            m.act.flags.add(OBJSTAF_SIT);
            send_act_msg_cx(m, cx, ActMsg::new(OBJMSG_STOP));
            set_motion(m, cx, MTI_SIT, ANILOOP_CONT, 0);
            m.set_move_state(cx, OBJSTA_SIT);
            ACT_OK
        }
        OBJMSG_STANDUP => {
            if !m.act.flags.sit() {
                return ACT_IGNORED;
            }
            if p3 == 0 {
                let st = m.act.state;
                if st.is_act_jump() || st.is_act_attack() || st.is_act_damage() || st.is_action() {
                    return ACT_IGNORED;
                }
                if active_req(m, cx, REQ_USESKILL) {
                    return ACT_IGNORED;
                }
            }
            m.set_move_state(cx, OBJSTA_SIT);
            m.act.flags.add(OBJSTAF_SIT);
            set_motion(m, cx, MTI_GETUP, ANILOOP_CONT, 0);
            ACT_OK
        }
        OBJMSG_PICKUP => {
            let st = m.act.state;
            if m.act.flags.sit()
                || st.is_action()
                || st.is_act_jump()
                || st.is_act_attack()
                || st.is_act_damage()
            {
                return ACT_IGNORED;
            }
            if active_req(m, cx, REQ_USESKILL | REQ_MOTION) {
                return ACT_IGNORED;
            }
            if st.move_() == OBJSTA_PICKUP {
                return ACT_IGNORED;
            }
            m.set_move_state(cx, OBJSTA_PICKUP);
            set_motion(m, cx, MTI_PICKUP, ANILOOP_1PLAY, MOP_FIXED);
            ACT_OK
        }
        OBJMSG_COLLECT => {
            m.clear_state(cx);
            m.reset_state(cx, OBJSTA_ACTION_ALL);
            m.set_state(cx, OBJSTA_ACTION_ALL, OBJSTA_COLLECT);
            m.mvrf.add(MVRF_NOACTION);
            set_motion(m, cx, MTI_COLLECT, ANILOOP_LOOP, MOP_FIXED);
            cx.emit(SimEvent::RemoveInvisible);
            ACT_OK
        }
        OBJMSG_STUN => {
            m.set_state(cx, OBJSTA_ACTION_ALL, OBJSTA_STUN);
            m.mvrf.add(MVRF_NOACTION);
            m.stun_cnt = p1 as i16; // BOUNDS: `(short)nParam1` truncation
            if m.act.state.bits() == OBJSTA_STAND {
                set_motion(m, cx, MTI_GROGGY, ANILOOP_LOOP, MOP_FIXED);
            }
            ACT_OK
        }
        OBJMSG_FALL => {
            if m.act.state.is_act_jump() {
                return ACT_IGNORED;
            }
            if m.params.mode & defs::MATCHLESS_MODE != 0 {
                return ACT_IGNORED;
            }
            m.mvrf.add(MVRF_NOACTION);
            if m.act.state.bits() != OBJSTA_DMG_FLY {
                m.set_state(cx, OBJSTA_DMG_ALL, OBJSTA_DMG_FLY);
                set_motion(m, cx, MTI_DMGFLY, ANILOOP_LOOP, MOP_FIXED);
            }
            ACT_OK
        }
        OBJMSG_TURNMOVE | OBJMSG_TURNMOVE2 => {
            let two = am.msg == OBJMSG_TURNMOVE2;
            if m.act.state.move_() == OBJSTA_FMOVE && ftoi_x86(m.angle) == p1 {
                return ACT_IGNORED; // (KEEP-QUIRK: TURNMOVE2 tests FMOVE as well)
            }
            if no_move(m) || root(m) {
                return ACT_IGNORED;
            }
            if two {
                if m.act.state.is_action() {
                    return ACT_IGNORED;
                }
                if m.act.state.dmg_fly_all() {
                    return ACT_IGNORED;
                }
                if m.act.state.is_act_jump() {
                    return ACT_BAD_TARGET;
                }
                if m.act.state.is_act_attack() {
                    return ACT_DROP;
                }
            } else {
                if m.act.state.dmg_fly_all() {
                    return ACT_IGNORED;
                }
                if m.act.state.is_act_attack() {
                    return ACT_DROP;
                }
            }
            if let Some(r) = dmg_clr(m, cx, ACT_DAMAGED) {
                return r;
            }
            m.angle = set_angle_quantized(p1 as f32);
            if m.params.flying_npc {
                m.angle_x = p2 as f32;
            }
            send_act_msg_cx(m, cx, ActMsg::new(OBJMSG_STOP_TURN));
            send_act_msg_cx(
                m,
                cx,
                ActMsg::new(if two { OBJMSG_BACKWARD } else { OBJMSG_FORWARD }),
            );
            ACT_OK
        }
        OBJMSG_FORWARD => {
            let st = m.act.state;
            if st.is_act_jump() {
                return ACT_BAD_TARGET;
            }
            if st.is_act_attack() {
                return ACT_DROP;
            }
            if let Some(r) = dmg_clr(m, cx, ACT_DAMAGED) {
                return r;
            }
            if m.act.state.move_() == OBJSTA_FMOVE || m.act.flags.sit() || no_move(m) || root(m) {
                return ACT_IGNORED;
            }
            m.set_move_state(cx, OBJSTA_FMOVE);
            if m.act.flags.is_walk() {
                set_motion(m, cx, MTI_WALK, ANILOOP_LOOP, 0);
            } else if set_motion(m, cx, MTI_RUN, ANILOOP_LOOP, 0) {
                set_blending(m, true);
            }
            cx.emit(SimEvent::BeganMove);
            ACT_OK
        }
        OBJMSG_STOP_RUN => {
            let st = m.act.state;
            if st.is_act_jump() {
                return ACT_BAD_TARGET;
            }
            if st.is_act_attack() {
                return ACT_DROP;
            }
            if let Some(r) = dmg_clr(m, cx, ACT_DAMAGED) {
                return r;
            }
            if m.act.state.move_() == OBJSTA_STOP_RUN || m.act.flags.sit() || no_move(m) || root(m)
            {
                return ACT_IGNORED;
            }
            m.set_move_state(cx, OBJSTA_STOP_RUN);
            ACT_OK
        }
        OBJMSG_BACKWARD => {
            let st = m.act.state;
            if st.is_act_jump() {
                return ACT_BAD_TARGET;
            }
            if st.is_act_attack() {
                return ACT_DROP;
            }
            if st.is_act_damage() {
                return ACT_DAMAGED;
            }
            if st.move_() == OBJSTA_BMOVE
                || m.act.flags.sit()
                || st.is_action()
                || no_move(m)
                || root(m)
            {
                return ACT_IGNORED;
            }
            m.set_move_state(cx, OBJSTA_BMOVE);
            set_motion(m, cx, MTI_WALK, ANILOOP_LOOP, 0);
            ACT_OK
        }
        OBJMSG_LTURN | OBJMSG_RTURN => {
            let want = if am.msg == OBJMSG_LTURN {
                OBJSTA_LTURN
            } else {
                OBJSTA_RTURN
            };
            let st = m.act.state;
            if st.is_act_jump() {
                return ACT_BAD_TARGET;
            }
            if st.is_act_attack() {
                return ACT_DROP;
            }
            if let Some(r) = dmg_clr(m, cx, ACT_DAMAGED) {
                return r;
            }
            if m.act.state.turn() == want
                || m.act.flags.sit()
                || m.act.state.is_action()
                || no_move(m)
                || root(m)
            {
                return ACT_IGNORED;
            }
            m.set_turn_state(cx, want);
            ACT_OK
        }
        OBJMSG_STOP_TURN => {
            if m.act.state.turn() == 0 {
                return ACT_IGNORED;
            }
            m.set_turn_state(cx, 0);
            ACT_OK
        }
        OBJMSG_LOOKUP | OBJMSG_LOOKDOWN => {
            let want = if am.msg == OBJMSG_LOOKUP {
                OBJSTA_LOOKUP
            } else {
                OBJSTA_LOOKDOWN
            };
            let st = m.act.state;
            if st.is_act_jump() {
                return ACT_BAD_TARGET;
            }
            if st.is_act_attack() {
                return ACT_DROP;
            }
            if let Some(r) = dmg_clr(m, cx, ACT_DAMAGED) {
                return r;
            }
            // KEEP-QUIRK (KQ-13): the original compares the TURN group with LOOKUP/LOOKDOWN, never equal
            if m.act.state.turn() == want
                || m.act.flags.sit()
                || m.act.state.is_action()
                || no_move(m)
                || root(m)
            {
                return ACT_IGNORED;
            }
            m.set_look_state(cx, want);
            ACT_OK
        }
        OBJMSG_STOP_LOOK => {
            if m.act.state.look() == 0 {
                return ACT_IGNORED;
            }
            m.set_look_state(cx, 0);
            ACT_OK
        }
        OBJMSG_JUMP => {
            let st = m.act.state;
            if st.is_act_jump() {
                return ACT_IGNORED;
            }
            if st.is_act_attack() {
                return ACT_DROP;
            }
            if st.is_act_damage() {
                return ACT_DAMAGED;
            }
            if m.act.flags.sit() || st.is_action() || no_move(m) || root(m) {
                return ACT_IGNORED;
            }
            if active_req(m, cx, REQ_USESKILL | REQ_MOTION) {
                return ACT_IGNORED;
            }
            send_act_msg_cx(m, cx, ActMsg::new(OBJMSG_STOP_TURN));
            m.set_jump_state(cx, OBJSTA_SJUMP1);
            if m.act.state.bits() & OBJSTA_COLLECT == 0 {
                set_motion(m, cx, MTI_JUMP1, ANILOOP_1PLAY, 0);
            }
            set_blending(m, false);
            ACT_OK
        }
        OBJMSG_ATK_MAGIC1 => atk_magic1(m, cx, p1),
        OBJMSG_ATK_RANGE1 => atk_range1(m, cx, p1, p2),
        OBJMSG_ATK1 | OBJMSG_ATK2 | OBJMSG_ATK3 | OBJMSG_ATK4 => atk_combo(m, cx, am.msg, p1, p3),
        OBJMSG_SP_ATK1 | OBJMSG_SP_ATK2 => atk_special(m, cx, am.msg, p1, p3),
        OBJMSG_MELEESKILL => melee_skill(m, cx, p1, p2),
        OBJMSG_MAGICSKILL | OBJMSG_MAGICCASTING => magic_skill(m, cx, am.msg, p1, p2, p3, p4),
        OBJMSG_DAMAGE | OBJMSG_DAMAGE_FORCE => ACT_IGNORED, // doc 17 owns the damage rules (§6.2)
        OBJMSG_DIE => {
            let attacker = target(cx, p2).map(|t| (ObjId(p2 as u32), t.angle)); // BOUNDS: id bit pattern
            die_msg(m, cx, p1 as u32, attacker); // BOUNDS: message numbers are small
            ACT_OK
        }
        OBJMSG_APPEAR => {
            m.set_state(cx, OBJSTA_ACTION_ALL, OBJSTA_APPEAR);
            set_motion(m, cx, MTI_APPEAR2, ANILOOP_1PLAY, MOP_FIXED | MOP_NO_TRANS);
            ACT_OK
        }
        OBJMSG_MODE_COMBAT => {
            let st = m.act.state;
            if m.act.flags.combat()
                || st.is_act_jump()
                || st.is_act_attack()
                || st.is_act_damage()
                || m.act.flags.sit()
                || st.is_action()
            {
                return ACT_IGNORED;
            }
            m.act.flags.add(OBJSTAF_COMBAT);
            m.motion.id = u32::MAX;
            set_motion(m, cx, MTI_STAND, ANILOOP_LOOP, 0);
            ACT_OK
        }
        OBJMSG_MODE_PEACE => {
            let st = m.act.state;
            if !m.act.flags.combat()
                || st.is_act_jump()
                || st.is_act_attack()
                || st.is_act_damage()
                || m.act.flags.sit()
                || st.is_action()
            {
                return ACT_IGNORED;
            }
            m.act.flags.remove(OBJSTAF_COMBAT);
            m.motion.id = u32::MAX;
            set_motion(m, cx, MTI_STAND, ANILOOP_LOOP, 0);
            ACT_OK
        }
        OBJMSG_MODE_WALK => {
            if m.act.flags.sit() || m.act.state.is_action() || m.act.flags.is_walk() {
                return ACT_IGNORED;
            }
            m.act.flags.add(OBJSTAF_WALK);
            ACT_OK
        }
        OBJMSG_MODE_RUN => {
            if m.act.flags.sit() || m.act.state.is_action() || !m.act.flags.is_walk() {
                return ACT_IGNORED;
            }
            m.act.flags.remove(OBJSTAF_WALK);
            ACT_OK
        }
        OBJMSG_MODE_FLY => mode_fly(m, cx, p1, p3),
        OBJMSG_MOTION => {
            let st = m.act.state;
            if m.act.flags.sit()
                || st.is_action()
                || st.is_act_jump()
                || st.is_act_attack()
                || st.is_act_damage()
                || no_move(m)
                || root(m)
            {
                return ACT_IGNORED;
            }
            m.act.flags.add(OBJSTAF_ETC);
            m.act.motion_ex = p1;
            if p1 != -1 {
                set_motion(m, cx, p1 as u32, p2 as u32, MOP_FIXED); // BOUNDS: ids/modes travel as bit patterns
            }
            cx.emit(SimEvent::RemoveInvisible);
            ACT_OK
        }
        OBJMSG_RESURRECTION => {
            m.clear_state(cx);
            m.set_state(cx, OBJSTA_DMG_ALL, OBJSTA_RESURRECTION);
            set_motion(m, cx, MTI_ASS_RESURRECTION, ANILOOP_1PLAY, MOP_FIXED);
            ACT_OK
        }
        OBJMSG_TEMP | OBJMSG_TEMP2 | OBJMSG_TEMP3 | OBJMSG_LFORWARD | OBJMSG_RFORWARD => {
            ACT_IGNORED
        } // DROP
        // MODE_TURN, BASE_RADY, DESTPOS, DESTOBJ, ACC_START/STOP, MODE_GROUND, MODE_TURBO_*: no ground
        // handler, the switch falls through to `return 1` like the original
        _ => ACT_OK,
    }
}

/// The checks every attack start shares: `Act`, `NOATTACK`, `NO_ATTACK_MODE`.
fn face(m: &mut SimMover, t: Vec3) {
    m.angle = set_angle_quantized(get_degree(t, m.pos));
}

fn ensure_combat(m: &mut SimMover, cx: &mut Cx<'_>) {
    if !m.act.flags.combat() {
        send_act_msg_cx(m, cx, ActMsg::new(OBJMSG_MODE_COMBAT));
    }
}

fn atk_magic1(m: &mut SimMover, cx: &mut Cx<'_>, p1: i32) -> ActResult {
    if m.act.state.is_action() {
        return ACT_IGNORED;
    }
    if m.act.flags.sit() {
        m.reset_state(cx, OBJSTA_MOVE_ALL);
    }
    if m.act.state.is_act_attack() {
        return ACT_IGNORED;
    }
    if m.mvrf.has(MVRF_NOATTACK) || m.params.mode & defs::NO_ATTACK_MODE != 0 {
        return ACT_BAD_TARGET;
    }
    let Some(t) = target(cx, p1) else {
        return ACT_BAD_TARGET;
    };
    if !t.alive {
        return ACT_BAD_TARGET;
    }
    if cx.server() && cx.rules.is_steal(m.id, ObjId(p1 as u32)) {
        return ACT_BAD_TARGET;
    }
    if m.act.state.is_act_damage() {
        m.reset_state(cx, OBJSTA_DMG_ALL);
    }
    ensure_combat(m, cx);
    send_act_msg_cx(m, cx, ActMsg::new(OBJMSG_STOP_TURN));
    send_act_msg_cx(m, cx, ActMsg::new(OBJMSG_STAND));
    m.set_state(cx, OBJSTA_ATK_ALL, OBJSTA_ATK_MAGIC1);
    set_motion(m, cx, MTI_ATK1, ANILOOP_1PLAY, 0);
    cx.emit(SimEvent::JJim {
        target: ObjId(p1 as u32),
    });
    m.atk_cnt = 1;
    cx.emit(SimEvent::AttackStarted {
        target: ObjId(p1 as u32),
        msg: OBJMSG_ATK_MAGIC1,
    });
    ACT_OK
}

fn atk_range1(m: &mut SimMover, cx: &mut Cx<'_>, p1: i32, p2: i32) -> ActResult {
    if m.act.state.is_action() {
        return ACT_IGNORED;
    }
    if m.mvrf.has(MVRF_NOATTACK) || m.params.mode & defs::NO_ATTACK_MODE != 0 {
        return ACT_BAD_TARGET;
    }
    if m.act.flags.sit() {
        m.reset_state(cx, OBJSTA_MOVE_ALL);
    }
    if m.act.state.is_act_attack() {
        return ACT_IGNORED;
    }
    let Some(t) = target(cx, p1) else {
        return ACT_BAD_TARGET;
    };
    if !t.alive {
        return ACT_BAD_TARGET;
    }
    m.act.id_target = ObjId(p1 as u32);
    if cx.server() && cx.rules.is_steal(m.id, ObjId(p1 as u32)) {
        return ACT_BAD_TARGET;
    }
    if let Some(r) = dmg_clr(m, cx, ACT_BAD_TARGET) {
        return r;
    }
    ensure_combat(m, cx);
    m.set_state(cx, OBJSTA_ATK_ALL, OBJSTA_ATK_RANGE1);
    set_motion(m, cx, p2 as u32, ANILOOP_1PLAY, 0); // BOUNDS: motion ids travel as bit patterns
    cx.emit(SimEvent::JJim {
        target: ObjId(p1 as u32),
    });
    cx.emit(SimEvent::AttackStarted {
        target: ObjId(p1 as u32),
        msg: OBJMSG_ATK_RANGE1,
    });
    m.atk_cnt = 1;
    ACT_OK
}

fn atk_combo(m: &mut SimMover, cx: &mut Cx<'_>, msg: ObjMsg, p1: i32, p3: i32) -> ActResult {
    if m.act.state.is_action() {
        return ACT_IGNORED;
    }
    if m.mvrf.has(MVRF_NOATTACK) || m.params.mode & defs::NO_ATTACK_MODE != 0 {
        return ACT_DROP;
    }
    // wands and bows do not melee ([S] always, [C] only the active mover)
    if (cx.server() || m.is_active)
        && let Some(st) = m.params.stance
        && (st.kind3 == defs::IK3_WAND || st.kind3 == defs::IK3_BOW)
    {
        return ACT_DROP;
    }
    let Some(t) = target(cx, p1) else {
        return ACT_BAD_TARGET;
    };
    m.act.id_target = ObjId(p1 as u32);
    if cx.server() && cx.rules.is_steal(m.id, ObjId(p1 as u32)) {
        return ACT_DROP;
    }
    if !t.alive {
        return ACT_BAD_TARGET;
    }
    if m.act.state.is_act_attack() {
        return ACT_IGNORED;
    }
    if m.act.flags.sit() {
        m.reset_state(cx, OBJSTA_MOVE_ALL);
    }
    if let Some(r) = dmg_clr(m, cx, ACT_DAMAGED) {
        return r;
    }
    ensure_combat(m, cx);
    send_act_msg_cx(m, cx, ActMsg::new(OBJMSG_STOP_TURN));
    send_act_msg_cx(m, cx, ActMsg::new(OBJMSG_STAND));
    let option = MOP_SWDFORCE | MOP_NO_TRANS;
    if m.is_player() {
        m.ani_speed = m.params.attack_speed;
    }
    let (state, mti) = match msg {
        OBJMSG_ATK1 => (OBJSTA_ATK1, MTI_ATK1),
        OBJMSG_ATK2 => (OBJSTA_ATK2, MTI_ATK2),
        OBJMSG_ATK3 => (OBJSTA_ATK3, MTI_ATK3),
        _ => (OBJSTA_ATK4, MTI_ATK4),
    };
    m.set_state(cx, OBJSTA_ATK_ALL, state);
    set_motion(m, cx, mti, ANILOOP_1PLAY, option);
    face(m, t.pos);
    cx.emit(SimEvent::JJim {
        target: ObjId(p1 as u32),
    });
    m.atk_cnt = 1;
    cx.emit(SimEvent::AttackStarted {
        target: ObjId(p1 as u32),
        msg,
    });
    m.act.objid_hit = ObjId(p1 as u32);
    m.act.atk_flags = (p3 as u32) & 0xFFFF; // LOWORD
    if cx.server() {
        if m.act.atk_flags == 0 {
            m.act.atk_flags = if cx.rules.attack_result(m.id, ObjId(p1 as u32), option) {
                AF_GENERIC
            } else {
                AF_MISS
            };
        }
    } else {
        m.act.atk_flags = AF_GENERIC;
    }
    m.act.atk_flags as ActResult // BOUNDS: LOWORD flags
}

fn atk_special(m: &mut SimMover, cx: &mut Cx<'_>, msg: ObjMsg, p1: i32, p3: i32) -> ActResult {
    let Some(t) = target(cx, p1) else {
        return ACT_BAD_TARGET;
    };
    if !t.alive {
        return ACT_BAD_TARGET;
    }
    if m.act.state.is_act_attack() {
        return ACT_IGNORED;
    }
    if m.mvrf.has(MVRF_NOATTACK) {
        return ACT_IGNORED;
    }
    if let Some(r) = dmg_clr(m, cx, ACT_DAMAGED) {
        return r;
    }
    ensure_combat(m, cx);
    send_act_msg_cx(m, cx, ActMsg::new(OBJMSG_STOP_TURN));
    send_act_msg_cx(m, cx, ActMsg::new(OBJMSG_STAND));
    m.set_state(cx, OBJSTA_ATK_ALL, OBJSTA_SP_ATK1); // also for SP_ATK2 (KEEP-QUIRK)
    set_motion(m, cx, p3 as u32, ANILOOP_1PLAY, MOP_SWDFORCE); // BOUNDS: motion ids travel as bit patterns
    face(m, t.pos);
    cx.emit(SimEvent::AttackStarted {
        target: ObjId(p1 as u32),
        msg,
    });
    ACT_OK
}

fn melee_skill(m: &mut SimMover, cx: &mut Cx<'_>, p1: i32, p2: i32) -> ActResult {
    if m.act.state.is_action() {
        return ACT_DROP;
    }
    if m.mvrf.has(MVRF_NOATTACK) {
        return ACT_DROP;
    }
    let Some(_t) = target(cx, p2) else {
        return ACT_DROP;
    };
    if cx.server() {
        // KEEP-QUIRK: `state & ATK_RANGE1 (0x60000)` is a bit test, so ATK2/ATK3/ATK4/RANGE3 match too
        if m.act.state.bits() & OBJSTA_ATK_RANGE1 != 0 {
            m.reset_state(cx, OBJSTA_ATK_RANGE1);
        }
        if m.act.state.is_act_attack() {
            return ACT_DROP;
        }
    }
    m.motion.id = u32::MAX;
    m.clear_state(cx);
    m.set_state(cx, OBJSTA_ATK_ALL, OBJSTA_ATK_MELEESKILL);
    let lp = if matches!(
        m.skill.skill,
        defs::SI_BLD_DOUBLESW_SILENTSTRIKE | defs::SI_ACR_YOYO_COUNTER
    ) {
        ANILOOP_CONT
    } else {
        ANILOOP_1PLAY
    };
    let mut motion = p1 as u32; // BOUNDS: ids travel as bit patterns
    if cx.client() && m.params.disguised {
        motion = MTI_ATK1;
    }
    set_motion(m, cx, motion, lp, MOP_SWDFORCE | MOP_NO_TRANS | MOP_FIXED);
    cx.emit(SimEvent::AttackStarted {
        target: ObjId(p2 as u32),
        msg: OBJMSG_MELEESKILL,
    });
    m.act.motion_hit_count = 0;
    ACT_OK
}

fn magic_skill(
    m: &mut SimMover,
    cx: &mut Cx<'_>,
    msg: ObjMsg,
    p1: i32,
    p2: i32,
    p3: i32,
    p4: i32,
) -> ActResult {
    if m.act.state.is_action() {
        return ACT_DROP;
    }
    if m.mvrf.has(MVRF_NOATTACK) {
        return ACT_DROP;
    }
    let Some(_t) = target(cx, p2) else {
        return ACT_DROP;
    };
    if cx.server() && m.act.state.is_act_attack() {
        return ACT_DROP;
    }
    m.motion.id = u32::MAX;
    m.clear_state(cx);
    m.act.casting_time = p3.wrapping_mul(4);
    m.skill.casting_end_frame = cx.frame().wrapping_add((p3.max(0) as u64) * 4);
    m.skill.casting_skill = p4 as u32; // BOUNDS: skill ids are positive
    if m.act.casting_time > 0 {
        m.act.count = 0;
        m.set_state(cx, OBJSTA_ATK_ALL, OBJSTA_ATK_CASTING1);
    } else {
        m.set_state(cx, OBJSTA_ATK_ALL, OBJSTA_ATK_MAGICSKILL);
    }
    let mut motion = p1 as u32; // BOUNDS: ids travel as bit patterns
    if cx.client() && msg == OBJMSG_MAGICSKILL && m.params.disguised {
        motion = MTI_ATK1;
    }
    set_motion(
        m,
        cx,
        motion,
        ANILOOP_1PLAY,
        MOP_SWDFORCE | MOP_NO_TRANS | MOP_FIXED,
    );
    cx.emit(SimEvent::AttackStarted {
        target: ObjId(p2 as u32),
        msg,
    });
    m.act.motion_hit_count = 0;
    ACT_OK
}

/// `OBJMSG_DIE` (Plan/31 §6.2): sit is reset, every state cleared, the horizontal velocity zeroed;
/// `DAMAGE_FORCE` enters `DMG_FLY` (with the knock-back `(145 degrees, 0.25)` away from the attacker when
/// one is known), everything else `DEAD`. Server NPCs start the 180-frame dead counter.
pub fn die_msg(m: &mut SimMover, cx: &mut Cx<'_>, cause: ObjMsg, attacker: Option<(ObjId, f32)>) {
    if m.act.flags.sit() {
        m.reset_state(cx, OBJSTA_MOVE_ALL);
    }
    m.clear_state(cx);
    m.act.delta.x = 0.0;
    m.act.delta.z = 0.0;
    cx.emit(SimEvent::RemoveInvisible);
    cx.emit(SimEvent::Died {
        attacker: attacker.map_or(ObjId::NONE, |(id, _)| id),
    });
    if cause == OBJMSG_DAMAGE_FORCE {
        m.set_state(cx, OBJSTA_DMG_ALL, OBJSTA_DMG_FLY);
        set_motion(m, cx, MTI_DMGFLY, ANILOOP_CONT, 0);
        if let Some((_, angle)) = attacker {
            super::damage::do_damage_fly(m, angle, 145.0, 0.25);
        }
    } else {
        m.set_state(cx, OBJSTA_DMG_ALL, OBJSTA_DEAD);
        set_motion(m, cx, MTI_DIE1, ANILOOP_CONT, 0);
    }
    if cx.server() && !m.is_player() {
        m.act.dead_cnt = 60 * 3;
    }
}
