//! Wand, melee-skill and magic-skill power, `ApplyDPC`, the magic-skill post-calculation and Asalraalaikum
//! (Plan/17 §5.6, §5.7; `MoverAttack.cpp:1181-1298, 1652-1807`; `AttackArbiter.cpp:587-624`; WP-COMBAT-03).

use super::defense::calc_defense;
use super::flags::{AtkFlags, MAX_CHARGE_LEVEL};
use super::generic::can_fly_by_attack;
use super::hit::is_critical_attack;
use super::rng::{wand_charge_multiplier, x_random, x_random_range};
use super::{AttackInfo, CombatConfig, CombatCx, CombatantView};
use crate::consts::NULL_ID;
use crate::consts::dst::{
    DST_ADDMAGIC, DST_CHR_DMG, DST_CRITICAL_BONUS, DST_DEX, DST_INT, DST_MASTRY_EARTH,
    DST_MASTRY_ELECTRICITY, DST_MASTRY_FIRE, DST_MASTRY_WATER, DST_MASTRY_WIND,
    DST_RESIST_MAGIC_RATE, DST_STA, DST_STR, RT_ATTACK, ST_EARTH, ST_EARTHWATER, ST_EARTHWIND,
    ST_ELECTRICITY, ST_ELECWIND, ST_FIRE, ST_FIREEARTH, ST_WATER, ST_WIND, WT_MAGIC_WAND,
};
use crate::stats;
use flyff_common::rng::GameRng;

/// `GetMagicHitPower(charge)` (`MoverAttack.cpp:1737-1753`): the wand autoattack ATK. `GetItemATKPower` of the hand
/// item plus `GetWeaponATK(WT_MAGIC_WAND)` on both ends, `x_random(min, max)`, `+ DST_CHR_DMG`, times the wand
/// charge multiplier (1.0 with `features.wand_autoattacks`). One draw.
pub fn magic_hit_power(
    att: &dyn CombatantView,
    charge: u32,
    cx: &CombatCx<'_>,
    rng: &mut dyn GameRng,
) -> i32 {
    let Some(prop) = att.hand_prop() else {
        return 0;
    };
    let (mut min, mut max) =
        stats::item_atk_power(att.stat(), att.rules(), prop, att.stat().weapon);
    let natk = stats::weapon_atk(att.stat(), att.rules(), &cx.cfg.rules, WT_MAGIC_WAND);
    min = min.wrapping_add(natk);
    max = max.wrapping_add(natk);
    let mut dmg = x_random_range(rng, min, max);
    dmg = dmg.wrapping_add(att.param(DST_CHR_DMG, 0));
    (dmg as f32 * wand_charge_multiplier(charge, cx.cfg.rules.wand_autoattacks)) as i32
}

/// `ApplyDPC(atk, info)` (this = the victim; `AF_MAGIC` and `AF_MELEESKILL`): the defense (unless the hit ignores
/// it), the critical roll (never for skills) and its multipliers.
///
/// `dmg = atk - CalcDefense` (0 when `info.param.can_ignore_def`: `dmg = atk`), at least 0. A critical sets
/// `AF_CRITICAL` and multiplies by 2.6 (attacker in `ATK4` or charge 4; knock-up 50 %) or 2.3 (30 %); the knock-up
/// roll only happens when the victim can be knocked up. Then the bonus `max(0.1, 1 + CRITICAL_BONUS / 100)` of the
/// **attacker** (`features.af_magic_fix`) or of the victim. No block, no element factor.
///
/// Draws: the defense roll, the critical roll, the knock-up roll.
pub fn apply_dpc(
    def: &dyn CombatantView,
    att: &dyn CombatantView,
    atk: i32,
    info: &mut AttackInfo<'_>,
    cx: &CombatCx<'_>,
    rng: &mut dyn GameRng,
) -> i32 {
    let mut dmg = if info.param.can_ignore_def(info.flags) {
        atk
    } else {
        atk.wrapping_sub(calc_defense(def, Some(att), info, cx, Some(&mut *rng)))
    };
    if dmg < 0 {
        dmg = 0;
    }
    if is_critical_attack(att, info, cx.cfg, rng) {
        info.flags |= AtkFlags::CRITICAL;
        let charge = info.charge_level() as i32;
        if att.in_atk4_state() || charge == MAX_CHARGE_LEVEL {
            dmg = (dmg as f32 * 2.6_f32) as i32;
            if can_fly_by_attack(def) && x_random(rng, 100) < 50 {
                info.flags |= AtkFlags::FLYING;
            }
        } else {
            dmg = (dmg as f32 * 2.3_f32) as i32;
            if can_fly_by_attack(def) && x_random(rng, 100) < 30 {
                info.flags |= AtkFlags::FLYING;
            }
        }
        let source = if cx.cfg.af_magic_fix { att } else { def };
        let mut bonus = 1.0_f32 + source.param(DST_CRITICAL_BONUS, 0) as f32 / 100.0_f32;
        if bonus < 0.1_f32 {
            bonus = 0.1_f32;
        }
        dmg = (dmg as f32 * bonus) as i32;
    }
    dmg
}

