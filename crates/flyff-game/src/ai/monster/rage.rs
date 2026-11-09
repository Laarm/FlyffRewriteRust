//! `StateRage` and `MoveProcessRage` (Plan/18 §9.6, §9.10, ref/12 §9.3, §9.5; `AIMonster.cpp:1561-1786`,
//! `2055-2191`; WP-AI-09): the chase at speed factor 2.0, the spawn and damage-position leashes, the stuck
//! teleport, the switch to the next party member and the exits for a vanished, flying, hidden or protected
//! target.

use super::helpers::{
    TargetView, hp_percent, is_in_range, is_moving, mover_ex, mover_is_die, target_view,
};
use super::{
    CHASE_TIMER_MS, MonsterAi, MonsterState, NEXT_SCAN_RANGE, NEXT_SCAN_RANGE_INSTANCE, RANGE_MOVE,
    RANGE_RETURN_TO_BEGIN, STUCK_MOVE_MAX, attack, summon,
};
use crate::ai::ctx::{AiCx, emote};
use crate::ai::msg::AiMsg;
use crate::ai::rng::x_random;
use crate::ai::scan::{RANK_GUARD, scan_target_next};
use crate::movement::intents;
use crate::mover::Mover;
use flyff_common::GameMs;
use flyff_common::math::Vec3;
use flyff_sim::kin::own_speed;
use flyff_sim::num::{get_degree_x, length, velocity_to_vec};
use flyff_sim::types::OBJSTA_DMG_FLY_ALL;

/// `MAX_JOB`: the length of the per-job range-attack table.
const MAX_JOB: i32 = 32;

fn set_state(state: MonsterState, param: Option<flyff_common::ObjId>) -> AiMsg {
    AiMsg::SetState {
        state: state as u32,
        param,
    }
}

/// The messages of `STATE_RAGE`.
pub(super) fn handle(ai: &mut MonsterAi, m: &mut Mover, cx: &mut AiCx<'_>, msg: AiMsg) {
    let base = ai.base_state;
    match msg {
        AiMsg::Init { param, .. } => {
            // a RUNAWAY -> RAGE switch carries no target and keeps the old one
            if let Some(p) = param
                && let Some(t) = target_view(cx, p)
            {
                ai.set_target(m, Some(p), t.party);
            }
            ai.tm_attack = GameMs(cx.now.0 + CHASE_TIMER_MS);
            // the first attack comes without a delay
            ai.tm_attack_delay = cx.now;
            ai.set_stop(m, cx, 0);
        }
        AiMsg::InitTargetClear => {
            // I killed my target: the next member of its party, else back to the base state
            match scan_target_next(cx, m, NEXT_SCAN_RANGE, ai.target, ai.target_party) {
                Some(t) => ai.send(m, cx, set_state(MonsterState::Rage, Some(t))),
                None => {
                    ai.set_target(m, None, 0);
                    ai.send(m, cx, set_state(base, None));
                }
            }
        }
        AiMsg::Process => move_process_rage(ai, m, cx),
        AiMsg::Damage { attacker, .. } => on_damage(ai, m, cx, attacker),
        AiMsg::Collision => ai.move_to_random(m, cx, MonsterState::Rage),
        AiMsg::DstDie { dead } => {
            if ai.target == Some(dead) {
                if let Some(t) =
                    scan_target_next(cx, m, NEXT_SCAN_RANGE, ai.target, ai.target_party)
                {
                    ai.send(m, cx, set_state(MonsterState::Rage, Some(t)));
                    return;
                }
                ai.set_target(m, None, 0);
            }
            // the death of ANY enemy ends the rage (KQ-13 Q10)
            ai.send(m, cx, set_state(base, None));
        }
        AiMsg::BeginMove => ai.go_target = false,
        // Arrival, Die, Exit: no-ops
        _ => {}
    }
}

/// `AIMSG_DAMAGE` in RAGE: a moving monster switches to the attacker half of the time (once per move), runs
/// away below `ex.runaway_hp`, calls helpers, and remembers where it was hit.
fn on_damage(ai: &mut MonsterAi, m: &mut Mover, cx: &mut AiCx<'_>, attacker: flyff_common::ObjId) {
    if is_moving(m) && !ai.go_target {
        if x_random(cx.rng, 2) == 0 {
            ai.move_to_obj(m, cx, attacker);
        }
        ai.go_target = true;
    }
    let (runaway_hp, help_who, unit_help_ms) = {
        let ex = mover_ex(m, cx);
        (ex.runaway_hp, ex.ai.help_who, ex.ai.unit_help_ms)
    };
    if hp_percent(m, cx) < i32::from(runaway_hp) {
        ai.send(m, cx, set_state(MonsterState::Runaway, None));
        cx.emote(m, emote::EVADE);
    }
    if help_who != 0 {
        // `m_tmHelp` starts at 0, so the first call is always due
        let due = ai
            .tm_help
            .is_none_or(|t| cx.now.0.saturating_sub(t.0) > u64::from(unit_help_ms));
        if due && !ai.call_helper_done {
            summon::call_helper(ai, m, cx);
            cx.emote(m, emote::HELPER);
            ai.tm_help = Some(cx.now);
            // 0 calls once and never again
            if unit_help_ms == 0 {
                ai.call_helper_done = true;
            }
        }
    } else {
        ai.call_helper_done = false;
    }
    ai.pos_damage = Some(m.sim.pos);
}

/// Gives up the chase: back to the base state (a STAND monster walks home first).
fn give_up(ai: &mut MonsterAi, m: &mut Mover, cx: &mut AiCx<'_>, stand_returns: bool) {
    let base = ai.base_state;
    if stand_returns && base == MonsterState::Stand {
        ai.do_return_to_begin(m, cx, true);
    }
    ai.send(m, cx, set_state(base, None));
}

