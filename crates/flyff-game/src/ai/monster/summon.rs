//! Calling helpers and summoning (Plan/18 §9.9; `AIMonster.cpp:1061-1160`, `1439-1488`; WP-AI-11). No V15
//! monster has a `Helper` or `Summon` AI block, so RAGE reaches these only for synthetic data.
//!
//! * [`call_helper`] (`CAIMonster::CallHelper`, RAGE `Damage`): the movers around the caller are not visible to a
//!   handler, so it queues an [`AiCmd::CallHelpers`]; the pipeline picks the helpers in the legacy dynamic-grid
//!   window (`ex.attack_first_range / 2`, or `x help_range_mul`, metres / MPU link units, no exact distance
//!   test), `help_who` 1 = anybody, 2 = movers of the caller's own kind, per slot at most `num` of them, and
//!   sends each `SetState{RAGE, target}` - or, when a slot asks for the party and the target has one, a random
//!   member of that party within 20 m (`rand() % n` from the world stream).
//! * [`sub_summon_process`] (`SubSummonProcess`, `__NEW_SUMMON_RULE`, RAGE `Process`): every 10 s drop the dead
//!   summons from the list, and while fewer than `summ_num` live, with chance `summ_prob` % create one monster
//!   `summ_id` within +-1.5 m (x drawn before z), aggressive, in the caller's layer, with an emoticon.

use super::MonsterAi;
use super::helpers::{TargetView, mover_ex, target_view};
use crate::ai::ctx::{AiCmd, AiCx, HelperCall, HelperSlot, emote};
use crate::ai::rng::{x_random, x_random_f};
use crate::ai::spawn::MonsterSpawn;
use crate::mover::Mover;
use flyff_common::math::Vec3;

/// `SUMMON_PERIOD`: one summon attempt per this long.
pub const SUMMON_PERIOD_MS: u64 = 10_000;
/// `m_nCallHelperMax` once a `Helper` AI block exists.
pub const HELPER_SLOTS: usize = 5;

/// `CallHelper(prop)` (WP-AI-11): asks the pipeline to send the movers around to the fight.
pub fn call_helper(ai: &mut MonsterAi, m: &mut Mover, cx: &mut AiCx<'_>) {
    let ex = mover_ex(m, cx);
    // `nRange`: half the scan range, or the scan range times the multiplier (int arithmetic)
    let n_range = if ex.ai.help_range_mul == 0 {
        ex.attack_first_range / 2
    } else {
        ex.attack_first_range.wrapping_mul(ex.ai.help_range_mul)
    };
    // slots without a `SetCallHelper` entry take any number and no party
    let slots = (0..HELPER_SLOTS)
        .map(|i| {
            ex.call_helpers.get(i).map_or(
                HelperSlot {
                    num: 0,
                    party: false,
                },
                |c| HelperSlot {
                    num: c.num,
                    party: c.party != 0,
                },
            )
        })
        .collect();
    let help_who = ex.ai.help_who;
    let target_party = ai
        .target
        .and_then(|id| target_view(cx, id))
        .map_or(0, |t| t.party);
    cx.cmds.push(AiCmd::CallHelpers(HelperCall {
        caller: m.id(),
        caller_mover: m.prop_id(),
        pos: m.core.pos,
        n_range,
        help_who,
        slots,
        target: ai.target,
        target_party,
    }));
}

/// `SubSummonProcess(target)` (WP-AI-11): at most every 10 s, one summon while fewer than `summ_num` live.
pub fn sub_summon_process(ai: &mut MonsterAi, m: &mut Mover, cx: &mut AiCx<'_>, t: &TargetView) {
    let _ = t;
    if cx.now.0.saturating_sub(ai.tm_summon.0) < SUMMON_PERIOD_MS {
        return;
    }
    ai.tm_summon = cx.now;
    let (prob, num, id) = {
        let r = &mover_ex(m, cx).ai;
        (r.summ_prob, r.summ_num, r.summ_id)
    };
    if prob == 0 {
        return;
    }
    // the summons that died or vanished leave the list
    ai.summons
        .retain(|&s| target_view(cx, s).is_some_and(|v| !v.die));
    if num <= i32::try_from(ai.summons.len()).unwrap_or(i32::MAX) {
        return;
    }
    if i64::from(x_random(cx.rng, 100)) >= i64::from(prob) {
        return;
    }
    let Ok(mover) = u32::try_from(id) else { return };
    if cx.shared.data.mover(mover).is_none() {
        tracing::error!(
            mover = id,
            "SubSummonProcess: the summoned mover does not exist"
        );
        return;
    }
    // somewhere in a 3 x 3 m square around the summoner, x drawn before z
    let local = Vec3::new(
        1.5 - x_random_f(cx.rng, 3.0),
        0.0,
        1.5 - x_random_f(cx.rng, 3.0),
    );
    cx.cmds.push(AiCmd::Spawn(MonsterSpawn {
        mover,
        world: cx.world,
        layer: cx.layer,
        pos: m.core.pos + local,
        active_attack: true,
        gold: None,
        owner_ai: Some(m.id()),
    }));
    cx.emote(m, emote::SUMMON);
}
