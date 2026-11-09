//! `StateIdle` and `MoveProcessIdle` (Plan/18 §9.4, ref/12 §9.3-§9.4; `AIMonster.cpp:311-571`; WP-AI-08).

use super::helpers::{is_in_range, mover_ex, mover_is_die, target_view};
use super::{
    CHASE_TIMER_MS, MonsterAi, MonsterState, RANGE_MOVE, RETURN_DONE_RANGE_IDLE,
    RETURN_WARP_AFTER_IDLE_MS,
};
use crate::ai::ctx::{AiCx, emote};
use crate::ai::msg::AiMsg;
use crate::ai::rng::x_random;
use crate::ai::scan::{JOB_ALL, RANK_GUARD, scan_target};
use crate::mover::Mover;
use flyff_common::GameMs;
use flyff_sim::types::OBJSTA_DMG_FLY_ALL;

/// The messages of `STATE_IDLE`.
pub(super) fn handle(ai: &mut MonsterAi, m: &mut Mover, cx: &mut AiCx<'_>, msg: AiMsg) {
    match msg {
        AiMsg::Init { .. } => {
            // `(int)RANGE_MOVE`, x3 for a flying NPC
            let mut range = RANGE_MOVE;
            if m.sim.params.flying_npc {
                range *= 3.0;
            }
            if !is_in_range(m.sim.pos - ai.spawn_pos, range) {
                ai.do_return_to_begin(m, cx, true);
            }
            ai.set_stop(m, cx, 0);
        }
        AiMsg::InitTargetClear => ai.set_target(m, None, 0),
        AiMsg::Process => move_process_idle(ai, m, cx),
        AiMsg::Damage { attacker, .. } => {
            // damage is ignored while the monster is on its way home (KQ-13 Q3)
            if !ai.returning {
                ai.move_to_obj(m, cx, attacker);
                ai.send(
                    m,
                    cx,
                    AiMsg::SetState {
                        state: MonsterState::Rage as u32,
                        param: Some(attacker),
                    },
                );
                ai.tm_attack = GameMs(cx.now.0 + CHASE_TIMER_MS);
                ai.pos_damage = Some(m.sim.pos);
            }
        }
        AiMsg::Die => ai.send(m, cx, AiMsg::Exit),
        AiMsg::Collision => ai.move_to_random(m, cx, MonsterState::Idle),
        AiMsg::Arrival { pos_arrival, .. } => arrival(ai, m, cx, pos_arrival),
        _ => {}
    }
}

/// `AIMSG_ARRIVAL` of IDLE and STAND (their handlers are identical): a flying monster that reached a wander
/// point starts move pattern 2 (and keeps flying: no stop); a ground monster stands still for 5-6 s, or for
/// 0-1 s while it is on its way home. (Monster looting is disabled by the loader: no loot completion.)
pub(super) fn arrival(ai: &mut MonsterAi, m: &mut Mover, cx: &mut AiCx<'_>, pos_arrival: bool) {
    if m.sim.params.flying_npc {
        if pos_arrival {
            crate::movement::intents::set_move_pattern(m, 2);
        }
    } else if ai.returning {
        let t = u64::from(x_random(cx.rng, 1000));
        ai.set_stop(m, cx, t);
    } else {
        let t = 5000 + u64::from(x_random(cx.rng, 1000));
        ai.set_stop(m, cx, t);
    }
}

/// `MoveProcessIdle` (18 §9.4).
fn move_process_idle(ai: &mut MonsterAi, m: &mut Mover, cx: &mut AiCx<'_>) {
    // a mover that is dead or flying through the air from a hit does nothing
    if mover_is_die(m) || m.sim.act.state.bits() & OBJSTA_DMG_FLY_ALL != 0 {
        return;
    }
    if ai.returning {
        // arrived at the spawn point? (full HP, enemies forgotten) -- the 20 s warp check runs regardless
        if is_in_range(ai.spawn_pos - m.sim.pos, RETURN_DONE_RANGE_IDLE) {
            ai.do_return_to_begin(m, cx, false);
            ai.send(
                m,
                cx,
                AiMsg::Arrival {
                    obj: None,
                    pos_arrival: false,
                },
            );
        }
        if cx.now.0.saturating_sub(ai.tm_return.0) >= RETURN_WARP_AFTER_IDLE_MS {
            // not getting home: put it there
            ai.warp_home(m, cx);
        }
        return;
    }
    if m.sim.pattern.pattern == 0 {
        if ai.is_end_stop(m, cx) {
            ai.move_to_random(m, cx, MonsterState::Idle);
        }
    } else if m.sim.pattern.event == 99 {
        // the flying pattern is over: back to ordinary moves (not broadcast: the client ends it itself)
        reset_pattern(m);
    }
    scan_and_engage(ai, m, cx);
}

/// `SetMovePattern(0)` without a broadcast.
pub(super) fn reset_pattern(m: &mut Mover) {
    m.sim.pattern = flyff_sim::mover::MovePatternState::default();
}

/// The scan block shared by `MoveProcessIdle` and `MoveProcessStand`: look for a player and, when there is
/// one the monster may fight, show the attack emoticon and go to RAGE.
pub(super) fn scan_and_engage(ai: &mut MonsterAi, m: &mut Mover, cx: &mut AiCx<'_>) {
    let ex = mover_ex(m, cx);
    // -1 means "never scan"
    if ex.ai.scan_job == -1 {
        return;
    }
    let range = ex.attack_first_range;
    if range > 10 || range <= 0 {
        if !ai.bad_range_logged {
            ai.bad_range_logged = true;
            tracing::error!(
                mover = m.prop_id(),
                range,
                "attack_first_range must be 1..=10: the monster never scans"
            );
        }
        return;
    }
    let (job, quest, item, chao) = (
        ex.ai.scan_job,
        ex.ai.scan_quest_id,
        ex.ai.scan_item_idx,
        ex.ai.scan_chao,
    );
    let mut target = None;
    // players that meet the prop's conditions
    if job != 0 || quest != 0 || item != 0 || chao != 0 {
        target = scan_target(cx, m, range, job, quest, item, chao);
    }
    // no such player and an aggressive monster: anybody
    if target.is_none() && m.monster().is_some_and(|x| x.active_attack) {
        target = scan_target(cx, m, range, JOB_ALL, 0, 0, 0);
    }
    let Some(t) = target else { return };
    let Some(view) = target_view(cx, t) else {
        return;
    };
    let guard = m.monster().is_some_and(|x| x.class == RANK_GUARD);
    // ground monsters do not aggro flying players (Q1); guards always do
    if m.sim.params.flying_npc || !view.fly || guard {
        cx.emote(m, emote::ATTACK);
        ai.send(
            m,
            cx,
            AiMsg::SetState {
                state: MonsterState::Rage as u32,
                param: Some(t),
            },
        );
    }
}
