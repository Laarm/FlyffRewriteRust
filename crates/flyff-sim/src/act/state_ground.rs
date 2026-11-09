//! Per-frame ground state processing (`ProcessState1`, `src:_AIInterface/ActionMoverState.cpp:32-1186`;
//! Plan/31 §7.1, §7.2, §7.4; WP-SIM-06/08/19).
//!
//! Called once per state group by `kin::process_action` with the group's value (`dw`) and the mover's
//! speed. Hits are detected on the frame clock (`anim::attr`), the game reacts to the events at the frame
//! barrier.

use super::{ActMsg, send_act_msg_cx};
use crate::anim::attr::{attrs_firing, is_attr_hit};
use crate::anim::select::{set_blending, set_motion};
use crate::mover::SimMover;
use crate::num::{angle_to_vector, angle_to_vector_xz, cosf, sinf, to_radian};
use crate::step::{Cx, HitKind, ShotKind, SimEvent};
use crate::types::*;
use flyff_common::ObjId;

fn end_frame(m: &SimMover) -> bool {
    m.motion.clock.end_frame
}

fn hit(m: &SimMover, cx: &Cx<'_>) -> bool {
    let clip = m.motion.clip.and_then(|c| cx.env.motions.clip(c));
    is_attr_hit(&m.motion.clock, clip, cx.env.flags.hit_attr_rule)
}

fn hit_count(m: &SimMover, cx: &Cx<'_>) -> usize {
    let clip = m.motion.clip.and_then(|c| cx.env.motions.clip(c));
    attrs_firing(&m.motion.clock, clip, MA_HIT, cx.env.flags.hit_attr_rule).len()
}

