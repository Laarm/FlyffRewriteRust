//! The attack side of RAGE (Plan/18 §9.7, §9.11; `AIMonster.cpp:1162-1435`; WP-AI-10): the choice between the
//! melee and the ranged attack ([`select_attack_type`]), the attack method roll with the deathblow
//! ([`atk_method_near`], [`atk_method_far`]), the Recovery AI, the execution through the combat pipeline of doc 17
//! ([`do_attack`]) and the re-attack delay ([`sub_attack_chance`]).
//!
//! ```text
//!   RAGE Process ── now > tm_attack_delay ──► sub_attack_chance
//!        Recovery AI (HP% <= recv_cond_me, MP > 0)  ──► heal, skip the attack, retry in 3 s
//!        method = near/far roll:  r = x_random(100) FIRST, then the deathblow 1/(HP% / 4.5)
//!        item   = atk1 / atk2 / atk3           (the item's attack range decides the branch)
//!          AR_RANGE / AR_WAND:  within (range_attack[job] & 0x7F) + 1 m -> do_attack, retry in 3 s
//!          otherwise:           IsRangeObj(GetAttackRange(item)) -> do_attack, go_target,
//!                               retry in re_attack_delay - 1000 + x_random(2000) ms
//! ```
//!
//! **Execution** (what `do_attack` calls): a ranged item goes to [`crate::combat::action::npc_range`] (the server-simulated
//! projectile), method 3 to [`crate::combat::action::npc_special`] (the deathblow, `SP_ATK1`), everything else is
//! `SendActMsg(OBJMSG_ATK1, target)` on the sim, **also for method 2** and without storing the attack item in the
//! act parameters: the hit frame uses the virtual item `dwAtk1` (KQ-13 Q6). The hit roll and the
//! `MeleeAttackNtf` follow at the barrier of the swing's first frame (doc 17, `on_npc_attack_started`); damage
//! at the hit frame of the motion.

use super::helpers::{
    TargetView, hp_percent, inc_hp, inc_mp, is_in_range, max_hp, max_mp, mover_ex,
};
use super::{CHASE_TIMER_MS, MonsterAi};
use crate::ai::ctx::AiCx;
use crate::ai::rng::x_random;
use crate::combat::action::{npc_range, npc_special};
use crate::mover::Mover;
use flyff_common::defs::attribute::{AR_RANGE, AR_WAND, BELLI_MELEE, BELLI_MELEE2X, BELLI_RANGE};
use flyff_common::defs::obj::XI_GEN_CURE01;
use flyff_common::math::Vec3;
use flyff_common::{GameMs, ObjId};
use flyff_data::mover_ex::MoverEx;
use flyff_data::props::mover::MoverProp;
use flyff_rules::combat::{attack_range, mul_div};
use flyff_rules::consts::dst::DST_HAWKEYE_RATE;
use flyff_sim::ActMsg;
use flyff_sim::bounds::{is_range_obj, radius};
use flyff_sim::types::OBJMSG_ATK1;

/// The retry delay after a ranged attack and after the Recovery AI (`SEC(3)`).
pub const RANGED_DELAY_MS: u64 = 3_000;

/// The deathblow divisor (`GetHitPointPercent() / 4.5f`): the method roll turns into method 3 with the chance
/// `1 / this`. 22 at full HP, rising as the HP falls; 0 (never) below 4.5 % HP.
pub fn deathblow_divisor(hp_percent: i32) -> u32 {
    // `(int)(percent / 4.5f)`; a negative percent cannot happen for a live monster
    (hp_percent as f32 / 4.5_f32).max(0.0) as u32
}

/// `berserk_factor` of 18 §9.11 over the numbers: the damage multiplier of the `Berserk` AI block, applied by the
/// damage pipeline to an NPC attacker whose HP percent is at or below the threshold (`MoverAttack.cpp:1063`).
/// `1.0` above it, without a `Berserk` block, or with threshold 0.
pub fn berserk_factor(ex: &MoverEx, hp_percent: i32) -> f32 {
    let ai = &ex.ai;
    if ai.berserk_hp > 0 && hp_percent <= ai.berserk_hp {
        ai.berserk_dmg_mul
    } else {
        1.0
    }
}

/// [`berserk_factor`] of the monster `prop` with `hp` of `max_hp` hit points (`MulDiv` percent): the number the
/// damage pipeline multiplies the damage of an NPC attacker with.
pub fn berserk_factor_of(prop: &MoverProp, hp: i32, max_hp: i32) -> f32 {
    let pct = if max_hp == 0 {
        0
    } else {
        mul_div(hp, 100, max_hp)
    };
    berserk_factor(&prop.ex, pct)
}