/// The stat a skill refers to (`dwReferStat1/2`): STR, DEX, INT, STA, else 0.
fn refer_stat(att: &dyn CombatantView, dst: u32) -> i32 {
    match dst {
        DST_STR => att.str_(),
        DST_DEX => att.dex(),
        DST_INT => att.int(),
        DST_STA => att.sta(),
        _ => 0,
    }
}

/// The refer-stat term of `GetMeleeSkillPower` and of `ApplyParam`'s heal (`RT_ATTACK` / `RT_HEAL`): `(int)(
/// (value / 10.0f) * stat + skill_level * (stat / 50.0f))` in `f32` when the refer target matches and the value
/// is set, else the raw stat.
pub fn refer_value(stat: i32, target: u32, wanted: u32, value: u32, skill_lvl: u32) -> i32 {
    if target == wanted && value != NULL_ID {
        ((value as f32 / 10.0_f32) * stat as f32 + skill_lvl as f32 * (stat as f32 / 50.0_f32))
            as i32
    } else {
        stat
    }
}

/// `GetMeleeSkillPower(info)` (`MoverAttack.cpp:1652-1732`): the ATK of a melee skill, also the base of a magic
/// skill. Needs the hand prop, the skill row and its level row (else 1, the original logs an error).
///
/// With `(wmin, wmax)` = `GetItemATKPower` of the hand weapon, `refer` the sum of the two refer stats
/// ([`refer_value`] with `RT_ATTACK`) and `ab_min / ab_max` the level row's ability range (PvP columns between
/// players): `pmin = (wmin + (ab_min + weapon.add_skill_min) * 5 + refer - 20) * (16 + lvl) / 13` in integers
/// (without a weapon item no `add_skill` term), `pmax` likewise; both go through `GetParam(DST_CHR_DMG, ·) +
/// plus_weapon_atk`; the roll is `pmin + x_random(max(1, pmax - pmin + 1))`. One draw.
pub fn melee_skill_power(
    att: &dyn CombatantView,
    def: &dyn CombatantView,
    info: &AttackInfo<'_>,
    rng: &mut dyn GameRng,
) -> i32 {
    let (Some(item), Some(skill), Some(add)) = (att.hand_prop(), info.skill.prop, info.skill.add)
    else {
        return 1;
    };
    let lvl = info.skill_level() as i32;
    let weapon = att.stat().weapon;
    let (wmin, wmax) = stats::item_atk_power(att.stat(), att.rules(), item, weapon);

    let s1 = refer_stat(att, skill.refer_stat1);
    let s2 = refer_stat(att, skill.refer_stat2);
    let s1 = refer_value(
        s1,
        skill.refer_target1,
        RT_ATTACK,
        skill.refer_value1,
        add.skill_lvl,
    );
    let s2 = refer_value(
        s2,
        skill.refer_target2,
        RT_ATTACK,
        skill.refer_value2,
        add.skill_lvl,
    );
    let refer = s1.wrapping_add(s2);

    let (ab_min, ab_max) = if att.is_player() && def.is_player() {
        (add.ability_min_pvp, add.ability_max_pvp)
    } else {
        (add.ability_min, add.ability_max)
    };
    let (ab_min, ab_max) = (ab_min as i32, ab_max as i32);
    let mul = 16_i32.wrapping_add(lvl);
    let (pmin, pmax) = match weapon {
        Some(w) => (
            wmin.wrapping_add(ab_min.wrapping_add(w.prop.add_skill_min).wrapping_mul(5))
                .wrapping_add(refer)
                .wrapping_sub(20)
                .wrapping_mul(mul)
                / 13,
            wmax.wrapping_add(ab_max.wrapping_add(w.prop.add_skill_max).wrapping_mul(5))
                .wrapping_add(refer)
                .wrapping_sub(20)
                .wrapping_mul(mul)
                / 13,
        ),
        None => (
            wmin.wrapping_add(ab_min.wrapping_mul(5))
                .wrapping_add(refer)
                .wrapping_sub(20)
                .wrapping_mul(mul)
                / 13,
            wmax.wrapping_add(ab_max.wrapping_mul(5))
                .wrapping_add(refer)
                .wrapping_sub(20)
                .wrapping_mul(mul)
                / 13,
        ),
    };
    let weapon_damage = stats::plus_weapon_atk(att.stat(), item.weapon_type);
    // `(float)` of the integer, `GetParam(CHR_DMG, (int)f)` and `(float)` again: f32 rounding above 2^24 is kept
    let f_min = (pmin as f32) as i32;
    let f_max = (pmax as f32) as i32;
    let power_min = att.param(DST_CHR_DMG, f_min).wrapping_add(weapon_damage) as f32;
    let power_max = att.param(DST_CHR_DMG, f_max).wrapping_add(weapon_damage) as f32;
    let mut span = (power_max - power_min) + 1.0_f32;
    if span <= 0.0_f32 {
        span = 1.0_f32;
    }
    let power = power_min + x_random(rng, span as u32) as f32;
    power as i32
}