/// `ProcessState1(dw, speed)`: the behaviour of one state-group value for one frame.
pub fn process_state(m: &mut SimMover, cx: &mut Cx<'_>, dw: u32, speed: f32) {
    let theta = to_radian(m.angle);
    match dw {
        OBJSTA_STAND => {
            if m.act.state.bits() == OBJSTA_STAND {
                if m.act.flags.has(OBJSTAF_ETC) {
                    // the custom motion ends with its animation; client CONT/LOOP motions of the active
                    // mover end on a mouse click
                    let released = if cx.server() || m.motion.clock.lp & ANILOOP_1PLAY != 0 {
                        end_frame(m)
                    } else {
                        m.is_active && m.input.lmb_held && end_frame(m)
                    };
                    if released {
                        m.act.flags.remove(OBJSTAF_ETC);
                    }
                } else if end_frame(m) {
                    if m.params.base_motion_mode {
                        set_motion(m, cx, MTI_STAND, ANILOOP_LOOP, 0);
                    } else if cx.rng.random_below(5000) < 2 {
                        set_motion(m, cx, MTI_IDLE2, ANILOOP_1PLAY, 0);
                    } else if cx.rng.random_below(2000) < 2 {
                        set_motion(m, cx, MTI_IDLE1, ANILOOP_1PLAY, 0);
                    } else {
                        set_motion(m, cx, MTI_STAND, ANILOOP_LOOP, 0);
                    }
                }
            }
            if m.act.ground != 0 && m.act.delta.y <= 0.0 {
                m.act.delta.x = 0.0;
                m.act.delta.z = 0.0;
            }
        }
        OBJSTA_FMOVE => {
            if m.mvrf.has(MVRF_NOMOVE) || m.params.chr_state & defs::CHS_LOOT != 0 {
                return;
            }
            let walk = m.act.flags.is_walk();
            let v = if walk { speed / 4.0 } else { speed };
            if m.params.flying_npc {
                m.act.delta = angle_to_vector(m.angle, -m.angle_x, v);
            } else {
                m.act.delta.x = sinf(theta) * v;
                m.act.delta.z = -cosf(theta) * v;
            }
            if m.act.state.bits() != OBJSTA_FMOVE {
                return;
            }
            set_motion(
                m,
                cx,
                if walk { MTI_WALK } else { MTI_RUN },
                ANILOOP_LOOP,
                0,
            );
            if cx.client() && !m.is_player() && hit(m, cx) {
                cx.emit(SimEvent::CameraQuake {
                    frames: 15,
                    scale: 1.0,
                });
            }
        }
        OBJSTA_STOP_RUN => {
            if m.act.state.is_act_damage() {
                return;
            }
            m.act.delta.x = 0.0;
            m.act.delta.z = 0.0;
            if m.act.state.bits() != OBJSTA_STOP_RUN {
                return;
            }
            set_motion(m, cx, MTI_RUN, ANILOOP_LOOP, 0);
        }
        OBJSTA_BMOVE => {
            if m.act.state.is_act_damage() {
                return;
            }
            m.act.delta.x = sinf(theta) * (-speed / 5.0);
            m.act.delta.z = -cosf(theta) * (-speed / 5.0);
            if m.act.state.bits() != OBJSTA_BMOVE {
                return;
            }
            set_motion(m, cx, MTI_BACK, ANILOOP_LOOP, 0);
        }
        OBJSTA_LEFT | OBJSTA_RIGHT => {}
        OBJSTA_SIT => {
            if m.motion.id == MTI_SIT {
                if end_frame(m) {
                    set_motion(m, cx, MTI_SITSTAND, ANILOOP_LOOP, 0);
                    if cx.client() {
                        cx.emit(SimEvent::SatDown); // the party stretching SFX (`#ifdef __CLIENT`)
                    }
                }
            } else if m.motion.id == MTI_GETUP {
                if end_frame(m) {
                    m.reset_state(cx, OBJSTA_MOVE_ALL);
                }
            } else if m.motion.id != MTI_SITSTAND {
                set_motion(m, cx, MTI_SITSTAND, ANILOOP_LOOP, 0);
            }
        }
        OBJSTA_PICKUP => {
            if end_frame(m) {
                m.reset_state(cx, OBJSTA_MOVE_ALL);
            }
        }
        OBJSTA_COLLECT => {}
        OBJSTA_STUN => {
            if m.motion.id != MTI_GROGGY
                && (m.act.state.bits() & !OBJSTA_ACTION_ALL) != OBJSTA_NOT_STAND
            {
                set_motion(m, cx, MTI_GROGGY, ANILOOP_LOOP, MOP_FIXED);
            }
            if m.params.chr_state & defs::CHS_STUN == 0 {
                m.stun_cnt = 0;
                m.mvrf.remove(MVRF_NOACTION);
                m.reset_state(cx, OBJSTA_ACTION_ALL);
                cx.emit(SimEvent::StunEnded);
            }
        }
        OBJSTA_APPEAR => {
            if end_frame(m) {
                cx.emit(SimEvent::AppearEnded);
                m.reset_state(cx, OBJSTA_ACTION_ALL);
            }
        }
        OBJSTA_LTURN => {
            if m.act.state.is_act_attack() {
                return;
            }
            m.angle = crate::num::add_angle(m.angle, 4.0);
        }
        OBJSTA_RTURN => {
            if m.act.state.is_act_attack() {
                return;
            }
            m.angle = crate::num::add_angle(m.angle, -4.0);
        }
        OBJSTA_LOOKUP => {
            if m.act.state.is_act_attack() {
                return;
            }
            if m.angle_x > -45.0 {
                m.angle_x += -1.0;
            }
        }
        OBJSTA_LOOKDOWN => {
            if m.act.state.is_act_attack() {
                return;
            }
            if m.angle_x < 45.0 {
                m.angle_x += 1.0;
            }
        }
        OBJSTA_SJUMP1 => {
            if m.mvrf.has(MVRF_NOMOVE) {
                return;
            }
            m.set_jump_state(cx, OBJSTA_SJUMP2);
            m.act.delta.y = 0.2 + (m.params.jumping as f32 / 1000.0);
            if m.act.state.bits() & OBJSTA_COLLECT == 0 {
                set_motion(m, cx, MTI_JUMP2, ANILOOP_CONT, 0);
            }
            set_blending(m, false);
        }
        OBJSTA_SJUMP2 => {
            if m.mvrf.has(MVRF_NOMOVE) {
                return;
            }
            if m.act.state.bits() & OBJSTA_COLLECT == 0 {
                set_motion(m, cx, MTI_JUMP2, ANILOOP_CONT, 0);
            }
            if m.act.delta.y <= 0.0 {
                m.set_jump_state(cx, OBJSTA_SJUMP3);
                if m.act.state.bits() & OBJSTA_COLLECT == 0 {
                    set_motion(m, cx, MTI_JUMP3, ANILOOP_CONT, 0);
                }
                set_blending(m, false);
            }
        }
        OBJSTA_SJUMP3 => {
            if m.mvrf.has(MVRF_NOMOVE) {
                return;
            }
            // KEEP-QUIRK: `||` (the original only converts while not damaged, but the second operand
            // makes the test pass whenever COLLECT is clear)
            // FIX (PL-172): not while an attack or a cast runs (a state word that carries both groups, e.g. a
            // reported one): the fall must not replace the attack motion
            if (m.act.state.bits() & OBJSTA_DMG_ALL == 0
                || m.act.state.bits() & OBJSTA_COLLECT == 0)
                && !m.act.state.is_act_attack()
                && set_motion(m, cx, MTI_JUMP3, ANILOOP_CONT, 0)
            {
                set_blending(m, false);
            }
            if m.act.ground != 0 {
                m.set_jump_state(cx, OBJSTA_SJUMP4);
                cx.emit(SimEvent::Landed);
                if m.act.state.bits() & OBJSTA_DMG_ALL != 0 {
                    return;
                }
                if set_motion(m, cx, MTI_JUMP4, ANILOOP_1PLAY, 0) {
                    set_blending(m, false);
                }
            }
        }
        OBJSTA_SJUMP4 => {
            m.reset_state(cx, OBJSTA_JUMP_ALL);
            if m.act.state.bits() & OBJSTA_DMG_ALL != 0 {
                return;
            }
            if end_frame(m) {
                return;
            }
            if cx.server() && m.is_player() {
                cx.emit(SimEvent::JumpCompleted);
            }
        }
        OBJSTA_ATK1 | OBJSTA_ATK2 | OBJSTA_ATK3 | OBJSTA_ATK4 => process_attack_combo(m, cx, dw),
        OBJSTA_SP_ATK1 | OBJSTA_SP_ATK2 => {
            if end_frame(m) {
                m.reset_state(cx, OBJSTA_ATK_ALL);
            }
            if hit(m, cx) {
                cx.emit(SimEvent::AttrHit {
                    kind: HitKind::Special,
                    state: dw,
                    target: m.act.id_target,
                    hit_index: 0,
                    frame: cx.frame(),
                });
            }
        }
        OBJSTA_ATK_MAGIC1 => {
            if end_frame(m) {
                m.reset_state(cx, OBJSTA_ATK_ALL);
            }
            if hit(m, cx) {
                cx.emit(SimEvent::AttrHit {
                    kind: HitKind::Wand,
                    state: dw,
                    target: m.act.id_target,
                    hit_index: 0,
                    frame: cx.frame(),
                });
            }
        }
        OBJSTA_ATK_RANGE1 => {
            if end_frame(m) {
                m.reset_state(cx, OBJSTA_ATK_ALL);
            }
            if hit(m, cx) {
                m.mvrf.add(MVRF_HITCONT);
                if cx
                    .env
                    .lookup
                    .snapshot(m.act.id_target)
                    .is_some_and(|t| t.alive)
                {
                    cx.emit(SimEvent::AttrHit {
                        kind: HitKind::Range,
                        state: dw,
                        target: m.act.id_target,
                        hit_index: 0,
                        frame: cx.frame(),
                    });
                }
            }
        }
        OBJSTA_ATK_MELEESKILL => process_melee_skill(m, cx),
        OBJSTA_ATK_CASTING1 | OBJSTA_ATK_CASTING2 => process_casting(m, cx, dw),
        OBJSTA_ATK_MAGICSKILL => process_magic_skill(m, cx),
        OBJSTA_DMG => {
            if end_frame(m) {
                m.reset_state(cx, OBJSTA_DMG_ALL);
            }
        }
        OBJSTA_DMG_FLY => {
            if m.act.ground != 0 {
                if m.act.state.bits() & OBJSTA_STUN != 0 {
                    m.reset_state(cx, OBJSTA_DMG_FLY);
                    return;
                }
                if m.act.delta.y == 0.0 {
                    m.act.delta.x = 0.0;
                    m.act.delta.z = 0.0;
                    m.act.delta_e.x = 0.0;
                    m.act.delta_e.z = 0.0;
                    m.act.delta.y = 0.04; // __BS_FIXED_KNOCKBACK (V15; was 0.12)
                    m.set_state(cx, OBJSTA_DMG_ALL, OBJSTA_DMG_DIE);
                    set_motion(m, cx, MTI_DMGDIE, ANILOOP_CONT, 0);
                }
            }
        }
        OBJSTA_DMG_DIE => {
            m.act.delta.x = 0.0;
            m.act.delta.z = 0.0;
            if end_frame(m) {
                if m.params.hp_positive {
                    m.set_state(cx, OBJSTA_DMG_ALL, OBJSTA_DMG_LIVE);
                    set_motion(m, cx, MTI_DMGLIVE, ANILOOP_1PLAY, 0);
                } else {
                    m.set_state(cx, OBJSTA_DMG_ALL, OBJSTA_DEAD);
                }
            }
        }
        OBJSTA_DMG_LIVE => {
            if end_frame(m) {
                m.mvrf.remove(MVRF_NOACTION);
                m.reset_state(cx, OBJSTA_DMG_ALL);
                m.reset_state(cx, OBJSTA_ATK_ALL);
            }
        }
        OBJSTA_DEAD => {
            if m.motion.clock.lp != ANILOOP_CONT {
                m.motion.clock.lp = ANILOOP_CONT;
            }
            m.mvrf.remove(MVRF_NOACTION);
            if cx.server() && !m.is_player() {
                m.act.dead_cnt -= 1;
                if m.act.dead_cnt <= 0 {
                    m.set_state(cx, OBJSTA_DMG_ALL, OBJSTA_DISAPPEAR);
                    m.act.count = 0;
                    cx.emit(SimEvent::DeadCountExpired);
                }
            }
            if cx.client() && !m.is_player() {
                let clip = m.motion.clip.and_then(|c| cx.env.motions.clip(c));
                if !attrs_firing(&m.motion.clock, clip, MA_QUAKE, cx.env.flags.hit_attr_rule)
                    .is_empty()
                {
                    let frames = if m.params.rank == defs::RANK_MATERIAL {
                        60
                    } else {
                        15
                    };
                    cx.emit(SimEvent::CameraQuake { frames, scale: 1.0 });
                }
            }
        }
        OBJSTA_DISAPPEAR => {
            // the server deletes the corpse one second after the state began (`m_nCount > FRAME_PER_SEC`);
            // `dead_cnt == i32::MIN` latches the one-shot event
            if cx.server() && m.act.count > 60 && m.act.dead_cnt != i32::MIN {
                m.act.dead_cnt = i32::MIN;
                cx.emit(SimEvent::Disappeared);
            }
        }
        OBJSTA_RESURRECTION if end_frame(m) => {
            m.reset_state(cx, OBJSTA_DMG_ALL);
        }
        _ => {}
    }
}

