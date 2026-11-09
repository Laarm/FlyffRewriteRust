//! `StateRunaway` and `MoveProcessRunaway` (Plan/18 §9.8; `AIMonster.cpp:2193-2342`; WP-AI-11): the monster
//! whose HP fell below `ex.runaway_hp` (`Evade` AI block, `RAGE` -> `RUNAWAY` in the `Damage` handler) walks
//! away from its target in a zig-zag, pauses at every arrival, heals every 2 s and returns to RAGE at 50 % HP.
//! No V15 monster has an `Evade` AI (`runaway_hp` stays -1), so this state only runs on synthetic data and on
//! data dropped into `overrides/`.
//!
//! ```text
//!   RAGE --Damage, HP% < runaway_hp--> RUNAWAY --Process: every 2 s +GetHPRecovery, HP% >= 50--> RAGE
//!                                        |--target gone (invalid)--> base state
//!                                        `--DstDie: next party member --> RAGE, else base state
//! ```
//!
//! The flight distance is `1.7 x` the attack range of the **target's** weapon (`II_WEA_HAN_HAND` for bare
//! hands, `DST_HAWKEYE_RATE` included): the monster keeps just outside its reach.

use super::MonsterAi;
use super::helpers::{
    TargetView, hp_percent, hp_recovery, inc_hp, is_in_range, mover_is_die, target_view,
};
use super::{MonsterState, NEXT_SCAN_RANGE};
use crate::ai::ctx::AiCx;
use crate::ai::msg::AiMsg;
use crate::ai::rng::x_random;
use crate::ai::scan::scan_target_next;
use crate::mover::Mover;
use flyff_common::GameMs;
use flyff_common::defs::item::II_WEA_HAN_HAND;
use flyff_rules::combat::attack_range;
use flyff_sim::num::{add_angle, velocity_to_vec};
use flyff_sim::types::OBJSTA_DMG_FLY_ALL;

/// `NEXT_TICK_RECOVERY`: the self-heal period of a running-away monster.
pub const NEXT_TICK_RECOVERY_MS: u64 = 2_000;

/// How far a running monster flees: `1.7 x GetAttackRange(target's hand item)`.
fn flee_range(cx: &AiCx<'_>, t: &TargetView) -> f32 {
    let item = if t.hand_item != 0 {
        t.hand_item
    } else {
        II_WEA_HAN_HAND
    };
    // an unknown hand item is a null dereference in the original: no reach
    let code = cx.shared.data.item(item).map_or(0, |p| p.attack_range);
    attack_range(code, t.hawkeye) * 1.7_f32
}

/// `AddAngle(float)`: turns the server copy (no message; the next `DestPosNtf` stamp carries the yaw).
fn turn(m: &mut Mover, degrees: f32) {
    m.sim.angle = add_angle(m.sim.angle, degrees);
}

/// `MoveToDst(pos + VelocityToVec(angle, len))`.
fn flee(ai: &mut MonsterAi, m: &mut Mover, len: f32) {
    let dst = m.sim.pos + velocity_to_vec(m.sim.angle, len);
    ai.move_to_pos(m, dst);
}

/// The messages of `STATE_RUNAWAY`.
pub(super) fn handle(ai: &mut MonsterAi, m: &mut Mover, cx: &mut AiCx<'_>, msg: AiMsg) {
    let base = ai.base_state;
    match msg {
        AiMsg::Init { .. } => {
            // the legacy `p2` flag ("the enemy cannot reach me here") is never passed in V15
            ai.target_no_move_pos = false;
            // run the other way, with +-20 degrees of zig-zag
            turn(m, 180.0 + (20 - x_random(cx.rng, 40) as i32) as f32);
            match ai.target.and_then(|id| target_view(cx, id)) {
                Some(t) => {
                    let r = flee_range(cx, &t);
                    flee(ai, m, r);
                    ai.first_runaway = true;
                }
                None => ai.send(m, cx, set_state(base)),
            }
        }
        AiMsg::Process => move_process_runaway(ai, m, cx),
        AiMsg::Damage { .. } => {
            if ai.target_no_move_pos && !ai.first_runaway {
                ai.send(m, cx, set_state(MonsterState::Rage));
            }
            ai.pos_damage = Some(m.sim.pos);
        }
        AiMsg::Collision => {
            turn(m, 180.0 + (20 - x_random(cx.rng, 40) as i32) as f32);
            flee(ai, m, 5.0);
        }
        AiMsg::DstDie { dead } => {
            if ai.target == Some(dead) {
                if let Some(t) =
                    scan_target_next(cx, m, NEXT_SCAN_RANGE, ai.target, ai.target_party)
                {
                    ai.send(
                        m,
                        cx,
                        AiMsg::SetState {
                            state: MonsterState::Rage as u32,
                            param: Some(t),
                        },
                    );
                    return;
                }
                ai.set_target(m, None, 0);
            }
            ai.send(m, cx, set_state(base));
        }
        AiMsg::Arrival { .. } => {
            ai.first_runaway = false;
            let delay = i64::from(mover_runaway_delay(m, cx));
            if delay != 0 {
                let ms = u64::try_from(delay).unwrap_or(0) + u64::from(x_random(cx.rng, 1_000));
                ai.set_stop(m, cx, ms);
            }
        }
        // Die, Exit, BeginMove ...: nothing
        _ => {}
    }
}

fn set_state(state: MonsterState) -> AiMsg {
    AiMsg::SetState {
        state: state as u32,
        param: None,
    }
}

fn mover_runaway_delay(m: &Mover, cx: &AiCx<'_>) -> i16 {
    super::helpers::mover_ex(m, cx).runaway_delay_ms
}

/// `MoveProcessRunaway`: flee again when the stop is over and the target is within the flight range, and heal.
fn move_process_runaway(ai: &mut MonsterAi, m: &mut Mover, cx: &mut AiCx<'_>) {
    if mover_is_die(m) || m.sim.act.state.bits() & OBJSTA_DMG_FLY_ALL != 0 {
        return;
    }
    let base = ai.base_state;
    // an invalid target ends the flight
    let Some(t) = ai.target.and_then(|id| target_view(cx, id)) else {
        ai.set_target(m, None, 0);
        ai.send(m, cx, set_state(base));
        return;
    };
    if !ai.is_end_stop(m, cx) {
        return;
    }
    let r = flee_range(cx, &t);
    // within reach of the target: keep running, with a little zig-zag
    if is_in_range(t.pos - m.sim.pos, r) {
        turn(m, (20 - x_random(cx.rng, 40) as i32) as f32);
        flee(ai, m, r);
    }
    if !ai.target_no_move_pos && cx.now.0 > ai.tm_recovery.0 {
        // the rest heals now and then; at half health the monster fights back
        ai.tm_recovery = GameMs(cx.now.0 + NEXT_TICK_RECOVERY_MS);
        let n = hp_recovery(m, cx);
        inc_hp(m, cx, n);
        if hp_percent(m, cx) >= 50 {
            ai.send(m, cx, set_state(MonsterState::Rage));
        }
    }
}