/// Leaves the chase through the leash: always walk home, then the base state.
fn leash_home(ai: &mut MonsterAi, m: &mut Mover, cx: &mut AiCx<'_>) {
    let base = ai.base_state;
    ai.do_return_to_begin(m, cx, true);
    ai.send(m, cx, set_state(base, None));
}

/// `MoveProcessRage` (18 §9.6).
fn move_process_rage(ai: &mut MonsterAi, m: &mut Mover, cx: &mut AiCx<'_>) {
    // a dead or flying-through-the-air monster does nothing
    if mover_is_die(m) || m.sim.act.state.bits() & OBJSTA_DMG_FLY_ALL != 0 {
        return;
    }
    let inst = cx.instance;
    let guard = m.monster().is_some_and(|x| x.class == RANK_GUARD);
    // the target must exist, live and be in this world
    let t = match ai.target.and_then(|id| target_view(cx, id)) {
        Some(t) if !t.die => t,
        _ => {
            let range = if inst {
                NEXT_SCAN_RANGE_INSTANCE
            } else {
                NEXT_SCAN_RANGE
            };
            match scan_target_next(cx, m, range, ai.target, ai.target_party) {
                Some(n) => ai.send(m, cx, set_state(MonsterState::Rage, Some(n))),
                None => give_up(ai, m, cx, true),
            }
            return;
        }
    };
    // the target stands in a safety zone: a monster that is not a guard goes home
    if t.in_safety && !guard {
        leash_home(ai, m, cx);
        return;
    }
    // a flying monster and a ground target (or the other way round): drop the aggro (KQ-13 Q1)
    if m.sim.params.flying_npc != t.fly {
        give_up(ai, m, cx, true);
        return;
    }
    // every 15 s: stuck? teleport onto the target
    if cx.now.0 > ai.tm_attack.0 {
        ai.tm_attack = GameMs(cx.now.0 + CHASE_TIMER_MS);
        let mut moved = m.sim.pos - ai.old_pos;
        moved.y = 0.0;
        if is_in_range(moved, STUCK_MOVE_MAX) && own_speed(&m.sim) > 0.0 {
            // a monster that cannot move (rooted by a spell) must not be warped
            intents::warp_npc(m, t.pos);
        }
        ai.old_pos = m.sim.pos;
    }
    // the leashes: 150 m (210 m flying) from the spawn point, 120 m from where it was last hit
    let mut spawn_range = RANGE_MOVE;
    if m.sim.params.flying_npc {
        spawn_range *= 3.0;
    }
    if !inst
        && !is_in_range(
            m.sim.pos - ai.spawn_pos,
            RANGE_RETURN_TO_BEGIN + spawn_range,
        )
    {
        leash_home(ai, m, cx);
        return;
    }
    // an aggressive monster has not been hit yet: its scan position counts
    let pos_damage = *ai.pos_damage.get_or_insert(m.sim.pos);
    if !inst && !is_in_range(m.sim.pos - pos_damage, RANGE_RETURN_TO_BEGIN) {
        leash_home(ai, m, cx);
        return;
    }
    // the target made itself invisible
    if t.transparent {
        if inst
            && let Some(n) =
                scan_target_next(cx, m, NEXT_SCAN_RANGE_INSTANCE, ai.target, ai.target_party)
        {
            ai.send(m, cx, set_state(MonsterState::Rage, Some(n)));
            return;
        }
        give_up(ai, m, cx, true);
        return;
    }
    // chase (or keep the range of a ranged attacker: no V15 monster has such an AI)
    ai.range_attack = attack::select_attack_type(ai, m, cx, &t);
    if ai.range_attack {
        keep_range(ai, m, cx, &t);
    } else {
        ai.move_to_obj(m, cx, t.id);
    }
    if mover_ex(m, cx).ai.summ_prob != 0 {
        summon::sub_summon_process(ai, m, cx, &t);
    }
    if cx.now.0 > ai.tm_attack_delay.0 {
        attack::sub_attack_chance(ai, m, cx, &t);
    }
}

/// The ranged branch of `MoveProcessRage` (`m_bRangeAttack`): hold the range `R = range_attack[job] & 0x7F`,
/// keeping it exactly when bit 0x80 is set. KEEP-QUIRK: the flee direction uses `GetDegreeX` (the pitch) as a
/// yaw (Q12).
fn keep_range(ai: &mut MonsterAi, m: &mut Mover, cx: &mut AiCx<'_>, t: &TargetView) {
    let mut job = t.job;
    if job < 0 {
        job = 0;
    }
    if job >= MAX_JOB {
        tracing::error!(job, "MoveProcessRage: job out of range");
        job = 0;
    }
    let packed = mover_ex(m, cx)
        .ai
        .range_attack
        .get(usize::try_from(job).unwrap_or(0))
        .copied()
        .unwrap_or(0);
    let range = f32::from(packed & 0x7F);
    let keep = packed & 0x80 != 0;
    let pos = m.sim.pos;
    let dist = length(t.pos - pos);
    let stop = |ai: &mut MonsterAi, m: &mut Mover| {
        if !m.sim.is_empty_dest() {
            // "move to my own position": a stop on the server
            ai.move_to_pos(m, pos);
        }
    };
    if keep {
        if (dist - range).abs() < 1.0 {
            stop(ai, m);
        } else if dist < range {
            let a = get_degree_x(pos, t.pos);
            let dst: Vec3 = pos + velocity_to_vec(a, 10.0);
            ai.move_to_pos(m, dst);
        } else {
            ai.move_to_obj(m, cx, t.id);
        }
    } else if dist < range {
        stop(ai, m);
    } else {
        ai.move_to_obj(m, cx, t.id);
    }
}