/// `_ProcessStateAttack` for ATK1..ATK4 (players and non-players), Plan/31 §7.4.
fn process_attack_combo(m: &mut SimMover, cx: &mut Cx<'_>, dw: u32) {
    if m.is_player() {
        let (req_level, next) = match dw {
            OBJSTA_ATK1 => (2, OBJMSG_ATK2),
            OBJSTA_ATK2 => (8, OBJMSG_ATK3),
            OBJSTA_ATK3 => (12, OBJMSG_ATK4),
            _ => (999, OBJMSG_ATK1),
        };
        if end_frame(m) {
            if cx.server() {
                m.act.objid_hit = ObjId::NONE;
            }
            m.reset_state(cx, OBJSTA_ATK_ALL);
            if cx.client() && m.is_active {
                client_combo_chain(m, cx, dw, req_level, next);
            }
        }
    } else {
        // non-players
        if cx.client() {
            boss_charge_fx(m, cx, dw);
        }
        if m.mvrf.has(MVRF_HITCONT) {
            cx.emit(SimEvent::MeleeContinue {
                state: dw,
                count: m.act.count,
                hit_count: m.act.motion_hit_count,
            });
        }
        if end_frame(m) {
            cx.emit(SimEvent::EndMeleeAttack);
            m.mvrf.remove(MVRF_HITCONT | MVRF_LASERCHARGE);
            m.act.objid_hit = ObjId::NONE;
            m.reset_state(cx, OBJSTA_ATK_ALL);
            m.act.motion_hit_count = 0;
        }
    }
    if m.motion.clock.pause > 0 {
        m.motion.clock.pause -= 1;
    } else {
        if cx.client()
            && m.is_player()
            && m.params.stance.is_some_and(|s| s.kind3 == defs::IK3_YOYO)
        {
            let clip = m.motion.clip.and_then(|c| cx.env.motions.clip(c));
            if !attrs_firing(&m.motion.clock, clip, MA_QUAKE, cx.env.flags.hit_attr_rule).is_empty()
            {
                cx.emit(SimEvent::YoyoThrowFx);
            }
        }
        let n = hit_count(m, cx);
        for _ in 0..n {
            m.mvrf.add(MVRF_HITCONT);
            if cx
                .env
                .lookup
                .snapshot(m.act.id_target)
                .is_some_and(|t| t.alive)
            {
                // ---- render-audio-m7 (doc 41 §17, WP-REND-27): a non-player swing reports its `m_nMotionHitCount`
                // (the boss handlers and the T11 sounds are keyed by it; the server ignores the word for a
                // melee hit)
                cx.emit(SimEvent::AttrHit {
                    kind: HitKind::Melee,
                    state: dw,
                    target: m.act.id_target,
                    hit_index: if m.is_player() {
                        0
                    } else {
                        m.act.motion_hit_count
                    },
                    frame: cx.frame(),
                });
                // `CMover::OnAttackMelee` ends every boss handler with `m_nMotionHitCount++`, ClockWorks
                // excepted (`MoverActEvent.cpp:1647-1695`); the client counts for its effects, the server's
                // count is flyff-game's (`bump_motion_hit_count`)
                if cx.client() && !m.is_player() && counts_motion_hits(m.params.mover_index) {
                    m.act.motion_hit_count += 1;
                }
                // `OnAttackMelee` (only called for a live target) sets `m_nPause = 5` of a player on the
                // client when the damage went through; the client's `OnDamageMsgC` always answers 1 for a
                // live victim (hit-stop, client only: D4, KQ-04)
                if cx.client() && m.is_player() {
                    m.motion.clock.pause = 5;
                }
            }
        }
    }
}