fn dist_sq(a: Vec3, b: Vec3) -> f32 {
    let d = a - b;
    d.x * d.x + d.y * d.y + d.z * d.z
}

/// `SelectAttackType(target)` (18 §9.7): `true` = a ranged attack. KEEP-QUIRK: the melee/ranged switch distance
/// uses `range_attack[0]` (job 0), not the target's job (Q12). V15 data has no ranged AI at all, so this is
/// always `false` there.
pub fn select_attack_type(ai: &MonsterAi, m: &Mover, cx: &AiCx<'_>, t: &TargetView) -> bool {
    let _ = ai;
    let ex = mover_ex(m, cx);
    let r0 = f32::from(ex.ai.range_attack.first().copied().unwrap_or(0) & 0x7F);
    let job = if t.is_player {
        usize::try_from(t.job.max(0)).unwrap_or(0)
    } else {
        0
    };
    let b_range = ex.ai.range_attack.get(job).copied().unwrap_or(0) != 0;
    let b_melee = ex.ai.melee_attack != 0;
    match (b_range, b_melee) {
        // both: melee inside half the range (integer half), ranged outside
        (true, true) => {
            let half = (r0 as i32 / 2) as f32;
            dist_sq(t.pos, m.sim.pos) >= half * half
        }
        (true, false) => true,
        _ => false,
    }
}

/// The attack method roll (`GetAtkMethod_Near` / `GetAtkMethod_Far`): `r = x_random(100)` is drawn **first**,
/// then the deathblow `x_random(n) == 0` with `n = HP% / 4.5` (only when `n > 0`; method 3), then the belligerence
/// table: near MELEE2X `r < 60` -> 1 else 2, MELEE `r < 80`, RANGE `r < 50`; far 50 / 30 / 20; every other
/// belligerence uses method 1.
fn atk_method(m: &Mover, cx: &mut AiCx<'_>, far: bool) -> u8 {
    let r = x_random(cx.rng, 100);
    let n = deathblow_divisor(hp_percent(m, cx));
    if n > 0 && x_random(cx.rng, n) == 0 {
        return 3;
    }
    let pick = |limit: u32| if r < limit { 1 } else { 2 };
    let table = if far { [50, 30, 20] } else { [60, 80, 50] };
    match m.monster().map_or(0, |e| e.belligerence) {
        BELLI_MELEE2X => pick(table[0]),
        BELLI_MELEE => pick(table[1]),
        BELLI_RANGE => pick(table[2]),
        _ => 1,
    }
}

/// `GetAtkMethod_Near()`: the method when the monster does not keep its distance (the melee branch).
pub fn atk_method_near(m: &Mover, cx: &mut AiCx<'_>) -> u8 {
    atk_method(m, cx, false)
}

/// `GetAtkMethod_Far()`: the method of a monster with a ranged AI.
pub fn atk_method_far(m: &Mover, cx: &mut AiCx<'_>) -> u8 {
    atk_method(m, cx, true)
}

/// `GetAtkItemId(method)`: `dwAtk1`, `dwAtk2`, `dwAtk3` for the methods 1 to 3, 0 for anything else.
pub fn atk_item_id(prop: &MoverProp, method: u8) -> u32 {
    match method {
        1 => prop.atk1,
        2 => prop.atk2,
        3 => prop.atk3,
        _ => 0,
    }
}

/// `DoAttack(method, target)` (18 §9.7; `AIMonster.cpp:1259-1305`): starts the attack of `method` on `target`
/// through the combat layer (see the module docs) and restarts the 15 s chase timer. A missing attack item is
/// logged (once per monster) and the attack skipped.
pub fn do_attack(ai: &mut MonsterAi, m: &mut Mover, cx: &mut AiCx<'_>, method: u8, target: ObjId) {
    if method == 0 {
        return;
    }
    let shared = cx.shared;
    let Some(prop) = shared.data.mover(m.prop_id()) else {
        return;
    };
    let item_id = atk_item_id(prop, method);
    let Some(item) = shared.data.item(item_id) else {
        if !ai.bad_atk_logged {
            ai.bad_atk_logged = true;
            tracing::error!(
                mover = prop.id,
                method,
                item = item_id,
                "CAIMonster::DoAttack: the attack item does not exist"
            );
        }
        return;
    };
    if item.attack_range == AR_RANGE || item.attack_range == AR_WAND {
        // a projectile from the hit frame (17: `DoAttackRange`)
        let _ = npc_range(m, cx, target, item_id);
    } else if method == 3 {
        // the deathblow: `SP_ATK1`, area or single hit at the hit frame
        let _ = npc_special(m, cx, target, item_id);
    } else {
        // always the first attack motion, also for method 2 (Q6); the act parameters stay as they are
        let _ = cx.send_act(m, ActMsg::with(OBJMSG_ATK1, [target.0 as i32, 0, 0, 0, 0]));
    }
    ai.tm_attack = GameMs(cx.now.0 + CHASE_TIMER_MS);
}

