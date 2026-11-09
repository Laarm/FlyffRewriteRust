//! Autoattack damage: hit power, the generic post-calculation and the knock-up rule (Plan/17 §5.5, §5.7;
//! `MoverAttack.cpp:145-158, 514-592, 1518-1650`; WP-COMBAT-02).

use super::defense::calc_defense;
use super::element::damage_property_factor;
use super::flags::{AtkFlags, rank_resists_knock_up};
use super::hit::{block_factor, is_critical_attack};
use super::rng::{bow_charge_multiplier, mul_div, x_random, x_random_range};
use super::{AttackInfo, CombatCx, CombatantView};
use crate::consts::dst::{
    DST_ABILITY_MAX, DST_ABILITY_MIN, DST_CHR_DMG, DST_CRITICAL_BONUS, WT_MELEE_YOYO,
};
use crate::stats;
use flyff_common::rng::GameRng;

/// `CanFlyByAttack()` of the victim: false when it is a flying NPC or flying, already in a knock-back state, or of
/// rank `SUPER`, `BOSS`, `MATERIAL` or `MIDBOSS`.
pub fn can_fly_by_attack(v: &dyn CombatantView) -> bool {
    !v.is_flying() && !v.in_dmg_fly() && !rank_resists_knock_up(v.rank())
}

/// `GetHitMinMax` for the striking hand of `att` (`info.npc_item` for an NPC).
///
/// Player: `min = 2 * ability_min`, `max = 2 * ability_max` of the hand item (none: 0 / 0), `GetParam(
/// DST_ABILITY_MIN / MAX, ·)`, `+ GetWeaponATK(type) + GetParam(DST_CHR_DMG)`, `(int)(· * item multiplier)`,
/// `+ (int)pow(opt, 1.5)` (see [`stats::attack_min_max`]). NPC: `dwAtkMin / dwAtkMax`, the two ability params,
/// `GetParam(DST_CHR_DMG)` on both (an override replaces, KEEP) and the attack item's ability range.
pub fn hit_min_max(
    att: &dyn CombatantView,
    info: &AttackInfo<'_>,
    cx: &CombatCx<'_>,
) -> (i32, i32) {
    if att.is_player() {
        return stats::attack_min_max(att.stat(), att.rules(), &cx.cfg.rules);
    }
    let p = att.mover_prop();
    let mut min = att.param(DST_ABILITY_MIN, p.atk_min as i32);
    let mut max = att.param(DST_ABILITY_MAX, p.atk_max as i32);
    min = att.param(DST_CHR_DMG, min);
    max = att.param(DST_CHR_DMG, max);
    if let Some(item) = info.npc_item {
        min = min.wrapping_add(item.ability_min as i32);
        max = max.wrapping_add(item.ability_max as i32);
    }
    (min, max)
}