/// The client's combo chaining of the active mover (Plan/31 §7.4, KQ-04: the cadence is client-paced):
/// when the level suffices and the attack button or auto-attack is held, the next combo attack is started
/// against the last hit target (yoyos stop after ATK1) and reported as `LocalAttack`.
fn client_combo_chain(m: &mut SimMover, cx: &mut Cx<'_>, dw: u32, req_level: u32, next: ObjMsg) {
    if m.params.level < req_level || !(m.input.lmb_held || m.input.auto_attack) {
        return;
    }
    let yoyo = m.params.stance.is_some_and(|s| s.kind3 == defs::IK3_YOYO);
    if yoyo && matches!(dw, OBJSTA_ATK2 | OBJSTA_ATK3 | OBJSTA_ATK4) {
        return;
    }
    let target = m.act.objid_hit;
    if target != ObjId::NONE && cx.env.lookup.snapshot(target).is_some() && m.input.next_skill_none
    {
        let r = send_act_msg_cx(m, cx, ActMsg::with(next, [target.0 as i32, 0, 0, 0, 0])); // BOUNDS: id bit pattern
        if r > 0 {
            cx.emit(SimEvent::LocalAttack {
                msg: next,
                target,
                flags: r,
            });
        }
    } else {
        m.act.objid_hit = ObjId::NONE;
    }
}