/// `GetMagicSkillPower(info)` (`MoverAttack.cpp:1181-1223`): [`melee_skill_power`], `GetParam(DST_ADDMAGIC, ·)`,
/// then the element mastery of the skill's spell type: `(int)(atk + atk * MASTRY / 100.0f)`; with
/// `features.element_fix` the four combined types add both masteries.
pub fn magic_skill_power(
    att: &dyn CombatantView,
    def: &dyn CombatantView,
    info: &AttackInfo<'_>,
    cfg: &CombatConfig,
    rng: &mut dyn GameRng,
) -> i32 {
    let mut atk = melee_skill_power(att, def, info, rng);
    atk = att.param(DST_ADDMAGIC, atk);
    let Some(skill) = info.skill.prop else {
        return atk;
    };
    let mastery = |dst: u32| att.param(dst, 0) as f32 / 100.0_f32;
    let ratio = match skill.spell_type {
        ST_FIRE => mastery(DST_MASTRY_FIRE),
        ST_WATER => mastery(DST_MASTRY_WATER),
        ST_ELECTRICITY => mastery(DST_MASTRY_ELECTRICITY),
        ST_WIND => mastery(DST_MASTRY_WIND),
        ST_EARTH => mastery(DST_MASTRY_EARTH),
        ST_FIREEARTH if cfg.element_fix => mastery(DST_MASTRY_FIRE) + mastery(DST_MASTRY_EARTH),
        ST_EARTHWATER if cfg.element_fix => mastery(DST_MASTRY_WATER) + mastery(DST_MASTRY_EARTH),
        ST_ELECWIND if cfg.element_fix => {
            mastery(DST_MASTRY_WIND) + mastery(DST_MASTRY_ELECTRICITY)
        }
        ST_EARTHWIND if cfg.element_fix => mastery(DST_MASTRY_WIND) + mastery(DST_MASTRY_EARTH),
        _ => return atk,
    };
    (atk as f32 + atk as f32 * ratio) as i32
}

/// `GetMagicSkillFactor(defender, skillType)` (this = the attacker): 1.1 when the skill's element equals the
/// attacker's weapon element (the item's, else the hand prop's; `features.element_fix`: only for a real element),
/// 0.9 when the weapon's element counters the skill's (`(skill, weapon)` in fire/water, water/electricity,
/// electricity/earth, earth/wind, wind/fire), else 1.0; 1.0 without any hand prop.
pub fn magic_skill_factor(att: &dyn CombatantView, skill_type: u32, cfg: &CombatConfig) -> f32 {
    let w = att.weapon_element();
    let item_type = if w.element != 0 {
        w.element
    } else if let Some(p) = att.hand_prop() {
        p.item_type
    } else {
        return 1.0;
    };
    let same = if cfg.element_fix {
        skill_type == item_type && skill_type != 0
    } else {
        skill_type == item_type
    };
    if same {
        1.1_f32
    } else if matches!(
        (skill_type, item_type),
        (1, 2) | (2, 3) | (3, 5) | (5, 4) | (4, 1)
    ) {
        0.9_f32
    } else {
        1.0_f32
    }
}

/// `PostCalcMagicSkill(atk, info)` (this = the attacker, `def` the victim): `atk - atk * RESIST_MAGIC_RATE / 100`
/// (integers), `a = (atk - CalcDefense) * (1 - resist(skill element))` and `(int)((int)a * magic_skill_factor)`.
/// 0 without the skill row.
pub fn post_calc_magic_skill(
    att: &dyn CombatantView,
    def: &dyn CombatantView,
    atk: i32,
    info: &AttackInfo<'_>,
    cx: &CombatCx<'_>,
) -> i32 {
    let Some(skill) = info.skill.prop else {
        return 0;
    };
    let skill_type = skill.item_type;
    let ndef = calc_defense(def, Some(att), info, cx, None);
    let atk = atk.wrapping_sub(atk.wrapping_mul(def.param(DST_RESIST_MAGIC_RATE, 0)) / 100);
    let a = atk.wrapping_sub(ndef) as f32 * (1.0_f32 - stats::resist(def.stat(), skill_type));
    ((a as i32) as f32 * magic_skill_factor(att, skill_type, cx.cfg)) as i32
}

/// `PostAsalraalaikum()` (vanilla, without `features.no_asal_mp`): the extra damage of Asalraalaikum:
/// `((STR / 10) * level) * (5 + MP / 10) + bonus(level)` with the bonus table 20, 30, ..., 90, 100, 150 for levels
/// 1 to 10 (0 elsewhere). The MP drain that goes with it is the caller's.
pub fn post_asalraalaikum(str_: i32, skill_level: u32, mp: i32) -> i32 {
    let add = match skill_level {
        1 => 20,
        2 => 30,
        3 => 40,
        4 => 50,
        5 => 60,
        6 => 70,
        7 => 80,
        8 => 90,
        9 => 100,
        10 => 150,
        _ => 0,
    };
    ((str_ / 10) as u32)
        .wrapping_mul(skill_level)
        .wrapping_mul((5_i32.wrapping_add(mp / 10)) as u32)
        .wrapping_add(add) as i32
}