/// `GetHitPower(info)` (`MoverAttack.cpp:1518-1586`): the ATK of an autoattack.
///
/// 1. the element factors (`info.def_factor` is set for [`post_calc_generic`]);
/// 2. [`hit_min_max`];
/// 3. the critical roll: on a critical `AF_CRITICAL`, the multipliers 1.1 / 1.4 (1.2 / 2.0 when the attacker's
///    level is higher and the victim an NPC, 1.4 / 1.8 for an NPC attacker), times `max(0.1, 1 +
///    CRITICAL_BONUS / 100)`, then the knock-up roll `x_random(100) < 15` (cancelled for a yo-yo, `AF_FORCE` and
///    player victims) that sets `AF_FLYING` when the victim can be knocked up;
/// 4. the damage roll `x_random(min, max)` and `MulDiv(atk, factor, 10000)`;
/// 5. `AF_RANGE`: the bow charge multiplier.
///
/// Draw order: critical, knock-up (only on a critical), damage.
pub fn hit_power(
    att: &dyn CombatantView,
    def: &dyn CombatantView,
    info: &mut AttackInfo<'_>,
    cx: &CombatCx<'_>,
    rng: &mut dyn GameRng,
) -> i32 {
    let (atk_factor, def_factor) = damage_property_factor(att, def, cx);
    info.def_factor = def_factor;
    let (mut min, mut max) = hit_min_max(att, info, cx);

    if is_critical_attack(att, info, cx.cfg, rng) {
        info.flags |= AtkFlags::CRITICAL;
        let (mut f_min, mut f_max) = (1.1_f32, 1.4_f32);
        if att.level() > def.level() {
            if !def.is_player() {
                f_min = 1.2_f32;
                f_max = 2.0_f32;
            }
            if !att.is_player() {
                f_min = 1.4_f32;
                f_max = 1.8_f32;
            }
        }
        let mut bonus = 1.0_f32 + att.param(DST_CRITICAL_BONUS, 0) as f32 / 100.0_f32;
        if bonus < 0.1_f32 {
            bonus = 0.1_f32;
        }
        min = (min as f32 * f_min * bonus) as i32;
        max = (max as f32 * f_max * bonus) as i32;

        let mut fly = x_random(rng, 100) < 15;
        let yoyo = att
            .hand_prop()
            .is_some_and(|p| p.weapon_type == WT_MELEE_YOYO);
        if yoyo || info.flags.intersects(AtkFlags::FORCE) {
            fly = false;
        }
        if def.is_player() {
            fly = false;
        }
        if can_fly_by_attack(def) && fly {
            info.flags |= AtkFlags::FLYING;
        }
    }

    let mut atk = x_random_range(rng, min, max);
    atk = mul_div(atk, atk_factor, 10_000);
    if info.flags.intersects(AtkFlags::RANGE) {
        atk = (atk as f32 * bow_charge_multiplier(info.charge_level())) as i32;
    }
    atk
}

/// `PostCalcGeneric(atk, info)` (`MoverAttack.cpp:1588-1650`): the damage of an autoattack after defense and
/// block (`this` = the attacker).
///
/// `dmg = atk - MulDiv(CalcDefense, info.def_factor, 10000)`; a positive damage rolls the block
/// ([`block_factor`]): with `features.dmgcorrect` a player-versus-player block needs `x_random(100) <
/// max_block_value` (and then applies the factor), a monster-versus-player block is x0.7, player-versus-monster
/// x0.4, otherwise the factor; without the switch every block applies the factor. A monster hitting a player
/// does at least 10 % of its ATK. A zero damage clears `AF_CRITICAL` and `AF_FLYING`.
///
/// Draw order: the defense roll (players with armour), the block roll, the PvP block roll.
pub fn post_calc_generic(
    att: &dyn CombatantView,
    def: &dyn CombatantView,
    atk: i32,
    info: &mut AttackInfo<'_>,
    cx: &CombatCx<'_>,
    rng: &mut dyn GameRng,
) -> i32 {
    let def_raw = calc_defense(def, Some(att), info, cx, Some(&mut *rng));
    let ndef = mul_div(def_raw, info.def_factor, 10_000);
    let mut dmg = atk.wrapping_sub(ndef);
    if dmg > 0 {
        let fb = block_factor(def, att, info, cx, rng);
        if fb < 1.0_f32 {
            let both_players = att.is_player() && def.is_player();
            if cx.cfg.dmgcorrect {
                if both_players && (x_random(rng, 100) as i32) < cx.cfg.rules.max_block_value {
                    info.flags |= AtkFlags::BLOCKING;
                    dmg = (dmg as f32 * fb) as i32;
                } else if !both_players {
                    info.flags |= AtkFlags::BLOCKING;
                    if def.is_player() && !att.is_player() {
                        dmg = (dmg as f32 * 0.7_f32) as i32;
                    } else if !def.is_player() && att.is_player() {
                        dmg = (dmg as f32 * 0.4_f32) as i32;
                    } else {
                        dmg = (dmg as f32 * fb) as i32;
                    }
                }
            } else {
                info.flags |= AtkFlags::BLOCKING;
                dmg = (dmg as f32 * fb) as i32;
            }
        }
    } else {
        dmg = 0;
    }
    if !att.is_player() && def.is_player() {
        let min = (atk as f32 * 0.1_f32).max(0.0_f32) as i32;
        dmg = dmg.max(min);
    }
    if dmg == 0 {
        info.flags &= !(AtkFlags::CRITICAL | AtkFlags::FLYING);
    }
    dmg
}