/// The movers whose `OnAttackMelee` handler ends with `m_nMotionHitCount++` (`MoverActEvent.cpp:1647-1695`):
/// BigMuscle, Krrr, the Mushmoot and the four Meteonykers; not the ClockWorks.
fn counts_motion_hits(mover_index: u32) -> bool {
    matches!(
        mover_index,
        defs::MI_BIGMUSCLE
            | defs::MI_KRRR
            | defs::MI_MUSHMOOT
            | defs::MI_DU_METEONYKER
            | defs::MI_DU_METEONYKER2
            | defs::MI_DU_METEONYKER3
            | defs::MI_DU_METEONYKER4
    )
}

fn boss_charge_fx(m: &mut SimMover, cx: &mut Cx<'_>, dw: u32) {
    let idx = m.params.mover_index;
    let charge = (matches!(idx, defs::MI_CLOCKWORK1 | defs::MI_LOADCLOCKWORK) && dw == OBJSTA_ATK3)
        || (idx == defs::MI_MUSHMOOT && (dw == OBJSTA_ATK3 || dw == OBJSTA_ATK4));
    if charge && !m.mvrf.has(MVRF_LASERCHARGE) {
        m.mvrf.add(MVRF_LASERCHARGE);
        cx.emit(SimEvent::BossChargeFx { state: dw });
    }
}