/// The Recovery AI (`m_bRecvCond`, unused in V15 data): at or below `recv_cond_me` % HP and with MP left the
/// monster heals `recv_cond_how` % of its maximum, pays `recv_cond_mp` MP (0 = all) and does not attack this
/// time. Returns `false` when it healed. The retry timer is set whenever the Recovery AI exists.
fn recovery(ai: &mut MonsterAi, m: &mut Mover, cx: &mut AiCx<'_>) -> bool {
    let (me, how, mp_cost) = {
        let r = &mover_ex(m, cx).ai;
        (r.recv_cond_me, r.recv_cond_how, r.recv_cond_mp)
    };
    let mut attack = true;
    if hp_percent(m, cx) <= me && m.mv.mp > 0 {
        let n = (max_hp(m, cx) as f32 * (how as f32 / 100.0_f32)) as i32;
        inc_hp(m, cx, n);
        let pos = m.sim.pos;
        cx.sfx(m, XI_GEN_CURE01, pos);
        let cost = if mp_cost > 0 { mp_cost } else { max_mp(m, cx) };
        inc_mp(m, cx, cost.wrapping_neg());
        attack = false;
    }
    ai.tm_attack_delay = GameMs(cx.now.0 + RANGED_DELAY_MS);
    attack
}

/// `SubAttackChance(target)` (18 §9.7), called by RAGE every tick once `now > tm_attack_delay`: the Recovery AI,
/// the method roll, the range test of the chosen item and the attack with its next delay.
///
/// * A ranged item (`AR_RANGE` / `AR_WAND`) fires within `(range_attack[job] & 0x7F) + 1` m (xz, `IsInRange`)
///   and waits 3 s; with the V15 data that range is 1 m, so a ranged item is effectively point-blank only and
///   the next tick re-rolls (KEEP, Q12).
/// * A melee item needs `IsRangeObj(target, GetAttackRange(item.range))` (`0.8 Ra + 0.8 Rb + r`, 3-D) and waits
///   `re_attack_delay - 1000 + x_random(2000)` ms; the delay may fall before `now`, then the next tick attacks.
pub fn sub_attack_chance(ai: &mut MonsterAi, m: &mut Mover, cx: &mut AiCx<'_>, t: &TargetView) {
    let job = if t.is_player {
        usize::try_from(t.job.max(0)).unwrap_or(0)
    } else {
        0
    };
    let (has_recovery, range_byte) = {
        let r = &mover_ex(m, cx).ai;
        (
            r.recv_cond != 0,
            r.range_attack.get(job).copied().unwrap_or(0),
        )
    };
    if has_recovery && !recovery(ai, m, cx) {
        return;
    }
    ai.atk_method = if ai.range_attack {
        atk_method_far(m, cx)
    } else {
        atk_method_near(m, cx)
    };
    let method = ai.atk_method;
    let shared = cx.shared;
    let Some(prop) = shared.data.mover(m.prop_id()) else {
        return;
    };
    let Some(item) = shared.data.item(atk_item_id(prop, method)) else {
        if !ai.bad_atk_logged {
            ai.bad_atk_logged = true;
            tracing::error!(
                mover = prop.id,
                method,
                "CAIMonster::SubAttackChance: the attack item does not exist"
            );
        }
        return;
    };
    let code = item.attack_range;
    if code == AR_RANGE || code == AR_WAND {
        // within the AI range (+1 m) the ranged attack fires
        let reach = f32::from(range_byte & 0x7F) + 1.0;
        if is_in_range(t.pos - m.sim.pos, reach) {
            do_attack(ai, m, cx, method, t.id);
            ai.tm_attack_delay = GameMs(cx.now.0 + RANGED_DELAY_MS);
        }
    } else {
        let hawkeye = m.mv.dst.table.param(DST_HAWKEYE_RATE, 0);
        let reach = attack_range(code, hawkeye);
        let in_reach = cx.lookup.snapshot(t.id).is_some_and(|s| {
            let mine = radius(&m.sim.params.bounds, m.sim.params.scale);
            is_range_obj(m.sim.pos, mine, s.pos, s.radius, reach)
        });
        if in_reach {
            do_attack(ai, m, cx, method, t.id);
            ai.go_target = true;
            // `dwReAttackDelay - SEC(1) + xRandom(SEC(2))` as int arithmetic: may be negative
            let delay =
                i64::from(prop.re_attack_delay) - 1_000 + i64::from(x_random(cx.rng, 2_000));
            ai.tm_attack_delay = GameMs(cx.now.0.saturating_add_signed(delay));
        }
    }
}