fn skill_is(m: &SimMover, id: u32) -> bool {
    m.skill.skill == id
}

/// `ProcessStateMeleeSkill` (Plan/31 §7.4).
fn process_melee_skill(m: &mut SimMover, cx: &mut Cx<'_>) {
    if skill_is(m, defs::SI_ACR_YOYO_COUNTER) {
        // never self-cancels
    } else if skill_is(m, defs::SI_BLD_DOUBLESW_SILENTSTRIKE) {
        if end_frame(m) {
            m.reset_state(cx, OBJSTA_ATK_ALL);
            return;
        }
    } else if m.motion.clock.frame_cur >= (m.motion.clock.frame_max as f32) * 0.65 {
        m.motion.clock.end_frame = true;
        m.reset_state(cx, OBJSTA_ATK_ALL);
        return;
    }
    if cx.frame() & 3 == 0 && m.act.shoot_cnt > 0 {
        cx.emit(SimEvent::RepeatShot {
            kind: ShotKind::MeleeSkill,
            frame: cx.frame(),
        });
        m.act.shoot_cnt -= 1;
    }
    if m.motion.clock.pause > 0 {
        m.motion.clock.pause -= 1;
    } else if hit(m, cx) {
        if skill_is(m, defs::SI_BLD_DOUBLESW_SILENTSTRIKE) {
            m.act.delta_e = angle_to_vector_xz(m.angle, 0.75);
        } else if skill_is(m, defs::SI_BIL_KNU_BELIALSMESHING) {
            m.act.delta_e = angle_to_vector_xz(m.angle, 0.35);
        }
        cx.emit(SimEvent::AttrHit {
            kind: HitKind::MeleeSkill,
            state: OBJSTA_ATK_MELEESKILL,
            target: m.skill.target,
            hit_index: m.act.motion_hit_count,
            frame: cx.frame(),
        });
        m.act.motion_hit_count += 1;
        if cx.client() {
            m.motion.clock.pause = 4;
        }
        if m.skill.act != OBJACT_USESKILL {
            return;
        }
        if m.skill.skill_count > 0 {
            m.act.shoot_cnt = m.skill.skill_count - 1;
        }
    }
}

/// `ProcessStateMagicCasting` (Plan/31 §7.4).
fn process_casting(m: &mut SimMover, cx: &mut Cx<'_>, dw: u32) {
    let opt = MOP_SWDFORCE | MOP_NO_TRANS | MOP_FIXED;
    if dw == OBJSTA_ATK_CASTING1 {
        if end_frame(m) {
            let next = m.motion.id.wrapping_add(1);
            set_motion(m, cx, next, ANILOOP_LOOP, opt);
            m.set_state(cx, OBJSTA_ATK_ALL, OBJSTA_ATK_CASTING2);
        }
    } else if dw == OBJSTA_ATK_CASTING2 && m.act.count >= m.act.casting_time {
        let mut next = m.motion.id.wrapping_add(1);
        if cx.client() && m.params.disguised {
            next = MTI_ATK1;
        }
        set_motion(m, cx, next, ANILOOP_1PLAY, opt);
        m.set_state(cx, OBJSTA_ATK_ALL, OBJSTA_ATK_MAGICSKILL);
    }
    if cx.client() {
        cx.emit(SimEvent::CastSpark);
    }
}

/// `ProcessStateMagicSkill` (Plan/31 §7.4).
fn process_magic_skill(m: &mut SimMover, cx: &mut Cx<'_>) {
    if end_frame(m) {
        m.reset_state(cx, OBJSTA_ATK_ALL);
        return;
    }
    if cx.frame() & 3 == 0 && m.act.shoot_cnt > 0 {
        cx.emit(SimEvent::RepeatShot {
            kind: ShotKind::MagicSkill,
            frame: cx.frame(),
        });
        m.act.shoot_cnt -= 1;
    }
    if hit(m, cx) {
        cx.emit(SimEvent::AttrHit {
            kind: HitKind::MagicSkill,
            state: OBJSTA_ATK_MAGICSKILL,
            target: m.skill.target,
            hit_index: m.act.motion_hit_count,
            frame: cx.frame(),
        });
        m.act.motion_hit_count += 1;
        if m.skill.act != OBJACT_USESKILL {
            return;
        }
        if m.skill.skill_count > 0 {
            m.act.shoot_cnt = m.skill.skill_count - 1;
        }
    }
}
